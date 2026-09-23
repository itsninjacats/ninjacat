package intake

import (
	"bytes"
	"encoding/json"
	"mime/multipart"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/itsninjacats/server/apps/storage"
)

// buildFlareBody mirrors comp/core/flare/helpers/send_flare.go's
// getFlareReader: case_id, email, source, rc_task_uuid (each only when
// non-empty), then the archive as the file part "flare_file", then
// agent_version and hostname AFTER the file — the real uploader writes
// metadata on both sides of the archive, so a handler that assumed it all
// arrives first would miss half of it.
func buildFlareBody(t *testing.T, caseID, email, source, rcTaskUUID, archive, agentVersion, hostname string) (*bytes.Buffer, string) {
	t.Helper()
	var buf bytes.Buffer
	mw := multipart.NewWriter(&buf)

	writeField := func(name, value string) {
		if value == "" {
			return
		}
		if err := mw.WriteField(name, value); err != nil {
			t.Fatalf("write field %q: %v", name, err)
		}
	}
	writeField("case_id", caseID)
	writeField("email", email)
	writeField("source", source)
	writeField("rc_task_uuid", rcTaskUUID)

	part, err := mw.CreateFormFile("flare_file", "datadog-agent-2026-09-23.zip")
	if err != nil {
		t.Fatalf("create flare_file part: %v", err)
	}
	if _, err := part.Write([]byte(archive)); err != nil {
		t.Fatalf("write archive: %v", err)
	}

	writeField("agent_version", agentVersion)
	writeField("hostname", hostname)

	if err := mw.Close(); err != nil {
		t.Fatalf("close multipart writer: %v", err)
	}
	return &buf, mw.FormDataContentType()
}

func postFlare(t *testing.T, e http.Handler, body *bytes.Buffer, contentType string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(http.MethodPost, "/support/flare", body)
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", contentType)
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	return w
}

// HEAD is the redirect probe resolveFlarePOSTURL makes before POSTing the
// archive: it accepts 200 or 404 as "reachable", so a bare 200 with nothing
// stored is the whole contract.
func TestHandleFlareHeadAnswersOKWithNoBody(t *testing.T) {
	a, node := newTestServer(t)
	e := a.engine() // /support/flare is registered on every engine

	req := httptest.NewRequest(http.MethodHead, "/support/flare", nil)
	req.Header.Set("Dd-Api-Key", testAPIKey)
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("HEAD /support/flare: got %d, want 200", w.Code)
	}
	if w.Body.Len() != 0 {
		t.Errorf("HEAD body: got %d bytes, want 0", w.Body.Len())
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("HEAD stored %d messages, want 0", n)
	}
}

// The worked example this file follows (testserver_test.go's
// TestHandleLogsStoresEveryItem): a real flare upload through the real
// engine, asserting on the row that reached agent_flares and on the response
// body the agent's analyzeResponse parses.
func TestHandleFlareStoresUploadAndAnswersDocumented(t *testing.T) {
	a, node := newTestServer(t)
	e := a.engine()

	body, contentType := buildFlareBody(t,
		"98765", "ops@example.test", "local", "rc-task-uuid-123",
		"PK\x03\x04 pretend zip bytes", "7.60.0", "web-01.example.test")

	w := postFlare(t, e, body, contentType)
	if w.Code != http.StatusOK {
		t.Fatalf("POST /support/flare: got %d, want 200 (%s)", w.Code, w.Body.String())
	}
	if ct := w.Header().Get("Content-Type"); ct != "application/json; charset=utf-8" {
		t.Errorf("Content-Type: got %q — the agent treats anything else as an error even on a 200", ct)
	}

	var resp struct {
		CaseID      int64  `json:"case_id"`
		Error       string `json:"error"`
		RequestUUID string `json:"request_uuid"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatalf("response is not valid JSON: %v (%s)", err, w.Body.String())
	}
	if resp.CaseID != 98765 {
		t.Errorf("case_id: got %d, want 98765 (echoed from the form field)", resp.CaseID)
	}
	if resp.Error != "" {
		t.Errorf("error: got %q, want empty on success", resp.Error)
	}
	if resp.RequestUUID == "" {
		t.Errorf("request_uuid: got empty, want a generated id")
	}

	rows := Rows[storage.AgentFlareRow](node)
	if len(rows) != 1 {
		t.Fatalf("agent_flares rows: got %d, want 1", len(rows))
	}
	r := rows[0]
	if r.TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", r.TenantID, testTenant)
	}
	if r.CaseID != "98765" || r.Email != "ops@example.test" || r.Source != "local" {
		t.Errorf("case_id/email/source: got %q/%q/%q", r.CaseID, r.Email, r.Source)
	}
	if r.AgentVersion != "7.60.0" || r.Hostname != "web-01.example.test" {
		t.Errorf("agent_version/hostname (written after the archive): got %q/%q", r.AgentVersion, r.Hostname)
	}
	if r.Filename != "datadog-agent-2026-09-23.zip" {
		t.Errorf("filename: got %q", r.Filename)
	}
	if r.Archive != "PK\x03\x04 pretend zip bytes" {
		t.Errorf("archive: got %q, want the bytes kept verbatim", r.Archive)
	}
	if r.SizeBytes != uint64(len(r.Archive)) {
		t.Errorf("size_bytes: got %d, want %d", r.SizeBytes, len(r.Archive))
	}
	if r.Fields["rc_task_uuid"] != "rc-task-uuid-123" {
		t.Errorf("fields[rc_task_uuid]: got %q, want the field kept even though it has no column of its own", r.Fields["rc_task_uuid"])
	}
}

// A brand-new upload — nothing to attach to yet — sends no case_id at all:
// getFlareReader only writes the field when non-empty. The response must
// still carry a usable (non-zero) numeric id.
func TestHandleFlareWithoutCaseIDGetsAGeneratedOne(t *testing.T) {
	a, _ := newTestServer(t)
	e := a.engine()

	body, contentType := buildFlareBody(t, "", "", "local", "", "zip-bytes", "7.60.0", "host-1")
	w := postFlare(t, e, body, contentType)
	if w.Code != http.StatusOK {
		t.Fatalf("POST /support/flare: got %d, want 200", w.Code)
	}

	var resp struct {
		CaseID int64 `json:"case_id"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatalf("response is not valid JSON: %v", err)
	}
	if resp.CaseID == 0 {
		t.Errorf("case_id: got 0, want a generated non-zero id")
	}
}

// A body whose Content-Type is not multipart/form-data cannot be parsed at
// all. It must still answer the documented 200+JSON contract (refusing it
// would just make the agent retry the same request) and the bytes must not
// vanish: storeRaw keeps them.
func TestHandleFlareMalformedContentTypeGoesToRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := a.engine()

	req := httptest.NewRequest(http.MethodPost, "/support/flare", bytes.NewReader([]byte("not a multipart body")))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/zip")
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("POST /support/flare: got %d, want 200 even on a decode failure", w.Code)
	}

	var resp struct {
		Error string `json:"error"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &resp); err != nil {
		t.Fatalf("response is not valid JSON: %v (%s)", err, w.Body.String())
	}
	if resp.Error == "" {
		t.Errorf("error: got empty, want a non-empty message")
	}

	if n := len(Rows[storage.AgentFlareRow](node)); n != 0 {
		t.Errorf("agent_flares rows: got %d, want 0", n)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 {
		t.Fatalf("raw payload rows: got %d, want 1", len(raw))
	}
	if raw[0].Intake != flareIntake || raw[0].Reason != "decode_error" {
		t.Errorf("intake/reason: got %q/%q, want flare/decode_error", raw[0].Intake, raw[0].Reason)
	}
	if raw[0].Body != "not a multipart body" {
		t.Errorf("body must be kept as it came: %q", raw[0].Body)
	}
}

// Well-formed multipart, but no flare_file part at all — the one shape this
// handler cannot store as a row.
func TestHandleFlareMissingArchiveGoesToRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := a.engine()

	var buf bytes.Buffer
	mw := multipart.NewWriter(&buf)
	if err := mw.WriteField("case_id", "1"); err != nil {
		t.Fatalf("write field: %v", err)
	}
	if err := mw.Close(); err != nil {
		t.Fatalf("close multipart writer: %v", err)
	}

	w := postFlare(t, e, &buf, mw.FormDataContentType())
	if w.Code != http.StatusOK {
		t.Fatalf("POST /support/flare: got %d, want 200", w.Code)
	}

	if n := len(Rows[storage.AgentFlareRow](node)); n != 0 {
		t.Errorf("agent_flares rows: got %d, want 0", n)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 || raw[0].Reason != "decode_error" {
		t.Fatalf("raw_payloads: got %+v, want one row with reason decode_error", raw)
	}
}
