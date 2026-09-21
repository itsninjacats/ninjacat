package intake

import (
	"net/http"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/apikeys"
)

// Context key under which the recognised API key is stored for the request.
const ctxKeyAPIKey = "ninjacat.apikey"

// RequireAPIKey checks the Dd-Api-Key header on every agent request.
//
// This is the HOT PATH: a hundred agents reporting every 15 seconds put tens
// of thousands of requests a minute through here. Hence:
//
//   - no database query (the keeper holds the keys in memory)
//   - no message passing to an actor, which would be the bottleneck —
//     measured: 7336 ns per Call against 56 ns for an atomic.Pointer read
//   - no allocation: a pointer load and one map lookup
//
// See apps/apikeys/store.go for why this is safe without locking.
func RequireAPIKey(store *apikeys.Store) gin.HandlerFunc {
	return func(c *gin.Context) {
		if store == nil {
			// No store is a configuration fault, not the caller's problem.
			c.AbortWithStatusJSON(http.StatusServiceUnavailable,
				gin.H{"errors": []string{"key store unavailable"}})
			return
		}

		// The header is how the agent authenticates almost everything.
		key := c.GetHeader("Dd-Api-Key")
		if key == "" {
			// ...but not everything. GET /api/v1/validate, which the agent
			// calls at startup and then periodically, passes the key as a
			// QUERY PARAMETER instead:
			//
			//   GET /api/v1/validate?api_key=<key>
			//
			// Rejecting that costs far more than a 403 on one endpoint: the
			// forwarder cannot validate its key, marks ITSELF unhealthy, and
			// the agent's /ready probe starts returning 500 — so Kubernetes
			// takes the pod out of service even though metrics, logs and
			// orchestrator payloads are all still flowing perfectly.
			//
			// Observed exactly that way in the k8s lab: 485 requests answered
			// 202, and the only failures in the whole capture were eight 403s
			// on /api/v1/validate.
			//
			// Checked second, not first, so the hot path stays a single
			// header read — query parsing only happens on the rare request
			// that has no header at all.
			key = c.Query("api_key")
		}

		info, ok := store.Lookup(key)
		if !ok {
			// 403 is a signal the agent understands: bad key, stop sending.
			// On /api/v1/validate it stops the whole pipeline.
			c.AbortWithStatusJSON(http.StatusForbidden,
				gin.H{"errors": []string{"invalid API key"}})
			return
		}

		// From here on the handlers know whose traffic this is.
		c.Set(ctxKeyAPIKey, info)
		c.Next()
	}
}

// TenantFromContext returns the tenant the data in this request belongs to.
func TenantFromContext(c *gin.Context) string {
	k, ok := KeyFromContext(c)
	if !ok {
		return ""
	}
	return k.TenantID
}

// KeyFromContext returns the key recognised by RequireAPIKey.
func KeyFromContext(c *gin.Context) (apikeys.Key, bool) {
	v, ok := c.Get(ctxKeyAPIKey)
	if !ok {
		return apikeys.Key{}, false
	}
	k, ok := v.(apikeys.Key)
	return k, ok
}
