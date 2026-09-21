// Package panelapi serves the SvelteKit panel.
//
// It listens on a different port from the agent intake, and that separation is
// deliberate: :8080 takes traffic from agents and has to be reachable from
// their machines, while :8081 serves the panel and can stay on an internal
// network.
//
// Nothing here talks to a database. Every handler translates HTTP into a
// message and a message back into JSON; the work happens in apps/query and
// apps/apikeys. That is what keeps this file free of SQL and lets the same
// panel work once querying moves to another node.
package panelapi

import (
	"fmt"
	"net/http"
	"os"
	"strconv"
	"strings"
	"time"

	"ergo.services/ergo/gen"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/apikeys"
	"github.com/itsninjacats/server/apps/query"
)

// callTimeout bounds how long a handler waits for an actor, in seconds.
//
// It is slightly longer than the worker's own query timeout, so a slow query
// fails with the worker's message rather than with an opaque call timeout.
const callTimeout = 8

// defaultTenant is used until the panel has real sessions carrying a tenant.
const defaultTenant = "default"

// maxPoints caps how many buckets one series may return.
//
// This is what keeps a careless request — a month at one-second resolution —
// from trying to render two and a half million points in a browser. The step
// is widened to fit instead of the request being rejected.
const maxPoints = 2000

type Server struct {
	Node  gen.Node
	Store *apikeys.Store
	r     *gin.Engine
}

// Handler builds the router and returns it. It does not block — the
// meta-process in apps/httpapi runs it.
func (a *Server) Handler() http.Handler {
	r := gin.New()
	r.Use(gin.Recovery())
	if os.Getenv("DEBUG") == "true" {
		r.Use(gin.Logger())
	}
	a.r = r

	r.GET("/ping", a.HandlePing)

	internal := r.Group("/internal")
	{
		internal.POST("/apikeys/refresh", a.HandleRefreshKeys)
		internal.GET("/apikeys/status", a.HandleKeyStatus)

		internal.GET("/metrics/names", a.HandleMetricNames)
		internal.GET("/metrics/hosts", a.HandleMetricHosts)
		internal.GET("/metrics/query", a.HandleMetricQuery)
	}

	return r
}

func (a *Server) HandlePing(c *gin.Context) {
	c.JSON(http.StatusOK, gin.H{"message": "pong"})
}

// ---------------------------------------------------------------------------
// API keys
// ---------------------------------------------------------------------------

// HandleRefreshKeys tells the keeper to re-read the keys from the database now.
//
// The panel calls this right after a key is added or removed, so the change
// takes effect immediately instead of waiting up to 30 seconds for the cycle.
//
// This is message passing used well: a rare control event. Checking keys on the
// hot path takes a completely different route — through atomic.Pointer, with no
// message at all.
func (a *Server) HandleRefreshKeys(c *gin.Context) {
	if a.Node == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "no node"})
		return
	}

	// Send is asynchronous — we do not wait for the keeper to finish reading.
	if err := a.Node.Send(apikeys.KeeperName, apikeys.Refresh{}); err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": err.Error()})
		return
	}

	c.JSON(http.StatusAccepted, gin.H{"status": "refresh requested"})
}

// HandleKeyStatus shows what the keeper currently holds in memory.
// A Call is appropriate here: called rarely, and we want the actor's answer.
func (a *Server) HandleKeyStatus(c *gin.Context) {
	if a.Store == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "no key store"})
		return
	}
	snap := a.Store.Current()
	c.JSON(http.StatusOK, gin.H{
		"keys":      snap.Len(),
		"refreshed": snap.UpdatedAt,
	})
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

// HandleMetricNames lists metric names, optionally filtered by ?search=.
func (a *Server) HandleMetricNames(c *gin.Context) {
	limit, _ := strconv.Atoi(c.Query("limit"))
	res, ok := a.ask(c, query.ListMetrics{
		TenantID: a.tenant(c),
		Search:   c.Query("search"),
		Limit:    limit,
	})
	if !ok {
		return
	}
	list, ok := res.(query.MetricsList)
	if !ok {
		a.unexpected(c, res)
		return
	}
	c.JSON(http.StatusOK, gin.H{"metrics": list.Names})
}

// HandleMetricHosts lists hosts, narrowed to one metric with ?metric=.
func (a *Server) HandleMetricHosts(c *gin.Context) {
	res, ok := a.ask(c, query.ListHosts{
		TenantID: a.tenant(c),
		Metric:   c.Query("metric"),
	})
	if !ok {
		return
	}
	list, ok := res.(query.HostsList)
	if !ok {
		a.unexpected(c, res)
		return
	}
	c.JSON(http.StatusOK, gin.H{"hosts": list.Hosts})
}

// HandleMetricQuery returns one metric over a time range, one series per host.
//
//	?metric=ninjacat.node.goroutines   required
//	?from=-1h | RFC3339                default -1h
//	?to=now | RFC3339                  default now
//	?host=a&host=b                     optional, repeatable
//	?step=60s                          optional, derived from the range if absent
//	?agg=avg|min|max|sum|count         default avg
func (a *Server) HandleMetricQuery(c *gin.Context) {
	metric := c.Query("metric")
	if metric == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "metric is required"})
		return
	}

	now := time.Now().UTC()
	from, err := parseTime(c.Query("from"), now.Add(-time.Hour), now)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "bad from: " + err.Error()})
		return
	}
	to, err := parseTime(c.Query("to"), now, now)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "bad to: " + err.Error()})
		return
	}
	if !to.After(from) {
		c.JSON(http.StatusBadRequest, gin.H{"error": "to must be after from"})
		return
	}

	step := resolveStep(c.Query("step"), to.Sub(from))

	res, ok := a.ask(c, query.QuerySeries{
		TenantID:    a.tenant(c),
		Metric:      metric,
		Hosts:       c.QueryArray("host"),
		From:        from,
		To:          to,
		Step:        step,
		Aggregation: c.Query("agg"),
	})
	if !ok {
		return
	}
	result, ok := res.(query.SeriesResult)
	if !ok {
		a.unexpected(c, res)
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"metric": metric,
		"from":   from,
		"to":     to,
		"step":   int(result.Step.Seconds()),
		"series": result.Series,
	})
}

// ---------------------------------------------------------------------------
// Plumbing
// ---------------------------------------------------------------------------

// ask sends a request to the query pool and unwraps the common failures, so
// each handler is left with nothing but its own result type to check.
func (a *Server) ask(c *gin.Context, request any) (any, bool) {
	if a.Node == nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": "no node"})
		return nil, false
	}
	res, err := a.Node.CallWithTimeout(query.PoolName, request, callTimeout)
	if err != nil {
		c.JSON(http.StatusServiceUnavailable, gin.H{"error": err.Error()})
		return nil, false
	}
	if failed, isFailed := res.(query.Failed); isFailed {
		c.JSON(http.StatusBadRequest, gin.H{"error": failed.Reason})
		return nil, false
	}
	return res, true
}

func (a *Server) unexpected(c *gin.Context, res any) {
	c.JSON(http.StatusInternalServerError, gin.H{"error": "unexpected reply from query pool"})
}

// tenant is where multi-tenancy will be wired in. The panel session will carry
// the tenant; until it does, everything reads the default one.
func (a *Server) tenant(c *gin.Context) string {
	if t := c.Query("tenant"); t != "" {
		return t
	}
	return defaultTenant
}

// parseTime accepts RFC3339, "now", or a relative offset such as "-24h".
//
// Relative offsets are what a dashboard actually uses: a saved view means "the
// last six hours", not "six hours before the day I saved it".
func parseTime(raw string, fallback, now time.Time) (time.Time, error) {
	raw = strings.TrimSpace(raw)
	switch {
	case raw == "":
		return fallback, nil
	case raw == "now":
		return now, nil
	case strings.HasPrefix(raw, "-") || strings.HasPrefix(raw, "+"):
		d, err := parseOffset(raw)
		if err != nil {
			return time.Time{}, err
		}
		return now.Add(d), nil
	}
	t, err := time.Parse(time.RFC3339, raw)
	if err != nil {
		return time.Time{}, err
	}
	return t.UTC(), nil
}

// parseOffset is time.ParseDuration plus days and weeks.
//
// Go's parser stops at hours, which is fine for timeouts and useless for a
// dashboard: "-7d" and "-30d" are the two ranges people reach for most. Rather
// than make callers write "-720h", the suffix is converted before parsing.
func parseOffset(raw string) (time.Duration, error) {
	units := []struct {
		suffix string
		mul    time.Duration
	}{
		{"w", 7 * 24 * time.Hour},
		{"d", 24 * time.Hour},
	}
	for _, u := range units {
		if !strings.HasSuffix(raw, u.suffix) {
			continue
		}
		n, err := strconv.ParseFloat(strings.TrimSuffix(raw, u.suffix), 64)
		if err != nil {
			return 0, fmt.Errorf("bad offset %q", raw)
		}
		return time.Duration(n * float64(u.mul)), nil
	}
	return time.ParseDuration(raw)
}

// resolveStep picks the bucket width.
//
// An explicit step is honoured but still capped, so the caller cannot ask for
// more points than a chart can draw. Without one, the step is derived to land
// near 300 points — dense enough to show shape, light enough to render.
func resolveStep(raw string, span time.Duration) time.Duration {
	var step time.Duration
	if raw != "" {
		if d, err := time.ParseDuration(raw); err == nil && d > 0 {
			step = d
		}
	}
	if step == 0 {
		step = span / 300
	}
	if min := span / maxPoints; step < min {
		step = min
	}
	if step < time.Second {
		step = time.Second
	}
	return step.Round(time.Second)
}
