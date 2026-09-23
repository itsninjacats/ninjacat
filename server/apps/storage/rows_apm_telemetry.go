package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// apm_telemetry — instrumentation-telemetry-intake.<site>'s one envelope
// shared by five producers (tracer libraries, the fleet installer,
// agent-telemetry, the trace-agent's onboarding events and the
// cluster-agent's remote-config events). See
// schema/migrations/0011_apm_telemetry.sql and intake/router_misc.go for the
// full shape and the message-batch fan-out rule.

// APMTelemetryWriter is the process handlers send WriteAPMTelemetry to.
const APMTelemetryWriter = gen.Atom("storage_apm_telemetry")

// WriteAPMTelemetry carries a batch of rows for one or more requests.
// Concrete slice, never []Row — see the note in messages.go.
type WriteAPMTelemetry struct{ Rows []APMTelemetryRow }

func (m WriteAPMTelemetry) rows() []Row { return toRows(m.Rows) }

// APMTelemetryRow is one telemetry request, or one entry of a message-batch
// request — see BatchIndex.
type APMTelemetryRow struct {
	TenantID   string
	ReceivedAt time.Time

	RequestType string
	Producer    string
	APIVersion  string
	RuntimeID   string

	// SeqID travels through json.Number end to end (see intake's apmSeqID) —
	// never float64, which would round a large sequence number past 2^53.
	SeqID *int64

	// TracerTime/EventTime are the parsed Unix-time value; the *Raw twins
	// keep the literal JSON text next to it, because a parse failure must
	// not erase what actually arrived. Both pointers are nil when the
	// producer sent nothing for that key — never the epoch.
	TracerTime    *time.Time
	TracerTimeRaw string
	EventTime     *time.Time
	EventTimeRaw  string

	ServiceName     string
	ServiceVersion  string
	Env             string
	LanguageName    string
	LanguageVersion string
	TracerVersion   string

	Hostname         string
	HostOS           string
	HostArchitecture string
	// HostExtra holds every host-block key besides hostname/os/architecture,
	// JSON-text-encoded by name — gohai and the tracer both put more than
	// those three fields there, and the schema does not need to know their
	// names in advance to keep them.
	HostExtra map[string]string

	// Payload is the producer-specific part, raw. For a batch parent row
	// this is the WHOLE batch array; for any other row, including a batch
	// child, it is that row's own payload.
	Payload string
	Debug   string
	Origin  string

	// Proxy headers: added by the trace-agent's HTTP hop, never by the
	// sender itself.
	Via                   string
	DDAgentHostname       string
	DDAgentEnv            string
	DatadogContainerID    string
	XDatadogContainerTags string

	// Extra holds every envelope key besides the ones with their own column,
	// JSON-text-encoded by name — an undeclared key stays visible instead of
	// silently disappearing.
	Extra map[string]string

	// BatchIndex is nil for the request's own row and set (0-based) for each
	// row a message-batch entry produced; ParentRequestType is that
	// request's own request_type ("message-batch") on those child rows and
	// empty otherwise. A pointer, not a 0 sentinel: entry 0 of a real batch
	// must stay distinguishable from "not a batch entry at all".
	BatchIndex        *uint32
	ParentRequestType string
}

// AppendTo must match the INSERT column list registered below, argument for
// argument — see registry.go and storagetest's arity test.
func (r APMTelemetryRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt,
		r.RequestType, r.Producer, r.APIVersion, r.RuntimeID,
		r.SeqID,
		r.TracerTime, r.TracerTimeRaw, r.EventTime, r.EventTimeRaw,
		r.ServiceName, r.ServiceVersion, r.Env, r.LanguageName, r.LanguageVersion, r.TracerVersion,
		r.Hostname, r.HostOS, r.HostArchitecture, orEmpty(r.HostExtra),
		r.Payload, r.Debug, r.Origin,
		r.Via, r.DDAgentHostname, r.DDAgentEnv, r.DatadogContainerID, r.XDatadogContainerTags,
		orEmpty(r.Extra),
		r.BatchIndex, r.ParentRequestType)
}

func init() {
	registerWriter(APMTelemetryWriter, WriterConfig{
		Name: "apm_telemetry",
		Insert: `INSERT INTO apm_telemetry
			(tenant_id, received_at, request_type, producer, api_version, runtime_id,
			 seq_id, tracer_time, tracer_time_raw, event_time, event_time_raw,
			 service_name, service_version, env, language_name, language_version, tracer_version,
			 hostname, host_os, host_architecture, host_extra,
			 payload, debug, origin,
			 via, dd_agent_hostname, dd_agent_env, datadog_container_id, x_datadog_container_tags,
			 extra, batch_index, parent_request_type)`,
		// Bursty-per-pass, the k8s_resources class: every instrumented
		// process heartbeats on its own schedule, so a deploy or a restart
		// storm makes many of them report within the same second, and a
		// message-batch request turns into several rows on top of its own.
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, APMTelemetryRow{})
	registerTypes(WriteAPMTelemetry{}, APMTelemetryRow{})
}
