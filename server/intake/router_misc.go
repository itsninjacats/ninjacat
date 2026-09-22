package intake

import (
	"bytes"
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
)

// Two small intakes with one endpoint each.
//
//	resources-intake.<site>                   /api/v2/genresources   x-protobuf, opaque
//	instrumentation-telemetry-intake.<site>   /api/v2/apmtelemetry   JSON envelope
//
//	config: none for resources — event platform, `genresources.` prefix.
//	        telemetry is switched off wholesale with DD_TELEMETRY_ENABLED.
//
// genresources has no published schema anywhere upstream, so it goes to
// raw_payloads (reason "no_schema") — see HandleGenResources. apmtelemetry
// decodes its JSON envelope and stores one row per request in apm_telemetry,
// plus one more per entry of a message-batch request (see HandleTelemetry);
// a body that is not a JSON object goes to raw_payloads too (reason
// "decode_error").

// resources-intake.<site> — Generic Resources track of the event platform.
//
// The agent never looks inside these bytes. An integration (Python or Rust,
// not in datadog-agent) hands a []byte to checkSender.EventPlatformEvent with
// eventType "genresources"; the event platform forwarder wraps it in a
// message and, because the pipeline is declared with ProtobufContentType, uses
// the stream strategy: one message per request, compressed, no batching and
// no envelope (comp/forwarder/eventplatform/impl/pipelines_genresources.go,
// comp/logs-library/sender/stream_strategy.go). There is no .proto for it in
// datadog-agent or agent-payload, so there is nothing published to decode
// with. The body is what the integration produced, byte for byte.
//
// What we can do without a schema: notice if the integration actually sent
// JSON despite the protobuf Content-Type (describe handles that), otherwise
// log size and leading bytes; either way the bytes go to raw_payloads.
func (a *Server) routeResources(g *gin.RouterGroup) {
	g.POST("/api/v2/genresources", a.HandleGenResources)
}

// HandleGenResources accepts one opaque integration payload per request.
func (a *Server) HandleGenResources(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[genresources] cannot read body: %v", err)
		return
	}
	log.Printf("[genresources] origin=%q %d B", c.GetHeader("DD-EVP-ORIGIN"), len(body))
	describe("genresources", c.GetHeader("Content-Type"), body)

	// No .proto exists for this track anywhere upstream (see the header
	// comment above), so there is nothing to decode into: the bytes are
	// everything there is, plus the sender identity the event platform
	// forwarder carries in headers rather than in the body.
	a.storeRaw(c, "genresources", "no_schema", apmOriginNote(c), body)
}

// instrumentation-telemetry-intake.<site> — one path, five producers (docs §4.6).
//
// Every producer sends the same outer envelope, with request_type as the
// discriminator; the rest of the envelope tells the producers apart:
//
//	tracer libraries via the trace-agent proxy (pkg/trace/api/telemetry.go,
//	telemetryRequest): api_version, request_type, tracer_time, runtime_id,
//	seq_id, application, host, payload, debug. The proxy adds Via,
//	DD-Agent-Hostname, DD-Agent-Env, Datadog-Container-ID and
//	X-Datadog-Container-Tags. request_type values (app-started,
//	app-heartbeat, generate-metrics, app-closing, ...) come from the tracer
//	telemetry spec, not from datadog-agent, which forwards them unread.
//
//	fleet installer (pkg/fleet/installer/telemetry/client.go, event): same
//	envelope plus origin; request_type is "logs" or "traces".
//
//	agent telemetry (comp/core/agenttelemetry/impl/sender.go, Payload):
//	api_version, request_type, event_time, debug, host, payload. request_type
//	is agent-metrics, agent-logs, message-batch (payload = array of
//	{request_type, payload}) or a profile event such as agent-bsod.
//
//	trace-agent onboarding (pkg/trace/telemetry/collector.go, OnboardingEvent)
//	and cluster-agent remote config (pkg/clusteragent/telemetry/collector.go,
//	ApmRemoteConfigEvent): request_type, api_version, payload{event_name,
//	tags, error}. request_type is apm-onboarding-event or
//	apm-remote-config-event.
//
// None of those types is importable (nested in agent modules, or unexported),
// so the envelope is read generically: application/host give up their known
// fields as real columns (apm_telemetry), and payload travels as raw JSON
// text regardless of producer — nobody outside the producer's own source can
// claim more authority over its shape than the JSON already has.
//
// message-batch is the one request_type that produces more than one row: its
// payload is [{request_type, payload}, ...], and each entry becomes its own
// apm_telemetry row (batch_index set, parent_request_type = "message-batch")
// in addition to the parent row, whose own payload column keeps the whole
// batch array.
func (a *Server) routeTelemetry(g *gin.RouterGroup) {
	g.POST("/api/v2/apmtelemetry", a.HandleTelemetry)
}

// HandleTelemetry accepts one telemetry envelope from any of the producers.
func (a *Server) HandleTelemetry(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[apmtelemetry] cannot read body: %v", err)
		return
	}

	var env map[string]json.RawMessage
	if err := json.Unmarshal(body, &env); err != nil {
		log.Printf("[apmtelemetry] not a JSON object (%v), raw follows", err)
		describe("apmtelemetry", c.GetHeader("Content-Type"), body)
		a.storeRaw(c, "apmtelemetry", "decode_error", err.Error(), body)
		return
	}

	reqType := telString(env["request_type"])
	producer := telProducer(env, reqType)
	log.Printf("[apmtelemetry] request_type=%s producer=%s api_version=%s %d B",
		reqType, producer, telString(env["api_version"]), len(body))

	// Identity: who is talking. Tracers and the fleet installer identify the
	// process in the envelope; the trace-agent proxy adds the host in headers.
	if raw, ok := env["runtime_id"]; ok {
		log.Printf("   runtime_id=%s seq_id=%s tracer_time=%s",
			telString(raw), telRaw(env["seq_id"]), telRaw(env["tracer_time"]))
	}
	if raw, ok := env["event_time"]; ok {
		log.Printf("   event_time=%s", telRaw(raw))
	}
	if raw, ok := env["application"]; ok {
		telLogApplication(raw)
	}
	if raw, ok := env["host"]; ok {
		telLogHost(raw)
	}
	if via := c.GetHeader("Via"); via != "" {
		log.Printf("   via=%q agent_hostname=%q agent_env=%q container_tags=%q",
			via, c.GetHeader("DD-Agent-Hostname"), c.GetHeader("DD-Agent-Env"),
			c.GetHeader("X-Datadog-Container-Tags"))
	}

	telLogPayload(reqType, env["payload"])

	tenant := TenantFromContext(c)
	if tenant == "" {
		// Same rule as storeRaw: a made-up tenant would put rows where no
		// query looks, so a request with none stores nothing. In practice
		// this cannot happen behind RequireAPIKey.
		return
	}

	// One row for the request itself, whatever producer sent it. For
	// message-batch this row's payload is the WHOLE batch array — "what did
	// this HTTP request contain" stays answerable from one row — and the
	// loop below adds one more row per entry, on top of it, not instead.
	base := apmBuildRow(tenant, time.Now().UTC(), env, reqType, producer, c)
	rows := []storage.APMTelemetryRow{base}

	if reqType == "message-batch" {
		batch, err := telBatch(env["payload"])
		if err != nil {
			log.Printf("   payload: batch is not an array (%v); kept only on the parent row", err)
		}
		for i, entry := range batch {
			childType := telString(entry["request_type"])
			child := base
			child.RequestType = childType
			child.Producer = telProducer(env, childType)
			child.Payload = apmRawText(entry["payload"])
			idx := uint32(i)
			child.BatchIndex = &idx
			child.ParentRequestType = reqType
			rows = append(rows, child)
		}
	}

	a.store(storage.APMTelemetryWriter, storage.WriteAPMTelemetry{Rows: rows}, len(rows))
}

// apmOriginNote is the note storeRaw gets for the tracks with no published
// schema: the event platform's origin headers plus Content-Type, the only
// context available alongside the opaque bytes.
func apmOriginNote(c *gin.Context) string {
	return fmt.Sprintf("DD-EVP-ORIGIN=%q DD-EVP-ORIGIN-VERSION=%q Content-Type=%q",
		c.GetHeader("DD-EVP-ORIGIN"), c.GetHeader("DD-EVP-ORIGIN-VERSION"), c.GetHeader("Content-Type"))
}

// apmUnixTime parses a telemetry timestamp: Unix seconds, with an optional
// fractional part, sent as a JSON number or (some producers) a JSON string.
// nil means "absent or not parseable" — never the epoch, which is why the raw
// JSON text travels alongside in *_raw regardless of whether this succeeds.
func apmUnixTime(raw json.RawMessage) *time.Time {
	if len(raw) == 0 {
		return nil
	}
	s := strings.Trim(strings.TrimSpace(string(raw)), `"`)
	f, err := strconv.ParseFloat(s, 64)
	if err != nil {
		return nil
	}
	sec := int64(f)
	nsec := int64((f - float64(sec)) * 1e9)
	t := time.Unix(sec, nsec).UTC()
	return &t
}

// apmSeqID parses seq_id through json.Number, never through float64 or a bare
// interface{} decode — either would silently round a large sequence number
// past 2^53.
func apmSeqID(raw json.RawMessage) *int64 {
	if len(raw) == 0 {
		return nil
	}
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var n json.Number
	if err := dec.Decode(&n); err != nil {
		return nil
	}
	v, err := n.Int64()
	if err != nil {
		return nil
	}
	return &v
}

// apmRawText is the *_raw twin of every parsed field above: the JSON text
// exactly as it arrived, kept even when parsing succeeds, because a parse
// failure must not erase the value entirely. Absent is "", not "null" or "-".
func apmRawText(raw json.RawMessage) string {
	if len(raw) == 0 {
		return ""
	}
	return string(raw)
}

// apmApplication reads the six fields every producer that sends an
// application block is documented to include.
func apmApplication(raw json.RawMessage) (serviceName, serviceVersion, env, languageName, languageVersion, tracerVersion string) {
	if len(raw) == 0 {
		return
	}
	var app map[string]json.RawMessage
	if err := json.Unmarshal(raw, &app); err != nil {
		return
	}
	return telString(app["service_name"]), telString(app["service_version"]), telString(app["env"]),
		telString(app["language_name"]), telString(app["language_version"]), telString(app["tracer_version"])
}

// apmHost reads hostname/os/architecture and keeps every other key the host
// block carried, JSON-text-encoded by name — gohai and the tracer both put
// more than those three fields there, and the schema does not need to know
// their names in advance to keep them.
func apmHost(raw json.RawMessage) (hostname, osName, arch string, extra map[string]string) {
	if len(raw) == 0 {
		return
	}
	var host map[string]json.RawMessage
	if err := json.Unmarshal(raw, &host); err != nil {
		return
	}
	hostname, osName, arch = telString(host["hostname"]), telString(host["os"]), telString(host["architecture"])
	for k, v := range host {
		if k == "hostname" || k == "os" || k == "architecture" {
			continue
		}
		if extra == nil {
			extra = make(map[string]string, len(host))
		}
		extra[k] = string(v)
	}
	return
}

// apmKnownEnvelopeKeys are the keys apmBuildRow reads by name; anything else
// in the envelope lands in Extra instead of being silently dropped — the same
// "keep the names even without a column" rule k8s_actions' extra_keys follows.
var apmKnownEnvelopeKeys = map[string]bool{
	"request_type": true, "api_version": true, "runtime_id": true, "seq_id": true,
	"tracer_time": true, "event_time": true, "application": true, "host": true,
	"payload": true, "debug": true, "origin": true,
}

func apmExtraKeys(env map[string]json.RawMessage) map[string]string {
	var extra map[string]string
	for k, v := range env {
		if apmKnownEnvelopeKeys[k] {
			continue
		}
		if extra == nil {
			extra = make(map[string]string)
		}
		extra[k] = string(v)
	}
	return extra
}

// apmBuildRow turns one envelope into the row that always exists: the
// request itself. For request_type message-batch, HandleTelemetry adds one
// more row per batch entry on top of this one.
func apmBuildRow(tenant string, receivedAt time.Time, env map[string]json.RawMessage, reqType, producer string, c *gin.Context) storage.APMTelemetryRow {
	serviceName, serviceVersion, envName, languageName, languageVersion, tracerVersion := apmApplication(env["application"])
	hostname, hostOS, hostArch, hostExtra := apmHost(env["host"])

	return storage.APMTelemetryRow{
		TenantID:   tenant,
		ReceivedAt: receivedAt,

		RequestType: reqType,
		Producer:    producer,
		APIVersion:  telString(env["api_version"]),
		RuntimeID:   telString(env["runtime_id"]),

		SeqID: apmSeqID(env["seq_id"]),

		TracerTime:    apmUnixTime(env["tracer_time"]),
		TracerTimeRaw: apmRawText(env["tracer_time"]),
		EventTime:     apmUnixTime(env["event_time"]),
		EventTimeRaw:  apmRawText(env["event_time"]),

		ServiceName:     serviceName,
		ServiceVersion:  serviceVersion,
		Env:             envName,
		LanguageName:    languageName,
		LanguageVersion: languageVersion,
		TracerVersion:   tracerVersion,

		Hostname:         hostname,
		HostOS:           hostOS,
		HostArchitecture: hostArch,
		HostExtra:        hostExtra,

		Payload: apmRawText(env["payload"]),
		Debug:   apmRawText(env["debug"]),
		Origin:  telString(env["origin"]),

		Via:                   c.GetHeader("Via"),
		DDAgentHostname:       c.GetHeader("DD-Agent-Hostname"),
		DDAgentEnv:            c.GetHeader("DD-Agent-Env"),
		DatadogContainerID:    c.GetHeader("Datadog-Container-Id"),
		XDatadogContainerTags: c.GetHeader("X-Datadog-Container-Tags"),

		Extra: apmExtraKeys(env),
	}
}

// telBatch splits a message-batch payload into its entries, each a raw
// {request_type, payload} object.
func telBatch(raw json.RawMessage) ([]map[string]json.RawMessage, error) {
	var batch []map[string]json.RawMessage
	if err := json.Unmarshal(raw, &batch); err != nil {
		return nil, err
	}
	return batch, nil
}

// telProducer names the producer from request_type, falling back to the
// envelope fields that only one producer sets.
func telProducer(env map[string]json.RawMessage, reqType string) string {
	switch reqType {
	case "apm-onboarding-event":
		return "trace-agent"
	case "apm-remote-config-event":
		return "cluster-agent"
	case "agent-metrics", "agent-logs":
		return "agent-telemetry"
	case "app-started", "app-heartbeat", "app-closing", "app-dependencies-loaded",
		"app-integrations-change", "app-client-configuration-change",
		"app-product-change", "app-extended-heartbeat", "generate-metrics",
		"distributions":
		return "tracer"
	}
	// "logs", "traces" and "message-batch" are used by more than one producer;
	// the envelope decides.
	switch {
	case env["event_time"] != nil:
		return "agent-telemetry" // profile events, e.g. agent-bsod
	case env["origin"] != nil:
		return "fleet-installer"
	case env["runtime_id"] != nil:
		return "tracer"
	}
	return "unknown"
}

// telLogApplication logs the fields shared by every producer that sends
// application: service, language and tracer version.
func telLogApplication(raw json.RawMessage) {
	var app map[string]any
	if err := json.Unmarshal(raw, &app); err != nil {
		log.Printf("   application: %v", err)
		return
	}
	log.Printf("   application service=%v version=%v env=%v language=%v/%v tracer=%v",
		app["service_name"], app["service_version"], app["env"],
		app["language_name"], app["language_version"], app["tracer_version"])
}

// telLogHost logs the host block; only hostname is common to all producers.
func telLogHost(raw json.RawMessage) {
	var host map[string]any
	if err := json.Unmarshal(raw, &host); err != nil {
		log.Printf("   host: %v", err)
		return
	}
	log.Printf("   host hostname=%v os=%v arch=%v keys=%s",
		host["hostname"], host["os"], host["architecture"], strings.Join(telKeys(host), ","))
}

// telLogPayload reports what is inside payload without assuming a schema.
// Variants with a known discriminator get it printed; everything else gets
// its top-level keys.
func telLogPayload(reqType string, raw json.RawMessage) {
	if len(raw) == 0 {
		log.Printf("   payload: absent")
		return
	}

	if reqType == "message-batch" {
		batch, err := telBatch(raw)
		if err != nil {
			log.Printf("   payload: batch is not an array (%v)", err)
			return
		}
		log.Printf("   payload: batch of %d", len(batch))
		for _, entry := range batch {
			telLogPayload(telString(entry["request_type"]), entry["payload"])
		}
		return
	}

	var obj map[string]json.RawMessage
	if err := json.Unmarshal(raw, &obj); err != nil {
		// Some tracer variants send a bare array here; report the shape.
		var arr []json.RawMessage
		if json.Unmarshal(raw, &arr) == nil {
			log.Printf("   payload[%s]: array of %d", reqType, len(arr))
			return
		}
		log.Printf("   payload[%s]: %v", reqType, err)
		return
	}

	switch reqType {
	case "apm-onboarding-event", "apm-remote-config-event":
		log.Printf("   payload[%s] event_name=%s keys=%s",
			reqType, telString(obj["event_name"]), strings.Join(telRawKeys(obj), ","))
	case "agent-metrics":
		var metrics map[string]json.RawMessage
		_ = json.Unmarshal(obj["metrics"], &metrics)
		log.Printf("   payload[%s] message=%s metrics=%d: %s",
			reqType, telString(obj["message"]), len(metrics), strings.Join(telRawKeys(metrics), ","))
	case "agent-logs", "logs":
		var logs []json.RawMessage
		_ = json.Unmarshal(obj["logs"], &logs)
		log.Printf("   payload[%s] logs=%d", reqType, len(logs))
	default:
		log.Printf("   payload[%s] keys=%s", reqType, strings.Join(telRawKeys(obj), ","))
	}
}

// telString unquotes a JSON string; anything else comes back verbatim so a
// number or null still shows up in the log.
func telString(raw json.RawMessage) string {
	var s string
	if json.Unmarshal(raw, &s) == nil {
		return s
	}
	return telRaw(raw)
}

func telRaw(raw json.RawMessage) string {
	if len(raw) == 0 {
		return "-"
	}
	return string(raw)
}

func telKeys(m map[string]any) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

func telRawKeys(m map[string]json.RawMessage) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
