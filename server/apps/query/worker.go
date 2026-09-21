package query

import (
	"context"
	"fmt"
	"sort"
	"strings"
	"time"

	"ergo.services/ergo/act"
	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// queryTimeout caps a single ClickHouse query.
//
// A panel request that has not answered in five seconds is already a bad
// experience, and letting it run longer only ties up a worker that the next
// request needs.
const queryTimeout = 5 * time.Second

// aggregations is the allow-list for the per-bucket function.
//
// The value is interpolated into SQL — it cannot be a bound parameter, because
// a function name is not a value. So it never comes from the caller directly:
// whatever arrives is looked up here, and anything unknown becomes avg.
var aggregations = map[string]string{
	"avg": "avg", "min": "min", "max": "max", "sum": "sum", "count": "count",
}

// maxGroupByKeys caps how many tag keys one query may group by.
//
// The cap is not about what ClickHouse can compute — it is about what comes
// back. The series count is the product of the keys' cardinalities, and one
// high-cardinality key like pod_name already turns a chart into thousands of
// lines; four keys is past what any legend can label. Beyond the cap the
// request is refused with a clear reason rather than truncated, because a
// silently truncated result is a chart that looks complete and lies.
const maxGroupByKeys = 4

// Log search limits: the default page and the hard ceiling. The ceiling is
// enforced here and not only in the HTTP layer, because this package is the
// last stop before ClickHouse and must not trust its callers to have clamped.
const (
	defaultLogLimit = 200
	maxLogLimit     = 1000
)

// facetLimit is how many rows each sidebar list gets. Fifty covers every
// realistic service list; a sidebar longer than that needs search, not more
// rows.
const facetLimit = 50

// hotTagColumns maps a tag key to the materialised scalar column that mirrors
// it on the metrics table.
//
// Filtering on the scalar column is ~17x faster than reaching into the tags
// map — the column reads straight off disk while tags['k'] unpacks every
// row's map — so when a filter names one of these keys it goes through the
// column instead. Like aggregations above, the column name is interpolated
// into SQL and therefore only ever comes from this map, never from the
// request.
//
// The columns materialise tags['k'][1], the FIRST value only. That is safe
// for these particular keys because they are single-valued in practice (a
// pod has one namespace, one deployment, one name), and it is the trade the
// schema already made — see docs/decisions/0001-tags-are-a-multiset.md.
var hotTagColumns = map[string]string{
	"env":             "env",
	"service":         "service",
	"kube_namespace":  "kube_namespace",
	"kube_deployment": "kube_deployment",
	"pod_name":        "pod_name",
}

// tagFilterSQL renders tag filters as a run of AND-ed predicates plus their
// bound arguments, to be appended to a WHERE clause.
//
// Filters AND together; the values inside one filter OR together (IN on a
// column, hasAny on the map) — the Datadog scope semantics that TagFilter
// documents. Matching against the map is membership, never equality, because
// a key holds an ARRAY of values: has()/hasAny() find kube_service:a on a
// point that also carries kube_service:b, where equality would miss it.
//
// A tag key is data, not an identifier, even though it selects "which tag".
// It is bound through a placeholder inside tags[?] exactly like a value, so
// a key containing a quote is just a key that matches nothing — it can never
// terminate the string literal and rewrite the query.
//
// useHotColumns says whether the table has the materialised scalar columns;
// metrics does, logs does not.
func tagFilterSQL(filters []TagFilter, useHotColumns bool) (string, []any, error) {
	var sb strings.Builder
	var args []any
	for _, f := range filters {
		if f.Key == "" || len(f.Values) == 0 {
			return "", nil, fmt.Errorf("tag filter needs a key and at least one value")
		}
		if col, ok := hotTagColumns[f.Key]; ok && useHotColumns {
			if len(f.Values) == 1 {
				sb.WriteString(" AND " + col + " = ?")
				args = append(args, f.Values[0])
			} else {
				sb.WriteString(" AND " + col + " IN (?)")
				args = append(args, f.Values)
			}
			continue
		}
		if len(f.Values) == 1 {
			sb.WriteString(" AND has(tags[?], ?)")
			args = append(args, f.Key, f.Values[0])
		} else {
			sb.WriteString(" AND hasAny(tags[?], ?)")
			args = append(args, f.Key, f.Values)
		}
	}
	return sb.String(), args, nil
}

// normalizeGroupBy validates the group-by list and returns it sorted and
// deduplicated.
//
// Sorted, because the series name is the values joined in key order and the
// same group must always produce the same name; deduplicated, because a
// repeated key would arrayJoin the same array twice and square the row count
// for no meaning at all.
func normalizeGroupBy(keys []string) ([]string, error) {
	if len(keys) == 0 {
		return nil, nil
	}
	seen := make(map[string]struct{}, len(keys))
	out := make([]string, 0, len(keys))
	for _, k := range keys {
		if k == "" {
			return nil, fmt.Errorf("group-by key must not be empty")
		}
		if _, dup := seen[k]; dup {
			continue
		}
		seen[k] = struct{}{}
		out = append(out, k)
	}
	if len(out) > maxGroupByKeys {
		return nil, fmt.Errorf("too many group-by keys: %d given, at most %d allowed", len(out), maxGroupByKeys)
	}
	sort.Strings(out)
	return out, nil
}

// groupColumnsSQL renders the select list for a tag grouping: one
// arrayJoin(tags[?]) per key, aliased g0..gN.
//
// arrayJoin fans a row out into one output row per value the key holds, so a
// pod carrying kube_service:a and kube_service:b contributes its points to
// BOTH series — the multiset semantics again, and exactly what Datadog does.
// A row where the key is absent has an empty array, produces no output rows,
// and so belongs to no group; a point without the tag has nothing to say
// about any of these series.
//
// Keys are bound, never spliced — same reasoning as tagFilterSQL.
func groupColumnsSQL(keys []string) (string, []any) {
	cols := make([]string, len(keys))
	args := make([]any, len(keys))
	for i, k := range keys {
		cols[i] = fmt.Sprintf("arrayJoin(tags[?]) AS g%d", i)
		args[i] = k
	}
	return strings.Join(cols, ", "), args
}

// messageTokens splits free text the way the tokenbf_v1 index does: into runs
// of ASCII letters and digits.
//
// This matters for correctness, not just speed — hasToken() throws if its
// needle contains a separator character, so the phrase has to be broken into
// clean tokens before it goes anywhere near the index.
func messageTokens(s string) []string {
	return strings.FieldsFunc(s, func(r rune) bool {
		return !('0' <= r && r <= '9' || 'a' <= r && r <= 'z' || 'A' <= r && r <= 'Z')
	})
}

// Worker answers one query at a time.
//
// There are several of these behind a pool, which is the whole reason queries
// go to a pool and writes go to a single actor per table. A write is a buffer
// append that must keep its ordering; a read is an independent request that
// blocks for as long as ClickHouse takes. Serialising reads through one actor
// would mean one slow query stalls every dashboard on the screen.
type Worker struct {
	act.Actor
	conn driver.Conn
}

func (w *Worker) Init(args ...any) error {
	if len(args) == 0 {
		return fmt.Errorf("query worker: missing connection in args")
	}
	conn, ok := args[0].(driver.Conn)
	if !ok {
		return fmt.Errorf("query worker: first arg must be driver.Conn, got %T", args[0])
	}
	w.conn = conn
	return nil
}

// HandleCall is where every query lands. Returning Failed rather than an error
// keeps the worker alive: a malformed request from the panel is not a reason
// for the supervisor to restart anything.
func (w *Worker) HandleCall(from gen.PID, ref gen.Ref, request any) (any, error) {
	ctx, cancel := context.WithTimeout(context.Background(), queryTimeout)
	defer cancel()

	switch req := request.(type) {
	case ListMetrics:
		res, err := w.listMetrics(ctx, req)
		if err != nil {
			w.Log().Warning("query: list metrics: %s", err)
			return Failed{Reason: err.Error()}, nil
		}
		return res, nil

	case ListHosts:
		res, err := w.listHosts(ctx, req)
		if err != nil {
			w.Log().Warning("query: list hosts: %s", err)
			return Failed{Reason: err.Error()}, nil
		}
		return res, nil

	case QuerySeries:
		res, err := w.querySeries(ctx, req)
		if err != nil {
			w.Log().Warning("query: series: %s", err)
			return Failed{Reason: err.Error()}, nil
		}
		return res, nil

	case ListTagKeys:
		res, err := w.listTagKeys(ctx, req)
		if err != nil {
			w.Log().Warning("query: list tag keys: %s", err)
			return Failed{Reason: err.Error()}, nil
		}
		return res, nil

	case ListTagValues:
		res, err := w.listTagValues(ctx, req)
		if err != nil {
			w.Log().Warning("query: list tag values: %s", err)
			return Failed{Reason: err.Error()}, nil
		}
		return res, nil

	case SearchLogs:
		res, err := w.searchLogs(ctx, req)
		if err != nil {
			w.Log().Warning("query: search logs: %s", err)
			return Failed{Reason: err.Error()}, nil
		}
		return res, nil

	case ListLogFacets:
		res, err := w.listLogFacets(ctx, req)
		if err != nil {
			w.Log().Warning("query: log facets: %s", err)
			return Failed{Reason: err.Error()}, nil
		}
		return res, nil
	}

	return Failed{Reason: fmt.Sprintf("unknown request %T", request)}, nil
}

func (w *Worker) listMetrics(ctx context.Context, req ListMetrics) (MetricsList, error) {
	limit := req.Limit
	if limit <= 0 || limit > 1000 {
		limit = 500
	}

	// The metric column is LowCardinality, so this reads the dictionaries
	// rather than the values — cheap even over a large table.
	sql := `SELECT DISTINCT metric FROM metrics WHERE tenant_id = ?`
	args := []any{req.TenantID}
	if req.Search != "" {
		sql += ` AND positionCaseInsensitive(metric, ?) > 0`
		args = append(args, req.Search)
	}
	sql += ` ORDER BY metric LIMIT ?`
	args = append(args, limit)

	rows, err := w.conn.Query(ctx, sql, args...)
	if err != nil {
		return MetricsList{}, err
	}
	defer rows.Close()

	out := MetricsList{Names: []string{}}
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			return MetricsList{}, err
		}
		out.Names = append(out.Names, name)
	}
	return out, rows.Err()
}

func (w *Worker) listHosts(ctx context.Context, req ListHosts) (HostsList, error) {
	sql := `SELECT DISTINCT host FROM metrics WHERE tenant_id = ?`
	args := []any{req.TenantID}
	if req.Metric != "" {
		sql += ` AND metric = ?`
		args = append(args, req.Metric)
	}
	sql += ` ORDER BY host LIMIT 1000`

	rows, err := w.conn.Query(ctx, sql, args...)
	if err != nil {
		return HostsList{}, err
	}
	defer rows.Close()

	out := HostsList{Hosts: []string{}}
	for rows.Next() {
		var host string
		if err := rows.Scan(&host); err != nil {
			return HostsList{}, err
		}
		out.Hosts = append(out.Hosts, host)
	}
	return out, rows.Err()
}

// querySeries reads raw points and buckets them on the fly.
//
// It deliberately reads ninjacat.metrics and not the metrics_1m rollup. At the
// volumes this deployment sees, ClickHouse buckets raw points in milliseconds,
// and one source means one code path and no chance of the two disagreeing.
// The rollup is there for when that stops being true; switching to it is a
// change to this function and nothing else.
func (w *Worker) querySeries(ctx context.Context, req QuerySeries) (SeriesResult, error) {
	if req.Metric == "" {
		return SeriesResult{}, fmt.Errorf("metric is required")
	}

	step := req.Step
	if step < time.Second {
		step = time.Minute
	}

	agg, ok := aggregations[strings.ToLower(req.Aggregation)]
	if !ok {
		agg = "avg"
	}

	groupKeys, err := normalizeGroupBy(req.GroupBy)
	if err != nil {
		return SeriesResult{}, err
	}

	// The grouping expressions go first in the SELECT, so their bound keys go
	// first in the args. Grouping by host stays a plain column; grouping by
	// tags is arrayJoin over the map (see groupColumnsSQL for why).
	groupCols := "host"
	var args []any
	if len(groupKeys) > 0 {
		var groupArgs []any
		groupCols, groupArgs = groupColumnsSQL(groupKeys)
		args = append(args, groupArgs...)
	}
	groupAliases := "host"
	if n := len(groupKeys); n > 0 {
		aliases := make([]string, n)
		for i := range aliases {
			aliases[i] = fmt.Sprintf("g%d", i)
		}
		groupAliases = strings.Join(aliases, ", ")
	}

	// agg is the only interpolated fragment the caller influences and it
	// comes from the map above, so it can only ever be one of five literals.
	// groupCols and groupAliases are built from counters, never from request
	// strings. Everything else the caller controls is bound.
	sql := fmt.Sprintf(`
		SELECT %s,
		       toStartOfInterval(timestamp, INTERVAL ? SECOND) AS bucket,
		       %s(value) AS value
		FROM metrics
		WHERE tenant_id = ? AND metric = ?
		  AND timestamp >= ? AND timestamp < ?`, groupCols, agg)

	args = append(args, int64(step.Seconds()), req.TenantID, req.Metric, req.From, req.To)

	if len(req.Hosts) > 0 {
		sql += ` AND host IN (?)`
		args = append(args, req.Hosts)
	}

	tagSQL, tagArgs, err := tagFilterSQL(req.Tags, true)
	if err != nil {
		return SeriesResult{}, err
	}
	sql += tagSQL
	args = append(args, tagArgs...)

	sql += fmt.Sprintf(` GROUP BY %s, bucket ORDER BY %s, bucket`, groupAliases, groupAliases)

	rows, err := w.conn.Query(ctx, sql, args...)
	if err != nil {
		return SeriesResult{}, err
	}
	defer rows.Close()

	// Rows arrive ordered by the grouping values, so a series is complete the
	// moment they change. No map, no second pass.
	out := SeriesResult{Series: []Series{}, Step: step}
	var current *Series
	currentName := ""

	groupVals := make([]string, max(len(groupKeys), 1))
	dest := make([]any, 0, len(groupVals)+2)
	for i := range groupVals {
		dest = append(dest, &groupVals[i])
	}
	var bucket time.Time
	var value float64
	dest = append(dest, &bucket, &value)

	for rows.Next() {
		if err := rows.Scan(dest...); err != nil {
			return SeriesResult{}, err
		}

		// The name doubles as the change detector. For the host grouping it
		// is the bare host; for tags it is "key:value, key:value" in key
		// order — groupKeys is sorted, so the name is stable per group.
		var name, host string
		var tags map[string]string
		if len(groupKeys) == 0 {
			host = groupVals[0]
			name = host
			tags = map[string]string{"host": host}
		} else {
			parts := make([]string, len(groupKeys))
			tags = make(map[string]string, len(groupKeys))
			for i, k := range groupKeys {
				parts[i] = k + ":" + groupVals[i]
				tags[k] = groupVals[i]
			}
			name = strings.Join(parts, ", ")
		}

		if current == nil || currentName != name {
			out.Series = append(out.Series, Series{
				Metric: req.Metric, Host: host, Name: name, Tags: tags,
				Points: []Point{},
			})
			current = &out.Series[len(out.Series)-1]
			currentName = name
		}
		current.Points = append(current.Points, Point{Timestamp: bucket, Value: value})
	}
	return out, rows.Err()
}

// listTagKeys enumerates the distinct tag keys on a metric's points.
//
// mapKeys reads only the map's key stream, not the values, and the keys are
// LowCardinality — this is a dictionary walk, not a row scan. Measured in the
// multiset decision doc at ~284ms over 800k rows, fine for a dropdown.
func (w *Worker) listTagKeys(ctx context.Context, req ListTagKeys) (TagKeysList, error) {
	sql := `SELECT DISTINCT arrayJoin(mapKeys(tags)) AS k FROM metrics WHERE tenant_id = ?`
	args := []any{req.TenantID}
	if req.Metric != "" {
		sql += ` AND metric = ?`
		args = append(args, req.Metric)
	}
	sql += ` ORDER BY k LIMIT 1000`

	rows, err := w.conn.Query(ctx, sql, args...)
	if err != nil {
		return TagKeysList{}, err
	}
	defer rows.Close()

	out := TagKeysList{Keys: []string{}}
	for rows.Next() {
		var key string
		if err := rows.Scan(&key); err != nil {
			return TagKeysList{}, err
		}
		out.Keys = append(out.Keys, key)
	}
	return out, rows.Err()
}

// listTagValues enumerates the distinct values one tag key holds.
//
// The arrayJoin runs in a subquery so the substring filter applies to each
// value AFTER the fan-out — filtering the array itself would keep or drop
// whole rows, not individual values. The key is bound inside tags[?] like
// any value; see tagFilterSQL for why that matters.
func (w *Worker) listTagValues(ctx context.Context, req ListTagValues) (TagValuesList, error) {
	if req.Key == "" {
		return TagValuesList{}, fmt.Errorf("key is required")
	}
	limit := req.Limit
	if limit <= 0 || limit > 1000 {
		limit = 200
	}

	sql := `SELECT DISTINCT v FROM (
		SELECT arrayJoin(tags[?]) AS v FROM metrics WHERE tenant_id = ?`
	args := []any{req.Key, req.TenantID}
	if req.Metric != "" {
		sql += ` AND metric = ?`
		args = append(args, req.Metric)
	}
	sql += `)`
	if req.Search != "" {
		sql += ` WHERE positionCaseInsensitive(v, ?) > 0`
		args = append(args, req.Search)
	}
	sql += ` ORDER BY v LIMIT ?`
	args = append(args, limit)

	rows, err := w.conn.Query(ctx, sql, args...)
	if err != nil {
		return TagValuesList{}, err
	}
	defer rows.Close()

	out := TagValuesList{Values: []string{}}
	for rows.Next() {
		var value string
		if err := rows.Scan(&value); err != nil {
			return TagValuesList{}, err
		}
		out.Values = append(out.Values, value)
	}
	return out, rows.Err()
}

// logsWhereSQL builds the WHERE clause shared by the search and its count.
//
// tenant_id first, always — same rule as everywhere else, and here it is
// also the first column of the table's ORDER BY, so it prunes granules
// before anything is read.
func logsWhereSQL(req SearchLogs) (string, []any, error) {
	sql := ` WHERE tenant_id = ? AND timestamp >= ? AND timestamp < ?`
	args := []any{req.TenantID, req.From, req.To}

	if req.Service != "" {
		sql += ` AND service = ?`
		args = append(args, req.Service)
	}
	if req.Host != "" {
		sql += ` AND host = ?`
		args = append(args, req.Host)
	}
	if req.Status != "" {
		sql += ` AND status = ?`
		args = append(args, req.Status)
	}

	if q := strings.TrimSpace(req.Query); q != "" {
		// hasToken first, position second — both are needed and the order is
		// the point. hasToken is the predicate the tokenbf_v1 index on
		// message understands, so ClickHouse can discard whole granules
		// before decompressing a single message body. But tokens alone only
		// say the words appear SOMEWHERE in the line, not that they appear
		// together in order, so position() then confirms the exact phrase on
		// the rows that survive the skip.
		for _, tok := range messageTokens(q) {
			sql += ` AND hasToken(message, ?)`
			args = append(args, tok)
		}
		sql += ` AND position(message, ?) > 0`
		args = append(args, q)
	}

	// No materialised tag columns on logs — every tag filter goes through
	// the map.
	tagSQL, tagArgs, err := tagFilterSQL(req.Tags, false)
	if err != nil {
		return "", nil, err
	}
	sql += tagSQL
	args = append(args, tagArgs...)
	return sql, args, nil
}

// searchLogs returns matching lines, newest first, plus the total match
// count.
//
// The count is a second query over the same predicate rather than a window
// trick: it touches only the skip indexes and the filter columns, never the
// message bodies, so it costs little next to fetching the page — and the
// explorer needs it to say "showing 200 of 12,431".
func (w *Worker) searchLogs(ctx context.Context, req SearchLogs) (LogsResult, error) {
	limit := req.Limit
	if limit <= 0 {
		limit = defaultLogLimit
	}
	if limit > maxLogLimit {
		limit = maxLogLimit
	}

	where, args, err := logsWhereSQL(req)
	if err != nil {
		return LogsResult{}, err
	}

	sql := `SELECT timestamp, host, service, source, status, message, tags FROM logs` +
		where + ` ORDER BY timestamp DESC LIMIT ?`
	rows, err := w.conn.Query(ctx, sql, append(append([]any{}, args...), limit)...)
	if err != nil {
		return LogsResult{}, err
	}
	defer rows.Close()

	out := LogsResult{Logs: []LogEntry{}}
	for rows.Next() {
		var e LogEntry
		if err := rows.Scan(&e.Timestamp, &e.Host, &e.Service, &e.Source, &e.Status, &e.Message, &e.Tags); err != nil {
			return LogsResult{}, err
		}
		out.Logs = append(out.Logs, e)
	}
	if err := rows.Err(); err != nil {
		return LogsResult{}, err
	}

	countRow := w.conn.QueryRow(ctx, `SELECT count() FROM logs`+where, args...)
	if err := countRow.Scan(&out.Count); err != nil {
		return LogsResult{}, err
	}
	return out, nil
}

// listLogFacets fills the explorer sidebar: top services, hosts and statuses
// by line count inside the window.
//
// Three small grouped queries rather than one clever one: each groups a
// LowCardinality column, which ClickHouse aggregates over dictionary ids,
// and three result shapes in one query would cost more in ceremony than the
// extra round trips do in time.
func (w *Worker) listLogFacets(ctx context.Context, req ListLogFacets) (LogFacets, error) {
	// The column name is interpolated, so it only ever comes from the fixed
	// calls below — never from the request.
	facet := func(column string) ([]FacetCount, error) {
		sql := fmt.Sprintf(`SELECT %s AS v, count() AS c FROM logs
			WHERE tenant_id = ? AND timestamp >= ? AND timestamp < ?
			GROUP BY v ORDER BY c DESC, v LIMIT %d`, column, facetLimit)
		rows, err := w.conn.Query(ctx, sql, req.TenantID, req.From, req.To)
		if err != nil {
			return nil, err
		}
		defer rows.Close()

		out := []FacetCount{}
		for rows.Next() {
			var f FacetCount
			if err := rows.Scan(&f.Value, &f.Count); err != nil {
				return nil, err
			}
			out = append(out, f)
		}
		return out, rows.Err()
	}

	var out LogFacets
	var err error
	if out.Services, err = facet("service"); err != nil {
		return LogFacets{}, err
	}
	if out.Hosts, err = facet("host"); err != nil {
		return LogFacets{}, err
	}
	if out.Statuses, err = facet("status"); err != nil {
		return LogFacets{}, err
	}
	return out, nil
}

func (w *Worker) HandleMessage(from gen.PID, message any) error { return nil }
func (w *Worker) Terminate(reason error)                        {}
