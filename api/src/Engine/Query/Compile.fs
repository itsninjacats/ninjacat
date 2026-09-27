/// Compiles one QueryPlan into SQL against the `metrics` table.
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
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Query.Plan

/// The columns a compiled query returns, in this order:
///
///   g0 … gN-1   Nullable(String)   one per `by` key, in the order of GroupBy;
///                                  NULL where the series has no such tag
///   bucket_ms   Int64    bucket start, unix milliseconds
///   value       Float64
type Row =
    { /// None: the series has no value for that `by` key — Datadog's N/A group.
      Groups: string option list
      BucketMs: int64
      Value: float }

/// Hands out parameter names, so each value gets its own placeholder.
type private Params() =
    let values = ResizeArray<string * SqlValue>()

    member _.Add(prefix: string, value: SqlValue) =
        let name = $"{prefix}{values.Count}"
        values.Add(name, value)
        Sql.param name value

    member _.All = List.ofSeq values

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

/// Datadog's `*` wildcard as a LIKE pattern. `_` and `%` are LIKE's own
/// wildcards and common in tag values (`kube_namespace`), so they are escaped.
let private likePattern (value: string) =
    value.Replace("\\", "\\\\").Replace("%", "\\%").Replace("_", "\\_").Replace("*", "%")

let private isWildcard (value: string) = value.Contains '*'

/// `host` lives in its own column, not in the tag map — the agent sends it
/// apart from the tags, and the table's ORDER BY uses it.
let rec private filterSql (p: Params) (filter: TagFilter) : string =
    match filter with
    | All -> "1"
    | Tag("host", v) when isWildcard v -> $"""host LIKE {p.Add("p", String(likePattern v))}"""
    | Tag("host", v) -> $"""host = {p.Add("p", String v)}"""
    | Tag(k, v) when isWildcard v ->
        $"""arrayExists(x -> x LIKE {p.Add("p", String(likePattern v))}, tags[{p.Add("k", String k)}])"""
    | Tag(k, v) -> $"""has(tags[{p.Add("k", String k)}], {p.Add("p", String v)})"""
    // A key-less tag is stored as a key with an empty value (intake/server.go
    // splitTag), so `{prod}` asks whether the key exists.
    | Bare v when isWildcard v -> $"""arrayExists(x -> x LIKE {p.Add("p", String(likePattern v))}, mapKeys(tags))"""
    | Bare v -> $"""mapContains(tags, {p.Add("k", String v)})"""
    // `IN` with a wildcard among its values (`name IN (web-*, db)`) is the OR
    // of its parts.
    //
    // WARNING(undocumented): wildcards inside IN. Datadog's tag rules forbid
    // `*` in a tag value, so a `*` in a filter can only ever mean one.
    | In(k, vs) when List.exists isWildcard vs ->
        let exact = vs |> List.filter (isWildcard >> not)
        let parts = [ for v in vs |> List.filter isWildcard -> Tag(k, v) ] @ (if exact.IsEmpty then [] else [ In(k, exact) ])
        filterSql p (Or parts)
    | In("host", vs) -> $"""has({p.Add("p", StringArray vs)}, host)"""
    | In(k, vs) -> $"""hasAny(tags[{p.Add("k", String k)}], {p.Add("p", StringArray vs)})"""
    | Not f -> $"NOT ({filterSql p f})"
    | And fs -> fs |> List.map (fun f -> $"({filterSql p f})") |> String.concat " AND "
    | Or fs -> fs |> List.map (fun f -> $"({filterSql p f})") |> String.concat " OR "

/// One expression per `by` key. A tag holds a list of values, and a series
/// with `role:api` and `role:web` belongs to both groups — arrayJoin gives it a
/// row in each, as the Go query worker already does.
///
/// A series without the tag is not dropped: Datadog shows it in an N/A group
/// (that is what exclude_null() exists to remove — docs:
/// dashboards/functions/exclusion). An empty list would make arrayJoin yield
/// no row at all, so it becomes [NULL] first.
let private groupSql (p: Params) (key: string) =
    match key with
    // WARNING(undocumented): a series with an empty host. It joins the N/A
    // group, as a missing tag does.
    | "host" -> "nullIf(toString(host), '')"
    | k ->
        let k = p.Add("k", String k)
        $"arrayJoin(if(empty(tags[{k}]), [NULL], CAST(tags[{k}], 'Array(Nullable(String))')))"

/// How far before `from` a query reads, in milliseconds.
let earliestOffsetMs (query: QueryPlan) =
    if query.Lookback = 0 then 0L
    else int64 (query.Lookback + 1) * int64 query.Step.TotalMilliseconds

/// The first bucket a query's series keep: `from`, less its lookback.
let keepFromMs (plan: Plan) (query: QueryPlan) =
    plan.From.ToUnixTimeMilliseconds() - int64 query.Lookback * int64 query.Step.TotalMilliseconds

let compile (tenant: TenantId) (plan: Plan) (query: QueryPlan) : Sql =
    let p = Params()
    let (TenantId t) = tenant

    let groups = query.GroupBy |> List.mapi (fun i k -> $"g{i}", groupSql p k)
    let aliases = groups |> List.map fst
    let groupSelect = groups |> List.map (fun (alias, expr) -> $"{expr} AS {alias}, ") |> String.concat ""
    let groupList = aliases |> List.map (fun a -> $"{a}, ") |> String.concat ""

    let tenantP = p.Add("tenant", String t)
    let metricP = p.Add("metric", String query.Metric)
    // With lookback, one bucket more than asked: the bucket holding `from`
    // minus the lookback may start before it, and would be partial.
    let fromMs = plan.From.ToUnixTimeMilliseconds() - earliestOffsetMs query
    let fromP = p.Add("from", Int64 fromMs)
    let toP = p.Add("to", Int64(plan.To.ToUnixTimeMilliseconds()))
    let stepP = p.Add("step", Int64(int64 query.Step.TotalSeconds))
    let filter = filterSql p query.Filter

    let text =
        $"""SELECT {groupList}toInt64(toUnixTimestamp(bucket)) * 1000 AS bucket_ms, {aggregate query.SpaceAgg}(v) AS value
FROM (
    SELECT series_id, {groupSelect}toStartOfInterval(timestamp, toIntervalSecond({stepP})) AS bucket, {timeAggregateSql stepP query.TimeAgg} AS v
    FROM metrics
    WHERE tenant_id = {tenantP}
      AND metric = {metricP}
      AND timestamp >= fromUnixTimestamp64Milli({fromP})
      AND timestamp < fromUnixTimestamp64Milli({toP})
      AND ({filter})
    GROUP BY series_id, {groupList}bucket
)
GROUP BY {groupList}bucket
ORDER BY {groupList}bucket"""

    { Text = text; Parameters = p.All }
