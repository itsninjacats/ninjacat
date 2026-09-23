package intake

import (
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/apikeys"
)

// Every intake has its own hostname and the Host header alone decides which
// engine answers. These tests drive Handler() with httptest and only vary
// req.Host, so they need no listener, no database and no agent.

const (
	testSite = "example.test"
	testKey  = "routes-test-key"
)

// newTestHandler builds the router with one known API key. The store is the
// same in-memory snapshot the keeper actor publishes into, filled by hand.
func newTestHandler(t *testing.T) http.Handler {
	t.Helper()
	gin.SetMode(gin.TestMode)
	t.Setenv("NINJACAT_ACK_UNKNOWN", "") // HandleUnknown must answer 404, not 202

	store := apikeys.NewStore()
	store.Publish(
		[]apikeys.Key{{ID: "k1", Name: "routes-test", TenantID: "tenant-1"}},
		[]string{apikeys.Hash(testKey)},
	)
	return (&Server{Store: store}).Handler()
}

// call sends one request to the handler with the given Host and returns the
// recorded response. withKey adds the Dd-Api-Key header the agent sends.
func call(h http.Handler, method, host, path string, withKey bool) *httptest.ResponseRecorder {
	req := httptest.NewRequest(method, "http://"+host+path, nil)
	req.Host = host
	if withKey {
		req.Header.Set("Dd-Api-Key", testKey)
	}
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	return rec
}

func TestHostRoutingReachesOwnIntake(t *testing.T) {
	h := newTestHandler(t)

	cases := []struct {
		host, method, path string
		want               int
	}{
		{"sbom-intake." + testSite, http.MethodPost, "/api/v2/sbom", http.StatusAccepted},
		{"cws-intake." + testSite, http.MethodPost, "/api/v2/secdump", http.StatusAccepted},
		{"http-intake.logs." + testSite, http.MethodPost, "/api/v2/logs", http.StatusAccepted},
		{"agent-http-intake.logs." + testSite, http.MethodPost, "/api/v2/logs", http.StatusAccepted},
		{"api." + testSite, http.MethodGet, "/api/v1/validate", http.StatusOK},
		{"app." + testSite, http.MethodGet, "/api/v1/validate", http.StatusOK},
		{"7-60-0-app.agent." + testSite, http.MethodGet, "/api/v1/validate", http.StatusOK},

		// The flare uploader rewrites its host the same way the core
		// forwarder rewrites app.<site>, just with "flare" as the suffix —
		// see the routes.go comment above this case. HEAD is the redirect
		// probe SendTo makes before POSTing; both must reach an engine that
		// serves /support/flare, which every engine does.
		{"7-60-0-flare.agent." + testSite, http.MethodHead, "/support/flare", http.StatusOK},
		{"7-60-0-flare.agent." + testSite, http.MethodPost, "/support/flare", http.StatusOK},

		// /v1/input is registered by both the logs intake and the profiler.
		// That is not a collision: each lives on its own host.
		{"http-intake.logs." + testSite, http.MethodPost, "/v1/input", http.StatusAccepted},
		{"intake.profile." + testSite, http.MethodPost, "/v1/input", http.StatusAccepted},

		// Multi-Region Failover splices "mrf." in after the product prefix,
		// so the prefix match lands the failover copy on the same intake.
		{"sbom-intake.logs.mrf." + testSite, http.MethodPost, "/api/v2/sbom", http.StatusAccepted},
		{"app.mrf." + testSite, http.MethodGet, "/api/v1/validate", http.StatusOK},

		// browser-intake.<site> carries browser, iOS and Android alike —
		// Datadog puts all three on one host and so do we.
		{"browser-intake." + testSite, http.MethodPost, "/api/v2/rum", http.StatusAccepted},
		{"browser-intake." + testSite, http.MethodPost, "/api/v2/replay", http.StatusAccepted},
		{"browser-intake." + testSite, http.MethodPost, "/api/v2/spans", http.StatusAccepted},
		{"browser-intake." + testSite, http.MethodPost, "/api/v2/logs", http.StatusAccepted},
		{"browser-intake." + testSite, http.MethodPost, "/api/v2/profile", http.StatusAccepted},
		{"browser-intake." + testSite, http.MethodPost, "/api/v2/debugger", http.StatusAccepted},

		// Port and trailing dot are stripped before matching.
		{"sbom-intake." + testSite + ":8443", http.MethodPost, "/api/v2/sbom", http.StatusAccepted},
		{"sbom-intake." + testSite + ".", http.MethodPost, "/api/v2/sbom", http.StatusAccepted},
	}
	for _, tc := range cases {
		rec := call(h, tc.method, tc.host, tc.path, true)
		if rec.Code != tc.want {
			t.Errorf("%s %s on %s: status %d, want %d (body %q)",
				tc.method, tc.path, tc.host, rec.Code, tc.want, rec.Body.String())
		}
	}
}

func TestHostRoutingIsolatesIntakes(t *testing.T) {
	h := newTestHandler(t)

	// The same path on a host that belongs to another intake is unknown
	// there: the engine's NoRoute answers, not the other intake's handler.
	cases := []struct{ host, path string }{
		{"cws-intake." + testSite, "/api/v2/sbom"},
		{"sbom-intake." + testSite, "/api/v2/secdump"},
		{"api." + testSite, "/api/v2/logs"},
		{"intake.profile." + testSite, "/api/v2/logs"},
		{"http-intake.logs." + testSite, "/api/v2/profile"},
		// /api/v2/rum and /api/v2/replay belong to the RUM host alone.
		{"http-intake.logs." + testSite, "/api/v2/rum"},
		{"browser-intake." + testSite, "/api/v2/sbom"},
	}
	for _, tc := range cases {
		rec := call(h, http.MethodPost, tc.host, tc.path, true)
		if rec.Code != http.StatusNotFound {
			t.Errorf("POST %s on %s: status %d, want 404", tc.path, tc.host, rec.Code)
		}
		if !strings.Contains(rec.Body.String(), "unknown endpoint") {
			t.Errorf("POST %s on %s: body %q, want the unknown-endpoint error", tc.path, tc.host, rec.Body.String())
		}
	}
}

func TestUnknownHostIsRefusedByName(t *testing.T) {
	h := newTestHandler(t)

	for _, host := range []string{
		"nobody." + testSite,
		testSite,
		"sbom-intake-" + testSite, // dash instead of dot: not our prefix
	} {
		rec := call(h, http.MethodPost, host, "/api/v2/sbom", true)
		if rec.Code != http.StatusNotFound {
			t.Errorf("host %q: status %d, want 404", host, rec.Code)
		}
		if !strings.Contains(rec.Body.String(), `no intake for host "`+host+`"`) {
			t.Errorf("host %q: body %q does not name the host", host, rec.Body.String())
		}
		if ct := rec.Header().Get("Content-Type"); ct != "application/json" {
			t.Errorf("host %q: Content-Type %q, want application/json", host, ct)
		}
	}

	// The port is not part of the name we report.
	rec := call(h, http.MethodGet, "nobody."+testSite+":8443", "/ping", false)
	if rec.Code != http.StatusNotFound || !strings.Contains(rec.Body.String(), `"nobody.`+testSite+`"`) {
		t.Errorf("host with port: status %d body %q", rec.Code, rec.Body.String())
	}
}

func TestAPIKeyIsRequiredEverywhereButInstall(t *testing.T) {
	h := newTestHandler(t)

	// install.datadoghq.com never sees a key: OCI registry, BTF and the
	// HEAD probe are anonymous, so the host sits outside RequireAPIKey.
	for _, host := range []string{"install.datadoghq.com", "install.datad0g.com"} {
		if rec := call(h, http.MethodHead, host, "/", false); rec.Code != http.StatusOK {
			t.Errorf("HEAD / on %s without key: status %d, want 200", host, rec.Code)
		}
		if rec := call(h, http.MethodGet, host, "/v2/", false); rec.Code != http.StatusOK {
			t.Errorf("GET /v2/ on %s without key: status %d, want 200", host, rec.Code)
		}
	}

	// Every other host refuses a missing or unknown key with 403, before
	// any handler runs.
	for _, tc := range []struct{ host, method, path string }{
		{"sbom-intake." + testSite, http.MethodPost, "/api/v2/sbom"},
		{"api." + testSite, http.MethodGet, "/api/v1/validate"},
		{"http-intake.logs." + testSite, http.MethodPost, "/v1/input"},
		{"browser-intake." + testSite, http.MethodPost, "/api/v2/rum"},
	} {
		rec := call(h, tc.method, tc.host, tc.path, false)
		if rec.Code != http.StatusForbidden {
			t.Errorf("%s %s on %s without key: status %d, want 403", tc.method, tc.path, tc.host, rec.Code)
		}
	}

	req := httptest.NewRequest(http.MethodPost, "http://sbom-intake."+testSite+"/api/v2/sbom", nil)
	req.Host = "sbom-intake." + testSite
	req.Header.Set("Dd-Api-Key", "not-the-key")
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusForbidden {
		t.Errorf("wrong key: status %d, want 403", rec.Code)
	}
}

func TestProbesNeedNoKey(t *testing.T) {
	h := newTestHandler(t)

	for _, host := range []string{
		"api." + testSite,
		"sbom-intake." + testSite,
		"http-intake.logs." + testSite,
		"install.datadoghq.com",
		"browser-intake." + testSite,
	} {
		for _, path := range []string{"/ping", "/_health"} {
			rec := call(h, http.MethodGet, host, path, false)
			if rec.Code != http.StatusOK {
				t.Errorf("GET %s on %s without key: status %d, want 200", path, host, rec.Code)
			}
		}
	}
}

// The browser SDK treats a CORS failure as success and discards the batch, so
// the RUM host's preflight and its access-control headers are part of routing
// rather than a detail of one handler: without them the host exists and loses
// everything sent to it, silently.
func TestRUMHostAnswersPreflight(t *testing.T) {
	h := newTestHandler(t)

	req := httptest.NewRequest(http.MethodOptions, "http://browser-intake."+testSite+"/api/v2/rum", nil)
	req.Host = "browser-intake." + testSite
	req.Header.Set("Origin", "https://shop.example")
	req.Header.Set("Access-Control-Request-Method", "POST")
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)

	if rec.Code != http.StatusNoContent {
		t.Fatalf("preflight: status %d, want 204 (body %q)", rec.Code, rec.Body.String())
	}
	if got := rec.Header().Get("Access-Control-Allow-Origin"); got != "https://shop.example" {
		t.Errorf("Access-Control-Allow-Origin: got %q, want the request origin", got)
	}
}

// The browser SDK cannot set a header on a beacon, so the RUM host is the one
// intake that takes its key from the query string — and the only one.
func TestRUMHostAcceptsQueryStringKey(t *testing.T) {
	h := newTestHandler(t)

	host := "browser-intake." + testSite
	rec := call(h, http.MethodPost, host, "/api/v2/rum?ddsource=browser&dd-api-key="+testKey, false)
	if rec.Code != http.StatusAccepted {
		t.Errorf("query-string key on %s: status %d, want 202 (body %q)", host, rec.Code, rec.Body.String())
	}

	// Everywhere else the key still has to be a header (or api_key, the
	// grandfathered /api/v1/validate spelling) — dd-api-key is not accepted.
	rec = call(h, http.MethodPost, "sbom-intake."+testSite, "/api/v2/sbom?dd-api-key="+testKey, false)
	if rec.Code != http.StatusForbidden {
		t.Errorf("dd-api-key on an agent intake: status %d, want 403 — query-string auth stays with the one client that has no choice", rec.Code)
	}
}
