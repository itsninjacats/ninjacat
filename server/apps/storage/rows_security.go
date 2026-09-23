package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/google/uuid"
)

// cws_activity_dumps, cws_dump_nodes, security_events, sbom_entities,
// sbom_components, sbom_vulnerabilities — the five security intake hosts
// (intake/router_security.go): CWS activity dumps, CWS runtime/remediation
// events, CSPM compliance checks, and container SBOMs. sdsresult has no
// published Go type yet and goes through raw_payloads (rows_raw.go)
// instead — nothing of its own lives here.

// ---------------------------------------------------------------------------
// cws_activity_dumps / cws_dump_nodes — POST /api/v2/secdump
// ---------------------------------------------------------------------------

const (
	CWSActivityDumpsWriter = gen.Atom("storage_cws_activity_dumps")
	CWSDumpNodesWriter     = gen.Atom("storage_cws_dump_nodes")
)

type WriteCWSActivityDumps struct{ Dumps []CWSActivityDumpRow }
type WriteCWSDumpNodes struct{ Nodes []CWSDumpNodeRow }

func (m WriteCWSActivityDumps) rows() []Row { return toRows(m.Dumps) }
func (m WriteCWSDumpNodes) rows() []Row     { return toRows(m.Nodes) }

// CWSActivityDumpRow is one secdump request: the "event" header part, the
// "dump" protobuf part's envelope and Metadata, and the raw dump bytes.
//
// Start/End/Size are pointers because they are only meaningful when Metadata
// itself was present — a nil Metadata must not read as "all counters were
// 0", which a real dump can legitimately report (kernel-boot-relative
// counters start at 0).
type CWSActivityDumpRow struct {
	TenantID   string
	ReceivedAt time.Time
	DumpID     uuid.UUID

	HeaderHost    string
	HeaderService string
	HeaderSource  string
	HeaderTags    map[string][]string
	DNSNames      string
	HeaderExtra   map[string]string

	// HeaderRaw is the "event" part's raw bytes, exactly as received,
	// regardless of whether it parsed as a JSON object — the same
	// unconditional-capture treatment Dump below gets for the "dump" part,
	// so a malformed "event" part never loses its bytes even when the
	// "dump" part decoded fine and the request otherwise looks fine too.
	HeaderRaw string

	DumpHost    string
	DumpService string
	DumpSource  string
	DumpTags    map[string][]string

	AgentVersion      string
	AgentCommit       string
	KernelVersion     string
	LinuxDistribution string
	Arch              string
	MetadataName      string
	ProtobufVersion   string
	DifferentiateArgs uint8
	Comm              string
	ContainerID       string
	Start             *uint64
	End               *uint64
	Size              *uint64
	Serialization     string
	CgroupID          string
	CgroupManager     string

	TreeNodeCount uint32

	// Dump is the "dump" part's raw protobuf bytes, exactly as received —
	// see the migration's comment on why this must stay lossless.
	Dump string
}

func (r CWSActivityDumpRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.DumpID,
		r.HeaderHost, r.HeaderService, r.HeaderSource, orEmpty(r.HeaderTags),
		r.DNSNames, orEmpty(r.HeaderExtra), r.HeaderRaw,
		r.DumpHost, r.DumpService, r.DumpSource, orEmpty(r.DumpTags),
		r.AgentVersion, r.AgentCommit, r.KernelVersion, r.LinuxDistribution,
		r.Arch, r.MetadataName, r.ProtobufVersion, r.DifferentiateArgs,
		r.Comm, r.ContainerID, r.Start, r.End, r.Size,
		r.Serialization, r.CgroupID, r.CgroupManager,
		r.TreeNodeCount, r.Dump)
}

// CWSDumpNodeRow is one ProcessActivityNode, flattened depth-first out of
// the dump's Tree. NodePath is the index path from the root and is enough on
// its own to reconstruct the tree; ParentPath and Depth save a caller from
// recomputing them from NodePath.
type CWSDumpNodeRow struct {
	TenantID   string
	ReceivedAt time.Time
	DumpID     uuid.UUID

	NodePath   []uint32
	Depth      uint16
	ParentPath []uint32

	// Node is protojson of this node with Children cleared — see the
	// migration's comment on why Children is dropped here.
	Node string

	PID            uint32
	PPID           uint32
	Comm           string
	ContainerID    string
	FilePath       string
	Args           []string
	ImageTags      []string
	MatchedRuleIDs []string
	GenerationType string

	FilesCount    uint32
	DNSCount      uint32
	SocketsCount  uint32
	SyscallsCount uint32
}

func (r CWSDumpNodeRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.DumpID,
		orEmptySlice(r.NodePath), r.Depth, orEmptySlice(r.ParentPath), r.Node,
		r.PID, r.PPID, r.Comm, r.ContainerID, r.FilePath,
		orEmptySlice(r.Args), orEmptySlice(r.ImageTags), orEmptySlice(r.MatchedRuleIDs),
		r.GenerationType,
		r.FilesCount, r.DNSCount, r.SocketsCount, r.SyscallsCount)
}

// ---------------------------------------------------------------------------
// security_events — POST /api/v2/secruntime, /api/v2/secinfo, /api/v2/compliance
// ---------------------------------------------------------------------------

const SecurityEventsWriter = gen.Atom("storage_security_events")

type WriteSecurityEvents struct{ Events []SecurityEventRow }

func (m WriteSecurityEvents) rows() []Row { return toRows(m.Events) }

// SecurityEventRow is one logs-pipeline envelope from secruntime, secinfo or
// compliance, paired with its decoded inner "message" event when that
// decoded. MessageDecoded distinguishes "the inner event decoded to an empty
// object" from "the inner event did not decode at all" — both leave Message
// looking similar (empty-ish) without it.
type SecurityEventRow struct {
	TenantID       string
	ReceivedAt     time.Time
	Track          string
	SeqInBatch     uint32
	Timestamp      *time.Time
	Hostname       string
	Service        string
	DDSource       string
	Status         string
	DDTags         map[string][]string
	Extra          map[string]string
	MessageRaw     string
	Message        string
	MessageDecoded uint8

	RuleID      string
	PolicyName  string
	EvtName     string
	EvtCategory string
	Title       string
	EventKind   string
}

func (r SecurityEventRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Track, r.SeqInBatch,
		r.Timestamp, r.Hostname, r.Service, r.DDSource, r.Status,
		orEmpty(r.DDTags), orEmpty(r.Extra),
		r.MessageRaw, r.Message, r.MessageDecoded,
		r.RuleID, r.PolicyName, r.EvtName, r.EvtCategory, r.Title, r.EventKind)
}

// ---------------------------------------------------------------------------
// sbom_entities / sbom_components / sbom_vulnerabilities — POST /api/v2/sbom
// ---------------------------------------------------------------------------

const (
	SBOMEntitiesWriter        = gen.Atom("storage_sbom_entities")
	SBOMComponentsWriter      = gen.Atom("storage_sbom_components")
	SBOMVulnerabilitiesWriter = gen.Atom("storage_sbom_vulnerabilities")
)

type WriteSBOMEntities struct{ Entities []SBOMEntityRow }
type WriteSBOMComponents struct{ Components []SBOMComponentRow }
type WriteSBOMVulnerabilities struct{ Vulnerabilities []SBOMVulnerabilityRow }

func (m WriteSBOMEntities) rows() []Row        { return toRows(m.Entities) }
func (m WriteSBOMComponents) rows() []Row      { return toRows(m.Components) }
func (m WriteSBOMVulnerabilities) rows() []Row { return toRows(m.Vulnerabilities) }

// SBOMEntityRow is one SBOMEntity out of an SBOMPayload. EntityID is
// generated at write time — never taken from the wire's own Id, which is the
// sender's value and not guaranteed unique across payloads — and is what
// SBOMComponentRow/SBOMVulnerabilityRow.EntityID join against.
//
// Error and Bom are the two arms of the wire's oneof: exactly one is
// non-empty per row.
type SBOMEntityRow struct {
	TenantID   string
	ReceivedAt time.Time
	EntityID   uuid.UUID

	PayloadVersion int32
	Host           string
	Source         *string
	DdEnv          *string

	Type                 string
	ID                   string
	GeneratedAt          *time.Time
	RepoTags             []string
	RepoDigests          []string
	InUse                uint8
	GenerationDurationMs *int64
	DDTags               map[string][]string
	Heartbeat            uint8
	Hash                 string
	Status               string
	KernelVersion        string
	CPUArchitecture      string

	Error string
	Bom   string
	// BomRaw is the raw protobuf bytes of the CycloneDX message, base64,
	// populated only as a backstop when protojson.Marshal on Bom failed
	// (protojson requires valid UTF-8 string fields; proto.Marshal does
	// not) — empty whenever Bom itself is set, to avoid doubling storage
	// on the overwhelmingly common case where protojson just works.
	BomRaw string

	ComponentCount     uint32
	VulnerabilityCount uint32
}

func (r SBOMEntityRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.EntityID,
		r.PayloadVersion, r.Host, r.Source, r.DdEnv,
		r.Type, r.ID, r.GeneratedAt, orEmptySlice(r.RepoTags), orEmptySlice(r.RepoDigests),
		r.InUse, r.GenerationDurationMs, orEmpty(r.DDTags), r.Heartbeat, r.Hash,
		r.Status, r.KernelVersion, r.CPUArchitecture,
		r.Error, r.Bom, r.BomRaw,
		r.ComponentCount, r.VulnerabilityCount)
}

// SBOMComponentRow is one cyclonedx_v1_4.Component, flattened recursively —
// a component's own sub-components get their own rows, linked back by
// ParentBomRef.
type SBOMComponentRow struct {
	TenantID   string
	ReceivedAt time.Time
	EntityID   uuid.UUID

	BomRef       string
	ParentBomRef string
	Depth        uint16

	Type    string
	Name    string
	Version string
	// Purl/Cpe/Group/Publisher/Author/Description are all proto3-optional
	// on the wire (cyclonedx_v1_4.Component) — nil is "never set", distinct
	// from a present empty string, which CycloneDX explicitly allows (e.g.
	// version's own doc comment: "RECOMMENDED to use an empty string").
	Purl        *string
	Cpe         *string
	Group       *string
	Publisher   *string
	Author      *string
	Description *string
	Scope       string

	Licenses []string
	Hashes   map[string]string
	// Properties is a multiset (see sbomPropertiesMultiMap): CycloneDX
	// allows repeated same-named Property entries, and real scanners emit
	// them.
	Properties map[string][]string

	// ExternalReferences and Evidence are protojson arrays of the
	// corresponding repeated proto fields — see the migration's comment.
	ExternalReferences string
	Evidence           string
}

func (r SBOMComponentRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.EntityID,
		r.BomRef, r.ParentBomRef, r.Depth,
		r.Type, r.Name, r.Version, r.Purl, r.Cpe, r.Group,
		r.Publisher, r.Author, r.Description, r.Scope,
		orEmptySlice(r.Licenses), orEmpty(r.Hashes), orEmpty(r.Properties),
		r.ExternalReferences, r.Evidence)
}

// SBOMVulnerabilityRow is one cyclonedx_v1_4.Vulnerability out of an
// entity's BOM.
type SBOMVulnerabilityRow struct {
	TenantID   string
	ReceivedAt time.Time
	EntityID   uuid.UUID

	BomRef string
	ID     string
	// SourceName/SourceURL come from cyclonedx_v1_4.Source's own two
	// proto3-optional fields (Name/Url) — nil when Source was present but
	// that particular field was not set, distinct from Source being absent
	// entirely (both stay nil in that case too, which is the same "we have
	// nothing" reading either way).
	SourceName *string
	SourceURL  *string

	Ratings     string
	Cwes        []int32
	Description *string
	Detail      *string
	// Recommendation is proto3-optional too — see the Description note.
	Recommendation *string
	Advisories     string

	Created   *time.Time
	Published *time.Time
	Updated   *time.Time

	AnalysisState         string
	AnalysisJustification string
	AnalysisResponse      []string
	AnalysisDetail        string

	AffectsRefs []string
	Affects     string
	// Properties is a multiset — see SBOMComponentRow's field of the same
	// name.
	Properties map[string][]string
}

func (r SBOMVulnerabilityRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.EntityID,
		r.BomRef, r.ID, r.SourceName, r.SourceURL,
		r.Ratings, orEmptySlice(r.Cwes), r.Description, r.Detail, r.Recommendation, r.Advisories,
		r.Created, r.Published, r.Updated,
		r.AnalysisState, r.AnalysisJustification, orEmptySlice(r.AnalysisResponse), r.AnalysisDetail,
		orEmptySlice(r.AffectsRefs), r.Affects, orEmpty(r.Properties))
}

func init() {
	registerWriter(CWSActivityDumpsWriter, WriterConfig{
		Name: "cws_activity_dumps",
		Insert: `INSERT INTO cws_activity_dumps
			(tenant_id, received_at, dump_id,
			 header_host, header_service, header_source, header_tags, dns_names, header_extra, header_raw,
			 dump_host, dump_service, dump_source, dump_tags,
			 agent_version, agent_commit, kernel_version, linux_distribution,
			 arch, metadata_name, protobuf_version, differentiate_args,
			 comm, container_id, start_raw, end_raw, size_raw,
			 serialization, cgroup_id, cgroup_manager,
			 tree_node_count, dump)`,
		// Human-scale like events/k8s_actions, not machine-scale: a dump is
		// triggered per profiled workload, not per request. Small batches, a
		// timer that does most of the flushing.
		MaxRows: 100, FlushInterval: 10 * time.Second,
	}, CWSActivityDumpRow{})

	registerWriter(CWSDumpNodesWriter, WriterConfig{
		Name: "cws_dump_nodes",
		Insert: `INSERT INTO cws_dump_nodes
			(tenant_id, received_at, dump_id, node_path, depth, parent_path, node,
			 pid, ppid, comm, container_id, file_path, args, image_tags,
			 matched_rule_ids, generation_type,
			 files_count, dns_count, sockets_count, syscalls_count)`,
		// Bursty-per-pass like k8s_resources: a single dump can unpack into
		// thousands of nodes in one request, so this is sized like the
		// Kubernetes writers rather than like its own parent table.
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, CWSDumpNodeRow{})

	registerWriter(SecurityEventsWriter, WriterConfig{
		Name: "security_events",
		Insert: `INSERT INTO security_events
			(tenant_id, received_at, track, seq_in_batch, timestamp,
			 hostname, service, ddsource, status, ddtags, extra,
			 message_raw, message, message_decoded,
			 rule_id, policy_name, evt_name, evt_category, title, event_kind)`,
		// Logs-shaped traffic (a runtime-security event per relevant syscall,
		// a compliance check per rule per pass): the highest-volume table in
		// this file, so it copies logs' own numbers rather than events' small
		// human-scale ones.
		MaxRows: 20000, FlushInterval: 1 * time.Second,
		BufferLimit: 500_000,
	}, SecurityEventRow{})

	registerWriter(SBOMEntitiesWriter, WriterConfig{
		Name: "sbom_entities",
		Insert: `INSERT INTO sbom_entities
			(tenant_id, received_at, entity_id, payload_version, host, source, dd_env,
			 type, id, generated_at, repo_tags, repo_digests, in_use,
			 generation_duration_ms, dd_tags, heartbeat, hash, status,
			 kernel_version, cpu_architecture, error, bom, bom_raw,
			 component_count, vulnerability_count)`,
		// Periodic full inventories, like container_images: every node
		// re-announces the SBOMs it holds, so arrivals are bursty and
		// repetitive (heartbeats included).
		MaxRows: 500, FlushInterval: 10 * time.Second,
		BufferLimit: 20_000, MaxInFlight: 2,
	}, SBOMEntityRow{})

	registerWriter(SBOMComponentsWriter, WriterConfig{
		Name: "sbom_components",
		Insert: `INSERT INTO sbom_components
			(tenant_id, received_at, entity_id, bom_ref, parent_bom_ref, depth,
			 type, name, version, purl, cpe, ` + "`group`" + `, publisher, author,
			 description, scope, licenses, hashes, properties,
			 external_references, evidence)`,
		// Whole-document payloads, like k8s_manifests: one entity's BOM can
		// unpack into hundreds of components, so row counts here stand for a
		// document's worth of data rather than individually meaningful units.
		MaxRows: 2000, FlushInterval: 10 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, SBOMComponentRow{})

	registerWriter(SBOMVulnerabilitiesWriter, WriterConfig{
		Name: "sbom_vulnerabilities",
		Insert: `INSERT INTO sbom_vulnerabilities
			(tenant_id, received_at, entity_id, bom_ref, id, source_name, source_url,
			 ratings, cwes, description, detail, recommendation, advisories,
			 created, published, updated,
			 analysis_state, analysis_justification, analysis_response, analysis_detail,
			 affects_refs, affects, properties)`,
		MaxRows: 2000, FlushInterval: 10 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, SBOMVulnerabilityRow{})

	registerTypes(
		WriteCWSActivityDumps{}, CWSActivityDumpRow{},
		WriteCWSDumpNodes{}, CWSDumpNodeRow{},
		WriteSecurityEvents{}, SecurityEventRow{},
		WriteSBOMEntities{}, SBOMEntityRow{},
		WriteSBOMComponents{}, SBOMComponentRow{},
		WriteSBOMVulnerabilities{}, SBOMVulnerabilityRow{},
	)
}
