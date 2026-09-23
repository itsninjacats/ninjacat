package intake

import (
	"bytes"
	"compress/gzip"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"reflect"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/google/pprof/profile"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// ---------------------------------------------------------------------------
// Test helpers: a real pprof attachment, and a multipart body builder.
// ---------------------------------------------------------------------------

// buildPprof returns a small but real gzip'd pprof profile — one sample, one
// location, one function, a period and a sample type — built with the same
// upstream type (github.com/google/pprof/profile) profDecodeProfile decodes
// with, so the round trip through profile.ParseData is exercised for real
// rather than assumed.
func buildPprof(t *testing.T) []byte {
	t.Helper()
	fn := &profile.Function{ID: 1, Name: "main.main", SystemName: "main.main", Filename: "main.go"}
	loc := &profile.Location{ID: 1, Address: 0x1000, Line: []profile.Line{{Function: fn, Line: 42}}}
	prof := &profile.Profile{
		SampleType: []*profile.ValueType{{Type: "cpu", Unit: "nanoseconds"}},
		Sample:     []*profile.Sample{{Location: []*profile.Location{loc}, Value: []int64{100}}},
		Location:   []*profile.Location{loc},
		Function:   []*profile.Function{fn},
		// Mapping is deliberately left nil: a real profile can legitimately
		// carry none (no shared object, e.g. a Go binary with inlined
		// symbols), and profileRow must not panic on the nil slice.
		TimeNanos:     1_700_000_000_000_000_000,
		DurationNanos: 10_000_000_000,
		PeriodType:    &profile.ValueType{Type: "cpu", Unit: "nanoseconds"},
		Period:        10_000_000,
	}
	var buf bytes.Buffer
	if err := prof.Write(&buf); err != nil {
		t.Fatalf("write pprof: %v", err)
	}
	return buf.Bytes()
}

// buildMultipart writes fields (form name -> raw part bytes) as a
// multipart/form-data body, in a deterministic (sorted) part order, and
// returns the body plus the Content-Type header value that names its
// boundary.
func buildMultipart(t *testing.T, fields map[string][]byte) ([]byte, string) {
	t.Helper()
	var buf bytes.Buffer
	w := multipart.NewWriter(&buf)
	for _, name := range sortedKeys(fields) {
		part, err := w.CreateFormField(name)
		if err != nil {
			t.Fatalf("create field %s: %v", name, err)
		}
		if _, err := part.Write(fields[name]); err != nil {
			t.Fatalf("write field %s: %v", name, err)
		}
	}
	if err := w.Close(); err != nil {
		t.Fatalf("close multipart writer: %v", err)
	}
	return buf.Bytes(), w.FormDataContentType()
}

func gzipBytes(t *testing.T, data []byte) []byte {
	t.Helper()
	var buf bytes.Buffer
	zw := gzip.NewWriter(&buf)
	if _, err := zw.Write(data); err != nil {
		t.Fatalf("gzip write: %v", err)
	}
	if err := zw.Close(); err != nil {
		t.Fatalf("gzip close: %v", err)
	}
	return buf.Bytes()
}

// postWithContentType is post() (testserver_test.go) with a caller-chosen
// Content-Type — needed for multipart bodies, whose boundary is part of the
// header value.
func postWithContentType(t *testing.T, e *gin.Engine, path, contentType string, body []byte) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(body))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", contentType)
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	return w
}

// testGinContext builds a *gin.Context carrying the given request headers,
// for calling profileRow directly without a full HTTP round trip.
func testGinContext(headers map[string]string) *gin.Context {
	gin.SetMode(gin.TestMode)
	c, _ := gin.CreateTestContext(httptest.NewRecorder())
	req := httptest.NewRequest(http.MethodPost, "/api/v2/profile", nil)
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	c.Request = req
	return c
}

// ---------------------------------------------------------------------------
// profileRow — the conversion at the heart of intake.profile.<site>.
// ---------------------------------------------------------------------------

// A real submission: one part that IS pprof (with a nil Mapping slice — a
// legitimate absence, not an error), one part that is NOT pprof, an event
// carrying a multi-valued tags_profiler tag and a key (custom_field) the
// converter never names — proving it survives in the raw Event column even
// though nothing extracts it into a column of its own.
func TestProfileRowFromRealSubmission(t *testing.T) {
	pprofBytes := buildPprof(t)
	eventJSON := []byte(`{
		"family": "go", "version": "1.2.3", "runtime": "go1.23", "language": "go",
		"end": "2026-09-23T10:30:00Z",
		"tags_profiler": "service:my-svc,env:prod,also:x,also:y",
		"custom_field": "from a newer agent"
	}`)
	parts := map[string][]byte{
		"event":     eventJSON,
		"cpu.pprof": pprofBytes,
		"notes.txt": []byte("not a profile"),
	}
	event := profDecodeEvent("profile", eventJSON)
	profs := map[string]*profile.Profile{}
	for _, name := range []string{"cpu.pprof", "notes.txt"} {
		if p := profDecodeProfile("profile", name, parts[name]); p != nil {
			profs[name] = p
		}
	}

	c := testGinContext(map[string]string{
		"Dd-Evp-Origin":         "dd-trace-go",
		"Dd-Evp-Origin-Version": "1.60.0",
	})
	row := profileRow("test-tenant", "profile", c, parts, event, profs)

	if row.TenantID != "test-tenant" || row.Variant != "profile" {
		t.Errorf("tenant/variant: got %q/%q", row.TenantID, row.Variant)
	}
	if row.DDEvpOrigin != "dd-trace-go" || row.DDEvpOriginVersion != "1.60.0" {
		t.Errorf("evp origin: got %q/%q", row.DDEvpOrigin, row.DDEvpOriginVersion)
	}
	// The lossless copy must contain the key no column extracts.
	if !bytes.Contains([]byte(row.Event), []byte("custom_field")) {
		t.Errorf("Event dropped an undeclared key: %q", row.Event)
	}
	if row.Family != "go" || row.Version != "1.2.3" || row.Runtime != "go1.23" || row.Language != "go" {
		t.Errorf("event fields: got family=%q version=%q runtime=%q language=%q",
			row.Family, row.Version, row.Runtime, row.Language)
	}
	// start was never sent: raw "" and parsed nil, not a zero time.
	if row.StartRaw != "" || row.StartParsed != nil {
		t.Errorf("start: got raw=%q parsed=%v, want absent (absent must not become a zero time)", row.StartRaw, row.StartParsed)
	}
	if row.EndParsed == nil || !row.EndParsed.Equal(time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)) {
		t.Errorf("end_parsed: got %v", row.EndParsed)
	}
	// A tag key repeated (also:x, also:y) must keep BOTH values — the
	// multiset rule every other Datadog tag column follows.
	if got := row.TagsProfiler["also"]; len(got) != 2 || got[0] != "x" || got[1] != "y" {
		t.Errorf("tags_profiler[also]: got %v, want [x y] — duplicate tag keys must not collapse", got)
	}

	// Attachments, in sorted-name order: cpu.pprof before notes.txt.
	if want := []string{"cpu.pprof", "notes.txt"}; !equalStrings(row.AttachName, want) {
		t.Fatalf("attach_name: got %v, want %v", row.AttachName, want)
	}
	if want := []uint8{1, 0}; !equalU8(row.AttachParsed, want) {
		t.Errorf("attach_parsed: got %v, want %v", row.AttachParsed, want)
	}
	if got := row.AttachSampleTypes[0]; len(got) != 1 || got[0] != "cpu" {
		t.Errorf("attach_sample_types[0]: got %v, want [cpu]", got)
	}
	// The non-pprof attachment gets an EMPTY array, never nil (the driver
	// rejects nil slices) and never a made-up sample type.
	if got := row.AttachSampleTypes[1]; len(got) != 0 {
		t.Errorf("attach_sample_types[1]: got %v, want empty", got)
	}
	if row.AttachPeriodType[0] != "cpu/nanoseconds" || row.AttachPeriodType[1] != "" {
		t.Errorf("attach_period_type: got %v", row.AttachPeriodType)
	}
	// Mapping was never set on the pprof profile (nil slice) — a legitimate
	// zero, not a decode failure, and must not panic building the row.
	if row.AttachMappingCount[0] != 0 {
		t.Errorf("attach_mapping_count[0]: got %d, want 0 (Mapping was nil upstream)", row.AttachMappingCount[0])
	}
	if row.AttachLocationCount[0] != 1 || row.AttachFunctionCount[0] != 1 {
		t.Errorf("attach location/function counts: got %d/%d, want 1/1",
			row.AttachLocationCount[0], row.AttachFunctionCount[0])
	}
	if row.AttachSampleCount[0] != 1 {
		t.Errorf("attach_sample_count[0]: got %d, want 1", row.AttachSampleCount[0])
	}
}

// No event part at all: the row must still build (attachments alone are
// worth keeping), with every event-derived column at its zero value rather
// than a panic on a nil map.
func TestProfileRowWithoutEventPart(t *testing.T) {
	parts := map[string][]byte{"cpu.pprof": []byte("not actually pprof either")}
	c := testGinContext(nil)
	row := profileRow("t", "profile-v1", c, parts, nil, map[string]*profile.Profile{})

	if row.Event != "" || row.Family != "" {
		t.Errorf("expected empty event-derived fields, got Event=%q Family=%q", row.Event, row.Family)
	}
	if len(row.AttachName) != 1 || row.AttachName[0] != "cpu.pprof" {
		t.Errorf("attachments still expected without an event part, got %v", row.AttachName)
	}
	if row.AttachParsed[0] != 0 {
		t.Errorf("attach_parsed: got %d, want 0 (garbage bytes are not pprof)", row.AttachParsed[0])
	}
}

func equalStrings(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func equalU8(a, b []uint8) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// ---------------------------------------------------------------------------
// profParseTimestamp — RFC3339 strings and epoch numbers at four magnitudes.
// ---------------------------------------------------------------------------

func TestProfParseTimestamp(t *testing.T) {
	want := time.Date(2025, 9, 23, 10, 30, 0, 0, time.UTC)
	cases := []struct {
		name string
		in   any
		want *time.Time
	}{
		{"RFC3339", "2025-09-23T10:30:00Z", &want},
		{"RFC3339Nano with fraction", "2025-09-23T10:30:00.5Z", profTimePtr(want.Add(500 * time.Millisecond))},
		{"epoch seconds", jsonNum("1758623400"), &want},
		{"epoch millis", jsonNum("1758623400000"), &want},
		{"epoch micros", jsonNum("1758623400000000"), &want},
		{"epoch nanos", jsonNum("1758623400000000000"), &want},
		{"zero is not a timestamp", jsonNum("0"), nil},
		{"negative is not a timestamp", jsonNum("-5"), nil},
		{"garbage string", "not a time", nil},
		{"wrong type entirely", true, nil},
		{"absent", nil, nil},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := profParseTimestamp(tc.in)
			switch {
			case tc.want == nil && got != nil:
				t.Errorf("got %v, want nil", got)
			case tc.want != nil && got == nil:
				t.Errorf("got nil, want %v", tc.want)
			case tc.want != nil && !got.Equal(*tc.want):
				t.Errorf("got %v, want %v", got, tc.want)
			}
		})
	}
}

// ---------------------------------------------------------------------------
// dbgDecodeLogs — JSON array and NDJSON, the framing gap rum-spec.md notes.
// ---------------------------------------------------------------------------

func TestDbgDecodeLogsJSONArray(t *testing.T) {
	body := []byte(`[{"service":"svc-a","ddsource":"dd_debugger"},{"service":"svc-b","ddsource":"dd_debugger"}]`)
	got := dbgDecodeLogs("application/json", body)
	if len(got) != 2 {
		t.Fatalf("entries: got %d, want 2", len(got))
	}
	if !bytes.Contains(got[0], []byte("svc-a")) || !bytes.Contains(got[1], []byte("svc-b")) {
		t.Errorf("entries did not keep their raw content: %v", got)
	}
}

// The browser SDK's debugger track sends NDJSON, not a JSON array — this
// pins the fix rum-spec.md flags as needed for that producer to land here.
func TestDbgDecodeLogsNDJSON(t *testing.T) {
	body := []byte("{\"service\":\"svc-a\"}\n{\"service\":\"svc-b\"}\n")
	got := dbgDecodeLogs("text/plain", body)
	if len(got) != 2 {
		t.Fatalf("entries: got %d, want 2", len(got))
	}
	if !bytes.Contains(got[0], []byte("svc-a")) || !bytes.Contains(got[1], []byte("svc-b")) {
		t.Errorf("entries did not keep their raw content: %v", got)
	}
}

func TestDbgDecodeLogsNeitherShapeReturnsNil(t *testing.T) {
	if got := dbgDecodeLogs("text/plain", []byte("not json at all")); got != nil {
		t.Errorf("got %v, want nil for undecodable body", got)
	}
	if got := dbgDecodeLogs("text/plain", []byte("   ")); got != nil {
		t.Errorf("got %v, want nil for a blank body", got)
	}
}

// ---------------------------------------------------------------------------
// symdb: dbgDecodeSymdbFile + symdbUploadRow — the camelCase/snake_case drift
// between the event part and the file envelope, and independent envelope-vs-
// scopes nil-ness.
// ---------------------------------------------------------------------------

func TestSymdbUploadRowKeepsBothSpellings(t *testing.T) {
	envelope := gzipBytes(t, []byte(`{
		"service": "env-svc", "version": "9.9.9", "language": "go",
		"upload_id": "env-upload-1", "batch_num": "7", "final": true,
		"scopes": [{"scope_type": "package", "name": "main", "scopes": []}]
	}`))
	event := []byte(`{
		"service": "evt-svc", "version": "1.0.0", "language": "go",
		"runtimeId": "rt-1", "uploadId": "evt-upload-1", "batchNum": "3",
		"final": false, "attachmentSize": 4096
	}`)

	envMap := dbgDecodeSymdbEvent(event)
	env, scopes, inflated := dbgDecodeSymdbFile(envelope)
	row := symdbUploadRow("t", event, envelope, "env:prod,team:x", envMap, env, scopes, inflated)

	// The raw event part is the lossless copy — must survive byte for byte,
	// same as profiles.event / symbol_uploads.meta / debugger_logs.entry.
	if row.Event != string(event) {
		t.Errorf("Event does not match the raw event part bytes")
	}
	if got := row.DDTags["env"]; len(got) != 1 || got[0] != "prod" {
		t.Errorf("ddtags[env]: got %v, want [prod]", got)
	}
	if row.Service != "evt-svc" || row.EnvService != "env-svc" {
		t.Errorf("service drift lost: event=%q envelope=%q, want evt-svc/env-svc", row.Service, row.EnvService)
	}
	if row.UploadID != "evt-upload-1" || row.EnvUploadID != "env-upload-1" {
		t.Errorf("upload id drift lost: event=%q envelope=%q", row.UploadID, row.EnvUploadID)
	}
	if row.BatchNum != "3" || row.EnvBatchNum != "7" {
		t.Errorf("batch num drift lost: event=%q envelope=%q", row.BatchNum, row.EnvBatchNum)
	}
	if row.Final == nil || *row.Final != 0 {
		t.Errorf("final (event, false): got %v, want a present 0", row.Final)
	}
	if row.AttachmentSize == nil || *row.AttachmentSize != 4096 {
		t.Errorf("attachment_size: got %v, want 4096", row.AttachmentSize)
	}
	if row.ScopesOK != 1 || row.ScopeCount != 1 {
		t.Errorf("scopes: ok=%d count=%d, want 1/1", row.ScopesOK, row.ScopeCount)
	}
	if row.InflatedSize == 0 {
		t.Errorf("inflated_size: got 0, want > 0")
	}
	// The raw gzip'd bytes are the lossless copy and must be exactly what was
	// received, not the inflated content.
	if row.File != string(envelope) {
		t.Errorf("File does not match the original gzip'd bytes")
	}
}

// The envelope's "scopes" key can be absent while everything else about the
// envelope decoded fine — scopes_ok must say so without blanking env_*.
func TestDbgDecodeSymdbFileMissingScopesKey(t *testing.T) {
	envelope := gzipBytes(t, []byte(`{"service": "s", "upload_id": "u"}`))
	env, scopes, _ := dbgDecodeSymdbFile(envelope)
	if env == nil {
		t.Fatal("envelope should still have decoded")
	}
	if scopes != nil {
		t.Errorf("scopes: got %v, want nil (key absent)", scopes)
	}
	if got := rawJSONText(env, "service"); got != "s" {
		t.Errorf("envelope service: got %q, want s", got)
	}
}

// Not gzip at all: the whole thing fails, but the caller (symdbUploadRow)
// must still keep the raw bytes in File — the lossless fallback that makes
// this an acceptable failure mode rather than a silent drop.
func TestSymdbUploadRowUndecodableFilePreservesRawBytes(t *testing.T) {
	garbage := []byte("not gzip at all")
	env, scopes, inflated := dbgDecodeSymdbFile(garbage)
	row := symdbUploadRow("t", []byte(`{"service":"s"}`), garbage, "", nil, env, scopes, inflated)
	if row.File != string(garbage) {
		t.Errorf("File: got %q, want the raw undecodable bytes preserved", row.File)
	}
	if row.ScopesOK != 0 || row.EnvService != "" {
		t.Errorf("expected zero envelope-derived fields, got scopes_ok=%d env_service=%q", row.ScopesOK, row.EnvService)
	}
}

// A producer that sends uploadId as a bare JSON number (not a string) must
// not lose it: UploadID and BatchNum are both identifiers kept as literal
// wire text (rawText), never as a string-only extract (asString) that would
// silently blank a numeric value.
func TestSymdbUploadRowNumericUploadID(t *testing.T) {
	event := []byte(`{"uploadId": 42, "batchNum": 7}`)
	envMap := dbgDecodeSymdbEvent(event)
	row := symdbUploadRow("t", event, nil, "", envMap, nil, nil, 0)
	if row.UploadID != "42" {
		t.Errorf("upload_id: got %q, want \"42\" (a numeric uploadId must not blank to \"\")", row.UploadID)
	}
	if row.BatchNum != "7" {
		t.Errorf("batch_num: got %q, want \"7\"", row.BatchNum)
	}
}

// ---------------------------------------------------------------------------
// symbolUploadRow / elfInfo — a real ELF ident header and a non-ELF one.
// ---------------------------------------------------------------------------

func fakeELF(class, endian byte) []byte {
	// Real ELF idents run 16 bytes; this test only needs the first 6
	// (magic, class, data) that elfInfo actually reads.
	return append([]byte{0x7f, 'E', 'L', 'F', class, endian}, make([]byte, 10)...)
}

func TestElfInfo(t *testing.T) {
	if class, endian := elfInfo(fakeELF(2, 1)); class != "ELF64" || endian != "LE" {
		t.Errorf("ELF64/LE ident: got %q/%q", class, endian)
	}
	if class, endian := elfInfo(fakeELF(1, 2)); class != "ELF32" || endian != "BE" {
		t.Errorf("ELF32/BE ident: got %q/%q", class, endian)
	}
	if class, endian := elfInfo([]byte("not an elf file")); class != "" || endian != "" {
		t.Errorf("non-ELF bytes: got %q/%q, want empty/empty", class, endian)
	}
}

func TestSymbolUploadRowExtractsMetaAndOtherParts(t *testing.T) {
	meta := []byte(`{
		"type": "elf_symbol_file", "arch": "amd64",
		"gnu_build_id": "abc123", "go_build_id": "",
		"file_hash": "deadbeef", "symbol_source": "agent",
		"origin": "profiler", "origin_version": "7.60.0",
		"filename": "myapp"
	}`)
	parts := map[string][]byte{
		"event":           meta,
		"elf_symbol_file": fakeELF(2, 1),
		"debug_extra":     []byte("a part this router does not name"),
	}
	decoded := map[string]any{}
	_ = jsonDecode(meta, &decoded)

	row := symbolUploadRow("t", parts, decoded)
	if row.Type != "elf_symbol_file" || row.Arch != "amd64" || row.SymbolSource != "agent" {
		t.Errorf("meta fields: got type=%q arch=%q symbol_source=%q", row.Type, row.Arch, row.SymbolSource)
	}
	if row.HasELF != 1 || row.ELFClass != "ELF64" || row.ELFEndianness != "LE" {
		t.Errorf("elf ident: got has_elf=%d class=%q endianness=%q", row.HasELF, row.ELFClass, row.ELFEndianness)
	}
	if row.ELFSize != uint64(len(parts["elf_symbol_file"])) {
		t.Errorf("elf_size: got %d, want %d", row.ELFSize, len(parts["elf_symbol_file"]))
	}
	if row.Meta != string(meta) {
		t.Errorf("Meta is not the raw event bytes")
	}
	if got := row.OtherParts["debug_extra"]; got != "a part this router does not name" {
		t.Errorf("other_parts[debug_extra]: got %q", got)
	}
}

func TestSymbolUploadRowNoELFPart(t *testing.T) {
	parts := map[string][]byte{"event": []byte(`{"type":"x"}`)}
	row := symbolUploadRow("t", parts, map[string]any{"type": "x"})
	if row.HasELF != 0 || row.ELFClass != "" || row.ELFEndianness != "" || row.ELFSize != 0 {
		t.Errorf("no elf part sent: got has_elf=%d class=%q endianness=%q size=%d",
			row.HasELF, row.ELFClass, row.ELFEndianness, row.ELFSize)
	}
}

// ---------------------------------------------------------------------------
// Handler tests: real requests through the real engine, asserting on the
// rows that reached storage (the worked example in testserver_test.go).
// ---------------------------------------------------------------------------

func TestHandleProfileStoresRow(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeProfile)

	body, ct := buildMultipart(t, map[string][]byte{
		"event":     []byte(`{"family":"go","tags_profiler":"service:my-svc"}`),
		"cpu.pprof": buildPprof(t),
	})

	if w := postWithContentType(t, e, "/api/v2/profile", ct, body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.ProfileRow](node)
	if len(rows) != 1 {
		t.Fatalf("profile rows: got %d, want 1", len(rows))
	}
	if rows[0].TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", rows[0].TenantID, testTenant)
	}
	if rows[0].Family != "go" {
		t.Errorf("family: got %q, want go", rows[0].Family)
	}
	if len(rows[0].AttachName) != 1 || rows[0].AttachName[0] != "cpu.pprof" {
		t.Errorf("attachments: got %v", rows[0].AttachName)
	}
}

// A body that is not multipart at all on this endpoint must not vanish.
func TestHandleProfileMalformedBodyStoresRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeProfile)

	if w := postWithContentType(t, e, "/api/v2/profile", "text/plain", []byte("not multipart")); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.RawPayloadRow](node)
	if len(rows) != 1 {
		t.Fatalf("raw payload rows: got %d, want 1", len(rows))
	}
	if rows[0].Intake != "profiling" || rows[0].Reason != "decode_error" {
		t.Errorf("intake/reason: got %q/%q, want profiling/decode_error", rows[0].Intake, rows[0].Reason)
	}
}

func TestHandleDebuggerJSONVariantStoresRows(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeDebugger)

	body := []byte(`[{"service":"svc-a","ddsource":"dd_debugger","extra_one":1}]`)
	path := "/api/v2/debugger?ddtags=env:prod,team:x"
	if w := postWithContentType(t, e, path, "application/json", body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.DebuggerLogRow](node)
	if len(rows) != 1 {
		t.Fatalf("debugger log rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.TenantID != testTenant || r.Service != "svc-a" || r.DDSource != "dd_debugger" {
		t.Errorf("row: got tenant=%q service=%q ddsource=%q", r.TenantID, r.Service, r.DDSource)
	}
	if got := r.DDTags["env"]; len(got) != 1 || got[0] != "prod" {
		t.Errorf("ddtags[env]: got %v, want [prod]", got)
	}
	if len(r.ExtraKeys) != 1 || r.ExtraKeys[0] != "extra_one" {
		t.Errorf("extra_keys: got %v, want [extra_one]", r.ExtraKeys)
	}
}

func TestHandleDebuggerSymdbVariantStoresRow(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeDebugger)

	eventPart := []byte(`{"service":"s","uploadId":"u1","batchNum":"1","final":true}`)
	envelope := gzipBytes(t, []byte(`{"service":"s","upload_id":"u1","batch_num":"1","scopes":[]}`))
	body, ct := buildMultipart(t, map[string][]byte{
		"event": eventPart,
		"file":  envelope,
	})

	path := "/api/v2/debugger?ddtags=env:prod,team:x"
	if w := postWithContentType(t, e, path, ct, body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.SymdbUploadRow](node)
	if len(rows) != 1 {
		t.Fatalf("symdb upload rows: got %d, want 1", len(rows))
	}
	if rows[0].TenantID != testTenant || rows[0].UploadID != "u1" || rows[0].EnvUploadID != "u1" {
		t.Errorf("row: got tenant=%q upload_id=%q env_upload_id=%q", rows[0].TenantID, rows[0].UploadID, rows[0].EnvUploadID)
	}
	// The event part must survive whole, not just its 8 named fields.
	if rows[0].Event != string(eventPart) {
		t.Errorf("event: got %q, want the raw event part preserved", rows[0].Event)
	}
	if got := rows[0].DDTags["env"]; len(got) != 1 || got[0] != "prod" {
		t.Errorf("ddtags[env]: got %v, want [prod] — the query param must not be dropped for the symdb variant", got)
	}
}

func TestHandleDebuggerDiagnosticsVariantStoresRows(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeDebugger)

	diag := []byte(`[{
		"service": "svc", "ddsource": "dd_debugger", "timestamp": 1758623400,
		"debugger": {"diagnostics": {
			"runtimeId": "rt-1", "probeId": "probe-1", "status": "INSTALLED", "probeVersion": "3",
			"exception": {"type": "NPE", "message": "boom"}
		}}
	}]`)
	body, ct := buildMultipart(t, map[string][]byte{"event": diag})

	path := "/api/v2/debugger?ddtags=env:prod,team:x"
	if w := postWithContentType(t, e, path, ct, body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.DebuggerDiagnosticRow](node)
	if len(rows) != 1 {
		t.Fatalf("diagnostic rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.ProbeID != "probe-1" || r.Status != "INSTALLED" {
		t.Errorf("probe fields: got probe_id=%q status=%q", r.ProbeID, r.Status)
	}
	if r.ExceptionType == nil || *r.ExceptionType != "NPE" {
		t.Errorf("exception_type: got %v, want NPE", r.ExceptionType)
	}
	if r.Timestamp == nil {
		t.Errorf("timestamp: got nil, want a parsed value")
	}
	if got := r.DDTags["env"]; len(got) != 1 || got[0] != "prod" {
		t.Errorf("ddtags[env]: got %v, want [prod] — the query param must not be dropped for the diagnostics variant", got)
	}
}

// A diagnostics event part that is not a JSON array at all (a genuine decode
// failure, distinct from a legitimately empty "[]" batch) must reach
// raw_payloads, not vanish silently.
func TestHandleDebuggerDiagnosticsMalformedEventStoresRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeDebugger)

	body, ct := buildMultipart(t, map[string][]byte{"event": []byte(`{"not": "an array"}`)})
	if w := postWithContentType(t, e, "/api/v2/debugger", ct, body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.RawPayloadRow](node)
	if len(rows) != 1 {
		t.Fatalf("raw payload rows: got %d, want 1", len(rows))
	}
	if rows[0].Reason != "decode_error" {
		t.Errorf("reason: got %q, want decode_error", rows[0].Reason)
	}
	if diagRows := Rows[storage.DebuggerDiagnosticRow](node); len(diagRows) != 0 {
		t.Errorf("diagnostic rows: got %d, want 0 (the batch never decoded)", len(diagRows))
	}
}

// Neither file nor event: nothing this router recognises, so it must reach
// raw_payloads rather than being dropped silently.
func TestHandleDebuggerUnknownVariantStoresRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeDebugger)

	body, ct := buildMultipart(t, map[string][]byte{"mystery": []byte("???")})
	if w := postWithContentType(t, e, "/api/v2/debugger", ct, body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.RawPayloadRow](node)
	if len(rows) != 1 {
		t.Fatalf("raw payload rows: got %d, want 1", len(rows))
	}
	if rows[0].Reason != "unexpected_shape" {
		t.Errorf("reason: got %q, want unexpected_shape", rows[0].Reason)
	}
}

func TestHandleSourcemapStoresRow(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeSourcemap)

	body, ct := buildMultipart(t, map[string][]byte{
		"event":           []byte(`{"type":"elf_symbol_file","arch":"amd64","gnu_build_id":"abc"}`),
		"elf_symbol_file": fakeELF(2, 1),
	})

	if w := postWithContentType(t, e, "/api/v2/srcmap", ct, body); w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202", w.Code)
	}

	rows := Rows[storage.SymbolUploadRow](node)
	if len(rows) != 1 {
		t.Fatalf("symbol upload rows: got %d, want 1", len(rows))
	}
	if rows[0].Arch != "amd64" || rows[0].ELFClass != "ELF64" {
		t.Errorf("row: got arch=%q elf_class=%q", rows[0].Arch, rows[0].ELFClass)
	}
}

// ---------------------------------------------------------------------------
// Integration: real ClickHouse, real INSERTs, columns read back. Proves the
// migration, AppendTo and ClickHouse's own types agree for all five tables.
// ---------------------------------------------------------------------------

func TestProfilingTablesRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)

	t.Run("profiles", func(t *testing.T) {
		end := now.Add(30 * time.Second)
		storagetest.Insert(t, conn, storage.ProfilesWriter, []storage.Row{
			storage.ProfileRow{
				TenantID: "default", ReceivedAt: now, Variant: "profile",
				Event: `{"family":"go"}`, Family: "go", EndRaw: "x", EndParsed: &end,
				TagsProfiler:        map[string][]string{"service": {"my-svc"}},
				AttachName:          []string{"cpu.pprof"},
				AttachBytes:         []string{"gzipbytes"},
				AttachSize:          []uint64{9},
				AttachParsed:        []uint8{1},
				AttachSampleTypes:   [][]string{{"cpu"}},
				AttachSampleUnits:   [][]string{{"nanoseconds"}},
				AttachSampleCount:   []uint64{42},
				AttachTimeNanos:     []int64{1},
				AttachDurationNanos: []int64{2},
				AttachPeriodType:    []string{"cpu/nanoseconds"},
				AttachPeriod:        []int64{10},
				AttachMappingCount:  []uint32{0},
				AttachLocationCount: []uint32{3},
				AttachFunctionCount: []uint32{3},
			},
		})
		if got := storagetest.Count(t, conn, "profiles"); got != 1 {
			t.Fatalf("profiles: %d rows, want 1", got)
		}
		row := storagetest.QueryRow(t, conn, "SELECT family, attach_sample_count[1], service FROM profiles")
		if row[0] != "go" {
			t.Errorf("family: got %v", row[0])
		}
		if row[1] != uint64(42) {
			t.Errorf("attach_sample_count[1]: got %v (%T), want 42", row[1], row[1])
		}
		// service is MATERIALIZED from tags_profiler['service'][1].
		if row[2] != "my-svc" {
			t.Errorf("service (materialized): got %v, want my-svc", row[2])
		}
	})

	t.Run("debugger_logs", func(t *testing.T) {
		storagetest.Insert(t, conn, storage.DebuggerLogsWriter, []storage.Row{
			storage.DebuggerLogRow{
				TenantID: "default", ReceivedAt: now, Service: "svc", DDSource: "dd_debugger",
				DDTags: map[string][]string{"env": {"prod"}}, Entry: `{"a":1}`, ExtraKeys: []string{"a"},
			},
		})
		if got := storagetest.Count(t, conn, "debugger_logs"); got != 1 {
			t.Fatalf("debugger_logs: %d rows, want 1", got)
		}
		row := storagetest.QueryRow(t, conn, "SELECT service, ddsource, entry, extra_keys FROM debugger_logs")
		if row[0] != "svc" || row[1] != "dd_debugger" {
			t.Errorf("service/ddsource: got %v/%v", row[0], row[1])
		}
		if row[2] != `{"a":1}` {
			t.Errorf("entry: got %v, want the raw JSON preserved", row[2])
		}
		if !reflect.DeepEqual(row[3], []string{"a"}) {
			t.Errorf("extra_keys: got %v, want [a]", row[3])
		}
	})

	t.Run("debugger_diagnostics", func(t *testing.T) {
		ts := now
		excType := "NPE"
		excMsg := "boom"
		storagetest.Insert(t, conn, storage.DebuggerDiagnosticsWriter, []storage.Row{
			storage.DebuggerDiagnosticRow{
				TenantID: "default", ReceivedAt: now, Timestamp: &ts,
				Service: "svc", ProbeID: "p1", Status: "INSTALLED",
				DDTags:           map[string][]string{"env": {"prod"}},
				ExceptionType:    &excType,
				ExceptionMessage: &excMsg,
				Message:          `{"x":1}`,
			},
			// No exception on this one — proves Nullable(String) round-trips
			// NULL, checked below by an actual SELECT rather than asserted
			// only in this comment.
			storage.DebuggerDiagnosticRow{
				TenantID: "default", ReceivedAt: now, Service: "svc2", Message: `{}`,
			},
		})
		if got := storagetest.Count(t, conn, "debugger_diagnostics"); got != 2 {
			t.Fatalf("debugger_diagnostics: %d rows, want 2", got)
		}
		row := storagetest.QueryRow(t, conn,
			"SELECT exception_type, exception_message FROM debugger_diagnostics WHERE service = 'svc'")
		if isNullColumn(row[0]) || isNullColumn(row[1]) {
			t.Errorf("exception_type/message for svc: got %v/%v, want non-NULL", row[0], row[1])
		}
		noExc := storagetest.QueryRow(t, conn,
			"SELECT exception_type, exception_message FROM debugger_diagnostics WHERE service = 'svc2'")
		if !isNullColumn(noExc[0]) || !isNullColumn(noExc[1]) {
			t.Errorf("exception_type/message for svc2: got %v/%v, want NULL/NULL", noExc[0], noExc[1])
		}
	})

	t.Run("symdb_uploads", func(t *testing.T) {
		final := uint8(1)
		size := uint64(4096)
		storagetest.Insert(t, conn, storage.SymdbUploadsWriter, []storage.Row{
			storage.SymdbUploadRow{
				TenantID: "default", ReceivedAt: now, Service: "svc", UploadID: "u1",
				Event: `{"uploadId":"u1"}`, DDTags: map[string][]string{"env": {"prod"}},
				Final: &final, AttachmentSize: &size, File: "gzbytes",
				EnvUploadID: "u1-env",
			},
		})
		if got := storagetest.Count(t, conn, "symdb_uploads"); got != 1 {
			t.Fatalf("symdb_uploads: %d rows, want 1", got)
		}
		row := storagetest.QueryRow(t, conn, "SELECT upload_id, env_upload_id, final, event FROM symdb_uploads")
		if row[0] != "u1" || row[1] != "u1-env" {
			t.Errorf("upload_id/env_upload_id: got %v/%v — camelCase and snake_case spellings must both survive", row[0], row[1])
		}
		if row[3] != `{"uploadId":"u1"}` {
			t.Errorf("event: got %v, want the raw event part preserved", row[3])
		}
	})

	t.Run("symbol_uploads", func(t *testing.T) {
		storagetest.Insert(t, conn, storage.SymbolUploadsWriter, []storage.Row{
			storage.SymbolUploadRow{
				TenantID: "default", ReceivedAt: now, Arch: "amd64", HasELF: 1,
				ELFClass: "ELF64", ELFEndianness: "LE", ELF: "elfbytes", ELFSize: 8,
				OtherParts: map[string]string{"extra": "part"},
			},
		})
		if got := storagetest.Count(t, conn, "symbol_uploads"); got != 1 {
			t.Fatalf("symbol_uploads: %d rows, want 1", got)
		}
		row := storagetest.QueryRow(t, conn, "SELECT arch, elf_class, other_parts['extra'] FROM symbol_uploads")
		if row[0] != "amd64" || row[1] != "ELF64" {
			t.Errorf("arch/elf_class: got %v/%v", row[0], row[1])
		}
		if row[2] != "part" {
			t.Errorf("other_parts['extra']: got %v, want part", row[2])
		}
	})
}

// isNullColumn reports whether a storagetest.QueryRow value scanned a SQL
// NULL — a nil interface for most drivers scans, or a typed nil pointer
// (e.g. a nil *string) for a Nullable column, which `v != nil` alone would
// get wrong (a non-nil interface holding a nil pointer is still != nil).
func isNullColumn(v any) bool {
	if v == nil {
		return true
	}
	rv := reflect.ValueOf(v)
	return rv.Kind() == reflect.Ptr && rv.IsNil()
}

func profTimePtr(t time.Time) *time.Time { return &t }

// jsonNum builds a json.Number the way jsonDecode would have (from decoding
// a bare number with UseNumber), for tests that exercise the epoch-number
// branch of profParseTimestamp without going through a full JSON decode.
func jsonNum(s string) any {
	var v any
	_ = jsonDecode([]byte(s), &v)
	return v
}
