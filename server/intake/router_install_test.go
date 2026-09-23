package intake

import (
	"bytes"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/itsninjacats/server/apps/storage"
)

// aiusage has no published schema (the Rust ai_prompt_logger's producer is
// not in datadog-agent), so a real request goes to raw_payloads whole,
// exactly like genresources.
func TestHandleAIUsageStoresRawPayload(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeAIUsage)

	body := []byte(`{"prompt":"summarise this","model":"claude"}`)
	req := httptest.NewRequest(http.MethodPost, "/api/v2/aiusage", bytes.NewReader(body))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("DD-EVP-ORIGIN", "ai_prompt_logger")

	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	if w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/aiusage: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.RawPayloadRow](node)
	if len(rows) != 1 {
		t.Fatalf("raw_payloads rows: got %d, want 1", len(rows))
	}
	if rows[0].Intake != "aiusage" || rows[0].Reason != "no_schema" {
		t.Errorf("intake/reason: got %q/%q, want aiusage/no_schema", rows[0].Intake, rows[0].Reason)
	}
	if rows[0].Body != string(body) {
		t.Errorf("body: got %q, want the exact bytes sent", rows[0].Body)
	}
}

// The connectivity diagnose is the agent's only real caller of this host: an
// empty body must store nothing at all, not even a raw_payloads row — it is
// not a payload, it is a probe.
func TestHandleLLMObsDiagnoseProbeStoresNothing(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeLLMObs)

	req := httptest.NewRequest(http.MethodPost, "/api/v2/llmobs", nil)
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("X-Requested-With", "datadog-agent-diagnose")

	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	if w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/llmobs (diagnose): got %d, want 202 (%s)", w.Code, w.Body.String())
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored for a diagnose probe, want 0", n)
	}
}

// A real payload on this host is unexpected — the agent never sends LLM
// Observability data here — but it is still kept rather than dropped, as a
// safety net in case that assumption turns out to be wrong.
func TestHandleLLMObsUnexpectedPayloadStoresRawPayload(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeLLMObs)

	body := []byte(`{"span":{"name":"llm-call"}}`)
	req := httptest.NewRequest(http.MethodPost, "/api/v2/llmobs", bytes.NewReader(body))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/json")

	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	if w.Code != http.StatusAccepted {
		t.Fatalf("POST /api/v2/llmobs: got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.RawPayloadRow](node)
	if len(rows) != 1 {
		t.Fatalf("raw_payloads rows: got %d, want 1", len(rows))
	}
	if rows[0].Intake != "llmobs" || rows[0].Reason != "no_schema" {
		t.Errorf("intake/reason: got %q/%q, want llmobs/no_schema", rows[0].Intake, rows[0].Reason)
	}
	if rows[0].Body != string(body) {
		t.Errorf("body: got %q, want the exact bytes sent", rows[0].Body)
	}
}

// The install host's own routes (OCI 404s, BTF, the HEAD probe) store
// nothing and never changed — pinned here so a future edit to this file
// notices if that stops being true. handleOCIManifest's error CODE is
// load-bearing: go-containerregistry parses it into a *transport.Error, so
// the exact string matters, not just the 404.
func TestInstallOCIManifestUnhostedReturnsKnownErrorCode(t *testing.T) {
	a, node := newTestServer(t)
	e := a.publicEngine(a.routeInstall)

	req := httptest.NewRequest(http.MethodGet, "/v2/datadog-agent/manifests/latest", nil)
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusNotFound {
		t.Fatalf("GET /v2/.../manifests/latest: got %d, want 404", w.Code)
	}
	if !bytes.Contains(w.Body.Bytes(), []byte(`"MANIFEST_UNKNOWN"`)) {
		t.Errorf("body: got %q, want it to carry the MANIFEST_UNKNOWN code", w.Body.String())
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored for an unhosted OCI manifest request, want 0", n)
	}
}
