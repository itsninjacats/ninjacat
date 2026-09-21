package intake

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"hash/fnv"
	"log"
	"maps"
	"net/http"
	"slices"
	"strconv"
	"strings"
	"time"

	"github.com/DataDog/agent-payload/v5/gogen"
	"github.com/DataDog/datadog-api-client-go/v2/api/datadogV1"
	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/sketch"
)

// api.<site> — the main forwarder.
//
//	config: dd_url / DD_DD_URL
//
// The only intake carrying several unrelated signals, because it predates
// Datadog's split into per-product intakes. Metrics, host metadata, check
// results and sketches all arrive here.
//
// The last two routes are the ones no agent ever calls — they exist for
// datadog-api-client and anything else submitting over HTTP without an agent.
// Same host, different caller.
//
// Declared in the agent's forwarder but not yet sent by 7.83:
//   /api/v2/events, /api/v2/service_checks, /api/v2/host_metadata,
//   /api/v1/sketches, /api/intake/metrics/v3{,beta}/{series,sketches}
//
// Not handled, read side: /api/v1/metrics, /api/v1/search
//
// The Private Action Runner also lives here — the only agent component whose
// loop is DRIVEN by what we answer, see the PAR section at the bottom.
//
// RULE FOR EVERY HANDLER BELOW: no field of a decoded struct disappears
// without a comment saying why. What has a column is stored; what has no
// column yet is logged (bounded, never a wall); what is deliberately kept out
// of the log — secrets, walls of text — says so next to the code.

func (a *Server) routeAPI(g *gin.RouterGroup) {
	g.POST("/intake/", a.HandleIntake)
	g.POST("/api/v1/series", a.HandleSeriesV1)
	g.POST("/api/v2/series", a.HandleSeriesV2)
	g.POST("/api/v1/check_run", a.HandleCheckRun)
	g.POST("/api/v1/metadata", a.HandleMetadata)
	g.POST("/api/beta/sketches", a.HandleSketches)

	// Not sent by any agent — API clients only.
	g.POST("/api/v1/events", a.HandleEvents)
	g.POST("/api/v1/distribution_points", a.HandleDistributionPoints)

	// Endpoints the agent READS from: each needs a real answer, not a 202.
	g.POST("/api/v2/intake-key", a.HandleIntakeKey)
	g.GET("/api/v1/query", a.HandleQuery)
	g.POST("/api/v2/profiles/symbols/query", a.HandleSymbolsQuery)

	// Private Action Runner. Enrollment and connections are JSON:API with
	// Content-Type application/vnd.api+json; the OPMS loop sends
	// application/json. bodyIsJSON and describe match both on "json".
	g.POST("/api/unstable/on_prem_runners", a.HandleRunnerEnroll)
	g.POST("/api/unstable/on_prem_runners/api_key_only", a.HandleRunnerEnroll)
	g.POST("/api/v2/on-prem-management-service/workflow-tasks/dequeue", a.HandleRunnerDequeue)
	g.POST("/api/v2/on-prem-management-service/workflow-tasks/publish-task-update", a.HandleRunnerTaskUpdate)
	g.POST("/api/v2/on-prem-management-service/workflow-tasks/heartbeat", a.HandleRunnerHeartbeat)
	g.GET("/api/v2/on-prem-management-service/runner/health-check", a.HandleRunnerHealthCheck)
	g.POST("/api/v2/actions/connections", a.HandleActionConnections)
}

// apiLogLimit caps every per-item listing in this file: that many entries,
// then "... N more".
const apiLogLimit = 8

// HandleSeriesV2 serves /api/v2/series.
//
// Protobuf from the agent, JSON from the public API and its clients — both
// land in gogen.MetricPayload, because the protobuf-generated struct carries
// json tags matching the v2 shape.
//
// gogen.MetricPayload_MetricSeries, field by field:
//
//	Resources       host -> row; every other type (device) -> log, no column
//	Metric, Tags, Points, Type, Unit, SourceTypeName, Interval -> row
//	Metadata        Origin{product, category, service} -> log, no column
//
// The JSON path has no AdditionalProperties: gogen is a protobuf struct, so
// encoding/json drops keys it does not declare without a trace. The fix
// belongs in payloads.go (datadogV2.MetricPayload carries them), not here.
func (a *Server) HandleSeriesV2(c *gin.Context) {
	defer ackSeries(c)
	if isDiagnose(c) {
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[v2/series] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	var payload gogen.MetricPayload
	if bodyIsJSON(c, body) {
		payload, err = parseSeriesJSONv2(body)
	} else {
		payload, err = parseSeriesProtobuf(body)
	}
	if err != nil {
		log.Printf("[v2/series] %v", err)
		return
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	var (
		points    []storage.MetricPoint
		origins   = map[string]int{}
		resources = map[string]int{}
		noIndex   int
	)
	for _, s := range payload.Series {
		host := resourceNamed(s.Resources, "host")
		for _, r := range s.Resources {
			// The agent writes exactly two resource types, host and device
			// (iterable_series.go); the wire allows any. Only host has a
			// column, the rest is shown here so a device series is not
			// mistaken for a host one.
			if r.GetType() != "host" {
				resources[r.GetType()+"="+r.GetName()]++
			}
		}
		origins[apiOriginName(s.GetMetadata())]++
		// The agent also writes Origin field 3 (metric_type = 9, "do not
		// index") which the published proto marks reserved, so gogen parks
		// it in XXX_unrecognized. Counted, not decoded: a reserved field has
		// no name to decode into.
		if len(s.GetMetadata().GetOrigin().XXX_unrecognized) > 0 {
			noIndex++
		}
		for _, p := range s.Points {
			points = append(points, storage.MetricPoint{
				TenantID: tenant, Timestamp: wireTime(p.GetTimestamp()),
				Metric: s.Metric, Host: host,
				MetricType: metricTypeName(int32(s.Type)), SourceType: s.SourceTypeName,
				Unit: s.Unit, Interval: uint32(s.Interval),
				Value: p.GetValue(), Tags: tagsToMap(s.Tags),
			})
		}
	}
	apiLogSeriesExtras("v2/series", len(payload.Series), len(points), origins, resources, noIndex)
	a.store(storage.MetricsWriter, storage.WriteMetrics{Points: points}, len(points))

	// The rows above are a projection; this is the source. Every series with
	// its Resources (host AND device), Metadata.Origin, XXX_unrecognized and
	// all of its Points, exactly as the wire carried them.
	_ = &payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// apiLogSeriesExtras reports what a series batch carried beyond its columns.
// One line per batch; the origin and resource tallies are bounded.
func apiLogSeriesExtras(label string, series, points int, origins, resources map[string]int, noIndex int) {
	log.Printf("[%s] %d series, %d points, origins: %s", label, series, points, apiTally(origins))
	if len(resources) > 0 {
		// TODO(ninjacat): tables. Device (and any other resource) has no
		// column on the metrics row.
		log.Printf("   non-host resources (no column yet): %s", apiTally(resources))
	}
	if noIndex > 0 {
		log.Printf("   %d series flagged do-not-index by the agent (Origin field 3)", noIndex)
	}
}

// apiOriginName renders series/sketch origin metadata as product/category/
// service. The numbers are Datadog's private origin.proto enums; the agent's
// side of the mapping is pkg/serializer/internal/metrics/origin_mapping.go:
// product 10 = agent, 1 = serverless, 19 = datadog exporter, 38 = GPU;
// category 10 = dogstatsd, 11 = integration metrics, 0 = unknown; service
// is one value per integration. Nil-safe: a client that sends none gets "-".
func apiOriginName(m *gogen.Metadata) string {
	o := m.GetOrigin()
	if o == nil {
		return "-"
	}
	return fmt.Sprintf("%d/%d/%d", o.GetOriginProduct(), o.GetOriginCategory(), o.GetOriginService())
}

// HandleSeriesV1 serves /api/v1/series.
//
// The older public API. Its JSON is a different shape — host is its own field,
// the type is a word, points are [timestamp, value] pairs — so it is read into
// datadogV1.MetricsPayload and turned into rows HERE, without being translated
// into the v2 struct first.
//
// This path is JSON only. The agent's serializer sends protobuf solely to
// /api/v2/series (pkg/serializer/serializer.go); v1 is the JSON fallback.
//
// datadogV1.Series declares host, interval, metric, points, tags, type. The
// agent's v1 encoder (encodeSerie in iterable_series.go) also writes device,
// source_type_name and unit, none of which the model knows, so they arrive
// in AdditionalProperties: source_type_name and unit have columns and are
// stored from there; device has none and is logged.
func (a *Server) HandleSeriesV1(c *gin.Context) {
	defer ackSeries(c)
	if isDiagnose(c) {
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[v1/series] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	payload, err := parseSeriesJSONv1(body)
	if err != nil {
		log.Printf("[v1/series] %v", err)
		return
	}
	apiLogExtra("v1/series", "payload", payload.AdditionalProperties, payload.UnparsedObject)

	var (
		points    []storage.MetricPoint
		resources = map[string]int{}
		extra     = map[string]int{}
		unparsed  int
	)
	for _, s := range payload.Series {
		if s.UnparsedObject != nil {
			// The generated UnmarshalJSON swallows a structurally wrong
			// series (points not an array, say) into UnparsedObject and
			// returns no error, so this series has no metric and no points.
			unparsed++
			continue
		}
		interval := uint32(0)
		if iv := s.Interval.Get(); s.Interval.IsSet() && iv != nil {
			interval = uint32(*iv)
		}
		if d := attrString(s.AdditionalProperties, "device"); d != "" {
			resources["device="+d]++
		}
		for k := range s.AdditionalProperties {
			switch k {
			case "device", "source_type_name", "unit":
			default:
				extra[k]++
			}
		}
		for _, p := range s.Points {
			// A v1 point is a bare [timestamp, value] pair, both nullable.
			if len(p) < 2 || p[0] == nil || p[1] == nil {
				continue
			}
			points = append(points, storage.MetricPoint{
				TenantID: tenant, Timestamp: wireTime(int64(*p[0])),
				Metric: s.Metric, Host: s.GetHost(),
				MetricType: normalizeTypeWord(s.GetType()),
				SourceType: attrString(s.AdditionalProperties, "source_type_name"),
				Unit:       attrString(s.AdditionalProperties, "unit"),
				Interval:   interval,
				Value:      *p[1], Tags: tagsToMap(s.Tags),
			})
		}
	}
	// v1 carries no origin metadata; the tally is nil so the line reads "-".
	apiLogSeriesExtras("v1/series", len(payload.Series), len(points), nil, resources, 0)
	if len(extra) > 0 {
		log.Printf("   undeclared series keys (no column yet): %s", apiTally(extra))
	}
	if unparsed > 0 {
		log.Printf("[v1/series] %d series could not be decoded into datadogV1.Series (UnparsedObject set)", unparsed)
	}
	a.store(storage.MetricsWriter, storage.WriteMetrics{Points: points}, len(points))

	// The rows above are a projection; this is the source. A series skipped
	// by the loop (UnparsedObject set) or a point skipped for a nil element
	// is still in here: UnparsedObject holds the raw object, Points the raw
	// pair. AdditionalProperties (device, source_type_name, unit, anything
	// undeclared) travel with each series.
	_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// HandleSketches serves /api/beta/sketches: DDSketch histograms the agent has
// already built. Bucket keys follow the agent's own mapping — see package
// sketch for why that matters.
//
// gogen.SketchPayload, field by field:
//
//	Sketches         -> rows, see below
//	Metadata         CommonMetadata about the SENDER, not the metric: agent
//	                 version, timezone, epoch, IPs -> log; api_key -> presence
//	                 only, never the value
//
// gogen.SketchPayload_Sketch:
//
//	Metric, Host, Tags     -> row
//	Dogsketches            -> one row each (the DDSketch: k/n bucket keys and
//	                          counts, plus cnt/min/max/avg/sum)
//	Distributions          the PRE-DDSketch encoding (field 3): a GK-style
//	                          quantile sketch with raw values v, ranks g/delta
//	                          and an unmerged buffer buf. The current agent
//	                          never writes it (sketch_series_list.go keeps
//	                          field 3 commented out) and our row is keyed on
//	                          DDSketch buckets, so there is nowhere to put
//	                          one -> counted and logged, not stored
//	Metadata               Origin -> log, no column
//
// Both lists are independent repeated fields, so one sketch may carry both;
// the loops below do not assume otherwise.
func (a *Server) HandleSketches(c *gin.Context) {
	defer ackSeries(c)
	if isDiagnose(c) {
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[sketches] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	var payload gogen.SketchPayload
	if bodyIsJSON(c, body) {
		payload, err = parseSketchesJSON(body)
	} else {
		payload, err = parseSketchesProtobuf(body)
	}
	if err != nil {
		log.Printf("[sketches] %v", err)
		return
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	// One row per dogsketch, not per metric: each dogsketch is its own point
	// in time.
	var (
		rows    []storage.SketchRow
		origins = map[string]int{}
		legacy  = map[string]int{}
	)
	for _, s := range payload.Sketches {
		origins[apiOriginName(s.GetMetadata())]++
		if n := len(s.Distributions); n > 0 {
			legacy[s.Metric] += n
		}
		for _, d := range s.Dogsketches {
			rows = append(rows, storage.SketchRow{
				TenantID: tenant, Timestamp: wireTime(d.Ts),
				Metric: s.Metric, Host: s.Host, Tags: tagsToMap(s.Tags),
				Count: uint64(d.Cnt), Min: d.Min, Max: d.Max, Avg: d.Avg, Sum: d.Sum,
				BucketKeys: d.K, BucketCounts: d.N,
			})
		}
	}

	m := payload.Metadata
	log.Printf("[sketches] %d sketches, %d dogsketches, origins: %s; sender agent=%s tz=%s epoch=%.0f internal_ip=%s public_ip=%s api_key=%s",
		len(payload.Sketches), len(rows), apiTally(origins),
		orUnknown(m.AgentVersion), orUnknown(m.Timezone), m.CurrentEpoch,
		orUnknown(m.InternalIp), orUnknown(m.PublicIp), apiPresent(m.ApiKey != ""))
	if len(legacy) > 0 {
		// TODO(ninjacat): tables. A legacy Distribution cannot be merged
		// with DDSketch buckets; it would need its own row shape.
		log.Printf("[sketches] legacy Distribution entries (pre-DDSketch, not stored): %s", apiTally(legacy))
	}
	a.store(storage.SketchesWriter, storage.WriteSketches{Sketches: rows}, len(rows))

	// The rows above are a projection; this is the source. Every sketch with
	// its Dogsketches AND its legacy Distributions (which have no row shape),
	// its Metadata.Origin, plus the sender's CommonMetadata — api_key value
	// included, so this must not be logged as-is.
	_ = &payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// HandleCheckRun serves /api/v1/check_run.
//
// A check answers "is this thing working", not "how much of it is there".
// Monitors of the "service is down" kind are built on these.
//
// datadogV1.ServiceCheck: check, host_name, message, status, tags, timestamp
// all have columns. The agent's own struct (servicecheck.ServiceCheck) has
// exactly these six keys, so AdditionalProperties should stay empty — when
// it does not, it is logged, as is UnparsedObject.
func (a *Server) HandleCheckRun(c *gin.Context) {
	defer ackSeries(c)
	if isDiagnose(c) {
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[check_run] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	runs, err := parseCheckRuns(body)
	if err != nil {
		log.Printf("[check_run] %v", err)
		return
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	rows := make([]storage.CheckRunRow, 0, len(runs))
	for i, r := range runs {
		apiLogExtra("check_run", "check #"+strconv.Itoa(i), r.AdditionalProperties, r.UnparsedObject)
		if r.UnparsedObject != nil {
			continue
		}
		rows = append(rows, storage.CheckRunRow{
			TenantID: tenant, Timestamp: wireTime(r.GetTimestamp()),
			CheckName: r.Check, Host: r.HostName,
			Status: statusName(int(r.Status)), Message: r.GetMessage(),
			Tags: tagsToMap(r.Tags),
		})
	}
	a.store(storage.ChecksWriter, storage.WriteCheckRuns{Runs: rows}, len(rows))

	// The rows above are a projection; this is the source. A check the loop
	// skipped (UnparsedObject set) is still here, raw, at the same index.
	_ = runs // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// statusName turns a check status code into its name.
func statusName(s int) string {
	switch s {
	case 0:
		return "OK"
	case 1:
		return "WARNING"
	case 2:
		return "CRITICAL"
	default:
		return "UNKNOWN"
	}
}

// HandleIntake serves /intake/: the Agent v5 endpoint, still fed by four
// producers that share nothing but the path. See decodeIntake.
func (a *Server) HandleIntake(c *gin.Context) {
	defer c.JSON(http.StatusOK, gin.H{"status": "ok"})
	if isDiagnose(c) {
		return
	}
	a.decodeIntake(c, "intake")
}

// decodeIntake tells the /intake/ shapes apart by their top-level keys — the
// path says nothing. In the order the keys are checked (docs §4.1):
//
//	events            {"apiKey":"", "events":{<source>:[...]}, "internalHostname"}
//	                  — agent events, including the synchronous shutdown
//	                  event, which is the same shape sent with Retryable=false
//	agent_checks      collectorimpl.Payload: check statuses + external host tags
//	gohai/systemStats hostimpl.Payload: host metadata, the only one stored
//	resources alone   legacy processes metadata from comp/metadata/resources
//
// None of the four has an importable type (docs §6), so fields are read by
// name. Before this split every shape was treated as host metadata and an
// events payload — which also carries internalHostname — produced a host row
// with an empty agent version and OS.
func (a *Server) decodeIntake(c *gin.Context, label string) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[%s] cannot read body: %v", label, err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	var m map[string]json.RawMessage
	if err := json.Unmarshal(body, &m); err != nil {
		log.Printf("[%s] JSON: %v", label, err)
		return
	}

	_, hasEvents := m["events"]
	_, hasChecks := m["agent_checks"]
	_, hasGohai := m["gohai"]
	_, hasStats := m["systemStats"]
	_, hasResources := m["resources"]
	switch {
	case hasEvents:
		intakeLogEvents(label, m)
	case hasChecks:
		intakeLogAgentChecks(label, m)
	case hasGohai || hasStats:
		a.intakeHost(c, label, m)
	case hasResources && len(m) == 1:
		intakeLogProcesses(label, m)
	default:
		log.Printf("[%s] unrecognised shape, keys: %s", label, apiKeys(m))
		// A fifth producer, or a known one with a new top-level key. The
		// envelope is decoded one level, every value raw.
		_ = m // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	}
}

// intakeLogEvents reports agent events. Each entry is pkg/metrics/event.Event
// as JSON: msg_title, msg_text, timestamp, priority, host, tags, alert_type,
// aggregation_key, source_type_name, event_type. apiKey is deliberately empty
// on this shape (the key travels in the header) and is not logged.
//
// TODO(ninjacat): tables. storage.EventsWriter already takes this shape from
// /api/v1/events; these should feed the same writer.
func intakeLogEvents(label string, m map[string]json.RawMessage) {
	// Decoded whole, before any logging: source -> events, every event a
	// free map with numbers kept as json.Number so a timestamp is not
	// rounded through float64. The log below is bounded; this is not.
	var bySource map[string][]map[string]any
	if err := apiUnmarshalNumber(m["events"], &bySource); err != nil {
		log.Printf("[%s] events: %v", label, err)
		// Nothing decoded; the raw events object is still m["events"].
		_ = m // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		return
	}
	var host string
	json.Unmarshal(m["internalHostname"], &host)

	total := 0
	for _, evs := range bySource {
		total += len(evs)
	}
	log.Printf("[%s] variant=events host=%s %d events from %d sources", label, host, total, len(bySource))
	shown := 0
log:
	for _, source := range slices.Sorted(maps.Keys(bySource)) {
		for _, ev := range bySource[source] {
			if shown == apiLogLimit {
				log.Printf("   ... and %d more", total-shown)
				break log
			}
			shown++
			tags, _ := ev["tags"].([]any)
			// msg_text is the one field left out: it is free text of up to
			// 4000 characters and the title identifies the event.
			log.Printf("   %s: %q host=%v ts=%v alert_type=%v priority=%v aggregation_key=%v event_type=%v source_type_name=%v tags=%d",
				source, ev["msg_title"], ev["host"], ev["timestamp"], ev["alert_type"],
				ev["priority"], ev["aggregation_key"], ev["event_type"], ev["source_type_name"], len(tags))
		}
	}
	apiLogUnknownKeys(label, "events payload", m, "apiKey", "events", "internalHostname")

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = bySource // source -> []event, all of them, msg_text included
	_ = host     // internalHostname
	_ = m        // the whole envelope, raw: apiKey (empty on this shape) and any key not named above
}

// intakeLogAgentChecks reports collectorimpl.Payload: the V5-era check status
// list and external host tags. agent_checks entries are positional arrays
// (check name, source type, instance id, status, message, ...) with no
// published type, so they are shown as-is, bounded; external_host_tags is
// [[hostname, {source: [tags]}], ...].
func intakeLogAgentChecks(label string, m map[string]json.RawMessage) {
	var (
		checksRaw  []json.RawMessage
		extTagsRaw []json.RawMessage
		host       string
		version    string
		id         string
		meta       map[string]any
		apiKeyOK   = apiRawPresent(m["apiKey"])
	)
	json.Unmarshal(m["agent_checks"], &checksRaw)
	json.Unmarshal(m["external_host_tags"], &extTagsRaw)
	json.Unmarshal(m["internalHostname"], &host)
	json.Unmarshal(m["agentVersion"], &version)
	json.Unmarshal(m["uuid"], &id)
	json.Unmarshal(m["meta"], &meta)

	// Every entry decoded, independent of the log limit below. A check is a
	// positional array, so []any with numbers as json.Number; an
	// external_host_tags entry is the pair [hostname, {source: [tags]}]. An
	// entry that does not fit is left nil at its index and stays raw in
	// checksRaw / extTagsRaw, so nothing goes missing without a trace.
	checks := make([][]any, len(checksRaw))
	for i, raw := range checksRaw {
		if err := apiUnmarshalNumber(raw, &checks[i]); err != nil {
			log.Printf("[%s] agent_checks[%d] is not an array: %v", label, i, err)
		}
	}
	extTags := make([]struct {
		Host string
		Tags map[string][]string
	}, len(extTagsRaw))
	for i, raw := range extTagsRaw {
		var pair []json.RawMessage
		if err := json.Unmarshal(raw, &pair); err != nil || len(pair) != 2 {
			log.Printf("[%s] external_host_tags[%d] is not a [hostname, tags] pair", label, i)
			continue
		}
		json.Unmarshal(pair[0], &extTags[i].Host)
		if err := json.Unmarshal(pair[1], &extTags[i].Tags); err != nil {
			log.Printf("[%s] external_host_tags[%d] tags: %v", label, i, err)
		}
	}

	// apiKey on this shape is the REAL key (CommonPayload.APIKey): presence
	// only, never the value.
	log.Printf("[%s] variant=agent_checks host=%s agent=%s uuid=%s api_key=%s checks=%d external_host_tags=%d meta: %s",
		label, host, version, id, apiPresent(apiKeyOK), len(checksRaw), len(extTagsRaw), apiKV(meta))
	for i, raw := range checksRaw {
		if i == apiLogLimit {
			log.Printf("   ... and %d more", len(checksRaw)-i)
			break
		}
		log.Printf("   check %s", apiTrunc(string(raw), 160))
	}
	for i, raw := range extTagsRaw {
		if i == apiLogLimit {
			log.Printf("   ... and %d more", len(extTagsRaw)-i)
			break
		}
		log.Printf("   external_host_tags %s", apiTrunc(string(raw), 160))
	}
	apiLogUnknownKeys(label, "agent_checks payload", m,
		"apiKey", "agentVersion", "uuid", "internalHostname", "meta", "agent_checks", "external_host_tags")

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = checks     // one positional array per check, all of them
	_ = checksRaw  // the same entries, raw, same indices
	_ = extTags    // one {Host, source -> tags} per entry, all of them
	_ = extTagsRaw // the same entries, raw, same indices
	_ = host       // internalHostname
	_ = version    // agentVersion
	_ = id         // uuid
	_ = meta       // meta
	_ = m          // the whole envelope, raw: apiKey (the real key) and any key not named above
}

// intakeHost pulls out host metadata (hostimpl.Payload). The payload is loose
// and shifts between agent versions, so fields are read by name rather than
// through a fixed struct. Key by key:
//
//	internalHostname, agentVersion, os     -> row
//	gohai (a STRING holding JSON)          platform, cpu, memory -> row;
//	                                       filesystem, network -> log, no column
//	host-tags.system                       -> row (Tags)
//	apiKey                                 the REAL key: presence only
//	uuid, agent-flavor, python, systemStats, meta, host-tags (other
//	sources), container-meta, network, logs, install-method, proxy-info,
//	otlp, fips_mode, fips_proxy_enabled    -> log, no column
//	resources                              the process snapshot, the same
//	                                       thing the process-agent sends in
//	                                       full: size only, it is a wall
func (a *Server) intakeHost(c *gin.Context, label string, m map[string]json.RawMessage) {
	var hostname, agentVersion, osName string
	json.Unmarshal(m["internalHostname"], &hostname)
	json.Unmarshal(m["agentVersion"], &agentVersion)
	json.Unmarshal(m["os"], &osName)

	// gohai arrives as a STRING holding nested JSON — the hardware inventory.
	var gohai map[string]json.RawMessage
	if raw, ok := m["gohai"]; ok {
		var inner string
		if json.Unmarshal(raw, &inner) == nil {
			json.Unmarshal([]byte(inner), &gohai)
		}
	}

	var hostTags map[string][]string
	json.Unmarshal(m["host-tags"], &hostTags)

	intakeLogHost(label, m, gohai, hostTags)

	// The row below is a projection; this is the source. m is the whole
	// envelope, raw, every key including apiKey (the real key), resources
	// (the process snapshot) and anything a newer agent adds; gohai is the
	// inventory string decoded one level (section -> raw JSON), every
	// section, not just the three with columns; hostTags is every source,
	// not just "system".
	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = m
	_ = gohai
	_ = hostTags

	tenant := TenantFromContext(c)
	if tenant == "" || hostname == "" {
		return
	}
	a.store(storage.HostsWriter, storage.WriteHosts{Hosts: []storage.HostRow{{
		TenantID:     tenant,
		Host:         hostname,
		SeenAt:       time.Now(),
		AgentVersion: agentVersion,
		OS:           osName,
		Platform:     gohaiSection(gohai, "platform"),
		CPU:          gohaiSection(gohai, "cpu"),
		Memory:       gohaiSection(gohai, "memory"),
		Tags:         hostTags["system"],
	}}}, 1)
}

// intakeLogHost reports the parts of host metadata that have no column.
func intakeLogHost(label string, m map[string]json.RawMessage, gohai map[string]json.RawMessage, hostTags map[string][]string) {
	var (
		id, flavor, python                  string
		fips, fipsProxy                     bool
		stats, meta, network, logs, install map[string]any
		proxy, otlp, containerMeta          map[string]any
		host, version                       string
	)
	json.Unmarshal(m["internalHostname"], &host)
	json.Unmarshal(m["agentVersion"], &version)
	json.Unmarshal(m["uuid"], &id)
	json.Unmarshal(m["agent-flavor"], &flavor)
	json.Unmarshal(m["python"], &python)
	json.Unmarshal(m["fips_mode"], &fips)
	json.Unmarshal(m["fips_proxy_enabled"], &fipsProxy)
	json.Unmarshal(m["systemStats"], &stats)
	json.Unmarshal(m["meta"], &meta)
	json.Unmarshal(m["network"], &network)
	json.Unmarshal(m["logs"], &logs)
	json.Unmarshal(m["install-method"], &install)
	json.Unmarshal(m["proxy-info"], &proxy)
	json.Unmarshal(m["otlp"], &otlp)
	json.Unmarshal(m["container-meta"], &containerMeta)

	log.Printf("[%s] variant=host host=%s agent=%s uuid=%s flavor=%s python=%s api_key=%s fips=%v fips_proxy=%v",
		label, host, version, id, flavor, python, apiPresent(apiRawPresent(m["apiKey"])), fips, fipsProxy)
	log.Printf("   meta: %s", apiKV(meta))
	log.Printf("   systemStats: %s", apiKV(stats))
	log.Printf("   network: %s logs: %s install-method: %s proxy-info: %s otlp: %s",
		apiKV(network), apiKV(logs), apiKV(install), apiKV(proxy), apiKV(otlp))
	if len(containerMeta) > 0 {
		log.Printf("   container-meta: %s", apiKV(containerMeta))
	}
	for _, source := range slices.Sorted(maps.Keys(hostTags)) {
		log.Printf("   host-tags[%s]: %s", source, apiHead(hostTags[source]))
	}
	if raw, ok := m["resources"]; ok {
		log.Printf("   resources: %d B (process snapshot, not decoded here — the process intake stores it)", len(raw))
	}
	// gohai sections beyond the three stored ones. filesystem is a list of
	// mounts; network is an object (or null on containers).
	for _, section := range []string{"filesystem", "network"} {
		if raw, ok := gohai[section]; ok {
			log.Printf("   gohai.%s: %d B (no column yet)", section, len(raw))
		}
	}
	apiLogUnknownKeys(label, "host payload", m,
		"apiKey", "agentVersion", "uuid", "internalHostname", "os", "agent-flavor", "python",
		"systemStats", "meta", "host-tags", "container-meta", "network", "logs", "install-method",
		"proxy-info", "otlp", "fips_mode", "fips_proxy_enabled", "resources", "gohai")
	apiLogUnknownKeys(label, "gohai", gohai, "cpu", "filesystem", "memory", "network", "platform")
}

// intakeLogProcesses reports the legacy processes shape from
// comp/metadata/resources: {"resources":{"processes":{"snaps":[...]},
// "meta":{"host":...}}}, "the format dates back from Agent V5". The snapshot
// itself is the wall the process intake already handles, so only its size.
func intakeLogProcesses(label string, m map[string]json.RawMessage) {
	var res struct {
		Processes struct {
			Snaps []json.RawMessage `json:"snaps"`
		} `json:"processes"`
		Meta map[string]any `json:"meta"`
	}
	if err := json.Unmarshal(m["resources"], &res); err != nil {
		log.Printf("[%s] resources: %v", label, err)
		// Nothing decoded; the raw resources object is still m["resources"].
		_ = m // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		return
	}
	size := 0
	for _, s := range res.Processes.Snaps {
		size += len(s)
	}
	log.Printf("[%s] variant=processes(legacy) meta: %s snaps=%d (%d B, not decoded here)",
		label, apiKV(res.Meta), len(res.Processes.Snaps), size)

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = res // meta decoded, every snapshot raw (the process intake owns that shape)
	_ = m   // the whole envelope, raw
}

// gohaiSection pulls one section of the gohai inventory into a flat map.
//
// gohai reports every value as a STRING, even numbers ("cpu_cores": "2"), so
// map[string]string matches the wire format exactly. Sections that are absent
// or shaped differently (network arrives as null on containers) yield nil
// rather than failing the whole write.
func gohaiSection(gohai map[string]json.RawMessage, name string) map[string]string {
	raw, ok := gohai[name]
	if !ok {
		return nil
	}
	var out map[string]string
	if err := json.Unmarshal(raw, &out); err != nil {
		return nil
	}
	return out
}

// HandleMetadata serves /api/v1/metadata: the inventory payloads. Eleven
// producers (comp/metadata/*) share the path, each an envelope of hostname,
// timestamp, uuid plus ONE variant key (docs §4.1); the cluster-agent ones
// carry clustername and cluster_id instead of hostname. None of the types is
// importable, so each variant is read by name — see metadataVariants.
//
// Every earlier capture was "{}" from the diagnostic sweep, which is why
// this used to be a bare ack; the decoder is built from the agent source
// (docs/zadania/metadata-endpoint.md), not from observation.
func (a *Server) HandleMetadata(c *gin.Context) {
	defer c.JSON(http.StatusOK, gin.H{"status": "ok"})
	if isDiagnose(c) {
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[metadata] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	var m map[string]json.RawMessage
	if err := json.Unmarshal(body, &m); err != nil {
		log.Printf("[metadata] JSON: %v", err)
		return
	}

	var env struct {
		Hostname    string `json:"hostname"`
		Clustername string `json:"clustername"`
		ClusterID   string `json:"cluster_id"`
		Timestamp   int64  `json:"timestamp"`
		UUID        string `json:"uuid"`
	}
	json.Unmarshal(body, &env)

	// One case per producer. Keys and shapes come from comp/metadata/*/impl
	// (Payload structs). Each variant is decoded whole and handed over on
	// its own, so what can arrive on this path is listed here, not inferred.
	// A variant whose value does not fit its shape is left raw in m under
	// the same key. Four variants are free maps upstream too (agent_metadata,
	// system_probe_metadata, security_agent_metadata, datadog_cluster_agent_
	// metadata), so key=value is the whole truth for them.
	var found []string
	for _, key := range metadataVariantKeys {
		raw, ok := m[key]
		if !ok {
			continue
		}
		found = append(found, key)
		log.Printf("[metadata] variant=%s host=%s cluster=%s/%s ts=%d uuid=%s",
			key, orUnknown(env.Hostname), env.Clustername, env.ClusterID, env.Timestamp, env.UUID)
		switch key {
		case "agent_metadata": // inventoryagent: flat config/feature map
			payload := metadataDecodeMap(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "host_metadata": // inventoryhost: cpu_*, kernel_*, cloud_provider*, dmi_*
			payload := metadataDecodeMap(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "check_metadata": // inventorychecks: check name -> instances
			payload := metadataDecodeChecks(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "logs_metadata": // inventorychecks: log source -> instances
			payload := metadataDecodeChecks(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "files_metadata": // inventorychecks: config file inventory
			payload := metadataDecodeMap(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "system_probe_metadata": // systemprobe
			payload := metadataDecodeMap(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "security_agent_metadata": // securityagent
			payload := metadataDecodeMap(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "signing_metadata": // packagesigning: signing_keys
			payload := metadataDecodeSigning(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "host_system_info_metadata": // hostsysteminfo: manufacturer, model, serial
			payload := metadataDecodeMap(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "host_gpu_metadata": // hostgpu: devices
			payload := metadataDecodeGPU(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "ha_agent_metadata": // haagent: enabled, state
			payload := metadataDecodeMap(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "datadog_cluster_agent_metadata": // clusteragent
			payload := metadataDecodeMap(raw)
			_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		case "clustercheck_metadata": // clusterchecks: check name -> instances
			payload := metadataDecodeChecks(raw)
			// Its two optional siblings, raw; absent keys yield nil.
			status, integrationStatus := m["clustercheck_status"], m["clustercheck_integration_status"]
			// TODO(ninjacat): tables. Complete, unconverted, ready to take.
			_ = payload
			_ = status
			_ = integrationStatus
		}
	}
	if len(found) == 0 {
		log.Printf("[metadata] no known variant key, keys: %s", apiKeys(m))
		// A producer this switch does not know. The envelope is decoded one
		// level, every value raw.
		_ = m // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		return
	}
	known := append([]string{"hostname", "clustername", "cluster_id", "timestamp", "uuid"}, found...)
	// clusterchecks carries two optional siblings next to its variant key.
	known = append(known, "clustercheck_status", "clustercheck_integration_status")
	apiLogUnknownKeys("metadata", strings.Join(found, "+"), m, known...)

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = env // hostname / clustername / cluster_id / timestamp / uuid
	_ = m   // the whole envelope, raw, variant values included
}

// metadataVariantKeys lists the variant keys of /api/v1/metadata in the order
// they are checked; HandleMetadata decodes each one in its own case.
var metadataVariantKeys = []string{
	"agent_metadata",
	"host_metadata",
	"check_metadata",
	"logs_metadata",
	"files_metadata",
	"system_probe_metadata",
	"security_agent_metadata",
	"signing_metadata",
	"host_system_info_metadata",
	"host_gpu_metadata",
	"ha_agent_metadata",
	"datadog_cluster_agent_metadata",
	"clustercheck_metadata",
}

// metadataDecodeMap decodes a free-map variant whole (numbers as
// json.Number), logs it bounded, and returns it; nil when it is not an
// object.
func metadataDecodeMap(raw json.RawMessage) map[string]any {
	var m map[string]any
	if err := apiUnmarshalNumber(raw, &m); err != nil {
		log.Printf("   not an object: %v", err)
		return nil
	}
	log.Printf("   %d keys: %s", len(m), apiKV(m))
	return m
}

// metadataDecodeChecks decodes map[string][]metadata — one entry per
// configured instance, each a free map (version, config.hash,
// config.provider, ...) — whole, logs the first apiLogLimit names, and
// returns all of it; nil when the shape does not fit.
func metadataDecodeChecks(raw json.RawMessage) map[string][]map[string]any {
	var m map[string][]map[string]any
	if err := apiUnmarshalNumber(raw, &m); err != nil {
		log.Printf("   not a name -> instances map: %v", err)
		return nil
	}
	names := slices.Sorted(maps.Keys(m))
	log.Printf("   %d names", len(names))
	for i, name := range names {
		if i == apiLogLimit {
			log.Printf("   ... and %d more", len(names)-i)
			break
		}
		first := map[string]any{}
		if len(m[name]) > 0 {
			first = m[name][0]
		}
		log.Printf("   %s: %d instances, first: %s", name, len(m[name]), apiKV(first))
	}
	return m
}

// metadataDecodeSigning decodes signing_metadata whole and returns every
// signing key; the log shows the first apiLogLimit. nil when the shape does
// not fit.
func metadataDecodeSigning(raw json.RawMessage) []map[string]any {
	var m struct {
		Keys []map[string]any `json:"signing_keys"`
	}
	if err := apiUnmarshalNumber(raw, &m); err != nil {
		log.Printf("   signing_keys: %v", err)
		return nil
	}
	log.Printf("   %d signing keys", len(m.Keys))
	for i, k := range m.Keys {
		if i == apiLogLimit {
			log.Printf("   ... and %d more", len(m.Keys)-i)
			break
		}
		log.Printf("   %s", apiKV(k))
	}
	return m.Keys
}

// metadataDecodeGPU decodes host_gpu_metadata whole and returns every
// device; the log shows the first apiLogLimit. nil when the shape does not
// fit.
func metadataDecodeGPU(raw json.RawMessage) []map[string]any {
	var m struct {
		Devices []map[string]any `json:"devices"`
	}
	if err := apiUnmarshalNumber(raw, &m); err != nil {
		log.Printf("   devices: %v", err)
		return nil
	}
	log.Printf("   %d GPU devices", len(m.Devices))
	for i, d := range m.Devices {
		if i == apiLogLimit {
			log.Printf("   ... and %d more", len(m.Devices)-i)
			break
		}
		log.Printf("   %s", apiKV(d))
	}
	return m.Devices
}

// HandleValidate answers the agent's API key check.
//
// The agent looks only at the status code. If a request got this far the
// middleware already recognised the key, so the answer is always 200: a bad
// key is rejected upstream with a 403, which stops the agent's pipeline.
//
// There is no body on this request, so there is nothing to hand over.
func (a *Server) HandleValidate(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"valid": true})
}

// HandleEvents accepts POST /api/v1/events.
//
// Unlike every other endpoint this one MUST return a body: the client
// deserializes the reply into EventCreateResponse and keeps the returned id.
// Answering "{}" fails on the client side.
//
// datadogV1.EventCreateRequest: aggregation_key, alert_type, date_happened,
// device_name, host, priority, source_type_name, tags, text, title all have
// columns. related_event_id has none and is logged when set; anything
// undeclared lands in AdditionalProperties and is logged too.
func (a *Server) HandleEvents(c *gin.Context) {
	if isDiagnose(c) {
		c.JSON(http.StatusAccepted, gin.H{"status": "ok"})
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[events] cannot read body: %v", err)
		c.JSON(http.StatusBadRequest, gin.H{"status": "error", "error": "unreadable body"})
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	events, err := parseEvents(body)
	if err != nil {
		log.Printf("[events] %v", err)
		c.JSON(http.StatusBadRequest, gin.H{"status": "error", "error": err.Error()})
		return
	}
	if len(events) == 0 {
		c.JSON(http.StatusBadRequest, gin.H{"status": "error", "error": "no event with a title"})
		return
	}

	tenant := TenantFromContext(c)
	rows := make([]storage.EventRow, 0, len(events))
	var firstID uint64

	for i, e := range events {
		id := uuid.New()
		num := binary.BigEndian.Uint64(id[:8]) >> 1 // >>1 keeps it inside int64
		if i == 0 {
			firstID = num
		}
		if rel, ok := e.GetRelatedEventIdOk(); ok && rel != nil {
			// TODO(ninjacat): tables. No parent-event column yet.
			log.Printf("[events] event #%d related_event_id=%d (no column yet)", i, *rel)
		}
		apiLogExtra("events", "event #"+strconv.Itoa(i), e.AdditionalProperties, e.UnparsedObject)
		rows = append(rows, storage.EventRow{
			TenantID: tenant, Timestamp: wireTime(e.GetDateHappened()),
			EventID: id, EventIDNum: num,
			Title: e.Title, Text: e.Text, Host: e.GetHost(),
			AlertType: alertTypeName(e), Priority: priorityName(e),
			AggregationKey: e.GetAggregationKey(),
			SourceTypeName: e.GetSourceTypeName(),
			DeviceName:     e.GetDeviceName(),
			Tags:           tagsToMap(e.Tags),
		})
	}

	if tenant != "" {
		a.store(storage.EventsWriter, storage.WriteEvents{Events: rows}, len(rows))
	}

	// The rows above are a projection; this is the source: every event with
	// related_event_id, AdditionalProperties and UnparsedObject intact.
	_ = events // TODO(ninjacat): tables. Complete, unconverted, ready to take.

	first := events[0]
	c.JSON(http.StatusAccepted, gin.H{
		"status": "ok",
		"event": gin.H{
			"id":               firstID,
			"id_str":           strconv.FormatUint(firstID, 10),
			"title":            first.Title,
			"text":             first.Text,
			"date_happened":    wireTime(first.GetDateHappened()).Unix(),
			"host":             first.GetHost(),
			"alert_type":       alertTypeName(first),
			"priority":         priorityName(first),
			"tags":             first.Tags,
			"source_type_name": first.GetSourceTypeName(),
			"device_name":      first.GetDeviceName(),
			"url":              "",
		},
	})
}

// Values Datadog accepts. Anything else is coerced rather than rejected:
// dropping an event because someone invented an alert type would lose the one
// thing we were asked to remember.
var (
	eventAlertTypes = map[string]bool{
		"error": true, "warning": true, "info": true, "success": true,
		"user_update": true, "recommendation": true, "snapshot": true,
	}
	eventPriorities = map[string]bool{"normal": true, "low": true}
)

func alertTypeName(e datadogV1.EventCreateRequest) string {
	v := strings.ToLower(string(e.GetAlertType()))
	if !eventAlertTypes[v] {
		return "info"
	}
	return v
}

func priorityName(e datadogV1.EventCreateRequest) string {
	p := e.Priority.Get()
	if p == nil {
		return "normal"
	}
	v := strings.ToLower(string(*p))
	if !eventPriorities[v] {
		return "normal"
	}
	return v
}

// HandleDistributionPoints accepts POST /api/v1/distribution_points.
//
// The agent never calls this — it sends finished sketches to
// /api/beta/sketches. Clients send RAW VALUES here, so the sketch is built on
// our side, with the agent's own bucketing so both sources merge by key.
//
// datadogV1.DistributionPointsSeries: host, metric, points, tags -> row.
// type has exactly one legal value ("distribution") and no column; it is
// logged only when it is something else. AdditionalProperties and
// UnparsedObject are logged; a point whose elements are neither timestamp
// nor value list (DistributionPointItem.UnparsedObject) is counted.
func (a *Server) HandleDistributionPoints(c *gin.Context) {
	defer ackSeries(c)
	if isDiagnose(c) {
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[distribution_points] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	payload, err := parseDistributionPoints(body)
	if err != nil {
		log.Printf("[distribution_points] %v", err)
		return
	}
	apiLogExtra("distribution_points", "payload", payload.AdditionalProperties, payload.UnparsedObject)

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	var rows []storage.SketchRow
	badItems := 0
	for i, s := range payload.Series {
		apiLogExtra("distribution_points", "series #"+strconv.Itoa(i), s.AdditionalProperties, s.UnparsedObject)
		if t := string(s.GetType()); t != "" && t != string(datadogV1.DISTRIBUTIONPOINTSTYPE_DISTRIBUTION) {
			log.Printf("[distribution_points] series #%d type=%q (only %q is defined)", i, t, datadogV1.DISTRIBUTIONPOINTSTYPE_DISTRIBUTION)
		}
		for _, pair := range s.Points {
			for _, item := range pair {
				if item.UnparsedObject != nil {
					badItems++
				}
			}
			ts, values, ok := distributionPoint(pair)
			if !ok {
				continue
			}
			keys, counts, stats := sketch.Build(values)
			rows = append(rows, storage.SketchRow{
				TenantID: tenant, Timestamp: ts,
				Metric: s.Metric, Host: s.GetHost(), Tags: tagsToMap(s.Tags),
				Count: uint64(stats.Count), Min: stats.Min, Max: stats.Max,
				Avg: stats.Avg, Sum: stats.Sum,
				BucketKeys: keys, BucketCounts: counts,
			})
		}
	}
	if badItems > 0 {
		log.Printf("[distribution_points] %d point elements were neither a timestamp nor a value list", badItems)
	}
	a.store(storage.SketchesWriter, storage.WriteSketches{Sketches: rows}, len(rows))

	// The rows above are a projection; this is the source. The RAW VALUES
	// of every point — the sketch built above is lossy — plus type,
	// AdditionalProperties and UnparsedObject on the payload and on each
	// series. A pair distributionPoint could not split is still in
	// s.Points, its odd element kept in DistributionPointItem.UnparsedObject.
	_ = payload // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// ackSeries is the reply the agent expects after a batch of metrics.
func ackSeries(c *gin.Context) {
	c.JSON(http.StatusAccepted, gin.H{"errors": []string{}})
}

// ---------------------------------------------------------------------------
// Endpoints the agent reads from
// ---------------------------------------------------------------------------

// HandleIntakeKey serves POST /api/v2/intake-key: the delegated-auth exchange.
//
// The agent sends an EMPTY body (Content-Type says JSON, there is none) with
// Authorization: Delegated <proof> and reads
// {"data":{"attributes":{"api_key":"…"}}} — an empty api_key is an error on
// its side, so a bare 202 stalls every product configured with
// delegated_auth. A request only gets here once RequireAPIKey recognised a
// Dd-Api-Key, so that key is what the agent gets back: the proof is logged by
// scheme only, never by value.
func (a *Server) HandleIntakeKey(c *gin.Context) {
	scheme, proof, _ := strings.Cut(c.GetHeader("Authorization"), " ")
	log.Printf("[intake-key] authorization scheme=%q tenant=%s", scheme, TenantFromContext(c))

	// No body on this request; the payload is the header. The proof is the
	// delegated identity — a secret, never logged, but it is what a table
	// would key on.
	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = scheme // "Delegated"
	_ = proof  // the delegated-auth proof, as sent

	// TODO(ninjacat): tables. Map the delegated identity to a key of its own.
	c.JSON(http.StatusOK, gin.H{"data": gin.H{"attributes": gin.H{
		"api_key": c.GetHeader("Dd-Api-Key"),
	}}})
}

// HandleQuery serves GET /api/v1/query for the cluster-agent's External
// Metrics Provider (zorkian client, needs an application key too).
//
// Several queries arrive joined by "," in one request and the reply matches
// them back by query_index. Nothing is queryable here yet, so the answer is a
// well-formed EMPTY result: 200 with "series": [] marks each metric invalid on
// the agent side, which is the truth, whereas a 202 would count as an API
// error. The raw query is logged as one string: tag sets contain commas too,
// so splitting on "," would miscount.
func (a *Server) HandleQuery(c *gin.Context) {
	from, to, query := c.Query("from"), c.Query("to"), c.Query("query")
	log.Printf("[query] from=%s to=%s query=%q", from, to, query)

	// A GET: the payload is the query string, complete as it came in.
	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = from
	_ = to
	_ = query // one or more queries joined by ",", not split (tag sets contain commas)

	// TODO(ninjacat): tables. Answer from the metrics table with query_index
	// per series and pointlist timestamps in milliseconds.
	c.JSON(http.StatusOK, gin.H{
		"status": "ok", "res_type": "time_series", "query": query,
		"series": []any{},
	})
}

// HandleSymbolsQuery serves POST /api/v2/profiles/symbols/query for the host
// profiler's symbol uploader: JSON:API {buildIds, arch}. The reply lists the
// build ids Datadog ALREADY holds, and the uploader only sends what is missing
// to /api/v2/srcmap. It unmarshals the reply as a JSON:API array, so a 202
// with "{}" fails the decode and no symbols are ever uploaded. An empty array
// is the correct answer: we hold nothing yet.
func (a *Server) HandleSymbolsQuery(c *gin.Context) {
	defer c.JSON(http.StatusOK, gin.H{"data": []any{}})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[symbols/query] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	res, ok := decodeJSONAPI("symbols/query", body)
	if !ok {
		return
	}
	var buildIDs []string
	json.Unmarshal(res.attrs["buildIds"], &buildIDs)
	log.Printf("[symbols/query] arch=%s buildIds(%d): %s", res.str("arch"), len(buildIDs), apiHead(buildIDs))
	res.logUnknown("symbols/query", "buildIds", "arch")

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = res      // the JSON:API resource: type, id, every attribute raw
	_ = buildIDs // all of them, not the logged apiLogLimit

	// TODO(ninjacat): tables. Answer from the symbol store once srcmap uploads
	// are kept.
}

// ---------------------------------------------------------------------------
// Private Action Runner
// ---------------------------------------------------------------------------
//
// PAR is a pull loop steered entirely by our answers: enrollment hands out
// the identity every later request is signed with, dequeue hands out work,
// health-check hands out timing. None of its request types are importable
// (pkg/privateactionrunner is internal to the agent), so decoding stops at
// the JSON:API envelope and the attribute keys.
//
// After enrollment the runner authenticates with X-Datadog-OnPrem-JWT, not
// Dd-Api-Key — the middleware in apikey_mw.go does not know that yet.

// HandleRunnerEnroll serves POST /api/unstable/on_prem_runners and its
// api_key_only variant (the default): par.CreateRunnerRequest as JSON:API,
// attributes runner_name, runner_modes, runner_host, public_key_pem,
// agent_hostname, orch_cluster_id, agent_flavor. public_key_pem is the
// runner's signing key: a PEM block is a wall, so its SHA-256 fingerprint
// stands in for it — enough to tell two enrollments apart.
//
// The runner needs a par.CreateRunnerResponse with a non-empty runner_id and
// org_id — it builds urn:dd:apps:on-prem-runner:<region>:<org_id>:<runner_id>
// from them, splits that on ":" and signs every OPMS request with the pair,
// so runner_id must not contain a colon. Status must be exactly 200: anything
// else is retried forever with backoff. The reply type must be
// "createRunnerResponse" for jsonapi.Unmarshal to accept it.
func (a *Server) HandleRunnerEnroll(c *gin.Context) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[par/enroll] cannot read body: %v", err)
		c.Status(http.StatusBadRequest)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	res, ok := decodeJSONAPI("par/enroll", body)
	if !ok {
		c.Status(http.StatusBadRequest)
		return
	}

	var modes []string
	json.Unmarshal(res.attrs["runner_modes"], &modes)
	tenant := TenantFromContext(c)
	runnerID := uuid.NewString()
	log.Printf("[par/enroll] %s runner=%q runner_host=%s host=%s flavor=%s modes=%v cluster=%s public_key=%s tenant=%s -> runner_id=%s",
		c.FullPath(), res.str("runner_name"), res.str("runner_host"), res.str("agent_hostname"), res.str("agent_flavor"),
		modes, res.str("orch_cluster_id"), apiFingerprint(res.str("public_key_pem")), tenant, runnerID)
	res.logUnknown("par/enroll", "runner_name", "runner_modes", "runner_host", "public_key_pem",
		"agent_hostname", "orch_cluster_id", "agent_flavor")

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = res      // the JSON:API resource: type, id, every attribute raw — public_key_pem in full, not its fingerprint
	_ = modes    // runner_modes, decoded
	_ = runnerID // the id handed out below; the runner signs every later request with it

	// TODO(ninjacat): tables. Persist the runner and its public_key_pem so
	// the OPMS JWTs can be verified.
	c.Header("Content-Type", "application/vnd.api+json")
	c.JSON(http.StatusOK, gin.H{"data": gin.H{
		"type": "createRunnerResponse",
		"id":   runnerID,
		"attributes": gin.H{
			"runner_id":       runnerID,
			"org_id":          tenantOrgID(tenant),
			"runner_modes":    modes,
			"agent_hostname":  res.str("agent_hostname"),
			"orch_cluster_id": res.str("orch_cluster_id"),
			"agent_flavor":    res.str("agent_flavor"),
		},
	}})
}

// tenantOrgID derives the numeric org_id PAR insists on from the tenant.
// Stable across restarts without a table, never zero, fits the int64 the
// runner parses it into.
func tenantOrgID(tenant string) int64 {
	h := fnv.New64a()
	h.Write([]byte(tenant))
	return int64(h.Sum64()>>1) | 1
}

// HandleRunnerDequeue serves the OPMS poll for work: DequeueJSONRequest with
// runner_started_at and last_task_received_at — its only two attributes.
//
// An EMPTY 200 means "no task" and is the normal idle answer; a body would be
// parsed as a task and must carry a signed_envelope the runner verifies by
// SHA-256 of the raw bytes. Any status other than 200 is an error on its
// side, so this is the one place where the project's default 202 is wrong.
func (a *Server) HandleRunnerDequeue(c *gin.Context) {
	defer c.Status(http.StatusOK)

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[par/dequeue] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	res, ok := decodeJSONAPI("par/dequeue", body)
	if !ok {
		return
	}
	log.Printf("[par/dequeue] started_at=%s last_task=%s version=%s modes=%s",
		res.str("runner_started_at"), res.str("last_task_received_at"),
		c.GetHeader("X-Datadog-OnPrem-Version"), c.GetHeader("X-Datadog-OnPrem-Modes"))
	res.logUnknown("par/dequeue", "runner_started_at", "last_task_received_at")

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = res // the JSON:API resource: type, id, every attribute raw

	// TODO(ninjacat): tables. A task queue, and a signing key for its envelopes.
}

// HandleRunnerTaskUpdate serves publish-task-update: the outcome of a task,
// PublishTaskUpdateJSONRequest with data.id "succeed_task" or "fail_task".
// Attributes: task_id, client, action_fqn, job_id, payload{branch, outputs,
// error_code, error_details, api_error}. outputs is the action's result —
// arbitrary JSON of any size — so only its size is shown. The runner accepts
// any status here.
func (a *Server) HandleRunnerTaskUpdate(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[par/task-update] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	res, ok := decodeJSONAPI("par/task-update", body)
	if !ok {
		return
	}
	var (
		payload map[string]json.RawMessage
		client  map[string]any
	)
	json.Unmarshal(res.attrs["payload"], &payload)
	json.Unmarshal(res.attrs["client"], &client)
	p := jsonAPIResource{attrs: payload}
	log.Printf("[par/task-update] outcome=%s task=%s job=%s action=%s client: %s",
		res.id, res.str("task_id"), res.str("job_id"), res.str("action_fqn"), apiKV(client))
	log.Printf("   branch=%s error_code=%s error_details=%s api_error=%s outputs=%d B",
		orUnknown(p.str("branch")), apiRawOr(payload["error_code"], "-"),
		orUnknown(p.str("error_details")), orUnknown(p.str("api_error")), len(payload["outputs"]))
	p.logUnknown("par/task-update payload", "branch", "outputs", "error_code", "error_details", "api_error")
	res.logUnknown("par/task-update", "task_id", "client", "action_fqn", "job_id", "payload")

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = res     // the JSON:API resource: type, id ("succeed_task"/"fail_task"), every attribute raw
	_ = payload // the payload attribute, one level down: branch, outputs (in full, not its size), error_code, error_details, api_error
	_ = client  // the client attribute, decoded

	// TODO(ninjacat): tables. Task outcomes; error_code is a numeric enum.
}

// HandleRunnerHeartbeat serves the per-task heartbeat: HeartbeatJSONRequest
// with task_id, client, action_fqn, job_id. Must be exactly 200 — a 404
// tells the runner the job is gone and it stops heartbeating; anything else
// is an error.
func (a *Server) HandleRunnerHeartbeat(c *gin.Context) {
	defer c.JSON(http.StatusOK, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[par/heartbeat] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	res, ok := decodeJSONAPI("par/heartbeat", body)
	if !ok {
		return
	}
	var client map[string]any
	json.Unmarshal(res.attrs["client"], &client)
	log.Printf("[par/heartbeat] task=%s job=%s action=%s client: %s",
		res.str("task_id"), res.str("job_id"), res.str("action_fqn"), apiKV(client))
	res.logUnknown("par/heartbeat", "task_id", "client", "action_fqn", "job_id")

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = res    // the JSON:API resource: type, id, every attribute raw
	_ = client // the client attribute, decoded
}

// HandleRunnerHealthCheck serves GET runner/health-check. The body is
// ignored; the runner reads X-Server-Time (RFC 3339) and X-Retry-After-Ms.
// No retry hint means "poll at your default interval". Status must be 200.
//
// A GET with no body: there is nothing to hand over beyond the two headers
// in the log line.
func (a *Server) HandleRunnerHealthCheck(c *gin.Context) {
	log.Printf("[par/health-check] version=%s modes=%s",
		c.GetHeader("X-Datadog-OnPrem-Version"), c.GetHeader("X-Datadog-OnPrem-Modes"))
	c.Header("X-Server-Time", time.Now().UTC().Format(time.RFC3339))
	c.JSON(http.StatusOK, gin.H{})
}

// HandleActionConnections serves POST /api/v2/actions/connections:
// autoconnections.ConnectionRequest as JSON:API — name, runner_id, tags,
// integration{type, credentials}. credentials holds SECRETS: its key names
// are logged (they say which credential shape the integration uses), its
// values never. Any 2xx is accepted.
func (a *Server) HandleActionConnections(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[par/connections] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()
	res, ok := decodeJSONAPI("par/connections", body)
	if !ok {
		return
	}
	var integration map[string]json.RawMessage
	json.Unmarshal(res.attrs["integration"], &integration)
	var tags []string
	json.Unmarshal(res.attrs["tags"], &tags)
	var integrationType string
	json.Unmarshal(integration["type"], &integrationType)
	var credentials map[string]json.RawMessage
	json.Unmarshal(integration["credentials"], &credentials)
	log.Printf("[par/connections] name=%q runner=%s integration=%s credential keys: %s tags(%d): %s",
		res.str("name"), res.str("runner_id"), integrationType, apiKeys(credentials), len(tags), apiHead(tags))
	jsonAPIResource{attrs: integration}.logUnknown("par/connections integration", "type", "credentials")
	res.logUnknown("par/connections", "name", "runner_id", "tags", "integration")

	// TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = res             // the JSON:API resource: type, id, every attribute raw
	_ = tags            // all of them, not the logged apiLogLimit
	_ = integrationType // integration.type
	_ = credentials     // integration.credentials, key -> raw value. SECRETS: never into a log
	_ = integration     // the integration attribute, one level down, raw

	// TODO(ninjacat): tables. Connections, with credentials kept out of logs.
}

// jsonAPIResource is a single-resource JSON:API document,
// {"data":{"type":…,"id":…,"attributes":{…}}}, decoded no further than the
// envelope. The concrete types belong to the agent and are not importable.
type jsonAPIResource struct {
	typ, id string
	attrs   map[string]json.RawMessage
}

// str reads one attribute as a string; anything else yields "".
func (r jsonAPIResource) str(key string) string {
	var s string
	json.Unmarshal(r.attrs[key], &s)
	return s
}

// logUnknown reports attributes the handler did not name. Each handler lists
// what it read, so a new attribute shows up here rather than vanishing.
func (r jsonAPIResource) logUnknown(label string, known ...string) {
	apiLogUnknownKeys(label, "attributes", r.attrs, known...)
}

// decodeJSONAPI pulls the resource out of body and logs its type, id and
// attribute keys under label. The bool is false when the body is not a
// JSON:API document. Envelope members beyond data/type/id/attributes (meta,
// links, included, relationships) are not expected from PAR; when they do
// arrive their keys are logged.
func decodeJSONAPI(label string, body []byte) (jsonAPIResource, bool) {
	var doc map[string]json.RawMessage
	if err := json.Unmarshal(body, &doc); err != nil {
		log.Printf("[%s] JSON: %v", label, err)
		return jsonAPIResource{}, false
	}
	var data map[string]json.RawMessage
	if err := json.Unmarshal(doc["data"], &data); err != nil {
		log.Printf("[%s] no JSON:API data object: %v", label, err)
		return jsonAPIResource{}, false
	}
	var r jsonAPIResource
	json.Unmarshal(data["type"], &r.typ)
	json.Unmarshal(data["id"], &r.id)
	json.Unmarshal(data["attributes"], &r.attrs)
	log.Printf("[%s] JSON:API type=%s id=%s attrs: %s", label, r.typ, r.id, apiKeys(r.attrs))
	apiLogUnknownKeys(label, "document", doc, "data")
	apiLogUnknownKeys(label, "data", data, "type", "id", "attributes")
	return r, true
}

// ---------------------------------------------------------------------------
// Log helpers: bounded renderings of decoded values
// ---------------------------------------------------------------------------

// apiLogExtra reports a datadog-api-client model's AdditionalProperties —
// every key the model does not declare, decoded with UseNumber so 64-bit
// values keep their digits — and its UnparsedObject, which the generated
// UnmarshalJSON fills INSTEAD of the fields when the shape does not fit.
func apiLogExtra(label, what string, extra, unparsed map[string]interface{}) {
	if len(extra) > 0 {
		log.Printf("[%s] %s undeclared keys (no column yet): %s", label, what, apiKV(extra))
	}
	if unparsed != nil {
		log.Printf("[%s] %s did not fit the model, kept raw, keys: %s", label, what, apiKeys(unparsed))
	}
}

// apiLogUnknownKeys reports the keys of m that are not in known.
func apiLogUnknownKeys[V any](label, what string, m map[string]V, known ...string) {
	var unknown []string
	for k := range m {
		if !slices.Contains(known, k) {
			unknown = append(unknown, k)
		}
	}
	if len(unknown) > 0 {
		slices.Sort(unknown)
		log.Printf("[%s] %s has keys this handler does not know: %s", label, what, apiHead(unknown))
	}
}

// apiKV renders a decoded object as sorted key=value pairs, bounded.
func apiKV(m map[string]any) string {
	if len(m) == 0 {
		return "-"
	}
	keys := slices.Sorted(maps.Keys(m))
	parts := make([]string, 0, len(keys))
	for _, k := range keys {
		parts = append(parts, k+"="+apiVal(m[k]))
	}
	return apiHead(parts)
}

// apiVal renders one decoded JSON value in a line-friendly way: containers
// by size, scalars as they are, json.Number untouched so a 64-bit id is not
// rounded through float64.
func apiVal(v any) string {
	switch x := v.(type) {
	case nil:
		return "null"
	case string:
		return apiTrunc(strconv.Quote(x), 80)
	case json.Number:
		return x.String()
	case map[string]any:
		return fmt.Sprintf("{%d keys}", len(x))
	case []any:
		return fmt.Sprintf("[%d]", len(x))
	default:
		return apiTrunc(fmt.Sprint(x), 80)
	}
}

// apiKeys lists a map's keys, sorted and bounded.
func apiKeys[V any](m map[string]V) string {
	if len(m) == 0 {
		return "(none)"
	}
	return apiHead(slices.Sorted(maps.Keys(m)))
}

// apiTally renders name -> count, most frequent first, bounded.
func apiTally(m map[string]int) string {
	if len(m) == 0 {
		return "-"
	}
	keys := slices.Sorted(maps.Keys(m))
	slices.SortStableFunc(keys, func(a, b string) int { return m[b] - m[a] })
	parts := make([]string, 0, len(keys))
	for _, k := range keys {
		parts = append(parts, fmt.Sprintf("%s x%d", k, m[k]))
	}
	return apiHead(parts)
}

// apiHead joins at most apiLogLimit items and says how many were left out.
func apiHead(items []string) string {
	if len(items) == 0 {
		return "-"
	}
	if len(items) <= apiLogLimit {
		return strings.Join(items, ", ")
	}
	return strings.Join(items[:apiLogLimit], ", ") + fmt.Sprintf(" ... %d more", len(items)-apiLogLimit)
}

func apiTrunc(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n] + "…"
}

func apiPresent(ok bool) string {
	if ok {
		return "present"
	}
	return "absent"
}

// apiUnmarshalNumber decodes raw into v with numbers kept as json.Number, so
// a 64-bit id or an epoch timestamp is not rounded through float64. apiVal
// already renders json.Number untouched.
func apiUnmarshalNumber(raw json.RawMessage, v any) error {
	dec := json.NewDecoder(bytes.NewReader(raw))
	dec.UseNumber()
	return dec.Decode(v)
}

// apiRawPresent is true when a raw JSON value is a non-empty string.
func apiRawPresent(raw json.RawMessage) bool {
	var s string
	return json.Unmarshal(raw, &s) == nil && s != ""
}

// apiRawOr renders a raw JSON scalar, or fallback when it is absent.
func apiRawOr(raw json.RawMessage, fallback string) string {
	if len(raw) == 0 {
		return fallback
	}
	return apiTrunc(string(raw), 80)
}

// apiFingerprint stands in for a PEM block: the first 16 hex digits of its
// SHA-256, or "absent".
func apiFingerprint(pem string) string {
	if pem == "" {
		return "absent"
	}
	sum := sha256.Sum256([]byte(pem))
	return "sha256:" + hex.EncodeToString(sum[:8])
}
