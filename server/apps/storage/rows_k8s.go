package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// The Kubernetes tables: k8s_resources, k8s_manifests, k8s_cluster and
// k8s_actions. One file because they arrive together — the cluster agent's
// collection pass produces the first three, and the fourth is the audit trail
// of what it was asked to do.

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
// The columns all kinds share are fields; the numbers that differ per kind
// (restarts, updated replicas, taints...) travel in Counts, so a new kind
// changes the intake translation, never this struct or the table.
// k8s_resources_current is fed from this table by a materialized view, so
// there is no separate row type or writer for it.
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
}

func (r K8sResourceRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.CollectedAt, r.ClusterID, r.ClusterName,
		r.Kind, r.Namespace, r.Name, r.UID, r.ResourceVersion,
		r.OwnerKind, r.OwnerName, r.NodeName, r.Phase, r.Status,
		r.Ready, r.Desired, r.Available, orEmpty(r.Counts),
		orEmpty(r.Labels), orEmpty(r.Annotations), orEmpty(r.Tags),
		r.AgentVersion, r.GroupID, r.GroupSize)
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
}

func (r K8sManifestRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.CollectedAt, r.ClusterID, r.ClusterName,
		r.UID, r.Kind, r.APIVersion, r.ResourceVersion,
		r.Content, r.ContentType, r.IsTerminated)
}

// K8sClusterRow is one CollectorCluster summary in ninjacat.k8s_cluster:
// capacity, allocatable and the version spread (version -> node count) of
// kubelets and apiservers. One row per cluster per collection pass.
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
}

func (r K8sClusterRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.CollectedAt, r.ClusterID, r.ClusterName,
		r.NodeCount, r.PodCapacity, r.PodAllocatable,
		r.CPUCapacity, r.CPUAllocatable, r.MemoryCapacity, r.MemoryAllocatable,
		orEmpty(r.KubeletVersions), orEmpty(r.APIServerVersions))
}

// K8sActionRow is one kubeactions result event in ninjacat.k8s_actions —
// the audit record of what the cluster agent did with a remote action.
//
// ExtraKeys holds the names of JSON keys the intake struct did not declare:
// the agent side of this is young, and knowing WHAT a newer agent added is
// worth a column even when its values are not kept.
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
}

func (r K8sActionRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.ActionID, r.OrgID,
		r.EventType, r.Status, r.ActionType,
		r.ClusterID, r.ClusterName, r.ResourceID,
		r.ResourceKind, r.ResourceName, r.ResourceNamespace,
		r.RequestedBy, r.Message, orEmptySlice(r.ExtraKeys))
}

func init() {
	registerWriter(K8sResourcesWriter, WriterConfig{
		Name: "k8s_resources",
		Insert: `INSERT INTO k8s_resources
			(tenant_id, collected_at, cluster_id, cluster_name, kind, namespace, name,
			 uid, resource_version, owner_kind, owner_name, node_name, phase, status,
			 ready, desired, available, counts, labels, annotations, tags,
			 agent_version, group_id, group_size)`,
		// The cluster agent sends whole collection passes at once: thousands of
		// objects in a burst every ~10s, then silence. MaxRows keeps a burst
		// from becoming one giant insert, the buffer holds a few full passes
		// while ClickHouse hiccups, and a second in-flight flush lets the next
		// pass start draining before the previous insert lands. Rows carry
		// label and annotation maps, so 50k of them is real memory — hence an
		// explicit ceiling instead of the 200k default.
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, K8sResourceRow{})

	registerWriter(K8sManifestsWriter, WriterConfig{
		Name: "k8s_manifests",
		Insert: `INSERT INTO k8s_manifests
			(tenant_id, collected_at, cluster_id, cluster_name, uid, kind, api_version,
			 resource_version, content, content_type, is_terminated)`,
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
			 memory_capacity, memory_allocatable, kubelet_versions, apiserver_versions)`,
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
			 resource_namespace, requested_by, message, extra_keys)`,
		// Human scale, like the events table: an operator triggers an action
		// and expects to see its result — a 10s timer is about as long as
		// that wait should get. Small everything, one flush at a time.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 10_000, MaxInFlight: 1,
	}, K8sActionRow{})
}
