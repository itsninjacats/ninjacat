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

func init() {
	registerWriter(ContainerEventsWriter, WriterConfig{
		Name: "container_events",
		Insert: `INSERT INTO container_events
			(tenant_id, timestamp, host, cluster_id, object_kind, event_type,
			 container_id, container_name, pod_uid, task_arn, source,
			 exit_code, created_at, exited_at, owner_type, owner_uid,
			 old_state, new_state, transition_at)`,
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
			 architecture, layer_count, layer_bytes, built_at, published_at, dd_tags)`,
		// Periodic full inventories: every node re-announces every image it
		// holds, so arrivals are bursty and repetitive. ReplacingMergeTree
		// absorbs the repetition; here we just batch the bursts, with a second
		// flush in flight for when many nodes report at once.
		MaxRows: 500, FlushInterval: 10 * time.Second,
		BufferLimit: 20_000, MaxInFlight: 2,
	}, ContainerImageRow{})
}
