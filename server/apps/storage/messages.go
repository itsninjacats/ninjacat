package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/google/uuid"
)

// Message types accepted by the writer actors.
//
// These are deliberately independent of the HTTP layer: storage knows nothing
// about Gin or Datadog wire formats. Handlers translate their own structures
// into these and send them. That way a second source (OTLP) can be plugged in
// without touching the write path.

// Row is anything that knows how to add itself to a prepared ClickHouse batch.
//
// This is what lets a single BatchWriter implementation serve every table:
// the writer owns buffering, flushing and backpressure, while each row type
// owns its own column order.
type Row interface {
	AppendTo(batch driver.Batch) error
}

// Messages sent to writers. Delivered with Send (asynchronous), so the handler
// never waits — the agent gets its 202 immediately.
//
// Why an actor instead of writing straight from the handler:
//
//   - ClickHouse dies from many small INSERTs ("Too many parts"). Something has
//     to buffer, and an actor processes messages one at a time, so the buffer
//     needs no locking.
//   - This is the single point every datapoint passes through — the natural
//     home for cardinality limits and tag stripping later on.
//
// Note this is the OPPOSITE of the API key situation. There the read is
// synchronous and blocks the response, so it goes through an atomic.Pointer.
// Here the write is asynchronous and can lag — an actor is ideal.
//
// IMPORTANT: each message carries a CONCRETE slice, not []Row.
//
// A single generic message with an interface field looks tidier, but Ergo
// cannot serialize it for delivery to another node:
//
//	RegisterTypes: unresolvable types: storage.WriteRows
//
// The encoder has to know what is inside the slice, and an interface does not
// tell it. Since the whole point of splitting storage into its own application
// is to run it on a separate node one day, the wire format must stay concrete.
// The Row interface still does its job — inside the writer's buffer, where
// nothing is serialized.

type WriteMetrics struct{ Points []MetricPoint }
type WriteSketches struct{ Sketches []SketchRow }
type WriteCheckRuns struct{ Runs []CheckRunRow }
type WriteLogs struct{ Entries []LogRow }
type WriteHosts struct{ Hosts []HostRow }
type WriteProcesses struct{ Processes []ProcessRow }
type WriteEvents struct{ Events []EventRow }
type WriteK8sResources struct{ Resources []K8sResourceRow }
type WriteK8sManifests struct{ Manifests []K8sManifestRow }
type WriteK8sCluster struct{ Clusters []K8sClusterRow }
type WriteK8sActions struct{ Actions []K8sActionRow }
type WriteContainerEvents struct{ Events []ContainerEventRow }
type WriteContainerImages struct{ Images []ContainerImageRow }

// rowsProvider lets BatchWriter stay generic despite the concrete messages:
// it asks the message for its rows instead of knowing the types itself.
// This interface is never serialized — it is only used locally, after the
// message has already arrived as a concrete value.
type rowsProvider interface {
	rows() []Row
}

func (m WriteMetrics) rows() []Row   { return toRows(m.Points) }
func (m WriteSketches) rows() []Row  { return toRows(m.Sketches) }
func (m WriteCheckRuns) rows() []Row { return toRows(m.Runs) }
func (m WriteLogs) rows() []Row      { return toRows(m.Entries) }
func (m WriteHosts) rows() []Row     { return toRows(m.Hosts) }
func (m WriteProcesses) rows() []Row { return toRows(m.Processes) }
func (m WriteEvents) rows() []Row    { return toRows(m.Events) }

func (m WriteK8sResources) rows() []Row    { return toRows(m.Resources) }
func (m WriteK8sManifests) rows() []Row    { return toRows(m.Manifests) }
func (m WriteK8sCluster) rows() []Row      { return toRows(m.Clusters) }
func (m WriteK8sActions) rows() []Row      { return toRows(m.Actions) }
func (m WriteContainerEvents) rows() []Row { return toRows(m.Events) }
func (m WriteContainerImages) rows() []Row { return toRows(m.Images) }

func toRows[T Row](items []T) []Row {
	out := make([]Row, len(items))
	for i, it := range items {
		out[i] = it
	}
	return out
}

// flush tells a writer to drain its buffer regardless of size.
// Sent periodically by a timer.
type flush struct{}

// ---------------------------------------------------------------------------
// Row types, one per table
// ---------------------------------------------------------------------------

// MetricPoint is one row in ninjacat.metrics.
type MetricPoint struct {
	TenantID   string
	Timestamp  time.Time
	Metric     string
	Host       string
	MetricType string // GAUGE / COUNT / RATE
	SourceType string
	Unit       string
	Interval   uint32
	Value      float64
	// Tags is key -> every value seen: Datadog tags are a multiset, and two
	// tags may legally share a key (docs/decisions/0001-tags-are-a-multiset.md).
	// Same shape on every row type carrying Datadog tags.
	Tags map[string][]string
}

func (r MetricPoint) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Metric, r.Host,
		r.MetricType, r.SourceType, r.Unit, r.Interval, r.Value, orEmpty(r.Tags))
}

// SketchRow is one DDSketch in ninjacat.sketches. Distribution metrics arrive
// as bucketed histograms rather than single values — see docs/datadog-agent.md.
type SketchRow struct {
	TenantID     string
	Timestamp    time.Time
	Metric       string
	Host         string
	Tags         map[string][]string
	Count        uint64
	Min          float64
	Max          float64
	Avg          float64
	Sum          float64
	BucketKeys   []int32
	BucketCounts []uint32
}

func (r SketchRow) AppendTo(b driver.Batch) error {
	keys := r.BucketKeys
	if keys == nil {
		keys = []int32{}
	}
	counts := r.BucketCounts
	if counts == nil {
		counts = []uint32{}
	}
	return b.Append(r.TenantID, r.Timestamp, r.Metric, r.Host, orEmpty(r.Tags),
		r.Count, r.Min, r.Max, r.Avg, r.Sum, keys, counts)
}

// CheckRunRow is one service check result in ninjacat.check_runs.
// Status is an Enum8 in ClickHouse, so it travels as its name, not its number.
type CheckRunRow struct {
	TenantID  string
	Timestamp time.Time
	CheckName string
	Host      string
	Status    string // OK / WARNING / CRITICAL / UNKNOWN
	Message   string
	Tags      map[string][]string
}

func (r CheckRunRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.CheckName, r.Host,
		r.Status, r.Message, orEmpty(r.Tags))
}

// LogRow is one log entry in ninjacat.logs.
type LogRow struct {
	TenantID  string
	Timestamp time.Time
	Host      string
	Service   string
	Source    string
	Status    string
	Message   string
	Tags      map[string][]string
}

func (r LogRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.Service,
		r.Source, r.Status, r.Message, orEmpty(r.Tags))
}

// HostRow is host metadata from /intake/, stored in ninjacat.hosts.
// The table is a ReplacingMergeTree keyed on (tenant_id, host), so repeated
// writes for the same host collapse to the newest one.
type HostRow struct {
	TenantID     string
	Host         string
	SeenAt       time.Time
	AgentVersion string
	OS           string
	Platform     map[string]string
	CPU          map[string]string
	Memory       map[string]string
	Tags         map[string][]string
}

func (r HostRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Host, r.SeenAt, r.AgentVersion, r.OS,
		orEmpty(r.Platform), orEmpty(r.CPU), orEmpty(r.Memory), orEmpty(r.Tags))
}

// ProcessRow is one process from a process-agent snapshot, stored in
// ninjacat.processes.
//
// Snapshots are BULKY: a few hundred processes per host every ~10s. The table
// has a shorter TTL than metrics for that reason.
type ProcessRow struct {
	TenantID    string
	Timestamp   time.Time
	Host        string
	PID         int32
	PPID        int32
	User        string
	Comm        string
	Exe         string
	Cmdline     string
	RSS         uint64
	VMS         uint64
	CPUPct      float32
	Threads     int32
	OpenFDs     int32
	State       string
	CreateTime  time.Time
	ContainerID string
	Tags        map[string][]string
}

func (r ProcessRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host,
		r.PID, r.PPID, r.User, r.Comm, r.Exe, r.Cmdline,
		r.RSS, r.VMS, r.CPUPct, r.Threads, r.OpenFDs,
		r.State, r.CreateTime, r.ContainerID, orEmpty(r.Tags))
}

// EventRow is one event in ninjacat.events: a deploy, a restart, a config
// change — something you draw on a chart as a vertical line.
//
// EventIDNum exists because Datadog's API hands the caller back a numeric
// event id, and clients store it to build links. We keep both: a UUID for
// ourselves and the numeric form we already returned.
type EventRow struct {
	TenantID       string
	Timestamp      time.Time
	EventID        uuid.UUID
	EventIDNum     uint64
	Title          string
	Text           string
	Host           string
	AlertType      string // error / warning / info / success / user_update / recommendation / snapshot
	Priority       string // normal / low
	AggregationKey string
	SourceTypeName string
	DeviceName     string
	Tags           map[string][]string
}

func (r EventRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.EventID, r.EventIDNum,
		r.Title, r.Text, r.Host, r.AlertType, r.Priority,
		r.AggregationKey, r.SourceTypeName, r.DeviceName, orEmpty(r.Tags))
}

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

// ContainerEventRow is one lifecycle event in ninjacat.container_events —
// the restart/OOM history.
//
// ExitCode and the three timestamps are POINTERS because the wire makes
// distinctions a zero value would erase: exit 0 and "no code reported" are
// different states (see lcExitCode on the intake side), and a missing
// timestamp must stay missing, never become 1970. nil travels to the table's
// Nullable columns as NULL.
type ContainerEventRow struct {
	TenantID      string
	Timestamp     time.Time
	Host          string
	ClusterID     string
	ObjectKind    string
	EventType     string
	ContainerID   string
	ContainerName string
	PodUID        string
	TaskARN       string
	Source        string
	ExitCode      *int32
	CreatedAt     *time.Time
	ExitedAt      *time.Time
	OwnerType     string
	OwnerUID      string
	OldState      string
	NewState      string
	TransitionAt  *time.Time
}

func (r ContainerEventRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.ClusterID,
		r.ObjectKind, r.EventType, r.ContainerID, r.ContainerName,
		r.PodUID, r.TaskARN, r.Source,
		r.ExitCode, r.CreatedAt, r.ExitedAt,
		r.OwnerType, r.OwnerUID, r.OldState, r.NewState, r.TransitionAt)
}

// ContainerImageRow is one image sighting in ninjacat.container_images.
//
// The table replaces on (tenant, digest, host): the digest identifies the
// immutable content, the host says where it sits, so Host is part of the
// row's identity, not just context. BuiltAt and PublishedAt are pointers for
// the same reason as ContainerEventRow's timestamps — the agent often has
// neither, and absence must survive the trip (nil -> NULL).
type ContainerImageRow struct {
	TenantID    string
	CollectedAt time.Time
	Host        string

	// ImageKey is what the table replaces on: the registry digest when there
	// is one, the image id otherwise. IdentitySource records which, because a
	// digestless image is a normal occurrence worth being able to ask about.
	ImageKey       string
	IdentitySource string // "digest" | "image_id"

	ImageID      string
	Digest       string
	Name         string
	ShortName    string
	Registry     string
	RepoTags     []string
	RepoDigests  []string
	SizeBytes    uint64
	OSName       string
	OSVersion    string
	Architecture string
	LayerCount   uint32
	LayerBytes   uint64
	BuiltAt      *time.Time
	PublishedAt  *time.Time
	DDTags       map[string][]string
}

func (r ContainerImageRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.CollectedAt, r.Host,
		r.ImageKey, r.IdentitySource, r.ImageID, r.Digest,
		r.Name, r.ShortName, r.Registry,
		orEmptySlice(r.RepoTags), orEmptySlice(r.RepoDigests),
		r.SizeBytes, r.OSName, r.OSVersion, r.Architecture,
		r.LayerCount, r.LayerBytes, r.BuiltAt, r.PublishedAt,
		orEmpty(r.DDTags))
}

// orEmpty guards against nil maps — the ClickHouse driver rejects them.
// Generic since the Kubernetes tables brought map values beyond string
// (Counts is int64, the cluster version spreads are uint32); the multiset tag
// maps (map[string][]string) and the older string-to-string callers all infer
// their types unchanged.
func orEmpty[K comparable, V any](m map[K]V) map[K]V {
	if m == nil {
		return map[K]V{}
	}
	return m
}

// orEmptySlice is the same guard for slices — the driver rejects nil there
// too, which SketchRow handles inline. Factored out here because the
// container rows carry several slices each and the inline form stops
// paying its way.
func orEmptySlice[T any](s []T) []T {
	if s == nil {
		return []T{}
	}
	return s
}
