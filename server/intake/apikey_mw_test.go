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
