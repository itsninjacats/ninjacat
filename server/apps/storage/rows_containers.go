package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// container_events and container_images — what the runtime did, and what it
// is holding. Both arrive on the event-platform intakes
// (contlcycle-intake / contimage-intake), which is why they share a file.

type WriteContainerEvents struct{ Events []ContainerEventRow }
type WriteContainerImages struct{ Images []ContainerImageRow }

func (m WriteContainerEvents) rows() []Row { return toRows(m.Events) }
func (m WriteContainerImages) rows() []Row { return toRows(m.Images) }

// ContainerEventRow is one lifecycle event in ninjacat.container_events —
// the restart/OOM history.
//
// POINTERS everywhere the wire uses a proto3 optional, because a zero value
// would erase a distinction the sender took care to make: exit 0 and "no code
// reported" are different states (see lcExitCode on the intake side), a
// missing timestamp must stay missing rather than become 1970, and a
// termination reason that was never set is not the empty string. nil travels
// to the table's Nullable columns as NULL.
//
// The state transition is stored TWICE, deliberately. OldState/NewState are
// the flattened strings the log line prints, kept because a dashboard wants a
// label and should not have to reassemble one; the structured fields beside
// them are what "every OOM kill with signal 9" needs. Both come from the same
// wire message, so they cannot disagree.
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

	// PayloadVersion is EventsPayload.Version — the schema version of the
	// batch, not of any event in it.
	PayloadVersion string

	// EventVariant names the arm of the Event oneof this row came from:
	// "container", "pod", "task", or "unknown" for a nil oneof or a variant
	// newer than our schema. Without it, an unknown-variant row looks exactly
	// like a typed event whose fields all happened to be empty.
	EventVariant string

	// Container transitions.
	ContainerKind      string
	Precision          string
	MissedIntermediate string

	OldStateKind string
	OldReason    *string
	OldExitCode  *int32
	OldSignal    *int32
	NewStateKind string
	NewReason    *string
	NewExitCode  *int32
	NewSignal    *int32

	// Pod transitions. PodStatusField says which status field moved; the
	// value itself is a oneof, so each arm gets its own columns and
	// *StateVariant records which arm was populated.
	PodStatusField string

	OldStateVariant     string
	OldPhase            *string
	OldConditionType    *string
	OldConditionStatus  *string
	OldConditionReason  *string
	OldConditionMessage *string

	NewStateVariant     string
	NewPhase            *string
	NewConditionType    *string
	NewConditionStatus  *string
	NewConditionReason  *string
	NewConditionMessage *string

	// ContainerNamePresent is 1 when the sender set ContainerName at all.
	// The name itself stays in the non-nullable ContainerName column — that
	// column cannot become Nullable without rewriting every stored row — so
	// presence travels beside it. See 0015_k8s_fidelity.sql.
	ContainerNamePresent uint8
}

func (r ContainerEventRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.ClusterID,
		r.ObjectKind, r.EventType, r.ContainerID, r.ContainerName,
		r.PodUID, r.TaskARN, r.Source,
		r.ExitCode, r.CreatedAt, r.ExitedAt,
		r.OwnerType, r.OwnerUID, r.OldState, r.NewState, r.TransitionAt,
		r.PayloadVersion, r.EventVariant,
		r.ContainerKind, r.Precision, r.MissedIntermediate,
		r.OldStateKind, r.OldReason, r.OldExitCode, r.OldSignal,
		r.NewStateKind, r.NewReason, r.NewExitCode, r.NewSignal,
		r.PodStatusField,
		r.OldStateVariant, r.OldPhase, r.OldConditionType, r.OldConditionStatus,
		r.OldConditionReason, r.OldConditionMessage,
		r.NewStateVariant, r.NewPhase, r.NewConditionType, r.NewConditionStatus,
		r.NewConditionReason, r.NewConditionMessage,
		r.ContainerNamePresent)
}

// ContainerImageRow is one image sighting in ninjacat.container_images.
//
// The table replaces on (tenant, digest, host): the digest identifies the
// immutable content, the host says where it sits, so Host is part of the
// row's identity, not just context. BuiltAt and PublishedAt are pointers for
// the same reason as ContainerEventRow's timestamps — the agent often has
// neither, and absence must survive the trip (nil -> NULL).
//
// The Layer* slices are PARALLEL and index-aligned: LayerDigests[i] belongs
// with LayerSizes[i] and LayerHistoryCreatedBy[i]. Order is data — an image
// IS its ordered layer stack — so nothing here is sorted or deduplicated.
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

	// PayloadVersion is ContainerImagePayload.Version; Source is its
	// proto3-optional Source ("agent" | "other"), nil when the sender left it
	// out — which pipeline produced an inventory is identity, not decoration.
	PayloadVersion string
	Source         *string

	LayerMediaTypes []string
	LayerDigests    []string
	LayerSizes      []int64
	// LayerURLs holds the alternative locations of each layer. The inner
	// slices must never be nil (the driver rejects a nil inside an
	// Array(Array(...))), which is why the intake fills an empty slice per
	// layer rather than leaving holes.
	LayerURLs [][]string

	// The build history of each layer: the Dockerfile, essentially.
	LayerHistoryCreated    []*time.Time
	LayerHistoryCreatedBy  []string
	LayerHistoryAuthor     []string
	LayerHistoryComment    []string
	LayerHistoryEmptyLayer []uint8

	// SizeNegative is 1 when the wire's signed size was below zero and was
	// clamped into SizeBytes. Casting a negative int64 to uint64 produces
	// ~18 exabytes, which then poisons every SUM over the table — so the
	// clamp is recorded rather than hidden.
	SizeNegative uint8

	// LayerSizeNegative is 1 when ANY single layer reported a negative size.
	// LayerBytes is their sum cast to uint64, so one negative layer either
	// wraps the total or quietly shrinks it; the sum is clamped at 0 and this
	// flag says the total cannot be trusted. LayerSizes keeps the raw values.
	LayerSizeNegative uint8
}

func (r ContainerImageRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.CollectedAt, r.Host,
		r.ImageKey, r.IdentitySource, r.ImageID, r.Digest,
		r.Name, r.ShortName, r.Registry,
		orEmptySlice(r.RepoTags), orEmptySlice(r.RepoDigests),
		r.SizeBytes, r.OSName, r.OSVersion, r.Architecture,
		r.LayerCount, r.LayerBytes, r.BuiltAt, r.PublishedAt,
		orEmpty(r.DDTags),
		r.PayloadVersion, r.Source,
		orEmptySlice(r.LayerMediaTypes), orEmptySlice(r.LayerDigests),
		orEmptySlice(r.LayerSizes), orEmptySlice(r.LayerURLs),
		orEmptySlice(r.LayerHistoryCreated), orEmptySlice(r.LayerHistoryCreatedBy),
		orEmptySlice(r.LayerHistoryAuthor), orEmptySlice(r.LayerHistoryComment),
		orEmptySlice(r.LayerHistoryEmptyLayer),
		r.SizeNegative, r.LayerSizeNegative)
}

func init() {
	registerWriter(ContainerEventsWriter, WriterConfig{
		Name: "container_events",
		Insert: `INSERT INTO container_events
			(tenant_id, timestamp, host, cluster_id, object_kind, event_type,
			 container_id, container_name, pod_uid, task_arn, source,
			 exit_code, created_at, exited_at, owner_type, owner_uid,
			 old_state, new_state, transition_at,
			 payload_version, event_variant,
			 container_kind, precision, missed_intermediate,
			 old_state_kind, old_reason, old_exit_code, old_signal,
			 new_state_kind, new_reason, new_exit_code, new_signal,
			 pod_status_field,
			 old_state_variant, old_phase, old_condition_type, old_condition_status,
			 old_condition_reason, old_condition_message,
			 new_state_variant, new_phase, new_condition_type, new_condition_status,
			 new_condition_reason, new_condition_message,
			 container_name_present)`,
		// Every node agent reports every restart and OOM in the fleet, and a
		// bad rollout turns that into a storm — the second in-flight flush is
		// for exactly that day. This is also the table that must not drop
		// rows lightly (each one is a crash somebody will look for), so the
		// buffer is the deepest of the new set.
		MaxRows: 1000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, ContainerEventRow{})

	registerWriter(ContainerImagesWriter, WriterConfig{
		Name: "container_images",
		Insert: `INSERT INTO container_images
			(tenant_id, collected_at, host, image_key, identity_source,
			 image_id, digest, name, short_name,
			 registry, repo_tags, repo_digests, size_bytes, os_name, os_version,
			 architecture, layer_count, layer_bytes, built_at, published_at, dd_tags,
			 payload_version, source,
			 layer_media_types, layer_digests, layer_sizes, layer_urls,
			 layer_history_created, layer_history_created_by, layer_history_author,
			 layer_history_comment, layer_history_empty_layer,
			 size_negative, layer_size_negative)`,
		// Periodic full inventories: every node re-announces every image it
		// holds, so arrivals are bursty and repetitive. ReplacingMergeTree
		// absorbs the repetition; here we just batch the bursts, with a second
		// flush in flight for when many nodes report at once.
		MaxRows: 500, FlushInterval: 10 * time.Second,
		BufferLimit: 20_000, MaxInFlight: 2,
	}, ContainerImageRow{})
}
