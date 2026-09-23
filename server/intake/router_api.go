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
// without a comment saying why. What has a column is stored; what cannot be
// decoded goes to raw_payloads (storeRaw); what is deliberately kept out of
// the log — secrets, walls of text — says so next to the code. The log lines
// stay, because they are how this protocol was discovered, but logging is
// never the only sink.
//
// WHERE THE ROWS GO:
//
//	/api/v1/series, /api/v2/series        metrics
//	/api/beta/sketches                    sketches + agent_batch_metadata
//	/api/v1/distribution_points           sketches
//	/api/v1/check_run                     check_runs
//	/api/v1/events                        events
//	/intake/  events variant              events
//	          agent_checks variant        agent_checks + external_host_tags
//	          gohai/systemStats variant   hosts
//	          resources (legacy V5)       raw_payloads
//	/api/v1/metadata                      agent_metadata
//	/api/v2/intake-key                    delegated_auth_requests
//	/api/v2/profiles/symbols/query        symbol_queries
//	PAR enrollment / task-update /
//	    heartbeat / dequeue / connections runner_enrollments,
//	                                      runner_task_updates,
//	                                      runner_heartbeats, runner_dequeues,
//	                                      action_connections
//	GET /api/v1/query                     nothing: a read endpoint with no
//	                                      query engine behind it yet

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
//	Resources       host -> its own column; every other type (device, ...)
//	                -> the resources map on the row
//	Metric, Tags, Points, Type, Unit, SourceTypeName, Interval -> row
//	Metadata        Origin{product, category, service} -> three columns
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

	var payload gogen.MetricPayload
	if bodyIsJSON(c, body) {
		payload, err = parseSeriesJSONv2(body)
	} else {
		payload, err = parseSeriesProtobuf(body)
	}
	if err != nil {
		log.Printf("[v2/series] %v", err)
		a.storeRaw(c, "series", "decode_error", err.Error(), body)
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
		// Every resource type but host goes into the row's resources map:
		// the agent writes exactly two (host and device, iterable_series.go)
		// but the wire allows any, and a device series must not be
		// indistinguishable from a host one after the fact.
		seriesResources := map[string]string{}
		for _, r := range s.Resources {
			if r.GetType() != "host" {
				seriesResources[r.GetType()] = r.GetName()
				resources[r.GetType()+"="+r.GetName()]++
			}
		}
		origin := s.GetMetadata().GetOrigin()
		origins[apiOriginName(s.GetMetadata())]++
		// The agent also writes Origin field 3 (metric_type = 9, "do not
		// index") which the published proto marks reserved, so gogen parks
		// it in XXX_unrecognized. Counted, not decoded: a reserved field has
		// no name to decode into, so there is nothing to put in a column.
		// origin is nil when the series carried no metadata (or metadata with
		// no origin) at all — a request field XXX_unrecognized cannot be read
		// off, direct field access on a nil *Origin panics, unlike the
		// GetOriginX() calls below which are nil-safe generated methods.
		if origin != nil && len(origin.XXX_unrecognized) > 0 {
			noIndex++
		}
		for _, p := range s.Points {
			points = append(points, storage.MetricPoint{
				TenantID: tenant, Timestamp: wireTime(p.GetTimestamp()),
				Metric: s.Metric, Host: host,
				MetricType: metricTypeName(int32(s.Type)), SourceType: s.SourceTypeName,
				Unit: s.Unit, Interval: uint32(s.Interval),
				Value: p.GetValue(), Tags: tagsToMultiMap(s.Tags),
				OriginProduct:  origin.GetOriginProduct(),
				OriginCategory: origin.GetOriginCategory(),
				OriginService:  origin.GetOriginService(),
				Resources:      seriesResources,
			})
		}
	}
	apiLogSeriesExtras("v2/series", len(payload.Series), len(points), origins, resources, noIndex)
	a.store(storage.MetricsWriter, storage.WriteMetrics{Points: points}, len(points))
}

// apiLogSeriesExtras reports what a series batch carried beyond its columns.
// One line per batch; the origin and resource tallies are bounded.
func apiLogSeriesExtras(label string, series, points int, origins, resources map[string]int, noIndex int) {
	log.Printf("[%s] %d series, %d points, origins: %s", label, series, points, apiTally(origins))
	if len(resources) > 0 {
		log.Printf("   non-host resources (stored in metrics.resources): %s", apiTally(resources))
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
// stored from there; device goes into the row's resources map under the key
// v2 would have used for it, so one query spans both wire versions. Anything
// else undeclared lands in the extra map, JSON-encoded.
//
// A series the model could not parse at all (UnparsedObject set) has no
// metric name and no points, so there is no row to build — its bytes go to
// raw_payloads instead of being counted and dropped.
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

	payload, err := parseSeriesJSONv1(body)
	if err != nil {
		log.Printf("[v1/series] %v", err)
		a.storeRaw(c, "series", "decode_error", err.Error(), body)
		return
	}
	apiLogExtra("v1/series", "payload", payload.AdditionalProperties, payload.UnparsedObject)
	if payload.UnparsedObject != nil {
		// The whole body fit no series list at all: nothing below will find a
		// metric in it, so the bytes are the only thing left to keep.
		a.storeRaw(c, "series", "unexpected_shape",
			"v1 payload did not fit datadogV1.MetricsPayload", body)
	}
	if len(payload.AdditionalProperties) > 0 {
		// A key beside `series` at the TOP of the payload. It belongs to the
		// batch, not to any one point, so there is no row to hang it on: the
		// metrics extra map is per series on purpose, and copying a
		// batch-level value onto every point would multiply it by the point
		// count. The body goes to raw_payloads instead, once.
		a.storeRaw(c, "series", "unexpected_shape",
			"v1 payload carried undeclared top-level keys: "+apiKeys(payload.AdditionalProperties), body)
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	// The originals of the series, for the ones the model could not fit.
	rawSeries := jsonFieldElements(body, "series")

	var (
		points    []storage.MetricPoint
		resources = map[string]int{}
		extra     = map[string]int{}
		unparsed  int
		badPoints int
	)
	for i, s := range payload.Series {
		if s.UnparsedObject != nil {
			// The generated UnmarshalJSON swallows a structurally wrong
			// series (points not an array, say) into UnparsedObject and
			// returns no error, so this series has no metric and no points.
			// The bytes it arrived as are kept in preference to
			// UnparsedObject, which was decoded without UseNumber and would
			// hand raw_payloads a rounded copy of any large number in it.
			unparsed++
			a.storeRaw(c, "series", "unexpected_shape",
				"v1 series #"+strconv.Itoa(i)+" did not fit datadogV1.Series",
				rawElementAt(rawSeries, i, s.UnparsedObject))
			continue
		}
		interval := uint32(0)
		if iv := s.Interval.Get(); s.Interval.IsSet() && iv != nil {
			interval = uint32(*iv)
		}
		// v1 has no resource list; the agent sends `device` as an undeclared
		// key. Store it under the type name v2 uses for the same thing.
		seriesResources := map[string]string{}
		if d := attrString(s.AdditionalProperties, "device"); d != "" {
			seriesResources["device"] = d
			resources["device="+d]++
		}
		for k := range s.AdditionalProperties {
			switch k {
			case "device", "source_type_name", "unit":
			default:
				extra[k]++
			}
		}
		seriesExtra := apiExtraAny(s.AdditionalProperties, "device", "source_type_name", "unit")
		rawPoints := jsonFieldElements(rawElementAt(rawSeries, i, nil), "points")
		for j, p := range s.Points {
			// A v1 point is a bare [timestamp, value] pair, both nullable.
			// A pair missing either element is not a point, and the series it
			// came from is stored anyway through its other points, so the
			// pair itself is the smallest thing worth keeping.
			if len(p) < 2 || p[0] == nil || p[1] == nil {
				badPoints++
				a.storeRaw(c, "series", "unexpected_shape",
					"v1 series #"+strconv.Itoa(i)+" ("+s.Metric+") point #"+strconv.Itoa(j)+
						" has a nil timestamp or value",
					rawElementAt(rawPoints, j, p))
				continue
			}
			points = append(points, storage.MetricPoint{
				TenantID: tenant, Timestamp: wireTime(int64(*p[0])),
				Metric: s.Metric, Host: s.GetHost(),
				MetricType: normalizeTypeWord(s.GetType()),
				SourceType: attrString(s.AdditionalProperties, "source_type_name"),
				Unit:       attrString(s.AdditionalProperties, "unit"),
				Interval:   interval,
				Value:      *p[1], Tags: tagsToMultiMap(s.Tags),
				// v1 carries no origin metadata at all, so the three origin
				// columns stay 0 — which is also what "not sent" means there.
				Resources: seriesResources,
				Extra:     seriesExtra,
			})
		}
	}
	// v1 carries no origin metadata; the tally is nil so the line reads "-".
	apiLogSeriesExtras("v1/series", len(payload.Series), len(points), nil, resources, 0)
	if len(extra) > 0 {
		log.Printf("   undeclared series keys (stored in metrics.extra): %s", apiTally(extra))
	}
	if unparsed > 0 {
		log.Printf("[v1/series] %d series could not be decoded into datadogV1.Series (UnparsedObject set), kept in raw_payloads", unparsed)
	}
	if badPoints > 0 {
		log.Printf("[v1/series] %d points had a nil timestamp or value, kept in raw_payloads", badPoints)
	}
	a.store(storage.MetricsWriter, storage.WriteMetrics{Points: points}, len(points))
}

// HandleSketches serves /api/beta/sketches: DDSketch histograms the agent has
// already built. Bucket keys follow the agent's own mapping — see package
// sketch for why that matters.
//
// gogen.SketchPayload, field by field:
//
//	Sketches         -> rows, see below
//	Metadata         CommonMetadata about the SENDER, not the metric: agent
//	                 version, timezone, epoch, IPs -> one agent_batch_metadata
//	                 row per request; api_key -> neither logged nor stored,
//	                 the request already authenticated
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
//	                          DDSketch buckets, so there is NOWHERE to put one
//	                          -> raw_payloads, whole, rather than a count in a
//	                          log line: a third-party sender still using it
//	                          would otherwise lose everything it sent
//	Metadata               Origin -> three columns
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

	var payload gogen.SketchPayload
	if bodyIsJSON(c, body) {
		payload, err = parseSketchesJSON(body)
	} else {
		payload, err = parseSketchesProtobuf(body)
	}
	if err != nil {
		log.Printf("[sketches] %v", err)
		a.storeRaw(c, "sketches", "decode_error", err.Error(), body)
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
			// A GK-style Distribution carries raw values, ranks and an
			// unmerged buffer — none of which maps onto a DDSketch bucket
			// row, and re-bucketing it would invent numbers the sender never
			// sent. The whole sketch goes to raw_payloads instead, metric
			// name and tags included, so it can be read back in full.
			a.storeRaw(c, "sketches", "no_schema",
				"legacy pre-DDSketch Distribution for "+s.Metric+" has no row shape",
				apiJSONBytes(s))
		}
		origin := s.GetMetadata().GetOrigin()
		for _, d := range s.Dogsketches {
			rows = append(rows, storage.SketchRow{
				TenantID: tenant, Timestamp: wireTime(d.Ts),
				Metric: s.Metric, Host: s.Host, Tags: tagsToMultiMap(s.Tags),
				Count: uint64(d.Cnt), Min: d.Min, Max: d.Max, Avg: d.Avg, Sum: d.Sum,
				BucketKeys: d.K, BucketCounts: d.N,
				OriginProduct:  origin.GetOriginProduct(),
				OriginCategory: origin.GetOriginCategory(),
				OriginService:  origin.GetOriginService(),
			})
		}
	}

	m := payload.Metadata
	log.Printf("[sketches] %d sketches, %d dogsketches, origins: %s; sender agent=%s tz=%s epoch=%.0f internal_ip=%s public_ip=%s api_key=%s",
		len(payload.Sketches), len(rows), apiTally(origins),
		orUnknown(m.AgentVersion), orUnknown(m.Timezone), m.CurrentEpoch,
		orUnknown(m.InternalIp), orUnknown(m.PublicIp), apiPresent(m.ApiKey != ""))
	if len(legacy) > 0 {
		log.Printf("[sketches] legacy Distribution entries (pre-DDSketch, kept in raw_payloads): %s", apiTally(legacy))
	}
	a.store(storage.SketchesWriter, storage.WriteSketches{Sketches: rows}, len(rows))
	a.storeBatchMetadata(tenant, "sketches", m)
}

// storeBatchMetadata records who sent a batch, as opposed to what was in it.
//
// CommonMetadata describes the SENDER — agent version, timezone, its clock and
// both of its IPs. None of it belongs on a sketch row, where it would repeat
// once per bucket, and all of it is what somebody wants when a host's numbers
// look wrong: a clock that disagrees with ours explains a gap on a chart
// faster than anything else.
//
// ApiKey is a field of CommonMetadata and is deliberately not carried over:
// the request already authenticated, and a credential does not belong in a
// table with a TTL. An all-empty block is not stored at all — the proto marks
// the field required, so an absent one arrives as a zero struct rather than as
// nothing.
func (a *Server) storeBatchMetadata(tenant, intake string, m gogen.CommonMetadata) {
	if tenant == "" {
		return
	}
	if m.AgentVersion == "" && m.Timezone == "" && m.CurrentEpoch == 0 &&
		m.InternalIp == "" && m.PublicIp == "" {
		return
	}
	a.store(storage.AgentBatchMetadataWriter, storage.WriteAgentBatchMetadata{
		Batches: []storage.AgentBatchMetadataRow{{
			TenantID:     tenant,
			ReceivedAt:   time.Now().UTC(),
			Intake:       intake,
			AgentVersion: m.AgentVersion,
			Timezone:     m.Timezone,
			CurrentEpoch: m.CurrentEpoch,
			InternalIP:   m.InternalIp,
			PublicIP:     m.PublicIp,
		}},
	}, 1)
}

// HandleCheckRun serves /api/v1/check_run.
//
// A check answers "is this thing working", not "how much of it is there".
// Monitors of the "service is down" kind are built on these.
//
// datadogV1.ServiceCheck: check, host_name, message, status, tags, timestamp
// all have columns. The agent's own struct (servicecheck.ServiceCheck) has
// exactly these six keys, so AdditionalProperties should stay empty — when it
// does not, a newer agent or a third-party client grew a field, which is
// precisely the case where the value is worth more than the count of it, so it
// goes into the extra column. A check the model could not fit at all
// (UnparsedObject) has no name and no status, so there is no row to build and
// its bytes go to raw_payloads.
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

	runs, originals, undecodable, err := parseCheckRuns(body)
	if err != nil {
		log.Printf("[check_run] %v", err)
		a.storeRaw(c, "check_run", "decode_error", err.Error(), body)
		return
	}
	for i, raw := range undecodable {
		a.storeRaw(c, "check_run", "unexpected_shape",
			"check #"+strconv.Itoa(i)+" is not a datadogV1.ServiceCheck", raw)
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	rows := make([]storage.CheckRunRow, 0, len(runs))
	for i, r := range runs {
		apiLogExtra("check_run", "check #"+strconv.Itoa(i), r.AdditionalProperties, r.UnparsedObject)
		if r.UnparsedObject != nil {
			// Either nothing decoded, or the check carried a status outside
			// 0..3 — which leaves Status at 0, and storing an unreadable
			// check as OK is the one thing a check_run table must never do.
			// The bytes are kept instead.
			a.storeRaw(c, "check_run", "unexpected_shape",
				"check #"+strconv.Itoa(i)+" did not fit datadogV1.ServiceCheck",
				rawElementAt(originals, i, r.UnparsedObject))
			continue
		}
		rows = append(rows, storage.CheckRunRow{
			TenantID: tenant, Timestamp: wireTime(r.GetTimestamp()),
			CheckName: r.Check, Host: r.HostName,
			Status: statusName(int(r.Status)), Message: r.GetMessage(),
			Tags:  tagsToMultiMap(r.Tags),
			Extra: apiExtraAny(r.AdditionalProperties),
		})
	}
	a.store(storage.ChecksWriter, storage.WriteCheckRuns{Runs: rows}, len(rows))
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

	var m map[string]json.RawMessage
	if err := json.Unmarshal(body, &m); err != nil {
		log.Printf("[%s] JSON: %v", label, err)
		a.storeRaw(c, label, "decode_error", err.Error(), body)
		return
	}

	_, hasEvents := m["events"]
	_, hasChecks := m["agent_checks"]
	_, hasGohai := m["gohai"]
	_, hasStats := m["systemStats"]
	_, hasResources := m["resources"]
	switch {
	case hasEvents:
		a.intakeEvents(c, label, m)
	case hasChecks:
		a.intakeAgentChecks(c, label, m)
	case hasGohai || hasStats:
		a.intakeHost(c, label, m)
	case hasResources && len(m) == 1:
		// The legacy V5 process snapshot. The process intake owns that
		// format and parses it in full; a second decoder here would be two
		// decoders to keep in step, so the payload is kept whole instead.
		intakeLogProcesses(label, m)
		a.storeRaw(c, label, "no_schema",
			"legacy V5 resources snapshot — the process intake owns this format", body)
	default:
		// A fifth producer, or a known one with a new top-level key. Nothing
		// here knows what it is, which is exactly what raw_payloads is for.
		log.Printf("[%s] unrecognised shape, keys: %s", label, apiKeys(m))
		a.storeRaw(c, label, "unexpected_shape",
			"no known /intake/ variant key, keys: "+apiKeys(m), body)
	}
}

// intakeEvents stores agent events. Each entry is pkg/metrics/event.Event as
// JSON: msg_title, msg_text, timestamp, priority, host, tags, alert_type,
// aggregation_key, source_type_name, event_type. apiKey is deliberately empty
// on this shape (the key travels in the header) and is not logged.
//
// They feed storage.EventsWriter — the SAME table as /api/v1/events. The two
// sources fill different subsets of it (the agent sends an event_type and no
// related_event_id, the public API the reverse), and splitting them would mean
// a UNION on every chart that asks "what happened around this time".
func (a *Server) intakeEvents(c *gin.Context, label string, m map[string]json.RawMessage) {
	// Decoded whole, before any logging: source -> events, every event a
	// free map with numbers kept as json.Number so a timestamp is not
	// rounded through float64. The log below is bounded; this is not.
	var bySource map[string][]map[string]any
	if err := apiUnmarshalNumber(m["events"], &bySource); err != nil {
		log.Printf("[%s] events: %v", label, err)
		a.storeRaw(c, label, "unexpected_shape",
			"events is not a source -> events map: "+err.Error(), m["events"])
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

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	// Sorted, so a batch lands in the table in an order that does not depend
	// on Go's map iteration — two replays of the same payload then produce the
	// same rows, which is what makes a capture reproducible.
	var rows []storage.EventRow
	for _, source := range slices.Sorted(maps.Keys(bySource)) {
		for _, ev := range bySource[source] {
			rows = append(rows, intakeEventRow(tenant, source, host, ev))
		}
	}
	a.store(storage.EventsWriter, storage.WriteEvents{Events: rows}, len(rows))
}

// intakeEventRow turns one pkg/metrics/event.Event into the shared events row.
//
// Pure, and separate from the handler, because the mapping is the part worth
// pinning: the agent's field names differ from the public API's for the same
// three things (msg_title/msg_text/timestamp against title/text/date_happened)
// and nothing but a test says they mean the same column.
//
// source is the key of the events map — the agent groups events by source type
// — and stands in when an event does not repeat it in source_type_name.
// Likewise the payload's internalHostname stands in for an event with no host
// of its own: the events rode in on that host's request, which is also how
// Datadog's own intake reads them.
func intakeEventRow(tenant, source, payloadHost string, ev map[string]any) storage.EventRow {
	id := uuid.New()

	alertRaw := evString(ev, "alert_type")
	priorityRaw := evString(ev, "priority")

	return storage.EventRow{
		TenantID:   tenant,
		Timestamp:  wireTime(evInt64(ev, "timestamp")),
		EventID:    id,
		EventIDNum: binary.BigEndian.Uint64(id[:8]) >> 1, // >>1 keeps it inside int64
		Title:      evString(ev, "msg_title"),
		Text:       evString(ev, "msg_text"),
		Host:       orString(evString(ev, "host"), payloadHost),
		AlertType:  coerceAlertType(alertRaw),
		Priority:   coercePriority(priorityRaw),

		AggregationKey: evString(ev, "aggregation_key"),
		SourceTypeName: orString(evString(ev, "source_type_name"), source),
		Tags:           tagsToMultiMap(evStrings(ev, "tags")),

		EventType:    evString(ev, "event_type"),
		AlertTypeRaw: alertRaw,
		PriorityRaw:  priorityRaw,
		Extra: apiExtraAny(ev, "msg_title", "msg_text", "timestamp", "host",
			"alert_type", "priority", "aggregation_key", "source_type_name",
			"event_type", "tags"),
	}
}

// evString, evInt64 and evStrings read one value out of a free JSON object
// decoded with UseNumber. They exist because none of the /intake/ producers
// has an importable type, so every field is read by name and a value of the
// wrong type has to mean "absent" rather than a panic.
func evString(ev map[string]any, key string) string {
	s, _ := ev[key].(string)
	return s
}

func evInt64(ev map[string]any, key string) int64 {
	switch v := ev[key].(type) {
	case json.Number:
		// json.Number, never float64: an epoch second is small, but the same
		// helper reads ids elsewhere and 2^53 is not a limit worth meeting.
		if n, err := v.Int64(); err == nil {
			return n
		}
	case float64:
		return int64(v)
	}
	return 0
}

func evStrings(ev map[string]any, key string) []string {
	list, ok := ev[key].([]any)
	if !ok {
		return nil
	}
	out := make([]string, 0, len(list))
	for _, v := range list {
		if s, ok := v.(string); ok {
			out = append(out, s)
		}
	}
	return out
}

// orString is firstNonEmpty for two values, for the many "the item said it, or
// the envelope did" fallbacks in this file.
func orString(primary, fallback string) string {
	if primary != "" {
		return primary
	}
	return fallback
}

// intakeAgentChecks stores collectorimpl.Payload: the V5-era check status list
// and external host tags.
//
// agent_checks entries are positional arrays (check name, source type,
// instance id, status, message, ...) with no published type and no key names
// anywhere on the wire, so the five documented positions become columns and
// anything after them is kept as a JSON array — a sixth element appearing one
// day is exactly the change this table exists to make visible.
//
// external_host_tags is [[hostname, {source: [tags]}], ...]: an agent telling
// us about hosts it is NOT running on. They get their own table because the
// host they describe is not the host that sent them.
func (a *Server) intakeAgentChecks(c *gin.Context, label string, m map[string]json.RawMessage) {
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
			a.storeRaw(c, label, "unexpected_shape",
				"external_host_tags["+strconv.Itoa(i)+"] is not a [hostname, tags] pair", raw)
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

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	now := time.Now().UTC()
	metaJSON := string(m["meta"])

	checkRows := make([]storage.AgentCheckRow, 0, len(checks))
	for i, positional := range checks {
		if positional == nil {
			// Not an array at all: the only copy of whatever it is are the
			// bytes at this index.
			a.storeRaw(c, label, "unexpected_shape",
				"agent_checks["+strconv.Itoa(i)+"] is not a positional array", checksRaw[i])
			continue
		}
		checkRows = append(checkRows, agentCheckRow(tenant, now, host, version, id, metaJSON, positional))
	}
	a.store(storage.AgentChecksWriter, storage.WriteAgentChecks{Checks: checkRows}, len(checkRows))

	var tagRows []storage.ExternalHostTagsRow
	for _, e := range extTags {
		// One row per SOURCE: a host carries several at once (the vSphere
		// collector's own tags and the cloud provider's, say) and flattening
		// them would lose which of the two said what.
		for _, source := range slices.Sorted(maps.Keys(e.Tags)) {
			tagRows = append(tagRows, storage.ExternalHostTagsRow{
				TenantID: tenant, ReceivedAt: now,
				Host: e.Host, Source: source,
				Tags: tagsToMultiMap(e.Tags[source]),
			})
		}
	}
	a.store(storage.ExternalHostTagsWriter, storage.WriteExternalHostTags{Tags: tagRows}, len(tagRows))
}

// agentCheckRow reads one positional agent_checks entry.
//
// The positions are the wire's only documentation: 0 check name, 1 source
// type, 2 instance id, 3 status, 4 message. A SHORTER array is normal (an
// entry may stop after the status), which is why the row's Status is a pointer
// — position 3 missing must not be reported as 0, which means OK.
func agentCheckRow(tenant string, at time.Time, host, version, uuid, meta string, positional []any) storage.AgentCheckRow {
	row := storage.AgentCheckRow{
		TenantID: tenant, ReceivedAt: at,
		Hostname: host, AgentVersion: version, UUID: uuid,
		Meta: meta,
	}
	row.CheckName = posString(positional, 0)
	row.SourceType = posString(positional, 1)
	row.InstanceID = posString(positional, 2)
	if len(positional) > 3 {
		if n, ok := posInt64(positional, 3); ok {
			row.Status = &n
		}
	}
	row.Message = posString(positional, 4)
	if len(positional) > 5 {
		// Everything the agent appended after the five documented positions,
		// as a JSON array so the POSITIONS survive — a map keyed by index
		// would be a lie about a format that has no names.
		row.PositionalExtra = string(apiJSONBytes(positional[5:]))
	}
	return row
}

// posString and posInt64 read one element of a positional array. Out of range
// and wrong type both mean "the entry stopped before here", which the wire
// allows and which must not become a zero that looks like data.
func posString(positional []any, i int) string {
	if i >= len(positional) {
		return ""
	}
	s, _ := positional[i].(string)
	return s
}

func posInt64(positional []any, i int) (int64, bool) {
	if i >= len(positional) {
		return 0, false
	}
	switch v := positional[i].(type) {
	case json.Number:
		if n, err := v.Int64(); err == nil {
			return n, true
		}
	case float64:
		return int64(v), true
	}
	return 0, false
}

// intakeHost pulls out host metadata (hostimpl.Payload). The payload is loose
// and shifts between agent versions, so fields are read by name rather than
// through a fixed struct. Key by key:
//
//	internalHostname, agentVersion, os     -> row
//	gohai (a STRING holding JSON)          platform, cpu, memory -> their own
//	                                       columns; filesystem, network -> the
//	                                       JSON columns of the same name; any
//	                                       other section -> gohai_extra
//	host-tags                              EVERY source -> host_tags; the
//	                                       "system" source also fills tags,
//	                                       which is what charts filter on
//	apiKey                                 the REAL key: presence only, never
//	                                       logged and never stored
//	uuid, agent-flavor, python, systemStats, meta, container-meta, network,
//	logs, install-method, proxy-info, otlp, fips_mode,
//	fips_proxy_enabled                     -> one column each
//	resources                              the legacy V5 process snapshot,
//	                                       verbatim: the process intake owns
//	                                       that format and parsing it twice
//	                                       would be two decoders to keep in
//	                                       step
//	anything else                           -> intake_extra
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

	tenant := TenantFromContext(c)
	if tenant == "" || hostname == "" {
		return
	}
	row := hostRow(tenant, hostname, agentVersion, osName, time.Now().UTC(), m, gohai, hostTags)
	a.store(storage.HostsWriter, storage.WriteHosts{Hosts: []storage.HostRow{row}}, 1)
}

// intakeHostKnownKeys are the envelope keys hostRow reads by name. Everything
// else goes to intake_extra — including apiKey, which is removed separately
// because a credential must not reach a column by way of a catch-all.
var intakeHostKnownKeys = []string{
	"apiKey", "internalHostname", "agentVersion", "os", "uuid", "agent-flavor",
	"python", "gohai", "host-tags", "systemStats", "meta", "network", "logs",
	"install-method", "proxy-info", "otlp", "container-meta",
	"fips_mode", "fips_proxy_enabled", "resources",
}

// gohaiKnownSections are the gohai sections with columns of their own; the
// rest land in gohai_extra.
//
// gohai.network is deliberately NOT one of them. The envelope has a top-level
// `network` of its own — the host's addresses, from comp/metadata/host — and
// that is what the network column holds. gohai's section is the interface
// inventory, a different thing with the same name, so it keeps its name inside
// gohai_extra rather than overwriting the other.
var gohaiKnownSections = []string{"platform", "cpu", "memory", "filesystem"}

// hostRow builds the hosts row from the /intake/ envelope.
//
// Pure, because this is where the fidelity of the fleet inventory is decided:
// which sections keep their JSON, which flatten to a map, which tag sources
// survive. A handler test cannot tell "the column was empty because the agent
// sent nothing" from "the decoder dropped it"; a table test on this function
// can.
func hostRow(tenant, hostname, agentVersion, osName string, seenAt time.Time,
	m map[string]json.RawMessage, gohai map[string]json.RawMessage,
	hostTags map[string][]string) storage.HostRow {

	row := storage.HostRow{
		TenantID:     tenant,
		Host:         hostname,
		SeenAt:       seenAt,
		AgentVersion: agentVersion,
		OS:           osName,
		Platform:     gohaiSection(gohai, "platform"),
		CPU:          gohaiSection(gohai, "cpu"),
		Memory:       gohaiSection(gohai, "memory"),
		// tags stays the "system" source alone: it is the column every chart
		// filters on, and mixing a cloud provider's tag set into it would
		// change what existing queries mean. Every source, that one included,
		// is in host_tags.
		Tags:     tagsToMultiMap(hostTags["system"]),
		HostTags: hostTags,

		// Kept as JSON text, not flattened: their shape moves between agent
		// versions (filesystem is a list of mounts, network arrives as null on
		// containers) and a column per key of a moving structure is a
		// migration per agent release.
		Filesystem: string(gohai["filesystem"]),
		GohaiExtra: apiExtraRaw(gohai, gohaiKnownSections...),

		Network:   string(m["network"]),
		Meta:      string(m["meta"]),
		Logs:      string(m["logs"]),
		OTLP:      string(m["otlp"]),
		Resources: string(m["resources"]),

		FIPSMode:         apiRawBool(m["fips_mode"]),
		FIPSProxyEnabled: apiRawBool(m["fips_proxy_enabled"]),

		IntakeExtra: apiExtraRaw(m, intakeHostKnownKeys...),
	}

	json.Unmarshal(m["uuid"], &row.UUID)
	json.Unmarshal(m["agent-flavor"], &row.AgentFlavor)
	json.Unmarshal(m["python"], &row.PythonVersion)

	// Flat objects whose keys ARE the question ("which install method?"), so
	// they stay queryable maps rather than text. Values are JSON-encoded:
	// install-method's are strings, but otlp-adjacent producers send numbers
	// and booleans, and one column cannot have a type per key.
	row.SystemStats = intakeObjectMap(m["systemStats"])
	row.InstallMethod = intakeObjectMap(m["install-method"])
	row.ProxyInfo = intakeObjectMap(m["proxy-info"])
	row.ContainerMeta = intakeObjectMap(m["container-meta"])

	return row
}

// intakeObjectMap flattens a JSON object into the Map(String, String) shape,
// values JSON-encoded. A value that is not an object at all (null, a list)
// yields nil rather than an error: this payload is loose by nature and one
// odd section must not cost the whole host row.
func intakeObjectMap(raw json.RawMessage) map[string]string {
	if len(raw) == 0 {
		return nil
	}
	var obj map[string]json.RawMessage
	if err := json.Unmarshal(raw, &obj); err != nil {
		return nil
	}
	return apiExtraRaw(obj)
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
	// gohai sections beyond the three flattened ones. filesystem is a list of
	// mounts and has its own JSON column; network is an object (or null on
	// containers) and rides in gohai_extra, because the envelope's top-level
	// network — the host's addresses — owns the column of that name.
	for _, section := range []string{"filesystem", "network"} {
		if raw, ok := gohai[section]; ok {
			log.Printf("   gohai.%s: %d B", section, len(raw))
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
		return
	}
	size := 0
	for _, s := range res.Processes.Snaps {
		size += len(s)
	}
	log.Printf("[%s] variant=processes(legacy) meta: %s snaps=%d (%d B, kept in raw_payloads)",
		label, apiKV(res.Meta), len(res.Processes.Snaps), size)
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
	var m map[string]json.RawMessage
	if err := json.Unmarshal(body, &m); err != nil {
		log.Printf("[metadata] JSON: %v", err)
		a.storeRaw(c, "metadata", "decode_error", err.Error(), body)
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
			metadataDecodeMap(raw)
		case "host_metadata": // inventoryhost: cpu_*, kernel_*, cloud_provider*, dmi_*
			metadataDecodeMap(raw)
		case "check_metadata": // inventorychecks: check name -> instances
			metadataDecodeChecks(raw)
		case "logs_metadata": // inventorychecks: log source -> instances
			metadataDecodeChecks(raw)
		case "files_metadata": // inventorychecks: config file inventory
			metadataDecodeMap(raw)
		case "system_probe_metadata": // systemprobe
			metadataDecodeMap(raw)
		case "security_agent_metadata": // securityagent
			metadataDecodeMap(raw)
		case "signing_metadata": // packagesigning: signing_keys
			metadataDecodeSigning(raw)
		case "host_system_info_metadata": // hostsysteminfo: manufacturer, model, serial
			metadataDecodeMap(raw)
		case "host_gpu_metadata": // hostgpu: devices
			metadataDecodeGPU(raw)
		case "ha_agent_metadata": // haagent: enabled, state
			metadataDecodeMap(raw)
		case "datadog_cluster_agent_metadata": // clusteragent
			metadataDecodeMap(raw)
		case "clustercheck_metadata": // clusterchecks: check name -> instances
			metadataDecodeChecks(raw)
			// Its two optional siblings ride in envelope_extra, next to the
			// row they belong to, rather than in a column of their own: they
			// are optional, undocumented and only meaningful together with
			// this variant.
		}
	}
	if len(found) == 0 {
		// A twelfth producer, or a variant key renamed between releases.
		// Nothing here can name it, so the bytes are what is kept.
		log.Printf("[metadata] no known variant key, keys: %s", apiKeys(m))
		a.storeRaw(c, "metadata", "no_schema",
			"no known /api/v1/metadata variant key, keys: "+apiKeys(m), body)
		return
	}
	known := append([]string{"hostname", "clustername", "cluster_id", "timestamp", "uuid"}, found...)
	// clusterchecks carries two optional siblings next to its variant key.
	known = append(known, "clustercheck_status", "clustercheck_integration_status")
	apiLogUnknownKeys("metadata", strings.Join(found, "+"), m, known...)

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	// The envelope's timestamp is optional, so it is a POINTER: an absent one
	// must not arrive in the column as 1970, which reads like a real value and
	// sorts before everything.
	var ts *time.Time
	if env.Timestamp > 0 {
		t := time.Unix(env.Timestamp, 0).UTC()
		ts = &t
	}
	// envelope_extra keeps every key that is neither the shared envelope nor a
	// variant this request carried — clustercheck_status among them.
	extra := apiExtraRaw(m, append([]string{"hostname", "clustername", "cluster_id",
		"timestamp", "uuid"}, found...)...)

	now := time.Now().UTC()
	rows := make([]storage.AgentMetadataRow, 0, len(found))
	for _, key := range found {
		// One row per variant key present. A request carrying two of them is
		// two rows, which is what the wire allows and what keeps `variant` a
		// real filter rather than a label on a mixture.
		rows = append(rows, storage.AgentMetadataRow{
			TenantID: tenant, ReceivedAt: now,
			Variant:  key,
			Hostname: env.Hostname, ClusterName: env.Clustername, ClusterID: env.ClusterID,
			Timestamp: ts, UUID: env.UUID,
			Payload:       string(m[key]),
			EnvelopeExtra: extra,
		})
	}
	a.store(storage.AgentMetadataWriter, storage.WriteAgentMetadata{Payloads: rows}, len(rows))
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
// columns, and so now do related_event_id and every undeclared key. alert_type
// and priority keep their coercion — an invented alert type must not cost us
// the event — but what the sender actually wrote is stored beside the coerced
// value: a coercion nobody can see is indistinguishable from data that never
// arrived.
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

	events, err := parseEvents(body)
	if err != nil {
		log.Printf("[events] %v", err)
		a.storeRaw(c, "events", "decode_error", err.Error(), body)
		c.JSON(http.StatusBadRequest, gin.H{"status": "error", "error": err.Error()})
		return
	}
	if len(events) == 0 {
		c.JSON(http.StatusBadRequest, gin.H{"status": "error", "error": "no event with a title"})
		return
	}

	tenant := TenantFromContext(c)
	rows := make([]storage.EventRow, 0, len(events))
	// The originals, for the events the model could not fit.
	rawEvents := jsonElements(body)
	var firstID uint64

	for i, e := range events {
		id := uuid.New()
		num := binary.BigEndian.Uint64(id[:8]) >> 1 // >>1 keeps it inside int64
		if i == 0 {
			firstID = num
		}
		apiLogExtra("events", "event #"+strconv.Itoa(i), e.AdditionalProperties, e.UnparsedObject)
		if e.UnparsedObject != nil {
			// Either nothing decoded at all — a row would then be empty
			// strings wearing an event id — or the event carried an
			// alert_type/priority outside the enum, which the generated
			// UnmarshalJSON reports the same way. Coercing an event whose own
			// words we could not read would put a guess in a column, so both
			// cases keep the bytes and build no row.
			a.storeRaw(c, "events", "unexpected_shape",
				"event #"+strconv.Itoa(i)+" did not fit datadogV1.EventCreateRequest",
				rawElementAt(rawEvents, i, e.UnparsedObject))
			continue
		}

		alertRaw := string(e.GetAlertType())
		priorityRaw := ""
		if p := e.Priority.Get(); p != nil {
			priorityRaw = string(*p)
		}

		rows = append(rows, storage.EventRow{
			TenantID: tenant, Timestamp: wireTime(e.GetDateHappened()),
			EventID: id, EventIDNum: num,
			Title: e.Title, Text: e.Text, Host: e.GetHost(),
			AlertType: alertTypeName(e), Priority: priorityName(e),
			AggregationKey: e.GetAggregationKey(),
			SourceTypeName: e.GetSourceTypeName(),
			DeviceName:     e.GetDeviceName(),
			Tags:           tagsToMultiMap(e.Tags),

			// The agent's event_type has no equivalent on this request, so
			// the column stays empty for the public API's events.
			RelatedEventID: e.RelatedEventId,
			AlertTypeRaw:   alertRaw,
			PriorityRaw:    priorityRaw,
			Extra:          apiExtraAny(e.AdditionalProperties),
		})
	}

	if tenant != "" {
		a.store(storage.EventsWriter, storage.WriteEvents{Events: rows}, len(rows))
	}

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
	return coerceAlertType(string(e.GetAlertType()))
}

func priorityName(e datadogV1.EventCreateRequest) string {
	p := e.Priority.Get()
	if p == nil {
		return "normal"
	}
	return coercePriority(string(*p))
}

// coerceAlertType and coercePriority are the coercion itself, on a bare
// string, so the /intake/ events variant applies the same rule as the public
// API rather than a second one that drifts. What the sender wrote is kept
// separately in alert_type_raw / priority_raw.
func coerceAlertType(raw string) string {
	v := strings.ToLower(raw)
	if !eventAlertTypes[v] {
		return "info"
	}
	return v
}

func coercePriority(raw string) string {
	v := strings.ToLower(raw)
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
// datadogV1.DistributionPointsSeries: host, metric, points, tags -> row,
// undeclared keys -> extra. type has exactly one legal value ("distribution")
// and no column; it is logged only when it is something else.
//
// THE RAW VALUES ARE NOT KEPT, and this is the one intake where the source is
// richer than the row: sketch.Build reduces them to buckets and the originals
// are gone. It is deliberate — a distribution's whole purpose is to be merged
// with the agent's own sketches, which arrive already bucketed, and keeping
// both forms would mean two tables that disagree about the same metric. See
// docs/tables/api.md.
//
// Everything the model could not fit IS kept, whole, in raw_payloads: a body
// that fit no series list, a series that fit no series, and a pair that could
// not be split. Those three used to be logged as "kept in raw_payloads" while
// nothing was stored.
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

	payload, err := parseDistributionPoints(body)
	if err != nil {
		log.Printf("[distribution_points] %v", err)
		a.storeRaw(c, "distribution_points", "decode_error", err.Error(), body)
		return
	}
	apiLogExtra("distribution_points", "payload", payload.AdditionalProperties, payload.UnparsedObject)
	if payload.UnparsedObject != nil {
		// The whole body fit no series list at all, so payload.Series is
		// empty and the loop below will not run: the bytes are the only thing
		// left to keep. Without this the request vanished behind a log line
		// that claimed it had been kept.
		a.storeRaw(c, "distribution_points", "unexpected_shape",
			"payload did not fit datadogV1.DistributionPointsPayload", body)
	}
	if len(payload.AdditionalProperties) > 0 {
		// A key beside `series` at the TOP of the payload belongs to the
		// batch, not to any one point, and the extra map on a sketch row is
		// per series — copying a batch-level value onto every row would
		// multiply it by the point count. The body goes to raw_payloads
		// instead, once. Same reasoning as HandleSeriesV1.
		a.storeRaw(c, "distribution_points", "unexpected_shape",
			"payload carried undeclared top-level keys: "+apiKeys(payload.AdditionalProperties), body)
	}

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	// The originals of the series, for the ones the model could not fit: its
	// UnparsedObject went through float64, these bytes did not.
	rawSeries := jsonFieldElements(body, "series")

	var rows []storage.SketchRow
	badItems, unparsedSeries := 0, 0
	for i, s := range payload.Series {
		apiLogExtra("distribution_points", "series #"+strconv.Itoa(i), s.AdditionalProperties, s.UnparsedObject)
		if s.UnparsedObject != nil {
			// The generated UnmarshalJSON fills UnparsedObject in two
			// unrelated cases: the series fit no known shape (fields empty,
			// nothing below will find a metric in it), or it carried a `type`
			// outside the single legal value (fields populated, only that one
			// word lost). Either way the object it could not fit holds what
			// was sent, so it is kept whole — and when the points did come
			// through they still become rows, because a typo in `type` must
			// not cost a host its distribution.
			unparsedSeries++
			a.storeRaw(c, "distribution_points", "unexpected_shape",
				"series #"+strconv.Itoa(i)+" did not fit datadogV1.DistributionPointsSeries",
				rawElementAt(rawSeries, i, s.UnparsedObject))
			if len(s.Points) == 0 {
				continue
			}
		}
		if t := string(s.GetType()); t != "" && t != string(datadogV1.DISTRIBUTIONPOINTSTYPE_DISTRIBUTION) {
			log.Printf("[distribution_points] series #%d type=%q (only %q is defined)", i, t, datadogV1.DISTRIBUTIONPOINTSTYPE_DISTRIBUTION)
		}
		seriesExtra := apiExtraAny(s.AdditionalProperties)
		rawPoints := jsonFieldElements(rawElementAt(rawSeries, i, nil), "points")
		for j, pair := range s.Points {
			bad := false
			for _, item := range pair {
				if item.UnparsedObject != nil {
					badItems++
					bad = true
				}
			}
			ts, values, ok := distributionPoint(pair)
			if !ok || bad {
				// Either an element fit neither shape of the oneof, or the
				// pair carried no value list at all. Both mean the numbers a
				// client sent cannot be turned into a sketch, and the pair
				// itself is the smallest thing that still holds them.
				a.storeRaw(c, "distribution_points", "unexpected_shape",
					"series #"+strconv.Itoa(i)+" ("+s.Metric+") point #"+strconv.Itoa(j)+
						" is not a [timestamp, [values]] pair", rawElementAt(rawPoints, j, pair))
				continue
			}
			keys, counts, stats := sketch.Build(values)
			rows = append(rows, storage.SketchRow{
				TenantID: tenant, Timestamp: ts,
				Metric: s.Metric, Host: s.GetHost(), Tags: tagsToMultiMap(s.Tags),
				Count: uint64(stats.Count), Min: stats.Min, Max: stats.Max,
				Avg: stats.Avg, Sum: stats.Sum,
				BucketKeys: keys, BucketCounts: counts,
				Extra: seriesExtra,
			})
		}
	}
	if badItems > 0 {
		log.Printf("[distribution_points] %d point elements were neither a timestamp nor a value list, kept in raw_payloads", badItems)
	}
	if unparsedSeries > 0 {
		log.Printf("[distribution_points] %d series did not fit datadogV1.DistributionPointsSeries (UnparsedObject set), kept in raw_payloads", unparsedSeries)
	}
	a.store(storage.SketchesWriter, storage.WriteSketches{Sketches: rows}, len(rows))
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
	tenant := TenantFromContext(c)
	log.Printf("[intake-key] authorization scheme=%q tenant=%s", scheme, tenant)

	// No body on this request; the payload is the header. The proof IS the
	// credential, so what is stored is its SHA-256 and never the proof: enough
	// to tell two delegated identities apart and to key a real mapping on
	// later, useless to anyone who steals the table.
	if tenant != "" {
		key, _ := KeyFromContext(c)
		a.store(storage.DelegatedAuthWriter, storage.WriteDelegatedAuthRequests{
			Requests: []storage.DelegatedAuthRow{{
				TenantID: tenant,
				At:       time.Now().UTC(),
				Scheme:   scheme,
				// Never the proof itself.
				ProofFingerprint: apiProofFingerprint(proof),
				APIKeyID:         key.ID,
			}},
		}, 1)
	}

	// The exchange itself is still an echo: the key that got the request past
	// RequireAPIKey is the key the agent gets back. Mapping a delegated
	// identity to a key of its own is what the rows above make possible.
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

	// A GET with no body and no data of its own: a question, not a payload.
	// Nothing is stored — a read endpoint's traffic is not telemetry, and the
	// answer is what is missing here, not a table.
	_ = from
	_ = to
	_ = query // one or more queries joined by ",", not split (tag sets contain commas)

	// TODO(ninjacat): a query engine. Answer from the metrics table with
	// query_index per series and pointlist timestamps in milliseconds.
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
	res, ok := decodeJSONAPI("symbols/query", body)
	if !ok {
		a.storeRaw(c, "symbols", "decode_error", "not a JSON:API document", body)
		return
	}
	var buildIDs []string
	json.Unmarshal(res.attrs["buildIds"], &buildIDs)
	log.Printf("[symbols/query] arch=%s buildIds(%d): %s", res.str("arch"), len(buildIDs), apiHead(buildIDs))
	res.logUnknown("symbols/query", "buildIds", "arch")

	// The question is worth keeping even though the answer is "none": these
	// build ids are the profiled binaries of the fleet, and this is what a
	// future /api/v2/srcmap handler diffs against. The order of buildIDs is
	// preserved because the uploader's reply is positional against it.
	if tenant := TenantFromContext(c); tenant != "" {
		a.store(storage.SymbolQueriesWriter, storage.WriteSymbolQueries{
			Queries: []storage.SymbolQueryRow{{
				TenantID: tenant,
				At:       time.Now().UTC(),
				Arch:     res.str("arch"),
				BuildIDs: buildIDs,
				Resource: string(body),
			}},
		}, 1)
	}

	// TODO(ninjacat): a symbol store. Answer from it once srcmap uploads are
	// kept.
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
	res, ok := decodeJSONAPI("par/enroll", body)
	if !ok {
		a.storeRaw(c, "par", "decode_error", "enroll: not a JSON:API document", body)
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

	// The runner and its public_key_pem, so the OPMS JWTs it signs with the
	// matching private key can one day be verified. PublicKeyPEM is the whole
	// block, not the fingerprint the log line prints: it is public by
	// construction, and without it nothing can ever check a signature.
	if tenant != "" {
		a.store(storage.RunnerEnrollmentsWriter, storage.WriteRunnerEnrollments{
			Runners: []storage.RunnerEnrollmentRow{{
				TenantID:      tenant,
				RunnerID:      runnerID,
				EnrolledAt:    time.Now().UTC(),
				Name:          res.str("runner_name"),
				Modes:         modes,
				Host:          res.str("runner_host"),
				PublicKeyPEM:  res.str("public_key_pem"),
				AgentHostname: res.str("agent_hostname"),
				OrchClusterID: res.str("orch_cluster_id"),
				AgentFlavor:   res.str("agent_flavor"),
				OrgID:         tenantOrgID(tenant),
				Attributes:    res.attributesJSON(),
			}},
		}, 1)
	}

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
	res, ok := decodeJSONAPI("par/dequeue", body)
	if !ok {
		a.storeRaw(c, "par", "decode_error", "dequeue: not a JSON:API document", body)
		return
	}
	log.Printf("[par/dequeue] started_at=%s last_task=%s version=%s modes=%s",
		res.str("runner_started_at"), res.str("last_task_received_at"),
		c.GetHeader("X-Datadog-OnPrem-Version"), c.GetHeader("X-Datadog-OnPrem-Modes"))
	res.logUnknown("par/dequeue", "runner_started_at", "last_task_received_at")

	// Every dequeue is a runner saying "I am alive, I started at X and last
	// had work at Y" — the only liveness signal a runner with no tasks ever
	// produces. Both timestamps stay STRINGS: their format is undocumented and
	// an unparseable value must not become 1970.
	if tenant := TenantFromContext(c); tenant != "" {
		a.store(storage.RunnerDequeuesWriter, storage.WriteRunnerDequeues{
			Dequeues: []storage.RunnerDequeueRow{{
				TenantID:           tenant,
				At:                 time.Now().UTC(),
				RunnerStartedAt:    res.str("runner_started_at"),
				LastTaskReceivedAt: res.str("last_task_received_at"),
				Version:            c.GetHeader("X-Datadog-OnPrem-Version"),
				Modes:              c.GetHeader("X-Datadog-OnPrem-Modes"),
			}},
		}, 1)
	}

	// TODO(ninjacat): a task queue, and a signing key for its envelopes.
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
	res, ok := decodeJSONAPI("par/task-update", body)
	if !ok {
		a.storeRaw(c, "par", "decode_error", "task-update: not a JSON:API document", body)
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

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	row := storage.RunnerTaskUpdateRow{
		TenantID:  tenant,
		At:        time.Now().UTC(),
		Outcome:   res.id,
		TaskID:    res.str("task_id"),
		ActionFQN: res.str("action_fqn"),
		JobID:     res.str("job_id"),
		Client:    string(res.attrs["client"]),

		Branch: p.str("branch"),
		// The action's own result, in full. It used to be logged as a byte
		// count, which is the one number about it nobody needs.
		Outputs:      string(payload["outputs"]),
		ErrorDetails: p.str("error_details"),
		APIError:     p.str("api_error"),

		// Undeclared attributes from both levels, the nested ones prefixed so
		// two keys of the same name stay apart.
		Extra: mergeExtra(
			apiExtraRaw(res.attrs, "task_id", "client", "action_fqn", "job_id", "payload"),
			"payload.", apiExtraRaw(payload, "branch", "outputs", "error_code", "error_details", "api_error"),
		),
	}
	// error_code is a numeric enum on the runner's side, and a succeeded task
	// sends none — so a pointer, and never a 0 that reads as an error code.
	var code int64
	if len(payload["error_code"]) > 0 && json.Unmarshal(payload["error_code"], &code) == nil {
		row.ErrorCode = &code
	}
	a.store(storage.RunnerTaskUpdatesWriter,
		storage.WriteRunnerTaskUpdates{Updates: []storage.RunnerTaskUpdateRow{row}}, 1)
}

// mergeExtra folds a second extra map into the first under a prefix, so two
// levels of one document can share a column without colliding.
func mergeExtra(base map[string]string, prefix string, nested map[string]string) map[string]string {
	if len(nested) == 0 {
		return base
	}
	if base == nil {
		base = make(map[string]string, len(nested))
	}
	for k, v := range nested {
		base[prefix+k] = v
	}
	return base
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
	res, ok := decodeJSONAPI("par/heartbeat", body)
	if !ok {
		a.storeRaw(c, "par", "decode_error", "heartbeat: not a JSON:API document", body)
		return
	}
	var client map[string]any
	json.Unmarshal(res.attrs["client"], &client)
	log.Printf("[par/heartbeat] task=%s job=%s action=%s client: %s",
		res.str("task_id"), res.str("job_id"), res.str("action_fqn"), apiKV(client))
	res.logUnknown("par/heartbeat", "task_id", "client", "action_fqn", "job_id")

	// Individually worth little, and exactly what answers "when did this job
	// stop making progress" — which is unanswerable from the task update,
	// because a job that hangs never sends one.
	if tenant := TenantFromContext(c); tenant != "" {
		a.store(storage.RunnerHeartbeatsWriter, storage.WriteRunnerHeartbeats{
			Heartbeats: []storage.RunnerHeartbeatRow{{
				TenantID:  tenant,
				At:        time.Now().UTC(),
				TaskID:    res.str("task_id"),
				ActionFQN: res.str("action_fqn"),
				JobID:     res.str("job_id"),
				Client:    string(res.attrs["client"]),
			}},
		}, 1)
	}
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
	res, ok := decodeJSONAPI("par/connections", body)
	if !ok {
		a.storeRaw(c, "par", "decode_error", "connections: not a JSON:API document", body)
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

	// SENSITIVE: credentials are stored verbatim, because a connection without
	// them cannot be used and a table that dropped them would be a lie. The
	// logging path above prints only their KEY NAMES and must keep doing so —
	// see the note on action_connections.credentials in the migration.
	if tenant := TenantFromContext(c); tenant != "" {
		a.store(storage.ActionConnectionsWriter, storage.WriteActionConnections{
			Connections: []storage.ActionConnectionRow{{
				TenantID:        tenant,
				At:              time.Now().UTC(),
				Name:            res.str("name"),
				RunnerID:        res.str("runner_id"),
				Tags:            tagsToMultiMap(tags),
				IntegrationType: integrationType,
				Credentials:     string(integration["credentials"]),
				Extra: mergeExtra(
					apiExtraRaw(res.attrs, "name", "runner_id", "tags", "integration"),
					"integration.", apiExtraRaw(integration, "type", "credentials"),
				),
			}},
		}, 1)
	}
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

// attributesJSON re-renders the attributes object for a native JSON column, so
// an attribute nobody has named yet is queryable without a migration. "{}" when
// there were none: a JSON column rejects an empty string.
func (r jsonAPIResource) attributesJSON() string {
	if len(r.attrs) == 0 {
		return "{}"
	}
	return string(apiJSONBytes(r.attrs))
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
		log.Printf("[%s] %s undeclared keys (stored in the row's extra map): %s", label, what, apiKV(extra))
	}
	if unparsed != nil {
		log.Printf("[%s] %s did not fit the model, kept in raw_payloads, keys: %s", label, what, apiKeys(unparsed))
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

// ---------------------------------------------------------------------------
// Storage helpers: decoded values on their way to a column
// ---------------------------------------------------------------------------

// apiJSONBytes renders an already-decoded value back to JSON for storeRaw.
//
// Re-encoding loses whitespace and key order, and for a value decoded with
// UseNumber — AdditionalProperties, which is what apiExtraAny feeds here —
// nothing else: a json.Number keeps its digits, so a 64-bit id survives the
// trip.
//
// UnparsedObject is NOT such a value. The generated UnmarshalJSON decodes it
// with plain encoding/json, so an integer above 2^53 inside an element that
// did not fit the model has already been rounded to float64 before this
// function sees it, and re-encoding would write the rounded number. That is
// why every batch handler pairs this with rawElementAt and keeps the bytes
// that arrived instead; this is the fallback for when they cannot be found.
//
// A value that cannot be encoded at all falls back to its Go rendering,
// because an approximate raw row beats an empty one.
func apiJSONBytes(v any) []byte {
	b, err := json.Marshal(v)
	if err != nil {
		return []byte(fmt.Sprint(v))
	}
	return b
}

// rawElementAt returns the bytes element i of a batch arrived as, falling back
// to a re-encoding of what the model kept when the body could not be re-split
// (a framing jsonElements does not recognise, or an index it never produced).
func rawElementAt(elements []json.RawMessage, i int, decoded any) []byte {
	if i >= 0 && i < len(elements) {
		return elements[i]
	}
	return apiJSONBytes(decoded)
}

// apiExtraAny JSON-encodes the values of every key not named in skip, for an
// `extra Map(String, String)` column.
//
// The VALUE is JSON, not the plain text of it: one column then takes a string,
// a number, an object and an array without needing a type per key, and a
// string that looks like a number stays readable as a string. nil when nothing
// is left, so orEmpty on the row does the rest.
func apiExtraAny(m map[string]any, skip ...string) map[string]string {
	var out map[string]string
	for k, v := range m {
		if slices.Contains(skip, k) {
			continue
		}
		if out == nil {
			out = make(map[string]string, len(m))
		}
		out[k] = string(apiJSONBytes(v))
	}
	return out
}

// apiExtraRaw is apiExtraAny for an envelope decoded one level down: the
// values are already raw JSON, so they are kept byte for byte.
func apiExtraRaw(m map[string]json.RawMessage, skip ...string) map[string]string {
	var out map[string]string
	for k, v := range m {
		if slices.Contains(skip, k) {
			continue
		}
		if out == nil {
			out = make(map[string]string, len(m))
		}
		out[k] = string(v)
	}
	return out
}

// apiRawBool decodes an optional JSON bool into the pointer a Nullable(UInt8)
// column takes. Absent stays absent: "FIPS is off" and "this agent does not
// know about FIPS" are different statements.
func apiRawBool(raw json.RawMessage) *uint8 {
	var b bool
	if len(raw) == 0 || json.Unmarshal(raw, &b) != nil {
		return nil
	}
	v := uint8(0)
	if b {
		v = 1
	}
	return &v
}

// apiProofFingerprint is the SHA-256 of a delegated-auth proof, full hex.
//
// Full, unlike apiFingerprint's log rendering: this one is stored and later
// compared against, and half a hash is not a key. The proof itself never
// leaves this function.
func apiProofFingerprint(proof string) string {
	if proof == "" {
		return ""
	}
	sum := sha256.Sum256([]byte(proof))
	return hex.EncodeToString(sum[:])
}
