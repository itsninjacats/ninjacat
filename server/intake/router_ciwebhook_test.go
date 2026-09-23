package intake

import (
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"testing"

	"github.com/gin-gonic/gin"

	"github.com/itsninjacats/server/apps/storage"
)

// getWithKey sends a GET with the test key, for the one route in this
// package whose answer the caller obeys rather than ignores.
func getWithKey(t *testing.T, e *gin.Engine, path string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	req.Header.Set("Dd-Api-Key", testAPIKey)
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)
	return w
}

// ---------------------------------------------------------------------------
// webhook-intake.<site>

// The Jenkins plugin's batch, built from DatadogWebhookBuildLogic (pipeline
// level) and DatadogWebhookPipelineLogic (stage and job). One batch MIXES
// levels and the backend tells them apart by the "level" field, not by URL —
// which is the reason one table carries all three.
const jenkinsWebhookBatch = `[
  {
    "payload_version": 1,
    "level": "pipeline",
    "url": "https://jenkins.example.test/job/ninjacat/42/",
    "start": "2026-09-23T10:00:00.000Z",
    "end": "2026-09-23T10:06:30.000Z",
    "partial_retry": false,
    "queue_time": 1500,
    "status": "success",
    "is_manual": true,
    "trace_id": "5678901234567890123",
    "span_id": "1234567890123456789",
    "pipeline_id": "42",
    "unique_id": "b0a1c2d3-4e5f-6071-8293-a4b5c6d7e8f9",
    "name": "ninjacat",
    "user": {"name": "michal", "email": "michal@example.test"},
    "parameters": {"BRANCH": "feat/write-layer", "DEPLOY": "false"},
    "tags": ["jenkins_tag:value", "kube_service:a", "kube_service:b", "bare_tag"],
    "node": {
      "name": "built-in",
      "hostname": "controller.example.test",
      "workspace": "/var/jenkins_home/workspace/ninjacat",
      "labels": ["linux", "docker"]
    },
    "git": {
      "repository_url": "https://github.com/itsninjacats/ninjacat",
      "default_branch": "main",
      "branch": "feat/write-layer",
      "sha": "0f4c2a1b9e7d6c5a4b3e2d1c0f9e8d7c6b5a4938",
      "message": "feat(intake): CI Visibility",
      "author_name": "Michal",
      "author_email": "michal@example.test",
      "committer_name": "Michal",
      "committer_email": "michal@example.test"
    },
    "parent_pipeline": {"trace_id": "999", "url": "https://jenkins.example.test/job/parent/1/"}
  },
  {
    "level": "stage",
    "id": "7",
    "name": "Build",
    "trace_id": "5678901234567890123",
    "span_id": "2222222222222222222",
    "parent_span_id": "1234567890123456789",
    "pipeline_unique_id": "b0a1c2d3-4e5f-6071-8293-a4b5c6d7e8f9",
    "pipeline_name": "ninjacat",
    "status": "success",
    "start": "2026-09-23T10:00:05.000Z",
    "end": "2026-09-23T10:04:00.000Z",
    "queue_time": 0,
    "url": "https://jenkins.example.test/job/ninjacat/42/execution/node/7/",
    "node": {"name": "agent-1", "hostname": "agent1.example.test", "workspace": "/w", "labels": []},
    "tags": []
  },
  {
    "level": "job",
    "id": "11",
    "name": "go test",
    "stage_id": "7",
    "stage_name": "Build",
    "trace_id": "5678901234567890123",
    "span_id": "3333333333333333333",
    "parent_span_id": "2222222222222222222",
    "pipeline_unique_id": "b0a1c2d3-4e5f-6071-8293-a4b5c6d7e8f9",
    "status": "error",
    "start": "2026-09-23T10:01:00.000Z",
    "end": "2026-09-23T10:03:00.000Z",
    "error": {
      "message": "exit status 1",
      "type": "ScriptError",
      "domain": "unknown",
      "stack": "at go test"
    },
    "node": {"name": "agent-1", "hostname": "agent1.example.test", "workspace": "/w", "labels": []},
    "tags": ["sh.args.script:go test ./..."],
    "something_jenkins_added": {"later": true}
  }
]`

func TestCIWebhookDecodesJenkinsBatch(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIWebhook)

	w := postCT(t, e, "/api/v2/webhook/?service=jenkins-prod", "application/json",
		[]byte(jenkinsWebhookBatch), map[string]string{
			"DD-CI-PROVIDER-NAME": "jenkins",
			"User-Agent":          "Jenkins/2.4",
			"Authorization":       "Bearer should-never-be-stored",
		})
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.CIWebhookEventRow](node)
	if len(rows) != 3 {
		t.Fatalf("rows: got %d, want 3 — a batch mixes levels and every element is a row", len(rows))
	}

	byLevel := map[string]storage.CIWebhookEventRow{}
	for _, r := range rows {
		byLevel[r.Level] = r
		if r.TenantID != testTenant {
			t.Errorf("%s tenant: got %q", r.Level, r.TenantID)
		}
		if r.Provider != "jenkins" {
			t.Errorf("%s provider: got %q, want jenkins (from DD-CI-PROVIDER-NAME)", r.Level, r.Provider)
		}
		if r.Service != "jenkins-prod" {
			t.Errorf("%s service: got %q, want the ?service= query parameter", r.Level, r.Service)
		}
		if r.Body == "" {
			t.Errorf("%s body: the element must be kept verbatim", r.Level)
		}
		// The header allowlist exists for this line: a 90-day table must
		// never become a place credentials are durable.
		for _, forbidden := range []string{"Authorization", "Dd-Api-Key"} {
			if v, ok := r.Headers[forbidden]; ok {
				t.Errorf("%s stored (%q) — the header allowlist leaked a credential", forbidden, v)
			}
		}
		if r.Headers["Dd-Ci-Provider-Name"] != "jenkins" {
			t.Errorf("%s headers: got %v", r.Level, r.Headers)
		}
	}

	pipeline, ok := byLevel["pipeline"]
	if !ok {
		t.Fatalf("no pipeline-level row, got levels %v", reflect.ValueOf(byLevel).MapKeys())
	}
	if pipeline.UniqueID != "b0a1c2d3-4e5f-6071-8293-a4b5c6d7e8f9" {
		t.Errorf("unique_id: got %q", pipeline.UniqueID)
	}
	// Ids are Strings, not UInt64: the same fields carry UUIDs from other
	// providers, and a numeric column would turn those into 0.
	if pipeline.TraceID != "5678901234567890123" {
		t.Errorf("trace_id: got %q, want the exact literal", pipeline.TraceID)
	}
	if pipeline.Status != "success" || pipeline.Name != "ninjacat" {
		t.Errorf("status/name: %q/%q", pipeline.Status, pipeline.Name)
	}
	if pipeline.PayloadVersion == nil || *pipeline.PayloadVersion != 1 {
		t.Errorf("payload_version: got %v, want 1", pipeline.PayloadVersion)
	}
	if pipeline.IsManual == nil || *pipeline.IsManual != 1 {
		t.Errorf("is_manual: got %v, want 1", pipeline.IsManual)
	}
	if pipeline.PartialRetry == nil || *pipeline.PartialRetry != 0 {
		t.Errorf("partial_retry: got %v, want 0 (the field said false)", pipeline.PartialRetry)
	}
	if pipeline.QueueTimeMs == nil || *pipeline.QueueTimeMs != 1500 {
		t.Errorf("queue_time_ms: got %v, want 1500", pipeline.QueueTimeMs)
	}
	if pipeline.StartParsed == nil || pipeline.StartParsed.Format("15:04:05") != "10:00:00" {
		t.Errorf("start_parsed: got %v", pipeline.StartParsed)
	}
	if pipeline.StartRaw != "2026-09-23T10:00:00.000Z" {
		t.Errorf("start_raw: got %q — the wire string is kept next to the parse", pipeline.StartRaw)
	}
	if pipeline.GitBranch != "feat/write-layer" || pipeline.GitDefaultBranch != "main" {
		t.Errorf("git: branch=%q default=%q", pipeline.GitBranch, pipeline.GitDefaultBranch)
	}
	if pipeline.UserName != "michal" || pipeline.UserEmail != "michal@example.test" {
		t.Errorf("user: %q/%q", pipeline.UserName, pipeline.UserEmail)
	}
	if pipeline.NodeWorkspace != "/var/jenkins_home/workspace/ninjacat" {
		t.Errorf("node.workspace: got %q", pipeline.NodeWorkspace)
	}
	if want := []string{"linux", "docker"}; !reflect.DeepEqual(pipeline.NodeLabels, want) {
		t.Errorf("node.labels: got %v, want %v", pipeline.NodeLabels, want)
	}
	if pipeline.Parameters["BRANCH"] != "feat/write-layer" {
		t.Errorf("parameters: got %v", pipeline.Parameters)
	}
	if pipeline.ParentPipelineURL != "https://jenkins.example.test/job/parent/1/" {
		t.Errorf("parent_pipeline.url: got %q", pipeline.ParentPipelineURL)
	}

	// tags is a genuine multiset: the plugin repeats keys for
	// per-configuration axes, and a plain map would keep only the last one.
	// See docs/decisions/0001-tags-are-a-multiset.md.
	if want := []string{"a", "b"}; !reflect.DeepEqual(pipeline.Tags["kube_service"], want) {
		t.Errorf("tags[kube_service]: got %v, want %v — a repeated tag key must keep every value",
			pipeline.Tags["kube_service"], want)
	}
	// A bare tag keeps its key with an empty value, the way splitTag defines it.
	if want := []string{""}; !reflect.DeepEqual(pipeline.Tags["bare_tag"], want) {
		t.Errorf("tags[bare_tag]: got %v, want %v", pipeline.Tags["bare_tag"], want)
	}

	stage := byLevel["stage"]
	if stage.ID != "7" || stage.ParentSpanID != "1234567890123456789" {
		t.Errorf("stage id/parent: %q/%q", stage.ID, stage.ParentSpanID)
	}
	if stage.PipelineUniqueID != "b0a1c2d3-4e5f-6071-8293-a4b5c6d7e8f9" {
		t.Errorf("stage pipeline_unique_id: got %q", stage.PipelineUniqueID)
	}

	job := byLevel["job"]
	if job.StageID != "7" || job.StageName != "Build" {
		t.Errorf("job stage: %q/%q", job.StageID, job.StageName)
	}
	if job.ErrorMessage != "exit status 1" || job.ErrorType != "ScriptError" ||
		job.ErrorDomain != "unknown" || job.ErrorStack != "at go test" {
		t.Errorf("job error: %+v", job)
	}
	// A key the provider added without telling anybody is kept with its
	// value, which is the point of the extra map.
	if job.Extra["something_jenkins_added"] != `{"later":true}` {
		t.Errorf("extra: got %v, want the undeclared key kept", job.Extra)
	}
	if want := []string{"go test ./..."}; !reflect.DeepEqual(job.Tags["sh.args.script"], want) {
		t.Errorf("a tag value containing a colon must split on the FIRST one only, got %v",
			job.Tags["sh.args.script"])
	}
}

// GitLab's own emitter was not readable (it lives in gitlab-org/gitlab), so
// the decoder falls back to object_kind for both the provider and the level
// rather than assuming Datadog's level-shaped schema. Whatever arrives, the
// element is stored whole.
func TestCIWebhookHandlesGitLabNativeShape(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIWebhook)

	body := `{
		"object_kind": "pipeline",
		"object_attributes": {"id": 31, "ref": "main", "sha": "abc123", "status": "success"},
		"project": {"web_url": "https://gitlab.example.test/ninjacat", "default_branch": "main"},
		"user": {"name": "michal", "email": "michal@example.test"},
		"builds": [{"id": 380, "stage": "test", "name": "unit", "status": "success"}]
	}`

	w := postCT(t, e, "/api/v2/webhook", "application/json", []byte(body), map[string]string{
		"X-Gitlab-Event":      "Pipeline Hook",
		"X-Gitlab-Event-UUID": "d6f1c0a2-1111-2222-3333-444455556666",
	})
	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 (%s)", w.Code, w.Body.String())
	}

	rows := Rows[storage.CIWebhookEventRow](node)
	if len(rows) != 1 {
		t.Fatalf("rows: got %d, want 1 — a provider that skipped the batcher sends one object", len(rows))
	}
	r := rows[0]
	// No DD-CI-PROVIDER-NAME header here, so the shape has to say it.
	if r.Provider != "gitlab" {
		t.Errorf("provider: got %q, want gitlab (sniffed from object_kind)", r.Provider)
	}
	if r.Level != "pipeline" {
		t.Errorf("level: got %q, want pipeline (object_kind is the discriminator here)", r.Level)
	}
	if r.DeliveryID != "d6f1c0a2-1111-2222-3333-444455556666" {
		t.Errorf("delivery_id: got %q, want X-Gitlab-Event-UUID", r.DeliveryID)
	}
	if r.UserName != "michal" {
		t.Errorf("user.name: got %q — GitLab's user object happens to match Datadog's", r.UserName)
	}
	// Every field that has no column of its own must still be somewhere.
	for _, key := range []string{"object_attributes", "project", "builds", "object_kind"} {
		if _, ok := r.Extra[key]; !ok {
			t.Errorf("extra is missing %q — a shape we do not model must not lose fields", key)
		}
	}
	if r.Body == "" {
		t.Error("the element must be kept verbatim")
	}
}

// A provider that names neither itself nor a level is still a full row:
// "unknown" is an honest answer and a guess that produced a real provider
// name would be worse than useless.
func TestCIWebhookLabelsUnrecognisedSendersUnknown(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIWebhook)

	postCT(t, e, "/api/v2/webhook", "application/json",
		[]byte(`[{"whatever":"this is","status":"ok"}]`), nil)

	rows := Rows[storage.CIWebhookEventRow](node)
	if len(rows) != 1 {
		t.Fatalf("rows: got %d, want 1", len(rows))
	}
	if rows[0].Provider != "unknown" || rows[0].Level != "" {
		t.Errorf("provider/level: got %q/%q, want unknown and empty", rows[0].Provider, rows[0].Level)
	}
	if rows[0].Extra["whatever"] != `"this is"` {
		t.Errorf("extra: got %v", rows[0].Extra)
	}
}

// Both path spellings are registered rather than relying on Gin's trailing
// slash redirect: the Jenkins plugin's URL ends in a slash, and a 301 on a
// POST drops the body, which here is a CI run that never appears.
func TestCIWebhookAcceptsBothPathSpellings(t *testing.T) {
	for _, path := range []string{"/api/v2/webhook", "/api/v2/webhook/"} {
		a, node := newTestServer(t)
		e := newTestEngine(t, a, a.routeCIWebhook)
		w := postCT(t, e, path, "application/json", []byte(`[{"level":"pipeline","name":"x"}]`), nil)
		if w.Code != http.StatusAccepted {
			t.Errorf("POST %s: got %d, want 202 — a redirect here loses the body", path, w.Code)
		}
		if got := len(Rows[storage.CIWebhookEventRow](node)); got != 1 {
			t.Errorf("POST %s: %d rows, want 1", path, got)
		}
	}
}

func TestCIWebhookStoresUndecodableBatchesRaw(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeCIWebhook)

	if w := postCT(t, e, "/api/v2/webhook", "application/json", []byte(`[{"a":1},`), nil); w.Code != http.StatusAccepted {
		t.Errorf("got %d, want 202", w.Code)
	}
	raw := Rows[storage.RawPayloadRow](node)
	if len(raw) != 1 || raw[0].Intake != "webhook" {
		t.Errorf("raw rows: got %d (%v)", len(raw), raw)
	}
	if len(Rows[storage.CIWebhookEventRow](node)) != 0 {
		t.Error("an undecodable batch must produce no webhook rows")
	}
}

// ---------------------------------------------------------------------------
// intake.synthetics.<site>

// The poller's decoder is all-or-nothing: the body goes into
// struct{Tests []common.SyntheticsTestConfig} in ONE Decode, and
// SyntheticsTestConfig's custom UnmarshalJSON errors on an unrecognised
// subtype — which discards every test in the response, not just the bad one.
// So the empty answer has to be exactly right, and that is worth asserting on
// the bytes rather than on a struct.
func TestSyntheticsAgentPollerGetsAnEmptyTestList(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeSyntheticsAgent)

	w := getWithKey(t, e, "/api/unstable/synthetics/agents/tests?agent_hostname=runner-7&agent_version=7.83.0")

	// 200, not 202: the poller treats anything else as a failure and five in
	// a row flip it unhealthy, handing scheduling to an in-memory fallback.
	if w.Code != http.StatusOK {
		t.Fatalf("got %d, want 200 — anything else counts as a poll failure (%s)", w.Code, w.Body.String())
	}

	var response struct {
		Tests []json.RawMessage `json:"tests"`
	}
	if err := json.Unmarshal(w.Body.Bytes(), &response); err != nil {
		t.Fatalf("response does not unmarshal into the poller's own struct: %v (%s)", err, w.Body.String())
	}
	if len(response.Tests) != 0 {
		t.Errorf("tests: got %d, want 0 — the catalogue is empty and a test with an unknown "+
			"subtype would fail the agent's whole poll", len(response.Tests))
	}
	// "tests": [] rather than null. Both decode, but the array is what the
	// field means and what a capture should show.
	if got := w.Body.String(); got != `{"tests":[]}` {
		t.Errorf("body: got %s, want {\"tests\":[]}", got)
	}

	// A GET carries no telemetry. Nothing is stored for it — not even a raw
	// payload, which would be one row every two seconds per agent.
	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored for a poll, want 0", n)
	}
}

// The poller sends its key in DD-API-KEY, which is the same header the
// default guard reads; without one it must never reach the handler.
func TestSyntheticsAgentPollerNeedsAKey(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeSyntheticsAgent)

	req := httptest.NewRequest(http.MethodGet, "/api/unstable/synthetics/agents/tests", nil)
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusForbidden {
		t.Errorf("no key: got %d, want 403", w.Code)
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored for a rejected poll, want 0", n)
	}
}
