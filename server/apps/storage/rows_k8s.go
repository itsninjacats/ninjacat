package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// The Kubernetes tables: k8s_resources, k8s_manifests, k8s_cluster and
// k8s_actions. One file because they arrive together — the cluster agent's
// collection pass produces the first three, and the fourth is the audit trail
// of what it was asked to do.
//
// Every orchestrator row here carries the FRAME HEADER (OrgID, SubscriptionID,
// HeaderTimestamp, Encoding). Today's agent leaves the first three at their
// zero values, which is the reason to keep them rather than a reason to drop
// them: the day a sender starts filling them, the data is already being
// stored. HeaderTimestamp is the raw int64 rather than a time.Time because
// the frame format documents no unit for it (see the migration) — and it is
// a pointer because 0 means "not provided", which is not the same as an
// instant.

type WriteK8sResources struct{ Resources []K8sResourceRow }
type WriteK8sManifests struct{ Manifests []K8sManifestRow }
type WriteK8sCluster struct{ Clusters []K8sClusterRow }
type WriteK8sActions struct{ Actions []K8sActionRow }

func (m WriteK8sResources) rows() []Row { return toRows(m.Resources) }
func (m WriteK8sManifests) rows() []Row { return toRows(m.Manifests) }
func (m WriteK8sCluster) rows() []Row   { return toRows(m.Clusters) }
func (m WriteK8sActions) rows() []Row   { return toRows(m.Actions) }

// K8sResourceRow is one Kubernetes object in one collection pass, stored in
// ninjacat.k8s_resources — generic across every kind the orchestrator
// collectors send (Pod, Node, Deployment, ...).
//
// THREE LAYERS, and the split is what keeps 27 kinds in one table:
//
//   - fields every kind has (identity, metadata timestamps, owners,
//     conditions, resource requirements, metrics) are columns;
//   - numbers that differ per kind travel in Counts, so a new kind changes the
//     intake translation and nothing else;
//   - typed extras exist for the kinds people ask about by name (Pod, Node,
//     Service, RBAC), and Object holds the WHOLE decoded message as JSON for
//     everything else — which is what makes a StorageClass or a NetworkPolicy
//     lossless without twenty columns nobody else would ever read.
//
// The parallel slices (Owner*, Condition*, Container*) are index-aligned and
// in wire order: ConditionTypes[i] belongs with ConditionMessages[i]. Nothing
// sorts them, because the order is what the sender observed.
//
// k8s_resources_current is fed from this table by a materialized view, so
// there is no separate row type or writer for it — but the view lists every
// column by name, so a field added here needs the same column on both tables
// AND in the view (schema/migrations/0015_k8s_fidelity.sql).
type K8sResourceRow struct {
	TenantID        string
	CollectedAt     time.Time
	ClusterID       string
	ClusterName     string
	Kind            string
	Namespace       string
	Name            string
	UID             string
	ResourceVersion string
	OwnerKind       string
	OwnerName       string
	NodeName        string
	Phase           string
	Status          string
	Ready           int32
	Desired         int32
	Available       int32
	Counts          map[string]int64
	Labels          map[string]string
	Annotations     map[string]string
	Tags            map[string][]string
	AgentVersion    string
	GroupID         int32
	GroupSize       int32

	// The 16-byte frame header.
	OrgID           int32
	SubscriptionID  uint8
	HeaderTimestamp *int64
	Encoding        string

	// Metadata, in full. CreationTimestamp is the one that mattered most:
	// without it, "how old is this object" was unanswerable for every kind,
	// because CollectedAt only says when WE saw it.
	CreationTimestamp          *time.Time
	DeletionTimestamp          *time.Time
	DeletionGracePeriodSeconds *int64
	Finalizers                 []string

	// Every owner reference, not just the first. OwnerKind/OwnerName above
	// stay as the convenience columns for the Pod -> ReplicaSet -> Deployment
	// walk; these three carry the rest, including the uid a join needs.
	OwnerKinds []string
	OwnerNames []string
	OwnerUIDs  []string

	// Conditions, parallel arrays. The times are pointers because a condition
	// that never probed or never updated sends 0, and 0 is absence.
	ConditionTypes          []string
	ConditionStatuses       []string
	ConditionReasons        []string
	ConditionMessages       []string
	ConditionLastTransition []*time.Time
	ConditionLastUpdate     []*time.Time
	ConditionLastProbe      []*time.Time
	// ConditionMessage is the agent's own one-line summary, a separate wire
	// field from the condition list above.
	ConditionMessage string

	// ResourceRequirements is JSON: one object carries several of them (one
	// per container, plus init containers), each with two maps and a type, so
	// a flat map would lose which container the numbers belong to.
	ResourceRequirements string

	// Metrics is ResourceMetrics.MetricValues — the agent's own computed
	// numbers for the object. A plain map: protobuf map keys are unique, so
	// the tag multiset shape would cost lookups for nothing.
	Metrics map[string]float64

	// Object is the whole decoded collector object as JSON. See the migration
	// header for why encoding/json rather than protojson (agent-payload's
	// process package is gogo-generated and has no protoreflect).
	Object string

	// Pod (41).
	PodIP             string
	NominatedNodeName string
	QOSClass          string
	PriorityClass     string
	StartTime         *time.Time
	ScheduledTime     *time.Time
	// HostName is the reporting agent's host, which is not node_name for
	// cluster-scoped kinds.
	HostName string

	// Container statuses, parallel arrays: regular containers first, then
	// init containers, with ContainerIsInit telling them apart. Only the SUM
	// of ready and restarts used to survive, so "which container is
	// crashlooping, and with what message" had no answer at all.
	ContainerNames    []string
	ContainerIDs      []string
	ContainerReady    []uint8
	ContainerRestarts []int32
	ContainerStates   []string
	ContainerMessages []string
	ContainerImages   []string
	ContainerImageIDs []string
	ContainerIsInit   []uint8

	// Node (45).
	PodCIDR                 string
	PodCIDRs                []string
	Unschedulable           uint8
	Taints                  string // JSON: key/value/effect/timeAdded per taint
	ProviderID              string
	NodeRoles               []string
	Capacity                map[string]int64
	Allocatable             map[string]int64
	NodeAddresses           string // JSON: address type -> address
	KubeletVersion          string
	KubeProxyVersion        string
	OperatingSystem         string
	Architecture            string
	KernelVersion           string
	OSImage                 string
	ContainerRuntimeVersion string

	// Service (44).
	ServiceType  string
	ClusterIP    string
	ServicePorts string // JSON

	// RBAC (54-57). Rules are the permissions actually granted — the most
	// security-relevant field in the whole orchestrator feed.
	RBACRules    string // JSON
	RBACSubjects string // JSON
	RBACRoleRef  string // JSON

	// Envelope is the frame envelope as JSON without its object list: the
	// per-kind extras the six named envelope fields do not cover (the
	// reporting agent's own Host and SystemInfo, a Pod frame's IsTerminated,
	// a Node frame's HostAliasMapping). It repeats on every row of a pass and
	// compresses to nearly nothing, which is a cheaper trade than one column
	// per kind-specific envelope field.
	Envelope string
}

func (r K8sResourceRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.CollectedAt, r.ClusterID, r.ClusterName,
		r.Kind, r.Namespace, r.Name, r.UID, r.ResourceVersion,
		r.OwnerKind, r.OwnerName, r.NodeName, r.Phase, r.Status,
		r.Ready, r.Desired, r.Available, orEmpty(r.Counts),
		orEmpty(r.Labels), orEmpty(r.Annotations), orEmpty(r.Tags),
		r.AgentVersion, r.GroupID, r.GroupSize,
		r.OrgID, r.SubscriptionID, r.HeaderTimestamp, r.Encoding,
		r.CreationTimestamp, r.DeletionTimestamp, r.DeletionGracePeriodSeconds,
		orEmptySlice(r.Finalizers),
		orEmptySlice(r.OwnerKinds), orEmptySlice(r.OwnerNames), orEmptySlice(r.OwnerUIDs),
		orEmptySlice(r.ConditionTypes), orEmptySlice(r.ConditionStatuses),
		orEmptySlice(r.ConditionReasons), orEmptySlice(r.ConditionMessages),
		orEmptySlice(r.ConditionLastTransition), orEmptySlice(r.ConditionLastUpdate),
		orEmptySlice(r.ConditionLastProbe), r.ConditionMessage,
		r.ResourceRequirements, orEmpty(r.Metrics), r.Object,
		r.PodIP, r.NominatedNodeName, r.QOSClass, r.PriorityClass,
		r.StartTime, r.ScheduledTime, r.HostName,
		orEmptySlice(r.ContainerNames), orEmptySlice(r.ContainerIDs),
		orEmptySlice(r.ContainerReady), orEmptySlice(r.ContainerRestarts),
		orEmptySlice(r.ContainerStates), orEmptySlice(r.ContainerMessages),
		orEmptySlice(r.ContainerImages), orEmptySlice(r.ContainerImageIDs),
		orEmptySlice(r.ContainerIsInit),
		r.PodCIDR, orEmptySlice(r.PodCIDRs), r.Unschedulable, r.Taints,
		r.ProviderID, orEmptySlice(r.NodeRoles),
		orEmpty(r.Capacity), orEmpty(r.Allocatable), r.NodeAddresses,
		r.KubeletVersion, r.KubeProxyVersion, r.OperatingSystem, r.Architecture,
		r.KernelVersion, r.OSImage, r.ContainerRuntimeVersion,
		r.ServiceType, r.ClusterIP, r.ServicePorts,
		r.RBACRules, r.RBACSubjects, r.RBACRoleRef, r.Envelope)
}

// K8sManifestRow is one raw object manifest in ninjacat.k8s_manifests.
// Content is the object's own YAML or JSON, exactly as the cluster agent
// collected it — whole documents, which is why the manifests writer runs
// with much smaller batches than everything else.
type K8sManifestRow struct {
	TenantID        string
	CollectedAt     time.Time
	ClusterID       string
	ClusterName     string
	UID             string
	Kind            string
	APIVersion      string
	ResourceVersion string
	Content         string
	ContentType     string
	IsTerminated    uint8

	OrgID           int32
	SubscriptionID  uint8
	HeaderTimestamp *int64
	Encoding        string

	GroupID         int32
	GroupSize       int32
	HostName        string
	AgentVersion    string
	OriginCollector string

	// Tags is every tag that applies to this manifest, from all three places
	// the wire attaches them (frame envelope, CRD/CR wrapper, the manifest
	// itself), merged — because a query asking for env:prod does not care
	// where the tag was attached. TagSources keeps the provenance anyway:
	// source name -> that source's raw tag strings.
	Tags       map[string][]string
	TagSources map[string][]string

	NodeName        string
	Type            int32
	Version         string
	ExtraAttributes map[string]string

	// ContentIsUTF8 is 1 when Content is valid UTF-8. The wire field is
	// []byte and a ClickHouse String is a byte string, so nothing is lost
	// either way — but a reader deserves to know before parsing.
	ContentIsUTF8 uint8
}

func (r K8sManifestRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.CollectedAt, r.ClusterID, r.ClusterName,
		r.UID, r.Kind, r.APIVersion, r.ResourceVersion,
		r.Content, r.ContentType, r.IsTerminated,
		r.OrgID, r.SubscriptionID, r.HeaderTimestamp, r.Encoding,
		r.GroupID, r.GroupSize, r.HostName, r.AgentVersion, r.OriginCollector,
		orEmpty(r.Tags), orEmpty(r.TagSources),
		r.NodeName, r.Type, r.Version, orEmpty(r.ExtraAttributes),
		r.ContentIsUTF8)
}

// K8sClusterRow is one CollectorCluster summary in ninjacat.k8s_cluster:
// capacity, allocatable and the version spread (version -> node count) of
// kubelets and apiservers. One row per cluster per collection pass.
//
// The Nodes* slices are the per-node breakdown behind NodeCount, parallel and
// in wire order. Without them "which of my nodes is still on the old kubelet"
// was a histogram with no names on it.
type K8sClusterRow struct {
	TenantID          string
	CollectedAt       time.Time
	ClusterID         string
	ClusterName       string
	NodeCount         uint32
	PodCapacity       uint32
	PodAllocatable    uint32
	CPUCapacity       uint64
	CPUAllocatable    uint64
	MemoryCapacity    uint64
	MemoryAllocatable uint64
	KubeletVersions   map[string]uint32
	APIServerVersions map[string]uint32

	OrgID           int32
	SubscriptionID  uint8
	HeaderTimestamp *int64
	Encoding        string

	GroupID      int32
	GroupSize    int32
	AgentVersion string

	// Two tag sets, kept apart: the frame envelope's (CollectorCluster.Tags)
	// and the Cluster object's own (Cluster.Tags). Different fields on the
	// wire, and merging them would erase which producer attached what.
	Tags        map[string][]string
	ClusterTags map[string][]string

	ResourceVersion   string
	CreationTimestamp *time.Time
	Metrics           map[string]float64

	// GPUs and anything else a node advertises beyond cpu/memory/pods.
	ExtendedResourcesCapacity    map[string]int64
	ExtendedResourcesAllocatable map[string]int64

	NodesName                    []string
	NodesRegion                  []string
	NodesInstanceType            []string
	NodesOS                      []string
	NodesOSImage                 []string
	NodesArchitecture            []string
	NodesKernelVersion           []string
	NodesContainerRuntimeVersion []string
	NodesKubeletVersion          []string
	// The wire sends these as Kubernetes quantity STRINGS ("16Gi", "4"),
	// unlike NodeStatus which sends int64 — converting would guess at units,
	// so they are kept verbatim.
	NodesAllocatable []map[string]string
	NodesCapacity    []map[string]string
}

func (r K8sClusterRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.CollectedAt, r.ClusterID, r.ClusterName,
		r.NodeCount, r.PodCapacity, r.PodAllocatable,
		r.CPUCapacity, r.CPUAllocatable, r.MemoryCapacity, r.MemoryAllocatable,
		orEmpty(r.KubeletVersions), orEmpty(r.APIServerVersions),
		r.OrgID, r.SubscriptionID, r.HeaderTimestamp, r.Encoding,
		r.GroupID, r.GroupSize, r.AgentVersion,
		orEmpty(r.Tags), orEmpty(r.ClusterTags),
		r.ResourceVersion, r.CreationTimestamp, orEmpty(r.Metrics),
		orEmpty(r.ExtendedResourcesCapacity), orEmpty(r.ExtendedResourcesAllocatable),
		orEmptySlice(r.NodesName), orEmptySlice(r.NodesRegion),
		orEmptySlice(r.NodesInstanceType), orEmptySlice(r.NodesOS),
		orEmptySlice(r.NodesOSImage), orEmptySlice(r.NodesArchitecture),
		orEmptySlice(r.NodesKernelVersion), orEmptySlice(r.NodesContainerRuntimeVersion),
		orEmptySlice(r.NodesKubeletVersion),
		orEmptySlice(r.NodesAllocatable), orEmptySlice(r.NodesCapacity))
}

// K8sActionRow is one kubeactions result event in ninjacat.k8s_actions —
// the audit record of what the cluster agent did with a remote action.
//
// ExtraKeys holds the NAMES of JSON keys the intake struct did not declare
// and Extra holds those keys with their VALUES: the agent side of this is
// young, and a newer agent's addition is worth keeping, not just counting.
// The names stay a column of their own because filtering on them then needs
// no JSON parsing.
type K8sActionRow struct {
	TenantID          string
	Timestamp         time.Time
	ActionID          string
	OrgID             int64
	EventType         string
	Status            string
	ActionType        string
	ClusterID         string
	ClusterName       string
	ResourceID        string
	ResourceKind      string
	ResourceName      string
	ResourceNamespace string
	RequestedBy       string
	Message           string
	ExtraKeys         []string

	// Payloads is whatever the executor attached, name -> bytes. ClickHouse
	// Strings are byte strings, so a non-text attachment survives unchanged.
	Payloads map[string]string
	// Extra is the undeclared keys with their values, as a JSON object.
	Extra string
	// TimestampRaw is the wire's timestamp string exactly as it arrived.
	// Timestamp above falls back to arrival time when this is empty or
	// unparseable, and that fallback used to erase its own evidence.
	TimestampRaw string
}

func (r K8sActionRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.ActionID, r.OrgID,
		r.EventType, r.Status, r.ActionType,
		r.ClusterID, r.ClusterName, r.ResourceID,
		r.ResourceKind, r.ResourceName, r.ResourceNamespace,
		r.RequestedBy, r.Message, orEmptySlice(r.ExtraKeys),
		orEmpty(r.Payloads), r.Extra, r.TimestampRaw)
}

func init() {
	registerWriter(K8sResourcesWriter, WriterConfig{
		Name: "k8s_resources",
		Insert: `INSERT INTO k8s_resources
			(tenant_id, collected_at, cluster_id, cluster_name, kind, namespace, name,
			 uid, resource_version, owner_kind, owner_name, node_name, phase, status,
			 ready, desired, available, counts, labels, annotations, tags,
			 agent_version, group_id, group_size,
			 org_id, subscription_id, header_timestamp, encoding,
			 creation_timestamp, deletion_timestamp, deletion_grace_period_seconds,
			 finalizers, owner_kinds, owner_names, owner_uids,
			 condition_types, condition_statuses, condition_reasons, condition_messages,
			 condition_last_transition, condition_last_update, condition_last_probe,
			 condition_message, resource_requirements, metrics, object,
			 pod_ip, nominated_node_name, qos_class, priority_class,
			 start_time, scheduled_time, host_name,
			 container_names, container_ids, container_ready, container_restarts,
			 container_states, container_messages, container_images,
			 container_image_ids, container_is_init,
			 pod_cidr, pod_cidrs, unschedulable, taints, provider_id, node_roles,
			 capacity, allocatable, node_addresses,
			 kubelet_version, kube_proxy_version, operating_system, architecture,
			 kernel_version, os_image, container_runtime_version,
			 service_type, cluster_ip, service_ports,
			 rbac_rules, rbac_subjects, rbac_role_ref, envelope)`,
		// The cluster agent sends whole collection passes at once: thousands of
		// objects in a burst every ~10s, then silence. MaxRows keeps a burst
		// from becoming one giant insert, the buffer holds a few full passes
		// while ClickHouse hiccups, and a second in-flight flush lets the next
		// pass start draining before the previous insert lands. Rows now carry
		// the object's JSON as well as its label and annotation maps, so the
		// explicit ceiling matters more than ever — 50k of these is real memory.
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, K8sResourceRow{})

	registerWriter(K8sManifestsWriter, WriterConfig{
		Name: "k8s_manifests",
		Insert: `INSERT INTO k8s_manifests
			(tenant_id, collected_at, cluster_id, cluster_name, uid, kind, api_version,
			 resource_version, content, content_type, is_terminated,
			 org_id, subscription_id, header_timestamp, encoding,
			 group_id, group_size, host_name, agent_version, origin_collector,
			 tags, tag_sources, node_name, type, version, extra_attributes,
			 content_is_utf8)`,
		// Every row is a whole YAML document, so these row counts stand for
		// megabytes: 200 rows can be 2 MB of insert and 5000 buffered can be
		// tens of MB. The tight ceilings bound memory, and a single in-flight
		// flush is plenty for what is bulk, not urgency.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, K8sManifestRow{})

	registerWriter(K8sClusterWriter, WriterConfig{
		Name: "k8s_cluster",
		Insert: `INSERT INTO k8s_cluster
			(tenant_id, collected_at, cluster_id, cluster_name, node_count,
			 pod_capacity, pod_allocatable, cpu_capacity, cpu_allocatable,
			 memory_capacity, memory_allocatable, kubelet_versions, apiserver_versions,
			 org_id, subscription_id, header_timestamp, encoding,
			 group_id, group_size, agent_version, tags, cluster_tags,
			 resource_version, creation_timestamp, metrics,
			 extended_resources_capacity, extended_resources_allocatable,
			 nodes_name, nodes_region, nodes_instance_type, nodes_os, nodes_os_image,
			 nodes_architecture, nodes_kernel_version, nodes_container_runtime_version,
			 nodes_kubelet_version, nodes_allocatable, nodes_capacity)`,
		// One row per cluster per pass — the quietest table here. The timer
		// does all the flushing; the thresholds only exist so a stall cannot
		// grow the buffer unbounded.
		MaxRows: 50, FlushInterval: 15 * time.Second,
		BufferLimit: 1_000, MaxInFlight: 1,
	}, K8sClusterRow{})

	registerWriter(K8sActionsWriter, WriterConfig{
		Name: "k8s_actions",
		Insert: `INSERT INTO k8s_actions
			(tenant_id, timestamp, action_id, org_id, event_type, status, action_type,
			 cluster_id, cluster_name, resource_id, resource_kind, resource_name,
			 resource_namespace, requested_by, message, extra_keys,
			 payloads, extra, timestamp_raw)`,
		// Human scale, like the events table: an operator triggers an action
		// and expects to see its result — a 10s timer is about as long as
		// that wait should get. Small everything, one flush at a time.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 10_000, MaxInFlight: 1,
	}, K8sActionRow{})
}
