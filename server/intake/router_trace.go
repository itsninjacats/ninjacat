package intake

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"log"
	"math"
	"net/http"
	"sort"
	"strconv"
	"strings"
	"time"

	pb "github.com/DataDog/datadog-agent/pkg/proto/pbgo/trace"
	"github.com/DataDog/datadog-agent/pkg/proto/pbgo/trace/idx"
	"github.com/DataDog/sketches-go/ddsketch"
	"github.com/DataDog/sketches-go/ddsketch/pb/sketchpb"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/tinylib/msgp/msgp"
	"google.golang.org/protobuf/proto"
)

// trace.agent.<site> — the trace-agent.
//
//	config: apm_config.apm_dd_url
//
// A separate binary from the node agent, which is why dd_url does not move it.
//
// What is stored, and where (schema/migrations/0003_traces.sql):
//
//	/api/v0.2/traces               -> spans                    one row per span
//	/api/v0.2/stats                -> apm_stats                one row per grouped stat
//	/api/v0.1/pipeline_stats       -> dsm_pipeline_stats       one row per StatsPoint
//	                                  dsm_backlogs             one row per Backlog
//	                                  dsm_bucket_transactions  one row per bucket blob pair
//	/api/v2/data_streams_messages  -> dsm_messages             one row per array element
//
// A body that does not decode goes to raw_payloads through storeRaw
// (intake/raw.go) under the intake label "trace" — never to a log line alone.
//
// Not handled: /api/v2/apmtelemetry

func (a *Server) routeTrace(g *gin.RouterGroup) {
	g.POST("/api/v0.2/traces", a.HandleTraces)
	g.POST("/api/v0.2/stats", a.HandleAPMStats)
	g.POST("/api/v0.1/pipeline_stats", a.HandlePipelineStats)
	g.POST("/api/v2/data_streams_messages", a.HandleDataStreamsMessages)
}

// Log limits. A payload can carry thousands of spans; the log shows enough to
// see the shape and the first few of everything, then counts the rest. The
// tables get everything regardless — these decide what is PRINTED, never what
// is stored.
const (
	traceLogChunks = 20 // chunks per tracer payload
	traceLogSpans  = 10 // spans per chunk
	traceLogTags   = 12 // entries from any tag map before "+N more"
	statsLogGroups = 10 // grouped stats per bucket
)

// traceIntake is the label raw_payloads records for everything on this host.
const traceIntake = "trace"

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

	var payload pb.AgentPayload
	if err := payload.UnmarshalVT(body); err != nil {
		log.Printf("[traces] protobuf AgentPayload: %v (%d bytes)", err, len(body))
		// The bytes outlive the failed decode: a protobuf that does not parse
		// is either a wire version we have not seen or a bug, and both need
		// the payload to answer.
		a.storeRaw(c, traceIntake, "decode_error",
			fmt.Sprintf("protobuf AgentPayload: %v", err), body)
		return
	}

	if len(payload.GetTracerPayloads()) == 0 && len(payload.GetIdxTracerPayloads()) == 0 {
		// proto.Unmarshal fails OPEN: unknown fields are skipped, so a body
		// meant for another endpoint — or any other protobuf message, since
		// the wire carries no type marker — decodes "successfully" into an
		// empty AgentPayload. Neither wire shape being populated is the one
		// signal we get, the same guard as HandleContainerLifecycle in
		// router_containers.go, and the bytes are exactly what a future
		// decoder needs.
		log.Printf("[traces] protobuf decoded to zero tracer payloads (%d bytes) — wrong payload type?", len(body))
		a.storeRaw(c, traceIntake, "unexpected_shape",
			"AgentPayload decoded with no TracerPayloads and no IdxTracerPayloads", body)
		return
	}

	traceLogPayload(&payload)
	for _, tp := range payload.GetTracerPayloads() {
		traceLogTracerPayload(tp)
	}

	// Two shapes can arrive in one envelope. TracerPayloads is the v0.4
	// format: strings inline, readable through the generated getters.
	// IdxTracerPayloads is the v1.0 format (pbgo/trace/idx): every string is
	// an index into a per-payload table, so a field is not readable until
	// resolved against it. Both become rows of the same table, told apart by
	// the wire_format column.
	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := traceSpanRows(tenant, time.Now().UTC(), &payload)
	a.store(storage.SpansWriter, storage.WriteSpans{Spans: rows}, len(rows))
}

// ---------------------------------------------------------------------------
// /api/v0.2/traces -> spans
// ---------------------------------------------------------------------------

// traceSpanRows flattens a whole AgentPayload into span rows, both wire
// shapes, in wire order.
//
// The three levels above a span (agent, tracer, chunk) are copied onto every
// row rather than normalized away. The alternative is two joins to answer
// "which agent version shipped this span", which is the question asked when a
// trace looks wrong, and the repeated values are LowCardinality in ClickHouse,
// so the copies cost dictionary ids.
func traceSpanRows(tenant string, receivedAt time.Time, p *pb.AgentPayload) []storage.SpanRow {
	if p == nil {
		return nil
	}

	agent := storage.SpanRow{
		TenantID:           tenant,
		ReceivedAt:         receivedAt,
		AgentHostname:      p.GetHostName(),
		AgentEnv:           p.GetEnv(),
		AgentVersion:       p.GetAgentVersion(),
		TargetTPS:          p.GetTargetTPS(),
		ErrorTPS:           p.GetErrorTPS(),
		RareSamplerEnabled: traceBool(p.GetRareSamplerEnabled()),
		AgentTags:          p.GetTags(),
	}

	var out []storage.SpanRow
	for _, tp := range p.GetTracerPayloads() {
		out = append(out, traceV04TracerRows(agent, tp)...)
	}
	for _, tp := range p.GetIdxTracerPayloads() {
		out = append(out, traceIdxTracerRows(agent, tp)...)
	}
	return out
}

// traceV04TracerRows converts one v0.4 tracer payload.
func traceV04TracerRows(base storage.SpanRow, tp *pb.TracerPayload) []storage.SpanRow {
	if tp == nil {
		return nil
	}

	base.WireFormat = "v04"
	base.ContainerID = tp.GetContainerID()
	base.Language = tp.GetLanguageName()
	base.LanguageVersion = tp.GetLanguageVersion()
	base.TracerVersion = tp.GetTracerVersion()
	base.RuntimeID = tp.GetRuntimeID()
	base.TracerEnv = tp.GetEnv()
	base.TracerHostname = tp.GetHostname()
	base.AppVersion = tp.GetAppVersion()
	base.TracerTags = tp.GetTags()
	traceSetContainerDebugV04(&base, tp.GetContainerDebug())

	var out []storage.SpanRow
	for _, ch := range tp.GetChunks() {
		if ch == nil {
			continue
		}
		chunk := base
		chunk.Priority = ch.GetPriority()
		chunk.Origin = ch.GetOrigin()
		chunk.DroppedTrace = traceBool(ch.GetDroppedTrace())
		chunk.ChunkTags = ch.GetTags()
		// v0.4 has no SamplingMechanism field; the same information travels in
		// meta["_dd.p.dm"], which is kept in the meta map untouched.

		for _, sp := range ch.GetSpans() {
			if sp == nil {
				continue
			}
			out = append(out, traceV04SpanRow(chunk, sp))
		}
	}
	return out
}

// traceSetContainerDebugV04 fills the five ContainerDebug columns from a v0.4
// payload.
//
// The whole sub-message is a pointer the agent sets ONLY when resolving the
// container tags gave it trouble, so absent has to stay absent: a zero latency
// would read as "the lookup was instant". nil pointers reach the table's
// Nullable columns as NULL.
func traceSetContainerDebugV04(row *storage.SpanRow, cd *pb.ContainerDebug) {
	if cd == nil {
		return
	}
	row.ContainerDebugError = traceStrPtr(cd.GetError())
	row.ContainerDebugLatencyMs = traceI64Ptr(cd.GetLatencyMs())
	row.ContainerDebugWasBuffered = traceU8Ptr(traceBool(cd.GetWasBuffered()))
	row.ContainerDebugBufferMs = traceI64Ptr(cd.GetBufferMs())
	row.ContainerDebugBufferEvictionReason = traceStrPtr(cd.GetBufferEvictionReason())
}

// traceSetContainerDebugIdx does the same from a v1.0 payload.
//
// A separate function rather than a generic one over both types, because the
// idx variant is NOT the same message with a different package: its two string
// fields are string-table refs (ErrorRef, BufferEvictionReasonRef), so it has
// to resolve them and the v0.4 one does not.
func traceSetContainerDebugIdx(row *storage.SpanRow, s traceStrings, cd *idx.ContainerDebug) {
	if cd == nil {
		return
	}
	row.ContainerDebugError = traceStrPtr(s.at(cd.GetErrorRef()))
	row.ContainerDebugLatencyMs = traceI64Ptr(cd.GetLatencyMs())
	row.ContainerDebugWasBuffered = traceU8Ptr(traceBool(cd.GetWasBuffered()))
	row.ContainerDebugBufferMs = traceI64Ptr(cd.GetBufferMs())
	row.ContainerDebugBufferEvictionReason = traceStrPtr(s.at(cd.GetBufferEvictionReasonRef()))
}

// traceV04SpanRow converts one v0.4 span onto an already-filled chunk row.
func traceV04SpanRow(row storage.SpanRow, sp *pb.Span) storage.SpanRow {
	meta := sp.GetMeta()

	row.Service = sp.GetService()
	row.Name = sp.GetName()
	row.Resource = sp.GetResource()
	row.SpanType = sp.GetType()
	row.TraceID = sp.GetTraceID()
	// The upper half of a 128-bit trace id has no struct field in v0.4: it
	// travels as hex in meta["_dd.p.tid"]. Parsed here so both wire formats
	// leave the same pair of columns behind, and left in meta as well — the
	// map is stored exactly as it arrived.
	row.TraceIDHigh = traceParseHex64(meta["_dd.p.tid"])
	row.SpanID = sp.GetSpanID()
	row.ParentID = sp.GetParentID()
	row.Start = time.Unix(0, sp.GetStart()).UTC()
	row.DurationNs = sp.GetDuration()
	row.Error = sp.GetError()
	// v0.4 has no kind field; the tracer puts it in meta. idx has a real enum.
	row.Kind = meta["span.kind"]
	row.Meta = meta
	row.Metrics = sp.GetMetrics()
	row.MetaStruct = traceMetaStruct(sp.GetMetaStruct())

	for _, l := range sp.GetSpanLinks() {
		if l == nil {
			continue
		}
		row.LinkTraceID = append(row.LinkTraceID, l.GetTraceID())
		row.LinkTraceIDHigh = append(row.LinkTraceIDHigh, l.GetTraceIDHigh())
		row.LinkSpanID = append(row.LinkSpanID, l.GetSpanID())
		row.LinkAttributes = append(row.LinkAttributes, traceStringAttrsJSON(l.GetAttributes()))
		row.LinkTracestate = append(row.LinkTracestate, l.GetTracestate())
		row.LinkFlags = append(row.LinkFlags, l.GetFlags())
	}
	for _, e := range sp.GetSpanEvents() {
		if e == nil {
			continue
		}
		row.EventTime = append(row.EventTime, time.Unix(0, int64(e.GetTimeUnixNano())).UTC())
		row.EventName = append(row.EventName, e.GetName())
		row.EventAttributes = append(row.EventAttributes, traceV04AttrsJSON(e.GetAttributes()))
	}
	return row
}

// traceIdxTracerRows converts one v1.0 (idx) tracer payload.
//
// Every string in this format is an index into the payload's own table, so
// nothing is readable until resolved — see traceStrings.
func traceIdxTracerRows(base storage.SpanRow, tp *idx.TracerPayload) []storage.SpanRow {
	if tp == nil {
		return nil
	}
	s := traceStrings(tp.GetStrings())

	base.WireFormat = "idx"
	base.ContainerID = s.at(tp.GetContainerIDRef())
	base.Language = s.at(tp.GetLanguageNameRef())
	base.LanguageVersion = s.at(tp.GetLanguageVersionRef())
	base.TracerVersion = s.at(tp.GetTracerVersionRef())
	base.RuntimeID = s.at(tp.GetRuntimeIDRef())
	base.TracerEnv = s.at(tp.GetEnvRef())
	base.TracerHostname = s.at(tp.GetHostnameRef())
	base.AppVersion = s.at(tp.GetAppVersionRef())
	base.TracerTags = s.scalarAttrs(tp.GetAttributes())
	base.TracerAttributesJSON = s.attrsJSON(tp.GetAttributes())
	traceSetContainerDebugIdx(&base, s, tp.GetContainerDebug())

	var out []storage.SpanRow
	for _, ch := range tp.GetChunks() {
		if ch == nil {
			continue
		}
		chunk := base
		chunk.Priority = ch.GetPriority()
		chunk.Origin = s.at(ch.GetOriginRef())
		chunk.DroppedTrace = traceBool(ch.GetDroppedTrace())
		chunk.SamplingMechanism = ch.GetSamplingMechanism()
		chunk.ChunkTags = s.scalarAttrs(ch.GetAttributes())
		chunk.ChunkAttributesJSON = s.attrsJSON(ch.GetAttributes())
		// v1.0 moved the trace id to the chunk and made it the full 128 bits
		// as raw bytes, where v0.4 splits it between the span and a meta key.
		chunk.TraceIDHigh, chunk.TraceID = traceSplitID(ch.GetTraceID())

		for _, sp := range ch.GetSpans() {
			if sp == nil {
				continue
			}
			out = append(out, traceIdxSpanRow(chunk, s, sp))
		}
	}
	return out
}

// traceIdxSpanRow converts one idx span onto an already-filled chunk row.
func traceIdxSpanRow(row storage.SpanRow, s traceStrings, sp *idx.Span) storage.SpanRow {
	row.Service = s.at(sp.GetServiceRef())
	row.Name = s.at(sp.GetNameRef())
	row.Resource = s.at(sp.GetResourceRef())
	row.SpanType = s.at(sp.GetTypeRef())
	row.SpanID = sp.GetSpanID()
	row.ParentID = sp.GetParentID()
	row.Start = time.Unix(0, int64(sp.GetStart())).UTC()
	row.DurationNs = int64(sp.GetDuration())
	if sp.GetError() {
		// v0.4 ships an int32 flag and idx a bool; the column keeps the v0.4
		// width so one query reads both.
		row.Error = 1
	}
	row.Kind = traceSpanKind(sp.GetKind())
	row.SpanEnv = s.at(sp.GetEnvRef())
	row.SpanVersion = s.at(sp.GetVersionRef())
	row.Component = s.at(sp.GetComponentRef())

	// idx replaced Meta/Metrics/MetaStruct with one typed map. The scalars are
	// projected back into the three columns so a query written for v0.4 works
	// unchanged; attributes_json holds every attribute in its declared type,
	// including the bytes, arrays and nested key-value lists that no
	// projection can carry.
	row.Meta, row.Metrics, row.MetaStruct = s.projectAttrs(sp.GetAttributes())
	row.AttributesJSON = s.attrsJSON(sp.GetAttributes())

	for _, l := range sp.GetLinks() {
		if l == nil {
			continue
		}
		high, low := traceSplitID(l.GetTraceID())
		row.LinkTraceID = append(row.LinkTraceID, low)
		row.LinkTraceIDHigh = append(row.LinkTraceIDHigh, high)
		row.LinkSpanID = append(row.LinkSpanID, l.GetSpanID())
		row.LinkAttributes = append(row.LinkAttributes, s.attrsJSON(l.GetAttributes()))
		row.LinkTracestate = append(row.LinkTracestate, s.at(l.GetTracestateRef()))
		row.LinkFlags = append(row.LinkFlags, l.GetFlags())
	}
	for _, e := range sp.GetEvents() {
		if e == nil {
			continue
		}
		row.EventTime = append(row.EventTime, time.Unix(0, int64(e.GetTime())).UTC())
		row.EventName = append(row.EventName, s.at(e.GetNameRef()))
		row.EventAttributes = append(row.EventAttributes, s.attrsJSON(e.GetAttributes()))
	}
	return row
}

// traceStrings resolves the idx string table.
//
// Index 0 is reserved for the empty string, so a Ref of 0 means "unset" and
// must never be used as an index. An out-of-range ref means the payload is
// inconsistent; it resolves to empty rather than panicking, because one bad
// ref must not cost the whole batch around it.
type traceStrings []string

func (s traceStrings) at(ref uint32) string {
	if ref == 0 || int(ref) >= len(s) {
		return ""
	}
	return s[ref]
}

// scalarAttrs renders an idx attribute map as the plain string map the
// tracer_tags / chunk_tags columns take. Non-scalar values are skipped here
// and survive in the matching *_attributes_json column.
func (s traceStrings) scalarAttrs(attrs map[uint32]*idx.AnyValue) map[string]string {
	if len(attrs) == 0 {
		return nil
	}
	out := make(map[string]string, len(attrs))
	for k, v := range attrs {
		key := s.at(k)
		switch av := v.GetValue().(type) {
		case *idx.AnyValue_StringValueRef:
			out[key] = s.at(av.StringValueRef)
		case *idx.AnyValue_BoolValue:
			out[key] = strconv.FormatBool(av.BoolValue)
		case *idx.AnyValue_IntValue:
			out[key] = strconv.FormatInt(av.IntValue, 10)
		case *idx.AnyValue_DoubleValue:
			out[key] = strconv.FormatFloat(av.DoubleValue, 'g', -1, 64)
		}
	}
	return out
}

// projectAttrs splits an idx span's typed attributes into the three v0.4
// maps: strings and bools to meta, numbers to metrics, bytes to meta_struct.
//
// An int attribute goes to metrics as a float64, which loses precision above
// 2^53 — accepted here and only here, because attributes_json keeps the exact
// int64 beside it. The projection is the convenience; the JSON is the record.
func (s traceStrings) projectAttrs(attrs map[uint32]*idx.AnyValue) (map[string]string, map[string]float64, map[string]string) {
	if len(attrs) == 0 {
		return nil, nil, nil
	}
	var meta, metaStruct map[string]string
	var metrics map[string]float64

	for k, v := range attrs {
		key := s.at(k)
		switch av := v.GetValue().(type) {
		case *idx.AnyValue_StringValueRef:
			if meta == nil {
				meta = map[string]string{}
			}
			meta[key] = s.at(av.StringValueRef)
		case *idx.AnyValue_BoolValue:
			if meta == nil {
				meta = map[string]string{}
			}
			meta[key] = strconv.FormatBool(av.BoolValue)
		case *idx.AnyValue_IntValue:
			if metrics == nil {
				metrics = map[string]float64{}
			}
			metrics[key] = float64(av.IntValue)
		case *idx.AnyValue_DoubleValue:
			if metrics == nil {
				metrics = map[string]float64{}
			}
			metrics[key] = av.DoubleValue
		case *idx.AnyValue_BytesValue:
			if metaStruct == nil {
				metaStruct = map[string]string{}
			}
			metaStruct[key] = string(av.BytesValue)
		}
	}
	return meta, metrics, metaStruct
}

// attrsJSON renders an idx attribute map as a JSON object whose values are
// TAGGED with their wire type: {"k":{"string":"v"},"n":{"int":5}}.
//
// The tag is the point. An attribute's type is part of what the tracer said,
// and a bare JSON value would turn 5, 5.0 and "5" into the same thing, while
// arrays and nested key-value lists have no untagged form at all. The same
// encoding is used for v0.4 span events and for both formats' span links, so
// one reader handles every attribute column in the table.
func (s traceStrings) attrsJSON(attrs map[uint32]*idx.AnyValue) string {
	if len(attrs) == 0 {
		return ""
	}
	out := make(map[string]any, len(attrs))
	for k, v := range attrs {
		out[s.at(k)] = s.anyValue(v, 0)
	}
	return traceJSON(out)
}

// traceAttrMaxDepth caps how far anyValue will follow a nested attribute.
//
// AnyValue is recursive on the wire (an array of key-value lists of arrays…)
// and nothing in the protobuf bounds it, so a hostile — or merely broken —
// tracer can hand us a value nested deep enough to exhaust the goroutine
// stack. A Go stack overflow is a fatal runtime error, not a panic a handler
// can recover, so it would take the whole node down rather than one request.
// Real tracers nest two or three levels; OTLP's own SDKs do not go past a
// handful. 32 is far past anything legitimate and far short of anything
// dangerous.
//
// This does not protect the DECODE itself: idx.TracerPayload arrives through
// vtprotobuf's generated UnmarshalVT, which has no nesting limit of its own
// and would blow the stack before this function is ever called. That is
// upstream generated code we do not own; the guard here is what keeps OUR
// conversion from being the thing that falls over, and it also bounds
// json.Marshal, which recurses over the value we build.
const traceAttrMaxDepth = 32

func (s traceStrings) anyValue(v *idx.AnyValue, depth int) any {
	if depth >= traceAttrMaxDepth {
		// Truncated, and SAID so: an empty value here would read as "the
		// tracer sent nothing", which is the one thing that is not true.
		return map[string]any{"_depth_exceeded": traceAttrMaxDepth}
	}
	switch av := v.GetValue().(type) {
	case *idx.AnyValue_StringValueRef:
		return map[string]any{"string": s.at(av.StringValueRef)}
	case *idx.AnyValue_BoolValue:
		return map[string]any{"bool": av.BoolValue}
	case *idx.AnyValue_IntValue:
		return map[string]any{"int": av.IntValue}
	case *idx.AnyValue_DoubleValue:
		return map[string]any{"double": av.DoubleValue}
	case *idx.AnyValue_BytesValue:
		// Base64 by json.Marshal's []byte rule, which is lossless and the
		// only portable way to put arbitrary bytes in JSON.
		return map[string]any{"bytes": av.BytesValue}
	case *idx.AnyValue_ArrayValue:
		vals := av.ArrayValue.GetValues()
		items := make([]any, 0, len(vals))
		for _, item := range vals {
			items = append(items, s.anyValue(item, depth+1))
		}
		return map[string]any{"array": items}
	case *idx.AnyValue_KeyValueList:
		kvs := av.KeyValueList.GetKeyValues()
		// A list, not an object: the wire is repeated KeyValue, so two
		// entries may share a key and the order is the tracer's.
		items := make([]any, 0, len(kvs))
		for _, kv := range kvs {
			items = append(items, map[string]any{
				"key":   s.at(kv.GetKey()),
				"value": s.anyValue(kv.GetValue(), depth+1),
			})
		}
		return map[string]any{"kvlist": items}
	}
	// A oneof arm this build does not know: recorded as null rather than
	// dropped, so the key still shows the attribute existed.
	return nil
}

// traceV04AttrsJSON renders a v0.4 span event's typed attributes in the same
// tagged form as the idx ones.
//
// AttributeAnyValue is a hand-rolled union (msgp cannot generate a protobuf
// oneof), so the discriminant decides which field is meaningful and the rest
// are ZERO, not nil — reading the wrong one is easy and silent.
func traceV04AttrsJSON(attrs map[string]*pb.AttributeAnyValue) string {
	if len(attrs) == 0 {
		return ""
	}
	out := make(map[string]any, len(attrs))
	for k, v := range attrs {
		out[k] = traceV04AnyValue(v)
	}
	return traceJSON(out)
}

func traceV04AnyValue(v *pb.AttributeAnyValue) any {
	if v == nil {
		return nil
	}
	switch v.GetType() {
	case pb.AttributeAnyValue_STRING_VALUE:
		return map[string]any{"string": v.GetStringValue()}
	case pb.AttributeAnyValue_BOOL_VALUE:
		return map[string]any{"bool": v.GetBoolValue()}
	case pb.AttributeAnyValue_INT_VALUE:
		return map[string]any{"int": v.GetIntValue()}
	case pb.AttributeAnyValue_DOUBLE_VALUE:
		return map[string]any{"double": v.GetDoubleValue()}
	case pb.AttributeAnyValue_ARRAY_VALUE:
		vals := v.GetArrayValue().GetValues()
		items := make([]any, 0, len(vals))
		for _, item := range vals {
			items = append(items, traceV04ArrayValue(item))
		}
		return map[string]any{"array": items}
	}
	return nil
}

// traceV04ArrayValue handles the array element type, which is the same union
// minus the array arm — v0.4 arrays cannot nest.
func traceV04ArrayValue(v *pb.AttributeArrayValue) any {
	if v == nil {
		return nil
	}
	switch v.GetType() {
	case pb.AttributeArrayValue_STRING_VALUE:
		return map[string]any{"string": v.GetStringValue()}
	case pb.AttributeArrayValue_BOOL_VALUE:
		return map[string]any{"bool": v.GetBoolValue()}
	case pb.AttributeArrayValue_INT_VALUE:
		return map[string]any{"int": v.GetIntValue()}
	case pb.AttributeArrayValue_DOUBLE_VALUE:
		return map[string]any{"double": v.GetDoubleValue()}
	}
	return nil
}

// traceStringAttrsJSON renders a v0.4 span link's plain string attributes in
// the same tagged form, so the link_attributes column has one shape across
// both wire formats.
func traceStringAttrsJSON(attrs map[string]string) string {
	if len(attrs) == 0 {
		return ""
	}
	out := make(map[string]any, len(attrs))
	for k, v := range attrs {
		out[k] = map[string]any{"string": v}
	}
	return traceJSON(out)
}

// traceJSON marshals a value for one of the *_json columns. json.Marshal
// sorts map keys, so the output is stable and two identical attribute sets
// compare equal in a query.
func traceJSON(v any) string {
	b, err := json.Marshal(v)
	if err != nil {
		// Nothing here can fail today (maps of strings, numbers, bools and
		// []byte), but a silent empty column would look like "no attributes".
		return fmt.Sprintf(`{"_encode_error":%q}`, err.Error())
	}
	return string(b)
}

// traceMetaStruct turns MetaStruct's msgpack blobs into the map the column
// takes. ClickHouse String is a byte string, so the bytes survive whole; they
// are NOT text and a reader must not treat them as such.
func traceMetaStruct(ms map[string][]byte) map[string]string {
	if len(ms) == 0 {
		return nil
	}
	out := make(map[string]string, len(ms))
	for k, v := range ms {
		out[k] = string(v)
	}
	return out
}

// traceSplitID splits an idx 128-bit trace id into (high, low).
//
// The wire carries big-endian bytes. A short id is left-padded and a long one
// truncated from the left, which is what a fixed-width integer pair means —
// the alternative is dropping an id we could have read.
func traceSplitID(b []byte) (high, low uint64) {
	if len(b) == 0 {
		return 0, 0
	}
	var buf [16]byte
	if len(b) >= 16 {
		copy(buf[:], b[len(b)-16:])
	} else {
		copy(buf[16-len(b):], b)
	}
	return binary.BigEndian.Uint64(buf[:8]), binary.BigEndian.Uint64(buf[8:])
}

// traceParseHex64 reads the hex upper half of a 128-bit trace id out of
// meta["_dd.p.tid"]. An unparseable value yields 0 and stays in meta, where a
// reader can still see what arrived.
func traceParseHex64(s string) uint64 {
	if s == "" {
		return 0
	}
	v, err := strconv.ParseUint(s, 16, 64)
	if err != nil {
		return 0
	}
	return v
}

// traceSpanKind renders the idx SpanKind enum as the lowercase OTel name, the
// same vocabulary v0.4 tracers put in meta["span.kind"]. UNSPECIFIED becomes
// empty: "the tracer did not say" is what an absent v0.4 meta key means too.
func traceSpanKind(k idx.SpanKind) string {
	name, ok := idx.SpanKind_name[int32(k)]
	if !ok || k == idx.SpanKind_SPAN_KIND_UNSPECIFIED {
		return ""
	}
	return strings.ToLower(strings.TrimPrefix(name, "SPAN_KIND_"))
}

func traceBool(b bool) uint8 {
	if b {
		return 1
	}
	return 0
}

func traceStrPtr(s string) *string { return &s }
func traceI64Ptr(v int64) *int64   { return &v }
func traceU8Ptr(v uint8) *uint8    { return &v }

// ---------------------------------------------------------------------------
// Logging. These lines are how the protocol was read in the first place and
// they stay — but they are no longer the only sink.
// ---------------------------------------------------------------------------

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
	// readable without resolving it. Counted here rather than expanded,
	// because nothing on this host has sent one yet and the expansion would
	// be written blind; the ROWS it produces are complete regardless.
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

// ---------------------------------------------------------------------------
// /api/v0.2/stats -> apm_stats
// ---------------------------------------------------------------------------

// HandleAPMStats accepts POST /api/v0.2/stats.
//
// msgpack, not protobuf — the only place in Datadog's protocol where that
// encoding appears. The generated type carries its own decoder.
//
// What arrives is NOT a sample: the agent feeds every trace to its
// concentrator before the sampler runs, so these counts are complete even when
// 99% of the spans were thrown away. That is why this endpoint exists apart
// from the traces one.
//
// NOTE FOR THE FUTURE: the Android SDK from 3.14 may POST msgpack here that is
// not a pb.StatsPayload. It fails the decode below and goes to raw_payloads
// like anything else — never dropped.
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

	var payload pb.StatsPayload
	if _, err := payload.UnmarshalMsg(body); err != nil {
		log.Printf("[apm-stats] msgpack StatsPayload: %v (%d bytes)", err, len(body))
		a.storeRaw(c, traceIntake, "decode_error",
			fmt.Sprintf("msgpack StatsPayload: %v", err), body)
		return
	}

	// Decode every latency sketch first, for every group in every bucket of
	// every client. The log below stops at statsLogGroups; this loop does not,
	// so a group past the limit still has its sketches decoded. A summary is
	// kept for EVERY group — including the ones whose bytes did not decode,
	// because "undecodable" is a state the table records, not a reason to
	// store nothing.
	okSummaries := make(map[*pb.ClientGroupedStats]storage.SketchSummary)
	errSummaries := make(map[*pb.ClientGroupedStats]storage.SketchSummary)
	okSketches := make(map[*pb.ClientGroupedStats]*ddsketch.DDSketch)
	errSketches := make(map[*pb.ClientGroupedStats]*ddsketch.DDSketch)
	for _, cs := range payload.GetStats() {
		for _, b := range cs.GetStats() {
			for _, g := range b.GetStats() {
				sum, sk, err := statsSummary(g.GetOkSummary())
				if err != nil {
					log.Printf("[apm-stats] %s %s ok_summary %d B: %v", g.GetService(), g.GetName(), len(g.GetOkSummary()), err)
				}
				okSummaries[g] = sum
				if sk != nil {
					okSketches[g] = sk
				}

				sum, sk, err = statsSummary(g.GetErrorSummary())
				if err != nil {
					log.Printf("[apm-stats] %s %s error_summary %d B: %v", g.GetService(), g.GetName(), len(g.GetErrorSummary()), err)
				}
				errSummaries[g] = sum
				if sk != nil {
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

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := apmStatRows(tenant, time.Now().UTC(), &payload, okSummaries, errSummaries)
	a.store(storage.APMStatsWriter, storage.WriteAPMStats{Stats: rows}, len(rows))
}

// apmStatRows flattens a StatsPayload into one row per ClientGroupedStats,
// with the payload, client and bucket levels denormalized onto each.
func apmStatRows(tenant string, receivedAt time.Time, p *pb.StatsPayload,
	okSummaries, errSummaries map[*pb.ClientGroupedStats]storage.SketchSummary) []storage.APMStatRow {
	if p == nil {
		return nil
	}

	base := storage.APMStatRow{
		TenantID:       tenant,
		ReceivedAt:     receivedAt,
		AgentHostname:  p.GetAgentHostname(),
		AgentEnv:       p.GetAgentEnv(),
		AgentVersion:   p.GetAgentVersion(),
		ClientComputed: traceBool(p.GetClientComputed()),
		SplitPayload:   traceBool(p.GetSplitPayload()),
	}

	var out []storage.APMStatRow
	for _, cs := range p.GetStats() {
		if cs == nil {
			continue
		}
		client := base
		client.ClientHostname = cs.GetHostname()
		client.ClientEnv = cs.GetEnv()
		client.ClientVersion = cs.GetVersion()
		client.ClientLang = cs.GetLang()
		client.ClientTracerVersion = cs.GetTracerVersion()
		client.ClientRuntimeID = cs.GetRuntimeID()
		client.ClientSequence = cs.GetSequence()
		client.ClientAgentAggregation = cs.GetAgentAggregation()
		client.ClientService = cs.GetService()
		client.ClientContainerID = cs.GetContainerID()
		// The one genuine flat "key:value" tag list in the trace protocol, so
		// the one place tagsToMultiMap applies here — a tag key may repeat and
		// a plain map would keep whichever came last.
		client.ClientTags = tagsToMultiMap(cs.GetTags())
		client.ClientGitCommitSha = cs.GetGitCommitSha()
		client.ClientImageTag = cs.GetImageTag()
		client.ClientProcessTags = cs.GetProcessTags()
		client.ClientProcessTagsHash = cs.GetProcessTagsHash()

		for _, b := range cs.GetStats() {
			if b == nil {
				continue
			}
			bucket := client
			bucket.BucketStart = time.Unix(0, int64(b.GetStart())).UTC()
			bucket.BucketDurationNs = b.GetDuration()
			bucket.AgentTimeShiftNs = b.GetAgentTimeShift()

			for _, g := range b.GetStats() {
				if g == nil {
					continue
				}
				row := bucket
				row.Service = g.GetService()
				row.Name = g.GetName()
				row.Resource = g.GetResource()
				row.HTTPStatusCode = g.GetHTTPStatusCode()
				row.SpanType = g.GetType()
				row.DBType = g.GetDBType()
				row.Hits = g.GetHits()
				row.Errors = g.GetErrors()
				row.DurationNs = g.GetDuration()
				row.Synthetics = traceBool(g.GetSynthetics())
				row.TopLevelHits = g.GetTopLevelHits()
				row.SpanKind = g.GetSpanKind()
				row.PeerTags = g.GetPeerTags()
				row.IsTraceRoot = statsTrilean(g.GetIsTraceRoot())
				row.GRPCStatusCode = g.GetGRPCStatusCode()
				row.HTTPMethod = g.GetHTTPMethod()
				row.HTTPEndpoint = g.GetHTTPEndpoint()
				row.ServiceSource = g.GetServiceSource()
				row.SpanDerivedPrimaryTags = g.GetSpanDerivedPrimaryTags()
				row.AdditionalMetricTags = g.GetAdditionalMetricTags()
				row.OkSummary = apmSummary(okSummaries, g, g.GetOkSummary())
				row.ErrorSummary = apmSummary(errSummaries, g, g.GetErrorSummary())
				out = append(out, row)
			}
		}
	}
	return out
}

// apmSummary looks up a summary the handler already decoded, falling back to
// decoding it here. The fallback is what lets the pure conversion function be
// called on its own in a test, without the handler's decode pass.
func apmSummary(m map[*pb.ClientGroupedStats]storage.SketchSummary, g *pb.ClientGroupedStats, raw []byte) storage.SketchSummary {
	if s, ok := m[g]; ok {
		return s
	}
	s, _, _ := statsSummary(raw)
	return s
}

// statsSummary turns one latency summary into the eight columns the table
// keeps: the raw bytes, the state, and — only when it really decoded — the
// numbers. It returns the sketch too, for the log, and the decode error, which
// the caller logs but does NOT treat as a reason to store nothing.
func statsSummary(raw []byte) (storage.SketchSummary, *ddsketch.DDSketch, error) {
	s := storage.SketchSummary{Raw: string(raw), State: storage.SketchAbsent}
	if len(raw) == 0 {
		return s, nil, nil
	}

	sk, err := statsDecodeSketch(raw)
	if err != nil {
		// Bytes arrived and are not a DDSketch. They stay in the raw column:
		// this is exactly the payload somebody will want when the version
		// skew that caused it is investigated.
		s.State = storage.SketchUndecodable
		return s, nil, err
	}
	if sk.IsEmpty() {
		// A valid sketch with no values — different from no sketch at all.
		s.State = storage.SketchEmpty
		return s, sk, nil
	}

	s.State = storage.SketchOK
	count := sk.GetCount()
	sum := sk.GetSum()
	s.Count = &count
	s.Sum = &sum
	if v, err := sk.GetMinValue(); err == nil {
		s.Min = &v
	}
	if v, err := sk.GetMaxValue(); err == nil {
		s.Max = &v
	}
	// The positive-value store only: these are durations and sizes, which are
	// non-negative by construction. The raw bytes keep the negative store and
	// the zero count regardless, so nothing is lost by not breaking them out.
	sk.GetPositiveValueStore().ForEach(func(index int, count float64) bool {
		s.BinKeys = append(s.BinKeys, int32(index))
		s.BinCounts = append(s.BinCounts, count)
		return false
	})
	return s, sk, nil
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

// statsTrilean names a pb.Trilean. The value goes to an Enum8 column, which
// travels by NAME through the driver, so these three strings are the schema's
// vocabulary and not just log text.
func statsTrilean(t pb.Trilean) string {
	switch t {
	case pb.Trilean_TRUE:
		return "true"
	case pb.Trilean_FALSE:
		return "false"
	}
	return "not_set"
}

// ---------------------------------------------------------------------------
// /api/v0.1/pipeline_stats -> dsm_pipeline_stats, dsm_backlogs,
// dsm_bucket_transactions
// ---------------------------------------------------------------------------

// HandlePipelineStats accepts POST /api/v0.1/pipeline_stats.
//
// Data Streams Monitoring stats straight from the tracer. The trace-agent is a
// pure reverse proxy here (pkg/trace/api/pipeline_stats.go): it swaps the
// local /v0.1/pipeline_stats path for this one, adds Via,
// X-Datadog-Additional-Tags and X-Datadog-Container-Tags, and forwards the
// body untouched.
//
// So the schema is dd-trace-go's, not the agent's: msgpack of
// internal/datastreams.StatsPayload, generated by tinylib/msgp with no field
// tags, which means the map keys ARE the Go field names. That is what
// dsmDecodeStatsPayload reads. Anything that does not decode goes to
// raw_payloads with the msgp error in the note.
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

	// Decompress() replaces the body but leaves the header alone, so this is
	// the encoding the tracer put on the wire.
	log.Printf("[pipeline-stats] encoding=%q via=%q additional_tags=%q container_tags=%q",
		c.GetHeader("Content-Encoding"), c.GetHeader("Via"),
		c.GetHeader("X-Datadog-Additional-Tags"), c.GetHeader("X-Datadog-Container-Tags"))
	describe("pipeline-stats", c.GetHeader("Content-Type"), body)

	payload, err := dsmDecodeStatsPayload(body)
	if err != nil {
		log.Printf("[pipeline-stats] msgpack datastreams.StatsPayload: %v (%d bytes)", err, len(body))
		a.storeRaw(c, traceIntake, "decode_error",
			fmt.Sprintf("msgpack datastreams.StatsPayload: %v", err), body)
		return
	}

	log.Printf("[pipeline-stats] env=%s service=%s lang=%s tracer=%s version=%s | %d buckets",
		traceOrDash(payload.Env), traceOrDash(payload.Service), traceOrDash(payload.Lang),
		traceOrDash(payload.TracerVersion), traceOrDash(payload.Version), len(payload.Buckets))

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	hdr := dsmHeaders{
		Via:             c.GetHeader("Via"),
		AdditionalTags:  c.GetHeader("X-Datadog-Additional-Tags"),
		ContainerTags:   c.GetHeader("X-Datadog-Container-Tags"),
		ContentEncoding: c.GetHeader("Content-Encoding"),
	}
	points, backlogs, buckets := dsmRows(tenant, time.Now().UTC(), payload, hdr)
	a.store(storage.DSMPipelineStatsWriter, storage.WriteDSMPipelineStats{Points: points}, len(points))
	a.store(storage.DSMBacklogsWriter, storage.WriteDSMBacklogs{Backlogs: backlogs}, len(backlogs))
	a.store(storage.DSMBucketTransactionsWriter, storage.WriteDSMBucketTransactions{Buckets: buckets}, len(buckets))
}

// dsmHeaders carries the four request headers the body does not repeat. The
// trace-agent adds three of them on the way through and they hold the only
// container identity a pipeline payload ever has.
type dsmHeaders struct {
	Via             string
	AdditionalTags  string
	ContainerTags   string
	ContentEncoding string
}

// dsmRows fans one decoded payload out into the three tables it feeds.
func dsmRows(tenant string, receivedAt time.Time, p *dsmStatsPayload, hdr dsmHeaders) (
	[]storage.DSMPipelineStatRow, []storage.DSMBacklogRow, []storage.DSMBucketTransactionRow) {
	if p == nil {
		return nil, nil, nil
	}

	common := storage.DSMCommon{
		TenantID:      tenant,
		ReceivedAt:    receivedAt,
		Env:           p.Env,
		Service:       p.Service,
		TracerVersion: p.TracerVersion,
		Lang:          p.Lang,
		Version:       p.Version,
		ProcessTags:   p.ProcessTags,
		ProductMask:   p.ProductMask,
	}

	var points []storage.DSMPipelineStatRow
	var backlogs []storage.DSMBacklogRow
	var buckets []storage.DSMBucketTransactionRow

	for _, b := range p.Buckets {
		bc := common
		bc.BucketStart = time.Unix(0, int64(b.Start)).UTC()
		bc.BucketDurationNs = b.Duration

		for _, sp := range b.Points {
			row := storage.DSMPipelineStatRow{
				DSMCommon:       bc,
				EdgeTags:        sp.EdgeTags,
				Hash:            sp.Hash,
				ParentHash:      sp.ParentHash,
				TimestampType:   sp.TimestampType,
				PathwayLatency:  dsmSketch(sp.PathwayLatency),
				EdgeLatency:     dsmSketch(sp.EdgeLatency),
				PayloadSize:     dsmSketch(sp.PayloadSize),
				Via:             hdr.Via,
				AdditionalTags:  hdr.AdditionalTags,
				ContainerTags:   hdr.ContainerTags,
				ContentEncoding: hdr.ContentEncoding,
			}
			// Unknown keys from all three levels ride the point row, prefixed
			// by where they were found. One column pair for the whole payload
			// keeps the schema stable while dd-trace-go grows fields.
			unknown := dsmMergeUnknown(p.Unknown, b.Unknown, sp.Unknown)
			row.UnknownKeys = dsmSortedKeys(unknown)
			if len(unknown) > 0 {
				row.UnknownJSON = traceJSON(unknown)
			}
			points = append(points, row)
		}

		for _, bl := range b.Backlogs {
			backlogs = append(backlogs, storage.DSMBacklogRow{
				DSMCommon: bc,
				Tags:      bl.Tags,
				Value:     bl.Value,
			})
		}

		// Its own table because these hang off the BUCKET: a bucket can carry
		// them with no stats points at all, and folding them into a point row
		// would either duplicate or lose them.
		if len(b.Transactions) > 0 || len(b.TransactionCheckpointIDs) > 0 {
			buckets = append(buckets, storage.DSMBucketTransactionRow{
				DSMCommon:                bc,
				Transactions:             string(b.Transactions),
				TransactionCheckpointIDs: string(b.TransactionCheckpointIDs),
			})
		}
	}
	return points, backlogs, buckets
}

// dsmSketch decodes one DSM distribution. Same three-state handling as the APM
// sketches, and the same rule: the raw bytes are kept whatever happens.
//
// NOTE the units differ from APM's. Pathway and edge latencies are SECONDS
// here, and PayloadSize is a distribution of message sizes in bytes — its
// dd-trace-go field name reads like a scalar, its []byte type does not.
func dsmSketch(raw []byte) storage.SketchSummary {
	s, _, _ := statsSummary(raw)
	return s
}

func dsmMergeUnknown(payload, bucket, point map[string]any) map[string]any {
	if len(payload)+len(bucket)+len(point) == 0 {
		return nil
	}
	out := make(map[string]any, len(payload)+len(bucket)+len(point))
	for k, v := range payload {
		out["payload."+k] = v
	}
	for k, v := range bucket {
		out["bucket."+k] = v
	}
	for k, v := range point {
		out["point."+k] = v
	}
	return out
}

func dsmSortedKeys(m map[string]any) []string {
	if len(m) == 0 {
		return nil
	}
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

// ---------------------------------------------------------------------------
// The Data Streams msgpack decoder.
//
// dd-trace-go generates its codec with tinylib/msgp and no field tags, so the
// wire is a msgpack map whose keys are the Go field names verbatim
// (internal/datastreams/payload.go, payload_msgp.go). The structs below mirror
// that file; they are NOT imported from dd-trace-go, which is not a dependency
// of this module — pulling a whole tracer in to read a tracer's payload would
// be the wrong trade.
//
// Every level keeps an Unknown map. dd-trace-go adds fields here with no
// version negotiation, so a key we do not know is expected, and skipping it
// would throw away the only evidence that a new field exists.
// ---------------------------------------------------------------------------

type dsmStatsPayload struct {
	Env           string
	Service       string
	TracerVersion string
	Lang          string
	Version       string
	ProcessTags   []string
	ProductMask   uint64
	Buckets       []dsmStatsBucket
	Unknown       map[string]any
}

type dsmStatsBucket struct {
	Start                    uint64
	Duration                 uint64
	Points                   []dsmStatsPoint
	Backlogs                 []dsmBacklog
	Transactions             []byte
	TransactionCheckpointIDs []byte
	Unknown                  map[string]any
}

type dsmStatsPoint struct {
	EdgeTags       []string
	Hash           uint64
	ParentHash     uint64
	PathwayLatency []byte
	EdgeLatency    []byte
	PayloadSize    []byte
	TimestampType  string
	Unknown        map[string]any
}

type dsmBacklog struct {
	Tags    []string
	Value   int64
	Unknown map[string]any
}

// dsmDecodeStatsPayload reads one msgpack StatsPayload.
func dsmDecodeStatsPayload(body []byte) (*dsmStatsPayload, error) {
	if len(body) == 0 {
		return nil, fmt.Errorf("empty body")
	}
	r := msgp.NewReader(bytes.NewReader(body))
	// ReadIntf (dsmUnknown) and the array readers allocate from the length
	// prefix before reading an element, guarded only by GetMaxElements, which
	// defaults to MaxUint32 and so never fires. Every element costs at least
	// one wire byte, so the body length is a sound ceiling: a forged header
	// now fails the decode (and lands in raw_payloads) instead of asking for
	// gigabytes. Same guard as civDecodeMsgpack.
	maxElems := uint32(math.MaxUint32)
	if len(body) < math.MaxUint32 {
		maxElems = uint32(len(body))
	}
	r.SetMaxElements(maxElems)
	p := &dsmStatsPayload{}

	err := dsmReadMap(r, func(key string) error {
		var err error
		switch key {
		case "Env":
			p.Env, err = r.ReadString()
		case "Service":
			p.Service, err = r.ReadString()
		case "TracerVersion":
			p.TracerVersion, err = r.ReadString()
		case "Lang":
			p.Lang, err = r.ReadString()
		case "Version":
			p.Version, err = r.ReadString()
		case "ProcessTags":
			p.ProcessTags, err = dsmReadStrings(r)
		case "ProductMask":
			p.ProductMask, err = r.ReadUint64()
		case "Stats":
			err = dsmReadArray(r, func() error {
				b, err := dsmReadBucket(r)
				if err != nil {
					return err
				}
				p.Buckets = append(p.Buckets, b)
				return nil
			})
		default:
			return dsmUnknown(r, key, &p.Unknown)
		}
		if err != nil {
			return fmt.Errorf("field %q: %w", key, err)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return p, nil
}

func dsmReadBucket(r *msgp.Reader) (dsmStatsBucket, error) {
	var b dsmStatsBucket
	err := dsmReadMap(r, func(key string) error {
		var err error
		switch key {
		case "Start":
			b.Start, err = r.ReadUint64()
		case "Duration":
			b.Duration, err = r.ReadUint64()
		case "Stats":
			err = dsmReadArray(r, func() error {
				p, err := dsmReadPoint(r)
				if err != nil {
					return err
				}
				b.Points = append(b.Points, p)
				return nil
			})
		case "Backlogs":
			err = dsmReadArray(r, func() error {
				bl, err := dsmReadBacklog(r)
				if err != nil {
					return err
				}
				b.Backlogs = append(b.Backlogs, bl)
				return nil
			})
		case "Transactions":
			b.Transactions, err = dsmReadBytes(r)
		case "TransactionCheckpointIds":
			// "Ids", not "IDs": the msgpack key is the Go field name in
			// dd-trace-go, which spells it that way deliberately to match the
			// Java tracer's wire format.
			b.TransactionCheckpointIDs, err = dsmReadBytes(r)
		default:
			return dsmUnknown(r, key, &b.Unknown)
		}
		if err != nil {
			return fmt.Errorf("bucket field %q: %w", key, err)
		}
		return nil
	})
	return b, err
}

func dsmReadPoint(r *msgp.Reader) (dsmStatsPoint, error) {
	var p dsmStatsPoint
	err := dsmReadMap(r, func(key string) error {
		var err error
		switch key {
		case "EdgeTags":
			p.EdgeTags, err = dsmReadStrings(r)
		case "Hash":
			p.Hash, err = r.ReadUint64()
		case "ParentHash":
			p.ParentHash, err = r.ReadUint64()
		case "PathwayLatency":
			p.PathwayLatency, err = dsmReadBytes(r)
		case "EdgeLatency":
			p.EdgeLatency, err = dsmReadBytes(r)
		case "PayloadSize":
			p.PayloadSize, err = dsmReadBytes(r)
		case "TimestampType":
			p.TimestampType, err = r.ReadString()
		default:
			return dsmUnknown(r, key, &p.Unknown)
		}
		if err != nil {
			return fmt.Errorf("point field %q: %w", key, err)
		}
		return nil
	})
	return p, err
}

func dsmReadBacklog(r *msgp.Reader) (dsmBacklog, error) {
	var b dsmBacklog
	err := dsmReadMap(r, func(key string) error {
		var err error
		switch key {
		case "Tags":
			b.Tags, err = dsmReadStrings(r)
		case "Value":
			b.Value, err = r.ReadInt64()
		default:
			return dsmUnknown(r, key, &b.Unknown)
		}
		if err != nil {
			return fmt.Errorf("backlog field %q: %w", key, err)
		}
		return nil
	})
	return b, err
}

// dsmReadMap walks a msgpack map, calling fn once per key. A nil in place of
// the map is accepted and leaves the struct zero — msgp writes nil for an
// absent value, and refusing it would fail a payload that is perfectly valid.
func dsmReadMap(r *msgp.Reader, fn func(key string) error) error {
	if isNil, err := dsmReadNil(r); err != nil || isNil {
		return err
	}
	n, err := r.ReadMapHeader()
	if err != nil {
		return err
	}
	for i := uint32(0); i < n; i++ {
		key, err := r.ReadString()
		if err != nil {
			return err
		}
		if err := fn(key); err != nil {
			return err
		}
	}
	return nil
}

func dsmReadArray(r *msgp.Reader, fn func() error) error {
	if isNil, err := dsmReadNil(r); err != nil || isNil {
		return err
	}
	n, err := r.ReadArrayHeader()
	if err != nil {
		return err
	}
	for i := uint32(0); i < n; i++ {
		if err := fn(); err != nil {
			return err
		}
	}
	return nil
}

func dsmReadStrings(r *msgp.Reader) ([]string, error) {
	var out []string
	err := dsmReadArray(r, func() error {
		s, err := r.ReadString()
		if err != nil {
			return err
		}
		out = append(out, s)
		return nil
	})
	return out, err
}

func dsmReadBytes(r *msgp.Reader) ([]byte, error) {
	if isNil, err := dsmReadNil(r); err != nil || isNil {
		return nil, err
	}
	return r.ReadBytes(nil)
}

func dsmReadNil(r *msgp.Reader) (bool, error) {
	t, err := r.NextType()
	if err != nil {
		return false, err
	}
	if t != msgp.NilType {
		return false, nil
	}
	return true, r.ReadNil()
}

// dsmUnknown keeps a field this decoder does not know, VALUE AND ALL.
//
// Recording only the name would say a field exists and nothing about what it
// carries, and the payload that would answer that arrives once. ReadIntf gives
// the value as ordinary Go types, which json.Marshal then renders — []byte
// becomes base64, which is lossless.
func dsmUnknown(r *msgp.Reader, key string, dst *map[string]any) error {
	v, err := r.ReadIntf()
	if err != nil {
		return fmt.Errorf("unknown field %q: %w", key, err)
	}
	if *dst == nil {
		*dst = map[string]any{}
	}
	(*dst)[key] = v
	return nil
}

// ---------------------------------------------------------------------------
// /api/v2/data_streams_messages -> dsm_messages
// ---------------------------------------------------------------------------

// HandleDataStreamsMessages accepts POST /api/v2/data_streams_messages.
//
// Not the trace-agent at all — an Event Platform track that happens to live on
// the trace.agent host (comp/forwarder/eventplatform/impl/
// pipelines_datastreams.go, event type "data-streams-message"). JSON, so the
// forwarder batches: the body is one array of raw messages (docs §0.4).
//
// The producer is not in the open agent repository, so no shape is invented:
// each element is stored as the JSON text it arrived as, with its top-level
// key names beside it. A body that is not an array goes to raw_payloads.
func (a *Server) HandleDataStreamsMessages(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})

	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[data-streams] cannot read body: %v", err)
		return
	}

	var msgs []json.RawMessage
	if err := json.Unmarshal(body, &msgs); err != nil {
		log.Printf("[data-streams] JSON array: %v (%d bytes)", err, len(body))
		describe("data-streams", c.GetHeader("Content-Type"), body)
		a.storeRaw(c, traceIntake, "unexpected_shape",
			fmt.Sprintf("body is not the JSON array the forwarder sends: %v", err), body)
		return
	}

	log.Printf("[data-streams] %d messages, %d B", len(msgs), len(body))

	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}
	rows := dsmMessageRows(tenant, time.Now().UTC(), msgs,
		c.GetHeader("Dd-Evp-Origin"), c.GetHeader("Dd-Evp-Origin-Version"),
		c.GetHeader("Content-Encoding"))
	a.store(storage.DSMMessagesWriter, storage.WriteDSMMessages{Messages: rows}, len(rows))
}

// dsmMessageRows turns the array into rows, keeping the batch position — the
// only ordering these messages have, since nothing inside them is known to be
// a timestamp.
func dsmMessageRows(tenant string, receivedAt time.Time, msgs []json.RawMessage,
	origin, originVersion, encoding string) []storage.DSMMessageRow {
	out := make([]storage.DSMMessageRow, 0, len(msgs))
	for i, m := range msgs {
		out = append(out, storage.DSMMessageRow{
			TenantID:           tenant,
			ReceivedAt:         receivedAt,
			Position:           uint32(i),
			Message:            string(m),
			Keys:               dsmMessageKeys(m),
			DDEVPOrigin:        origin,
			DDEVPOriginVersion: originVersion,
			ContentEncoding:    encoding,
		})
	}
	return out
}

// dsmMessageKeys lists an element's top-level key names, sorted.
//
// The one thing extractable without guessing at semantics, and enough to turn
// "what does this track actually send?" into a GROUP BY. An element that is
// not an object (an array, a number, a string) simply has none.
func dsmMessageKeys(m json.RawMessage) []string {
	var obj map[string]json.RawMessage
	if err := json.Unmarshal(m, &obj); err != nil {
		return nil
	}
	keys := make([]string, 0, len(obj))
	for k := range obj {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}
