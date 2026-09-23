package intake

import (
	"encoding/binary"
	"encoding/json"
	"net/http"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/DataDog/agent-payload/v5/process"
	"github.com/itsninjacats/server/apps/storage"
)

// The orchestrator converter is one reflection-based function for ~27 message
// types, so the tests pin the contract it must keep for all of them: the kind
// comes from the element's type name, identity comes from Metadata, and the
// envelope (cluster, group, agent version) lands on every row.
//
// The kinds with no typed case of their own are covered too, because that is
// where the loss used to be: a Role's rules, a Node's taints, an HPA's
// targets were decoded and dropped. They now reach a column or the object
// JSON, and these tests are what says so.

// testFrame is a frame header with every field set, so a test can tell "the
// converter copied the header" from "the header happened to be zero". A real
// agent leaves all four at their zero values today.
var testFrame = orchFrame{
	OrgID:          77,
	SubscriptionID: 3,
	Timestamp:      func() *int64 { v := int64(1700000000); return &v }(),
	Encoding:       "protobuf",
}

func TestOrchResourceRowsIdentity(t *testing.T) {
	now := time.Now().UTC()
	meta := func(ns, name, uid string, owner *process.OwnerReference) *process.Metadata {
		md := &process.Metadata{Namespace: ns, Name: name, Uid: uid, ResourceVersion: "42"}
		if owner != nil {
			md.OwnerReferences = []*process.OwnerReference{owner}
		}
		return md
	}

	cases := []struct {
		name string
		body process.MessageBody
		want struct {
			kind, ns, objName, uid, ownerKind, ownerName string
		}
	}{
		{
			name: "pod",
			body: &process.CollectorPod{
				ClusterName: "prod", ClusterId: "cid-1", GroupId: 3, GroupSize: 7,
				Pods: []*process.Pod{{
					Metadata: meta("default", "web-abc12", "uid-pod", &process.OwnerReference{Kind: "ReplicaSet", Name: "web"}),
				}},
			},
			want: struct{ kind, ns, objName, uid, ownerKind, ownerName string }{
				"Pod", "default", "web-abc12", "uid-pod", "ReplicaSet", "web",
			},
		},
		{
			name: "deployment",
			body: &process.CollectorDeployment{
				ClusterName: "prod", ClusterId: "cid-1", GroupId: 3, GroupSize: 7,
				Deployments: []*process.Deployment{{
					Metadata: meta("shop", "checkout", "uid-dep", nil),
				}},
			},
			want: struct{ kind, ns, objName, uid, ownerKind, ownerName string }{
				"Deployment", "shop", "checkout", "uid-dep", "", "",
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rows := orchResourceRows("tenant-1", now, testFrame, tc.body)
			if len(rows) != 1 {
				t.Fatalf("got %d rows, want 1", len(rows))
			}
			r := rows[0]
			if r.Kind != tc.want.kind {
				t.Errorf("Kind = %q, want %q", r.Kind, tc.want.kind)
			}
			if r.Namespace != tc.want.ns || r.Name != tc.want.objName || r.UID != tc.want.uid {
				t.Errorf("identity = %s/%s uid=%s, want %s/%s uid=%s",
					r.Namespace, r.Name, r.UID, tc.want.ns, tc.want.objName, tc.want.uid)
			}
			if r.OwnerKind != tc.want.ownerKind || r.OwnerName != tc.want.ownerName {
				t.Errorf("owner = %s/%s, want %s/%s", r.OwnerKind, r.OwnerName, tc.want.ownerKind, tc.want.ownerName)
			}
			if r.ResourceVersion != "42" {
				t.Errorf("ResourceVersion = %q, want 42", r.ResourceVersion)
			}
			// The envelope must land on every row, whatever the kind.
			if r.TenantID != "tenant-1" || r.ClusterName != "prod" || r.ClusterID != "cid-1" {
				t.Errorf("envelope = tenant=%q cluster=%q (%s)", r.TenantID, r.ClusterName, r.ClusterID)
			}
			if r.GroupID != 3 || r.GroupSize != 7 {
				t.Errorf("group = %d/%d, want 3/7", r.GroupID, r.GroupSize)
			}
			if !r.CollectedAt.Equal(now) {
				t.Errorf("CollectedAt = %v, want %v", r.CollectedAt, now)
			}
		})
	}
}

// Pod numbers are sums over the container statuses, the same arithmetic the
// log line uses: ready containers counted, restarts added up across all of
// them, total taken from the slice length.
func TestOrchResourceRowsPodCounts(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorPod{
		Pods: []*process.Pod{{
			Metadata: &process.Metadata{Namespace: "ns", Name: "p", Uid: "u"},
			NodeName: "node-1",
			Phase:    "Running",
			Status:   "CrashLoopBackOff",
			ContainerStatuses: []*process.ContainerStatus{
				{Name: "app", Ready: true, RestartCount: 4},
				{Name: "sidecar", Ready: true, RestartCount: 1},
				{Name: "init-ish", Ready: false, RestartCount: 0},
			},
		}},
	})
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.NodeName != "node-1" || r.Phase != "Running" || r.Status != "CrashLoopBackOff" {
		t.Errorf("node/phase/status = %q/%q/%q", r.NodeName, r.Phase, r.Status)
	}
	if r.Ready != 2 || r.Desired != 3 {
		t.Errorf("ready/desired = %d/%d, want 2/3", r.Ready, r.Desired)
	}
	want := map[string]int64{
		"restarts": 5, "ready_containers": 2, "total_containers": 3,
		"init_restarts": 0, "ready_init_containers": 0, "total_init_containers": 0,
		"pod_restart_count": 0,
	}
	if !reflect.DeepEqual(r.Counts, want) {
		t.Errorf("Counts = %v, want %v", r.Counts, want)
	}
}

// Deployment numbers come from dedicated fields; the stable snake_case keys
// are part of the storage contract, so they are pinned here.
func TestOrchResourceRowsDeploymentCounts(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorDeployment{
		Deployments: []*process.Deployment{{
			Metadata:            &process.Metadata{Namespace: "ns", Name: "d", Uid: "u"},
			ReplicasDesired:     5,
			ReadyReplicas:       3,
			UpdatedReplicas:     4,
			AvailableReplicas:   3,
			UnavailableReplicas: 2,
		}},
	})
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.Ready != 3 || r.Desired != 5 || r.Available != 3 {
		t.Errorf("ready/desired/available = %d/%d/%d, want 3/5/3", r.Ready, r.Desired, r.Available)
	}
	want := map[string]int64{"ready": 3, "desired": 5, "updated": 4, "available": 3, "unavailable": 2, "replicas": 0}
	if !reflect.DeepEqual(r.Counts, want) {
		t.Errorf("Counts = %v, want %v", r.Counts, want)
	}
}

// A nil element in the collection and an object without Metadata must neither
// panic nor produce a row: without namespace/name/uid there is nothing to
// store the object under.
func TestOrchResourceRowsToleratesNils(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorPod{
		Pods: []*process.Pod{
			nil,
			{}, // Metadata omitted on the wire
			{Metadata: &process.Metadata{Namespace: "ns", Name: "ok", Uid: "u"}},
		},
	})
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1 (nil element and nil Metadata skipped)", len(rows))
	}
	if rows[0].Name != "ok" {
		t.Errorf("surviving row is %q, want ok", rows[0].Name)
	}
}

// The CRD and CR frames wrap an inner *process.CollectorManifest that the
// agent may leave nil; the converter must treat that as an empty collection.
// A nil element inside a populated envelope gets the same treatment.
func TestOrchManifestRowsNilEnvelope(t *testing.T) {
	now := time.Now().UTC()

	if rows := orchManifestRows("t", now, testFrame, nil, nil); len(rows) != 0 {
		t.Errorf("nil envelope: got %d rows, want 0", len(rows))
	}
	// The exact path the handler takes for an empty CRD frame.
	if rows := orchManifestRows("t", now, testFrame, (&process.CollectorManifestCRD{}).GetManifest(), nil); len(rows) != 0 {
		t.Errorf("empty CRD frame: got %d rows, want 0", len(rows))
	}

	rows := orchManifestRows("t", now, testFrame, &process.CollectorManifest{
		ClusterName: "prod", ClusterId: "cid",
		Manifests: []*process.Manifest{
			nil,
			{Uid: "u1", Kind: "ConfigMap", ApiVersion: "v1", ResourceVersion: "7",
				Content: []byte("kind: ConfigMap"), ContentType: "yaml", IsTerminated: true},
		},
	}, nil)
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.UID != "u1" || r.Kind != "ConfigMap" || r.Content != "kind: ConfigMap" || r.IsTerminated != 1 {
		t.Errorf("row = %+v", r)
	}
}

// An empty (or unparseable) timestamp means "not provided" and must become
// arrival time — the wireTime rule — never 1970-01-01.
func TestKubeActionRowTimestamp(t *testing.T) {
	arrival := time.Date(2026, 9, 21, 12, 0, 0, 0, time.UTC)

	cases := []struct {
		name string
		ts   string
		want time.Time
	}{
		{"empty falls back to arrival", "", arrival},
		{"unparseable falls back to arrival", "yesterday-ish", arrival},
		{"valid RFC 3339 is kept", "2026-09-20T08:30:00Z", time.Date(2026, 9, 20, 8, 30, 0, 0, time.UTC)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			row := kubeActionRow("t", arrival, kubeActionResultEvent{Timestamp: tc.ts})
			if !row.Timestamp.Equal(tc.want) {
				t.Errorf("Timestamp = %v, want %v", row.Timestamp, tc.want)
			}
			if row.Timestamp.Year() == 1970 {
				t.Errorf("Timestamp collapsed to the epoch")
			}
		})
	}
}

// Keys the struct does not declare must surface, sorted, in ExtraKeys — the
// whole trip: raw JSON through kubeActionDecode into the row.
func TestKubeActionRowExtraKeys(t *testing.T) {
	raw := json.RawMessage(`{
		"action_id": "a-1",
		"event_type": "action_executed",
		"status": "success",
		"zeta_field": 1,
		"alpha_field": {"nested": true}
	}`)
	ev, err := kubeActionDecode(raw)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	row := kubeActionRow("t", time.Now().UTC(), ev)
	if row.ActionID != "a-1" || row.EventType != "action_executed" || row.Status != "success" {
		t.Errorf("declared fields = %q/%q/%q", row.ActionID, row.EventType, row.Status)
	}
	want := []string{"alpha_field", "zeta_field"}
	if !reflect.DeepEqual(row.ExtraKeys, want) {
		t.Errorf("ExtraKeys = %v, want %v", row.ExtraKeys, want)
	}
}

// ---------------------------------------------------------------------------
// Fidelity: the fields the converter used to decode and drop
// ---------------------------------------------------------------------------

// The frame header was read into a variable and thrown away before the switch
// ran, for every orchestrator payload of every type. Today's agent leaves it
// zero, which is precisely why the columns have to exist before a sender
// starts filling them.
func TestOrchResourceRowsCarriesFrameHeader(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorPod{
		Pods: []*process.Pod{{Metadata: &process.Metadata{Name: "p", Uid: "u"}}},
	})
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]
	if r.OrgID != 77 || r.SubscriptionID != 3 || r.Encoding != "protobuf" {
		t.Errorf("header = org=%d sub=%d encoding=%q, want 77/3/protobuf", r.OrgID, r.SubscriptionID, r.Encoding)
	}
	if r.HeaderTimestamp == nil || *r.HeaderTimestamp != 1700000000 {
		t.Errorf("HeaderTimestamp = %v, want 1700000000 as the raw wire integer", r.HeaderTimestamp)
	}

	// 0 on the wire means "not provided" and must stay absent rather than
	// become an instant in whichever unit we guessed.
	rows = orchResourceRows("t", time.Now().UTC(), orchFrameOf(process.MessageHeader{}), &process.CollectorPod{
		Pods: []*process.Pod{{Metadata: &process.Metadata{Name: "p", Uid: "u"}}},
	})
	if rows[0].HeaderTimestamp != nil {
		t.Errorf("HeaderTimestamp = %v for an unset header, want nil", *rows[0].HeaderTimestamp)
	}
}

// Metadata is the part every one of the 27 kinds shares, and five of its
// fields never reached a column: when the object was created, whether it is
// being deleted, its finalizers, and every owner reference after the first —
// including the uid, which is the only thing a join can use.
func TestOrchResourceRowsMetadataInFull(t *testing.T) {
	created := int64(1600000000)
	deleted := int64(1600003600)

	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorPod{
		Pods: []*process.Pod{{Metadata: &process.Metadata{
			Name: "web-abc", Uid: "uid-1",
			CreationTimestamp:          created,
			DeletionTimestamp:          deleted,
			DeletionGracePeriodSeconds: 30,
			Finalizers:                 []string{"kubernetes.io/pvc-protection"},
			OwnerReferences: []*process.OwnerReference{
				{Kind: "ReplicaSet", Name: "web", Uid: "uid-rs"},
				{Kind: "Foo", Name: "bar", Uid: "uid-foo"},
			},
		}}},
	})
	r := rows[0]

	if r.CreationTimestamp == nil || r.CreationTimestamp.Unix() != created {
		t.Errorf("CreationTimestamp = %v, want %d", r.CreationTimestamp, created)
	}
	if r.DeletionTimestamp == nil || r.DeletionTimestamp.Unix() != deleted {
		t.Errorf("DeletionTimestamp = %v, want %d", r.DeletionTimestamp, deleted)
	}
	if r.DeletionGracePeriodSeconds == nil || *r.DeletionGracePeriodSeconds != 30 {
		t.Errorf("DeletionGracePeriodSeconds = %v, want 30", r.DeletionGracePeriodSeconds)
	}
	if !reflect.DeepEqual(r.Finalizers, []string{"kubernetes.io/pvc-protection"}) {
		t.Errorf("Finalizers = %v", r.Finalizers)
	}
	if r.OwnerKind != "ReplicaSet" || r.OwnerName != "web" {
		t.Errorf("first owner = %s/%s, want ReplicaSet/web", r.OwnerKind, r.OwnerName)
	}
	if !reflect.DeepEqual(r.OwnerUIDs, []string{"uid-rs", "uid-foo"}) {
		t.Errorf("OwnerUIDs = %v, want both, in wire order", r.OwnerUIDs)
	}
	if len(r.OwnerKinds) != 2 || len(r.OwnerNames) != 2 {
		t.Errorf("owner arrays = %v / %v, want two entries each", r.OwnerKinds, r.OwnerNames)
	}
}

// An object that is NOT being deleted has no grace period, and 0 there is
// absence — while for an object that IS being deleted, 0 is a force delete
// and a real value. The pointer is what keeps the two apart.
func TestOrchResourceRowsGracePeriodAbsentWithoutDeletion(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorPod{
		Pods: []*process.Pod{{Metadata: &process.Metadata{Name: "p", Uid: "u"}}},
	})
	if rows[0].DeletionGracePeriodSeconds != nil {
		t.Errorf("grace period = %v for a live object, want nil", *rows[0].DeletionGracePeriodSeconds)
	}
	if rows[0].DeletionTimestamp != nil || rows[0].CreationTimestamp != nil {
		t.Errorf("absent metadata dates became %v / %v, want nil — never 1970",
			rows[0].CreationTimestamp, rows[0].DeletionTimestamp)
	}
}

// A three-container pod where one container is crashlooping: only the SUM of
// ready and restarts used to survive, so "which container, and with what
// message" had no answer at all. Init containers were counted nowhere.
func TestOrchResourceRowsPodContainerStatuses(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorPod{
		Pods: []*process.Pod{{
			Metadata:          &process.Metadata{Namespace: "shop", Name: "checkout-1", Uid: "u"},
			IP:                "10.4.2.9",
			NominatedNodeName: "node-9",
			QOSClass:          "Burstable",
			PriorityClass:     "high",
			StartTime:         1600000100,
			ScheduledTime:     1600000050,
			Host:              &process.Host{Name: "cluster-agent-1"},
			ContainerStatuses: []*process.ContainerStatus{
				{Name: "app", ContainerID: "containerd://aaa", Ready: true, RestartCount: 0,
					State: "running", Image: "shop/app:1.2", ImageID: "sha256:aaa"},
				{Name: "sidecar", ContainerID: "containerd://bbb", Ready: false, RestartCount: 7,
					State: "waiting", Message: "back-off 5m0s restarting failed container",
					Image: "envoy:1.29", ImageID: "sha256:bbb"},
			},
			InitContainerStatuses: []*process.ContainerStatus{
				{Name: "wait-for-db", ContainerID: "containerd://ccc", Ready: true, State: "terminated"},
			},
			Conditions: []*process.PodCondition{
				{Type: "Ready", Status: "False", Reason: "ContainersNotReady",
					Message:            "containers with unready status: [sidecar]",
					LastTransitionTime: 1600000200, LastProbeTime: 1600000300},
			},
			ResourceRequirements: []*process.ResourceRequirements{
				{Name: "app", Requests: map[string]int64{"cpu": 100}, Limits: map[string]int64{"memory": 536870912}},
			},
			Metrics: &process.ResourceMetrics{MetricValues: map[string]float64{"cpu.usage": 0.42}},
		}},
	})
	r := rows[0]

	if r.PodIP != "10.4.2.9" || r.NominatedNodeName != "node-9" || r.QOSClass != "Burstable" || r.PriorityClass != "high" {
		t.Errorf("scheduling fields = %q %q %q %q", r.PodIP, r.NominatedNodeName, r.QOSClass, r.PriorityClass)
	}
	if r.StartTime == nil || r.ScheduledTime == nil {
		t.Fatalf("start/scheduled = %v / %v, want both set", r.StartTime, r.ScheduledTime)
	}
	if r.HostName != "cluster-agent-1" {
		t.Errorf("HostName = %q, want the reporting host", r.HostName)
	}

	// Regular containers first, then init containers, told apart by the flag.
	wantNames := []string{"app", "sidecar", "wait-for-db"}
	if !reflect.DeepEqual(r.ContainerNames, wantNames) {
		t.Errorf("ContainerNames = %v, want %v (regular first, init appended)", r.ContainerNames, wantNames)
	}
	if !reflect.DeepEqual(r.ContainerIsInit, []uint8{0, 0, 1}) {
		t.Errorf("ContainerIsInit = %v, want [0 0 1]", r.ContainerIsInit)
	}
	if !reflect.DeepEqual(r.ContainerReady, []uint8{1, 0, 1}) {
		t.Errorf("ContainerReady = %v", r.ContainerReady)
	}
	if !reflect.DeepEqual(r.ContainerRestarts, []int32{0, 7, 0}) {
		t.Errorf("ContainerRestarts = %v", r.ContainerRestarts)
	}
	// The line an operator is actually after.
	if r.ContainerMessages[1] != "back-off 5m0s restarting failed container" {
		t.Errorf("ContainerMessages[1] = %q", r.ContainerMessages[1])
	}
	if r.ContainerImages[1] != "envoy:1.29" || r.ContainerImageIDs[1] != "sha256:bbb" {
		t.Errorf("container image/imageID = %q / %q", r.ContainerImages[1], r.ContainerImageIDs[1])
	}

	// Conditions: the reason and message are the half that says what to do.
	if len(r.ConditionTypes) != 1 || r.ConditionTypes[0] != "Ready" || r.ConditionStatuses[0] != "False" {
		t.Fatalf("conditions = %v / %v", r.ConditionTypes, r.ConditionStatuses)
	}
	if r.ConditionReasons[0] != "ContainersNotReady" ||
		r.ConditionMessages[0] != "containers with unready status: [sidecar]" {
		t.Errorf("condition reason/message = %q / %q", r.ConditionReasons[0], r.ConditionMessages[0])
	}
	if r.ConditionLastTransition[0] == nil || r.ConditionLastProbe[0] == nil {
		t.Errorf("condition times = %v / %v, want both set", r.ConditionLastTransition[0], r.ConditionLastProbe[0])
	}
	// PodCondition has no lastUpdateTime, so that slot must be absent rather
	// than a fabricated date — the arrays stay index-aligned either way.
	if r.ConditionLastUpdate[0] != nil {
		t.Errorf("ConditionLastUpdate = %v, want nil: PodCondition has no such field", r.ConditionLastUpdate[0])
	}

	if !strings.Contains(r.ResourceRequirements, `"cpu":100`) {
		t.Errorf("ResourceRequirements = %q, want the request numbers", r.ResourceRequirements)
	}
	if r.Metrics["cpu.usage"] != 0.42 {
		t.Errorf("Metrics = %v", r.Metrics)
	}
}

// A cordoned node with a taint. unschedulable was logged and dropped; the
// taints, the per-node capacity and every version string never had a column,
// so "which node is full, and on what kubelet" was a cluster-wide sum.
func TestOrchResourceRowsNodeDetail(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorNode{
		Nodes: []*process.Node{{
			Metadata:      &process.Metadata{Name: "node-1", Uid: "u"},
			PodCIDR:       "10.244.1.0/24",
			PodCIDRs:      []string{"10.244.1.0/24"},
			Unschedulable: true,
			ProviderID:    "aws:///eu-west-1a/i-0abc",
			Roles:         []string{"worker"},
			Taints: []*process.Taint{
				{Key: "node.kubernetes.io/unreachable", Value: "", Effect: "NoExecute", TimeAdded: 1600000000},
			},
			Status: &process.NodeStatus{
				Status:                  "Ready,SchedulingDisabled",
				Capacity:                map[string]int64{"cpu": 8000, "memory": 33554432000},
				Allocatable:             map[string]int64{"cpu": 7800},
				NodeAddresses:           map[string]string{"InternalIP": "10.0.1.7"},
				KubeletVersion:          "v1.29.4",
				KubeProxyVersion:        "v1.29.4",
				OperatingSystem:         "linux",
				Architecture:            "amd64",
				KernelVersion:           "6.1.0",
				OsImage:                 "Amazon Linux 2",
				ContainerRuntimeVersion: "containerd://1.7.13",
				Conditions: []*process.NodeCondition{
					{Type: "MemoryPressure", Status: "False", Reason: "KubeletHasSufficientMemory",
						LastTransitionTime: 1600000500},
				},
			},
		}},
	})
	r := rows[0]

	if r.Unschedulable != 1 {
		t.Errorf("Unschedulable = %d, want 1 — the cordon flag is the first thing anyone asks", r.Unschedulable)
	}
	if r.PodCIDR != "10.244.1.0/24" || len(r.PodCIDRs) != 1 {
		t.Errorf("pod CIDRs = %q / %v", r.PodCIDR, r.PodCIDRs)
	}
	if r.ProviderID != "aws:///eu-west-1a/i-0abc" {
		t.Errorf("ProviderID = %q", r.ProviderID)
	}
	if !strings.Contains(r.Taints, "node.kubernetes.io/unreachable") || !strings.Contains(r.Taints, "NoExecute") {
		t.Errorf("Taints = %q, want the taint key and effect", r.Taints)
	}
	if r.Capacity["cpu"] != 8000 || r.Allocatable["cpu"] != 7800 {
		t.Errorf("capacity/allocatable = %v / %v", r.Capacity, r.Allocatable)
	}
	if !strings.Contains(r.NodeAddresses, "10.0.1.7") {
		t.Errorf("NodeAddresses = %q", r.NodeAddresses)
	}
	if r.KubeletVersion != "v1.29.4" || r.ContainerRuntimeVersion != "containerd://1.7.13" || r.OSImage != "Amazon Linux 2" {
		t.Errorf("versions = %q / %q / %q", r.KubeletVersion, r.ContainerRuntimeVersion, r.OSImage)
	}
	// NodeStatus.Conditions is the node health signal, and it lives one level
	// down from the object — the converter has to look in Status too.
	if len(r.ConditionTypes) != 1 || r.ConditionTypes[0] != "MemoryPressure" {
		t.Errorf("conditions = %v, want the ones nested in Status", r.ConditionTypes)
	}
	if r.Counts["taints"] != 1 {
		t.Errorf("Counts[taints] = %d, want 1", r.Counts["taints"])
	}
}

// An HPA had zero coverage: min/max/current/desired replicas — the entire
// point of autoscaling — were decoded and thrown away.
func TestOrchResourceRowsHPACounts(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorHorizontalPodAutoscaler{
		HorizontalPodAutoscalers: []*process.HorizontalPodAutoscaler{{
			Metadata: &process.Metadata{Namespace: "shop", Name: "checkout", Uid: "u"},
			Spec:     &process.HorizontalPodAutoscalerSpec{MinReplicas: 2, MaxReplicas: 20},
			Status:   &process.HorizontalPodAutoscalerStatus{CurrentReplicas: 6, DesiredReplicas: 11},
			Conditions: []*process.HorizontalPodAutoscalerCondition{
				{ConditionType: "ScalingLimited", ConditionStatus: "True", Reason: "TooManyReplicas"},
			},
		}},
	})
	r := rows[0]
	if r.Kind != "HorizontalPodAutoscaler" {
		t.Fatalf("Kind = %q", r.Kind)
	}
	if r.Ready != 6 || r.Desired != 11 {
		t.Errorf("current/desired = %d/%d, want 6/11", r.Ready, r.Desired)
	}
	want := map[string]int64{"min_replicas": 2, "max_replicas": 20, "current_replicas": 6, "desired_replicas": 11}
	if !reflect.DeepEqual(r.Counts, want) {
		t.Errorf("Counts = %v, want %v", r.Counts, want)
	}
	// An HPA names its condition fields ConditionType/ConditionStatus where
	// every other kind says Type/Status. Reading only one convention would
	// have stored a row of empty conditions and looked like a healthy one.
	if len(r.ConditionTypes) != 1 || r.ConditionTypes[0] != "ScalingLimited" || r.ConditionStatuses[0] != "True" {
		t.Errorf("conditions = %v / %v, want ScalingLimited/True", r.ConditionTypes, r.ConditionStatuses)
	}
}

// Role.Rules is the list of permissions actually granted — arguably the most
// security-relevant field in the whole orchestrator feed, and it had no column
// anywhere.
func TestOrchResourceRowsRoleRules(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorRole{
		Roles: []*process.Role{{
			Metadata: &process.Metadata{Namespace: "kube-system", Name: "secret-reader", Uid: "u"},
			Rules: []*process.PolicyRule{
				{ApiGroups: []string{""}, Resources: []string{"secrets"}, Verbs: []string{"get", "list"}},
			},
		}},
	})
	r := rows[0]
	if r.Counts["rules"] != 1 {
		t.Errorf("Counts[rules] = %d, want 1", r.Counts["rules"])
	}
	for _, want := range []string{"secrets", "get", "list"} {
		if !strings.Contains(r.RBACRules, want) {
			t.Errorf("RBACRules = %q, want it to contain %q", r.RBACRules, want)
		}
	}
}

// Twenty of the twenty-seven kinds have no typed case, and that is fine only
// because the whole message reaches the object column. A StorageClass is the
// worked example: nothing about a provisioner or its parameters has a column
// of its own, and none of it may be lost.
func TestOrchResourceRowsObjectAndEnvelopeAreLossless(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorStorageClass{
		ClusterName: "prod",
		StorageClasses: []*process.StorageClass{{
			Metadata:          &process.Metadata{Name: "gp3", Uid: "u"},
			Provisioner:       "ebs.csi.aws.com",
			Parameters:        map[string]string{"type": "gp3", "encrypted": "true"},
			ReclaimPolicy:     "Delete",
			VolumeBindingMode: "WaitForFirstConsumer",
		}},
	})
	r := rows[0]
	for _, want := range []string{"ebs.csi.aws.com", "gp3", "WaitForFirstConsumer", "Delete"} {
		if !strings.Contains(r.Object, want) {
			t.Errorf("Object = %q, want it to contain %q", r.Object, want)
		}
	}

	// The envelope keeps the per-kind extras nothing else reads, and must NOT
	// repeat the object list — that would square the payload.
	if !strings.Contains(r.Envelope, "prod") {
		t.Errorf("Envelope = %q, want the cluster name", r.Envelope)
	}
	if strings.Contains(r.Envelope, "ebs.csi.aws.com") {
		t.Errorf("Envelope repeats the object list: %q", r.Envelope)
	}
}

// CollectorPod carries envelope fields the six named ones do not cover, and
// they used to vanish with the frame: the cluster agent's own host and system
// info, and the flag that says this batch is the terminated-pods one.
func TestOrchResourceRowsEnvelopeKeepsPodFrameExtras(t *testing.T) {
	rows := orchResourceRows("t", time.Now().UTC(), testFrame, &process.CollectorPod{
		HostName:     "cluster-agent-1",
		IsTerminated: true,
		Info:         &process.SystemInfo{Uuid: "sys-uuid"},
		Pods:         []*process.Pod{{Metadata: &process.Metadata{Name: "p", Uid: "u"}}},
	})
	r := rows[0]
	if r.HostName != "cluster-agent-1" {
		t.Errorf("HostName = %q, want the envelope's", r.HostName)
	}
	for _, want := range []string{"isTerminated", "sys-uuid"} {
		if !strings.Contains(r.Envelope, want) {
			t.Errorf("Envelope = %q, want it to contain %q", r.Envelope, want)
		}
	}
}

// The cluster summary kept nine numbers and dropped the envelope, the
// cluster's own tags, its extended resources and the whole per-node
// breakdown — so "which of my nodes is still on the old kubelet" was a
// histogram with no names on it.
func TestOrchClusterRowsNodesInfo(t *testing.T) {
	rows := orchClusterRows("t", time.Now().UTC(), testFrame, &process.CollectorCluster{
		ClusterName: "prod", ClusterId: "cid", GroupId: 1, GroupSize: 2,
		Tags:         []string{"env:prod", "team:infra"},
		AgentVersion: &process.AgentVersion{Major: 7, Minor: 55, Patch: 1},
		Cluster: &process.Cluster{
			NodeCount:                 2,
			ResourceVersion:           "9912",
			CreationTimestamp:         1600000000,
			Tags:                      []string{"kube_cluster_name:prod"},
			KubeletVersions:           map[string]int32{"v1.29.4": 1, "v1.28.9": 1},
			Metrics:                   &process.ResourceMetrics{MetricValues: map[string]float64{"nodes.ready": 2}},
			ExtendedResourcesCapacity: map[string]int64{"nvidia.com/gpu": 4},
			NodesInfo: []*process.ClusterNodeInfo{
				{Name: "node-1", Region: "eu-west-1", InstanceType: "m6i.large", KubeletVersion: "v1.29.4",
					ResourceCapacity: map[string]string{"cpu": "2"}},
				{Name: "node-2", Region: "eu-west-1", InstanceType: "m6i.xlarge", KubeletVersion: "v1.28.9"},
			},
		},
	})
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]

	if r.OrgID != 77 || r.Encoding != "protobuf" {
		t.Errorf("frame header missing: org=%d encoding=%q", r.OrgID, r.Encoding)
	}
	if r.GroupID != 1 || r.GroupSize != 2 || r.AgentVersion != "7.55.1" {
		t.Errorf("envelope = group %d/%d agent %q", r.GroupID, r.GroupSize, r.AgentVersion)
	}
	// Two tag sets, NOT merged: which producer attached a tag is the point.
	if !reflect.DeepEqual(r.Tags["env"], []string{"prod"}) {
		t.Errorf("envelope Tags = %v", r.Tags)
	}
	if !reflect.DeepEqual(r.ClusterTags["kube_cluster_name"], []string{"prod"}) {
		t.Errorf("ClusterTags = %v", r.ClusterTags)
	}
	if r.ResourceVersion != "9912" || r.CreationTimestamp == nil {
		t.Errorf("resource version / creation = %q / %v", r.ResourceVersion, r.CreationTimestamp)
	}
	if r.Metrics["nodes.ready"] != 2 || r.ExtendedResourcesCapacity["nvidia.com/gpu"] != 4 {
		t.Errorf("metrics / extended = %v / %v", r.Metrics, r.ExtendedResourcesCapacity)
	}

	if !reflect.DeepEqual(r.NodesName, []string{"node-1", "node-2"}) {
		t.Errorf("NodesName = %v, want both in wire order", r.NodesName)
	}
	if !reflect.DeepEqual(r.NodesKubeletVersion, []string{"v1.29.4", "v1.28.9"}) {
		t.Errorf("NodesKubeletVersion = %v", r.NodesKubeletVersion)
	}
	if r.NodesCapacity[0]["cpu"] != "2" {
		t.Errorf("NodesCapacity[0] = %v", r.NodesCapacity[0])
	}
	// An Array(Map(...)) rejects a nil element, and a node that reported no
	// quantities is normal — the hole must be an empty map, not nil.
	if r.NodesCapacity[1] == nil || r.NodesAllocatable[1] == nil {
		t.Errorf("empty node maps are nil; the driver rejects a nil inside an array")
	}
}

// A CRD frame carries tags in three places and manifest rows used to carry
// none of them. Merged for querying, kept apart in TagSources for provenance.
func TestOrchManifestRowsMergesTagsFromThreeSources(t *testing.T) {
	rows := orchManifestRows("t", time.Now().UTC(), testFrame, &process.CollectorManifest{
		ClusterName: "prod", ClusterId: "cid", GroupId: 4, GroupSize: 8,
		HostName:        "cluster-agent-1",
		AgentVersion:    &process.AgentVersion{Major: 7, Minor: 55, Patch: 1},
		OriginCollector: process.OriginCollector_datadogAgent,
		Tags:            []string{"env:prod"},
		Manifests: []*process.Manifest{{
			Uid: "u1", Kind: "DatadogAgent", ApiVersion: "datadoghq.com/v2alpha1",
			Type: 81, Version: "v1", NodeName: "node-3",
			Content: []byte("apiVersion: datadoghq.com/v2alpha1"), ContentType: "json",
			Tags:            []string{"team:infra"},
			ExtraAttributes: map[string]string{"crd_group": "datadoghq.com"},
		}},
	}, []string{"source:crd"})

	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1", len(rows))
	}
	r := rows[0]

	if r.GroupID != 4 || r.GroupSize != 8 || r.HostName != "cluster-agent-1" || r.AgentVersion != "7.55.1" {
		t.Errorf("envelope = group %d/%d host %q agent %q", r.GroupID, r.GroupSize, r.HostName, r.AgentVersion)
	}
	if r.OriginCollector == "" || r.OrgID != 77 {
		t.Errorf("origin/header = %q / %d", r.OriginCollector, r.OrgID)
	}
	for k, want := range map[string]string{"env": "prod", "source": "crd", "team": "infra"} {
		if got := r.Tags[k]; len(got) != 1 || got[0] != want {
			t.Errorf("Tags[%q] = %v, want [%s] — all three sources merge", k, got, want)
		}
	}
	if !reflect.DeepEqual(r.TagSources["wrapper"], []string{"source:crd"}) {
		t.Errorf("TagSources[wrapper] = %v — provenance must survive the merge", r.TagSources["wrapper"])
	}
	if r.NodeName != "node-3" || r.Type != 81 || r.Version != "v1" {
		t.Errorf("manifest fields = %q / %d / %q", r.NodeName, r.Type, r.Version)
	}
	if r.ExtraAttributes["crd_group"] != "datadoghq.com" {
		t.Errorf("ExtraAttributes = %v", r.ExtraAttributes)
	}
	if r.ContentIsUTF8 != 1 {
		t.Errorf("ContentIsUTF8 = %d for a YAML document, want 1", r.ContentIsUTF8)
	}
}

// Content is []byte on the wire and a byte string in ClickHouse, so nothing
// is corrupted — but a reader must be told before it tries to parse.
func TestOrchManifestRowsFlagsNonUTF8Content(t *testing.T) {
	rows := orchManifestRows("t", time.Now().UTC(), testFrame, &process.CollectorManifest{
		Manifests: []*process.Manifest{{Uid: "u", Content: []byte{0xff, 0xfe, 0x00}}},
	}, nil)
	if rows[0].ContentIsUTF8 != 0 {
		t.Errorf("ContentIsUTF8 = 1 for bytes that are not valid UTF-8")
	}
	if len(rows[0].Content) != 3 {
		t.Errorf("content length = %d, want 3 — the bytes themselves must survive", len(rows[0].Content))
	}
}

// ECS tasks were decoded and discarded with a comment: no Kubernetes identity
// to store them under. They have their own table now, keyed on the ARN.
func TestOrchECSTaskRows(t *testing.T) {
	rows := orchECSTaskRows("t", time.Now().UTC(), testFrame, &process.CollectorECSTask{
		AwsAccountID: 123456789012,
		ClusterName:  "ecs-prod", ClusterId: "ecs-cid", Region: "eu-west-1",
		GroupId: 2, GroupSize: 5,
		HostName:     "ecs-agent-1",
		AgentVersion: &process.AgentVersion{Major: 7, Minor: 55, Patch: 1},
		Tags:         []string{"env:prod"},
		Tasks: []*process.ECSTask{
			nil,
			{
				Arn: "arn:aws:ecs:eu-west-1:123:task/abc", ResourceVersion: "rv-1",
				LaunchType: "FARGATE", DesiredStatus: "RUNNING", KnownStatus: "RUNNING",
				Family: "checkout", Version: "3", AvailabilityZone: "eu-west-1a",
				ServiceName: "checkout-svc", VpcId: "vpc-1",
				ContainerInstanceArn:    "arn:aws:ecs:...:container-instance/xyz",
				DaemonName:              "datadog-agent",
				Host:                    &process.Host{Name: "ip-10-0-1-7"},
				Limits:                  map[string]float64{"CPU": 1024, "Memory": 2048},
				EphemeralStorageMetrics: map[string]int64{"utilized": 100},
				PullStartedAt:           1600000000,
				PullStoppedAt:           1600000030,
				Containers:              []*process.ECSContainer{{Name: "app", DockerID: "d1"}},
				Tags:                    []string{"service:checkout"},
				EcsTags:                 []string{"ecs:tag"},
				ContainerInstanceTags:   []string{"instance:tag"},
			},
		},
	})
	if len(rows) != 1 {
		t.Fatalf("got %d rows, want 1 (the nil task is skipped)", len(rows))
	}
	r := rows[0]

	if r.ARN != "arn:aws:ecs:eu-west-1:123:task/abc" || r.Family != "checkout" || r.LaunchType != "FARGATE" {
		t.Errorf("task identity = %q / %q / %q", r.ARN, r.Family, r.LaunchType)
	}
	if r.AWSAccountID != 123456789012 {
		t.Errorf("AWSAccountID = %d — an int64 id must not go through a float", r.AWSAccountID)
	}
	if r.Region != "eu-west-1" || r.HostName != "ecs-agent-1" || r.TaskHostName != "ip-10-0-1-7" {
		t.Errorf("hosts/region = %q / %q / %q", r.Region, r.HostName, r.TaskHostName)
	}
	if r.Limits["CPU"] != 1024 || r.EphemeralStorageMetrics["utilized"] != 100 {
		t.Errorf("limits/storage = %v / %v", r.Limits, r.EphemeralStorageMetrics)
	}
	if r.PullStartedAt == nil || r.PullStoppedAt == nil || r.ExecutionStoppedAt != nil {
		t.Errorf("pull/execution times = %v / %v / %v — the unset one must stay nil",
			r.PullStartedAt, r.PullStoppedAt, r.ExecutionStoppedAt)
	}
	if r.ContainerCount != 1 || !strings.Contains(r.Containers, "app") {
		t.Errorf("containers = %d / %q", r.ContainerCount, r.Containers)
	}
	// Four tag sets, four columns: merging them would answer "is this
	// env:prod" while destroying "who said so".
	if len(r.EnvelopeTags["env"]) != 1 || len(r.Tags["service"]) != 1 ||
		len(r.ECSTags["ecs"]) != 1 || len(r.ContainerInstanceTags["instance"]) != 1 {
		t.Errorf("tag sets = %v / %v / %v / %v", r.EnvelopeTags, r.Tags, r.ECSTags, r.ContainerInstanceTags)
	}
}

// The attachments and the VALUES of undeclared keys were both dropped "by
// design". Both are small, both arrive once, and both are what somebody
// debugging a failed remote action wants.
func TestKubeActionRowKeepsPayloadsAndExtraValues(t *testing.T) {
	raw := json.RawMessage(`{
		"action_id": "a-1",
		"status": "failed",
		"timestamp": "not-a-date",
		"payloads": {"stderr": "YmFk"},
		"new_field": {"nested": 7}
	}`)
	ev, err := kubeActionDecode(raw)
	if err != nil {
		t.Fatalf("decode: %v", err)
	}
	arrival := time.Date(2026, 9, 21, 12, 0, 0, 0, time.UTC)
	row := kubeActionRow("t", arrival, ev)

	if row.Payloads["stderr"] != "bad" {
		t.Errorf("Payloads[stderr] = %q, want the base64 decoded by encoding/json", row.Payloads["stderr"])
	}
	if !strings.Contains(row.Extra, "new_field") || !strings.Contains(row.Extra, "7") {
		t.Errorf("Extra = %q, want the undeclared key WITH its value", row.Extra)
	}
	if !reflect.DeepEqual(row.ExtraKeys, []string{"new_field"}) {
		t.Errorf("ExtraKeys = %v — the names stay a column of their own", row.ExtraKeys)
	}
	// The fallback to arrival time is right, but it used to erase its own
	// evidence: the unparseable string had to be read out of a log.
	if row.TimestampRaw != "not-a-date" {
		t.Errorf("TimestampRaw = %q, want the string exactly as it arrived", row.TimestampRaw)
	}
	if !row.Timestamp.Equal(arrival) {
		t.Errorf("Timestamp = %v, want the arrival fallback", row.Timestamp)
	}
}

// ---------------------------------------------------------------------------
// Through the real engine
// ---------------------------------------------------------------------------

// orchFrameBytes builds the 16-byte V3 frame the cluster agent sends: version,
// encoding, type, subscription id, int32 org id, int64 timestamp, then the
// protobuf body. Written out by hand because agent-payload exports a decoder
// and no encoder — which is also why a test that goes through the HTTP path is
// worth having: process.DecodeMessage is the only thing that says these bytes
// are the shape a real agent produces.
func orchFrameBytes(t *testing.T, msgType uint8, body interface{ Marshal() ([]byte, error) }) []byte {
	t.Helper()
	payload, err := body.Marshal()
	if err != nil {
		t.Fatalf("marshal body: %v", err)
	}
	frame := make([]byte, 0, 16+len(payload))
	frame = append(frame,
		uint8(process.MessageV3),
		uint8(process.MessageEncodingProtobuf),
		msgType,
		3, // subscription id
	)
	frame = binary.LittleEndian.AppendUint32(frame, uint32(77))         // org id
	frame = binary.LittleEndian.AppendUint64(frame, uint64(1700000000)) // timestamp
	return append(frame, payload...)
}

// The whole path for a Kubernetes collection: key guard, host routing, frame
// decode, conversion, and the batch arriving at the writer that owns the
// table. A handler that returns 202 and sends nothing looks perfectly healthy
// from outside, which is the failure this catches.
func TestHandleOrchestratorStoresResourceRows(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeKubeops)

	body := orchFrameBytes(t, process.TypeCollectorPod, &process.CollectorPod{
		ClusterName: "prod", ClusterId: "cid",
		Pods: []*process.Pod{
			{Metadata: &process.Metadata{Namespace: "shop", Name: "checkout-1", Uid: "u1"},
				ContainerStatuses: []*process.ContainerStatus{{Name: "app", Ready: true}}},
			{Metadata: &process.Metadata{Namespace: "shop", Name: "checkout-2", Uid: "u2"}},
		},
	})

	w := post(t, e, "/api/v2/orch", body)
	if w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/orch: got %d, want 202 (%s)", w.Code, w.Body.String())
	}
	// The 202 with an empty object is the contract every sender on these
	// intakes depends on; nothing here may ever answer differently.
	if got := strings.TrimSpace(w.Body.String()); got != "{}" {
		t.Errorf("body = %q, want {}", got)
	}

	if msgs := node.SentTo(storage.K8sResourcesWriter); len(msgs) != 1 {
		t.Fatalf("messages to %s: got %d, want 1 — one batch per request", storage.K8sResourcesWriter, len(msgs))
	}
	rows := Rows[storage.K8sResourceRow](node)
	if len(rows) != 2 {
		t.Fatalf("stored rows: got %d, want 2 — every object in the frame becomes a row", len(rows))
	}
	for i, r := range rows {
		if r.TenantID != testTenant {
			t.Errorf("row %d tenant: got %q, want %q", i, r.TenantID, testTenant)
		}
		if r.Kind != "Pod" || r.ClusterID != "cid" {
			t.Errorf("row %d = kind %q cluster %q", i, r.Kind, r.ClusterID)
		}
		// The header the frame carried, read off the wire rather than passed
		// in by a test helper.
		if r.OrgID != 77 || r.SubscriptionID != 3 || r.Encoding != "protobuf" {
			t.Errorf("row %d header = org=%d sub=%d encoding=%q", i, r.OrgID, r.SubscriptionID, r.Encoding)
		}
		if r.HeaderTimestamp == nil || *r.HeaderTimestamp != 1700000000 {
			t.Errorf("row %d HeaderTimestamp = %v", i, r.HeaderTimestamp)
		}
	}
}

// ECS tasks arrive on the same intake as the Kubernetes collectors and used
// to be decoded and discarded. They must reach their own writer, not
// k8s_resources.
func TestHandleOrchestratorStoresECSTasks(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeKubeops)

	body := orchFrameBytes(t, process.TypeCollectorECSTask, &process.CollectorECSTask{
		ClusterName: "ecs-prod", Region: "eu-west-1",
		Tasks: []*process.ECSTask{{Arn: "arn:task/1", Family: "checkout"}},
	})

	if w := post(t, e, "/api/v2/orch", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}
	rows := Rows[storage.ECSTaskRow](node)
	if len(rows) != 1 {
		t.Fatalf("ecs_tasks rows: got %d, want 1", len(rows))
	}
	if rows[0].ARN != "arn:task/1" || rows[0].TenantID != testTenant {
		t.Errorf("row = %+v", rows[0])
	}
	if n := len(Rows[storage.K8sResourceRow](node)); n != 0 {
		t.Errorf("%d k8s_resources rows for an ECS frame — a task has no Kubernetes identity", n)
	}
}

// A message from the process agent's own family on an orchestrator intake is
// a misconfigured agent. The frame is whole and decoded, this router has
// nowhere to put it, and the bytes must survive for whoever owns that track.
func TestHandleOrchestratorStoresUnexpectedShapeRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeKubeops)

	body := orchFrameBytes(t, process.TypeCollectorProc, &process.CollectorProc{HostName: "h1"})

	if w := post(t, e, "/api/v2/orch", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 — a misrouted frame is still accepted", w.Code)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 {
		t.Fatalf("raw_payloads rows: got %d, want 1", len(raw))
	}
	if raw[0].Intake != "orchestrator" || raw[0].Reason != "unexpected_shape" {
		t.Errorf("intake/reason = %q/%q, want orchestrator/unexpected_shape", raw[0].Intake, raw[0].Reason)
	}
	if !strings.Contains(raw[0].Note, "CollectorProc") {
		t.Errorf("note = %q, want it to name the type that arrived", raw[0].Note)
	}
	if raw[0].BodyBytes != uint64(len(body)) {
		t.Errorf("stored %d bytes of a %d byte frame", raw[0].BodyBytes, len(body))
	}
}

// A body that is not a frame at all: the decoder error is the only thing we
// know about it, so the bytes are what a future decoder will need.
func TestHandleOrchestratorStoresUndecodableFrameRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeKubeops)

	if w := post(t, e, "/api/v2/orch", []byte("not a frame")); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 || raw[0].Reason != "decode_error" {
		t.Fatalf("raw_payloads: %d rows, reason %q", len(raw), func() string {
			if len(raw) == 0 {
				return ""
			}
			return raw[0].Reason
		}())
	}
	if raw[0].Body != "not a frame" {
		t.Errorf("body = %q, want the bytes the handler saw", raw[0].Body)
	}
}

// kubeactions is JSON, not a frame, and a body that is not a JSON array has
// only its bytes to offer.
func TestHandleKubeActionsStoresRowsAndRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeKubeops)

	body := []byte(`[{"action_id":"a-1","event_type":"action_executed","status":"success",
		"cluster_name":"prod","resource_kind":"Deployment","resource_name":"checkout"}]`)
	if w := post(t, e, "/api/v2/kubeactions", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}
	rows := Rows[storage.K8sActionRow](node)
	if len(rows) != 1 || rows[0].ActionID != "a-1" || rows[0].TenantID != testTenant {
		t.Fatalf("k8s_actions rows = %+v", rows)
	}

	a2, node2 := newTestServer(t)
	e2 := newTestEngine(t, a2, a2.routeKubeops)
	if w := post(t, e2, "/api/v2/kubeactions", []byte(`{"not":"an array"}`)); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}
	raw := Rows[storage.RawPayloadRow](node2)
	if len(raw) != 1 || raw[0].Intake != "kubeactions" || raw[0].Reason != "decode_error" {
		t.Errorf("raw_payloads = %+v", raw)
	}
}
