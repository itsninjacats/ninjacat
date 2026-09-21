package intake

import (
	"encoding/json"
	"log"
	"net/http"
	"sort"
	"strings"

	"github.com/gin-gonic/gin"
)

// Two small intakes with one endpoint each.
//
//	resources-intake.<site>                   /api/v2/genresources   x-protobuf, opaque
//	instrumentation-telemetry-intake.<site>   /api/v2/apmtelemetry   JSON envelope
//
//	config: none for resources — event platform, `genresources.` prefix.
//	        telemetry is switched off wholesale with DD_TELEMETRY_ENABLED.
//
// Nothing is stored yet.

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
// log size and leading bytes.
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
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	log.Printf("[genresources] origin=%q %d B", c.GetHeader("DD-EVP-ORIGIN"), len(body))
	describe("genresources", c.GetHeader("Content-Type"), body)

	// body is the integration's payload byte for byte, one per request; with
	// no schema published there is nothing deeper to decode into. The origin
	// (DD-EVP-ORIGIN, DD-EVP-ORIGIN-VERSION) and Content-Type are on c.
	_ = body // TODO(ninjacat): tables. Complete, unconverted, ready to take.
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
// so the envelope is read generically. The payload shape depends on
// request_type; only its keys are logged, plus the few fields that identify
// the producer.
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
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	var env map[string]json.RawMessage
	if err := json.Unmarshal(body, &env); err != nil {
		log.Printf("[apmtelemetry] not a JSON object (%v), raw follows", err)
		describe("apmtelemetry", c.GetHeader("Content-Type"), body)
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

	// The log above is a partial view. env is the whole envelope, every key
	// raw; payload is the producer-specific part, its shape fixed by
	// request_type. Held once per producer so storage sees what each one can
	// carry. The proxy headers (Via, DD-Agent-Hostname, DD-Agent-Env,
	// Datadog-Container-ID, X-Datadog-Container-Tags) are still on c.
	payload := env["payload"]
	if reqType == "message-batch" {
		// Tracers and agent telemetry both batch: payload is
		// [{request_type, payload}], each entry's payload shaped like a
		// top-level one of that request_type.
		batch, err := telBatch(payload)
		if err != nil {
			log.Printf("   payload: batch is not an array (%v); kept raw", err)
		}
		_ = batch // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	}
	switch producer {
	case "tracer":
		// Envelope: api_version, request_type, tracer_time, runtime_id,
		// seq_id, application, host, payload, debug. request_type and the
		// payload shape come from the tracer telemetry spec: app-started,
		// app-heartbeat, app-closing, app-dependencies-loaded,
		// app-integrations-change, app-client-configuration-change,
		// app-product-change, app-extended-heartbeat, generate-metrics,
		// distributions, logs, message-batch.
		tracer := payload
		_ = tracer // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case "fleet-installer":
		// The tracer envelope plus origin; request_type is logs or traces.
		installer := payload
		_ = installer // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case "agent-telemetry":
		// Envelope: api_version, request_type, event_time, debug, host,
		// payload. agent-metrics: {message, metrics{name: {...}}};
		// agent-logs: {logs[...]}; message-batch: see above; profile events
		// such as agent-bsod carry their own object.
		agent := payload
		_ = agent // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case "trace-agent":
		// apm-onboarding-event: {event_name, tags, error}.
		onboarding := payload
		_ = onboarding // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case "cluster-agent":
		// apm-remote-config-event: {event_name, tags, error}.
		remoteConfig := payload
		_ = remoteConfig // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	default:
		// A request_type this list does not know, from an envelope that
		// names no producer. Nothing is dropped; it just has no home yet.
		unknown := payload
		_ = unknown // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	}
	_ = env // TODO(ninjacat): tables. Complete, unconverted, ready to take.
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
