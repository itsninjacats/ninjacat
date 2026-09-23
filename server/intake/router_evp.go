package intake

import (
	"bytes"
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
	"github.com/google/uuid"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/structpb"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/itsninjacats/server/apps/storage"
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
// Stored: agent_discovery, agent_health_reports + agent_health_issues,
// event_management_events, host_software, synthetics_results,
// openlineage_events, query_action_results — see
// server/schema/migrations/0010_evp.sql and server/docs/tables/evp.md. A body
// that fails to decode, or a list element that fails evpObject, goes to
// storeRaw rather than being silently dropped; decode success paths never
// call storeRaw. The log lines below predate storage and stay — they are how
// this protocol was reverse-engineered — but logging is no longer the only
// sink.

// evpMaxItems caps per-item log lines on tracks that carry long lists. It is
// a log cap only: decoding never stops at it, and neither does storage.
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
//
// Decoded with UseNumber: several tracks here carry ids and counters whose
// wire type is JSON, and a plain float64 loses precision above 2^53 — the
// same rule the rest of the intake applies to uint64 ids. json.Number
// re-marshals to the exact literal it was decoded from, which is what lets
// evpJSON below store nested objects losslessly.
func evpObject(label string, raw []byte) (map[string]any, bool) {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	var m map[string]any
	if err := dec.Decode(&m); err != nil {
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

// evpStr renders a nested value for a LOG line; missing is "-", long strings
// are cut. Never use this to build a stored value — see evpAsString.
func evpStr(m map[string]any, path ...string) string {
	return evpRender(evpPath(m, path...))
}

// evpRender renders one decoded JSON value for a LOG line: scalars as-is,
// composites as their shape. Long strings are cut so one field cannot flood
// a line. This is a logging helper; storage uses evpAsString/evpJSON, which
// never truncate.
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
// Logging only — the cap never limits what datasetArrays stores.
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
// Storage conversion helpers — exact values, never the truncated/rendered
// form the log helpers above produce.

// evpAsString renders a decoded JSON value as the string a column holds:
// a string verbatim, a json.Number by its exact literal (never through
// float64), a bool as "true"/"false", nil as "", anything else re-marshaled.
func evpAsString(v any) string {
	switch t := v.(type) {
	case nil:
		return ""
	case string:
		return t
	case json.Number:
		return t.String()
	case bool:
		if t {
			return "true"
		}
		return "false"
	default:
		b, _ := json.Marshal(t)
		return string(b)
	}
}

// evpFieldStr reads one key of an object as a string; a nil object (a path
// that did not resolve to a map) yields "" rather than panicking.
func evpFieldStr(m map[string]any, key string) string {
	if m == nil {
		return ""
	}
	return evpAsString(m[key])
}

// evpPathStr is evpFieldStr for a nested path.
func evpPathStr(m map[string]any, path ...string) string {
	return evpAsString(evpPath(m, path...))
}

// evpFieldValue reads one key of an object, nil-safe when m is nil.
func evpFieldValue(m map[string]any, key string) any {
	if m == nil {
		return nil
	}
	return m[key]
}

// evpJSON re-marshals a decoded value for a JSON-text column. nil renders as
// "" (no value at all), distinct from an explicit JSON null or {}.
func evpJSON(v any) string {
	if v == nil {
		return ""
	}
	b, err := json.Marshal(v)
	if err != nil {
		return ""
	}
	return string(b)
}

// evpStringSlice reads a value as a string list: a JSON array renders each
// element with evpAsString, a bare string is a one-element list (some
// producers send a single tag as a scalar), anything else is empty.
func evpStringSlice(v any) []string {
	switch t := v.(type) {
	case []any:
		if len(t) == 0 {
			return nil
		}
		out := make([]string, 0, len(t))
		for _, it := range t {
			out = append(out, evpAsString(it))
		}
		return out
	case string:
		if t == "" {
			return nil
		}
		return []string{t}
	default:
		return nil
	}
}

// evpBoolPtr reads a value as *uint8 for a Nullable(UInt8) boolean column:
// nil when the value is absent or not a JSON bool, distinguishing "the
// producer never said" from "false".
func evpBoolPtr(v any) *uint8 {
	b, ok := v.(bool)
	if !ok {
		return nil
	}
	u := boolToUint8(b)
	return &u
}

// evpInt64Ptr reads a value as *int64 for a Nullable(Int64) column, using
// json.Number so a large id or counter never rounds through float64.
func evpInt64Ptr(v any) *int64 {
	n, ok := v.(json.Number)
	if !ok {
		return nil
	}
	i, err := n.Int64()
	if err != nil {
		return nil
	}
	return &i
}

// evpFloat64Ptr is evpInt64Ptr for a Nullable(Float64) column.
func evpFloat64Ptr(v any) *float64 {
	n, ok := v.(json.Number)
	if !ok {
		return nil
	}
	f, err := n.Float64()
	if err != nil {
		return nil
	}
	return &f
}

// evpParseTime parses a wire timestamp string as RFC3339 (with or without
// fractional seconds). A string that is empty or does not parse yields nil —
// never a fallback to now(), which would invent a timestamp nobody sent. The
// raw string is kept in its own column regardless, so nothing is lost by a
// format this does not recognise.
func evpParseTime(s string) *time.Time {
	if s == "" {
		return nil
	}
	for _, layout := range []string{time.RFC3339Nano, time.RFC3339} {
		if t, err := time.Parse(layout, s); err == nil {
			u := t.UTC()
			return &u
		}
	}
	return nil
}

// evpUnknownMap builds a Map(String,String) of the keys of m NOT in known,
// each value re-marshaled to JSON so a caller can hold a nested object, an
// array or a scalar uniformly. Returns nil (not an empty map) when there is
// nothing undeclared, matching orEmpty's contract at the storage layer.
func evpUnknownMap(m map[string]any, known map[string]bool) map[string]string {
	if len(m) == 0 {
		return nil
	}
	var out map[string]string
	for k, v := range m {
		if known[k] {
			continue
		}
		if out == nil {
			out = make(map[string]string, len(m))
		}
		out[k] = evpJSON(v)
	}
	return out
}

// evpSortedKeys lists an object's top-level keys, sorted — the stored
// counterpart of evpKeys, which renders them joined for a log line.
func evpSortedKeys(m map[string]any) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

func boolToUint8(b bool) uint8 {
	if b {
		return 1
	}
	return 0
}

// evpTimestampPtr converts a protobuf Timestamp, ptr-optional on the wire,
// to a *time.Time: nil stays nil rather than becoming the Unix epoch.
func evpTimestampPtr(ts *timestamppb.Timestamp) *time.Time {
	if ts == nil {
		return nil
	}
	t := ts.AsTime().UTC()
	return &t
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
		a.storeRaw(c, "agentdiscovery", "decode_error", err.Error(), body)
		return
	}

	if len(batch.Payloads) == 0 {
		// proto.Unmarshal fails OPEN: unknown fields are skipped, so a
		// payload meant for another endpoint decodes into an empty struct.
		// An empty decode is the only signal we get — not a decode error,
		// so no storeRaw: see the intake audit's note on fail-open guards.
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

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := agentDiscoveryRows(tenant, time.Now().UTC(), &batch)
	a.store(storage.AgentDiscoveryWriter, storage.WriteAgentDiscovery{Rows: rows}, len(rows))
}

// agentDiscoveryRows converts one decoded batch into agent_discovery rows,
// one per payload. HostId is the batch's own identity and repeats onto every
// row — the batch itself gets no separate table.
func agentDiscoveryRows(tenant string, receivedAt time.Time, batch *agentdiscovery.AgentDiscoveryPayloadBatch) []storage.AgentDiscoveryRow {
	payloads := batch.GetPayloads()
	rows := make([]storage.AgentDiscoveryRow, 0, len(payloads))
	for _, p := range payloads {
		if p == nil {
			continue
		}
		row := storage.AgentDiscoveryRow{
			TenantID:           tenant,
			ReceivedAt:         receivedAt,
			HostID:             batch.GetHostId(),
			Integration:        p.GetIntegration(),
			Runtime:            p.GetRuntime(),
			RuntimeID:          p.GetRuntimeId(),
			IngestionTimestamp: evpTimestampPtr(p.GetIngestionTimestamp()),
			EnvVars:            agentDiscoveryEnvVars(p.GetEnvVars()),
		}
		for _, f := range p.GetConfigFiles() {
			if f == nil {
				continue
			}
			row.ConfigPaths = append(row.ConfigPaths, f.GetPath())
			row.ConfigContents = append(row.ConfigContents, string(f.GetContent()))
			row.ConfigTruncated = append(row.ConfigTruncated, boolToUint8(f.GetTruncated()))
			row.ConfigFormats = append(row.ConfigFormats, f.GetPayloadFormat().String())
		}
		rows = append(rows, row)
	}
	return rows
}

// agentDiscoveryEnvVars keeps VALUES, not just names — see the migration's
// header comment for why that is deliberate rather than an oversight.
func agentDiscoveryEnvVars(vars []*agentdiscovery.AgentDiscoveryEnvVar) map[string]string {
	if len(vars) == 0 {
		return nil
	}
	out := make(map[string]string, len(vars))
	for _, v := range vars {
		if v == nil {
			continue
		}
		out[v.GetName()] = v.GetValue()
	}
	return out
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
		a.storeRaw(c, "agenthealth", "decode_error", err.Error(), body)
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

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	reportRow, issueRows := healthReportRows(tenant, time.Now().UTC(), uuid.New(), &report)
	a.store(storage.AgentHealthReportsWriter,
		storage.WriteAgentHealthReports{Rows: []storage.AgentHealthReportRow{reportRow}}, 1)
	a.store(storage.AgentHealthIssuesWriter,
		storage.WriteAgentHealthIssues{Rows: issueRows}, len(issueRows))
}

// healthReportRows converts one decoded HealthReport into its report row and
// the issue rows keyed off its Issues map. reportID is generated by the
// caller (the wire has no report-level id of its own) and is what joins the
// two tables.
//
// Issue keys are sorted before rows are built: Go map iteration order is
// random, and two ingests of the same report must produce the same row
// order, or a diff between them would be meaningless.
func healthReportRows(tenant string, receivedAt time.Time, reportID uuid.UUID, report *healthplatform.HealthReport) (storage.AgentHealthReportRow, []storage.AgentHealthIssueRow) {
	var hostname string
	var agentVersion *string
	var parIDs []string
	if host := report.GetHost(); host != nil {
		hostname = host.GetHostname()
		agentVersion = host.AgentVersion // ptr-optional field, host is non-nil here
		parIDs = host.GetParIds()
	}

	emittedAt := report.GetEmittedAt()
	reportRow := storage.AgentHealthReportRow{
		TenantID:        tenant,
		ReceivedAt:      receivedAt,
		ReportID:        reportID,
		SchemaVersion:   report.GetSchemaVersion(),
		EventType:       report.GetEventType(),
		EmittedAt:       emittedAt,
		EmittedAtParsed: evpParseTime(emittedAt),
		Service:         report.GetService(),
		Host:            hostname,
		AgentVersion:    agentVersion,
		ParIDs:          parIDs,
		IssueCount:      uint32(len(report.GetIssues())),
	}

	issues := report.GetIssues()
	ids := make([]string, 0, len(issues))
	for id := range issues {
		ids = append(ids, id)
	}
	sort.Strings(ids)

	issueRows := make([]storage.AgentHealthIssueRow, 0, len(ids))
	for _, id := range ids {
		issueRows = append(issueRows, healthIssueRow(tenant, receivedAt, reportID, id, issues[id]))
	}
	return reportRow, issueRows
}

// healthIssueRow converts one entry of a HealthReport's Issues map.
func healthIssueRow(tenant string, receivedAt time.Time, reportID uuid.UUID, key string, issue *healthplatform.Issue) storage.AgentHealthIssueRow {
	detectedAt := issue.GetDetectedAt()
	row := storage.AgentHealthIssueRow{
		TenantID:         tenant,
		ReceivedAt:       receivedAt,
		ReportID:         reportID,
		IssueKey:         key,
		ID:               issue.GetId(),
		IssueName:        issue.GetIssueName(),
		Title:            issue.GetTitle(),
		Description:      issue.GetDescription(),
		Category:         issue.GetCategory(),
		Location:         issue.GetLocation(),
		Severity:         issue.GetSeverity().String(),
		DetectedAt:       detectedAt,
		DetectedAtParsed: evpParseTime(detectedAt),
		Source:           issue.GetSource(),
		Extra:            evpStructJSON(issue.GetExtra()),
		Tags:             tagsToMultiMap(issue.GetTags()),
		IssueType:        issue.GetIssueType(),
	}

	if rem := issue.GetRemediation(); rem != nil {
		row.RemediationSummary = rem.GetSummary()
		for _, step := range rem.GetSteps() {
			if step == nil {
				continue
			}
			row.RemediationStepOrder = append(row.RemediationStepOrder, step.GetOrder())
			row.RemediationStepText = append(row.RemediationStepText, step.GetText())
		}
		if script := rem.GetScript(); script != nil {
			row.ScriptLanguage = script.GetLanguage()
			row.ScriptLanguageVersion = script.GetLanguageVersion()
			row.ScriptFilename = script.GetFilename()
			row.ScriptRequiresRoot = boolToUint8(script.GetRequiresRoot())
			row.ScriptContent = script.GetContent()
		}
	}

	if lifecycle := issue.GetPersistedIssue(); lifecycle != nil {
		row.PersistedState = lifecycle.GetState().String()
		row.FirstSeen = lifecycle.GetFirstSeen()
		row.LastSeen = lifecycle.GetLastSeen()
		row.ResolvedAt = lifecycle.ResolvedAt // ptr-optional field, lifecycle is non-nil here
	}

	return row
}

// evpStructJSON renders a structpb.Struct (Issue.Extra: genuinely schemaless
// per issue) as JSON text.
func evpStructJSON(s *structpb.Struct) string {
	if s == nil {
		return ""
	}
	return evpJSON(s.AsMap())
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

// eventManagementKnownKeys are the data.attributes keys the intake documents
// by name; anything else lands in OuterExtra.
var eventManagementKnownKeys = map[string]bool{
	"host": true, "title": true, "category": true, "integration_id": true,
	"message": true, "timestamp": true, "tags": true, "aggregation_key": true,
	"attributes": true, "system-notable-events": true,
}

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
		a.storeRaw(c, "events", "decode_error", "body is not a JSON object", body)
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

	row, ok := eventManagementRow(m)
	if !ok {
		// data.attributes was missing or not an object — the one shape this
		// track's envelope requires and does not have.
		a.storeRaw(c, "events", "unexpected_shape", "data.attributes is missing or not an object", body)
		return
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	row.TenantID = tenant
	row.ReceivedAt = time.Now().UTC()
	a.store(storage.EventManagementWriter, storage.WriteEventManagementEvents{Rows: []storage.EventManagementEventRow{row}}, 1)
}

// eventManagementRow converts one decoded envelope. TenantID/ReceivedAt are
// left zero-valued — the caller fills them once the tenant is known, so this
// function stays testable without a context.
func eventManagementRow(m map[string]any) (storage.EventManagementEventRow, bool) {
	attrs, ok := evpPath(m, "data", "attributes").(map[string]any)
	if !ok {
		return storage.EventManagementEventRow{}, false
	}
	inner, _ := attrs["attributes"].(map[string]any)
	timestamp := evpFieldStr(attrs, "timestamp")

	row := storage.EventManagementEventRow{
		DataType:         evpPathStr(m, "data", "type"),
		Host:             evpFieldStr(attrs, "host"),
		Title:            evpFieldStr(attrs, "title"),
		Category:         evpFieldStr(attrs, "category"),
		IntegrationID:    evpFieldStr(attrs, "integration_id"),
		Message:          evpFieldStr(attrs, "message"),
		Timestamp:        timestamp,
		TimestampParsed:  evpParseTime(timestamp),
		Tags:             tagsToMultiMap(evpStringSlice(attrs["tags"])),
		AggregationKey:   evpFieldStr(attrs, "aggregation_key"),
		NotableEventType: evpPathStr(attrs, "system-notable-events", "event_type"),
		Attributes:       evpJSON(inner),
		OuterExtra:       evpUnknownMap(attrs, eventManagementKnownKeys),
	}
	return row, true
}

// ---------------------------------------------------------------------------
// softinv-intake.<site>

// softinvEntryKnownKeys are the pkg/inventory/software fields this intake
// documents; anything else on a software entry lands in Extra.
var softinvEntryKnownKeys = map[string]bool{
	"software_type": true, "name": true, "version": true, "publisher": true,
	"deployment_status": true, "deployment_time": true, "product_code": true,
	"is_64_bit": true, "install_paths": true,
}

// softinvPayloadKnownKeys are the payload-level fields this intake
// documents; anything else lands in PayloadExtra.
var softinvPayloadKnownKeys = map[string]bool{"hostname": true, "host_software": true}

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
		a.storeRaw(c, "softinv", "unexpected_shape", "body is not a JSON array or object", body)
		return
	}

	tenant := TenantFromContext(c)
	now := time.Now().UTC()

	// Every payload is decoded; the per-entry cap below only limits what is
	// printed. A payload that fails to decode is skipped here but is still in
	// items[i], raw, and reaches storeRaw.
	payloads := make([]map[string]any, 0, len(items))
	var rows []storage.HostSoftwareRow
	log.Printf("[softinv] %d payloads (%d bytes)", len(items), len(body))
	for _, raw := range items {
		m, ok := evpObject("softinv", raw)
		if !ok {
			a.storeRaw(c, "softinv", "decode_error", "item is not a JSON object", raw)
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
		if tenant != "" {
			rows = append(rows, hostSoftwareRows(tenant, now, m)...)
		}
	}

	if tenant == "" {
		return
	}
	a.store(storage.HostSoftwareWriter, storage.WriteHostSoftware{Rows: rows}, len(rows))
}

// hostSoftwareRows converts one decoded softinv payload into one row per
// software entry. A software entry that is not itself a JSON object is
// skipped — see docs/tables/evp.md's dropped_by_decision list.
func hostSoftwareRows(tenant string, receivedAt time.Time, payload map[string]any) []storage.HostSoftwareRow {
	hostname := evpFieldStr(payload, "hostname")
	payloadExtra := evpUnknownMap(payload, softinvPayloadKnownKeys)

	entries, _ := evpPath(payload, "host_software", "software").([]any)
	rows := make([]storage.HostSoftwareRow, 0, len(entries))
	for _, e := range entries {
		em, ok := e.(map[string]any)
		if !ok {
			continue
		}
		deployTime := evpFieldStr(em, "deployment_time")
		rows = append(rows, storage.HostSoftwareRow{
			TenantID:             tenant,
			ReceivedAt:           receivedAt,
			Hostname:             hostname,
			SoftwareType:         evpFieldStr(em, "software_type"),
			Name:                 evpFieldStr(em, "name"),
			Version:              evpFieldStr(em, "version"),
			Publisher:            evpFieldStr(em, "publisher"),
			DeploymentStatus:     evpFieldStr(em, "deployment_status"),
			DeploymentTime:       deployTime,
			DeploymentTimeParsed: evpParseTime(deployTime),
			ProductCode:          evpFieldStr(em, "product_code"),
			Is64Bit:              evpBoolPtr(em["is_64_bit"]),
			InstallPaths:         evpStringSlice(em["install_paths"]),
			Extra:                evpUnknownMap(em, softinvEntryKnownKeys),
			PayloadExtra:         payloadExtra,
		})
	}
	return rows
}

// ---------------------------------------------------------------------------
// http-synthetics.<site>

// syntheticsKnownKeys are the top-level result object keys this intake
// documents; anything else lands in Extra.
var syntheticsKnownKeys = map[string]bool{
	"test": true, "location": true, "result": true, "_dd": true,
	"enrichment": true, "v": true,
}

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
		a.storeRaw(c, "synthetics", "unexpected_shape", "body is not a JSON array or object", body)
		return
	}

	tenant := TenantFromContext(c)
	now := time.Now().UTC()

	// Every result is decoded. One that fails to decode is skipped here but
	// is still in items[i], raw, and reaches storeRaw.
	results := make([]map[string]any, 0, len(items))
	var rows []storage.SyntheticsResultRow
	log.Printf("[synthetics] %d results (%d bytes)", len(items), len(body))
	for _, raw := range items {
		m, ok := evpObject("synthetics", raw)
		if !ok {
			a.storeRaw(c, "synthetics", "decode_error", "item is not a JSON object", raw)
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

		if tenant != "" {
			rows = append(rows, syntheticsResultRow(m))
		}
	}

	if tenant == "" {
		return
	}
	for i := range rows {
		rows[i].TenantID = tenant
		rows[i].ReceivedAt = now
	}
	a.store(storage.SyntheticsResultsWriter, storage.WriteSyntheticsResults{Rows: rows}, len(rows))
}

// syntheticsResultRow converts one decoded test result. TenantID/ReceivedAt
// are left zero-valued for the same reason as eventManagementRow.
func syntheticsResultRow(m map[string]any) storage.SyntheticsResultRow {
	test, _ := m["test"].(map[string]any)
	location, _ := m["location"].(map[string]any)
	res, _ := m["result"].(map[string]any)
	netstats, _ := res["netstats"].(map[string]any)

	assertions, _ := res["assertions"].([]any)
	var aType, aOp, aExpected, aActual []string
	var aValid []uint8
	for _, raw := range assertions {
		am, _ := raw.(map[string]any)
		aType = append(aType, evpFieldStr(am, "type"))
		aOp = append(aOp, evpFieldStr(am, "operator"))
		aExpected = append(aExpected, evpJSON(evpFieldValue(am, "expected")))
		aActual = append(aActual, evpJSON(evpFieldValue(am, "actual")))
		valid, _ := am["valid"].(bool)
		aValid = append(aValid, boolToUint8(valid))
	}

	var failureCode, failureMessage *string
	if failure, ok := res["failure"].(map[string]any); ok {
		code := evpFieldStr(failure, "code")
		msg := evpFieldStr(failure, "message")
		failureCode, failureMessage = &code, &msg
	}

	started := evpFieldStr(res, "testStartedAt")
	finished := evpFieldStr(res, "testFinishedAt")
	triggered := evpFieldStr(res, "testTriggeredAt")

	return storage.SyntheticsResultRow{
		TestID:      evpFieldStr(test, "id"),
		TestName:    evpFieldStr(test, "name"),
		TestType:    evpFieldStr(test, "type"),
		TestSubtype: evpFieldStr(test, "subType"),
		TestVersion: evpFieldStr(test, "version"),

		LocationID:          evpFieldStr(location, "id"),
		LocationName:        evpFieldStr(location, "name"),
		LocationDisplayName: evpFieldStr(location, "displayName"),

		ResultID:        evpFieldStr(res, "id"),
		ResultInitialID: evpFieldStr(res, "initialId"),
		Status:          evpFieldStr(res, "status"),
		RunType:         evpFieldStr(res, "runType"),
		Duration:        evpAsString(evpFieldValue(res, "duration")),

		TestStartedAt:         started,
		TestStartedAtParsed:   evpParseTime(started),
		TestFinishedAt:        finished,
		TestFinishedAtParsed:  evpParseTime(finished),
		TestTriggeredAt:       triggered,
		TestTriggeredAtParsed: evpParseTime(triggered),

		AssertionType:     aType,
		AssertionOperator: aOp,
		AssertionExpected: aExpected,
		AssertionActual:   aActual,
		AssertionValid:    aValid,

		FailureCode:    failureCode,
		FailureMessage: failureMessage,

		Config: evpJSON(evpFieldValue(res, "config")),

		NetstatsPacketsSent:          evpInt64Ptr(netstats["packetsSent"]),
		NetstatsPacketsReceived:      evpInt64Ptr(netstats["packetsReceived"]),
		NetstatsPacketLossPercentage: evpFloat64Ptr(netstats["packetLossPercentage"]),
		NetstatsJitter:               evpFloat64Ptr(netstats["jitter"]),
		NetstatsLatency:              evpFloat64Ptr(netstats["latency"]),
		NetstatsHops:                 evpInt64Ptr(netstats["hops"]),

		Netpath:    evpJSON(evpFieldValue(res, "netpath")),
		DD:         evpJSON(evpFieldValue(m, "_dd")),
		Enrichment: evpJSON(evpFieldValue(m, "enrichment")),
		V:          evpAsString(evpFieldValue(m, "v")),
		Extra:      evpUnknownMap(m, syntheticsKnownKeys),
	}
}

// ---------------------------------------------------------------------------
// data-obs-intake.<site>

// lineageKnownKeys are the OpenLineage RunEvent top-level keys this intake
// documents; anything else lands in Extra.
var lineageKnownKeys = map[string]bool{
	"eventType": true, "eventTime": true, "producer": true, "schemaURL": true,
	"run": true, "job": true, "inputs": true, "outputs": true,
}

// Data Observability: two producers on one host.
//
//	/api/v1/lineage        OpenLineage RunEvents, forwarded verbatim by the
//	                       trace-agent's reverse proxy (ol_proxy_config).
//	                       ?api-version=2 when the agent is told to add it.
//	/api/v2/query-actions  results of Data Observability query actions from
//	                       Python integrations, batched JSON.
//
// The lineage proxy carries the API key as "Authorization: Bearer <key>",
// the OpenLineage client's convention, NOT Dd-Api-Key. This host is therefore
// the one intake built with engineAuth rather than engine, so that its guard
// reads Bearer as well — see routes.go and RequireAPIKeyFrom.
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
		a.storeRaw(c, "lineage", "unexpected_shape", "body is not a JSON array or object", body)
		return
	}

	tenant := TenantFromContext(c)
	now := time.Now().UTC()
	apiVersion := c.Query("api-version")
	via := c.GetHeader("Via")

	// Every event is decoded; evpDatasets caps only the rendered names. An
	// event that fails to decode is skipped here but is still in items[i],
	// raw, and reaches storeRaw.
	events := make([]map[string]any, 0, len(items))
	var rows []storage.OpenLineageEventRow
	log.Printf("[lineage] api-version=%s via=%q — %d events (%d bytes)",
		evpOr(apiVersion), via, len(items), len(body))
	for _, raw := range items {
		m, ok := evpObject("lineage", raw)
		if !ok {
			a.storeRaw(c, "lineage", "decode_error", "item is not a JSON object", raw)
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

		if tenant != "" {
			rows = append(rows, openLineageEventRow(m, apiVersion, via))
		}
	}

	if tenant == "" {
		return
	}
	for i := range rows {
		rows[i].TenantID = tenant
		rows[i].ReceivedAt = now
	}
	a.store(storage.OpenLineageWriter, storage.WriteOpenLineageEvents{Rows: rows}, len(rows))
}

// openLineageEventRow converts one decoded RunEvent. TenantID/ReceivedAt are
// left zero-valued for the same reason as eventManagementRow.
func openLineageEventRow(m map[string]any, apiVersion, via string) storage.OpenLineageEventRow {
	run, _ := m["run"].(map[string]any)
	job, _ := m["job"].(map[string]any)
	eventTime := evpFieldStr(m, "eventTime")

	inNS, inName, inFacets := datasetArrays(m["inputs"])
	outNS, outName, outFacets := datasetArrays(m["outputs"])

	return storage.OpenLineageEventRow{
		EventType:       evpFieldStr(m, "eventType"),
		EventTime:       eventTime,
		EventTimeParsed: evpParseTime(eventTime),
		Producer:        evpFieldStr(m, "producer"),
		SchemaURL:       evpFieldStr(m, "schemaURL"),

		RunID:        evpFieldStr(run, "runId"),
		RunFacets:    evpJSON(evpFieldValue(run, "facets")),
		JobNamespace: evpFieldStr(job, "namespace"),
		JobName:      evpFieldStr(job, "name"),
		JobFacets:    evpJSON(evpFieldValue(job, "facets")),

		InputNamespace:  inNS,
		InputName:       inName,
		InputFacets:     inFacets,
		OutputNamespace: outNS,
		OutputName:      outName,
		OutputFacets:    outFacets,

		APIVersion: apiVersion,
		Via:        via,
		Extra:      evpUnknownMap(m, lineageKnownKeys),
	}
}

// datasetArrays reads an inputs[]/outputs[] list into three parallel arrays,
// index-aligned within this one list (inputs and outputs are independent
// lists, not one shared index).
func datasetArrays(v any) (namespaces, names, facets []string) {
	list, _ := v.([]any)
	for _, it := range list {
		dm, _ := it.(map[string]any)
		namespaces = append(namespaces, evpFieldStr(dm, "namespace"))
		names = append(names, evpFieldStr(dm, "name"))
		facets = append(facets, evpJSON(evpFieldValue(dm, "facets")))
	}
	return namespaces, names, facets
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
		a.storeRaw(c, "query-actions", "unexpected_shape", "body is not a JSON array or object", body)
		return
	}

	tenant := TenantFromContext(c)
	now := time.Now().UTC()
	ddOrigin := c.GetHeader("Dd-Evp-Origin")
	ddOriginVersion := c.GetHeader("Dd-Evp-Origin-Version")

	// Every entry is decoded — the cap only limits what is printed. An entry
	// that fails to decode is skipped here but is still in items[i], raw, and
	// reaches storeRaw.
	results := make([]map[string]any, 0, len(items))
	var rows []storage.QueryActionResultRow
	log.Printf("[query-actions] %d entries (%d bytes)", len(items), len(body))
	for i, raw := range items {
		m, ok := evpObject("query-actions", raw)
		if !ok {
			a.storeRaw(c, "query-actions", "decode_error", "item is not a JSON object", raw)
			continue
		}
		results = append(results, m)
		if i < evpMaxItems {
			log.Printf("   %s", evpFields(m))
		} else if i == evpMaxItems {
			log.Printf("   ... %d more", len(items)-i)
		}

		if tenant != "" {
			rows = append(rows, storage.QueryActionResultRow{
				Result:             evpJSON(m),
				Keys:               evpSortedKeys(m),
				DDEVPOrigin:        ddOrigin,
				DDEVPOriginVersion: ddOriginVersion,
			})
		}
	}

	if tenant == "" {
		return
	}
	for i := range rows {
		rows[i].TenantID = tenant
		rows[i].ReceivedAt = now
	}
	a.store(storage.QueryActionResultsWriter, storage.WriteQueryActionResults{Rows: rows}, len(rows))
}
