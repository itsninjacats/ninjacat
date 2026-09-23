package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// The tables the api.<site> intake needed and did not have.
//
// Everything here used to be decoded and logged: the sender identity on a
// sketch batch, the V5 collector's check list, the thirteen /api/v1/metadata
// variants, and the whole Private Action Runner loop. A log line is not a
// sink, so each of those got a table. See schema/migrations/0014_api_fidelity.sql
// for the columns and docs/tables/api.md for what feeds what.
//
// They share a file because they share an owner (router_api.go) and because
// registry.go's recipe is per-table, not per-file: each writer below declares
// its own atom, config and zero row, and nothing outside this file changes.
//
// NOTE ON EDF ORDER: registerTypes appends in filename order, and this file
// sorts before rows_raw.go, so it lands ahead of WriteRawPayloads in the tail
// that register.go's frozen list does not cover. Harmless while every role
// runs in one node and every node is deployed together — which is the case
// today — and worth knowing before the first split.

// Writer process names. The core writers' names live in application.go because
// two packages reference them; these are declared next to the tables they
// serve, which is where registry.go says a new table's registration belongs.
const (
	AgentBatchMetadataWriter = gen.Atom("storage_agent_batch_metadata")
	AgentChecksWriter        = gen.Atom("storage_agent_checks")
	ExternalHostTagsWriter   = gen.Atom("storage_external_host_tags")
	AgentMetadataWriter      = gen.Atom("storage_agent_metadata")
	DelegatedAuthWriter      = gen.Atom("storage_delegated_auth_requests")
	SymbolQueriesWriter      = gen.Atom("storage_symbol_queries")

	RunnerEnrollmentsWriter = gen.Atom("storage_runner_enrollments")
	RunnerTaskUpdatesWriter = gen.Atom("storage_runner_task_updates")
	RunnerHeartbeatsWriter  = gen.Atom("storage_runner_heartbeats")
	RunnerDequeuesWriter    = gen.Atom("storage_runner_dequeues")
	ActionConnectionsWriter = gen.Atom("storage_action_connections")
)

// orJSONObject guards a native JSON column: it rejects an empty string, and
// "nothing to store" is an empty object. Plain String columns holding JSON do
// NOT go through this — there, empty means the key was absent, which is a
// distinction worth keeping and impossible to recover later.
func orJSONObject(s string) string {
	if s == "" {
		return "{}"
	}
	return s
}

// ---------------------------------------------------------------------------
// agent_batch_metadata
// ---------------------------------------------------------------------------

type WriteAgentBatchMetadata struct{ Batches []AgentBatchMetadataRow }

func (m WriteAgentBatchMetadata) rows() []Row { return toRows(m.Batches) }

// AgentBatchMetadataRow is one sender-identity block: gogen.CommonMetadata off
// a sketch payload, minus its ApiKey field, which is deliberately not a column
// — the request already authenticated, and a credential does not belong in a
// table with a TTL.
type AgentBatchMetadataRow struct {
	TenantID     string
	ReceivedAt   time.Time
	Intake       string
	AgentVersion string
	Timezone     string
	// CurrentEpoch is the agent's own clock at send time, seconds. Against
	// ReceivedAt it is a clock-skew measurement for free.
	CurrentEpoch float64
	InternalIP   string
	PublicIP     string
}

func (r AgentBatchMetadataRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Intake, r.AgentVersion,
		r.Timezone, r.CurrentEpoch, r.InternalIP, r.PublicIP)
}

// ---------------------------------------------------------------------------
// agent_checks
// ---------------------------------------------------------------------------

type WriteAgentChecks struct{ Checks []AgentCheckRow }

func (m WriteAgentChecks) rows() []Row { return toRows(m.Checks) }

// AgentCheckRow is one entry of collectorimpl.Payload's agent_checks list.
//
// The wire shape is a POSITIONAL array with no key names at all — check name,
// source type, instance id, status, message — so the five documented positions
// become columns and anything the agent appends after them is kept as a JSON
// array. Status is a pointer because the array may be shorter than four
// elements and status 0 means OK: a non-nullable column would report every
// truncated entry as healthy.
type AgentCheckRow struct {
	TenantID     string
	ReceivedAt   time.Time
	Hostname     string
	AgentVersion string
	UUID         string

	CheckName  string
	SourceType string
	InstanceID string
	Status     *int64
	Message    string

	PositionalExtra string
	Meta            string
}

func (r AgentCheckRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Hostname, r.AgentVersion, r.UUID,
		r.CheckName, r.SourceType, r.InstanceID, r.Status, r.Message,
		r.PositionalExtra, r.Meta)
}

// ---------------------------------------------------------------------------
// external_host_tags
// ---------------------------------------------------------------------------

type WriteExternalHostTags struct{ Tags []ExternalHostTagsRow }

func (m WriteExternalHostTags) rows() []Row { return toRows(m.Tags) }

// ExternalHostTagsRow is one (host, source) pair of tags an agent reported for
// a host it is NOT running on — a vSphere collector tagging its VMs, a cloud
// integration tagging instances. They cannot go on the hosts row: the host
// they describe is not the host that sent them, and one host has several rows
// here at once, one per source.
type ExternalHostTagsRow struct {
	TenantID   string
	ReceivedAt time.Time
	Host       string
	Source     string
	Tags       map[string][]string
}

func (r ExternalHostTagsRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Host, r.Source, orEmpty(r.Tags))
}

// ---------------------------------------------------------------------------
// agent_metadata
// ---------------------------------------------------------------------------

type WriteAgentMetadata struct{ Payloads []AgentMetadataRow }

func (m WriteAgentMetadata) rows() []Row { return toRows(m.Payloads) }

// AgentMetadataRow is one variant of /api/v1/metadata: the shared envelope as
// columns and the variant's own body as JSON text under the key that named it.
//
// One table for all thirteen variants because none of their types is
// importable and each changes shape with the agent release. Thirteen tables of
// one column each would buy nothing and cost thirteen migrations a year.
type AgentMetadataRow struct {
	TenantID    string
	ReceivedAt  time.Time
	Variant     string
	Hostname    string
	ClusterName string
	ClusterID   string
	// Timestamp is a pointer: the envelope's field is optional and 1970 is not
	// an answer. ReceivedAt is always ours.
	Timestamp *time.Time
	UUID      string

	Payload       string
	EnvelopeExtra map[string]string
}

func (r AgentMetadataRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Variant, r.Hostname,
		r.ClusterName, r.ClusterID, r.Timestamp, r.UUID,
		r.Payload, orEmpty(r.EnvelopeExtra))
}

// ---------------------------------------------------------------------------
// delegated_auth_requests
// ---------------------------------------------------------------------------

type WriteDelegatedAuthRequests struct{ Requests []DelegatedAuthRow }

func (m WriteDelegatedAuthRequests) rows() []Row { return toRows(m.Requests) }

// DelegatedAuthRow is one /api/v2/intake-key exchange.
//
// ProofFingerprint is the SHA-256 of the delegated-auth proof, hex — never the
// proof. That is enough to tell two delegated identities apart and to key a
// real mapping on later, and useless to anyone who steals the table.
type DelegatedAuthRow struct {
	TenantID         string
	At               time.Time
	Scheme           string
	ProofFingerprint string
	APIKeyID         string
}

func (r DelegatedAuthRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.At, r.Scheme, r.ProofFingerprint, r.APIKeyID)
}

// ---------------------------------------------------------------------------
// symbol_queries
// ---------------------------------------------------------------------------

type WriteSymbolQueries struct{ Queries []SymbolQueryRow }

func (m WriteSymbolQueries) rows() []Row { return toRows(m.Queries) }

// SymbolQueryRow is one "which of these build ids do you already hold?" from
// the host profiler's symbol uploader. We answer "none" because nothing stores
// symbols yet; the build ids are still the profiled binaries of the fleet, and
// this is what a future /api/v2/srcmap handler will diff against. BuildIDs
// keeps its order — the uploader's reply is positional against it.
type SymbolQueryRow struct {
	TenantID string
	At       time.Time
	Arch     string
	BuildIDs []string
	Resource string
}

func (r SymbolQueryRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.At, r.Arch, orEmptySlice(r.BuildIDs), r.Resource)
}

// ---------------------------------------------------------------------------
// Private Action Runner
// ---------------------------------------------------------------------------

type WriteRunnerEnrollments struct{ Runners []RunnerEnrollmentRow }
type WriteRunnerTaskUpdates struct{ Updates []RunnerTaskUpdateRow }
type WriteRunnerHeartbeats struct{ Heartbeats []RunnerHeartbeatRow }
type WriteRunnerDequeues struct{ Dequeues []RunnerDequeueRow }
type WriteActionConnections struct{ Connections []ActionConnectionRow }

func (m WriteRunnerEnrollments) rows() []Row { return toRows(m.Runners) }
func (m WriteRunnerTaskUpdates) rows() []Row { return toRows(m.Updates) }
func (m WriteRunnerHeartbeats) rows() []Row  { return toRows(m.Heartbeats) }
func (m WriteRunnerDequeues) rows() []Row    { return toRows(m.Dequeues) }
func (m WriteActionConnections) rows() []Row { return toRows(m.Connections) }

// RunnerEnrollmentRow is one Private Action Runner identity.
//
// PublicKeyPEM is the whole PEM block, not the fingerprint the log line
// prints. It is public by construction, and it has to be kept: every OPMS
// request after enrollment is signed with the matching private key, and
// without this column nothing can ever verify one.
//
// The table is a ReplacingMergeTree on (tenant_id, runner_id): a runner that
// re-enrolls after a restart or a key rotation must not leave a second
// identity behind, because a verifier with two public keys has no rule for
// choosing between them.
type RunnerEnrollmentRow struct {
	TenantID   string
	RunnerID   string
	EnrolledAt time.Time

	Name          string
	Modes         []string
	Host          string
	PublicKeyPEM  string
	AgentHostname string
	OrchClusterID string
	AgentFlavor   string

	// OrgID is what we answered with, derived from the tenant by FNV-64a.
	// Stored because the derivation is ours: if the hash ever changes, the
	// runners enrolled under the old one are only recoverable from here.
	OrgID int64

	// Attributes is the request's JSON:API attributes as they arrived, for a
	// native JSON column, so a new attribute is queryable without a migration.
	Attributes string
}

func (r RunnerEnrollmentRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.RunnerID, r.EnrolledAt, r.Name,
		orEmptySlice(r.Modes), r.Host, r.PublicKeyPEM, r.AgentHostname,
		r.OrchClusterID, r.AgentFlavor, r.OrgID, orJSONObject(r.Attributes))
}

// RunnerTaskUpdateRow is the outcome of one action. Outcome is the JSON:API
// document id, "succeed_task" or "fail_task". ErrorCode is a pointer because
// it is a numeric enum on the runner's side and a succeeded task sends none.
// Outputs is the action's own result, arbitrary JSON of any size — previously
// logged as a byte count, which is the one number about it nobody needs.
type RunnerTaskUpdateRow struct {
	TenantID  string
	At        time.Time
	Outcome   string
	TaskID    string
	ActionFQN string
	JobID     string
	Client    string

	Branch       string
	Outputs      string
	ErrorCode    *int64
	ErrorDetails string
	APIError     string

	// Extra holds undeclared attributes from either level of the document,
	// values JSON-encoded, prefixed "payload." when they came from the nested
	// object.
	Extra map[string]string
}

func (r RunnerTaskUpdateRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.At, r.Outcome, r.TaskID, r.ActionFQN, r.JobID,
		r.Client, r.Branch, r.Outputs, r.ErrorCode, r.ErrorDetails, r.APIError,
		orEmpty(r.Extra))
}

// RunnerHeartbeatRow is one per-task heartbeat: high frequency, low value
// individually, and exactly what answers "when did this job stop making
// progress".
type RunnerHeartbeatRow struct {
	TenantID  string
	At        time.Time
	TaskID    string
	ActionFQN string
	JobID     string
	Client    string
}

func (r RunnerHeartbeatRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.At, r.TaskID, r.ActionFQN, r.JobID, r.Client)
}

// RunnerDequeueRow is one idle poll for work. It is the only liveness signal a
// runner with no tasks produces. Both timestamps stay STRINGS: their format is
// undocumented and an unparseable value must not become 1970.
type RunnerDequeueRow struct {
	TenantID           string
	At                 time.Time
	RunnerStartedAt    string
	LastTaskReceivedAt string
	Version            string
	Modes              string
}

func (r RunnerDequeueRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.At, r.RunnerStartedAt, r.LastTaskReceivedAt,
		r.Version, r.Modes)
}

// ActionConnectionRow is one connection an action runner may use to reach a
// third-party system.
//
// SENSITIVE: Credentials holds that integration's SECRETS verbatim — tokens,
// passwords, private keys. It is stored because a connection without its
// credentials cannot be used, and dropping them would make the table a lie.
// The logging path prints only the KEY NAMES of this object and must keep
// doing so; anything that reads this column is reading a credential store.
type ActionConnectionRow struct {
	TenantID        string
	At              time.Time
	Name            string
	RunnerID        string
	Tags            map[string][]string
	IntegrationType string
	Credentials     string
	Extra           map[string]string
}

func (r ActionConnectionRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.At, r.Name, r.RunnerID, orEmpty(r.Tags),
		r.IntegrationType, r.Credentials, orEmpty(r.Extra))
}

func init() {
	// Sender identity arrives with every sketch batch — one row per request,
	// not per metric. Same shape as check_runs: a trickle, so a small
	// threshold and a longer timer.
	registerWriter(AgentBatchMetadataWriter, WriterConfig{
		Name: "agent_batch_metadata",
		Insert: `INSERT INTO agent_batch_metadata
			(tenant_id, received_at, intake, agent_version, timezone, current_epoch,
			 internal_ip, public_ip)`,
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, AgentBatchMetadataRow{})

	// A dozen-odd checks per host per /intake/ request: check_runs' class.
	registerWriter(AgentChecksWriter, WriterConfig{
		Name: "agent_checks",
		Insert: `INSERT INTO agent_checks
			(tenant_id, received_at, hostname, agent_version, uuid,
			 check_name, source_type, instance_id, status, message,
			 positional_extra, meta)`,
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, AgentCheckRow{})

	registerWriter(ExternalHostTagsWriter, WriterConfig{
		Name: "external_host_tags",
		Insert: `INSERT INTO external_host_tags
			(tenant_id, received_at, host, source, tags)`,
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, ExternalHostTagsRow{})

	// Whole-document payloads, like k8s_manifests: a row here is an inventory
	// blob, so the counts stand for megabytes. Tight ceilings, one flush in
	// flight.
	registerWriter(AgentMetadataWriter, WriterConfig{
		Name: "agent_metadata",
		Insert: `INSERT INTO agent_metadata
			(tenant_id, received_at, variant, hostname, cluster_name, cluster_id,
			 timestamp, uuid, payload, envelope_extra)`,
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, AgentMetadataRow{})

	registerWriter(DelegatedAuthWriter, WriterConfig{
		Name: "delegated_auth_requests",
		Insert: `INSERT INTO delegated_auth_requests
			(tenant_id, at, scheme, proof_fingerprint, api_key_id)`,
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, DelegatedAuthRow{})

	// A quiet per-fleet summary, like k8s_cluster: the timer does all the
	// flushing.
	registerWriter(SymbolQueriesWriter, WriterConfig{
		Name: "symbol_queries",
		Insert: `INSERT INTO symbol_queries
			(tenant_id, at, arch, build_ids, resource)`,
		MaxRows: 50, FlushInterval: 15 * time.Second,
		BufferLimit: 1_000, MaxInFlight: 1,
	}, SymbolQueryRow{})

	// Upsert-by-key state at human frequency, like hosts: a runner enrolls
	// once and re-enrolls on restart.
	registerWriter(RunnerEnrollmentsWriter, WriterConfig{
		Name: "runner_enrollments",
		Insert: `INSERT INTO runner_enrollments
			(tenant_id, runner_id, enrolled_at, name, modes, host, public_key_pem,
			 agent_hostname, orch_cluster_id, agent_flavor, org_id, attributes)`,
		MaxRows: 100, FlushInterval: 10 * time.Second,
	}, RunnerEnrollmentRow{})

	// Human-scale audit trail, like events: tens or hundreds a day, and a
	// timer short enough that a failed action is visible while someone is
	// still looking for it.
	registerWriter(RunnerTaskUpdatesWriter, WriterConfig{
		Name: "runner_task_updates",
		Insert: `INSERT INTO runner_task_updates
			(tenant_id, at, outcome, task_id, action_fqn, job_id, client,
			 branch, outputs, error_code, error_details, api_error, extra)`,
		MaxRows: 200, FlushInterval: 2 * time.Second,
	}, RunnerTaskUpdateRow{})

	// The two polling loops are the chattiest tables of the group: one request
	// per runner per interval, forever, whether or not there is work.
	registerWriter(RunnerHeartbeatsWriter, WriterConfig{
		Name: "runner_heartbeats",
		Insert: `INSERT INTO runner_heartbeats
			(tenant_id, at, task_id, action_fqn, job_id, client)`,
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, RunnerHeartbeatRow{})

	registerWriter(RunnerDequeuesWriter, WriterConfig{
		Name: "runner_dequeues",
		Insert: `INSERT INTO runner_dequeues
			(tenant_id, at, runner_started_at, last_task_received_at, version, modes)`,
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, RunnerDequeueRow{})

	registerWriter(ActionConnectionsWriter, WriterConfig{
		Name: "action_connections",
		Insert: `INSERT INTO action_connections
			(tenant_id, at, name, runner_id, tags, integration_type, credentials, extra)`,
		MaxRows: 50, FlushInterval: 15 * time.Second,
		BufferLimit: 1_000, MaxInFlight: 1,
	}, ActionConnectionRow{})

	registerTypes(
		WriteAgentBatchMetadata{}, AgentBatchMetadataRow{},
		WriteAgentChecks{}, AgentCheckRow{},
		WriteExternalHostTags{}, ExternalHostTagsRow{},
		WriteAgentMetadata{}, AgentMetadataRow{},
		WriteDelegatedAuthRequests{}, DelegatedAuthRow{},
		WriteSymbolQueries{}, SymbolQueryRow{},
		WriteRunnerEnrollments{}, RunnerEnrollmentRow{},
		WriteRunnerTaskUpdates{}, RunnerTaskUpdateRow{},
		WriteRunnerHeartbeats{}, RunnerHeartbeatRow{},
		WriteRunnerDequeues{}, RunnerDequeueRow{},
		WriteActionConnections{}, ActionConnectionRow{},
	)
}
