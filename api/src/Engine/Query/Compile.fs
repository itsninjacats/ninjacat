/// Compiles one QueryPlan into SQL against `metrics`, or against its
/// one-minute rollup `metrics_1m` when the step is whole minutes.
///
/// The shape follows Datadog's order of operations — time first, then space
/// (docs: dashboards/guide/query-to-the-graph):
///
///   inner   one row per (series, bucket): the series' own values in that
///           bucket, combined with the time aggregator
///   outer   one row per (group, bucket): the series in that group combined
///           with the space aggregator
///
/// Doing it in one pass would average raw points across hosts, which weighs a
/// host that reports twice as often twice as much — not what Datadog shows.
///
/// Every value that came from the request is a bound parameter; the SQL text
/// holds only identifiers this file chose.
module NinjaCat.Api.Engine.Query.Compile

open System
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.MetricQuery
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Query.Plan

/// The columns a compiled query returns, in this order:
///
///   g0 … gN-1   Nullable(String)   one per `by` key, in the order of GroupBy;
///                                  NULL where the series has no such tag
///   bucket_ms   Int64              bucket start, unix milliseconds
///   value       Float64
///   series_id   UInt64             only when the plan fills gaps: then each
///                                  row is one series' bucket, uncombined
///   bucket_ms   Int64    bucket start, unix milliseconds
///   value       Float64
type Row =
    { /// None: the series has no value for that `by` key — Datadog's N/A group.
      Groups: string option list
      /// Set when the query returns series one by one (see `compile`), for F#
      /// to fill and combine them; None when ClickHouse already combined them.
      SeriesId: uint64 option
      BucketMs: int64
      Value: float }

let private aggregate =
    function
    | Avg -> "avg"
    | Sum -> "sum"
    | Min -> "min"
    | Max -> "max"
    | Count -> "count"

/// The inner, per-series aggregate. `step` is the bucket width placeholder.
///
/// One series has one metric type, so `any(metric_type)` over its rows in a
/// bucket is that type — the formula can follow it without a separate lookup.
/// UNSPECIFIED and anything unknown are treated as GAUGE (silnik-kwerend.md).
///
/// A RATE point is events per second over the metric's `interval`, so its
/// event count is value × interval.
///
/// WARNING(undocumented): a RATE row with interval 0. Such rows exist, and
/// would count as no events at all; the interval is floored at 1 second.
let private timeAggregateSql (step: string) =
    function
    | Plain Count -> "toFloat64(count())"
    | Plain agg -> $"{aggregate agg}(value)"
    | AsCount ->
        "multiIf(any(metric_type) = 'RATE', sum(value * greatest(interval, 1)), "
        + "any(metric_type) = 'COUNT', sum(value), avg(value))"
    | AsRate ->
        // For RATE, the bucket is floored to the metric's interval: a 5 s
        // bucket over a metric sent every 10 s would otherwise double it.
        $"multiIf(any(metric_type) = 'RATE', sum(value * greatest(interval, 1)) / greatest({step}, max(interval)), "
        + $"any(metric_type) = 'COUNT', sum(value) / {step}, avg(value))"

/// The same over the rollup, whose per-minute states merge into the bucket:
/// an average is the merged sums over the merged counts, never an average of
/// averages. `metric_type` and `interval` are kept per minute as `any` and
/// `max`, so the per-series reads above give the same answer here.
let private rollupAggregateSql (step: string) =
    function
    | Plain Count -> "toFloat64(countMerge(points))"
    | Plain agg -> $"{aggregate agg}Merge(value_{aggregate agg})"
    | AsCount ->
        "multiIf(any(metric_type) = 'RATE', sumMerge(value_events), "
        + "any(metric_type) = 'COUNT', sumMerge(value_sum), avgMerge(value_avg))"
    | AsRate ->
        $"multiIf(any(metric_type) = 'RATE', sumMerge(value_events) / greatest({step}, max(interval)), "
        + $"any(metric_type) = 'COUNT', sumMerge(value_sum) / {step}, avgMerge(value_avg))"

/// One expression per `by` key. A tag holds a list of values, and a series
/// with `role:api` and `role:web` belongs to both groups — arrayJoin gives it a
/// row in each.
///
/// A series without the tag is not dropped: Datadog shows it in an N/A group
/// (that is what exclude_null() exists to remove — docs:
/// dashboards/functions/exclusion). An empty list would make arrayJoin yield
/// no row at all, so it becomes [NULL] first.
let private groupSql (p: SqlParams) (key: string) =
    match key with
    // WARNING(undocumented): a series with an empty host. It joins the N/A
    // group, as a missing tag does.
    | "host" -> "nullIf(toString(host), '')"
    | k ->
        let k = p.Add("k", String k)
        $"arrayJoin(if(empty(tags[{k}]), [NULL], CAST(tags[{k}], 'Array(Nullable(String))')))"

/// The first bucket a query's series keep: `from`, less its lookback.
let keepFromMs (plan: Plan) (query: QueryPlan) =
    plan.From.ToUnixTimeMilliseconds() - int64 query.Lookback * int64 query.Step.TotalMilliseconds

/// `compile` with the table given rather than chosen by the step.
let compileOn (table: Table) (tenant: TenantId) (plan: Plan) (query: QueryPlan) : Sql =
    let p = SqlParams()
    let (TenantId t) = tenant

    let groups = query.GroupBy |> List.mapi (fun i k -> $"g{i}", groupSql p k)
    let aliases = groups |> List.map fst
    let groupSelect = groups |> List.map (fun (alias, expr) -> $"{expr} AS {alias}, ") |> String.concat ""
    let groupList = aliases |> List.map (fun a -> $"{a}, ") |> String.concat ""

    let tenantP = p.Add("tenant", String t)
    let metricP = p.Add("metric", String query.Metric)
    let fromMs = plan.From.ToUnixTimeMilliseconds() - earliestOffsetMs query
    let fromP = p.Add("from", Int64 fromMs)
    let toP = p.Add("to", Int64(plan.To.ToUnixTimeMilliseconds()))
    let stepP = p.Add("step", Int64(int64 query.Step.TotalSeconds))
    let filter = Filter.sql p query.Filter

    let raw =
        $"""metrics
    WHERE tenant_id = {tenantP}
      AND metric = {metricP}
      AND timestamp >= fromUnixTimestamp64Milli({fromP})
      AND timestamp < fromUnixTimestamp64Milli({toP})
      AND ({filter})"""

    // A rollup row is a whole minute, so it holds points after an unaligned
    // `to`: the minutes before the one holding `to` come from the rollup, and
    // that minute's points before `to` from the raw table, as the same
    // states. Past the raw retention that last part minute is gone. The
    // bucket holding an unaligned `from` is partial either way, and the
    // series drop it (keepFromMs).
    let rollup =
        $"""(
        SELECT series_id, host, tags, minute, metric_type, interval,
               points, value_avg, value_min, value_max, value_sum, value_events
        FROM metrics_1m
        WHERE tenant_id = {tenantP}
          AND metric = {metricP}
          AND minute >= fromUnixTimestamp64Milli({fromP})
          AND minute < toStartOfMinute(fromUnixTimestamp64Milli({toP}))
          AND ({filter})
        UNION ALL
        SELECT series_id, host, any(tags), toStartOfMinute(timestamp) AS tail_minute, any(metric_type), max(interval),
               countState(value), avgState(value), minState(value), maxState(value), sumState(value),
               sumState(value * greatest(interval, 1))
        FROM metrics
        WHERE tenant_id = {tenantP}
          AND metric = {metricP}
          AND timestamp >= greatest(fromUnixTimestamp64Milli({fromP}), toStartOfMinute(fromUnixTimestamp64Milli({toP})))
          AND timestamp < fromUnixTimestamp64Milli({toP})
          AND ({filter})
        GROUP BY series_id, host, tail_minute
    )"""

    let source, time, timeAgg =
        match table with
        | PointsTable -> raw, "timestamp", timeAggregateSql stepP query.TimeAgg
        | RollupTable -> rollup, "minute", rollupAggregateSql stepP query.TimeAgg

    let perSeries = query.Fill <> NoFill

    // Filling happens per series, before series are combined — so with a fill
    // ClickHouse stops at the inner query, and Series.combine does the rest.
    let text =
        if perSeries then
            $"""SELECT {groupList}toInt64(toUnixTimestamp(bucket)) * 1000 AS bucket_ms, v AS value, series_id
FROM (
    SELECT series_id, {groupSelect}toStartOfInterval({time}, toIntervalSecond({stepP})) AS bucket, {timeAgg} AS v
    FROM {source}
    GROUP BY series_id, {groupList}bucket
)
ORDER BY {groupList}series_id, bucket"""
        else
        $"""SELECT {groupList}toInt64(toUnixTimestamp(bucket)) * 1000 AS bucket_ms, {aggregate query.SpaceAgg}(v) AS value
FROM (
    SELECT series_id, {groupSelect}toStartOfInterval({time}, toIntervalSecond({stepP})) AS bucket, {timeAgg} AS v
    FROM {source}
    GROUP BY series_id, {groupList}bucket
)
GROUP BY {groupList}bucket
ORDER BY {groupList}bucket"""

    { Text = text; Parameters = p.All }

let compile (tenant: TenantId) (plan: Plan) (query: QueryPlan) : Sql =
    compileOn (tableFor query.Step) tenant plan query
