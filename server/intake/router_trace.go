package intake

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"strconv"
	"strings"
	"time"

	pb "github.com/DataDog/datadog-agent/pkg/proto/pbgo/trace"
	"github.com/DataDog/sketches-go/ddsketch"
	"github.com/DataDog/sketches-go/ddsketch/pb/sketchpb"
	"github.com/gin-gonic/gin"
	"google.golang.org/protobuf/proto"
)

// trace.agent.<site> — the trace-agent.
//
//	config: apm_config.apm_dd_url
//
// A separate binary from the node agent, which is why dd_url does not move it.
// None of the handlers store anything yet — see docs/traces.md.
//
// Not handled: /api/v2/apmtelemetry

func (a *Server) routeTrace(g *gin.RouterGroup) {
	g.POST("/api/v0.2/traces", a.HandleTraces)
	g.POST("/api/v0.2/stats", a.HandleAPMStats)
	g.POST("/api/v0.1/pipeline_stats", a.HandlePipelineStats)
	g.POST("/api/v2/data_streams_messages", a.HandleDataStreamsMessages)
}

// Log limits. A payload can carry thousands of spans; the log shows enough to
// see the shape and the first few of everything, then counts the rest.
const (
	traceLogChunks = 20 // chunks per tracer payload
	traceLogSpans  = 10 // spans per chunk
	traceLogTags   = 12 // entries from any tag map before "+N more"
	statsLogGroups = 10 // grouped stats per bucket
)

// HandleTraces accepts POST /api/v0.2/traces.
//
// A protobuf AgentPayload, compressed with gzip or zstd depending on the
// agent's compressor; the Decompress middleware has already undone that.
//
// What arrives is a SAMPLE — the agent runs its samplers before this writer.
// The complete counts live in the stats endpoint below.
//
// The response body is parsed by the agent (rate_by_service feeds its
// priority sampler), so it stays exactly this shape.
func (a *Server) HandleTraces(c *gin.Context) {
	defer c.JSON(http.StatusOK, gin.H{"rate_by_service": gin.H{}})
	if isDiagnose(c) {
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[traces] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	var payload pb.AgentPayload
	if err := payload.UnmarshalVT(body); err != nil {
		log.Printf("[traces] protobuf AgentPayload: %v (%d bytes)", err, len(body))
		return
	}

	traceLogPayload(&payload)
	for _, tp := range payload.GetTracerPayloads() {
		traceLogTracerPayload(tp)
	}

	// The log above stops at traceLogChunks / traceLogSpans / traceLogTags;
	// the payload does not. UnmarshalVT decoded every tracer payload, chunk
	// and span before the first log line, and the limits only decide what is
	// printed. Every span keeps its full Meta, Metrics and MetaStruct maps,
	// SpanLinks and SpanEvents, exactly as the agent sent them.
	//
	// Two shapes can arrive in one envelope. TracerPayloads is the v0.4
	// format: strings inline, readable through the generated getters.
	// IdxTracerPayloads is the v1.0 format (pbgo/trace/idx): every string is
	// an index into a per-payload table, so a field is not readable until
	// resolved against it. Both are set aside; storage must expect either.
	tracerPayloads := payload.GetTracerPayloads()
	idxTracerPayloads := payload.GetIdxTracerPayloads()
	_ = &payload          // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = tracerPayloads    // v0.4 — []*pb.TracerPayload, strings inline
	_ = idxTracerPayloads // v1.0 — []*idx.TracerPayload, strings via table
}

// traceLogPayload reports the agent-level envelope: which agent, and the
// sampler settings it was running with when it chose these traces.
func traceLogPayload(p *pb.AgentPayload) {
	log.Printf("[traces] host=%s env=%s agent=%s target_tps=%g error_tps=%g rare_sampler=%t | %d tracer payloads, %d B",
		p.GetHostName(), p.GetEnv(), p.GetAgentVersion(), p.GetTargetTPS(), p.GetErrorTPS(),
		p.GetRareSamplerEnabled(), len(p.GetTracerPayloads()), proto.Size(p))
	if tags := p.GetTags(); len(tags) > 0 {
		log.Printf("   agent tags %s", traceTags(tags))
	}
	// IdxTracerPayloads is the v1.0 string-table format (pbgo/trace/idx):
	// every string is an index into a per-payload table, so a field is not
	// readable without resolving it. Nothing on this host has sent it yet;
	// counted so its arrival is visible, expanded in the log once a real one
	// is captured. Decoded in full regardless — HandleTraces sets it aside.
	if n := len(p.GetIdxTracerPayloads()); n > 0 {
		log.Printf("   %d idx (v1.0) tracer payloads — not expanded in the log, see comment", n)
	}
}

// traceLogTracerPayload reports one tracer's identity — the process that
// produced the traces — then its chunks.
func traceLogTracerPayload(tp *pb.TracerPayload) {
	log.Printf("[traces] tracer %s/%s v%s runtime=%s container=%s host=%s env=%s app=%s | %d chunks",
		tp.GetLanguageName(), tp.GetLanguageVersion(), tp.GetTracerVersion(),
		traceOrDash(tp.GetRuntimeID()), traceOrDash(tp.GetContainerID()), traceOrDash(tp.GetHostname()),
		traceOrDash(tp.GetEnv()), traceOrDash(tp.GetAppVersion()), len(tp.GetChunks()))
	if tags := tp.GetTags(); len(tags) > 0 {
		log.Printf("   tracer tags %s", traceTags(tags))
	}
	if cd := tp.GetContainerDebug(); cd != nil {
		// Only set when the agent had trouble resolving the container: how
		// long the lookup took, whether the payload waited for it, and why
		// it left the buffer.
		log.Printf("   container debug error=%q latency=%dms buffered=%t buffer=%dms eviction=%q",
			cd.GetError(), cd.GetLatencyMs(), cd.GetWasBuffered(), cd.GetBufferMs(),
			cd.GetBufferEvictionReason())
	}

	for i, ch := range tp.GetChunks() {
		if i == traceLogChunks {
			log.Printf("   ... and %d more chunks", len(tp.GetChunks())-traceLogChunks)
			break
		}
		traceLogChunk(ch)
	}
}

// traceLogChunk reports one trace chunk: the sampling decision that let it
// through, and its spans.
func traceLogChunk(ch *pb.TraceChunk) {
	log.Printf("   chunk priority=%d(%s) origin=%s dropped=%t | %d spans",
		ch.GetPriority(), tracePriority(ch.GetPriority()), traceOrDash(ch.GetOrigin()),
		ch.GetDroppedTrace(), len(ch.GetSpans()))
	if tags := ch.GetTags(); len(tags) > 0 {
		log.Printf("      chunk tags %s", traceTags(tags))
	}
	for i, sp := range ch.GetSpans() {
		if i == traceLogSpans {
			log.Printf("      ... and %d more spans", len(ch.GetSpans())-traceLogSpans)
			break
		}
		traceLogSpan(sp)
	}
}

// Meta/Metrics keys worth a place in the log. Everything else is counted.
var (
	traceMetaKeys = []string{
		"span.kind", "http.method", "http.status_code", "http.url", "error.type",
		"error.message", "_dd.p.tid", "_dd.origin", "language", "runtime-id", "version",
	}
	traceMetricKeys = []string{
		"_sampling_priority_v1", "_dd.top_level", "_dd.measured", "_dd1.sr.eausr",
		"_dd.rule_psr", "_dd.agent_psr", "http.status_code",
	}
)

// traceLogSpan reports one span. IDs are uint64 and printed in decimal —
// never through a float conversion, which would lose the low bits. The
// 128-bit trace id's upper half, if any, arrives in Meta as _dd.p.tid.
func traceLogSpan(sp *pb.Span) {
	errFlag := ""
	if sp.GetError() != 0 {
		errFlag = " ERROR"
	}
	log.Printf("      span %s %s %q type=%s | trace=%s span=%s parent=%s | start=%s dur=%s%s",
		sp.GetService(), sp.GetName(), sp.GetResource(), traceOrDash(sp.GetType()),
		strconv.FormatUint(sp.GetTraceID(), 10), strconv.FormatUint(sp.GetSpanID(), 10),
		traceParent(sp.GetParentID()),
		time.Unix(0, sp.GetStart()).UTC().Format(time.RFC3339Nano),
		time.Duration(sp.GetDuration()), errFlag)

	// Meta and Metrics are the bulk of a span (tens of keys each). The log
	// shows the keys that identify the call and the failure; the full maps
	// go to the tables.
	if meta := sp.GetMeta(); len(meta) > 0 {
		log.Printf("         meta %d keys: %s", len(meta), tracePick(meta, traceMetaKeys, "%s"))
	}
	if metrics := sp.GetMetrics(); len(metrics) > 0 {
		log.Printf("         metrics %d keys: %s", len(metrics), tracePick(metrics, traceMetricKeys, "%g"))
	}
	// MetaStruct values are msgpack blobs (AppSec events, process tags) with
	// no published Go type per key — keys and sizes only.
	if ms := sp.GetMetaStruct(); len(ms) > 0 {
		parts := make([]string, 0, len(ms))
		for _, k := range sortedKeys(ms) {
			parts = append(parts, fmt.Sprintf("%s=%dB", k, len(ms[k])))
		}
		log.Printf("         meta_struct %s", strings.Join(parts, " "))
	}
	// Links and events are the OpenTelemetry-shaped extras. Counts plus the
	// linked trace ids are what the log needs; their attribute maps (typed
	// AnyValue for events) belong in the tables.
	if links := sp.GetSpanLinks(); len(links) > 0 {
		ids := make([]string, 0, len(links))
		for _, l := range links {
			ids = append(ids, strconv.FormatUint(l.GetTraceID(), 10)+"/"+strconv.FormatUint(l.GetSpanID(), 10))
		}
		log.Printf("         %d links -> %s", len(links), strings.Join(ids, " "))
	}
	if events := sp.GetSpanEvents(); len(events) > 0 {
		names := make([]string, 0, len(events))
		for _, e := range events {
			names = append(names, e.GetName())
		}
		log.Printf("         %d events: %s", len(events), strings.Join(names, ","))
	}
}

// tracePriority names a sampling priority (pkg/trace/sampler).
func tracePriority(p int32) string {
	switch p {
	case -1:
		return "user_drop"
	case 0:
		return "auto_drop"
	case 1:
		return "auto_keep"
	case 2:
		return "user_keep"
	}
	return "?"
}

func traceParent(id uint64) string {
	if id == 0 {
		return "root"
	}
	return strconv.FormatUint(id, 10)
}

// tracePick renders the keys of interest that are present in m, in the given
// order, then the count of what it did not show.
func tracePick[V any](m map[string]V, keys []string, format string) string {
	parts := make([]string, 0, len(keys)+1)
	for _, k := range keys {
		if v, ok := m[k]; ok {
			parts = append(parts, k+"="+fmt.Sprintf(format, v))
		}
	}
	if rest := len(m) - len(parts); rest > 0 {
		parts = append(parts, fmt.Sprintf("(+%d other)", rest))
	}
	return strings.Join(parts, " ")
}

// traceTags renders a tag map sorted, capped at traceLogTags entries.
func traceTags(m map[string]string) string {
	keys := sortedKeys(m)
	parts := make([]string, 0, len(keys)+1)
	for i, k := range keys {
		if i == traceLogTags {
			parts = append(parts, fmt.Sprintf("(+%d more)", len(keys)-traceLogTags))
			break
		}
		parts = append(parts, k+":"+m[k])
	}
	return strings.Join(parts, ",")
}

// traceOrDash keeps empty optional strings visible in the log.
func traceOrDash(s string) string {
	if s == "" {
		return "-"
	}
	return s
}

// HandleAPMStats accepts POST /api/v0.2/stats.
//
// msgpack, not protobuf — the only place in Datadog's protocol where that
// encoding appears. The generated type carries its own decoder.
//
// What arrives is NOT a sample: the agent feeds every trace to its
// concentrator before the sampler runs, so these counts are complete even when
// 99% of the spans were thrown away. That is why this endpoint exists apart
// from the traces one.
func (a *Server) HandleAPMStats(c *gin.Context) {
	defer c.JSON(http.StatusOK, gin.H{})
	if isDiagnose(c) {
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[apm-stats] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	var payload pb.StatsPayload
	if _, err := payload.UnmarshalMsg(body); err != nil {
		log.Printf("[apm-stats] msgpack StatsPayload: %v (%d bytes)", err, len(body))
		return
	}

	// Decode every latency sketch first, for every group in every bucket of
	// every client. The log below stops at statsLogGroups; this loop does not,
	// so a group past the limit still has its sketches decoded. Keyed by the
	// group pointer: one entry per summary that decoded, nothing for an empty
	// or undecodable one — the raw bytes stay in the group either way.
	okSketches := make(map[*pb.ClientGroupedStats]*ddsketch.DDSketch)
	errSketches := make(map[*pb.ClientGroupedStats]*ddsketch.DDSketch)
	for _, cs := range payload.GetStats() {
		for _, b := range cs.GetStats() {
			for _, g := range b.GetStats() {
				if sk, err := statsDecodeSketch(g.GetOkSummary()); err != nil {
					log.Printf("[apm-stats] %s %s ok_summary %d B: %v", g.GetService(), g.GetName(), len(g.GetOkSummary()), err)
				} else if sk != nil {
					okSketches[g] = sk
				}
				if sk, err := statsDecodeSketch(g.GetErrorSummary()); err != nil {
					log.Printf("[apm-stats] %s %s error_summary %d B: %v", g.GetService(), g.GetName(), len(g.GetErrorSummary()), err)
				} else if sk != nil {
					errSketches[g] = sk
				}
			}
		}
	}

	log.Printf("[apm-stats] agent=%s env=%s version=%s client_computed=%t split=%t | %d clients, %d B",
		payload.GetAgentHostname(), payload.GetAgentEnv(), payload.GetAgentVersion(),
		payload.GetClientComputed(), payload.GetSplitPayload(), len(payload.GetStats()), len(body))
	for _, cs := range payload.GetStats() {
		statsLogClient(cs, okSketches, errSketches)
	}

	// The whole StatsPayload as UnmarshalMsg produced it: every client, every
	// bucket, every group with all 23 fields, OkSummary and ErrorSummary as
	// the raw protobuf bytes the concentrator shipped. Beside it, the sketches
	// as sketches: the quantiles in the log are a projection computed for
	// reading, not the data. A *ddsketch.DDSketch keeps the full bin store
	// and index mapping, so it merges with others and answers any quantile
	// later; store that, or the raw bytes, never p50/p99 alone.
	_ = &payload    // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = okSketches  // group -> decoded OkSummary, every group of every bucket
	_ = errSketches // group -> decoded ErrorSummary, every group of every bucket
}

// statsDecodeSketch decodes one latency summary. Returns (nil, nil) for an
// absent summary, (nil, err) for bytes that are not a DDSketch, and the sketch
// otherwise — including an empty one, which is still a valid sketch.
//
// The concentrator builds these with sketches-go (pkg/trace/stats/statsraw.go:
// LogCollapsingLowestDenseDDSketch at relativeAccuracy 0.01, 2048 bins) and
// ships proto.Marshal(sketch.ToProto()), so sketches-go is the right decoder
// HERE — its only runtime dependency is google.golang.org/protobuf, already in
// go.mod. This is the opposite of the metric sketches: those follow the
// agent's own pkg/util/quantile (see docs/ddsketch-agenta.md and package
// sketch) and must never go through this library. Values are span durations
// in nanoseconds.
func statsDecodeSketch(raw []byte) (*ddsketch.DDSketch, error) {
	if len(raw) == 0 {
		return nil, nil
	}
	var spb sketchpb.DDSketch
	if err := proto.Unmarshal(raw, &spb); err != nil {
		return nil, fmt.Errorf("undecodable: %w", err)
	}
	sk, err := ddsketch.FromProto(&spb)
	if err != nil {
		return nil, fmt.Errorf("not a DDSketch: %w", err)
	}
	return sk, nil
}

// statsLogClient reports one ClientStatsPayload: the tracer (or, when the
// agent aggregated, the aggregation key) these buckets belong to. The sketch
// maps come pre-decoded from the handler; this function only reads them.
func statsLogClient(cs *pb.ClientStatsPayload, okSketches, errSketches map[*pb.ClientGroupedStats]*ddsketch.DDSketch) {
	log.Printf("[apm-stats] client host=%s env=%s version=%s service=%s lang=%s tracer=%s runtime=%s seq=%d container=%s aggregation=%s | %d buckets",
		traceOrDash(cs.GetHostname()), traceOrDash(cs.GetEnv()), traceOrDash(cs.GetVersion()), traceOrDash(cs.GetService()),
		traceOrDash(cs.GetLang()), traceOrDash(cs.GetTracerVersion()), traceOrDash(cs.GetRuntimeID()), cs.GetSequence(),
		traceOrDash(cs.GetContainerID()), traceOrDash(cs.GetAgentAggregation()), len(cs.GetStats()))
	// Source-control and process identity are recent additions; empty on
	// most tracers, so only printed when set.
	if cs.GetGitCommitSha() != "" || cs.GetImageTag() != "" || cs.GetProcessTags() != "" || cs.GetProcessTagsHash() != 0 {
		log.Printf("   git=%s image=%s process_tags=%q process_tags_hash=%d",
			traceOrDash(cs.GetGitCommitSha()), traceOrDash(cs.GetImageTag()), cs.GetProcessTags(), cs.GetProcessTagsHash())
	}
	if tags := cs.GetTags(); len(tags) > 0 {
		log.Printf("   tags %s", strings.Join(tags, ","))
	}

	for _, b := range cs.GetStats() {
		log.Printf("   bucket start=%s duration=%s time_shift=%s | %d groups",
			time.Unix(0, int64(b.GetStart())).UTC().Format(time.RFC3339),
			time.Duration(b.GetDuration()), time.Duration(b.GetAgentTimeShift()), len(b.GetStats()))
		for i, g := range b.GetStats() {
			if i == statsLogGroups {
				log.Printf("      ... and %d more groups", len(b.GetStats())-statsLogGroups)
				break
			}
			statsLogGroup(g, okSketches[g], errSketches[g])
		}
	}
}

// statsLogGroup reports one aggregation row: the key it was grouped by, the
// counts, and what the two latency sketches hold. ok and errSk are the
// decoded summaries, nil when absent or undecodable.
func statsLogGroup(g *pb.ClientGroupedStats, ok, errSk *ddsketch.DDSketch) {
	log.Printf("      %s %s %q type=%s kind=%s http=%s %d %s grpc=%s db=%s root=%s synthetics=%t src=%s",
		g.GetService(), g.GetName(), g.GetResource(), traceOrDash(g.GetType()), traceOrDash(g.GetSpanKind()),
		traceOrDash(g.GetHTTPMethod()), g.GetHTTPStatusCode(), traceOrDash(g.GetHTTPEndpoint()),
		traceOrDash(g.GetGRPCStatusCode()), traceOrDash(g.GetDBType()), statsTrilean(g.GetIsTraceRoot()),
		g.GetSynthetics(), traceOrDash(g.GetServiceSource()))
	log.Printf("         hits=%d errors=%d top_level=%d total=%s | ok %s | err %s",
		g.GetHits(), g.GetErrors(), g.GetTopLevelHits(), time.Duration(g.GetDuration()),
		statsSketch(g.GetOkSummary(), ok), statsSketch(g.GetErrorSummary(), errSk))
	// The three tag lists drive extra aggregation dimensions (peer service,
	// primary tags, metric tags) and are often empty.
	if len(g.GetPeerTags())+len(g.GetSpanDerivedPrimaryTags())+len(g.GetAdditionalMetricTags()) > 0 {
		log.Printf("         peer_tags=%s primary_tags=%s metric_tags=%s",
			rcJoin(g.GetPeerTags()), rcJoin(g.GetSpanDerivedPrimaryTags()), rcJoin(g.GetAdditionalMetricTags()))
	}
}

// statsSketch renders a latency summary for the log: its size, count and a few
// quantiles. raw is the wire form, sk the decoded sketch from
// statsDecodeSketch (nil when raw is empty or did not decode — the decode
// error was logged by the handler).
func statsSketch(raw []byte, sk *ddsketch.DDSketch) string {
	if len(raw) == 0 {
		return "none"
	}
	if sk == nil {
		return fmt.Sprintf("%dB undecodable", len(raw))
	}
	if sk.IsEmpty() {
		return fmt.Sprintf("%dB empty", len(raw))
	}
	p50, _ := sk.GetValueAtQuantile(0.5)
	p99, _ := sk.GetValueAtQuantile(0.99)
	maxV, _ := sk.GetMaxValue()
	return fmt.Sprintf("%dB n=%g p50=%s p99=%s max=%s acc=%.3g",
		len(raw), sk.GetCount(), time.Duration(p50), time.Duration(p99), time.Duration(maxV),
		sk.IndexMapping.RelativeAccuracy())
}

func statsTrilean(t pb.Trilean) string {
	switch t {
	case pb.Trilean_TRUE:
		return "true"
	case pb.Trilean_FALSE:
		return "false"
	}
	return "unset"
}

// HandlePipelineStats accepts POST /api/v0.1/pipeline_stats.
//
// Data Streams Monitoring stats straight from the tracer. The trace-agent is a
// pure reverse proxy here (pkg/trace/api/pipeline_stats.go): it swaps the
// local /v0.1/pipeline_stats path for this one, adds Via,
// X-Datadog-Additional-Tags and X-Datadog-Container-Tags, and forwards the
// body untouched. No Go type for it exists in datadog-agent — the encoding is
// whatever the tracer chose, and on this host that tends to be msgpack rather
// than protobuf (docs §5.4). Logged, not decoded, until real tracer payloads
// have been captured.
func (a *Server) HandlePipelineStats(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	if isDiagnose(c) {
		return
	}

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[pipeline-stats] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	// Decompress() replaces the body but leaves the header alone, so this is
	// the encoding the tracer put on the wire.
	log.Printf("[pipeline-stats] encoding=%q via=%q additional_tags=%q container_tags=%q",
		c.GetHeader("Content-Encoding"), c.GetHeader("Via"),
		c.GetHeader("X-Datadog-Additional-Tags"), c.GetHeader("X-Datadog-Container-Tags"))
	describe("pipeline-stats", c.GetHeader("Content-Type"), body)

	// No decoder: the shape is unknown (docs/zadania/dsm-nieznane-payloady.md)
	// and guessing one would be worse than none. The raw body, already
	// decompressed by the middleware, is the thing to keep — byte for byte
	// what the tracer sent, so the decoder can be chosen once a real capture
	// has been read.
	_ = body // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// HandleDataStreamsMessages accepts POST /api/v2/data_streams_messages.
//
// Not the trace-agent at all — an Event Platform track that happens to live on
// the trace.agent host (comp/forwarder/eventplatform/impl/
// pipelines_datastreams.go, event type "data-streams-message"). JSON, so the
// forwarder batches: the body is one array of raw messages (docs §0.4).
func (a *Server) HandleDataStreamsMessages(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[data-streams] cannot read body: %v", err)
		return
	}
	// The raw payload outlives every path out of this handler: a decoder
	// that fails or does not exist yet must not make the bytes disappear.
	defer func() { _ = body }()

	var msgs []json.RawMessage
	if err := json.Unmarshal(body, &msgs); err != nil {
		log.Printf("[data-streams] JSON array: %v (%d bytes)", err, len(body))
		describe("data-streams", c.GetHeader("Content-Type"), body)
		// Not the array the forwarder is expected to send. Nothing is lost:
		// the body survives as it came, for a reader that knows the shape.
		_ = body // TODO(ninjacat): tables. Complete, unconverted, ready to take.
		return
	}

	log.Printf("[data-streams] %d messages, %d B", len(msgs), len(body))

	// Each message is one element of the array, untouched: the producer is
	// not in the open agent repository (docs/zadania/dsm-nieznane-payloady.md),
	// so there is no struct to decode into and none is invented here. The
	// raw body goes alongside — the same bytes, still one document.
	_ = msgs // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	_ = body // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}
