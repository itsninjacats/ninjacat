package query

import (
	"context"
	"fmt"
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

	// agg is the only interpolated fragment and it comes from the map above,
	// so it can only ever be one of five literals. Everything the caller
	// controls is bound.
	sql := fmt.Sprintf(`
		SELECT host,
		       toStartOfInterval(timestamp, INTERVAL ? SECOND) AS bucket,
		       %s(value) AS value
		FROM metrics
		WHERE tenant_id = ? AND metric = ?
		  AND timestamp >= ? AND timestamp < ?`, agg)

	args := []any{int64(step.Seconds()), req.TenantID, req.Metric, req.From, req.To}

	if len(req.Hosts) > 0 {
		sql += ` AND host IN (?)`
		args = append(args, req.Hosts)
	}
	sql += ` GROUP BY host, bucket ORDER BY host, bucket`

	rows, err := w.conn.Query(ctx, sql, args...)
	if err != nil {
		return SeriesResult{}, err
	}
	defer rows.Close()

	// Rows arrive ordered by host, so a series is complete the moment the
	// host changes. No map, no second pass.
	out := SeriesResult{Series: []Series{}, Step: step}
	var current *Series

	for rows.Next() {
		var host string
		var bucket time.Time
		var value float64
		if err := rows.Scan(&host, &bucket, &value); err != nil {
			return SeriesResult{}, err
		}
		if current == nil || current.Host != host {
			out.Series = append(out.Series, Series{
				Metric: req.Metric, Host: host, Points: []Point{},
			})
			current = &out.Series[len(out.Series)-1]
		}
		current.Points = append(current.Points, Point{Timestamp: bucket, Value: value})
	}
	return out, rows.Err()
}

func (w *Worker) HandleMessage(from gen.PID, message any) error { return nil }
func (w *Worker) Terminate(reason error)                        {}
