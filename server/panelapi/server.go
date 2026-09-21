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
		internal.GET("/metrics/tags", a.HandleMetricTagKeys)
		internal.GET("/metrics/tag-values", a.HandleMetricTagValues)
		internal.GET("/metrics/query", a.HandleMetricQuery)

		internal.GET("/logs/search", a.HandleLogSearch)
		internal.GET("/logs/facets", a.HandleLogFacets)
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

// HandleMetricTagKeys lists tag keys, narrowed to one metric with ?metric=.
func (a *Server) HandleMetricTagKeys(c *gin.Context) {
	res, ok := a.ask(c, query.ListTagKeys{
		TenantID: a.tenant(c),
		Metric:   c.Query("metric"),
	})
	if !ok {
		return
	}
	list, ok := res.(query.TagKeysList)
	if !ok {
		a.unexpected(c, res)
		return
	}
	c.JSON(http.StatusOK, gin.H{"keys": list.Keys})
}

// HandleMetricTagValues lists the values one tag key holds.
//
//	?key=env         required
//	?metric=...      optional, narrows to one metric
//	?search=pro      optional substring
//	?limit=200       optional
func (a *Server) HandleMetricTagValues(c *gin.Context) {
	key := c.Query("key")
	if key == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "key is required"})
		return
	}
	limit, _ := strconv.Atoi(c.Query("limit"))
	res, ok := a.ask(c, query.ListTagValues{
		TenantID: a.tenant(c),
		Metric:   c.Query("metric"),
		Key:      key,
		Search:   c.Query("search"),
		Limit:    limit,
	})
	if !ok {
		return
	}
	list, ok := res.(query.TagValuesList)
	if !ok {
		a.unexpected(c, res)
		return
	}
	c.JSON(http.StatusOK, gin.H{"values": list.Values})
}

// HandleMetricQuery returns one metric over a time range, one series per
// host — or, with ?by=, one series per combination of tag values.
//
//	?metric=ninjacat.node.goroutines   required
//	?from=-1h | RFC3339                default -1h
//	?to=now | RFC3339                  default now
//	?host=a&host=b                     optional, repeatable
//	?tag=env:prod&tag=svc:a&tag=svc:b  optional, repeatable; same key ORs,
//	                                   different keys AND — Datadog scoping
//	?by=env&by=service                 optional, repeatable; group by tags
//	                                   instead of by host
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

	tags, err := parseTagFilters(c.QueryArray("tag"))
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
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
		Tags:        tags,
		GroupBy:     c.QueryArray("by"),
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
// Logs
// ---------------------------------------------------------------------------

// HandleLogSearch returns matching log lines, newest first, plus the total
// match count.
//
//	?q=connection refused    optional free-text phrase over message
//	?service=nginx           optional exact match
//	?host=web-1              optional exact match
//	?status=error            optional exact match
//	?tag=env:prod            optional, repeatable; same semantics as metrics
//	?from=-1h | RFC3339      default -1h
//	?to=now | RFC3339        default now
//	?limit=200               default 200, capped at 1000
func (a *Server) HandleLogSearch(c *gin.Context) {
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

	tags, err := parseTagFilters(c.QueryArray("tag"))
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	limit, _ := strconv.Atoi(c.Query("limit"))

	res, ok := a.ask(c, query.SearchLogs{
		TenantID: a.tenant(c),
		Query:    c.Query("q"),
		Service:  c.Query("service"),
		Host:     c.Query("host"),
		Status:   c.Query("status"),
		Tags:     tags,
		From:     from,
		To:       to,
		Limit:    limit,
	})
	if !ok {
		return
	}
	result, ok := res.(query.LogsResult)
	if !ok {
		a.unexpected(c, res)
		return
	}
	c.JSON(http.StatusOK, gin.H{"logs": result.Logs, "count": result.Count})
}

// HandleLogFacets returns the explorer sidebar: top services, hosts and
// statuses by line count in the window.
//
//	?from=-1h | RFC3339      default -1h
//	?to=now | RFC3339        default now
func (a *Server) HandleLogFacets(c *gin.Context) {
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

	res, ok := a.ask(c, query.ListLogFacets{
		TenantID: a.tenant(c),
		From:     from,
		To:       to,
	})
	if !ok {
		return
	}
	facets, ok := res.(query.LogFacets)
	if !ok {
		a.unexpected(c, res)
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"services": facets.Services,
		"hosts":    facets.Hosts,
		"statuses": facets.Statuses,
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

// parseTagFilters turns repeated ?tag=key:value params into filters.
//
// Repeats of the SAME key merge into one filter and are OR-ed by the worker;
// distinct keys stay separate filters and are AND-ed. That asymmetry is how
// Datadog scopes read: "kube_service:a kube_service:b env:prod" means "(in
// service a OR b) AND in prod". Key order is first appearance, values keep
// their given order — the worker does not care, but stable output makes the
// tests honest.
//
// The value may itself contain colons (an image tag, a URL); only the first
// colon splits.
func parseTagFilters(raw []string) ([]query.TagFilter, error) {
	var filters []query.TagFilter
	index := map[string]int{}
	for _, t := range raw {
		key, value, ok := strings.Cut(t, ":")
		if !ok || key == "" || value == "" {
			return nil, fmt.Errorf("bad tag %q: want key:value", t)
		}
		if i, seen := index[key]; seen {
			filters[i].Values = append(filters[i].Values, value)
			continue
		}
		index[key] = len(filters)
		filters = append(filters, query.TagFilter{Key: key, Values: []string{value}})
	}
	return filters, nil
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
