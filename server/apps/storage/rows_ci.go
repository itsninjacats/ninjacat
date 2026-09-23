package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// CI Visibility / Test Optimization, CI provider webhooks, git metadata and
// the synthetics test catalogue — eight tables, one file, registered from
// init() per registry.go.
//
// Schema: schema/migrations/0017_ci_visibility.sql.
// Docs:   docs/tables/ci_visibility.md, docs/tables/synthetics_agent.md.
// Intake: intake/router_civisibility.go.
//
// They live together because they are one product and are added in one
// change; they stay separate TABLES because they are six different wire
// formats (see the migration's header). Nothing here declares a nested named
// struct type: every row is flat plus maps and slices of primitives, which
// keeps registerTypes to the Write*/Row pairs and avoids the EDF
// "unresolvable types" trap register_edf_test.go exists for.

const (
	// CITestEventsWriter is the process HandleCITestCycle sends to.
	CITestEventsWriter = gen.Atom("storage_ci_test_events")
	// CICoverageWriter is the process HandleCITestCov sends to.
	CICoverageWriter = gen.Atom("storage_ci_coverage")
	// GitCommitsWriter takes commit sightings from search_commits and packfile.
	GitCommitsWriter = gen.Atom("storage_git_commits")
	// GitPackfilesWriter takes packfile uploads.
	GitPackfilesWriter = gen.Atom("storage_git_packfiles")
	// CISettingsRequestsWriter takes the tracer-configuration questions and
	// the answers we gave them.
	CISettingsRequestsWriter = gen.Atom("storage_ci_settings_requests")
	// CIWebhookEventsWriter takes CI provider webhook batches.
	CIWebhookEventsWriter = gen.Atom("storage_ci_webhook_events")
	// CIPipelineEventsWriter takes the datadog-ci CLI's tag/measure/span calls.
	CIPipelineEventsWriter = gen.Atom("storage_ci_pipeline_events")
	// SyntheticsTestConfigsWriter takes synthetic test definitions.
	//
	// NOTHING SENDS TO IT YET, on purpose: the table is the catalogue the
	// agent's poller reads (intake/router_civisibility.go's
	// HandleSyntheticsAgentTests), and a future panel is what fills it. The
	// writer is registered now so the table has an INSERT that the arity and
	// round-trip tests exercise — an unregistered table is one nobody checks.
	SyntheticsTestConfigsWriter = gen.Atom("storage_synthetics_test_configs")
)

// ---------------------------------------------------------------------------
// ci_test_events

// WriteCITestEvents carries a batch of CI Visibility events. Concrete slice,
// never []Row — see the note in messages.go.
type WriteCITestEvents struct{ Events []CITestEventRow }

func (m WriteCITestEvents) rows() []Row { return toRows(m.Events) }

// CITestEventRow is one event out of a citestcycle payload's events[].
//
// SessionID/ModuleID/SuiteID are plain UInt64 and not pointers: an event type
// that does not carry an id sends none, and 0 is the tracer's own spelling of
// that (msgp omitempty on a uint64). The flags below ARE pointers, because
// there the absence means "this tracer has the feature off" and false means
// "the feature is on and said no" — a distinction a report on retried tests
// depends on.
type CITestEventRow struct {
	TenantID   string
	ReceivedAt time.Time

	EventType      string
	EventVersion   int32
	PayloadVersion int32

	SessionID uint64
	ModuleID  uint64
	SuiteID   uint64

	TraceID  uint64
	SpanID   uint64
	ParentID uint64

	ITRCorrelationID string

	Service  string
	Env      string
	Name     string
	Resource string
	SpanType string

	// Start keeps nanosecond precision: the column is DateTime64(9) and the
	// wire value is ns since epoch.
	Start      time.Time
	DurationNs int64
	Error      int32

	TestName             string
	TestSuite            string
	TestModule           string
	TestFramework        string
	TestFrameworkVersion string
	TestStatus           string
	TestType             string
	TestSourceFile       string
	TestSourceStart      *int64
	TestSourceEnd        *int64
	TestParameters       string
	TestCodeowners       string
	TestCommand          string
	TestSessionName      string

	GitRepositoryURL        string
	GitBranch               string
	GitTag                  string
	GitCommitSHA            string
	GitCommitMessage        string
	GitCommitAuthorName     string
	GitCommitAuthorEmail    string
	GitCommitAuthorDate     string
	GitCommitCommitterName  string
	GitCommitCommitterEmail string
	GitCommitCommitterDate  string

	CIProviderName   string
	CIPipelineID     string
	CIPipelineName   string
	CIPipelineNumber string
	CIPipelineURL    string
	CIJobID          string
	CIJobName        string
	CIJobURL         string
	CIStageName      string
	CIWorkspacePath  string
	CINodeName       string
	CINodeLabels     string

	OSPlatform     string
	OSVersion      string
	OSArchitecture string
	RuntimeName    string
	RuntimeVersion string
	Language       string
	RuntimeID      string
	LibraryVersion string

	TestIsNew             *uint8
	TestIsRetry           *uint8
	TestIsModified        *uint8
	TestSkippedByITR      *uint8
	ITRUnskippable        *uint8
	ITRForcedRun          *uint8
	CodeCoverageEnabled   *uint8
	TestRetryReason       string
	EarlyFlakeAbortReason string

	EVPSubdomain  string
	ContainerID   string
	AgentHostname string
	AgentVersion  string

	Meta     map[string]string
	Metrics  map[string]float64
	Metadata map[string]string

	// Content is the event's content object rendered as JSON, whatever the
	// request's wire format was.
	Content string
}

// AppendTo must match the INSERT column list registered below, argument for
// argument. Nothing checks that at compile time; storagetest's arity test
// does it at test time, off the zero row handed to registerWriter.
func (r CITestEventRow) AppendTo(b driver.Batch) error {
	return b.Append(
		r.TenantID, r.ReceivedAt,
		r.EventType, r.EventVersion, r.PayloadVersion,
		r.SessionID, r.ModuleID, r.SuiteID,
		r.TraceID, r.SpanID, r.ParentID,
		r.ITRCorrelationID,
		r.Service, r.Env, r.Name, r.Resource, r.SpanType,
		r.Start, r.DurationNs, r.Error,
		r.TestName, r.TestSuite, r.TestModule, r.TestFramework, r.TestFrameworkVersion,
		r.TestStatus, r.TestType, r.TestSourceFile, r.TestSourceStart, r.TestSourceEnd,
		r.TestParameters, r.TestCodeowners, r.TestCommand, r.TestSessionName,
		r.GitRepositoryURL, r.GitBranch, r.GitTag, r.GitCommitSHA, r.GitCommitMessage,
		r.GitCommitAuthorName, r.GitCommitAuthorEmail, r.GitCommitAuthorDate,
		r.GitCommitCommitterName, r.GitCommitCommitterEmail, r.GitCommitCommitterDate,
		r.CIProviderName, r.CIPipelineID, r.CIPipelineName, r.CIPipelineNumber, r.CIPipelineURL,
		r.CIJobID, r.CIJobName, r.CIJobURL, r.CIStageName, r.CIWorkspacePath,
		r.CINodeName, r.CINodeLabels,
		r.OSPlatform, r.OSVersion, r.OSArchitecture, r.RuntimeName, r.RuntimeVersion,
		r.Language, r.RuntimeID, r.LibraryVersion,
		r.TestIsNew, r.TestIsRetry, r.TestIsModified, r.TestSkippedByITR,
		r.ITRUnskippable, r.ITRForcedRun, r.CodeCoverageEnabled,
		r.TestRetryReason, r.EarlyFlakeAbortReason,
		r.EVPSubdomain, r.ContainerID, r.AgentHostname, r.AgentVersion,
		orEmpty(r.Meta), orEmpty(r.Metrics), orEmpty(r.Metadata),
		r.Content)
}

// ---------------------------------------------------------------------------
// ci_coverage

// WriteCICoverage carries a batch of per-test coverage entries.
type WriteCICoverage struct{ Coverage []CICoverageRow }

func (m WriteCICoverage) rows() []Row { return toRows(m.Coverage) }

// CICoverageRow is one entry of a citestcov payload's coverages[].
//
// FilesFilename and FilesBitmap are parallel and always the same length: a
// file reported without line-level data keeps an empty bitmap at its index
// rather than shifting the arrays out of step.
type CICoverageRow struct {
	TenantID   string
	ReceivedAt time.Time

	PayloadVersion int32

	SessionID uint64
	SuiteID   uint64
	// SpanID is a pointer because the encoder omits it entirely in
	// suite-skipping mode, and 0 is a legal span id.
	SpanID *uint64

	FilesFilename []string
	FilesBitmap   []string

	// Raw is this entry's bytes exactly as they arrived; RawFormat is
	// "msgpack" or "json".
	Raw       string
	RawFormat string

	Event string

	Extra map[string]string

	EVPSubdomain  string
	ContainerID   string
	AgentHostname string
	AgentVersion  string
}

// AppendTo must match the INSERT column list registered below.
func (r CICoverageRow) AppendTo(b driver.Batch) error {
	return b.Append(
		r.TenantID, r.ReceivedAt, r.PayloadVersion,
		r.SessionID, r.SuiteID, r.SpanID,
		orEmptySlice(r.FilesFilename), orEmptySlice(r.FilesBitmap),
		r.Raw, r.RawFormat, r.Event, orEmpty(r.Extra),
		r.EVPSubdomain, r.ContainerID, r.AgentHostname, r.AgentVersion)
}

// ---------------------------------------------------------------------------
// git_commits

// WriteGitCommits carries a batch of commit sightings.
type WriteGitCommits struct{ Commits []GitCommitRow }

func (m WriteGitCommits) rows() []Row { return toRows(m.Commits) }

// GitCommitRow records that a sha was mentioned for a repository.
//
// It is NOT a claim that we hold the objects — search_commits sends the
// tracer's local commit list to ask what we have, so most rows here come
// from a question rather than an upload. PackfileID is non-nil only when the
// sha arrived as a packfile's pushedSha, which is the case where we do.
type GitCommitRow struct {
	TenantID      string
	RepositoryURL string
	SHA           string
	SeenAt        time.Time
	PackfileID    *string
	Source        string
}

// AppendTo must match the INSERT column list registered below.
func (r GitCommitRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.RepositoryURL, r.SHA, r.SeenAt, r.PackfileID, r.Source)
}

// ---------------------------------------------------------------------------
// git_packfiles

// WriteGitPackfiles carries packfile uploads.
type WriteGitPackfiles struct{ Packfiles []GitPackfileRow }

func (m WriteGitPackfiles) rows() []Row { return toRows(m.Packfiles) }

// GitPackfileRow is one .pack file upload. The client sends one request per
// pack file, so one row per request.
type GitPackfileRow struct {
	TenantID   string
	ReceivedAt time.Time

	RepositoryURL string
	PushedSHA     string

	// PackfileID is sha256 of Packfile, hex. The upload carries no id of its
	// own and git_commits.packfile_id has to point at something.
	PackfileID string
	Filename   string
	Packfile   string
	SizeBytes  uint64

	OtherParts map[string]string
}

// AppendTo must match the INSERT column list registered below.
func (r GitPackfileRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.RepositoryURL, r.PushedSHA,
		r.PackfileID, r.Filename, r.Packfile, r.SizeBytes, orEmpty(r.OtherParts))
}

// ---------------------------------------------------------------------------
// ci_settings_requests

// WriteCISettingsRequests carries tracer-configuration questions.
type WriteCISettingsRequests struct{ Requests []CISettingsRequestRow }

func (m WriteCISettingsRequests) rows() []Row { return toRows(m.Requests) }

// CISettingsRequestRow is one question a tracer asked and the answer we gave.
//
// The response is stored alongside the request because the answer is a
// contract we invented (every feature off, empty lists) rather than one we
// received: the day it changes, what a given tracer was told is the
// difference between a bug report and a mystery.
type CISettingsRequestRow struct {
	TenantID   string
	ReceivedAt time.Time

	Endpoint    string
	RequestID   string
	RequestType string

	Service       string
	Env           string
	RepositoryURL string
	Branch        string
	SHA           string
	TestLevel     string
	Module        string
	CommitMessage string
	PageState     string

	Configurations string

	RequestBody  string
	ResponseBody string
}

// AppendTo must match the INSERT column list registered below.
func (r CISettingsRequestRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Endpoint, r.RequestID, r.RequestType,
		r.Service, r.Env, r.RepositoryURL, r.Branch, r.SHA, r.TestLevel,
		r.Module, r.CommitMessage, r.PageState, r.Configurations,
		r.RequestBody, r.ResponseBody)
}

// ---------------------------------------------------------------------------
// ci_webhook_events

// WriteCIWebhookEvents carries CI provider webhook elements.
type WriteCIWebhookEvents struct{ Events []CIWebhookEventRow }

func (m WriteCIWebhookEvents) rows() []Row { return toRows(m.Events) }

// CIWebhookEventRow is one element of a webhook batch — a pipeline, a stage
// or a job, told apart by Level.
//
// The ids are Strings, not UInt64, even though Jenkins sends trace_id and
// span_id as decimal numbers: GitLab and the other providers put their own
// identifiers in the same fields, and at least one of them is a UUID. A
// String keeps every provider's spelling; a UInt64 would quietly become 0
// for the ones that are not numeric.
type CIWebhookEventRow struct {
	TenantID   string
	ReceivedAt time.Time

	Provider string
	Level    string
	Service  string

	PayloadVersion *int64
	PartialRetry   *uint8
	IsManual       *uint8

	TraceID          string
	SpanID           string
	ParentSpanID     string
	ID               string
	UniqueID         string
	PipelineID       string
	PipelineUniqueID string
	PipelineName     string
	StageID          string
	StageName        string
	ParentStageID    string

	Name   string
	URL    string
	Status string

	StartRaw    string
	StartParsed *time.Time
	EndRaw      string
	EndParsed   *time.Time
	QueueTimeMs *int64

	NodeName      string
	NodeHostname  string
	NodeWorkspace string
	NodeLabels    []string

	GitRepositoryURL  string
	GitDefaultBranch  string
	GitBranch         string
	GitSHA            string
	GitTag            string
	GitMessage        string
	GitAuthorName     string
	GitAuthorEmail    string
	GitAuthorTime     string
	GitCommitterName  string
	GitCommitterEmail string
	GitCommitTime     string

	UserName  string
	UserEmail string

	ErrorMessage string
	ErrorType    string
	ErrorDomain  string
	ErrorStack   string

	ParentPipelineTraceID string
	ParentPipelineURL     string

	Parameters map[string]string

	// Tags is the multiset shape: the wire form is an array of "key:value"
	// strings and the Jenkins plugin repeats keys for per-configuration axes.
	Tags map[string][]string

	DeliveryID string
	Headers    map[string]string
	Extra      map[string]string
	Body       string
}

// AppendTo must match the INSERT column list registered below.
func (r CIWebhookEventRow) AppendTo(b driver.Batch) error {
	return b.Append(
		r.TenantID, r.ReceivedAt, r.Provider, r.Level, r.Service,
		r.PayloadVersion, r.PartialRetry, r.IsManual,
		r.TraceID, r.SpanID, r.ParentSpanID, r.ID, r.UniqueID,
		r.PipelineID, r.PipelineUniqueID, r.PipelineName,
		r.StageID, r.StageName, r.ParentStageID,
		r.Name, r.URL, r.Status,
		r.StartRaw, r.StartParsed, r.EndRaw, r.EndParsed, r.QueueTimeMs,
		r.NodeName, r.NodeHostname, r.NodeWorkspace, orEmptySlice(r.NodeLabels),
		r.GitRepositoryURL, r.GitDefaultBranch, r.GitBranch, r.GitSHA, r.GitTag,
		r.GitMessage, r.GitAuthorName, r.GitAuthorEmail, r.GitAuthorTime,
		r.GitCommitterName, r.GitCommitterEmail, r.GitCommitTime,
		r.UserName, r.UserEmail,
		r.ErrorMessage, r.ErrorType, r.ErrorDomain, r.ErrorStack,
		r.ParentPipelineTraceID, r.ParentPipelineURL,
		orEmpty(r.Parameters), orEmpty(r.Tags), r.DeliveryID,
		orEmpty(r.Headers), orEmpty(r.Extra), r.Body)
}

// ---------------------------------------------------------------------------
// ci_pipeline_events

// WriteCIPipelineEvents carries datadog-ci CLI calls.
type WriteCIPipelineEvents struct{ Events []CIPipelineEventRow }

func (m WriteCIPipelineEvents) rows() []Row { return toRows(m.Events) }

// CIPipelineEventRow is one datadog-ci tag / measure / custom-span call.
type CIPipelineEventRow struct {
	TenantID   string
	ReceivedAt time.Time

	Kind     string
	DataType string

	Provider string
	CILevel  *int64
	CIEnv    map[string]string

	// Tags is a plain map: the wire form here is a JSON object, so keys are
	// unique by construction. This is NOT the "key:value" string list that
	// makes a Datadog tag multiset — see ci_webhook_events above for that.
	Tags    map[string]string
	Metrics map[string]float64

	SpanName     string
	SpanStartRaw string
	SpanEndRaw   string

	Attributes string
	Body       string
	Extra      map[string]string
}

// AppendTo must match the INSERT column list registered below.
func (r CIPipelineEventRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Kind, r.DataType,
		r.Provider, r.CILevel, orEmpty(r.CIEnv),
		orEmpty(r.Tags), orEmpty(r.Metrics),
		r.SpanName, r.SpanStartRaw, r.SpanEndRaw,
		r.Attributes, r.Body, orEmpty(r.Extra))
}

// ---------------------------------------------------------------------------
// synthetics_test_configs

// WriteSyntheticsTestConfigs carries synthetic test definitions.
type WriteSyntheticsTestConfigs struct{ Configs []SyntheticsTestConfigRow }

func (m WriteSyntheticsTestConfigs) rows() []Row { return toRows(m.Configs) }

// SyntheticsTestConfigRow is one test the agent's poller may be handed.
//
// Fields are exactly datadog-agent's common.SyntheticsTestConfig
// (comp/syntheticstestscheduler/common/data.go). Subtype is load-bearing:
// the agent's custom UnmarshalJSON switches on it to pick the shape of
// Request, and an unrecognised value fails the ENTIRE poll response, not
// just that one test. Only "UDP", "TCP" and "ICMP" are valid today.
type SyntheticsTestConfigRow struct {
	TenantID  string
	UpdatedAt time.Time

	PublicID string
	Version  int32
	Type     string
	Subtype  string

	TickEvery int32
	OrgID     int64
	MainDC    string
	ResultID  string
	RunType   string

	// Assertions and Request are JSON text: the poller hands them straight
	// back out as JSON and nothing filters on them.
	Assertions string
	Request    string

	Enabled uint8
}

// AppendTo must match the INSERT column list registered below.
func (r SyntheticsTestConfigRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.UpdatedAt, r.PublicID, r.Version, r.Type, r.Subtype,
		r.TickEvery, r.OrgID, r.MainDC, r.ResultID, r.RunType,
		r.Assertions, r.Request, r.Enabled)
}

func init() {
	registerWriter(CITestEventsWriter, WriterConfig{
		Name: "ci_test_events",
		Insert: `INSERT INTO ci_test_events
			(tenant_id, received_at, event_type, event_version, payload_version,
			 session_id, module_id, suite_id, trace_id, span_id, parent_id,
			 itr_correlation_id, service, env, name, resource, span_type,
			 start, duration_ns, error,
			 test_name, test_suite, test_module, test_framework, test_framework_version,
			 test_status, test_type, test_source_file, test_source_start, test_source_end,
			 test_parameters, test_codeowners, test_command, test_session_name,
			 git_repository_url, git_branch, git_tag, git_commit_sha, git_commit_message,
			 git_commit_author_name, git_commit_author_email, git_commit_author_date,
			 git_commit_committer_name, git_commit_committer_email, git_commit_committer_date,
			 ci_provider_name, ci_pipeline_id, ci_pipeline_name, ci_pipeline_number, ci_pipeline_url,
			 ci_job_id, ci_job_name, ci_job_url, ci_stage_name, ci_workspace_path,
			 ci_node_name, ci_node_labels,
			 os_platform, os_version, os_architecture, runtime_name, runtime_version,
			 language, runtime_id, library_version,
			 test_is_new, test_is_retry, test_is_modified, test_skipped_by_itr,
			 itr_unskippable, itr_forced_run, code_coverage_enabled,
			 test_retry_reason, early_flake_abort_reason,
			 evp_subdomain, container_id, agent_hostname, agent_version,
			 meta, metrics, metadata, content)`,
		// Bursty-per-pass, like k8s_resources: a CI run posts one payload per
		// flush with every test that finished since the last one, so batches
		// arrive in clumps and then nothing for minutes. Rows carry two maps
		// and a raw content blob, so the buffer ceiling is in rows that cost
		// real memory.
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, CITestEventRow{})

	registerWriter(CICoverageWriter, WriterConfig{
		Name: "ci_coverage",
		Insert: `INSERT INTO ci_coverage
			(tenant_id, received_at, payload_version, session_id, suite_id, span_id,
			 files_filename, files_bitmap, raw, raw_format, event, extra,
			 evp_subdomain, container_id, agent_hostname, agent_version)`,
		// A row holds a bitmap per covered file plus the raw entry, so this
		// is the bulky-document class (k8s_manifests, raw_payloads): tight
		// ceilings, one flush in flight.
		MaxRows: 500, FlushInterval: 5 * time.Second,
		BufferLimit: 10_000, MaxInFlight: 1,
	}, CICoverageRow{})

	registerWriter(GitCommitsWriter, WriterConfig{
		Name: "git_commits",
		Insert: `INSERT INTO git_commits
			(tenant_id, repository_url, sha, seen_at, packfile_id, source)`,
		// One search_commits request carries up to a thousand shas, and every
		// CI run repeats most of them — ReplacingMergeTree collapses the
		// duplicates, so the writer's job is just to not flood. Sized like
		// processes: large batches, moderate interval.
		MaxRows: 5000, FlushInterval: 5 * time.Second,
	}, GitCommitRow{})

	registerWriter(GitPackfilesWriter, WriterConfig{
		Name: "git_packfiles",
		Insert: `INSERT INTO git_packfiles
			(tenant_id, received_at, repository_url, pushed_sha, packfile_id,
			 filename, packfile, size_bytes, other_parts)`,
		// A row is a whole git pack. Same reasoning as agent_flares: the
		// counts stand for megabytes, so the ceiling is tight and only one
		// flush is ever in flight.
		MaxRows: 50, FlushInterval: 10 * time.Second,
		BufferLimit: 2_000, MaxInFlight: 1,
	}, GitPackfileRow{})

	registerWriter(CISettingsRequestsWriter, WriterConfig{
		Name: "ci_settings_requests",
		Insert: `INSERT INTO ci_settings_requests
			(tenant_id, received_at, endpoint, request_id, request_type,
			 service, env, repository_url, branch, sha, test_level,
			 module, commit_message, page_state, configurations,
			 request_body, response_body)`,
		// Human-scale: one request per test process at startup. Small
		// threshold, short timer — a tracer being misconfigured is something
		// somebody is watching for right now.
		MaxRows: 200, FlushInterval: 5 * time.Second,
	}, CISettingsRequestRow{})

	registerWriter(CIWebhookEventsWriter, WriterConfig{
		Name: "ci_webhook_events",
		Insert: `INSERT INTO ci_webhook_events
			(tenant_id, received_at, provider, level, service,
			 payload_version, partial_retry, is_manual,
			 trace_id, span_id, parent_span_id, id, unique_id,
			 pipeline_id, pipeline_unique_id, pipeline_name,
			 stage_id, stage_name, parent_stage_id,
			 name, url, status,
			 start_raw, start_parsed, end_raw, end_parsed, queue_time_ms,
			 node_name, node_hostname, node_workspace, node_labels,
			 git_repository_url, git_default_branch, git_branch, git_sha, git_tag,
			 git_message, git_author_name, git_author_email, git_author_time,
			 git_committer_name, git_committer_email, git_commit_time,
			 user_name, user_email,
			 error_message, error_type, error_domain, error_stack,
			 parent_pipeline_trace_id, parent_pipeline_url,
			 parameters, tags, delivery_id, headers, extra, body)`,
		// The Jenkins plugin batches to 5 MB per request and a busy
		// controller emits a stage per step, so these arrive as bursts of
		// hundreds. Bursty-per-pass sizing, one notch below k8s_resources
		// because the rows carry a raw body each.
		MaxRows: 1000, FlushInterval: 5 * time.Second,
		BufferLimit: 20_000, MaxInFlight: 2,
	}, CIWebhookEventRow{})

	registerWriter(CIPipelineEventsWriter, WriterConfig{
		Name: "ci_pipeline_events",
		Insert: `INSERT INTO ci_pipeline_events
			(tenant_id, received_at, kind, data_type, provider, ci_level, ci_env,
			 tags, metrics, span_name, span_start_raw, span_end_raw,
			 attributes, body, extra)`,
		// Human-scale, like events: a CI job calls `datadog-ci tag` a handful
		// of times. A short timer so a tag shows up while somebody is still
		// looking at the run that set it.
		MaxRows: 200, FlushInterval: 2 * time.Second,
	}, CIPipelineEventRow{})

	registerWriter(SyntheticsTestConfigsWriter, WriterConfig{
		Name: "synthetics_test_configs",
		Insert: `INSERT INTO synthetics_test_configs
			(tenant_id, updated_at, public_id, version, type, subtype,
			 tick_every, org_id, main_dc, result_id, run_type,
			 assertions, request, enabled)`,
		// Upsert/low-frequency state, like hosts: a test definition changes
		// when a human changes it. ReplacingMergeTree collapses the
		// duplicates; the timer does all the flushing.
		MaxRows: 100, FlushInterval: 10 * time.Second,
	}, SyntheticsTestConfigRow{})

	registerTypes(
		WriteCITestEvents{}, CITestEventRow{},
		WriteCICoverage{}, CICoverageRow{},
		WriteGitCommits{}, GitCommitRow{},
		WriteGitPackfiles{}, GitPackfileRow{},
		WriteCISettingsRequests{}, CISettingsRequestRow{},
		WriteCIWebhookEvents{}, CIWebhookEventRow{},
		WriteCIPipelineEvents{}, CIPipelineEventRow{},
		WriteSyntheticsTestConfigs{}, SyntheticsTestConfigRow{},
	)
}
