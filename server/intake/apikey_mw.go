package intake

import (
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/apikeys"
)

// Context key under which the recognised API key is stored for the request.
const ctxKeyAPIKey = "ninjacat.apikey"

// KeySource extracts a candidate API key from a request, or "" when it has
// none to offer. Sources are cheap by contract: they are on the hot path, and
// one that parsed a body or allocated per call would not belong here.
type KeySource func(c *gin.Context) string

// KeyFromHeader reads a plain header.
//
// Header names are canonicalised by net/http, so KeyFromHeader("Dd-Api-Key")
// matches DD-API-KEY, dd-api-key and every other spelling an agent or proxy
// might send.
func KeyFromHeader(name string) KeySource {
	return func(c *gin.Context) string { return c.GetHeader(name) }
}

// KeyFromBearer reads "Authorization: Bearer <key>".
//
// The scheme is matched case-insensitively (RFC 7235 says it is
// case-insensitive, and clients disagree in practice). Anything that is not a
// Bearer credential — Basic, a bare token — yields "", so the next source
// gets its turn instead of a garbage lookup.
func KeyFromBearer() KeySource {
	return func(c *gin.Context) string {
		v := c.GetHeader("Authorization")
		if len(v) < 7 || !strings.EqualFold(v[:7], "Bearer ") {
			return ""
		}
		return strings.TrimSpace(v[7:])
	}
}

// KeyFromQuery reads a query parameter. See RequireAPIKeyFrom for why this is
// not enabled by default.
func KeyFromQuery(name string) KeySource {
	return func(c *gin.Context) string { return c.Query(name) }
}

// RequireAPIKey is the default: the Dd-Api-Key header, then the api_key query
// parameter.
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
//
// The query parameter is second, not first, so the common request costs one
// header read and never parses a query string. It is there because GET
// /api/v1/validate, which the agent calls at startup and then periodically,
// passes the key that way:
//
//	GET /api/v1/validate?api_key=<key>
//
// Rejecting that costs far more than a 403 on one endpoint: the forwarder
// cannot validate its key, marks ITSELF unhealthy, and the agent's /ready
// probe starts returning 500 — so Kubernetes takes the pod out of service even
// though metrics, logs and orchestrator payloads are all still flowing
// perfectly. Observed exactly that way in the k8s lab: 485 requests answered
// 202, and the only failures in the whole capture were eight 403s on
// /api/v1/validate.
func RequireAPIKey(store *apikeys.Store) gin.HandlerFunc {
	return RequireAPIKeyFrom(store,
		KeyFromHeader("Dd-Api-Key"),
		KeyFromQuery("api_key"),
	)
}

// RequireAPIKeyFrom is RequireAPIKey with the extraction made explicit: the
// sources are tried in order and the first non-empty candidate is looked up.
//
// It exists because not every Datadog product authenticates the same way, and
// the differences are the clients', not ours. OpenLineage's HTTP transport
// sends "Authorization: Bearer <key>" because that is the OpenLineage
// convention, and the trace-agent's lineage proxy forwards it verbatim — so
// data-obs-intake needs KeyFromBearer or it answers 403 to a correctly
// configured agent. The browser RUM SDK cannot set a header on a beacon at
// all and has only the query string.
//
// WHY QUERY-STRING AUTH IS NOT ON BY DEFAULT: a key in a URL leaks. It lands
// in proxy access logs, in load-balancer logs, in Referer headers and in
// browser history, all of which are kept longer and read more widely than
// anything that holds a header. The one intake that has no alternative gets it
// explicitly; everything else gets the header. (/api/v1/validate is the
// grandfathered exception in RequireAPIKey — the agent gives us no choice
// there either.)
//
// Ordering is the only thing that costs anything: put the source the intake
// actually uses first, so the usual request stops at the first check. The hot
// path is unchanged either way — the work is still a pointer load and one map
// lookup, and the sources only decide which string is hashed.
func RequireAPIKeyFrom(store *apikeys.Store, sources ...KeySource) gin.HandlerFunc {
	return func(c *gin.Context) {
		if store == nil {
			// No store is a configuration fault, not the caller's problem.
			c.AbortWithStatusJSON(http.StatusServiceUnavailable,
				gin.H{"errors": []string{"key store unavailable"}})
			return
		}

		var key string
		for _, from := range sources {
			if key = from(c); key != "" {
				break
			}
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
