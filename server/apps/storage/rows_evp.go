package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/google/uuid"
)

// EVP misc: eight tables fed by the six agentdiscovery-intake/agenthealth-
// intake/event-management-intake/softinv-intake/http-synthetics/
// data-obs-intake hosts (server/intake/router_evp.go). None of these share a
// wire shape, so this file is one row type per table rather than one
// reusable pattern — see schema/migrations/0010_evp.sql for the column
// rationale and docs/tables/evp.md for the source mapping.

// ---------------------------------------------------------------------------
// agent_discovery — /api/v2/agentdiscovery

const AgentDiscoveryWriter = gen.Atom("storage_agent_discovery")

type WriteAgentDiscovery struct{ Rows []AgentDiscoveryRow }

func (m WriteAgentDiscovery) rows() []Row { return toRows(m.Rows) }

// AgentDiscoveryRow is one AgentDiscoveryPayload out of a
// AgentDiscoveryPayloadBatch — HostId repeats onto every payload's row since
// the batch itself has no other identity worth a separate table.
//
// EnvVarNames/EnvVarValues keep VALUES, credential-shaped as they are — see
// the migration's header comment for why that is a deliberate choice, not an
// oversight. They are two parallel arrays, not a map: EnvVars is a repeated
// field on the wire, so a real agent CAN repeat a name, and a map would
// silently collapse that to the last value and lose the original order.
type AgentDiscoveryRow struct {
	TenantID   string
	ReceivedAt time.Time

	HostID             string
	Integration        string
	Runtime            string
	RuntimeID          string
	IngestionTimestamp *time.Time

	// Parallel arrays, index-aligned with each other.
	ConfigPaths     []string
	ConfigContents  []string
	ConfigTruncated []uint8
	ConfigFormats   []string

	// Parallel arrays, index-aligned with each other: EnvVarNames[i] pairs
	// with EnvVarValues[i]. See the type comment above for why not a map.
	EnvVarNames  []string
	EnvVarValues []string
}

func (r AgentDiscoveryRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt,
		r.HostID, r.Integration, r.Runtime, r.RuntimeID, r.IngestionTimestamp,
		orEmptySlice(r.ConfigPaths), orEmptySlice(r.ConfigContents),
		orEmptySlice(r.ConfigTruncated), orEmptySlice(r.ConfigFormats),
		orEmptySlice(r.EnvVarNames), orEmptySlice(r.EnvVarValues))
}

func init() {
	registerWriter(AgentDiscoveryWriter, WriterConfig{
		Name: "agent_discovery",
		Insert: `INSERT INTO agent_discovery
			(tenant_id, received_at, host_id, integration, runtime, runtime_id,
			 ingestion_timestamp, config_paths, config_contents, config_truncated,
			 config_formats, env_var_names, env_var_values)`,
		// Bulky-document class, like k8s_manifests/raw_payloads: a row can
		// carry whole config files, content included, so counts here stand
		// for a document's worth of bytes rather than a metric point.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, AgentDiscoveryRow{})
	registerTypes(WriteAgentDiscovery{}, AgentDiscoveryRow{})
}

// ---------------------------------------------------------------------------
// agent_health_reports / agent_health_issues — /api/v2/agenthealth

const AgentHealthReportsWriter = gen.Atom("storage_agent_health_reports")
const AgentHealthIssuesWriter = gen.Atom("storage_agent_health_issues")

type WriteAgentHealthReports struct{ Rows []AgentHealthReportRow }
type WriteAgentHealthIssues struct{ Rows []AgentHealthIssueRow }

func (m WriteAgentHealthReports) rows() []Row { return toRows(m.Rows) }
func (m WriteAgentHealthIssues) rows() []Row  { return toRows(m.Rows) }

// AgentHealthReportRow is one HealthReport. ReportID is generated at ingest
// (see router_evp.go) since the wire carries no report-level id, only a map
// of per-issue ids — it is the join key into agent_health_issues.
type AgentHealthReportRow struct {
	TenantID   string
	ReceivedAt time.Time

	ReportID        uuid.UUID
	SchemaVersion   string
	EventType       string
	EmittedAt       string
	EmittedAtParsed *time.Time
	Service         string
	Host            string
	// ptr-optional on the wire (HostInfo.AgentVersion *string).
	AgentVersion *string
	ParIDs       []string
	IssueCount   uint32
}

func (r AgentHealthReportRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt,
		r.ReportID, r.SchemaVersion, r.EventType, r.EmittedAt, r.EmittedAtParsed,
		r.Service, r.Host, r.AgentVersion, orEmptySlice(r.ParIDs), r.IssueCount)
}

// AgentHealthIssueRow is one entry of a HealthReport's Issues map. IssueKey
// is sorted into deterministic order before rows are built — see router_evp.go
// — because Go map iteration order is random and two ingests of the same
// report must produce the same row order.
type AgentHealthIssueRow struct {
	TenantID   string
	ReceivedAt time.Time

	ReportID    uuid.UUID
	IssueKey    string
	ID          string
	IssueName   string
	Title       string
	Description string
	Category    string
	Location    string
	Severity    string // Issue.Severity enum name

	DetectedAt       string
	DetectedAtParsed *time.Time
	Source           string
	// structpb.Struct rendered as JSON — genuinely schemaless per issue.
	Extra string

	RemediationSummary    string
	RemediationStepOrder  []int32
	RemediationStepText   []string
	ScriptLanguage        string
	ScriptLanguageVersion string
	ScriptFilename        string
	// ptr-optional: nil means "no script at all", distinct from a script that
	// explicitly reports requires_root=false — both would be indistinguishable
	// 0s otherwise.
	ScriptRequiresRoot *uint8
	ScriptContent      string

	Tags map[string][]string

	PersistedState string // PersistedIssue.State enum name
	FirstSeen      string
	LastSeen       string
	// ptr-optional on the wire (PersistedIssue.ResolvedAt *string).
	ResolvedAt *string
	IssueType  string
}

func (r AgentHealthIssueRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt,
		r.ReportID, r.IssueKey, r.ID, r.IssueName, r.Title, r.Description,
		r.Category, r.Location, r.Severity, r.DetectedAt, r.DetectedAtParsed,
		r.Source, r.Extra,
		r.RemediationSummary, orEmptySlice(r.RemediationStepOrder), orEmptySlice(r.RemediationStepText),
		r.ScriptLanguage, r.ScriptLanguageVersion, r.ScriptFilename, r.ScriptRequiresRoot, r.ScriptContent,
		orEmpty(r.Tags),
		r.PersistedState, r.FirstSeen, r.LastSeen, r.ResolvedAt, r.IssueType)
}

func init() {
	registerWriter(AgentHealthReportsWriter, WriterConfig{
		Name: "agent_health_reports",
		Insert: `INSERT INTO agent_health_reports
			(tenant_id, received_at, report_id, schema_version, event_type,
			 emitted_at, emitted_at_parsed, service, host, agent_version,
			 par_ids, issue_count)`,
		// Human-scale audit data, like events/k8s_actions: one report per
		// agent per health-check interval, not a per-second signal.
		MaxRows: 200, FlushInterval: 5 * time.Second,
	}, AgentHealthReportRow{})
	registerTypes(WriteAgentHealthReports{}, AgentHealthReportRow{})

	registerWriter(AgentHealthIssuesWriter, WriterConfig{
		Name: "agent_health_issues",
		Insert: `INSERT INTO agent_health_issues
			(tenant_id, received_at, report_id, issue_key, id, issue_name,
			 title, description, category, location, severity, detected_at,
			 detected_at_parsed, source, extra, remediation_summary,
			 remediation_step_order, remediation_step_text, script_language,
			 script_language_version, script_filename, script_requires_root,
			 script_content, tags, persisted_state, first_seen, last_seen,
			 resolved_at, issue_type)`,
		// Same class as its parent report, but several issues per report —
		// low-frequency status, like check_runs: trickles in, small
		// threshold, longer timer.
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, AgentHealthIssueRow{})
	registerTypes(WriteAgentHealthIssues{}, AgentHealthIssueRow{})
}

// ---------------------------------------------------------------------------
// event_management_events — /api/v2/events

const EventManagementWriter = gen.Atom("storage_event_management_events")

type WriteEventManagementEvents struct{ Rows []EventManagementEventRow }

func (m WriteEventManagementEvents) rows() []Row { return toRows(m.Rows) }

// EventManagementEventRow is one JSON:API-shaped envelope from
// /api/v2/events. Attributes is the INNER data.attributes.attributes object,
// kept nested and separate from OuterExtra — see the migration comment.
type EventManagementEventRow struct {
	TenantID   string
	ReceivedAt time.Time

	DataType        string
	Host            string
	Title           string
	Category        string
	IntegrationID   string
	Message         string
	Timestamp       string
	TimestampParsed *time.Time

	Tags             map[string][]string
	AggregationKey   string
	NotableEventType string
	// Inner data.attributes.attributes, JSON, kept nested.
	Attributes string
	OuterExtra map[string]string
}

func (r EventManagementEventRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt,
		r.DataType, r.Host, r.Title, r.Category, r.IntegrationID, r.Message,
		r.Timestamp, r.TimestampParsed,
		orEmpty(r.Tags), r.AggregationKey, r.NotableEventType, r.Attributes,
		orEmpty(r.OuterExtra))
}

func init() {
	registerWriter(EventManagementWriter, WriterConfig{
		Name: "event_management_events",
		Insert: `INSERT INTO event_management_events
			(tenant_id, received_at, data_type, host, title, category,
			 integration_id, message, timestamp, timestamp_parsed, tags,
			 aggregation_key, notable_event_type, attributes, outer_extra)`,
		// Human-scale events, like the core events table: tens or hundreds
		// a day, short-ish flush so a notable event shows up promptly.
		MaxRows: 200, FlushInterval: 2 * time.Second,
	}, EventManagementEventRow{})
	registerTypes(WriteEventManagementEvents{}, EventManagementEventRow{})
}

// ---------------------------------------------------------------------------
// host_software — /api/v2/softinv

const HostSoftwareWriter = gen.Atom("storage_host_software")

type WriteHostSoftware struct{ Rows []HostSoftwareRow }

func (m WriteHostSoftware) rows() []Row { return toRows(m.Rows) }

// HostSoftwareRow is one software entry out of a softinv payload's
// host_software.software array — not one row per payload, since a full
// inventory runs to hundreds of entries and the point of this table is a
// query like "every host running curl < 8.0".
type HostSoftwareRow struct {
	TenantID   string
	ReceivedAt time.Time

	Hostname             string
	SoftwareType         string
	Name                 string
	Version              string
	Publisher            string
	DeploymentStatus     string
	DeploymentTime       string
	DeploymentTimeParsed *time.Time
	ProductCode          string
	// Absent (unknown bitness) is common and real — Nullable rather than a
	// false default, which would claim 32-bit for software whose
	// architecture the agent never determined.
	Is64Bit      *uint8
	InstallPaths []string
	// Undeclared keys on the software entry itself, JSON per key.
	Extra map[string]string
	// Undeclared keys at the payload level, repeated onto every row the
	// payload produced.
	PayloadExtra map[string]string
}

func (r HostSoftwareRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt,
		r.Hostname, r.SoftwareType, r.Name, r.Version, r.Publisher,
		r.DeploymentStatus, r.DeploymentTime, r.DeploymentTimeParsed, r.ProductCode,
		r.Is64Bit, orEmptySlice(r.InstallPaths),
		orEmpty(r.Extra), orEmpty(r.PayloadExtra))
}

func init() {
	registerWriter(HostSoftwareWriter, WriterConfig{
		Name: "host_software",
		Insert: `INSERT INTO host_software
			(tenant_id, received_at, hostname, software_type, name, version,
			 publisher, deployment_status, deployment_time,
			 deployment_time_parsed, product_code, is_64_bit, install_paths,
			 extra, payload_extra)`,
		// Bulky periodic snapshot, like processes: a few hundred rows per
		// host per inventory pass. Large batches, moderate interval.
		MaxRows: 10_000, FlushInterval: 3 * time.Second,
		BufferLimit: 100_000, MaxInFlight: 2,
	}, HostSoftwareRow{})
	registerTypes(WriteHostSoftware{}, HostSoftwareRow{})
}

// ---------------------------------------------------------------------------
// synthetics_results — /api/v2/synthetics

const SyntheticsResultsWriter = gen.Atom("storage_synthetics_results")

type WriteSyntheticsResults struct{ Rows []SyntheticsResultRow }

func (m WriteSyntheticsResults) rows() []Row { return toRows(m.Rows) }

// SyntheticsResultRow is one common.TestResult entry. Assertions travel as
// five parallel arrays — order and count both matter, which is exactly what
// the intake's own tally-only logging used to lose.
type SyntheticsResultRow struct {
	TenantID   string
	ReceivedAt time.Time

	TestID      string
	TestName    string
	TestType    string
	TestSubtype string
	TestVersion string

	LocationID          string
	LocationName        string
	LocationDisplayName string

	ResultID        string
	ResultInitialID string
	Status          string
	RunType         string
	// Wire type is undocumented; kept as sent rather than guessed at.
	Duration string

	TestStartedAt         string
	TestStartedAtParsed   *time.Time
	TestFinishedAt        string
	TestFinishedAtParsed  *time.Time
	TestTriggeredAt       string
	TestTriggeredAtParsed *time.Time

	// Parallel arrays, index-aligned with each other.
	AssertionType     []string
	AssertionOperator []string
	AssertionExpected []string
	AssertionActual   []string
	AssertionValid    []uint8

	// failure's presence is itself a signal, so both are ptr-optional.
	FailureCode    *string
	FailureMessage *string

	Config string

	NetstatsPacketsSent          *int64
	NetstatsPacketsReceived      *int64
	NetstatsPacketLossPercentage *float64
	NetstatsJitter               *float64
	NetstatsLatency              *float64
	NetstatsHops                 *int64

	Netpath    string
	DD         string
	Enrichment string
	V          string
	Extra      map[string]string
}

func (r SyntheticsResultRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt,
		r.TestID, r.TestName, r.TestType, r.TestSubtype, r.TestVersion,
		r.LocationID, r.LocationName, r.LocationDisplayName,
		r.ResultID, r.ResultInitialID, r.Status, r.RunType, r.Duration,
		r.TestStartedAt, r.TestStartedAtParsed,
		r.TestFinishedAt, r.TestFinishedAtParsed,
		r.TestTriggeredAt, r.TestTriggeredAtParsed,
		orEmptySlice(r.AssertionType), orEmptySlice(r.AssertionOperator),
		orEmptySlice(r.AssertionExpected), orEmptySlice(r.AssertionActual),
		orEmptySlice(r.AssertionValid),
		r.FailureCode, r.FailureMessage,
		r.Config,
		r.NetstatsPacketsSent, r.NetstatsPacketsReceived, r.NetstatsPacketLossPercentage,
		r.NetstatsJitter, r.NetstatsLatency, r.NetstatsHops,
		r.Netpath, r.DD, r.Enrichment, r.V, orEmpty(r.Extra))
}

func init() {
	registerWriter(SyntheticsResultsWriter, WriterConfig{
		Name: "synthetics_results",
		Insert: `INSERT INTO synthetics_results
			(tenant_id, received_at, test_id, test_name, test_type, test_subtype,
			 test_version, location_id, location_name, location_display_name,
			 result_id, result_initial_id, status, run_type, duration,
			 test_started_at, test_started_at_parsed, test_finished_at,
			 test_finished_at_parsed, test_triggered_at, test_triggered_at_parsed,
			 assertion_type, assertion_operator, assertion_expected,
			 assertion_actual, assertion_valid, failure_code, failure_message,
			 config, netstats_packets_sent, netstats_packets_received,
			 netstats_packet_loss_percentage, netstats_jitter, netstats_latency,
			 netstats_hops, netpath, dd, enrichment, v, extra)`,
		// Low-frequency status, like check_runs: results trickle in on a
		// per-test schedule, not a hot path.
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, SyntheticsResultRow{})
	registerTypes(WriteSyntheticsResults{}, SyntheticsResultRow{})
}

// ---------------------------------------------------------------------------
// openlineage_events — /api/v1/lineage

const OpenLineageWriter = gen.Atom("storage_openlineage_events")

type WriteOpenLineageEvents struct{ Rows []OpenLineageEventRow }

func (m WriteOpenLineageEvents) rows() []Row { return toRows(m.Rows) }

// OpenLineageEventRow is one OpenLineage RunEvent. Facets are OpenLineage's
// own extension mechanism — genuinely open-ended by spec — so they stay JSON
// text rather than columns.
type OpenLineageEventRow struct {
	TenantID   string
	ReceivedAt time.Time

	EventType       string
	EventTime       string
	EventTimeParsed *time.Time
	Producer        string
	SchemaURL       string

	RunID        string
	RunFacets    string
	JobNamespace string
	JobName      string
	JobFacets    string

	// Parallel arrays per list: InputNamespace[i]/InputName[i]/
	// InputFacets[i] describe input dataset i; outputs are independent.
	InputNamespace  []string
	InputName       []string
	InputFacets     []string
	OutputNamespace []string
	OutputName      []string
	OutputFacets    []string

	APIVersion string
	Via        string
	Extra      map[string]string
}

func (r OpenLineageEventRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt,
		r.EventType, r.EventTime, r.EventTimeParsed, r.Producer, r.SchemaURL,
		r.RunID, r.RunFacets, r.JobNamespace, r.JobName, r.JobFacets,
		orEmptySlice(r.InputNamespace), orEmptySlice(r.InputName), orEmptySlice(r.InputFacets),
		orEmptySlice(r.OutputNamespace), orEmptySlice(r.OutputName), orEmptySlice(r.OutputFacets),
		r.APIVersion, r.Via, orEmpty(r.Extra))
}

func init() {
	registerWriter(OpenLineageWriter, WriterConfig{
		Name: "openlineage_events",
		Insert: `INSERT INTO openlineage_events
			(tenant_id, received_at, event_type, event_time, event_time_parsed,
			 producer, schema_url, run_id, run_facets, job_namespace, job_name,
			 job_facets, input_namespace, input_name, input_facets,
			 output_namespace, output_name, output_facets, api_version, via,
			 extra)`,
		// Low-frequency status, like check_runs: one event per pipeline run
		// state transition, not a per-second signal.
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, OpenLineageEventRow{})
	registerTypes(WriteOpenLineageEvents{}, OpenLineageEventRow{})
}

// ---------------------------------------------------------------------------
// query_action_results — /api/v2/query-actions

const QueryActionResultsWriter = gen.Atom("storage_query_action_results")

type WriteQueryActionResults struct{ Rows []QueryActionResultRow }

func (m WriteQueryActionResults) rows() []Row { return toRows(m.Rows) }

// QueryActionResultRow is one entry of a query-actions batch — the one track
// in this file with zero field names asserted anywhere upstream, so Result
// is the entry verbatim as JSON and Keys names its top-level fields.
type QueryActionResultRow struct {
	TenantID   string
	ReceivedAt time.Time

	Result             string
	Keys               []string
	DDEVPOrigin        string
	DDEVPOriginVersion string
}

func (r QueryActionResultRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt,
		r.Result, orEmptySlice(r.Keys), r.DDEVPOrigin, r.DDEVPOriginVersion)
}

func init() {
	registerWriter(QueryActionResultsWriter, WriterConfig{
		Name: "query_action_results",
		Insert: `INSERT INTO query_action_results
			(tenant_id, received_at, result, keys, dd_evp_origin,
			 dd_evp_origin_version)`,
		// Human-scale, like events: query actions run on demand, not a
		// per-second signal.
		MaxRows: 200, FlushInterval: 2 * time.Second,
	}, QueryActionResultRow{})
	registerTypes(WriteQueryActionResults{}, QueryActionResultRow{})
}
