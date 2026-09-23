package storage_test

import (
	"testing"
	"time"

	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// The arity tests prove AppendTo and the INSERT agree about COUNT. Only
// ClickHouse can prove they agree about TYPES, and 0015_k8s_fidelity.sql adds
// sixty columns to k8s_resources alone — parallel arrays, Array(Nullable(...)),
// Array(Array(String)), Array(Map(String,String)) and a Nullable(Int64) that
// used to be a date. Every one of those is a shape the driver can reject at
// insert time with nothing in-process noticing first.
//
// So this walks the whole path once per table: scratch database, every
// migration applied from empty, a real batch through the registered INSERT,
// and the values read back.
func TestK8sFidelityRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	// A real timestamp, never time.Time{}: year 1 is outside DateTime64's
	// range, the value clamps, and the table's TTL drops the part on the spot
	// — a zero-valued row inserts successfully and then counts as zero rows.
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	created := now.Add(-72 * time.Hour)
	headerTS := int64(1700000000)
	grace := int64(30)

	// A pod as the intake builds it: two container statuses (one of them
	// crashlooping), a condition with a message, per-container requirements
	// and the whole object as JSON.
	pod := storage.K8sResourceRow{
		TenantID: "default", CollectedAt: now,
		ClusterID: "cid", ClusterName: "prod",
		Kind: "Pod", Namespace: "shop", Name: "checkout-1", UID: "uid-1",
		ResourceVersion: "9912",
		OwnerKind:       "ReplicaSet", OwnerName: "checkout",
		NodeName: "node-1", Phase: "Running", Status: "CrashLoopBackOff",
		Ready: 1, Desired: 2,
		Counts:      map[string]int64{"restarts": 7},
		Labels:      map[string]string{"app": "checkout"},
		Annotations: map[string]string{"kubectl.kubernetes.io/last-applied-configuration": "{}"},
		Tags:        map[string][]string{"kube_service": {"a", "b"}},

		OrgID: 77, SubscriptionID: 3, HeaderTimestamp: &headerTS, Encoding: "protobuf",

		CreationTimestamp:          &created,
		DeletionTimestamp:          &now,
		DeletionGracePeriodSeconds: &grace,
		Finalizers:                 []string{"kubernetes.io/pvc-protection"},

		OwnerKinds: []string{"ReplicaSet"}, OwnerNames: []string{"checkout"}, OwnerUIDs: []string{"uid-rs"},

		ConditionTypes: []string{"Ready"}, ConditionStatuses: []string{"False"},
		ConditionReasons:  []string{"ContainersNotReady"},
		ConditionMessages: []string{"containers with unready status: [sidecar]"},
		// Array(Nullable(DateTime)): one set, one absent, in one array — the
		// shape that makes "never probed" survive without a fake date.
		ConditionLastTransition: []*time.Time{&created},
		ConditionLastUpdate:     []*time.Time{nil},
		ConditionLastProbe:      []*time.Time{&created},
		ConditionMessage:        "ReplicaSet is progressing",

		ResourceRequirements: `[{"name":"app","requests":{"cpu":100}}]`,
		Metrics:              map[string]float64{"cpu.usage": 0.42},
		Object:               `{"metadata":{"name":"checkout-1"}}`,
		Envelope:             `{"clusterName":"prod","isTerminated":false}`,

		PodIP: "10.4.2.9", QOSClass: "Burstable", StartTime: &created, HostName: "cluster-agent-1",

		ContainerNames: []string{"app", "sidecar"},
		ContainerIDs:   []string{"containerd://a", "containerd://b"},
		ContainerReady: []uint8{1, 0}, ContainerRestarts: []int32{0, 7},
		ContainerStates:   []string{"running", "waiting"},
		ContainerMessages: []string{"", "back-off 5m0s restarting failed container"},
		ContainerImages:   []string{"shop/app:1.2", "envoy:1.29"},
		ContainerImageIDs: []string{"sha256:a", "sha256:b"},
		ContainerIsInit:   []uint8{0, 0},
	}

	// A node, for the Map(String, Int64) capacity columns and the JSON ones.
	node := storage.K8sResourceRow{
		TenantID: "default", CollectedAt: now,
		ClusterID: "cid", Kind: "Node", Name: "node-1", UID: "uid-node",
		Status: "Ready,SchedulingDisabled", Unschedulable: 1,
		PodCIDRs:       []string{"10.244.1.0/24"},
		Taints:         `[{"key":"node.kubernetes.io/unreachable","effect":"NoExecute"}]`,
		Capacity:       map[string]int64{"cpu": 8000, "memory": 33554432000},
		Allocatable:    map[string]int64{"cpu": 7800},
		NodeRoles:      []string{"worker"},
		NodeAddresses:  `{"InternalIP":"10.0.1.7"}`,
		KubeletVersion: "v1.29.4",
	}

	storagetest.Insert(t, conn, storage.K8sResourcesWriter, []storage.Row{pod, node})

	if got := storagetest.Count(t, conn, "k8s_resources"); got != 2 {
		t.Fatalf("k8s_resources: %d rows, want 2", got)
	}

	// The columns that carry the new shapes, read back as ClickHouse returns
	// them. A tag that repeats its key must still be two values.
	row := storagetest.QueryRow(t, conn, `
		SELECT container_messages[2], condition_reasons[1], tags['kube_service'],
		       header_timestamp, deletion_grace_period_seconds, metrics['cpu.usage']
		FROM k8s_resources WHERE kind = 'Pod'`)
	if got, want := row[0].(string), "back-off 5m0s restarting failed container"; got != want {
		t.Errorf("container_messages[2] = %q, want %q", got, want)
	}
	if got := row[1].(string); got != "ContainersNotReady" {
		t.Errorf("condition_reasons[1] = %q", got)
	}
	if got, ok := row[2].([]string); !ok || len(got) != 2 {
		t.Errorf("tags['kube_service'] = %v, want two values — a duplicate-key tag must survive", row[2])
	}
	if got, ok := row[3].(*int64); !ok || got == nil || *got != headerTS {
		t.Errorf("header_timestamp = %v, want the raw wire integer %d", row[3], headerTS)
	}
	if got, ok := row[4].(*int64); !ok || got == nil || *got != 30 {
		t.Errorf("deletion_grace_period_seconds = %v", row[4])
	}
	if got := row[5].(float64); got != 0.42 {
		t.Errorf("metrics['cpu.usage'] = %v", got)
	}

	// The condition time arrays keep their holes: index 1 of last_update is
	// NULL while last_transition at the same index is a date.
	row = storagetest.QueryRow(t, conn, `
		SELECT isNull(condition_last_update[1]), isNull(condition_last_transition[1])
		FROM k8s_resources WHERE kind = 'Pod'`)
	if row[0].(uint8) != 1 || row[1].(uint8) != 0 {
		t.Errorf("condition times = update NULL:%v transition NULL:%v, want 1 and 0", row[0], row[1])
	}

	// And the Map(String, Int64) columns the node fills.
	row = storagetest.QueryRow(t, conn, `
		SELECT capacity['cpu'], unschedulable FROM k8s_resources WHERE kind = 'Node'`)
	if row[0].(int64) != 8000 || row[1].(uint8) != 1 {
		t.Errorf("node capacity/unschedulable = %v / %v", row[0], row[1])
	}

	// THE MATERIALIZED VIEW. 0015 drops and recreates k8s_resources_current_mv
	// against the widened column list; if the two tables or the view's SELECT
	// ever disagree, the view stops firing and the "right now" table silently
	// freezes at whatever it last held. FINAL because the target is a
	// ReplacingMergeTree and the parts are not merged yet.
	row = storagetest.QueryRow(t, conn, `
		SELECT count(), anyIf(pod_ip, kind = 'Pod'), anyIf(kubelet_version, kind = 'Node')
		FROM k8s_resources_current FINAL`)
	if got := row[0].(uint64); got != 2 {
		t.Fatalf("k8s_resources_current has %d rows, want 2 — the materialized view stopped firing", got)
	}
	if got := row[1].(string); got != "10.4.2.9" {
		t.Errorf("current pod_ip = %q — a column added to the table but not the view arrives empty", got)
	}
	if got := row[2].(string); got != "v1.29.4" {
		t.Errorf("current kubelet_version = %q", got)
	}
}

// container_events and container_images: the structured transition columns
// (Nullable everywhere absence matters) and the parallel layer arrays,
// including Array(Array(String)) which rejects a nil inner slice.
func TestContainerFidelityRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	reason := "OOMKilled"
	exit := int32(137)
	signal := int32(9)
	built := now.Add(-24 * time.Hour)
	source := "agent"

	storagetest.Insert(t, conn, storage.ContainerEventsWriter, []storage.Row{
		storage.ContainerEventRow{
			TenantID: "default", Timestamp: now, Host: "node-1", ClusterID: "cid",
			ObjectKind: "Container", EventType: "Delete", ContainerID: "containerd://abc",
			ExitCode: &exit, PayloadVersion: "v1", EventVariant: "container",
			ContainerKind: "CONTAINER_KIND_REGULAR", Precision: "PRECISION_EXACT",
			MissedIntermediate: "MISSED_INTERMEDIATE_PROVEN",
			OldStateKind:       "CONTAINER_STATE_KIND_RUNNING",
			NewStateKind:       "CONTAINER_STATE_KIND_TERMINATED",
			NewReason:          &reason, NewExitCode: &exit, NewSignal: &signal,
			ContainerNamePresent: 1,
		},
	})

	row := storagetest.QueryRow(t, conn, `
		SELECT new_reason, new_signal, isNull(old_reason), precision, container_name_present
		FROM container_events`)
	if got, ok := row[0].(*string); !ok || got == nil || *got != "OOMKilled" {
		t.Errorf("new_reason = %v", row[0])
	}
	if got, ok := row[1].(*int32); !ok || got == nil || *got != 9 {
		t.Errorf("new_signal = %v", row[1])
	}
	// The state that carried no reason must read back as NULL, not "".
	if row[2].(uint8) != 1 {
		t.Errorf("old_reason is not NULL — absent became an empty string")
	}
	if row[3].(string) != "PRECISION_EXACT" || row[4].(uint8) != 1 {
		t.Errorf("precision/name_present = %v / %v", row[3], row[4])
	}

	storagetest.Insert(t, conn, storage.ContainerImagesWriter, []storage.Row{
		storage.ContainerImageRow{
			TenantID: "default", CollectedAt: now, Host: "node-1",
			ImageKey: "sha256:dig", IdentitySource: "digest",
			ImageID: "sha256:img", Digest: "sha256:dig", Name: "shop/app",
			SizeBytes: 300, LayerCount: 2, LayerBytes: 300, BuiltAt: &built,
			DDTags:         map[string][]string{"env": {"prod"}},
			PayloadVersion: "v1", Source: &source,

			LayerMediaTypes: []string{"application/vnd.oci.image.layer.v1.tar+gzip", ""},
			LayerDigests:    []string{"sha256:l1", "sha256:l2"},
			LayerSizes:      []int64{100, 200},
			// The second layer has no URLs, and an empty inner slice is the
			// only thing the driver accepts there — nil takes the batch down.
			LayerURLs:              [][]string{{"https://example.invalid/blob"}, {}},
			LayerHistoryCreated:    []*time.Time{&built, nil},
			LayerHistoryCreatedBy:  []string{"RUN apk add curl", "COPY . /app"},
			LayerHistoryAuthor:     []string{"ci", ""},
			LayerHistoryComment:    []string{"base", ""},
			LayerHistoryEmptyLayer: []uint8{0, 1},
			SizeNegative:           0,
		},
	})

	row = storagetest.QueryRow(t, conn, `
		SELECT layer_history_created_by[2], layer_urls[1][1], isNull(layer_history_created[2]),
		       source, layer_sizes[2]
		FROM container_images`)
	if row[0].(string) != "COPY . /app" {
		t.Errorf("layer_history_created_by[2] = %q — the Dockerfile is the point", row[0])
	}
	if row[1].(string) != "https://example.invalid/blob" {
		t.Errorf("layer_urls[1][1] = %q", row[1])
	}
	if row[2].(uint8) != 1 {
		t.Errorf("layer_history_created[2] is not NULL — a missing date became one")
	}
	if got, ok := row[3].(*string); !ok || got == nil || *got != "agent" {
		t.Errorf("source = %v", row[3])
	}
	if row[4].(int64) != 200 {
		t.Errorf("layer_sizes[2] = %v", row[4])
	}
}

// The three remaining tables of this migration: the cluster summary with its
// Array(Map(String,String)) node breakdown, a manifest with three tag
// sources, an action with its attachments, and the new ecs_tasks table.
func TestOrchestratorSideTablesRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	created := now.Add(-time.Hour)
	headerTS := int64(1700000000)

	storagetest.Insert(t, conn, storage.K8sClusterWriter, []storage.Row{
		storage.K8sClusterRow{
			TenantID: "default", CollectedAt: now, ClusterID: "cid", ClusterName: "prod",
			NodeCount: 2, KubeletVersions: map[string]uint32{"v1.29.4": 1, "v1.28.9": 1},
			OrgID: 77, HeaderTimestamp: &headerTS, Encoding: "protobuf",
			GroupID: 1, GroupSize: 2, AgentVersion: "7.55.1",
			Tags:            map[string][]string{"env": {"prod"}},
			ClusterTags:     map[string][]string{"kube_cluster_name": {"prod"}},
			ResourceVersion: "9912", CreationTimestamp: &created,
			Metrics:                      map[string]float64{"nodes.ready": 2},
			ExtendedResourcesCapacity:    map[string]int64{"nvidia.com/gpu": 4},
			NodesName:                    []string{"node-1", "node-2"},
			NodesRegion:                  []string{"eu-west-1", "eu-west-1"},
			NodesInstanceType:            []string{"m6i.large", "m6i.xlarge"},
			NodesOS:                      []string{"linux", "linux"},
			NodesOSImage:                 []string{"Amazon Linux 2", "Amazon Linux 2"},
			NodesArchitecture:            []string{"amd64", "amd64"},
			NodesKernelVersion:           []string{"6.1.0", "6.1.0"},
			NodesContainerRuntimeVersion: []string{"containerd://1.7.13", "containerd://1.7.13"},
			NodesKubeletVersion:          []string{"v1.29.4", "v1.28.9"},
			// An Array(Map(...)) rejects a nil element; the second node
			// reported no quantities, which is normal.
			NodesAllocatable: []map[string]string{{"cpu": "2"}, {}},
			NodesCapacity:    []map[string]string{{"cpu": "2"}, {}},
		},
	})
	row := storagetest.QueryRow(t, conn, `
		SELECT nodes_kubelet_version[2], nodes_capacity[1]['cpu'],
		       extended_resources_capacity['nvidia.com/gpu'], cluster_tags['kube_cluster_name']
		FROM k8s_cluster`)
	if row[0].(string) != "v1.28.9" {
		t.Errorf("nodes_kubelet_version[2] = %q — the per-node breakdown is the point", row[0])
	}
	if row[1].(string) != "2" {
		t.Errorf("nodes_capacity[1]['cpu'] = %q", row[1])
	}
	if row[2].(int64) != 4 {
		t.Errorf("extended_resources_capacity = %v — GPUs had no home anywhere", row[2])
	}
	if got, ok := row[3].([]string); !ok || len(got) != 1 || got[0] != "prod" {
		t.Errorf("cluster_tags = %v", row[3])
	}

	storagetest.Insert(t, conn, storage.K8sManifestsWriter, []storage.Row{
		storage.K8sManifestRow{
			TenantID: "default", CollectedAt: now, ClusterID: "cid",
			UID: "u1", Kind: "DatadogAgent", APIVersion: "datadoghq.com/v2alpha1",
			Content: "apiVersion: datadoghq.com/v2alpha1", ContentType: "json",
			OrgID: 77, Encoding: "protobuf", GroupID: 4, GroupSize: 8,
			HostName: "cluster-agent-1", AgentVersion: "7.55.1", OriginCollector: "datadogAgent",
			Tags: map[string][]string{"env": {"prod"}, "team": {"infra"}},
			TagSources: map[string][]string{
				"envelope": {"env:prod"}, "wrapper": {"source:crd"}, "manifest": {"team:infra"},
			},
			NodeName: "node-3", Type: 81, Version: "v1",
			ExtraAttributes: map[string]string{"crd_group": "datadoghq.com"},
			ContentIsUTF8:   1,
		},
	})
	row = storagetest.QueryRow(t, conn, `
		SELECT tag_sources['wrapper'], extra_attributes['crd_group'], content_is_utf8, origin_collector
		FROM k8s_manifests`)
	if got, ok := row[0].([]string); !ok || len(got) != 1 || got[0] != "source:crd" {
		t.Errorf("tag_sources['wrapper'] = %v — provenance must survive the merge", row[0])
	}
	if row[1].(string) != "datadoghq.com" || row[2].(uint8) != 1 || row[3].(string) != "datadogAgent" {
		t.Errorf("manifest extras = %v / %v / %v", row[1], row[2], row[3])
	}

	storagetest.Insert(t, conn, storage.K8sActionsWriter, []storage.Row{
		storage.K8sActionRow{
			TenantID: "default", Timestamp: now, ActionID: "a-1", OrgID: 77,
			EventType: "action_executed", Status: "failed",
			ExtraKeys: []string{"new_field"},
			Payloads:  map[string]string{"stderr": "bad"},
			Extra:     `{"new_field":{"nested":7}}`,
			// The unparseable string that made Timestamp fall back to arrival.
			TimestampRaw: "not-a-date",
		},
	})
	row = storagetest.QueryRow(t, conn, `SELECT payloads['stderr'], extra, timestamp_raw FROM k8s_actions`)
	if row[0].(string) != "bad" || row[2].(string) != "not-a-date" {
		t.Errorf("action payload/raw timestamp = %v / %v", row[0], row[2])
	}
	if row[1].(string) != `{"new_field":{"nested":7}}` {
		t.Errorf("extra = %q — the VALUES of undeclared keys, not just their names", row[1])
	}

	pullStarted := now.Add(-10 * time.Minute)
	storagetest.Insert(t, conn, storage.ECSTasksWriter, []storage.Row{
		storage.ECSTaskRow{
			TenantID: "default", CollectedAt: now,
			OrgID: 77, SubscriptionID: 3, HeaderTimestamp: &headerTS, Encoding: "protobuf",
			AWSAccountID: 123456789012, ClusterID: "ecs-cid", ClusterName: "ecs-prod",
			Region: "eu-west-1", GroupID: 2, GroupSize: 5, HostName: "ecs-agent-1",
			AgentVersion: "7.55.1",
			EnvelopeTags: map[string][]string{"env": {"prod"}},
			ARN:          "arn:aws:ecs:eu-west-1:123:task/abc", ResourceVersion: "rv-1",
			LaunchType: "FARGATE", DesiredStatus: "RUNNING", KnownStatus: "RUNNING",
			Family: "checkout", Version: "3", AvailabilityZone: "eu-west-1a",
			ServiceName: "checkout-svc", VpcID: "vpc-1",
			ContainerInstanceARN: "arn:container-instance/xyz", DaemonName: "datadog-agent",
			TaskHostName:            "ip-10-0-1-7",
			Limits:                  map[string]float64{"CPU": 1024},
			EphemeralStorageMetrics: map[string]int64{"utilized": 100},
			PullStartedAt:           &pullStarted,
			Containers:              `[{"name":"app"}]`,
			ContainerCount:          1,
			Tags:                    map[string][]string{"service": {"checkout"}},
			ECSTags:                 map[string][]string{"ecs": {"tag"}},
			ContainerInstanceTags:   map[string][]string{"instance": {"tag"}},
		},
	})
	if got := storagetest.Count(t, conn, "ecs_tasks"); got != 1 {
		t.Fatalf("ecs_tasks: %d rows, want 1", got)
	}
	row = storagetest.QueryRow(t, conn, `
		SELECT aws_account_id, arn, limits['CPU'], isNull(pull_stopped_at), container_instance_tags['instance']
		FROM ecs_tasks`)
	// An int64 account id must stay exact; a float64 anywhere on the path
	// would round it above 2^53.
	if row[0].(int64) != 123456789012 {
		t.Errorf("aws_account_id = %v, want 123456789012 exactly", row[0])
	}
	if row[1].(string) != "arn:aws:ecs:eu-west-1:123:task/abc" {
		t.Errorf("arn = %q", row[1])
	}
	if row[2].(float64) != 1024 {
		t.Errorf("limits['CPU'] = %v", row[2])
	}
	if row[3].(uint8) != 1 {
		t.Errorf("pull_stopped_at is not NULL — a task still pulling has none")
	}
	if got, ok := row[4].([]string); !ok || len(got) != 1 {
		t.Errorf("container_instance_tags = %v", row[4])
	}
}
