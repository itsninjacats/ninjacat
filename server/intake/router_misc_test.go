package intake

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// ---------------------------------------------------------------------------
// Converter-level tests: the pure functions, no HTTP, no gin.
// ---------------------------------------------------------------------------

// A tracer telemetry sequence number is documented as a monotonically
// increasing counter with no upper bound the spec promises to respect; a
// real client library has been seen past 2^53, the point past which a
// float64 (or a bare interface{} JSON decode) silently rounds. json.Number
// must keep every digit.
func TestApmSeqIDKeepsPrecisionPastFloat64(t *testing.T) {
	const big = 9007199254740993 // 2^53 + 1
	raw := json.RawMessage(`9007199254740993`)

	got := apmSeqID(raw)
	if got == nil {
		t.Fatalf("apmSeqID(%s) = nil, want %d", raw, big)
	}
	if *got != big {
		t.Errorf("apmSeqID(%s) = %d, want %d", raw, *got, big)
	}

	if got := apmSeqID(nil); got != nil {
		t.Errorf("apmSeqID(nil) = %v, want nil — absent, not zero", *got)
	}
	if got := apmSeqID(json.RawMessage(`"not-a-number"`)); got != nil {
		t.Errorf("apmSeqID(non-numeric) = %v, want nil", *got)
	}
}

// tracer_time/event_time are Unix seconds with a fractional part; the raw
// text must survive even when nothing parses it, so a producer bug in the
// parser is never the reason a value disappears.
func TestApmUnixTimeParsesFractionalSecondsAndKeepsRawOnFailure(t *testing.T) {
	got := apmUnixTime(json.RawMessage(`1732000000.5`))
	if got == nil {
		t.Fatal("apmUnixTime(1732000000.5) = nil, want a time")
	}
	want := time.Unix(1732000000, 500000000).UTC()
	if !got.Equal(want) {
		t.Errorf("apmUnixTime(1732000000.5) = %v, want %v", got, want)
	}

	if got := apmUnixTime(nil); got != nil {
		t.Errorf("apmUnixTime(nil) = %v, want nil — absent, not epoch", *got)
	}
	if got := apmUnixTime(json.RawMessage(`"not a time"`)); got != nil {
		t.Errorf("apmUnixTime(garbage) = %v, want nil, not epoch", *got)
	}

	// apmRawText must keep the literal text regardless of whether apmUnixTime
	// could parse it — this is what lets the *_raw column recover a value the
	// parser above got wrong.
	if got := apmRawText(json.RawMessage(`"not a time"`)); got != `"not a time"` {
		t.Errorf("apmRawText(garbage) = %q, want the literal JSON text", got)
	}
	if got := apmRawText(nil); got != "" {
		t.Errorf("apmRawText(nil) = %q, want empty", got)
	}
}

// A nil (absent) application or host sub-message must not panic and must
// come back as zero values, not as an error — many producers omit either.
func TestApmApplicationAndHostToleratesAbsence(t *testing.T) {
	svc, ver, env, lang, langVer, tracerVer := apmApplication(nil)
	if svc != "" || ver != "" || env != "" || lang != "" || langVer != "" || tracerVer != "" {
		t.Errorf("apmApplication(nil) returned non-empty fields")
	}

	host, os, arch, extra := apmHost(nil)
	if host != "" || os != "" || arch != "" || extra != nil {
		t.Errorf("apmHost(nil) returned non-empty fields")
	}
}

// gohai and the tracer both send more host keys than hostname/os/architecture
// (kernel_version, cpu_cores, ...); those must land in HostExtra by name
// rather than being silently dropped because they have no column of their
// own.
func TestApmHostKeepsUndeclaredKeys(t *testing.T) {
	raw := json.RawMessage(`{
		"hostname": "h1", "os": "linux", "architecture": "amd64",
		"kernel_version": "5.15.0", "cpu_cores": 8
	}`)

	hostname, osName, arch, extra := apmHost(raw)
	if hostname != "h1" || osName != "linux" || arch != "amd64" {
		t.Errorf("apmHost known fields: got %q/%q/%q", hostname, osName, arch)
	}
	if got := extra["kernel_version"]; got != `"5.15.0"` {
		t.Errorf("host_extra[kernel_version] = %q, want the raw JSON string", got)
	}
	if got := extra["cpu_cores"]; got != `8` {
		t.Errorf("host_extra[cpu_cores] = %q, want the raw JSON number", got)
	}
	if _, ok := extra["hostname"]; ok {
		t.Errorf("host_extra kept a declared key (hostname) as well")
	}
}

// A key the envelope carries that this file does not read by name (a future
// producer's addition) must stay visible in Extra rather than disappear —
// the same rule k8s_actions' extra_keys follows for undeclared JSON keys.
func TestApmExtraKeysKeepsUndeclaredEnvelopeKeys(t *testing.T) {
	env := map[string]json.RawMessage{
		"request_type": json.RawMessage(`"app-started"`),
		"api_version":  json.RawMessage(`"v2"`),
		"future_field": json.RawMessage(`"surprise"`),
	}
	extra := apmExtraKeys(env)
	if got := extra["future_field"]; got != `"surprise"` {
		t.Errorf("extra[future_field] = %q, want the raw JSON text", got)
	}
	if _, ok := extra["request_type"]; ok {
		t.Errorf("extra kept a declared key (request_type) as well")
	}
	if len(extra) != 1 {
		t.Errorf("extra has %d keys, want 1", len(extra))
	}
}

// ---------------------------------------------------------------------------
// Handler-level tests: real requests through the real engine.
// ---------------------------------------------------------------------------

// A tracer's app-started envelope is the simplest, most common shape: one
// request, one row, with the runtime/application/host fields that identify
// which process sent it.
func TestHandleTelemetryStoresTracerAppStarted(t *testing.T) {
	a, node := newTestServer(t)

	body := []byte(`{
		"api_version": "v2",
		"request_type": "app-started",
		"tracer_time": 1732000000.5,
		"runtime_id": "11111111-1111-1111-1111-111111111111",
		"seq_id": 9007199254740993,
		"application": {
			"service_name": "checkout", "service_version": "1.0.0", "env": "prod",
			"language_name": "go", "language_version": "1.22", "tracer_version": "1.60.0"
		},
		"host": {"hostname": "h1", "os": "linux", "architecture": "amd64"},
		"payload": {"integrations": []},
		"debug": false
	}`)

	e := newTestEngine(t, a, a.routeTelemetry)
	if w := post(t, e, "/api/v2/apmtelemetry", body); w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/apmtelemetry: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.APMTelemetryRow](node)
	if len(rows) != 1 {
		t.Fatalf("apm_telemetry rows: got %d, want 1", len(rows))
	}
	r := rows[0]

	if r.TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", r.TenantID, testTenant)
	}
	if r.RequestType != "app-started" || r.Producer != "tracer" {
		t.Errorf("request_type/producer: got %q/%q, want app-started/tracer", r.RequestType, r.Producer)
	}
	if r.RuntimeID != "11111111-1111-1111-1111-111111111111" {
		t.Errorf("runtime_id: got %q", r.RuntimeID)
	}
	if r.SeqID == nil || *r.SeqID != 9007199254740993 {
		t.Errorf("seq_id: got %v, want 9007199254740993 without rounding", r.SeqID)
	}
	if r.TracerTime == nil || !r.TracerTime.Equal(time.Unix(1732000000, 500000000).UTC()) {
		t.Errorf("tracer_time: got %v", r.TracerTime)
	}
	if r.ServiceName != "checkout" || r.Env != "prod" || r.LanguageName != "go" {
		t.Errorf("application: got service=%q env=%q language=%q", r.ServiceName, r.Env, r.LanguageName)
	}
	if r.Hostname != "h1" || r.HostOS != "linux" {
		t.Errorf("host: got hostname=%q os=%q", r.Hostname, r.HostOS)
	}
	if r.BatchIndex != nil {
		t.Errorf("batch_index: got %v, want nil — this is not a batch entry", *r.BatchIndex)
	}
	if r.ParentRequestType != "" {
		t.Errorf("parent_request_type: got %q, want empty", r.ParentRequestType)
	}
}

// message-batch fans out into one row per entry, in addition to the parent
// row — the parent keeps the whole batch as its own payload, and each child
// carries its own request_type, payload and position in the batch.
func TestHandleTelemetryMessageBatchProducesOneRowPerEntryPlusParent(t *testing.T) {
	a, node := newTestServer(t)

	body := []byte(`{
		"api_version": "v2",
		"request_type": "message-batch",
		"runtime_id": "22222222-2222-2222-2222-222222222222",
		"application": {"service_name": "worker"},
		"host": {"hostname": "h2"},
		"payload": [
			{"request_type": "logs", "payload": {"logs": [{"message": "started"}]}},
			{"request_type": "generate-metrics", "payload": {"series": []}}
		]
	}`)

	e := newTestEngine(t, a, a.routeTelemetry)
	if w := post(t, e, "/api/v2/apmtelemetry", body); w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/apmtelemetry: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.APMTelemetryRow](node)
	if len(rows) != 3 {
		t.Fatalf("apm_telemetry rows: got %d, want 3 (1 parent + 2 batch entries)", len(rows))
	}

	parent := rows[0]
	if parent.RequestType != "message-batch" {
		t.Errorf("parent request_type: got %q, want message-batch", parent.RequestType)
	}
	if parent.BatchIndex != nil {
		t.Errorf("parent batch_index: got %v, want nil", *parent.BatchIndex)
	}
	if parent.ParentRequestType != "" {
		t.Errorf("parent parent_request_type: got %q, want empty", parent.ParentRequestType)
	}
	// The parent row keeps the WHOLE batch, not just the last entry's payload.
	var wholeBatch []json.RawMessage
	if err := json.Unmarshal([]byte(parent.Payload), &wholeBatch); err != nil {
		t.Fatalf("parent payload is not the batch array: %v (%s)", err, parent.Payload)
	}
	if len(wholeBatch) != 2 {
		t.Errorf("parent payload holds %d batch entries, want 2", len(wholeBatch))
	}

	first, second := rows[1], rows[2]
	if first.RequestType != "logs" || first.BatchIndex == nil || *first.BatchIndex != 0 {
		t.Errorf("entry 0: request_type=%q batch_index=%v, want logs/0", first.RequestType, first.BatchIndex)
	}
	if first.ParentRequestType != "message-batch" {
		t.Errorf("entry 0 parent_request_type: got %q, want message-batch", first.ParentRequestType)
	}
	if !bytes.Contains([]byte(first.Payload), []byte("started")) {
		t.Errorf("entry 0 payload: got %q, want it to hold its own logs payload", first.Payload)
	}
	if second.RequestType != "generate-metrics" || second.BatchIndex == nil || *second.BatchIndex != 1 {
		t.Errorf("entry 1: request_type=%q batch_index=%v, want generate-metrics/1", second.RequestType, second.BatchIndex)
	}
	// Every row of one request shares its identity, since batch entries carry
	// no envelope of their own.
	if first.RuntimeID != parent.RuntimeID || second.Hostname != parent.Hostname {
		t.Errorf("batch entries did not inherit the parent envelope's identity fields")
	}
}

// agent-telemetry is identified by event_time when request_type alone
// (agent-metrics here, but also the ambiguous logs/traces/message-batch
// values) does not say which producer this is; the parsed value must round
// trip through apm_telemetry's Nullable(DateTime) column.
func TestHandleTelemetryStoresAgentTelemetryEventTime(t *testing.T) {
	a, node := newTestServer(t)

	body := []byte(`{
		"api_version": "v2",
		"request_type": "agent-metrics",
		"event_time": 1732000000,
		"host": {"hostname": "agent-1", "os": "linux", "architecture": "amd64"},
		"payload": {"message": "ok", "metrics": {"cpu": {"avg": 1.2}}}
	}`)

	e := newTestEngine(t, a, a.routeTelemetry)
	if w := post(t, e, "/api/v2/apmtelemetry", body); w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/apmtelemetry: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.APMTelemetryRow](node)
	if len(rows) != 1 {
		t.Fatalf("apm_telemetry rows: got %d, want 1", len(rows))
	}
	r := rows[0]

	if r.Producer != "agent-telemetry" {
		t.Errorf("producer: got %q, want agent-telemetry", r.Producer)
	}
	want := time.Unix(1732000000, 0).UTC()
	if r.EventTime == nil || !r.EventTime.Equal(want) {
		t.Errorf("event_time: got %v, want %v", r.EventTime, want)
	}
	if r.EventTimeRaw != "1732000000" {
		t.Errorf("event_time_raw: got %q, want the literal 1732000000", r.EventTimeRaw)
	}
	if r.Hostname != "agent-1" {
		t.Errorf("hostname: got %q", r.Hostname)
	}
}

// A body that is not a JSON object at all (garbage, or a bare JSON array)
// cannot be read as an envelope, so it goes to raw_payloads instead of being
// logged and dropped.
func TestHandleTelemetryNonObjectBodyGoesToRawPayloads(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeTelemetry)

	if w := post(t, e, "/api/v2/apmtelemetry", []byte(`not json at all`)); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	if n := len(Rows[storage.APMTelemetryRow](node)); n != 0 {
		t.Errorf("apm_telemetry rows: got %d, want 0 — an undecodable body has no envelope to build one from", n)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 {
		t.Fatalf("raw_payloads rows: got %d, want 1", len(raw))
	}
	if raw[0].Intake != "apmtelemetry" || raw[0].Reason != "decode_error" {
		t.Errorf("intake/reason: got %q/%q, want apmtelemetry/decode_error", raw[0].Intake, raw[0].Reason)
	}
	if raw[0].Body != "not json at all" {
		t.Errorf("body: got %q", raw[0].Body)
	}
}

// genresources has no published schema anywhere upstream, so every request
// — whatever the integration actually sent — goes to raw_payloads whole,
// with the event platform's origin headers kept in the note since there is
// no column for them.
func TestHandleGenResourcesStoresRawPayload(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeResources)

	payload := []byte{0x0a, 0x04, 't', 'e', 's', 't', 0xff, 0x00} // opaque, not valid JSON or UTF-8
	req := httptest.NewRequest(http.MethodPost, "/api/v2/genresources", bytes.NewReader(payload))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/x-protobuf")
	req.Header.Set("DD-EVP-ORIGIN", "agent")
	req.Header.Set("DD-EVP-ORIGIN-VERSION", "7.58.2")

	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	if w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/genresources: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.RawPayloadRow](node)
	if len(rows) != 1 {
		t.Fatalf("raw_payloads rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.Intake != "genresources" || r.Reason != "no_schema" {
		t.Errorf("intake/reason: got %q/%q, want genresources/no_schema", r.Intake, r.Reason)
	}
	if r.Body != string(payload) {
		t.Errorf("body: got %q, want the exact bytes sent", r.Body)
	}
	if r.BodyBytes != uint64(len(payload)) {
		t.Errorf("body_bytes: got %d, want %d", r.BodyBytes, len(payload))
	}
	if !bytes.Contains([]byte(r.Note), []byte("DD-EVP-ORIGIN=\"agent\"")) {
		t.Errorf("note: got %q, want it to carry DD-EVP-ORIGIN", r.Note)
	}
}

// ---------------------------------------------------------------------------
// Integration: migration 0011, the INSERT and AppendTo agreeing with
// ClickHouse's own types — proven by writing real rows and reading them back.
// ---------------------------------------------------------------------------

// A parent row and one batch-child row of it, inserted through the
// registered writer, must both land with the columns that distinguish them:
// batch_index NULL on the parent and set on the child, the nullable
// timestamps surviving the trip, and the json.Number seq_id landing intact
// in ClickHouse's own Int64.
func TestAPMTelemetryRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	received := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	tracerTime := time.Date(2026, 9, 23, 10, 29, 59, 500000000, time.UTC)
	bigSeqID := int64(9007199254740993) // 2^53 + 1

	parent := storage.APMTelemetryRow{
		TenantID:    "default",
		ReceivedAt:  received,
		RequestType: "message-batch",
		Producer:    "tracer",
		APIVersion:  "v2",
		RuntimeID:   "11111111-1111-1111-1111-111111111111",
		SeqID:       &bigSeqID,
		TracerTime:  &tracerTime,
		ServiceName: "checkout",
		Hostname:    "h1",
		Payload:     `[{"request_type":"logs","payload":{}}]`,
		// BatchIndex and ParentRequestType stay zero: this is the request's
		// own row, not a batch entry.
	}
	batchIdx := uint32(0)
	child := parent
	child.RequestType = "logs"
	child.Payload = `{}`
	child.BatchIndex = &batchIdx
	child.ParentRequestType = "message-batch"

	storagetest.Insert(t, conn, storage.APMTelemetryWriter, []storage.Row{parent, child})

	if got := storagetest.Count(t, conn, "apm_telemetry"); got != 2 {
		t.Fatalf("apm_telemetry: %d rows, want 2", got)
	}

	row := storagetest.QueryRow(t, conn,
		`SELECT request_type, seq_id, batch_index, parent_request_type
		 FROM apm_telemetry WHERE parent_request_type = '' LIMIT 1`)
	if len(row) != 4 {
		t.Fatalf("SELECT returned %d columns, want 4", len(row))
	}
	if row[0] != "message-batch" {
		t.Errorf("parent request_type: got %v, want message-batch", row[0])
	}
	// Nullable columns scan as pointers: non-nil here proves seq_id survived
	// ClickHouse's own Int64 without going through a float64 anywhere.
	seqID, ok := row[1].(*int64)
	if !ok || seqID == nil || *seqID != bigSeqID {
		t.Errorf("parent seq_id: got %v (%T), want %d without float rounding", row[1], row[1], bigSeqID)
	}
	parentBatchIdx, ok := row[2].(*uint32)
	if !ok {
		t.Fatalf("parent batch_index: unexpected scan type %T", row[2])
	}
	if parentBatchIdx != nil {
		t.Errorf("parent batch_index: got %d, want NULL — this is the request's own row", *parentBatchIdx)
	}

	childRow := storagetest.QueryRow(t, conn,
		`SELECT request_type, batch_index, parent_request_type
		 FROM apm_telemetry WHERE parent_request_type = 'message-batch' LIMIT 1`)
	if childRow[0] != "logs" {
		t.Errorf("child request_type: got %v, want logs", childRow[0])
	}
	childBatchIdx, ok := childRow[1].(*uint32)
	if !ok || childBatchIdx == nil {
		t.Fatalf("child batch_index: got %v, want 0", childRow[1])
	}
	if *childBatchIdx != 0 {
		t.Errorf("child batch_index: got %d, want 0", *childBatchIdx)
	}
}
