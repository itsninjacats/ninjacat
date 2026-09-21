package intake

import (
	"encoding/json"
	"log"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/DataDog/agent-payload/v5/agentdiscovery"
	"github.com/DataDog/agent-payload/v5/healthplatform"
	"github.com/gin-gonic/gin"
	"google.golang.org/protobuf/proto"
)

// Six more single-purpose hosts, one route function each.
//
//	agentdiscovery-intake.<site>     /api/v2/agentdiscovery  protobuf  routeAgentDiscovery
//	agenthealth-intake.<site>        /api/v2/agenthealth     JSON      routeAgentHealth
//	event-management-intake.<site>   /api/v2/events          JSON      routeEventManagement
//	softinv-intake.<site>            /api/v2/softinv         JSON      routeSoftwareInventory
//	http-synthetics.<site>           /api/v2/synthetics      JSON      routeSynthetics
//	data-obs-intake.<site>           /api/v1/lineage         JSON      routeDataObs
//	                                 /api/v2/query-actions   JSON
//
//	config: none for the event platform tracks — DD_SITE only, see
//	        router_containers.go. agenthealth is the exception: a plain
//	        http.Client under `dd_url`. lineage is the trace-agent's reverse
//	        proxy under `ol_proxy_config.dd_url`.
//
// How the event platform frames a body decides what a handler expects:
//
//   - protobuf, or useStreamStrategy → exactly ONE message per request.
//     agentdiscovery (protobuf) and events (JSON, stream) are single objects.
//   - JSON otherwise → BatchStrategy: "[" + raw messages joined by "," + "]".
//     softinv, synthetics and query-actions arrive as arrays.
//
// The agent's `diagnose` sends an EMPTY body down every track it does not
// skip, so an empty body is a probe, not a fault.
//
// Two tracks have importable types and are decoded with them. The rest are
// JSON with no published Go type; those are read generically — identity
// fields by their documented names, everything else as sorted keys — never
// through structs invented here.
//
// Nothing is stored yet. Every handler decodes the WHOLE body and leaves the
// complete result in place at its end, marked TODO(ninjacat): tables. The log
// cap below limits what is printed, never what is decoded or kept. Tracks that
// arrive as a list keep the raw list next to the decoded one, so an item that
// fails to decode is still there, byte for byte, at the same index.

// evpMaxItems caps per-item log lines on tracks that carry long lists. It is
// a log cap only: decoding never stops at it.
const evpMaxItems = 20

// evpBody reads the body and treats an empty one as the diagnostic probe.
func evpBody(c *gin.Context, label string) ([]byte, bool) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[%s] cannot read body: %v", label, err)
		return nil, false
	}
	if len(body) == 0 {
		log.Printf("[%s] empty body — agent diagnose probe?", label)
		return nil, false
	}
	return body, true
}

// evpList decodes a batched JSON track: an array of raw messages, or a bare
// object from a client that skipped the batcher. The result is the complete
// list, one raw message per item, untouched; callers keep it next to whatever
// they decode from it so a failed item never disappears.
func evpList(label string, body []byte) ([]json.RawMessage, bool) {
	items, err := decodeJSONList[json.RawMessage](body)
	if err != nil {
		log.Printf("[%s] json: %v (%d bytes)", label, err, len(body))
		describe(label, "", body)
		return nil, false
	}
	return items, true
}

// evpObject decodes one JSON object into a map. On failure it logs and returns
// false; the caller still holds raw, so nothing is lost by skipping the item.
func evpObject(label string, raw []byte) (map[string]any, bool) {
	var m map[string]any
	if err := json.Unmarshal(raw, &m); err != nil {
		log.Printf("[%s] not a JSON object: %v (%d bytes)", label, err, len(raw))
		return nil, false
	}
	return m, true
}

// evpKeys lists an object's keys, sorted, for logs.
func evpKeys(m map[string]any) string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return strings.Join(keys, ", ")
}

// evpPath walks nested objects: evpPath(m, "data", "attributes", "host").
func evpPath(m map[string]any, path ...string) any {
	var cur any = m
	for _, p := range path {
		obj, ok := cur.(map[string]any)
		if !ok {
			return nil
		}
		cur = obj[p]
	}
	return cur
}

// evpStr renders a nested value for a log line; missing is "-".
func evpStr(m map[string]any, path ...string) string {
	return evpRender(evpPath(m, path...))
}

// evpRender renders one decoded JSON value: scalars as-is, composites as
// their shape. Long strings are cut so one field cannot flood a line.
func evpRender(v any) string {
	switch t := v.(type) {
	case nil:
		return "-"
	case string:
		if t == "" {
			return `""`
		}
		if len(t) > 80 {
			t = t[:77] + "..."
		}
		if strings.ContainsAny(t, " \t\n\"") {
			return strconv.Quote(t)
		}
		return t
	case []any:
		return "[" + strconv.Itoa(len(t)) + " items]"
	case map[string]any:
		return "{" + evpKeys(t) + "}"
	default:
		b, _ := json.Marshal(t)
		return string(b)
	}
}

// evpFields renders an object as sorted "key=value" pairs: every scalar
// verbatim, composites as their shape. This is the shape log for tracks whose
// field names are not documented anywhere we can read.
func evpFields(m map[string]any) string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	parts := make([]string, 0, len(keys))
	for _, k := range keys {
		parts = append(parts, k+"="+evpRender(m[k]))
	}
	return strings.Join(parts, " ")
}

// evpTally counts list items by the value at path: "a=3 b=1", sorted.
func evpTally(items []any, path ...string) string {
	counts := map[string]int{}
	for _, it := range items {
		m, _ := it.(map[string]any)
		counts[evpStr(m, path...)]++
	}
	return evpCounts(counts)
}

// evpCounts renders "key=count" pairs, sorted by key.
func evpCounts(counts map[string]int) string {
	keys := make([]string, 0, len(counts))
	for k := range counts {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	parts := make([]string, 0, len(keys))
	for _, k := range keys {
		parts = append(parts, k+"="+strconv.Itoa(counts[k]))
	}
	return strings.Join(parts, " ")
}

// evpOr renders an empty string as "-".
func evpOr(s string) string {
	if s == "" {
		return "-"
	}
	return s
}

// evpDatasets renders OpenLineage inputs/outputs as namespace/name, capped.
func evpDatasets(v any) string {
	list, _ := v.([]any)
	if len(list) == 0 {
		return "-"
	}
	names := make([]string, 0, min(len(list), evpMaxItems))
	for i, it := range list {
		if i == evpMaxItems {
			names = append(names, "+"+strconv.Itoa(len(list)-i)+" more")
			break
		}
		m, _ := it.(map[string]any)
		names = append(names, evpStr(m, "namespace")+"/"+evpStr(m, "name"))
	}
	return strings.Join(names, ", ")
}

// ---------------------------------------------------------------------------
// agentdiscovery-intake.<site>

// Config files discovery: which integration config files and env vars each
// agent runtime sees. Protobuf, schema in agent-payload.
func (a *Server) routeAgentDiscovery(g *gin.RouterGroup) {
	g.POST("/api/v2/agentdiscovery", a.HandleAgentDiscovery)
}

// HandleAgentDiscovery accepts one AgentDiscoveryPayloadBatch per request.
func (a *Server) HandleAgentDiscovery(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, ok := evpBody(c, "agentdiscovery")
	if !ok {
		return
	}

	var batch agentdiscovery.AgentDiscoveryPayloadBatch
	if err := proto.Unmarshal(body, &batch); err != nil {
		log.Printf("[agentdiscovery] protobuf: %v (%d bytes)", err, len(body))
		return
	}

	if len(batch.Payloads) == 0 {
		// proto.Unmarshal fails OPEN: unknown fields are skipped, so a
		// payload meant for another endpoint decodes into an empty struct.
		// An empty decode is the only signal we get.
		log.Printf("[agentdiscovery] decoded to zero payloads (%d bytes) — wrong payload type?", len(body))
		return
	}

	log.Printf("[agentdiscovery] host_id=%s — %d payloads", batch.HostId, len(batch.Payloads))
	for _, p := range batch.Payloads {
		ingested := "-"
		if ts := p.GetIngestionTimestamp(); ts != nil {
			ingested = ts.AsTime().UTC().Format(time.RFC3339)
		}
		log.Printf("   %-16s runtime=%s/%s ingested=%s config_files=%d env_vars=%d",
			p.Integration, p.Runtime, p.RuntimeId, ingested, len(p.ConfigFiles), len(p.EnvVars))
		for _, f := range p.ConfigFiles {
			log.Printf("      %s %d B %s truncated=%v",
				f.Path, len(f.Content), f.PayloadFormat, f.Truncated)
		}
		// Names only: env var values are the integration's credentials.
		if len(p.EnvVars) > 0 {
			names := make([]string, 0, len(p.EnvVars))
			for _, v := range p.EnvVars {
				names = append(names, v.Name)
			}
			sort.Strings(names)
			log.Printf("      env: %s", strings.Join(names, ", "))
		}
	}

	// batch is the whole AgentDiscoveryPayloadBatch as decoded by proto:
	// every payload with its config files (content included) and env vars
	// (values included — only the log above hides them).
	_ = &batch // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// ---------------------------------------------------------------------------
// agenthealth-intake.<site>

// Health platform: issues the agent found with its own setup.
//
// Not an event platform track. The agent builds the URL itself, marshals the
// proto with encoding/json — so keys are snake_case, enums are numbers — and
// posts ONE HealthReport per request with a plain http.Client.
func (a *Server) routeAgentHealth(g *gin.RouterGroup) {
	g.POST("/api/v2/agenthealth", a.HandleAgentHealth)
}

// HandleAgentHealth accepts one HealthReport per request.
func (a *Server) HandleAgentHealth(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, ok := evpBody(c, "agenthealth")
	if !ok {
		return
	}

	// encoding/json mirrors the sender; structpb.Struct in Issue.Extra has
	// its own UnmarshalJSON, so the nested Struct decodes too.
	var report healthplatform.HealthReport
	if err := json.Unmarshal(body, &report); err != nil {
		log.Printf("[agenthealth] json: %v (%d bytes)", err, len(body))
		describe("agenthealth", c.GetHeader("Content-Type"), body)
		return
	}

	log.Printf("[agenthealth] host=%s agent=%s service=%s type=%s schema=%s at=%s — %d issues",
		report.GetHost().GetHostname(), report.GetHost().GetAgentVersion(),
		report.Service, report.EventType, report.SchemaVersion, report.EmittedAt, len(report.Issues))

	// Location is the agent component that has the problem (logs-agent,
	// system-probe, ...); source is the check that found it.
	byLocation := map[string]int{}
	ids := make([]string, 0, len(report.Issues))
	for id, issue := range report.Issues {
		ids = append(ids, id)
		byLocation[evpOr(issue.GetLocation())]++
	}
	sort.Strings(ids)
	if len(byLocation) > 0 {
		log.Printf("   components: %s", evpCounts(byLocation))
	}

	for _, id := range ids {
		issue := report.Issues[id]
		log.Printf("   %-40s %s %s in=%s %s/%s source=%s %q",
			id,
			strings.TrimPrefix(issue.GetSeverity().String(), "ISSUE_SEVERITY_"),
			strings.TrimPrefix(issue.GetPersistedIssue().GetState().String(), "ISSUE_STATE_"),
			evpOr(issue.GetLocation()), evpOr(issue.GetCategory()), evpOr(issue.GetIssueName()),
			evpOr(issue.GetSource()), issue.GetTitle())

		lifecycle := issue.GetPersistedIssue()
		log.Printf("      detected=%s first_seen=%s last_seen=%s tags=%d extra={%s} remediation=%s",
			evpOr(issue.GetDetectedAt()), evpOr(lifecycle.GetFirstSeen()),
			evpOr(lifecycle.GetLastSeen()), len(issue.GetTags()),
			evpStructKeys(issue.GetExtra().GetFields()), evpRemediation(issue.GetRemediation()))
	}

	// report is the whole HealthReport: host, every issue keyed by id with
	// its lifecycle, tags, extra Struct and remediation, as the agent sent it.
	_ = &report // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// evpStructKeys lists the keys of a structpb.Struct's fields, sorted.
func evpStructKeys[V any](fields map[string]V) string {
	keys := make([]string, 0, len(fields))
	for k := range fields {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return strings.Join(keys, ", ")
}

// evpRemediation summarises what the agent proposes to fix an issue.
func evpRemediation(r *healthplatform.Remediation) string {
	if r == nil {
		return "-"
	}
	s := strconv.Itoa(len(r.Steps)) + " steps"
	if r.Script != nil {
		s += " +" + r.Script.Language + " script"
	}
	if r.Summary != "" {
		s += " " + strconv.Quote(r.Summary)
	}
	return s
}

// ---------------------------------------------------------------------------
// event-management-intake.<site>

// Event Management: notable events, logon duration, anomaly notifications.
//
// JSON but useStreamStrategy, so ONE JSON:API-like envelope per request:
//
//	{"data": {"type": "event", "attributes": {host, title, category,
//	          integration_id, message, timestamp, tags, aggregation_key,
//	          attributes: {...}, "system-notable-events": {event_type}}}}
//
// No Go type: each producer builds its own map[string]any. The inner
// `attributes` is per producer: {status, priority, custom} for notable
// events, {changed_resource, author, prev_value, ...} for change events.
func (a *Server) routeEventManagement(g *gin.RouterGroup) {
	g.POST("/api/v2/events", a.HandleEventManagement)
}

// HandleEventManagement accepts one event envelope per request.
func (a *Server) HandleEventManagement(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, ok := evpBody(c, "events")
	if !ok {
		return
	}

	m, ok := evpObject("events", body)
	if !ok {
		describe("events", c.GetHeader("Content-Type"), body)
		return
	}

	attrs, _ := evpPath(m, "data", "attributes").(map[string]any)
	log.Printf("[events] type=%s integration=%s category=%s event_type=%s host=%s at=%s title=%s",
		evpStr(m, "data", "type"), evpStr(attrs, "integration_id"),
		evpStr(attrs, "category"), evpStr(attrs, "system-notable-events", "event_type"),
		evpStr(attrs, "host"), evpStr(attrs, "timestamp"), evpStr(attrs, "title"))
	log.Printf("   tags=%s aggregation_key=%s message=%s keys: %s",
		evpStr(attrs, "tags"), evpStr(attrs, "aggregation_key"), evpStr(attrs, "message"),
		evpKeys(attrs))
	if inner, ok := attrs["attributes"].(map[string]any); ok {
		log.Printf("   attributes: %s", evpFields(inner))
	}

	// m is the whole envelope, decoded: data.type, every attribute the
	// producer set, and the producer-specific inner attributes.
	_ = m // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// ---------------------------------------------------------------------------
// softinv-intake.<site>

// Software inventory: installed packages per host.
//
// Batched JSON: an array of {hostname, host_software: {software: [...]}}. The
// type lives in comp/softwareinventory/impl, not importable; each entry is a
// pkg/inventory/software wire entry: software_type, name, version, publisher,
// deployment_status, deployment_time, product_code, is_64_bit, install_paths.
func (a *Server) routeSoftwareInventory(g *gin.RouterGroup) {
	g.POST("/api/v2/softinv", a.HandleSoftwareInventory)
}

// HandleSoftwareInventory accepts an array of inventory payloads.
func (a *Server) HandleSoftwareInventory(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, ok := evpBody(c, "softinv")
	if !ok {
		return
	}

	items, ok := evpList("softinv", body)
	if !ok {
		return
	}

	// Every payload is decoded; the per-entry cap below only limits what is
	// printed. A payload that fails to decode is skipped here but is still in
	// items[i], raw.
	payloads := make([]map[string]any, 0, len(items))
	log.Printf("[softinv] %d payloads (%d bytes)", len(items), len(body))
	for _, raw := range items {
		m, ok := evpObject("softinv", raw)
		if !ok {
			continue
		}
		payloads = append(payloads, m)
		entries, _ := evpPath(m, "host_software", "software").([]any)
		log.Printf("   host=%s software=%d keys: %s", evpStr(m, "hostname"), len(entries), evpKeys(m))
		if len(entries) == 0 {
			continue
		}
		log.Printf("      by type: %s | by status: %s",
			evpTally(entries, "software_type"), evpTally(entries, "deployment_status"))
		// A full host inventory runs to hundreds of lines; the first few
		// show the entry shape, the tallies show the rest.
		for i, e := range entries {
			if i == evpMaxItems {
				log.Printf("      ... %d more", len(entries)-i)
				break
			}
			em, _ := e.(map[string]any)
			log.Printf("      %s %s %s publisher=%s status=%s keys: %s",
				evpStr(em, "software_type"), evpStr(em, "name"), evpStr(em, "version"),
				evpStr(em, "publisher"), evpStr(em, "deployment_status"), evpKeys(em))
		}
	}

	// payloads is every inventory payload decoded, with its full software
	// list — not the first evpMaxItems the log shows. items is the same list
	// raw, one message per payload, including any that failed to decode.
	_ = payloads // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = items    // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// ---------------------------------------------------------------------------
// http-synthetics.<site>

// Synthetics: results of network tests the agent ran on the server's behalf.
//
// Batched JSON: an array of common.TestResult from
// comp/syntheticstestscheduler — not importable:
//
//	{test: {id, name, type, subType, version}, location: {id, name, displayName},
//	 result: {id, initialId, status, runType, duration, testStartedAt,
//	          testFinishedAt, testTriggeredAt, assertions: [{type, operator,
//	          expected, actual, valid}], failure: {code, message},
//	          config: {request: {host, port, ...}}, netstats: {packetsSent,
//	          packetsReceived, packetLossPercentage, jitter, latency, hops},
//	          netpath: payload.NetworkPath},
//	 _dd: {...}, enrichment: opaque, v}
//
// netpath is the same type netpath-intake carries.
func (a *Server) routeSynthetics(g *gin.RouterGroup) {
	g.POST("/api/v2/synthetics", a.HandleSynthetics)
}

// HandleSynthetics accepts an array of test results.
func (a *Server) HandleSynthetics(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, ok := evpBody(c, "synthetics")
	if !ok {
		return
	}

	items, ok := evpList("synthetics", body)
	if !ok {
		return
	}

	// Every result is decoded. One that fails to decode is skipped here but
	// is still in items[i], raw.
	results := make([]map[string]any, 0, len(items))
	log.Printf("[synthetics] %d results (%d bytes)", len(items), len(body))
	for _, raw := range items {
		m, ok := evpObject("synthetics", raw)
		if !ok {
			continue
		}
		results = append(results, m)
		res, _ := m["result"].(map[string]any)
		log.Printf("   test=%s name=%s %s/%s v=%s location=%s result=%s status=%s run=%s duration=%s keys: %s",
			evpStr(m, "test", "id"), evpStr(m, "test", "name"),
			evpStr(m, "test", "type"), evpStr(m, "test", "subType"), evpStr(m, "v"),
			evpStr(m, "location", "id"), evpStr(res, "id"), evpStr(res, "status"),
			evpStr(res, "runType"), evpStr(res, "duration"), evpKeys(m))

		assertions, _ := res["assertions"].([]any)
		valid := 0
		for _, as := range assertions {
			if am, _ := as.(map[string]any); am != nil && am["valid"] == true {
				valid++
			}
		}
		log.Printf("      target=%s:%s assertions=%d/%d packets=%s/%s loss=%s%% netpath=%s failure=%s %s",
			evpStr(res, "config", "request", "host"), evpStr(res, "config", "request", "port"),
			valid, len(assertions),
			evpStr(res, "netstats", "packetsReceived"), evpStr(res, "netstats", "packetsSent"),
			evpStr(res, "netstats", "packetLossPercentage"), evpStr(res, "netpath", "hops"),
			evpStr(res, "failure", "code"), evpStr(res, "failure", "message"))
	}

	// results is every test result decoded: test, location, the full result
	// with all assertions, netstats and netpath, _dd and enrichment. items is
	// the same list raw, including any that failed to decode.
	_ = results // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = items   // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// ---------------------------------------------------------------------------
// data-obs-intake.<site>

// Data Observability: two producers on one host.
//
//	/api/v1/lineage        OpenLineage RunEvents, forwarded verbatim by the
//	                       trace-agent's reverse proxy (ol_proxy_config).
//	                       ?api-version=2 when the agent is told to add it.
//	/api/v2/query-actions  results of Data Observability query actions from
//	                       Python integrations, batched JSON.
//
// The lineage proxy carries the API key as "Authorization: Bearer <key>",
// the OpenLineage client's convention, NOT Dd-Api-Key. RequireAPIKey does not
// read that header yet, so this route answers 403 until it does.
func (a *Server) routeDataObs(g *gin.RouterGroup) {
	g.POST("/api/v1/lineage", a.HandleOpenLineage)
	g.POST("/api/v2/query-actions", a.HandleQueryActions)
}

// HandleOpenLineage accepts OpenLineage events.
//
// The OpenLineage HTTP transport posts one RunEvent per request; an array is
// tolerated in case a client batches. Field names follow the published spec
// (RunEvent in the Go client's spec.gen.go): eventType, eventTime, producer,
// schemaURL, run.runId, job.namespace/name, inputs[]/outputs[] datasets with
// namespace/name, and facets on run, job and each dataset.
//
// The OpenLineage Go client is not used on purpose: its openlineage package
// imports its transport package, which drags cloud.google.com/go/datacatalog
// and google.golang.org/api into the binary for what is one JSON object.
func (a *Server) HandleOpenLineage(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, ok := evpBody(c, "lineage")
	if !ok {
		return
	}

	items, ok := evpList("lineage", body)
	if !ok {
		return
	}

	// Every event is decoded; evpDatasets caps only the rendered names. An
	// event that fails to decode is skipped here but is still in items[i], raw.
	events := make([]map[string]any, 0, len(items))
	log.Printf("[lineage] api-version=%s via=%q — %d events (%d bytes)",
		evpOr(c.Query("api-version")), c.GetHeader("Via"), len(items), len(body))
	for _, raw := range items {
		m, ok := evpObject("lineage", raw)
		if !ok {
			continue
		}
		events = append(events, m)
		log.Printf("   %-8s job=%s/%s run=%s at=%s producer=%s schema=%s keys: %s",
			evpStr(m, "eventType"), evpStr(m, "job", "namespace"), evpStr(m, "job", "name"),
			evpStr(m, "run", "runId"), evpStr(m, "eventTime"), evpStr(m, "producer"),
			evpStr(m, "schemaURL"), evpKeys(m))
		log.Printf("      inputs: %s | outputs: %s | run facets: %s | job facets: %s",
			evpDatasets(m["inputs"]), evpDatasets(m["outputs"]),
			evpStr(m, "run", "facets"), evpStr(m, "job", "facets"))
	}

	// events is every RunEvent decoded, with all inputs, outputs and facets
	// on run, job and datasets. items is the same list raw, including any
	// that failed to decode. This is the /api/v1/lineage half of data-obs;
	// query-actions keeps its own list below.
	_ = events // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = items  // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// HandleQueryActions accepts an array of query-action results.
//
// The agent side (comp/dataobs/queryactions) only schedules the checks from
// remote config; the results come from Python integrations, so no Go type
// documents them. Every top-level field is logged as sent.
func (a *Server) HandleQueryActions(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, ok := evpBody(c, "query-actions")
	if !ok {
		return
	}

	items, ok := evpList("query-actions", body)
	if !ok {
		return
	}

	// Every entry is decoded — the cap only limits what is printed. An entry
	// that fails to decode is skipped here but is still in items[i], raw.
	results := make([]map[string]any, 0, len(items))
	log.Printf("[query-actions] %d entries (%d bytes)", len(items), len(body))
	for i, raw := range items {
		m, ok := evpObject("query-actions", raw)
		if !ok {
			continue
		}
		results = append(results, m)
		if i < evpMaxItems {
			log.Printf("   %s", evpFields(m))
		} else if i == evpMaxItems {
			log.Printf("   ... %d more", len(items)-i)
		}
	}

	// results is every query-action result decoded, every field as the
	// integration sent it. items is the same list raw, including any that
	// failed to decode. This is the /api/v2/query-actions half of data-obs;
	// lineage keeps its own list above.
	_ = results // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = items   // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}
