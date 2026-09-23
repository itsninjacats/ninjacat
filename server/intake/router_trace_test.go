package intake

import (
	"bytes"
	"compress/gzip"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"testing"
	"time"

	"ergo.services/ergo/gen"
	pb "github.com/DataDog/datadog-agent/pkg/proto/pbgo/trace"
	"github.com/DataDog/datadog-agent/pkg/proto/pbgo/trace/idx"
	"github.com/DataDog/sketches-go/ddsketch"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
	"github.com/tinylib/msgp/msgp"
	"google.golang.org/protobuf/proto"
)

// The trace intake's conversion tests. Payloads are built as native Go proto
// literals, the way the other router tests in this package build theirs, and
// the pure conversion functions are called directly — no HTTP, no gin — so a
// failure names the field rather than the request.

var traceTestTime = time.Date(2026, 3, 4, 5, 6, 7, 0, time.UTC)

// ---------------------------------------------------------------------------
// v0.4
// ---------------------------------------------------------------------------

// A real Python tracer's payload: one agent, one tracer process, one chunk,
// two spans, the second a child of the first. This pins the thing the whole
// table is for — that the three envelope levels land on EVERY span row, so
// "which agent version shipped this" is answerable without a join.
func TestTraceSpanRowsDenormalizesEnvelope(t *testing.T) {
	payload := &pb.AgentPayload{
		HostName:           "ip-10-0-0-1",
		Env:                "prod",
		AgentVersion:       "7.58.2",
		TargetTPS:          10,
		ErrorTPS:           10,
		RareSamplerEnabled: true,
		Tags:               map[string]string{"cluster": "eu-1"},
		TracerPayloads: []*pb.TracerPayload{{
			ContainerID:     "abc123",
			LanguageName:    "python",
			LanguageVersion: "3.12.1",
			TracerVersion:   "2.9.0",
			RuntimeID:       "0f4c-runtime",
			Env:             "prod",
			Hostname:        "pod-1",
			AppVersion:      "1.4.0",
			Tags:            map[string]string{"team": "payments"},
			Chunks: []*pb.TraceChunk{{
				Priority:     1,
				Origin:       "lambda",
				DroppedTrace: false,
				Tags:         map[string]string{"_dd.p.dm": "-1"},
				Spans: []*pb.Span{
					{
						Service: "checkout", Name: "flask.request", Resource: "GET /pay",
						Type: "web", TraceID: 111, SpanID: 222, ParentID: 0,
						Start: 1_700_000_000_000_000_123, Duration: 5_000_000, Error: 0,
						Meta:    map[string]string{"span.kind": "server", "_dd.p.tid": "67890abcdef01234"},
						Metrics: map[string]float64{"_sampling_priority_v1": 1},
					},
					{
						Service: "checkout", Name: "postgres.query", Resource: "SELECT 1",
						Type: "db", TraceID: 111, SpanID: 333, ParentID: 222,
						Start: 1_700_000_000_001_000_000, Duration: 900_000, Error: 1,
					},
				},
			}},
		}},
	}

	rows := traceSpanRows("t1", traceTestTime, payload)
	if len(rows) != 2 {
		t.Fatalf("rows: got %d, want 2 — one per span", len(rows))
	}

	for i, r := range rows {
		if r.TenantID != "t1" {
			t.Errorf("row %d tenant: got %q", i, r.TenantID)
		}
		if r.WireFormat != "v04" {
			t.Errorf("row %d wire_format: got %q, want v04", i, r.WireFormat)
		}
		if r.AgentHostname != "ip-10-0-0-1" || r.AgentVersion != "7.58.2" {
			t.Errorf("row %d lost the agent envelope: %q %q", i, r.AgentHostname, r.AgentVersion)
		}
		if r.RareSamplerEnabled != 1 || r.TargetTPS != 10 {
			t.Errorf("row %d lost the sampler settings: rare=%d target=%g", i, r.RareSamplerEnabled, r.TargetTPS)
		}
		if r.Language != "python" || r.RuntimeID != "0f4c-runtime" || r.ContainerID != "abc123" {
			t.Errorf("row %d lost the tracer identity: %q %q %q", i, r.Language, r.RuntimeID, r.ContainerID)
		}
		if r.Priority != 1 || r.Origin != "lambda" {
			t.Errorf("row %d lost the chunk's sampling decision: %d %q", i, r.Priority, r.Origin)
		}
		if got := r.ChunkTags["_dd.p.dm"]; got != "-1" {
			t.Errorf("row %d chunk tags: got %v", i, r.ChunkTags)
		}
		// ContainerDebug was absent, so every column must be NULL rather than
		// a zero that reads as "the lookup was instant".
		if r.ContainerDebugError != nil || r.ContainerDebugLatencyMs != nil || r.ContainerDebugWasBuffered != nil {
			t.Errorf("row %d invented a container debug record out of an absent one", i)
		}
	}

	root, child := rows[0], rows[1]
	if root.ParentID != 0 {
		t.Errorf("root parent_id: got %d, want 0 — 0 means root and is kept as it came", root.ParentID)
	}
	if child.ParentID != 222 {
		t.Errorf("child parent_id: got %d, want 222", child.ParentID)
	}
	// The upper half of a 128-bit trace id has no struct field in v0.4: it is
	// hex in meta["_dd.p.tid"]. Losing it would make a trace unfindable by the
	// id people paste from a URL.
	if root.TraceIDHigh != 0x67890abcdef01234 {
		t.Errorf("trace_id_high: got %x, want 67890abcdef01234 — parsed from meta[_dd.p.tid]", root.TraceIDHigh)
	}
	if child.TraceIDHigh != 0 {
		t.Errorf("child trace_id_high: got %x, want 0 — the meta key was absent", child.TraceIDHigh)
	}
	if root.Kind != "server" {
		t.Errorf("kind: got %q, want server — v0.4 carries it in meta[span.kind]", root.Kind)
	}
	if !root.Start.Equal(time.Unix(0, 1_700_000_000_000_000_123).UTC()) {
		t.Errorf("start: got %v — nanoseconds must survive", root.Start)
	}
	if child.Error != 1 {
		t.Errorf("error flag: got %d, want 1", child.Error)
	}
	// The meta map is stored exactly as it arrived, _dd.p.tid included: the
	// parsed column is a convenience, not a replacement.
	if root.Meta["_dd.p.tid"] != "67890abcdef01234" {
		t.Errorf("meta lost _dd.p.tid after it was parsed out: %v", root.Meta)
	}
}

// A payload with nil chunks, nil spans and a nil tracer payload in the slices:
// protobuf permits them and one real malformed sender must not take the whole
// batch down with a panic.
func TestTraceSpanRowsToleratesNils(t *testing.T) {
	payload := &pb.AgentPayload{
		TracerPayloads: []*pb.TracerPayload{
			nil,
			{Chunks: []*pb.TraceChunk{nil, {Spans: []*pb.Span{nil, {Service: "s", SpanID: 1}}}}},
		},
	}

	rows := traceSpanRows("t1", traceTestTime, payload)
	if len(rows) != 1 {
		t.Fatalf("rows: got %d, want 1 — the nils are skipped, the real span is not", len(rows))
	}
	if rows[0].Service != "s" {
		t.Errorf("service: got %q", rows[0].Service)
	}
	if traceSpanRows("t1", traceTestTime, nil) != nil {
		t.Error("a nil payload should produce no rows")
	}
}

// ContainerDebug is set only when the agent had trouble resolving container
// tags. When it IS set, every field has to survive — including a zero latency,
// which is a measurement, not an absence.
func TestTraceSpanRowsKeepsContainerDebug(t *testing.T) {
	payload := &pb.AgentPayload{TracerPayloads: []*pb.TracerPayload{{
		ContainerDebug: &pb.ContainerDebug{
			Error:                "context deadline exceeded",
			LatencyMs:            0,
			WasBuffered:          true,
			BufferMs:             250,
			BufferEvictionReason: "timeout",
		},
		Chunks: []*pb.TraceChunk{{Spans: []*pb.Span{{SpanID: 1}}}},
	}}}

	rows := traceSpanRows("t1", traceTestTime, payload)
	r := rows[0]
	if r.ContainerDebugError == nil || *r.ContainerDebugError != "context deadline exceeded" {
		t.Fatalf("container_debug_error: got %v", r.ContainerDebugError)
	}
	if r.ContainerDebugLatencyMs == nil || *r.ContainerDebugLatencyMs != 0 {
		t.Errorf("latency 0 must be stored as 0, not NULL: got %v", r.ContainerDebugLatencyMs)
	}
	if r.ContainerDebugWasBuffered == nil || *r.ContainerDebugWasBuffered != 1 {
		t.Errorf("was_buffered: got %v", r.ContainerDebugWasBuffered)
	}
	if r.ContainerDebugBufferEvictionReason == nil || *r.ContainerDebugBufferEvictionReason != "timeout" {
		t.Errorf("eviction reason: got %v", r.ContainerDebugBufferEvictionReason)
	}
}

// Span links and span events are repeated fields, so their ORDER is part of
// the data. The parallel arrays must line up index for index, and the event
// attributes must keep the type the tracer declared — an int attribute that
// comes back as a string is a different fact.
func TestTraceSpanRowsKeepsLinksAndEventsInOrder(t *testing.T) {
	sp := &pb.Span{
		SpanID: 1,
		SpanLinks: []*pb.SpanLink{
			{TraceID: 10, TraceIDHigh: 99, SpanID: 11, Tracestate: "dd=s:1", Flags: 0x80000001,
				Attributes: map[string]string{"rel": "follows"}},
			{TraceID: 20, SpanID: 21},
		},
		SpanEvents: []*pb.SpanEvent{
			{TimeUnixNano: 1_700_000_000_000_000_000, Name: "exception", Attributes: map[string]*pb.AttributeAnyValue{
				"code":    {Type: pb.AttributeAnyValue_INT_VALUE, IntValue: 42},
				"message": {Type: pb.AttributeAnyValue_STRING_VALUE, StringValue: "boom"},
				"ratio":   {Type: pb.AttributeAnyValue_DOUBLE_VALUE, DoubleValue: 0.5},
				"retry":   {Type: pb.AttributeAnyValue_BOOL_VALUE, BoolValue: true},
				"frames": {Type: pb.AttributeAnyValue_ARRAY_VALUE, ArrayValue: &pb.AttributeArray{
					Values: []*pb.AttributeArrayValue{
						{Type: pb.AttributeArrayValue_STRING_VALUE, StringValue: "a"},
						{Type: pb.AttributeArrayValue_INT_VALUE, IntValue: 7},
					}}},
			}},
			{TimeUnixNano: 1_700_000_000_000_000_001, Name: "log"},
		},
	}
	payload := &pb.AgentPayload{TracerPayloads: []*pb.TracerPayload{{
		Chunks: []*pb.TraceChunk{{Spans: []*pb.Span{sp}}},
	}}}

	r := traceSpanRows("t1", traceTestTime, payload)[0]

	if want := []uint64{10, 20}; !reflect.DeepEqual(r.LinkTraceID, want) {
		t.Errorf("link_trace_id: got %v, want %v — wire order is the data", r.LinkTraceID, want)
	}
	if want := []uint64{99, 0}; !reflect.DeepEqual(r.LinkTraceIDHigh, want) {
		t.Errorf("link_trace_id_high: got %v, want %v", r.LinkTraceIDHigh, want)
	}
	if want := []uint32{0x80000001, 0}; !reflect.DeepEqual(r.LinkFlags, want) {
		t.Errorf("link_flags: got %v, want %v — the presence bit must survive", r.LinkFlags, want)
	}
	if got, want := r.LinkAttributes[0], `{"rel":{"string":"follows"}}`; got != want {
		t.Errorf("link attributes: got %s, want %s", got, want)
	}
	if r.LinkAttributes[1] != "" {
		t.Errorf("a link with no attributes should leave the column empty, got %q", r.LinkAttributes[1])
	}

	if want := []string{"exception", "log"}; !reflect.DeepEqual(r.EventName, want) {
		t.Errorf("event_name: got %v, want %v", r.EventName, want)
	}
	if !r.EventTime[1].Equal(time.Unix(0, 1_700_000_000_000_000_001).UTC()) {
		t.Errorf("event_time[1]: got %v — nanoseconds must survive", r.EventTime[1])
	}

	var attrs map[string]map[string]any
	if err := json.Unmarshal([]byte(r.EventAttributes[0]), &attrs); err != nil {
		t.Fatalf("event attributes are not JSON: %v (%s)", err, r.EventAttributes[0])
	}
	// The tag names the wire type. 42 as {"int":42} and 0.5 as {"double":0.5}
	// stay distinguishable, which a bare JSON number would not.
	if _, ok := attrs["code"]["int"]; !ok {
		t.Errorf("code lost its INT_VALUE tag: %v", attrs["code"])
	}
	if _, ok := attrs["ratio"]["double"]; !ok {
		t.Errorf("ratio lost its DOUBLE_VALUE tag: %v", attrs["ratio"])
	}
	if v, ok := attrs["retry"]["bool"].(bool); !ok || !v {
		t.Errorf("retry lost its BOOL_VALUE: %v", attrs["retry"])
	}
	arr, ok := attrs["frames"]["array"].([]any)
	if !ok || len(arr) != 2 {
		t.Fatalf("frames lost its array: %v", attrs["frames"])
	}
}

// MetaStruct values are msgpack blobs, not text. They must reach the table
// byte for byte — an AppSec event that lost a byte is not an AppSec event.
func TestTraceSpanRowsKeepsMetaStructBytes(t *testing.T) {
	raw := []byte{0x81, 0xa3, 'k', 'e', 'y', 0x00, 0xff}
	payload := &pb.AgentPayload{TracerPayloads: []*pb.TracerPayload{{
		Chunks: []*pb.TraceChunk{{Spans: []*pb.Span{{
			SpanID: 1, MetaStruct: map[string][]byte{"appsec": raw},
		}}}},
	}}}

	r := traceSpanRows("t1", traceTestTime, payload)[0]
	if got := r.MetaStruct["appsec"]; got != string(raw) {
		t.Errorf("meta_struct: got %q (%d bytes), want the %d bytes as they arrived", got, len(got), len(raw))
	}
}

// ---------------------------------------------------------------------------
// idx (v1.0)
// ---------------------------------------------------------------------------

// The v1.0 format nobody has captured yet. Every string is an index into a
// per-payload table, the trace id moved to the chunk as 16 raw bytes, and
// attributes became a typed union — so this is the test that says the decoder
// was written against the generated types rather than guessed at.
func TestTraceSpanRowsResolvesIdxPayload(t *testing.T) {
	// Index 0 is reserved for the empty string; a Ref of 0 means "unset".
	strs := []string{"", "checkout", "flask.request", "GET /pay", "web", "python",
		"3.12.1", "2.9.0", "runtime-1", "prod", "pod-1", "1.4.0", "http.method",
		"GET", "retries", "payload", "tags", "a", "dd=s:1", "exception"}

	traceID := []byte{
		0x67, 0x89, 0x0a, 0xbc, 0xde, 0xf0, 0x12, 0x34,
		0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x6f,
	}

	payload := &pb.AgentPayload{
		HostName: "agent-host",
		IdxTracerPayloads: []*idx.TracerPayload{{
			Strings:            strs,
			LanguageNameRef:    5,
			LanguageVersionRef: 6,
			TracerVersionRef:   7,
			RuntimeIDRef:       8,
			EnvRef:             9,
			HostnameRef:        10,
			AppVersionRef:      11,
			Attributes: map[uint32]*idx.AnyValue{
				16: {Value: &idx.AnyValue_KeyValueList{KeyValueList: &idx.KeyValueList{
					KeyValues: []*idx.KeyValue{{
						Key:   12,
						Value: &idx.AnyValue{Value: &idx.AnyValue_StringValueRef{StringValueRef: 13}},
					}},
				}}},
			},
			Chunks: []*idx.TraceChunk{{
				Priority:          2,
				TraceID:           traceID,
				SamplingMechanism: 4,
				Spans: []*idx.Span{{
					ServiceRef:  1,
					NameRef:     2,
					ResourceRef: 3,
					TypeRef:     4,
					SpanID:      333,
					ParentID:    222,
					Start:       1_700_000_000_000_000_123,
					Duration:    5_000_000,
					Error:       true,
					Kind:        idx.SpanKind_SPAN_KIND_SERVER,
					EnvRef:      9,
					VersionRef:  11,
					Attributes: map[uint32]*idx.AnyValue{
						12: {Value: &idx.AnyValue_StringValueRef{StringValueRef: 13}},
						14: {Value: &idx.AnyValue_IntValue{IntValue: 3}},
						15: {Value: &idx.AnyValue_BytesValue{BytesValue: []byte{0x00, 0x01}}},
						16: {Value: &idx.AnyValue_ArrayValue{ArrayValue: &idx.ArrayValue{
							Values: []*idx.AnyValue{{Value: &idx.AnyValue_StringValueRef{StringValueRef: 17}}},
						}}},
					},
					Links: []*idx.SpanLink{{
						TraceID:       traceID,
						SpanID:        11,
						TracestateRef: 18,
						Flags:         1,
					}},
					Events: []*idx.SpanEvent{{Time: 1_700_000_000_000_000_000, NameRef: 19}},
				}},
			}},
		}},
	}

	rows := traceSpanRows("t1", traceTestTime, payload)
	if len(rows) != 1 {
		t.Fatalf("rows: got %d, want 1", len(rows))
	}
	r := rows[0]

	if r.WireFormat != "idx" {
		t.Errorf("wire_format: got %q, want idx", r.WireFormat)
	}
	if r.AgentHostname != "agent-host" {
		t.Errorf("the agent envelope is shared by both formats, got %q", r.AgentHostname)
	}
	if r.Service != "checkout" || r.Name != "flask.request" || r.Resource != "GET /pay" || r.SpanType != "web" {
		t.Errorf("string refs unresolved: %q %q %q %q", r.Service, r.Name, r.Resource, r.SpanType)
	}
	// Ref 0 is "unset", not "index 0 of real content".
	if r.ContainerID != "" || r.Component != "" {
		t.Errorf("a Ref of 0 must resolve to empty, got %q / %q", r.ContainerID, r.Component)
	}
	if r.TraceIDHigh != 0x67890abcdef01234 || r.TraceID != 0x6f {
		t.Errorf("trace id bytes split wrong: high=%x low=%x", r.TraceIDHigh, r.TraceID)
	}
	if r.SamplingMechanism != 4 {
		t.Errorf("sampling_mechanism: got %d, want 4 — idx only, no v0.4 equivalent", r.SamplingMechanism)
	}
	if r.Kind != "server" {
		t.Errorf("kind: got %q, want server", r.Kind)
	}
	if r.Error != 1 {
		t.Errorf("error: got %d, want 1 — idx ships a bool, the column keeps v0.4's width", r.Error)
	}
	if r.SpanEnv != "prod" || r.SpanVersion != "1.4.0" {
		t.Errorf("per-span overrides lost: %q %q", r.SpanEnv, r.SpanVersion)
	}
	// Scalars are projected into the v0.4 maps so one query reads both formats.
	if r.Meta["http.method"] != "GET" {
		t.Errorf("string attribute did not reach meta: %v", r.Meta)
	}
	if r.Metrics["retries"] != 3 {
		t.Errorf("int attribute did not reach metrics: %v", r.Metrics)
	}
	if r.MetaStruct["payload"] != string([]byte{0x00, 0x01}) {
		t.Errorf("bytes attribute did not reach meta_struct: %q", r.MetaStruct["payload"])
	}
	// ...and the typed form is kept whole, which is the only place the array
	// and the bytes survive as what they are.
	var attrs map[string]map[string]any
	if err := json.Unmarshal([]byte(r.AttributesJSON), &attrs); err != nil {
		t.Fatalf("attributes_json is not JSON: %v (%s)", err, r.AttributesJSON)
	}
	if _, ok := attrs["tags"]["array"]; !ok {
		t.Errorf("the array attribute has no untagged form and must be in attributes_json: %v", attrs)
	}
	if got, ok := attrs["payload"]["bytes"].(string); !ok || got != "AAE=" {
		t.Errorf("bytes attribute in attributes_json: got %v, want base64 AAE=", attrs["payload"])
	}
	if r.TracerAttributesJSON == "" {
		t.Error("payload-level typed attributes were dropped")
	}
	if r.TracerTags["tags"] != "" {
		t.Errorf("a non-scalar attribute must not be flattened into tracer_tags: %v", r.TracerTags)
	}

	if len(r.LinkTraceID) != 1 || r.LinkTraceIDHigh[0] != 0x67890abcdef01234 || r.LinkTraceID[0] != 0x6f {
		t.Errorf("link trace id bytes split wrong: %v / %v", r.LinkTraceIDHigh, r.LinkTraceID)
	}
	if r.LinkTracestate[0] != "dd=s:1" {
		t.Errorf("link tracestate ref unresolved: %q", r.LinkTracestate[0])
	}
	if len(r.EventName) != 1 || r.EventName[0] != "exception" {
		t.Errorf("event name ref unresolved: %v", r.EventName)
	}
}

// A ref past the end of the string table is a malformed payload. It must
// resolve to empty rather than panic: one bad ref cannot be allowed to cost
// the batch around it.
func TestTraceStringsOutOfRange(t *testing.T) {
	s := traceStrings{"", "a"}
	cases := []struct {
		name string
		ref  uint32
		want string
	}{
		{"zero is unset", 0, ""},
		{"in range", 1, "a"},
		{"past the end", 99, ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := s.at(tc.ref); got != tc.want {
				t.Errorf("at(%d): got %q, want %q", tc.ref, got, tc.want)
			}
		})
	}
}

// The 128-bit trace id arrives as bytes whose length the schema does not
// control. Short ids are the ones a hand-written client produces.
func TestTraceSplitID(t *testing.T) {
	cases := []struct {
		name      string
		in        []byte
		high, low uint64
	}{
		{"empty", nil, 0, 0},
		{"full 16 bytes", []byte{1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2}, 1 << 56, 2},
		{"short is left-padded", []byte{0xff}, 0, 0xff},
		{"long is truncated from the left", append([]byte{0xaa, 0xbb}, make([]byte, 16)...), 0, 0},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			high, low := traceSplitID(tc.in)
			if high != tc.high || low != tc.low {
				t.Errorf("got high=%x low=%x, want high=%x low=%x", high, low, tc.high, tc.low)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// apm_stats
// ---------------------------------------------------------------------------

// The complete counts, which is what every APM page is built from. This pins
// the three-state IsTraceRoot, the one genuine tag multiset in the trace
// protocol, and that the sketch bytes are kept whatever they decode to.
func TestAPMStatRowsDenormalizesAndKeepsSketches(t *testing.T) {
	okBytes := traceTestSketch(t, 1e6, 2e6, 3e6)

	payload := &pb.StatsPayload{
		AgentHostname:  "agent-1",
		AgentEnv:       "prod",
		AgentVersion:   "7.58.2",
		ClientComputed: true,
		SplitPayload:   true,
		Stats: []*pb.ClientStatsPayload{{
			Hostname:      "pod-1",
			Env:           "prod",
			Version:       "1.4.0",
			Lang:          "python",
			TracerVersion: "2.9.0",
			RuntimeID:     "runtime-1",
			Sequence:      7,
			Service:       "checkout",
			ContainerID:   "abc123",
			// A pod behind two services sends kube_service twice. A plain map
			// would keep one of them.
			Tags:            []string{"kube_service:a", "kube_service:b", "bare"},
			GitCommitSha:    "deadbeef",
			ImageTag:        "v1.4.0",
			ProcessTags:     "entrypoint:gunicorn",
			ProcessTagsHash: 18446744073709551615, // max uint64: must not go through a float
			Stats: []*pb.ClientStatsBucket{{
				Start:          1_700_000_000_000_000_000,
				Duration:       10_000_000_000,
				AgentTimeShift: -500,
				Stats: []*pb.ClientGroupedStats{
					{
						Service: "checkout", Name: "flask.request", Resource: "GET /pay",
						HTTPStatusCode: 200, Type: "web", Hits: 120, Errors: 3,
						Duration: 900_000_000, TopLevelHits: 120, SpanKind: "server",
						IsTraceRoot: pb.Trilean_TRUE, HTTPMethod: "GET", HTTPEndpoint: "/pay",
						PeerTags:             []string{"db.hostname:pg-1"},
						AdditionalMetricTags: []string{"region:eu"},
						OkSummary:            okBytes,
						ErrorSummary:         []byte("not a sketch"),
					},
					{
						Service: "checkout", Name: "postgres.query",
						IsTraceRoot: pb.Trilean_NOT_SET,
					},
				},
			}},
		}},
	}

	rows := apmStatRows("t1", traceTestTime, payload, nil, nil)
	if len(rows) != 2 {
		t.Fatalf("rows: got %d, want 2 — one per grouped stat", len(rows))
	}
	r := rows[0]

	if r.AgentVersion != "7.58.2" || r.ClientComputed != 1 || r.SplitPayload != 1 {
		t.Errorf("payload level lost: %q %d %d", r.AgentVersion, r.ClientComputed, r.SplitPayload)
	}
	if r.ClientRuntimeID != "runtime-1" || r.ClientSequence != 7 {
		t.Errorf("client level lost: %q %d", r.ClientRuntimeID, r.ClientSequence)
	}
	if r.ClientProcessTagsHash != 18446744073709551615 {
		t.Errorf("process_tags_hash: got %d — a uint64 must never pass through a float64", r.ClientProcessTagsHash)
	}
	if want := []string{"a", "b"}; !reflect.DeepEqual(r.ClientTags["kube_service"], want) {
		t.Errorf("client tags: got %v, want kube_service=%v — tags are a multiset", r.ClientTags, want)
	}
	if got, ok := r.ClientTags["bare"]; !ok || !reflect.DeepEqual(got, []string{""}) {
		t.Errorf("a bare tag must keep its key: got %v", r.ClientTags)
	}
	if !r.BucketStart.Equal(time.Unix(0, 1_700_000_000_000_000_000).UTC()) {
		t.Errorf("bucket_start: got %v", r.BucketStart)
	}
	if r.AgentTimeShiftNs != -500 {
		t.Errorf("agent_time_shift_ns: got %d, want -500 — it is signed", r.AgentTimeShiftNs)
	}
	if r.IsTraceRoot != "true" || rows[1].IsTraceRoot != "not_set" {
		t.Errorf("is_trace_root: got %q and %q — NOT_SET and FALSE are different answers",
			r.IsTraceRoot, rows[1].IsTraceRoot)
	}

	if r.OkSummary.State != storage.SketchOK {
		t.Fatalf("ok_summary state: got %q, want ok", r.OkSummary.State)
	}
	if r.OkSummary.Raw != string(okBytes) {
		t.Error("the raw sketch bytes must be kept: they are the only form that merges")
	}
	if r.OkSummary.Count == nil || *r.OkSummary.Count != 3 {
		t.Errorf("ok_count: got %v, want 3", r.OkSummary.Count)
	}
	if len(r.OkSummary.BinKeys) == 0 || len(r.OkSummary.BinKeys) != len(r.OkSummary.BinCounts) {
		t.Errorf("bins must come in parallel arrays: %d keys, %d counts",
			len(r.OkSummary.BinKeys), len(r.OkSummary.BinCounts))
	}
	// Bytes that are not a DDSketch are a real signal, not a reason to store
	// nothing: the state says so and the bytes stay.
	if r.ErrorSummary.State != storage.SketchUndecodable {
		t.Errorf("error_summary state: got %q, want undecodable", r.ErrorSummary.State)
	}
	if r.ErrorSummary.Raw != "not a sketch" {
		t.Errorf("undecodable bytes were dropped: %q", r.ErrorSummary.Raw)
	}
	if r.ErrorSummary.Count != nil {
		t.Error("an undecodable summary must not invent a count")
	}
	// A group with no summary at all is the third state.
	if rows[1].OkSummary.State != storage.SketchAbsent || rows[1].OkSummary.Raw != "" {
		t.Errorf("absent summary: got state %q", rows[1].OkSummary.State)
	}
}

// An empty DDSketch is a valid sketch that measured nothing — a different fact
// from "there was no sketch", and the table keeps them apart.
func TestStatsSummaryEmptySketch(t *testing.T) {
	sk, err := ddsketch.NewDefaultDDSketch(0.01)
	if err != nil {
		t.Fatalf("new sketch: %v", err)
	}
	raw, err := proto.Marshal(sk.ToProto())
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	s, _, err := statsSummary(raw)
	if err != nil {
		t.Fatalf("statsSummary: %v", err)
	}
	if s.State != storage.SketchEmpty {
		t.Errorf("state: got %q, want empty", s.State)
	}
	if s.Count != nil {
		t.Error("an empty sketch must not report a count")
	}
	if s.Raw != string(raw) {
		t.Error("the bytes must be kept even when the sketch is empty")
	}
}

func traceTestSketch(t *testing.T, values ...float64) []byte {
	t.Helper()
	sk, err := ddsketch.NewDefaultDDSketch(0.01)
	if err != nil {
		t.Fatalf("new sketch: %v", err)
	}
	for _, v := range values {
		if err := sk.Add(v); err != nil {
			t.Fatalf("add %g: %v", v, err)
		}
	}
	raw, err := proto.Marshal(sk.ToProto())
	if err != nil {
		t.Fatalf("marshal sketch: %v", err)
	}
	return raw
}

// ---------------------------------------------------------------------------
// Data Streams Monitoring
// ---------------------------------------------------------------------------

// A pipeline_stats payload encoded exactly the way dd-trace-go's msgp codec
// does it — a map whose keys are the Go field names — and decoded back. The
// unknown key is the point of the test as much as the known ones: dd-trace-go
// adds fields to this payload with no version negotiation.
func TestDSMDecodeStatsPayload(t *testing.T) {
	pathway := traceTestSketch(t, 0.2, 0.3)

	var buf bytes.Buffer
	w := msgp.NewWriter(&buf)
	must := func(err error) {
		t.Helper()
		if err != nil {
			t.Fatalf("encode: %v", err)
		}
	}
	must(w.WriteMapHeader(9))
	must(w.WriteString("Env"))
	must(w.WriteString("prod"))
	must(w.WriteString("Service"))
	must(w.WriteString("orders"))
	must(w.WriteString("TracerVersion"))
	must(w.WriteString("2.9.0"))
	must(w.WriteString("Lang"))
	must(w.WriteString("go"))
	must(w.WriteString("Version"))
	must(w.WriteString("1.4.0"))
	must(w.WriteString("ProcessTags"))
	must(w.WriteArrayHeader(2))
	must(w.WriteString("entrypoint:worker"))
	must(w.WriteString("cwd:/srv"))
	must(w.WriteString("ProductMask"))
	must(w.WriteUint64(3))
	// A field this decoder has never heard of.
	must(w.WriteString("SomethingNew"))
	must(w.WriteString("hello"))
	must(w.WriteString("Stats"))
	must(w.WriteArrayHeader(1))
	{
		must(w.WriteMapHeader(5))
		must(w.WriteString("Start"))
		must(w.WriteUint64(1_700_000_000_000_000_000))
		must(w.WriteString("Duration"))
		must(w.WriteUint64(10_000_000_000))
		must(w.WriteString("Stats"))
		must(w.WriteArrayHeader(1))
		{
			must(w.WriteMapHeader(7))
			must(w.WriteString("EdgeTags"))
			must(w.WriteArrayHeader(2))
			must(w.WriteString("type:kafka"))
			must(w.WriteString("topic:orders"))
			must(w.WriteString("Hash"))
			must(w.WriteUint64(18446744073709551615))
			must(w.WriteString("ParentHash"))
			must(w.WriteUint64(42))
			must(w.WriteString("PathwayLatency"))
			must(w.WriteBytes(pathway))
			must(w.WriteString("EdgeLatency"))
			must(w.WriteNil())
			must(w.WriteString("PayloadSize"))
			must(w.WriteBytes([]byte("garbage")))
			must(w.WriteString("TimestampType"))
			must(w.WriteString("current"))
		}
		must(w.WriteString("Backlogs"))
		must(w.WriteArrayHeader(1))
		{
			must(w.WriteMapHeader(2))
			must(w.WriteString("Tags"))
			must(w.WriteArrayHeader(2))
			must(w.WriteString("type:kafka_commit"))
			must(w.WriteString("partition:3"))
			must(w.WriteString("Value"))
			must(w.WriteInt64(-17))
		}
		must(w.WriteString("Transactions"))
		must(w.WriteBytes([]byte{0x01, 0x02, 0x03}))
	}
	must(w.Flush())

	p, err := dsmDecodeStatsPayload(buf.Bytes())
	if err != nil {
		t.Fatalf("decode: %v", err)
	}

	if p.Env != "prod" || p.Service != "orders" || p.Lang != "go" || p.ProductMask != 3 {
		t.Errorf("payload fields: %+v", p)
	}
	if want := []string{"entrypoint:worker", "cwd:/srv"}; !reflect.DeepEqual(p.ProcessTags, want) {
		t.Errorf("process_tags: got %v, want %v", p.ProcessTags, want)
	}
	if got := p.Unknown["SomethingNew"]; got != "hello" {
		t.Errorf("an unknown field must be kept VALUE AND ALL: got %v", p.Unknown)
	}
	if len(p.Buckets) != 1 || len(p.Buckets[0].Points) != 1 || len(p.Buckets[0].Backlogs) != 1 {
		t.Fatalf("buckets: %+v", p.Buckets)
	}

	sp := p.Buckets[0].Points[0]
	if sp.Hash != 18446744073709551615 {
		t.Errorf("hash: got %d — a 64-bit hash must not pass through a float", sp.Hash)
	}
	if want := []string{"type:kafka", "topic:orders"}; !reflect.DeepEqual(sp.EdgeTags, want) {
		t.Errorf("edge_tags: got %v, want %v", sp.EdgeTags, want)
	}
	if !bytes.Equal(sp.PathwayLatency, pathway) {
		t.Error("pathway latency bytes did not survive the round trip")
	}
	if sp.EdgeLatency != nil {
		t.Errorf("a nil in place of a sketch must stay nil, got %v", sp.EdgeLatency)
	}
	if p.Buckets[0].Backlogs[0].Value != -17 {
		t.Errorf("backlog value: got %d, want -17 — it is signed", p.Buckets[0].Backlogs[0].Value)
	}
	if !bytes.Equal(p.Buckets[0].Transactions, []byte{1, 2, 3}) {
		t.Errorf("transactions blob: got %v", p.Buckets[0].Transactions)
	}

	// ...and through to the rows.
	points, backlogs, buckets := dsmRows("t1", traceTestTime, p, dsmHeaders{Via: "trace-agent"})
	if len(points) != 1 || len(backlogs) != 1 || len(buckets) != 1 {
		t.Fatalf("rows: %d points, %d backlogs, %d bucket blobs", len(points), len(backlogs), len(buckets))
	}
	if points[0].Service != "orders" || points[0].Via != "trace-agent" {
		t.Errorf("payload level or proxy header lost: %+v", points[0])
	}
	if points[0].PathwayLatency.State != storage.SketchOK {
		t.Errorf("pathway latency state: got %q", points[0].PathwayLatency.State)
	}
	if points[0].EdgeLatency.State != storage.SketchAbsent {
		t.Errorf("edge latency state: got %q, want absent", points[0].EdgeLatency.State)
	}
	if points[0].PayloadSize.State != storage.SketchUndecodable || points[0].PayloadSize.Raw != "garbage" {
		t.Errorf("payload size state/raw: %q / %q", points[0].PayloadSize.State, points[0].PayloadSize.Raw)
	}
	if want := []string{"payload.SomethingNew"}; !reflect.DeepEqual(points[0].UnknownKeys, want) {
		t.Errorf("unknown keys: got %v, want %v", points[0].UnknownKeys, want)
	}
	if points[0].UnknownJSON != `{"payload.SomethingNew":"hello"}` {
		t.Errorf("unknown json: got %s", points[0].UnknownJSON)
	}
	if backlogs[0].Value != -17 || len(backlogs[0].Tags) != 2 {
		t.Errorf("backlog row: %+v", backlogs[0])
	}
	if buckets[0].Transactions != string([]byte{1, 2, 3}) {
		t.Errorf("bucket transactions row: %q", buckets[0].Transactions)
	}
}

// A body that is not a msgpack StatsPayload must fail cleanly so the handler
// can hand it to raw_payloads, never panic and never half-decode.
func TestDSMDecodeStatsPayloadRejectsGarbage(t *testing.T) {
	cases := []struct {
		name string
		body []byte
	}{
		{"empty", nil},
		{"json", []byte(`{"Env":"prod"}`)},
		{"truncated map", []byte{0x81}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if _, err := dsmDecodeStatsPayload(tc.body); err == nil {
				t.Error("want an error, got none")
			}
		})
	}
}

// The data_streams_messages track has no published schema, so the element is
// kept as it came and only its top-level key names are extracted — enough to
// answer "what does this send?" with a GROUP BY.
func TestDSMMessageRows(t *testing.T) {
	msgs := []json.RawMessage{
		json.RawMessage(`{"b":1,"a":{"nested":true}}`),
		json.RawMessage(`"just a string"`),
	}

	rows := dsmMessageRows("t1", traceTestTime, msgs, "browser", "5.0.0", "deflate")
	if len(rows) != 2 {
		t.Fatalf("rows: got %d, want 2", len(rows))
	}
	if rows[0].Position != 0 || rows[1].Position != 1 {
		t.Errorf("batch order is the only ordering these have: got %d, %d", rows[0].Position, rows[1].Position)
	}
	if rows[0].Message != `{"b":1,"a":{"nested":true}}` {
		t.Errorf("the element must be kept verbatim: %s", rows[0].Message)
	}
	if want := []string{"a", "b"}; !reflect.DeepEqual(rows[0].Keys, want) {
		t.Errorf("keys: got %v, want %v (sorted)", rows[0].Keys, want)
	}
	if rows[1].Keys != nil {
		t.Errorf("a non-object element has no top-level keys, got %v", rows[1].Keys)
	}
	if rows[0].DDEVPOrigin != "browser" || rows[0].DDEVPOriginVersion != "5.0.0" {
		t.Errorf("the EVP origin headers are this track's only schema hint: %+v", rows[0])
	}
}

// ---------------------------------------------------------------------------
// Handlers, through the real engine
// ---------------------------------------------------------------------------

// The agent parses the traces response body — rate_by_service feeds its
// priority sampler — so the status and the shape are a contract, not a
// convention. This pins both, plus that the spans reached the right writer
// with the tenant from the key.
func TestHandleTracesStoresSpansAndKeepsResponse(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeTrace)

	payload := &pb.AgentPayload{
		HostName: "agent-1",
		TracerPayloads: []*pb.TracerPayload{{
			LanguageName: "go",
			Chunks: []*pb.TraceChunk{{Spans: []*pb.Span{
				{Service: "a", Name: "one", SpanID: 1, TraceID: 7, Start: 1_700_000_000_000_000_000},
				{Service: "a", Name: "two", SpanID: 2, ParentID: 1, TraceID: 7},
			}}},
		}},
	}
	body, err := payload.MarshalVT()
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	w := post(t, e, "/api/v0.2/traces", body)
	if w.Code != http.StatusOK {
		t.Fatalf("status: got %d, want 200", w.Code)
	}
	if got := w.Body.String(); got != `{"rate_by_service":{}}` {
		t.Errorf("body: got %s, want {\"rate_by_service\":{}} — the agent parses it", got)
	}

	if msgs := node.SentTo(storage.SpansWriter); len(msgs) != 1 {
		t.Fatalf("messages to %s: got %d, want 1 — one batch per request", storage.SpansWriter, len(msgs))
	}
	rows := Rows[storage.SpanRow](node)
	if len(rows) != 2 {
		t.Fatalf("stored spans: got %d, want 2", len(rows))
	}
	for i, r := range rows {
		if r.TenantID != testTenant {
			t.Errorf("row %d tenant: got %q, want %q", i, r.TenantID, testTenant)
		}
	}
	if rows[0].Name != "one" || rows[1].Name != "two" {
		t.Errorf("span order must survive: got %q, %q", rows[0].Name, rows[1].Name)
	}
}

// A protobuf that does not parse is either a wire version we have not seen or
// a bug. Both need the bytes, so they go to raw_payloads — and the agent still
// gets the 200 it expects, because failing it would only make the agent retry
// the same undecodable payload forever.
func TestHandleTracesStoresUndecodableBodyRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeTrace)

	w := post(t, e, "/api/v0.2/traces", []byte("this is not protobuf at all"))
	if w.Code != http.StatusOK {
		t.Fatalf("status: got %d, want 200 even on a decode failure", w.Code)
	}

	if n := len(Rows[storage.SpanRow](node)); n != 0 {
		t.Errorf("stored %d spans from an undecodable body, want 0", n)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 {
		t.Fatalf("raw payload rows: got %d, want 1", len(raw))
	}
	if raw[0].Intake != traceIntake || raw[0].Reason != "decode_error" {
		t.Errorf("intake/reason: got %q/%q, want trace/decode_error", raw[0].Intake, raw[0].Reason)
	}
	if raw[0].Body != "this is not protobuf at all" {
		t.Errorf("the body must be kept as it came: %q", raw[0].Body)
	}
}

// The agent's startup connectivity sweep sends an empty body with this header.
// It must not reach a decoder and must not produce a raw_payloads row —
// otherwise every agent restart fills the debugging table.
func TestHandleTracesIgnoresDiagnoseProbe(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeTrace)

	req := httptest.NewRequest(http.MethodPost, "/api/v0.2/traces", bytes.NewReader(nil))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("X-Requested-With", "datadog-agent-diagnose")
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("status: got %d, want 200", w.Code)
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("a diagnose probe stored %d messages, want 0", n)
	}
}

// The stats endpoint answers with an empty JSON object; the agent only checks
// the status. The rows are what matters here.
func TestHandleAPMStatsStoresGroupedStats(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeTrace)

	payload := &pb.StatsPayload{
		AgentHostname: "agent-1",
		Stats: []*pb.ClientStatsPayload{{
			Service: "checkout",
			Tags:    []string{"env:prod"},
			Stats: []*pb.ClientStatsBucket{{
				Start:    1_700_000_000_000_000_000,
				Duration: 10_000_000_000,
				Stats: []*pb.ClientGroupedStats{
					{Service: "checkout", Name: "flask.request", Hits: 5, IsTraceRoot: pb.Trilean_TRUE},
					{Service: "checkout", Name: "postgres.query", Hits: 9},
				},
			}},
		}},
	}
	body, err := payload.MarshalMsg(nil)
	if err != nil {
		t.Fatalf("marshal msgpack: %v", err)
	}

	w := post(t, e, "/api/v0.2/stats", body)
	if w.Code != http.StatusOK {
		t.Fatalf("status: got %d, want 200 (%s)", w.Code, w.Body.String())
	}
	if got := w.Body.String(); got != `{}` {
		t.Errorf("body: got %s, want {}", got)
	}

	rows := Rows[storage.APMStatRow](node)
	if len(rows) != 2 {
		t.Fatalf("stored rows: got %d, want 2", len(rows))
	}
	if rows[0].TenantID != testTenant || rows[0].Hits != 5 || rows[1].Hits != 9 {
		t.Errorf("rows: %+v", rows)
	}
	if rows[0].IsTraceRoot != "true" || rows[1].IsTraceRoot != "not_set" {
		t.Errorf("is_trace_root: %q, %q", rows[0].IsTraceRoot, rows[1].IsTraceRoot)
	}
	if got := rows[0].ClientTags["env"]; !reflect.DeepEqual(got, []string{"prod"}) {
		t.Errorf("client tags: got %v", rows[0].ClientTags)
	}
}

// msgpack that is not a StatsPayload — which the Android SDK from 3.14 may
// well send here — goes to raw_payloads rather than being dropped.
func TestHandleAPMStatsStoresUndecodableBodyRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeTrace)

	w := post(t, e, "/api/v0.2/stats", []byte("definitely not msgpack stats"))
	if w.Code != http.StatusOK {
		t.Fatalf("status: got %d, want 200", w.Code)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 || raw[0].Reason != "decode_error" {
		t.Fatalf("raw payload rows: %+v", raw)
	}
}

// The data_streams_messages track batches JSON. A body that is not the array
// the forwarder sends is an unexpected shape, not a decode error, and the
// distinction is what the reason column is for.
func TestHandleDataStreamsMessages(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeTrace)

	w := post(t, e, "/api/v2/data_streams_messages", []byte(`[{"a":1},{"b":2}]`))
	if w.Code != http.StatusAccepted {
		t.Fatalf("status: got %d, want 202", w.Code)
	}
	rows := Rows[storage.DSMMessageRow](node)
	if len(rows) != 2 {
		t.Fatalf("stored rows: got %d, want 2", len(rows))
	}
	if rows[0].Message != `{"a":1}` || rows[1].Position != 1 {
		t.Errorf("rows: %+v", rows)
	}

	a2, node2 := newTestServer(t)
	e2 := newTestEngine(t, a2, a2.routeTrace)
	if w := post(t, e2, "/api/v2/data_streams_messages", []byte(`{"not":"an array"}`)); w.Code != http.StatusAccepted {
		t.Fatalf("status: got %d, want 202", w.Code)
	}
	raw := Rows[storage.RawPayloadRow](node2)
	if len(raw) != 1 || raw[0].Reason != "unexpected_shape" {
		t.Fatalf("raw payload rows: %+v", raw)
	}
}

// ---------------------------------------------------------------------------
// Integration: the migration, the INSERT and AppendTo have to agree
// ---------------------------------------------------------------------------

// Every arity assertion in storagetest runs against a fake batch, so it proves
// the column COUNT and nothing about the types. This one inserts real rows
// into a scratch ClickHouse built from the real migrations and reads them
// back: a DateTime64(9) that cannot take a time.Time, an Enum8 that rejects a
// state name, a Nullable that will not take a nil pointer — all of those fail
// here and nowhere else.
func TestTraceTablesRoundTripThroughClickHouse(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	// Inside every TTL window on purpose: the tables drop rows by
	// toDateTime(start) / toDateTime(bucket_start), and ClickHouse applies
	// that at insert time, so a fixed date in the past would make this test
	// count zero rows and look like a broken INSERT.
	now := time.Now().UTC().Add(-time.Hour)
	start := now.Truncate(time.Second).Add(123456789 * time.Nanosecond)
	count := 3.0
	span := storage.SpanRow{
		TenantID: "t1", ReceivedAt: now, WireFormat: "idx",
		AgentHostname: "agent-1", AgentVersion: "7.58.2", RareSamplerEnabled: 1,
		AgentTags:  map[string]string{"cluster": "eu-1"},
		Language:   "python",
		TracerTags: map[string]string{"team": "payments"},
		// A NULL-able group left entirely absent, which is the common case.
		Priority: 2, Origin: "lambda", SamplingMechanism: 4,
		Service: "checkout", Name: "flask.request", Resource: "GET /pay", SpanType: "web",
		TraceID: 0x6f, TraceIDHigh: 0x67890abcdef01234, SpanID: 333, ParentID: 222,
		Start: start, DurationNs: 5_000_000, Error: 1, Kind: "server",
		Meta:            map[string]string{"http.method": "GET"},
		Metrics:         map[string]float64{"retries": 3},
		MetaStruct:      map[string]string{"appsec": "\x00\x01"},
		AttributesJSON:  `{"tags":{"array":[{"string":"a"}]}}`,
		LinkTraceID:     []uint64{10},
		LinkTraceIDHigh: []uint64{99},
		LinkSpanID:      []uint64{11},
		LinkAttributes:  []string{`{"rel":{"string":"follows"}}`},
		LinkTracestate:  []string{"dd=s:1"},
		LinkFlags:       []uint32{0x80000001},
		EventTime:       []time.Time{start},
		EventName:       []string{"exception"},
		EventAttributes: []string{`{"code":{"int":42}}`},
	}
	// A second row with every optional thing filled, so the Nullable columns
	// are exercised in both directions.
	spanWithDebug := span
	spanWithDebug.SpanID = 334
	spanWithDebug.ContainerDebugError = traceStrPtr("context deadline exceeded")
	spanWithDebug.ContainerDebugLatencyMs = traceI64Ptr(0)
	spanWithDebug.ContainerDebugWasBuffered = traceU8Ptr(1)
	spanWithDebug.ContainerDebugBufferMs = traceI64Ptr(250)
	spanWithDebug.ContainerDebugBufferEvictionReason = traceStrPtr("timeout")

	storagetest.Insert(t, conn, storage.SpansWriter, []storage.Row{span, spanWithDebug})
	if n := storagetest.Count(t, conn, "spans"); n != 2 {
		t.Fatalf("spans: got %d rows, want 2", n)
	}
	got := storagetest.QueryRow(t, conn,
		"SELECT trace_id_hex, trace_id, trace_id_high, kind, meta_struct['appsec'], "+
			"link_flags[1], event_name[1], container_debug_error "+
			"FROM spans WHERE span_id = 333")
	// The materialized column is the form people paste out of a URL, and it
	// has to be the two integer columns concatenated, zero padded.
	if got[0] != "67890abcdef01234000000000000006f" {
		t.Errorf("trace_id_hex: got %v", got[0])
	}
	if got[1] != uint64(0x6f) || got[2] != uint64(0x67890abcdef01234) {
		t.Errorf("trace id columns: got %v / %v", got[1], got[2])
	}
	if got[4] != "\x00\x01" {
		t.Errorf("meta_struct went through as text: got %q", got[4])
	}
	if got[5] != uint32(0x80000001) {
		t.Errorf("link_flags[1]: got %v", got[5])
	}
	if got[7] != (*string)(nil) {
		if p, ok := got[7].(*string); !ok || p != nil {
			t.Errorf("container_debug_error should be NULL for the row that had none: got %#v", got[7])
		}
	}

	stat := storage.APMStatRow{
		TenantID: "t1", ReceivedAt: now, AgentHostname: "agent-1",
		ClientComputed: 1, ClientService: "checkout",
		ClientTags:       map[string][]string{"kube_service": {"a", "b"}},
		BucketStart:      start,
		BucketDurationNs: 10_000_000_000, AgentTimeShiftNs: -500,
		Service: "checkout", Name: "flask.request", Hits: 120, Errors: 3,
		IsTraceRoot: "true", PeerTags: []string{"db.hostname:pg-1"},
		OkSummary: storage.SketchSummary{
			Raw: "\x01\x02", State: storage.SketchOK, Count: &count,
			BinKeys: []int32{-3, 4}, BinCounts: []float64{1, 2},
		},
		ErrorSummary: storage.SketchSummary{Raw: "not a sketch", State: storage.SketchUndecodable},
	}
	storagetest.Insert(t, conn, storage.APMStatsWriter, []storage.Row{stat})
	got = storagetest.QueryRow(t, conn,
		"SELECT is_trace_root, ok_summary_state, error_summary_state, ok_count, error_count, "+
			"client_tags['kube_service'], ok_bin_keys FROM apm_stats")
	if got[0] != "true" || got[1] != "ok" || got[2] != "undecodable" {
		t.Errorf("enum columns travel by name: got %v / %v / %v", got[0], got[1], got[2])
	}
	if want := []string{"a", "b"}; !reflect.DeepEqual(got[5], want) {
		t.Errorf("client_tags multiset: got %v, want %v", got[5], want)
	}
	if want := []int32{-3, 4}; !reflect.DeepEqual(got[6], want) {
		t.Errorf("ok_bin_keys: got %v, want %v", got[6], want)
	}

	common := storage.DSMCommon{
		TenantID: "t1", ReceivedAt: now, Env: "prod", Service: "orders",
		Lang: "go", ProcessTags: []string{"entrypoint:worker"}, ProductMask: 3,
		BucketStart: start, BucketDurationNs: 10_000_000_000,
	}
	storagetest.Insert(t, conn, storage.DSMPipelineStatsWriter, []storage.Row{
		storage.DSMPipelineStatRow{
			DSMCommon: common,
			EdgeTags:  []string{"type:kafka"},
			Hash:      18446744073709551615, ParentHash: 42, TimestampType: "current",
			PathwayLatency: storage.SketchSummary{Raw: "\x01", State: storage.SketchOK, Count: &count},
			Via:            "trace-agent",
			UnknownKeys:    []string{"payload.SomethingNew"},
			UnknownJSON:    `{"payload.SomethingNew":"hello"}`,
		},
	})
	got = storagetest.QueryRow(t, conn, "SELECT hash, parent_hash, edge_latency_state, unknown_keys FROM dsm_pipeline_stats")
	if got[0] != uint64(18446744073709551615) {
		t.Errorf("hash: got %v — a 64-bit hash must survive the round trip whole", got[0])
	}
	if got[2] != "absent" {
		t.Errorf("a zero-value SketchSummary must still be a valid Enum8: got %v", got[2])
	}

	storagetest.Insert(t, conn, storage.DSMBacklogsWriter, []storage.Row{
		storage.DSMBacklogRow{DSMCommon: common, Tags: []string{"type:kafka_commit"}, Value: -17},
	})
	storagetest.Insert(t, conn, storage.DSMBucketTransactionsWriter, []storage.Row{
		storage.DSMBucketTransactionRow{DSMCommon: common, Transactions: "\x01\x02\x03"},
	})
	storagetest.Insert(t, conn, storage.DSMMessagesWriter, []storage.Row{
		storage.DSMMessageRow{TenantID: "t1", ReceivedAt: now, Position: 1,
			Message: `{"a":1}`, Keys: []string{"a"}, DDEVPOrigin: "browser"},
	})
	for table, want := range map[string]uint64{
		"dsm_backlogs": 1, "dsm_bucket_transactions": 1, "dsm_messages": 1,
	} {
		if n := storagetest.Count(t, conn, table); n != want {
			t.Errorf("%s: got %d rows, want %d", table, n, want)
		}
	}
}

// The zero row of every table this file owns must be insertable: a nil map or
// slice is rejected by the driver, and a real payload with no tags produces
// exactly that. storagetest asserts the arity against the writer's INSERT at
// the same time.
func TestTraceRowArity(t *testing.T) {
	storagetest.AssertArity(t, storage.SpansWriter, storage.SpanRow{})
	storagetest.AssertArity(t, storage.APMStatsWriter, storage.APMStatRow{})
	storagetest.AssertArity(t, storage.DSMPipelineStatsWriter, storage.DSMPipelineStatRow{})
	storagetest.AssertArity(t, storage.DSMBacklogsWriter, storage.DSMBacklogRow{})
	storagetest.AssertArity(t, storage.DSMBucketTransactionsWriter, storage.DSMBucketTransactionRow{})
	storagetest.AssertArity(t, storage.DSMMessagesWriter, storage.DSMMessageRow{})
}

// ---------------------------------------------------------------------------
// Hostile input
// ---------------------------------------------------------------------------

// AnyValue nests without limit on the wire, so an attribute can be a list of
// lists of lists as deep as the sender cares to encode. Our renderer must stop
// on its own: a Go stack overflow is a fatal runtime error that takes the
// whole node with it, not a failed request. The truncation has to be VISIBLE
// in the column, because an empty value would read as "no attributes".
func TestTraceAttrsJSONStopsAtDepthLimit(t *testing.T) {
	s := traceStrings{"", "deep", "leaf"}

	// One AnyValue per level, each an array holding the next, far past the cap.
	leaf := &idx.AnyValue{Value: &idx.AnyValue_StringValueRef{StringValueRef: 2}}
	v := leaf
	for i := 0; i < traceAttrMaxDepth*4; i++ {
		v = &idx.AnyValue{Value: &idx.AnyValue_ArrayValue{
			ArrayValue: &idx.ArrayValue{Values: []*idx.AnyValue{v}},
		}}
	}

	got := s.attrsJSON(map[uint32]*idx.AnyValue{1: v})
	if got == "" {
		t.Fatal("a deeply nested attribute produced no JSON at all")
	}
	if !strings.Contains(got, `"_depth_exceeded"`) {
		t.Errorf("no truncation marker in the rendered value: %.200s", got)
	}
	// The cap must actually bind: the marker replaces the tail, so the leaf
	// string never appears.
	if strings.Contains(got, `"leaf"`) {
		t.Error("the depth cap did not bind — the whole chain was rendered")
	}

	// A legitimately nested value — the two or three levels real tracers send
	// — must still round-trip whole.
	shallow := &idx.AnyValue{Value: &idx.AnyValue_KeyValueList{
		KeyValueList: &idx.KeyValueList{KeyValues: []*idx.KeyValue{{
			Key: 1,
			Value: &idx.AnyValue{Value: &idx.AnyValue_ArrayValue{
				ArrayValue: &idx.ArrayValue{Values: []*idx.AnyValue{leaf}},
			}},
		}}},
	}}
	if got := s.attrsJSON(map[uint32]*idx.AnyValue{1: shallow}); got != `{"deep":{"kvlist":[{"key":"deep","value":{"array":[{"string":"leaf"}]}}]}}` {
		t.Errorf("shallow nesting was damaged by the guard: %s", got)
	}
}

// ---------------------------------------------------------------------------
// /api/v0.1/pipeline_stats through HTTP
// ---------------------------------------------------------------------------

// dsmTestBody encodes a minimal but complete dd-trace-go StatsPayload the way
// its msgp codec does — a map whose keys are the Go field names — with one
// point, one backlog and one transactions blob, so a request exercises all
// three tables the handler fans out to.
func dsmTestBody(t *testing.T) []byte {
	t.Helper()

	var buf bytes.Buffer
	w := msgp.NewWriter(&buf)
	must := func(err error) {
		t.Helper()
		if err != nil {
			t.Fatalf("encode: %v", err)
		}
	}
	must(w.WriteMapHeader(3))
	must(w.WriteString("Env"))
	must(w.WriteString("prod"))
	must(w.WriteString("Service"))
	must(w.WriteString("orders"))
	must(w.WriteString("Stats"))
	must(w.WriteArrayHeader(1))
	{
		must(w.WriteMapHeader(5))
		must(w.WriteString("Start"))
		must(w.WriteUint64(1_700_000_000_000_000_000))
		must(w.WriteString("Duration"))
		must(w.WriteUint64(10_000_000_000))
		must(w.WriteString("Stats"))
		must(w.WriteArrayHeader(1))
		{
			must(w.WriteMapHeader(2))
			must(w.WriteString("Hash"))
			must(w.WriteUint64(18446744073709551615))
			must(w.WriteString("EdgeTags"))
			must(w.WriteArrayHeader(1))
			must(w.WriteString("type:kafka"))
		}
		must(w.WriteString("Backlogs"))
		must(w.WriteArrayHeader(1))
		{
			must(w.WriteMapHeader(2))
			must(w.WriteString("Tags"))
			must(w.WriteArrayHeader(1))
			must(w.WriteString("partition:3"))
			must(w.WriteString("Value"))
			must(w.WriteInt64(-17))
		}
		must(w.WriteString("Transactions"))
		must(w.WriteBytes([]byte{0x01, 0x02, 0x03}))
	}
	must(w.Flush())
	return buf.Bytes()
}

// postPipelineStats sends a gzipped pipeline_stats body with the three headers
// the trace-agent adds on the way through. The engine's own Decompress()
// unwraps it, which is what the real proxy chain does.
func postPipelineStats(t *testing.T, e *gin.Engine, body []byte, extra map[string]string) *httptest.ResponseRecorder {
	t.Helper()

	var gz bytes.Buffer
	zw := gzip.NewWriter(&gz)
	if _, err := zw.Write(body); err != nil {
		t.Fatalf("gzip: %v", err)
	}
	if err := zw.Close(); err != nil {
		t.Fatalf("gzip close: %v", err)
	}

	req := httptest.NewRequest(http.MethodPost, "/api/v0.1/pipeline_stats", bytes.NewReader(gz.Bytes()))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/msgpack")
	req.Header.Set("Content-Encoding", "gzip")
	for k, v := range extra {
		req.Header.Set(k, v)
	}

	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	return w
}

// The whole pipeline_stats request as a tracer's agent actually makes it:
// gzipped msgpack behind the trace-agent's proxy headers. One request has to
// reach three different writers, and the container identity lives ONLY in
// those headers — the body never repeats it, so a handler that reads them
// after the body would lose it silently.
func TestHandlePipelineStatsStoresAllThreeTables(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeTrace)

	w := postPipelineStats(t, e, dsmTestBody(t), map[string]string{
		"Via":                       "trace-agent 7.58.2",
		"X-Datadog-Additional-Tags": "_dd.tags.container:cid",
		"X-Datadog-Container-Tags":  "kube_namespace:shop",
	})
	if w.Code != http.StatusAccepted {
		t.Fatalf("status: got %d, want 202 (%s)", w.Code, w.Body.String())
	}
	if got := w.Body.String(); got != `{}` {
		t.Errorf("body: got %s, want {}", got)
	}

	// One batch per table, addressed to the writer that owns it.
	for _, writer := range []gen.Atom{
		storage.DSMPipelineStatsWriter,
		storage.DSMBacklogsWriter,
		storage.DSMBucketTransactionsWriter,
	} {
		if msgs := node.SentTo(writer); len(msgs) != 1 {
			t.Errorf("messages to %s: got %d, want 1", writer, len(msgs))
		}
	}

	points := Rows[storage.DSMPipelineStatRow](node)
	if len(points) != 1 {
		t.Fatalf("pipeline stat rows: got %d, want 1", len(points))
	}
	p := points[0]
	if p.TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", p.TenantID, testTenant)
	}
	if p.Service != "orders" || p.Env != "prod" {
		t.Errorf("payload level lost: %+v", p)
	}
	if p.Hash != 18446744073709551615 {
		t.Errorf("hash: got %d — a 64-bit hash must not pass through a float", p.Hash)
	}
	// The four headers only the handler can see.
	if p.Via != "trace-agent 7.58.2" || p.AdditionalTags != "_dd.tags.container:cid" ||
		p.ContainerTags != "kube_namespace:shop" || p.ContentEncoding != "gzip" {
		t.Errorf("proxy headers did not reach the row: via=%q additional=%q container=%q encoding=%q",
			p.Via, p.AdditionalTags, p.ContainerTags, p.ContentEncoding)
	}

	backlogs := Rows[storage.DSMBacklogRow](node)
	if len(backlogs) != 1 || backlogs[0].Value != -17 || backlogs[0].TenantID != testTenant {
		t.Errorf("backlog rows: %+v", backlogs)
	}
	buckets := Rows[storage.DSMBucketTransactionRow](node)
	if len(buckets) != 1 || buckets[0].Transactions != string([]byte{1, 2, 3}) {
		t.Errorf("bucket transaction rows: %+v", buckets)
	}
	if n := len(Rows[storage.RawPayloadRow](node)); n != 0 {
		t.Errorf("a payload that decoded cleanly produced %d raw rows, want 0", n)
	}
}

// A body the msgp decoder cannot read — a tracer version that changed the
// schema, or a mislabelled encoding — must still answer 202 and land in
// raw_payloads, because this endpoint is a proxy target and the sender has
// nowhere to retry to.
func TestHandlePipelineStatsStoresUndecodableBodyRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeTrace)

	w := postPipelineStats(t, e, []byte("not a msgpack stats payload"), nil)
	if w.Code != http.StatusAccepted {
		t.Fatalf("status: got %d, want 202", w.Code)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 || raw[0].Reason != "decode_error" {
		t.Fatalf("raw payload rows: %+v", raw)
	}
	if raw[0].Intake != traceIntake {
		t.Errorf("intake label: got %q, want %q", raw[0].Intake, traceIntake)
	}
	if n := len(Rows[storage.DSMPipelineStatRow](node)); n != 0 {
		t.Errorf("%d pipeline stat rows from an undecodable body, want 0", n)
	}
}

// The agent's connectivity sweep hits this route too, with an empty body. It
// must not reach the decoder and must not fill raw_payloads on every restart.
func TestHandlePipelineStatsIgnoresDiagnoseProbe(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeTrace)

	req := httptest.NewRequest(http.MethodPost, "/api/v0.1/pipeline_stats", bytes.NewReader(nil))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("X-Requested-With", "datadog-agent-diagnose")
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusAccepted {
		t.Errorf("status: got %d, want 202", w.Code)
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("a diagnose probe stored %d messages, want 0", n)
	}
}
