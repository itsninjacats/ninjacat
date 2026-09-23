package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// ecs_tasks — CollectorECSTask (message type 200), which arrives on the same
// orchestrator intake as the Kubernetes collectors and used to be decoded and
// thrown away.
//
// Its own file and its own table rather than a squeeze into k8s_resources: an
// ECS task has no Kubernetes Metadata, so it has no namespace, no name and no
// uid — the three columns k8s_resources sorts by. Forcing it in would either
// invent identity or leave the sort key empty, and both make every query over
// that table worse for the sake of a different cloud's objects.

// ECSTasksWriter is the process handlers send WriteECSTasks to.
//
// The atom lives here, not in application.go, because this table's file owns
// everything about it — see registry.go. The older writers keep their names in
// application.go only because two packages already reference them there.
const ECSTasksWriter = gen.Atom("storage_ecs_tasks")

// WriteECSTasks carries a batch of ECS tasks. Concrete slice, never []Row —
// see the note in messages.go.
type WriteECSTasks struct{ Tasks []ECSTaskRow }

func (m WriteECSTasks) rows() []Row { return toRows(m.Tasks) }

// ECSTaskRow is one ECS task in one collection pass.
//
// FOUR tag sets, kept apart on purpose. The frame envelope carries one
// (EnvelopeTags) and the task carries three more that ECS itself keeps
// distinct: Datadog tags, the task's own ECS tags, and the tags of the
// container instance it runs on. Merging them would answer "is this tagged
// env:prod" while destroying "who said so", and the container instance's tags
// in particular belong to the machine, not the workload.
//
// Containers is JSON rather than twenty parallel arrays: an ECSContainer
// carries ports, networks, volumes, health and log options, each a repeated
// sub-message of its own, so the parallel-array trick would need arrays of
// arrays of structs. ContainerCount stands beside it so the common question
// needs no parsing.
type ECSTaskRow struct {
	TenantID    string
	CollectedAt time.Time

	// The 16-byte frame header, as on every other orchestrator row.
	OrgID           int32
	SubscriptionID  uint8
	HeaderTimestamp *int64
	Encoding        string

	// The envelope: CollectorECSTask itself.
	AWSAccountID int64
	ClusterID    string
	ClusterName  string
	Region       string
	GroupID      int32
	GroupSize    int32
	HostName     string
	AgentVersion string
	EnvelopeTags map[string][]string

	// The task.
	ARN                  string
	ResourceVersion      string
	LaunchType           string
	DesiredStatus        string
	KnownStatus          string
	Family               string
	Version              string
	AvailabilityZone     string
	ServiceName          string
	VpcID                string
	ContainerInstanceARN string
	DaemonName           string
	TaskHostName         string

	Limits                  map[string]float64
	EphemeralStorageMetrics map[string]int64

	// Unix-seconds on the wire and all three genuinely optional: a task still
	// pulling has no PullStoppedAt, and 0 must not become 1970.
	PullStartedAt      *time.Time
	PullStoppedAt      *time.Time
	ExecutionStoppedAt *time.Time

	Containers     string
	ContainerCount uint32

	Tags                  map[string][]string
	ECSTags               map[string][]string
	ContainerInstanceTags map[string][]string
}

// AppendTo must match the INSERT column list registered below, argument for
// argument. storagetest's arity test checks that off the zero row handed to
// registerWriter.
func (r ECSTaskRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.CollectedAt,
		r.OrgID, r.SubscriptionID, r.HeaderTimestamp, r.Encoding,
		r.AWSAccountID, r.ClusterID, r.ClusterName, r.Region,
		r.GroupID, r.GroupSize, r.HostName, r.AgentVersion,
		orEmpty(r.EnvelopeTags),
		r.ARN, r.ResourceVersion, r.LaunchType, r.DesiredStatus, r.KnownStatus,
		r.Family, r.Version, r.AvailabilityZone, r.ServiceName, r.VpcID,
		r.ContainerInstanceARN, r.DaemonName, r.TaskHostName,
		orEmpty(r.Limits), orEmpty(r.EphemeralStorageMetrics),
		r.PullStartedAt, r.PullStoppedAt, r.ExecutionStoppedAt,
		r.Containers, r.ContainerCount,
		orEmpty(r.Tags), orEmpty(r.ECSTags), orEmpty(r.ContainerInstanceTags))
}

func init() {
	registerWriter(ECSTasksWriter, WriterConfig{
		Name: "ecs_tasks",
		Insert: `INSERT INTO ecs_tasks
			(tenant_id, collected_at, org_id, subscription_id, header_timestamp, encoding,
			 aws_account_id, cluster_id, cluster_name, region, group_id, group_size,
			 host_name, agent_version, envelope_tags,
			 arn, resource_version, launch_type, desired_status, known_status,
			 family, version, availability_zone, service_name, vpc_id,
			 container_instance_arn, daemon_name, task_host_name,
			 limits, ephemeral_storage_metrics,
			 pull_started_at, pull_stopped_at, execution_stopped_at,
			 containers, container_count,
			 tags, ecs_tags, container_instance_tags)`,
		// Same shape of traffic as k8s_resources — whole collection passes in
		// a burst, then silence — at a fraction of the volume, since a cluster
		// has far fewer tasks than a Kubernetes cluster has objects. Same
		// batching, a tenth of the buffer.
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 2,
	}, ECSTaskRow{})

	// NEW types only: the thirteen original ones keep their place in
	// register.go's frozen list, and Ergo hands out wire identities in
	// registration order.
	registerTypes(WriteECSTasks{}, ECSTaskRow{})
}
