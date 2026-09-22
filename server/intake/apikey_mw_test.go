package intake

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/apikeys"
)

// The agent authenticates GET /api/v1/validate with a query parameter rather
// than the usual header. Refusing it does not merely fail that one call: the
// forwarder reports itself unhealthy, the agent's readiness probe turns 500,
// and Kubernetes pulls a pod that is otherwise shipping data correctly.
func TestAPIKeyAcceptedFromQueryParam(t *testing.T) {
	gin.SetMode(gin.TestMode)

	const key = "0123456789abcdef0123456789abcdef"
	store := apikeys.NewStore()
	store.Publish(
		[]apikeys.Key{{ID: "1", Name: "lab", TenantID: "t1"}},
		[]string{apikeys.Hash(key)},
	)

	run := func(target string, header string) int {
		r := gin.New()
		r.Use(RequireAPIKey(store))
		r.GET("/api/v1/validate", func(c *gin.Context) { c.Status(http.StatusOK) })
		req := httptest.NewRequest(http.MethodGet, target, nil)
		if header != "" {
			req.Header.Set("Dd-Api-Key", header)
		}
		w := httptest.NewRecorder()
		r.ServeHTTP(w, req)
		return w.Code
	}

	if got := run("/api/v1/validate?api_key="+key, ""); got != http.StatusOK {
		t.Errorf("query parameter key: got %d, want 200", got)
	}
	if got := run("/api/v1/validate", key); got != http.StatusOK {
		t.Errorf("header key: got %d, want 200", got)
	}
	if got := run("/api/v1/validate?api_key=wrong", ""); got != http.StatusForbidden {
		t.Errorf("wrong key in query: got %d, want 403", got)
	}
	if got := run("/api/v1/validate", ""); got != http.StatusForbidden {
		t.Errorf("no key at all: got %d, want 403", got)
	}
}

// Each KeySource must find the key where its client puts it, and the sources
// must be tried in order until one produces something.
//
// The bearer case is not hypothetical: OpenLineage's HTTP transport sends
// "Authorization: Bearer <key>" and the trace-agent's lineage proxy forwards
// it verbatim, so data-obs-intake answered 403 to a correctly configured agent
// until its engine was given this source.
func TestKeySources(t *testing.T) {
	gin.SetMode(gin.TestMode)

	const key = "0123456789abcdef0123456789abcdef"
	store := apikeys.NewStore()
	store.Publish(
		[]apikeys.Key{{ID: "1", Name: "lab", TenantID: "t1"}},
		[]string{apikeys.Hash(key)},
	)

	// One guard with every source, so a case that passes proves the source it
	// names fired — no other source could have supplied the key.
	all := RequireAPIKeyFrom(store,
		KeyFromHeader("Dd-Api-Key"),
		KeyFromBearer(),
		KeyFromQuery("api_key"),
	)

	cases := []struct {
		name    string
		auth    gin.HandlerFunc
		target  string
		headers map[string]string
		want    int
	}{
		{
			name: "header", auth: all, target: "/x",
			headers: map[string]string{"Dd-Api-Key": key}, want: http.StatusOK,
		},
		{
			// Go canonicalises DD-API-KEY to the same header, so the agent's
			// own spelling has to work too.
			name: "header in the agent's spelling", auth: all, target: "/x",
			headers: map[string]string{"DD-API-KEY": key}, want: http.StatusOK,
		},
		{
			name: "bearer", auth: all, target: "/x",
			headers: map[string]string{"Authorization": "Bearer " + key}, want: http.StatusOK,
		},
		{
			// RFC 7235 makes the scheme case-insensitive and clients disagree.
			name: "bearer, lowercase scheme", auth: all, target: "/x",
			headers: map[string]string{"Authorization": "bearer " + key}, want: http.StatusOK,
		},
		{
			// Not a Bearer credential: the source must yield nothing rather
			// than hash "dXNlcjpwYXNz" and hand the next source a used turn.
			name: "basic auth is not a bearer key", auth: all, target: "/x",
			headers: map[string]string{"Authorization": "Basic " + key}, want: http.StatusForbidden,
		},
		{
			name: "query", auth: all, target: "/x?api_key=" + key, want: http.StatusOK,
		},
		{
			name: "no key at all", auth: all, target: "/x", want: http.StatusForbidden,
		},
		{
			// The default guard must NOT accept a bearer token. Query-string
			// and Authorization auth are enabled per intake, deliberately:
			// a key in a URL leaks into proxy logs, and a guard that accepts
			// everything everywhere makes that choice for every endpoint.
			name: "default guard ignores bearer",
			auth: RequireAPIKey(store), target: "/x",
			headers: map[string]string{"Authorization": "Bearer " + key},
			want:    http.StatusForbidden,
		},
		{
			name: "default guard still takes the header",
			auth: RequireAPIKey(store), target: "/x",
			headers: map[string]string{"Dd-Api-Key": key}, want: http.StatusOK,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			r := gin.New()
			r.Use(tc.auth)
			r.GET("/x", func(c *gin.Context) { c.Status(http.StatusOK) })

			req := httptest.NewRequest(http.MethodGet, tc.target, nil)
			for k, v := range tc.headers {
				req.Header.Set(k, v)
			}
			w := httptest.NewRecorder()
			r.ServeHTTP(w, req)

			if w.Code != tc.want {
				t.Errorf("got %d, want %d", w.Code, tc.want)
			}
		})
	}
}
