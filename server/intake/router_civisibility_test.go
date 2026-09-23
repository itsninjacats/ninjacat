package intake

import (
	"bytes"
	"encoding/json"
	"fmt"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"net/textproto"
	"reflect"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/tinylib/msgp/msgp"

	"github.com/itsninjacats/server/apps/storage"
)

// CI Visibility decoder and handler tests.
//
// The payloads below are built from the wire structs of the clients that
// send them (dd-trace-go ddtrace/tracer/civisibility_tslv.go for the event
// envelope, internal/civisibility/utils/net/*.go for the JSON:API
// endpoints), not from a capture — there is no captured CI Visibility
// traffic yet, which is exactly why the response contracts are asserted key
// by key here: a renamed JSON tag compiles, passes a round-trip against our
// own struct, and then silently changes what a customer's test suite does.

// ---------------------------------------------------------------------------
// Test payload builders

// civTestEvent builds one event of a citestcycle payload.
func civTestEvent(eventType string, version int64, content map[string]any) map[string]any {
	return map[string]any{"type": eventType, "version": version, "content": content}
}

// civTestPayload is the four-event payload a small test session produces:
// one session end, one module end, one suite end and one test. Ids are the
// ones dd-trace-go's own doc comment uses, so a reader can line the two up.
func civTestPayload() map[string]any {
	const (
		sessionID = uint64(123456789)
		moduleID  = uint64(234567890)
		suiteID   = uint64(123123123)
	)
	common := func() map[string]any {
		return map[string]any{
			"service":  "my-service",
			"error":    int64(0),
			"start":    int64(1654698415668011500),
			"duration": int64(796143),
		}
	}

	session := common()
	session["type"] = "test_session_end"
	session["test_session_id"] = sessionID
	session["name"] = "go.test_session"
	session["resource"] = "test_session.my-service"
	session["meta"] = map[string]any{
		"test.command":            "go test ./...",
		"test_session.name":       "unit-tests",
		"git.repository_url":      "https://github.com/itsninjacats/ninjacat",
		"git.branch":              "feat/write-layer",
		"git.commit.sha":          "0f4c2a1b9e7d6c5a4b3e2d1c0f9e8d7c6b5a4938",
		"git.commit.author.name":  "Michal",
		"git.commit.author.email": "michal@example.test",
		"ci.provider.name":        "github",
		"ci.pipeline.id":          "9911",
		"ci.pipeline.number":      "42",
		"ci.job.name":             "unit",
		"ci.workspace_path":       "/home/runner/work/ninjacat",
		"os.platform":             "linux",
		"os.architecture":         "arm64",
		"runtime.name":            "go",
		"runtime.version":         "go1.26.3",
	}

	module := common()
	module["type"] = "test_module_end"
	module["test_session_id"] = sessionID
	module["test_module_id"] = moduleID
	module["name"] = "go.test_module"
	module["resource"] = "intake"
	module["meta"] = map[string]any{"test.module": "intake"}

	suite := common()
	suite["type"] = "test_suite_end"
	suite["test_session_id"] = sessionID
	suite["test_module_id"] = moduleID
	suite["test_suite_id"] = suiteID
	suite["name"] = "go.test_suite"
	suite["resource"] = "router_civisibility_test.go"
	suite["meta"] = map[string]any{
		"test.module": "intake",
		"test.suite":  "router_civisibility_test.go",
	}

	test := common()
	test["type"] = "test"
	test["test_session_id"] = sessionID
	test["test_module_id"] = moduleID
	test["test_suite_id"] = suiteID
	test["trace_id"] = uint64(1111111111111111111)
	test["span_id"] = uint64(2222222222222222222)
	test["parent_id"] = uint64(0)
	test["itr_correlation_id"] = "corr-1"
	test["name"] = "go.test"
	test["resource"] = "router_civisibility_test.go.TestCITestCycle"
	test["meta"] = map[string]any{
		"test.name":              "TestCITestCycle",
		"test.suite":             "router_civisibility_test.go",
		"test.module":            "intake",
		"test.framework":         "golang.org/pkg/testing",
		"test.framework_version": "go1.26.3",
		"test.status":            "pass",
		"test.type":              "test",
		"test.source.file":       "intake/router_civisibility_test.go",
		"test.codeowners":        `["@itsninjacats/backend"]`,
		"test.is_retry":          "true",
		"test.is_new":            "false",
		"test.retry_reason":      "early_flake_detection",
	}
	test["metrics"] = map[string]any{
		"test.source.start":   float64(120),
		"test.source.end":     float64(180),
		"_dd.host.vcpu_count": float64(8),
	}

	return map[string]any{
		"version": int64(1),
		"metadata": map[string]any{
			"*": map[string]any{
				"language":        "go",
				"runtime-id":      "6f2b1d0e-1111-2222-3333-444455556666",
				"library_version": "2.4.0",
				"env":             "ci",
			},
			"test": map[string]any{
				"library_version": "2.4.0-test-override",
			},
		},
		// Each event is {type, version, content} — the content maps above are
		// the CONTENT, not the event. version is 2 for a test and 1 for
		// everything else (civisibility_tslv.go:298 vs :323/:348/:371).
		"events": []any{
			civTestEvent("test_session_end", 1, session),
			civTestEvent("test_module_end", 1, module),
			civTestEvent("test_suite_end", 1, suite),
			civTestEvent("test", 2, test),
		},
	}
}

// civMsgpack encodes a decoded-shaped value the way the tracer's msgp codec
// would. AppendIntf is msgp's own generic encoder, so this exercises the
// real format rather than a hand-rolled approximation.
func civMsgpack(t *testing.T, v any) []byte {
	t.Helper()
	b, err := msgp.AppendIntf(nil, v)
	if err != nil {
		t.Fatalf("encode msgpack: %v", err)
	}
	return b
}

func civJSONBytes(t *testing.T, v any) []byte {
	t.Helper()
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("encode json: %v", err)
	}
	return b
}

// postCT is post with a content type of the caller's choosing — the base
// helper always sends application/json, and half of this protocol does not.
func postCT(t *testing.T, e *gin.Engine, path, contentType string, body []byte,
	headers map[string]string) *httptest.ResponseRecorder {
	t.Helper()

	req := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(body))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", contentType)
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	return w
}

// ---------------------------------------------------------------------------
// citestcycle

// The same session, encoded both ways, must produce identical rows. msgpack
// is what a tracer sends; JSON is what a hand-rolled client or a replayed
// capture sends, and dd-trace-go's own bazel mode converts one to the other.
// A decoder that only agreed with itself per format would be two decoders.
func TestCITestCycleDecodesMsgpackAndJSON(t *testing.T) {
	payload := civTestPayload()

	cases := []struct {
		name        string
		contentType string
		body        []byte
	}{
		{"msgpack", "application/msgpack", civMsgpack(t, payload)},
		{"json", "application/json", civJSONBytes(t, payload)},
		// No content type at all: the sniffer has to pick the format from the
		// first byte, because the coverage endpoint next door proves the
		// header cannot be relied on.
		{"sniffed msgpack", "", civMsgpack(t, payload)},
		{"sniffed json", "", civJSONBytes(t, payload)},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeCITestCycle)

			if w := postCT(t, e, "/api/v2/citestcycle", tc.contentType, tc.body, nil); w.Code != http.StatusAccepted {
				t.Fatalf("POST /api/v2/citestcycle: got %d, want 202 (%s)", w.Code, w.Body.String())
			}

			rows := Rows[storage.CITestEventRow](node)
			if len(rows) != 4 {
				t.Fatalf("stored rows: got %d, want 4 — every event in the envelope becomes a row", len(rows))
			}

			byType := map[string]storage.CITestEventRow{}
			for _, r := range rows {
				byType[r.EventType] = r
				if r.TenantID != testTenant {
					t.Errorf("%s tenant: got %q, want %q", r.EventType, r.TenantID, testTenant)
				}
				if r.PayloadVersion != 1 {
					t.Errorf("%s payload_version: got %d, want 1", r.EventType, r.PayloadVersion)
				}
				if r.Service != "my-service" {
					t.Errorf("%s service: got %q", r.EventType, r.Service)
				}
				if r.DurationNs != 796143 {
					t.Errorf("%s duration_ns: got %d, want 796143", r.EventType, r.DurationNs)
				}
				// 1654698415668011500 ns is 2022-06-08T14:26:55.6680115Z. The
				// column is DateTime64(9) precisely so the last three digits
				// survive; a ms-truncating conversion would make two tests in
				// the same millisecond unorderable.
				if got := r.Start.UnixNano(); got != 1654698415668011500 {
					t.Errorf("%s start: got %d ns, want 1654698415668011500 — nanosecond precision must survive",
						r.EventType, got)
				}
			}

			// Which ids each event type carries is structural, not optional:
			// the tracer promotes them out of meta per type (a test has all
			// three, a session end only its own).
			session := byType["test_session_end"]
			if session.SessionID != 123456789 || session.ModuleID != 0 || session.SuiteID != 0 {
				t.Errorf("test_session_end ids: got session=%d module=%d suite=%d, want 123456789/0/0",
					session.SessionID, session.ModuleID, session.SuiteID)
			}
			if session.GitBranch != "feat/write-layer" || session.CIProviderName != "github" {
				t.Errorf("session git/ci hot fields: branch=%q provider=%q",
					session.GitBranch, session.CIProviderName)
			}
			if session.OSPlatform != "linux" || session.RuntimeVersion != "go1.26.3" {
				t.Errorf("session os/runtime hot fields: platform=%q runtime=%q",
					session.OSPlatform, session.RuntimeVersion)
			}

			module := byType["test_module_end"]
			if module.SessionID != 123456789 || module.ModuleID != 234567890 || module.SuiteID != 0 {
				t.Errorf("test_module_end ids: got %d/%d/%d, want 123456789/234567890/0",
					module.SessionID, module.ModuleID, module.SuiteID)
			}

			suite := byType["test_suite_end"]
			if suite.SuiteID != 123123123 {
				t.Errorf("test_suite_end suite id: got %d, want 123123123", suite.SuiteID)
			}
			// Only test and span events carry span/trace ids; msgp omits them
			// for the *_end types.
			if suite.SpanID != 0 || suite.TraceID != 0 {
				t.Errorf("test_suite_end must carry no span/trace id, got span=%d trace=%d",
					suite.SpanID, suite.TraceID)
			}

			test := byType["test"]
			if test.SpanID != 2222222222222222222 || test.TraceID != 1111111111111111111 {
				t.Errorf("test span/trace: got %d/%d", test.SpanID, test.TraceID)
			}
			if test.ITRCorrelationID != "corr-1" {
				t.Errorf("itr_correlation_id: got %q, want corr-1", test.ITRCorrelationID)
			}
			if test.TestName != "TestCITestCycle" || test.TestStatus != "pass" ||
				test.TestFramework != "golang.org/pkg/testing" {
				t.Errorf("test hot fields: name=%q status=%q framework=%q",
					test.TestName, test.TestStatus, test.TestFramework)
			}
			// test.source.start arrives in metrics (a numeric SetTag), not in
			// meta — a decoder reading only meta would leave both NULL.
			if test.TestSourceStart == nil || *test.TestSourceStart != 120 {
				t.Errorf("test_source_start: got %v, want 120 (it lives in metrics, not meta)", test.TestSourceStart)
			}
			if test.TestSourceEnd == nil || *test.TestSourceEnd != 180 {
				t.Errorf("test_source_end: got %v, want 180", test.TestSourceEnd)
			}
			// Absent means NULL, not false: the tracer only writes these tags
			// when the feature is on.
			if test.TestIsRetry == nil || *test.TestIsRetry != 1 {
				t.Errorf("test_is_retry: got %v, want 1", test.TestIsRetry)
			}
			if test.TestIsNew == nil || *test.TestIsNew != 0 {
				t.Errorf("test_is_new: got %v, want 0 (the tag said false)", test.TestIsNew)
			}
			if test.TestSkippedByITR != nil {
				t.Errorf("test_skipped_by_itr: got %v, want nil — the tag was never sent", test.TestSkippedByITR)
			}
			if test.TestRetryReason != "early_flake_detection" {
				t.Errorf("test_retry_reason: got %q", test.TestRetryReason)
			}

			// meta and metrics survive whole; the hot columns are copies, not
			// extractions.
			if test.Meta["test.codeowners"] != `["@itsninjacats/backend"]` {
				t.Errorf("meta must be kept verbatim, got %q", test.Meta["test.codeowners"])
			}
			if got := test.Metrics["_dd.host.vcpu_count"]; got != 8 {
				t.Errorf("metrics must be kept verbatim, _dd.host.vcpu_count = %v", got)
			}

			// Envelope metadata: "*" merged under the per-type entry, with
			// the per-type entry winning.
			if test.Language != "go" || test.RuntimeID != "6f2b1d0e-1111-2222-3333-444455556666" {
				t.Errorf("envelope metadata did not reach the row: language=%q runtime_id=%q",
					test.Language, test.RuntimeID)
			}
			if test.LibraryVersion != "2.4.0-test-override" {
				t.Errorf("library_version: got %q, want the per-type metadata entry to win over \"*\"",
					test.LibraryVersion)
			}
			if session.LibraryVersion != "2.4.0" {
				t.Errorf("session library_version: got %q, want the \"*\" entry", session.LibraryVersion)
			}
			if test.Env != "ci" {
				t.Errorf("env: got %q, want ci (from the \"*\" metadata entry)", test.Env)
			}
			if test.Metadata["language"] != "go" {
				t.Errorf("metadata column: got %v", test.Metadata)
			}

			// content is JSON whichever format arrived, and it round-trips.
			var content map[string]any
			if err := json.Unmarshal([]byte(test.Content), &content); err != nil {
				t.Fatalf("content is not JSON: %v (%q)", err, test.Content)
			}
			if content["resource"] != "router_civisibility_test.go.TestCITestCycle" {
				t.Errorf("content lost a field: %v", content["resource"])
			}
		})
	}
}

// A session id above 2^53 must survive, and this is the one bug in this file
// that cannot be caught by reading the code: JSON decoded without UseNumber,
// or msgpack read through a float64 helper, rounds 9007199254740993 to
// ...992 and the row then joins to nothing. Both formats are exercised
// because they reach the value through completely different decoders.
func TestCITestCycleKeepsFullWidthIDs(t *testing.T) {
	const bigID = uint64(9007199254740993) // 2^53 + 1

	payload := map[string]any{
		"version": int64(1),
		"events": []any{civTestEvent("test", 2, map[string]any{
			"type":            "test",
			"test_session_id": bigID,
			"span_id":         uint64(18446744073709551615), // math.MaxUint64
			"service":         "s",
			"start":           int64(1654698415668011500),
			"duration":        int64(1),
			"error":           int64(0),
		})},
	}

	cases := []struct {
		name        string
		contentType string
		body        []byte
	}{
		{"msgpack", "application/msgpack", civMsgpack(t, payload)},
		{"json", "application/json", civJSONBytes(t, payload)},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeCITestCycle)
			postCT(t, e, "/api/v2/citestcycle", tc.contentType, tc.body, nil)

			rows := Rows[storage.CITestEventRow](node)
			if len(rows) != 1 {
				t.Fatalf("rows: got %d, want 1", len(rows))
			}
			if rows[0].SessionID != bigID {
				t.Errorf("session_id: got %d, want %d — an id that went through float64 loses its last digits",
					rows[0].SessionID, bigID)
			}
			if rows[0].SpanID != 18446744073709551615 {
				t.Errorf("span_id: got %d, want MaxUint64", rows[0].SpanID)
			}
		})
	}
}

// The EVP-proxy headers say which of the two transports a payload took, and
// three of the four names are not the obvious ones — the container id has no
// X- prefix and the agent version is only in Via.
func TestCITestCycleKeepsAgentHeaders(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCITestCycle)

	postCT(t, e, "/api/v2/citestcycle", "application/msgpack",
		civMsgpack(t, civTestPayload()), map[string]string{
			"X-Datadog-EVP-Subdomain": "citestcycle-intake",
			"Datadog-Container-ID":    "abc123",
			"X-Datadog-Hostname":      "runner-7",
			"Via":                     "trace-agent 7.83.0",
		})

	rows := Rows[storage.CITestEventRow](node)
	if len(rows) == 0 {
		t.Fatal("no rows stored")
	}
	r := rows[0]
	if r.EVPSubdomain != "citestcycle-intake" || r.ContainerID != "abc123" || r.AgentHostname != "runner-7" {
		t.Errorf("evp headers: subdomain=%q container=%q hostname=%q",
			r.EVPSubdomain, r.ContainerID, r.AgentHostname)
	}
	if r.AgentVersion != "7.83.0" {
		t.Errorf("agent_version: got %q, want 7.83.0 — the proxy announces itself in Via, not in a header of its own",
			r.AgentVersion)
	}
}

// A body no decoder can read must reach raw_payloads rather than vanish: the
// citestcycle transport has no retry at all, so a payload we drop is gone
// from the tracer's point of view too.
func TestCITestCycleStoresUndecodableBodiesRaw(t *testing.T) {
	cases := []struct{ name, contentType, body string }{
		{"not msgpack", "application/msgpack", "definitely not msgpack"},
		{"not json", "application/json", "{definitely not json"},
		{"array instead of envelope", "application/json", `[1,2,3]`},
		{"envelope with no events", "application/json", `{"version":1,"metadata":{}}`},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeCITestCycle)

			if w := postCT(t, e, "/api/v2/citestcycle", tc.contentType, []byte(tc.body), nil); w.Code != http.StatusAccepted {
				t.Errorf("got %d, want 202 — a rejected payload is one the tracer never retries", w.Code)
			}
			raw := Rows[storage.RawPayloadRow](node)
			if len(raw) != 1 {
				t.Fatalf("raw rows: got %d, want 1", len(raw))
			}
			if raw[0].Intake != "citestcycle" {
				t.Errorf("intake label: got %q, want citestcycle", raw[0].Intake)
			}
			if len(Rows[storage.CITestEventRow](node)) != 0 {
				t.Errorf("an undecodable payload must produce no event rows")
			}
		})
	}
}

// ---------------------------------------------------------------------------
// citestcov

// civMultipart builds a multipart body with explicit per-part content types,
// which mime/multipart's CreateFormFile cannot do.
func civMultipart(t *testing.T, parts []civPart) (body []byte, contentType string) {
	t.Helper()
	var buf bytes.Buffer
	w := multipart.NewWriter(&buf)
	for _, p := range parts {
		h := textproto.MIMEHeader{}
		disp := fmt.Sprintf(`form-data; name=%q`, p.Name)
		if p.FileName != "" {
			disp += fmt.Sprintf(`; filename=%q`, p.FileName)
		}
		h.Set("Content-Disposition", disp)
		if p.ContentType != "" {
			h.Set("Content-Type", p.ContentType)
		}
		part, err := w.CreatePart(h)
		if err != nil {
			t.Fatalf("create part %q: %v", p.Name, err)
		}
		if _, err := part.Write(p.Data); err != nil {
			t.Fatalf("write part %q: %v", p.Name, err)
		}
	}
	if err := w.Close(); err != nil {
		t.Fatalf("close multipart: %v", err)
	}
	return buf.Bytes(), w.FormDataContentType()
}

func civCoveragePayload() map[string]any {
	return map[string]any{
		"version": int64(2),
		"coverages": []any{
			map[string]any{
				"test_session_id": uint64(123456789),
				"test_suite_id":   uint64(123123123),
				"span_id":         uint64(2222222222222222222),
				"files": []any{
					map[string]any{"filename": "intake/router_civisibility.go", "bitmap": []byte{0x01, 0x02, 0x03}},
					map[string]any{"filename": "intake/raw.go"}, // no bitmap: file-level coverage only
				},
			},
			// Suite-level coverage: the encoder omits span_id entirely.
			map[string]any{
				"test_session_id": uint64(123456789),
				"test_suite_id":   uint64(999),
				"files":           []any{map[string]any{"filename": "apps/storage/rows_ci.go"}},
			},
		},
	}
}

// The coverage part is called "coveragex" by dd-trace-go and "coverage1" by
// dd-trace-py, and both are shipped clients — so the decoder must find it by
// content type, never by form name. That is the single most load-bearing fact
// about this endpoint and it gets its own table.
func TestCITestCovFindsCoveragePartByContentType(t *testing.T) {
	payload := civCoveragePayload()

	cases := []struct {
		name     string
		partName string
		partCT   string
		partData []byte
		wantFmt  string
	}{
		{"dd-trace-go coveragex msgpack", "coveragex", "application/msgpack", civMsgpack(t, payload), "msgpack"},
		{"dd-trace-py coverage1 msgpack", "coverage1", "application/msgpack", civMsgpack(t, payload), "msgpack"},
		{"json variant", "coveragex", "application/json", civJSONBytes(t, payload), "json"},
		{"no content type, sniffed", "coverageX", "", civMsgpack(t, payload), "msgpack"},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeCITestCov)

			body, ct := civMultipart(t, []civPart{
				{Name: "event", FileName: "fileevent.json", ContentType: "application/json",
					Data: []byte(`{"dummy": true}`)},
				{Name: tc.partName, FileName: "filecoveragex", ContentType: tc.partCT, Data: tc.partData},
			})

			if w := postCT(t, e, "/api/v2/citestcov", ct, body, nil); w.Code != http.StatusAccepted {
				t.Fatalf("POST /api/v2/citestcov: got %d, want 202 (%s)", w.Code, w.Body.String())
			}

			rows := Rows[storage.CICoverageRow](node)
			if len(rows) != 2 {
				t.Fatalf("coverage rows: got %d, want 2 — one per coverages[] entry", len(rows))
			}

			first := rows[0]
			if first.TenantID != testTenant {
				t.Errorf("tenant: got %q", first.TenantID)
			}
			if first.PayloadVersion != 2 {
				t.Errorf("payload_version: got %d, want 2", first.PayloadVersion)
			}
			if first.RawFormat != tc.wantFmt {
				t.Errorf("raw_format: got %q, want %q", first.RawFormat, tc.wantFmt)
			}
			if first.SessionID != 123456789 || first.SuiteID != 123123123 {
				t.Errorf("ids: session=%d suite=%d", first.SessionID, first.SuiteID)
			}
			if first.SpanID == nil || *first.SpanID != 2222222222222222222 {
				t.Errorf("span_id: got %v, want 2222222222222222222", first.SpanID)
			}
			// Parallel arrays stay in step: a file with no bitmap keeps an
			// empty string at its index rather than shifting the two apart.
			wantNames := []string{"intake/router_civisibility.go", "intake/raw.go"}
			if !reflect.DeepEqual(first.FilesFilename, wantNames) {
				t.Errorf("files_filename: got %v, want %v", first.FilesFilename, wantNames)
			}
			if len(first.FilesBitmap) != len(first.FilesFilename) {
				t.Fatalf("files_bitmap has %d entries for %d filenames — the arrays must stay in step",
					len(first.FilesBitmap), len(first.FilesFilename))
			}
			if first.FilesBitmap[1] != "" {
				t.Errorf("a file with no bitmap must keep an empty string, got %q", first.FilesBitmap[1])
			}
			if first.FilesBitmap[0] == "" {
				t.Error("the bitmap bytes were dropped — they are opaque but they are the payload")
			}
			if first.Event != `{"dummy": true}` {
				t.Errorf("event part: got %q", first.Event)
			}

			// Second entry: suite-level coverage, no span at all. NULL, not 0
			// — 0 is a legal span id.
			if rows[1].SpanID != nil {
				t.Errorf("span_id: got %v, want nil — the encoder omits it in suite-skipping mode", rows[1].SpanID)
			}

			// Each row's raw bytes are ITS OWN entry, not the whole part:
			// storing the part per row would cost the square of the payload.
			if len(first.Raw) == 0 || len(rows[1].Raw) == 0 {
				t.Fatal("raw is empty")
			}
			if first.Raw == rows[1].Raw {
				t.Error("both rows kept the same bytes — raw must be delimited per coverages[] entry")
			}
			if len(first.Raw) >= len(tc.partData) {
				t.Errorf("raw (%d B) is not smaller than the whole part (%d B) — it was not delimited",
					len(first.Raw), len(tc.partData))
			}
		})
	}
}

// A multipart with no coverage part at all, and a coverage part that does
// not decode, both have to reach raw_payloads.
func TestCITestCovStoresUndecodablePayloadsRaw(t *testing.T) {
	cases := []struct {
		name  string
		parts []civPart
	}{
		{"event only", []civPart{
			{Name: "event", ContentType: "application/json", Data: []byte(`{"dummy": true}`)},
		}},
		{"coverage part is garbage", []civPart{
			{Name: "event", ContentType: "application/json", Data: []byte(`{"dummy": true}`)},
			{Name: "coveragex", ContentType: "application/msgpack", Data: []byte("not msgpack at all")},
		}},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeCITestCov)
			body, ct := civMultipart(t, tc.parts)

			if w := postCT(t, e, "/api/v2/citestcov", ct, body, nil); w.Code != http.StatusAccepted {
				t.Errorf("got %d, want 202", w.Code)
			}
			if got := len(Rows[storage.RawPayloadRow](node)); got != 1 {
				t.Errorf("raw rows: got %d, want 1", got)
			}
			if got := len(Rows[storage.CICoverageRow](node)); got != 0 {
				t.Errorf("coverage rows: got %d, want 0", got)
			}
		})
	}

	// A body that is not multipart at all takes the same path.
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCITestCov)
	postCT(t, e, "/api/v2/citestcov", "application/json", []byte(`{"not":"multipart"}`), nil)
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 || raw[0].Intake != "citestcov" {
		t.Errorf("a non-multipart body must reach raw_payloads under the citestcov label, got %d rows", len(raw))
	}
}

// ---------------------------------------------------------------------------
// The four endpoints a tracer CONSULTS
//
// These assertions read the response as a map and check keys by name rather
// than unmarshalling into our own struct. That is deliberate: a test that
// round-trips through the same struct that produced the body cannot catch a
// renamed JSON tag, and a renamed tag here is exactly the failure that
// changes what a customer's test suite does.

func civSettingsRequestBody() []byte {
	return []byte(`{"data":{"id":"req-1","type":"ci_app_test_service_libraries_settings",
		"attributes":{"service":"my-service","env":"ci",
		"repository_url":"https://github.com/itsninjacats/ninjacat",
		"branch":"feat/write-layer","sha":"0f4c2a1b",
		"configurations":{"os.platform":"linux","runtime.name":"go","custom":{"team":"backend"}}}}}`)
}

// Every Test Optimization feature must be reported OFF, with every key
// present and spelled the way the client's struct tags spell it. Each flag
// individually gates one of the other endpoints, so this one response is
// what keeps a real tracer from asking us questions we cannot answer.
func TestCISettingsReportsEveryFeatureOff(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIVisibilityAPI)

	w := post(t, e, "/api/v2/libraries/tests/services/setting", civSettingsRequestBody())
	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200 (%s)", w.Code, w.Body.String())
	}
	if ct := w.Header().Get("Content-Type"); !bytes.Contains([]byte(ct), []byte("json")) {
		t.Errorf("content type %q — the client sets ExpectJSONResponse and retries anything else", ct)
	}

	var body map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &body); err != nil {
		t.Fatalf("response is not JSON: %v", err)
	}
	data, _ := body["data"].(map[string]any)
	if data == nil {
		t.Fatalf("response has no data object: %s", w.Body.String())
	}
	if data["id"] != "req-1" {
		t.Errorf("data.id: got %v, want the request's id echoed back", data["id"])
	}
	attrs, _ := data["attributes"].(map[string]any)
	if attrs == nil {
		t.Fatalf("response has no data.attributes: %s", w.Body.String())
	}

	// Field for field against dd-trace-go's SettingsResponseData. A key that
	// is MISSING is as bad as one that is true: the client would read its
	// zero value today and something else the day the struct changes.
	for _, key := range []string{
		"code_coverage", "coverage_report_upload_enabled", "flaky_test_retries_enabled",
		"itr_enabled", "require_git", "tests_skipping", "known_tests_enabled",
		"impacted_tests_enabled",
	} {
		v, ok := attrs[key]
		if !ok {
			t.Errorf("attributes.%s is missing", key)
			continue
		}
		if v != false {
			t.Errorf("attributes.%s: got %v, want false — enabling a feature we have not built "+
				"makes a customer's suite act on data we do not have", key, v)
		}
	}

	efd, _ := attrs["early_flake_detection"].(map[string]any)
	if efd == nil {
		t.Fatalf("attributes.early_flake_detection is missing")
	}
	if efd["enabled"] != false {
		t.Errorf("early_flake_detection.enabled: got %v, want false", efd["enabled"])
	}
	if _, ok := efd["faulty_session_threshold"]; !ok {
		t.Error("early_flake_detection.faulty_session_threshold is missing — the client's field is *int and null is the value that means no threshold")
	}
	slow, _ := efd["slow_test_retries"].(map[string]any)
	if slow == nil {
		t.Fatalf("early_flake_detection.slow_test_retries is missing")
	}
	// The four keys are literally "5s", "10s", "30s", "5m".
	for _, key := range []string{"5s", "10s", "30s", "5m"} {
		if _, ok := slow[key]; !ok {
			t.Errorf("slow_test_retries.%s is missing", key)
		}
	}

	tm, _ := attrs["test_management"].(map[string]any)
	if tm == nil {
		t.Fatalf("attributes.test_management is missing")
	}
	if tm["enabled"] != false {
		t.Errorf("test_management.enabled: got %v, want false", tm["enabled"])
	}
	if tm["attempt_to_fix_retries"] != float64(0) {
		t.Errorf("test_management.attempt_to_fix_retries: got %v, want 0", tm["attempt_to_fix_retries"])
	}

	// The question and our answer both reach the table.
	rows := Rows[storage.CISettingsRequestRow](node)
	if len(rows) != 1 {
		t.Fatalf("settings rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.Endpoint != "settings" || r.RequestID != "req-1" {
		t.Errorf("endpoint/request_id: got %q/%q", r.Endpoint, r.RequestID)
	}
	if r.Service != "my-service" || r.Env != "ci" || r.Branch != "feat/write-layer" || r.SHA != "0f4c2a1b" {
		t.Errorf("attributes did not reach the row: %+v", r)
	}
	if !bytes.Contains([]byte(r.Configurations), []byte(`"custom"`)) {
		t.Errorf("configurations: got %q, want the nested custom object kept", r.Configurations)
	}
	if r.RequestBody == "" || r.ResponseBody == "" {
		t.Error("both the request and the answer we gave must be stored")
	}
	if !bytes.Contains([]byte(r.ResponseBody), []byte(`"itr_enabled":false`)) {
		t.Errorf("response_body does not look like the answer we sent: %q", r.ResponseBody)
	}
}

func TestCISkippableTestsSkipsNothing(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIVisibilityAPI)

	body := []byte(`{"data":{"type":"test_params","attributes":{"test_level":"test",
		"service":"my-service","env":"ci","repository_url":"https://git/x","sha":"abc",
		"configurations":{}}}}`)
	w := post(t, e, "/api/v2/ci/tests/skippable", body)
	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200 (%s)", w.Code, w.Body.String())
	}

	var got map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &got); err != nil {
		t.Fatalf("response is not JSON: %v", err)
	}
	data, ok := got["data"].([]any)
	if !ok {
		t.Fatalf("data is not an array: %s", w.Body.String())
	}
	if len(data) != 0 {
		t.Errorf("data: got %d entries, want 0 — deciding a test is safe to skip needs coverage joined "+
			"against a diff, and neither half exists", len(data))
	}
	// meta is SENT, not omitted: dd-trace-py treats a missing meta as a
	// malformed response and gives up on the whole call.
	meta, ok := got["meta"].(map[string]any)
	if !ok {
		t.Fatalf("meta is missing — the python client needs it to be present")
	}
	if _, ok := meta["correlation_id"]; !ok {
		t.Error("meta.correlation_id is missing")
	}
	if _, ok := meta["coverage"]; !ok {
		t.Error("meta.coverage is missing")
	}

	rows := Rows[storage.CISettingsRequestRow](node)
	if len(rows) != 1 || rows[0].Endpoint != "skippable" || rows[0].TestLevel != "test" {
		t.Errorf("skippable row: %+v", rows)
	}
}

// has_next false is the one field in this file where the wrong answer is
// worse than useless: dd-trace-go's pagination loop has no iteration cap and
// keeps POSTing for as long as has_next is true.
func TestCIKnownTestsTerminatesPagination(t *testing.T) {
	for _, path := range []string{"/api/v2/ci/libraries/tests", "/api/v2/ci/libraries/tests/flaky"} {
		t.Run(path, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeCIVisibilityAPI)

			body := []byte(`{"data":{"id":"kt-1","type":"ci_app_libraries_tests_request",
				"attributes":{"service":"my-service","env":"ci","repository_url":"https://git/x",
				"configurations":{},"page_info":{"page_state":"cursor-1"}}}}`)
			w := post(t, e, path, body)
			if w.Code != http.StatusOK {
				t.Fatalf("got %d, want 200 (%s)", w.Code, w.Body.String())
			}

			var got map[string]any
			if err := json.Unmarshal(w.Body.Bytes(), &got); err != nil {
				t.Fatalf("response is not JSON: %v", err)
			}
			data, _ := got["data"].(map[string]any)
			attrs, _ := data["attributes"].(map[string]any)
			if attrs == nil {
				t.Fatalf("no data.attributes: %s", w.Body.String())
			}
			tests, ok := attrs["tests"].(map[string]any)
			if !ok {
				t.Fatalf("attributes.tests is not an object: %s", w.Body.String())
			}
			if len(tests) != 0 {
				t.Errorf("tests: got %d modules, want 0", len(tests))
			}
			page, _ := attrs["page_info"].(map[string]any)
			if page == nil {
				t.Fatalf("attributes.page_info is missing")
			}
			if page["has_next"] != false {
				t.Errorf("page_info.has_next: got %v, want false — the client loops forever on true",
					page["has_next"])
			}

			rows := Rows[storage.CISettingsRequestRow](node)
			if len(rows) != 1 {
				t.Fatalf("rows: got %d, want 1", len(rows))
			}
			if rows[0].PageState != "cursor-1" {
				t.Errorf("page_state: got %q, want cursor-1", rows[0].PageState)
			}
		})
	}
}

func TestCITestManagementQuarantinesNothing(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIVisibilityAPI)

	body := []byte(`{"data":{"id":"tm-1","type":"ci_app_libraries_tests_request",
		"attributes":{"repository_url":"https://git/x","sha":"abc","module":"intake",
		"commit_message":"fix things","branch":"main"}}}`)
	w := post(t, e, "/api/v2/test/libraries/test-management/tests", body)
	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200 (%s)", w.Code, w.Body.String())
	}

	var got map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &got); err != nil {
		t.Fatalf("response is not JSON: %v", err)
	}
	data, _ := got["data"].(map[string]any)
	attrs, _ := data["attributes"].(map[string]any)
	modules, ok := attrs["modules"].(map[string]any)
	if !ok {
		t.Fatalf("attributes.modules is not an object: %s", w.Body.String())
	}
	if len(modules) != 0 {
		t.Errorf("modules: got %d, want 0", len(modules))
	}

	rows := Rows[storage.CISettingsRequestRow](node)
	if len(rows) != 1 {
		t.Fatalf("rows: got %d, want 1", len(rows))
	}
	if rows[0].Module != "intake" || rows[0].CommitMessage != "fix things" {
		t.Errorf("test-management attributes did not reach the row: %+v", rows[0])
	}
}

// A question we cannot parse still produces a row and still gets a valid
// answer: the endpoint label plus the raw body is how we learn what a new
// client sends, and a 4xx here is terminal for the client.
func TestCIConfigEndpointsKeepUndecodableRequests(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIVisibilityAPI)

	w := post(t, e, "/api/v2/libraries/tests/services/setting", []byte(`{not json`))
	if w.Code != http.StatusOK {
		t.Errorf("got %d, want 200 — an unreadable question still gets a well-formed answer", w.Code)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 || raw[0].Intake != "cisettings" {
		t.Fatalf("raw rows: got %d (%v)", len(raw), raw)
	}
	rows := Rows[storage.CISettingsRequestRow](node)
	if len(rows) != 1 {
		t.Fatalf("settings rows: got %d, want 1 — the row is what says a client is sending "+
			"something we cannot read", len(rows))
	}
	if rows[0].RequestBody != `{not json` {
		t.Errorf("request_body: got %q", rows[0].RequestBody)
	}
}

// ---------------------------------------------------------------------------
// git metadata

func TestGitSearchCommitsStoresCandidatesAndAnswersEmpty(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIVisibilityAPI)

	body := []byte(`{"data":[{"id":"aaa111","type":"commit"},{"id":"bbb222","type":"commit"}],
		"meta":{"repository_url":"https://github.com/itsninjacats/ninjacat"}}`)
	w := post(t, e, "/api/v2/git/repository/search_commits", body)
	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200 (%s)", w.Code, w.Body.String())
	}

	var got map[string]any
	if err := json.Unmarshal(w.Body.Bytes(), &got); err != nil {
		t.Fatalf("response is not JSON: %v", err)
	}
	data, ok := got["data"].([]any)
	if !ok {
		t.Fatalf("data is not an array: %s", w.Body.String())
	}
	// Empty because "which of these do you have" is a READ, and the intake
	// has no read path by design. Empty makes the client upload its packs,
	// which is where git_packfiles gets filled from.
	if len(data) != 0 {
		t.Errorf("data: got %d commits, want 0", len(data))
	}
	meta, _ := got["meta"].(map[string]any)
	if meta == nil || meta["repository_url"] != "https://github.com/itsninjacats/ninjacat" {
		t.Errorf("meta.repository_url must be echoed, got %v", meta)
	}

	rows := Rows[storage.GitCommitRow](node)
	if len(rows) != 2 {
		t.Fatalf("git_commits rows: got %d, want 2", len(rows))
	}
	for _, r := range rows {
		if r.TenantID != testTenant {
			t.Errorf("tenant: got %q", r.TenantID)
		}
		if r.RepositoryURL != "https://github.com/itsninjacats/ninjacat" {
			t.Errorf("repository_url: got %q", r.RepositoryURL)
		}
		if r.Source != "search_commits" {
			t.Errorf("source: got %q, want search_commits", r.Source)
		}
		// NULL: the client is ASKING about this sha. A non-nil packfile id
		// would claim we hold objects we have never seen.
		if r.PackfileID != nil {
			t.Errorf("packfile_id: got %v, want nil — a candidate sha is a question, not an upload", r.PackfileID)
		}
	}
	if rows[0].SHA != "aaa111" || rows[1].SHA != "bbb222" {
		t.Errorf("shas: got %q and %q", rows[0].SHA, rows[1].SHA)
	}
}

// 204 No Content is the only status both shipped clients call success:
// dd-trace-go accepts any 2xx, dd-trace-py checks for exactly 204.
func TestGitPackfileAnswers204AndStoresBothTables(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIVisibilityAPI)

	pack := []byte("PACK\x00\x00\x00\x02\x00\x00\x00\x01binary-git-objects")
	body, ct := civMultipart(t, []civPart{
		{Name: "pushedSha", ContentType: "application/json",
			Data: []byte(`{"data":{"id":"headsha123","type":"commit"},"meta":{"repository_url":"https://git/x"}}`)},
		{Name: "packfile", FileName: "pack-abc.pack", ContentType: "application/octet-stream", Data: pack},
	})

	w := postCT(t, e, "/api/v2/git/repository/packfile", ct, body, nil)
	if w.Code != http.StatusNoContent {
		t.Fatalf("got %d, want 204 — dd-trace-py checks for exactly this (%s)", w.Code, w.Body.String())
	}
	if w.Body.Len() != 0 {
		t.Errorf("204 must carry no body, got %q", w.Body.String())
	}

	packs := Rows[storage.GitPackfileRow](node)
	if len(packs) != 1 {
		t.Fatalf("git_packfiles rows: got %d, want 1", len(packs))
	}
	p := packs[0]
	if p.RepositoryURL != "https://git/x" || p.PushedSHA != "headsha123" {
		t.Errorf("pushedSha part did not reach the row: repo=%q sha=%q", p.RepositoryURL, p.PushedSHA)
	}
	if p.Packfile != string(pack) {
		t.Errorf("the pack bytes must be kept verbatim, got %d B", len(p.Packfile))
	}
	if p.SizeBytes != uint64(len(pack)) {
		t.Errorf("size_bytes: got %d, want %d", p.SizeBytes, len(pack))
	}
	if p.Filename != "pack-abc.pack" {
		t.Errorf("filename: got %q", p.Filename)
	}
	if len(p.PackfileID) != 64 {
		t.Errorf("packfile_id: got %q, want a 64-char sha256 hex", p.PackfileID)
	}

	// The pushed sha also lands in git_commits, this time WITH a packfile id
	// — the one path where "we hold the objects" is true.
	commits := Rows[storage.GitCommitRow](node)
	if len(commits) != 1 {
		t.Fatalf("git_commits rows: got %d, want 1", len(commits))
	}
	if commits[0].SHA != "headsha123" || commits[0].Source != "packfile" {
		t.Errorf("commit row: %+v", commits[0])
	}
	if commits[0].PackfileID == nil || *commits[0].PackfileID != p.PackfileID {
		t.Errorf("packfile_id: got %v, want %q — this is what makes the two tables joinable",
			commits[0].PackfileID, p.PackfileID)
	}
}

// ---------------------------------------------------------------------------
// datadog-ci tag / measure / trace

func TestCIPipelineEventsDecodeEachCLICall(t *testing.T) {
	cases := []struct {
		name  string
		path  string
		kind  string
		body  string
		check func(t *testing.T, r storage.CIPipelineEventRow)
	}{
		{
			name: "datadog-ci tag",
			path: "/api/v2/ci/pipeline/tags",
			kind: "tag",
			body: `{"data":{"type":"ci_custom_tag","attributes":{"ci_level":1,"provider":"github",
				"ci_env":{"GITHUB_RUN_ID":"9911","GITHUB_SHA":"abc"},
				"tags":{"team":"backend","release":"2.4.0"}}}}`,
			check: func(t *testing.T, r storage.CIPipelineEventRow) {
				if r.DataType != "ci_custom_tag" || r.Provider != "github" {
					t.Errorf("data_type/provider: %q/%q", r.DataType, r.Provider)
				}
				if r.CILevel == nil || *r.CILevel != 1 {
					t.Errorf("ci_level: got %v, want 1 (pipeline)", r.CILevel)
				}
				if r.CIEnv["GITHUB_RUN_ID"] != "9911" {
					t.Errorf("ci_env is the join key back to the pipeline and must be a real map, got %v", r.CIEnv)
				}
				if r.Tags["team"] != "backend" || r.Tags["release"] != "2.4.0" {
					t.Errorf("tags: got %v", r.Tags)
				}
			},
		},
		{
			name: "datadog-ci measure",
			path: "/api/v2/ci/pipeline/metrics",
			kind: "measure",
			body: `{"data":{"type":"ci_custom_metric","attributes":{"ci_level":3,"provider":"gitlab",
				"ci_env":{"CI_JOB_ID":"77"},"metrics":{"build.seconds":42.5,"artifacts.mb":3}}}}`,
			check: func(t *testing.T, r storage.CIPipelineEventRow) {
				if r.Metrics["build.seconds"] != 42.5 || r.Metrics["artifacts.mb"] != 3 {
					t.Errorf("metrics: got %v", r.Metrics)
				}
				if r.CILevel == nil || *r.CILevel != 3 {
					t.Errorf("ci_level: got %v, want 3 (job)", r.CILevel)
				}
			},
		},
		{
			name: "datadog-ci trace",
			path: "/api/intake/ci/custom_spans",
			kind: "custom_span",
			body: `{"data":{"type":"ci_app_custom_span","attributes":{"name":"build",
				"start_time":"2026-09-23T10:00:00Z","end_time":"2026-09-23T10:05:00Z",
				"tags":{"step":"compile"},"measures":{"warnings":7},
				"something_new":{"nested":true}}}}`,
			check: func(t *testing.T, r storage.CIPipelineEventRow) {
				if r.SpanName != "build" || r.SpanStartRaw != "2026-09-23T10:00:00Z" {
					t.Errorf("span hot fields: name=%q start=%q", r.SpanName, r.SpanStartRaw)
				}
				// "measures" is the custom-span spelling of the same idea;
				// both land in the metrics column.
				if r.Metrics["warnings"] != 7 {
					t.Errorf("measures did not reach the metrics column: %v", r.Metrics)
				}
				// A key this decoder does not name is kept rather than
				// dropped — the whole reason for the extra map.
				if r.Extra["something_new"] != `{"nested":true}` {
					t.Errorf("extra: got %v, want the undeclared key kept with its value", r.Extra)
				}
				// ci_level is genuinely absent here, so NULL.
				if r.CILevel != nil {
					t.Errorf("ci_level: got %v, want nil — a custom span sends none", r.CILevel)
				}
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeCIVisibilityAPI)

			if w := post(t, e, tc.path, []byte(tc.body)); w.Code != http.StatusAccepted {
				t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
			}
			rows := Rows[storage.CIPipelineEventRow](node)
			if len(rows) != 1 {
				t.Fatalf("rows: got %d, want 1", len(rows))
			}
			if rows[0].Kind != tc.kind {
				t.Errorf("kind: got %q, want %q", rows[0].Kind, tc.kind)
			}
			if rows[0].TenantID != testTenant {
				t.Errorf("tenant: got %q", rows[0].TenantID)
			}
			if rows[0].Body != tc.body {
				t.Error("the raw body must be kept whole")
			}
			tc.check(t, rows[0])
		})
	}
}

func TestCIPipelineEventsStoreUndecodableBodiesRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIVisibilityAPI)

	if w := post(t, e, "/api/v2/ci/pipeline/tags", []byte(`{"data":`)); w.Code != http.StatusAccepted {
		t.Errorf("got %d, want 202", w.Code)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 || raw[0].Intake != "cipipeline" {
		t.Errorf("raw rows: got %d (%v)", len(raw), raw)
	}
	if len(Rows[storage.CIPipelineEventRow](node)) != 0 {
		t.Error("an undecodable body must produce no pipeline rows")
	}
}
