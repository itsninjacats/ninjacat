/// What the panel asks of the telemetry tables: metric names, tag keys and
/// values, one metric's series, log search and its facets.
///
/// Everything here is pure: a request becomes `Sql` (text and bound values)
/// or, for series, a request to the query engine, and the server runs it. A
/// caller's text is never put into the SQL itself.
module NinjaCat.Api.Engine.Panel

open System
open System.Globalization
open System.Text.RegularExpressions
open NinjaCat.Api.Engine.Api.V2
open NinjaCat.Api.Engine.MetricQuery
open NinjaCat.Api.Engine.MetricQuery.Ast

// --- reading the request ---------------------------------------------------------

/// `tag=env:prod&tag=env:staging&tag=service:api` as one filter: the values
/// of one key are OR'd, different keys are AND'd. Only the first colon
/// splits, so a value keeps its own colons.
let parseTagFilters (raw: string list) : Result<TagFilter, string> =
    let split (tag: string) =
        match tag.IndexOf ':' with
        | i when i > 0 && i < tag.Length - 1 -> Ok(tag.Substring(0, i), tag.Substring(i + 1))
        | _ -> Error $"bad tag \"{tag}\": want key:value"

    let pairs = raw |> List.map split

    match pairs |> List.tryPick (fun p -> match p with Error e -> Some e | Ok _ -> None) with
    | Some e -> Error e
    | None ->
        let pairs = pairs |> List.choose Result.toOption
        // Keys in the order they first appear.
        let keys = pairs |> List.map fst |> List.distinct

        let ofKey (key: string) : TagFilter =
            match pairs |> List.filter (fun (k, _) -> k = key) |> List.map snd with
            | [ value ] -> Tag(key, value)
            | values -> In(key, values)

        match keys with
        | [] -> Ok All
        | [ key ] -> Ok(ofKey key)
        | keys -> Ok(And(List.map ofKey keys))

/// A point in time from a query parameter: empty (the fallback), "now", an
/// offset from now ("-6h"), or RFC 3339. A dashboard wants "the last six
/// hours", not six hours before the day it was saved.
let parseTime (raw: string) (fallback: DateTimeOffset) (now: DateTimeOffset) : Result<DateTimeOffset, string> =
    let raw = raw.Trim()

    if raw = "" then
        Ok fallback
    elif raw = "now" then
        Ok now
    elif raw.StartsWith "-" || raw.StartsWith "+" then
        Duration.parse raw |> Result.map (fun offset -> now + offset)
    else
        match
            DateTimeOffset.TryParseExact(
                raw,
                [| "yyyy-MM-dd'T'HH:mm:ssK"; "yyyy-MM-dd'T'HH:mm:ss.FFFFFFFK" |],
                CultureInfo.InvariantCulture,
                DateTimeStyles.AssumeUniversal
            )
        with
        | true, parsed -> Ok(parsed.ToUniversalTime())
        | false, _ -> Error $"cannot parse \"{raw}\" as a time: want RFC 3339, \"now\" or an offset like -1h"

/// The words of a search text as ClickHouse's hasToken wants them: runs of
/// ASCII letters and digits.
let messageTokens (text: string) : string list =
    Regex.Matches(text, "[0-9a-zA-Z]+") |> Seq.map _.Value |> List.ofSeq

// --- SQL ------------------------------------------------------------------------

let private tenantOf (TenantId tenant) = SqlValue.String tenant

let private boundedLimit (asked: int) (fallback: int) : SqlValue =
    SqlValue.Int64(int64 (if asked <= 0 || asked > 1000 then fallback else asked))

/// Metric names, optionally those containing `search`.
let listMetrics (tenant: TenantId) (search: string) (limit: int) : Sql =
    let bound = SqlParams()
    let where = $"tenant_id = {bound.Add(tenantOf tenant)}"
    let searchFilter = if search = "" then "" else $" AND positionCaseInsensitive(metric, {bound.Add(SqlValue.String search)}) > 0"

    { Text = $"SELECT DISTINCT metric FROM metrics WHERE {where}{searchFilter} ORDER BY metric LIMIT {bound.Add(boundedLimit limit 500)}"
      Parameters = bound.All }

/// Tag keys seen, optionally on one metric.
let listTagKeys (tenant: TenantId) (metric: string) : Sql =
    let bound = SqlParams()
    let where = $"tenant_id = {bound.Add(tenantOf tenant)}"
    let metricFilter = if metric = "" then "" else $" AND metric = {bound.Add(SqlValue.String metric)}"

    { Text = $"SELECT DISTINCT arrayJoin(mapKeys(tags)) AS k FROM metrics WHERE {where}{metricFilter} ORDER BY k LIMIT 1000"
      Parameters = bound.All }

/// Values of one tag key, optionally on one metric and containing `search`.
let listTagValues (tenant: TenantId) (metric: string) (key: string) (search: string) (limit: int) : Sql =
    let bound = SqlParams()
    let values = $"SELECT arrayJoin(tags[{bound.Add(SqlValue.String key)}]) AS v FROM metrics WHERE tenant_id = {bound.Add(tenantOf tenant)}"
    let metricFilter = if metric = "" then "" else $" AND metric = {bound.Add(SqlValue.String metric)}"
    let searchFilter = if search = "" then "" else $" WHERE positionCaseInsensitive(v, {bound.Add(SqlValue.String search)}) > 0"

    { Text = $"SELECT DISTINCT v FROM ({values}{metricFilter}){searchFilter} ORDER BY v LIMIT {bound.Add(boundedLimit limit 200)}"
      Parameters = bound.All }

type SeriesQuery =
    { Metric: string
      Hosts: string list
      From: DateTimeOffset
      To: DateTimeOffset
      /// The bucket width asked for; None leaves it to the engine.
      Step: TimeSpan option
      /// avg, sum, min or max; "" is avg.
      Aggregation: string
      Tags: TagFilter
      /// Tag keys to split series by; empty splits by host.
      GroupBy: string list }

/// One metric over a time range as the query engine takes it: the same
/// request `avg:metric{tags} by {keys}` would parse into. So the panel's
/// series are aggregated like every other query — over time within a series
/// first, then across the series of a group.
let seriesRequest (q: SeriesQuery) : Timeseries.ParsedTimeseriesRequest =
    let filter =
        match q.Hosts, q.Tags with
        | [], tags -> tags
        | hosts, All -> In("host", hosts)
        | hosts, tags -> And [ In("host", hosts); tags ]

    let query: MetricQuery =
        { SpaceAgg = Some(if q.Aggregation = "" then "avg" else q.Aggregation.ToLowerInvariant())
          Metric = q.Metric
          Filter = filter
          GroupBy = (if q.GroupBy.IsEmpty then [ "host" ] else q.GroupBy |> List.distinct |> List.sort)
          Modifiers = [] }

    { From = q.From
      To = q.To
      Interval = q.Step
      Queries = [ { Name = Some "q"; Query = query } ]
      Formulas = [] }

/// One series as the panel shows it.
type Series =
    { /// The host, or `key:value, key:value` of its group.
      Name: string
      /// Set when the query was not grouped: then a series is a host.
      Host: string option
      Tags: Map<string, string>
      Points: (DateTimeOffset * float) list }

/// The engine's answer to `seriesRequest q` as the panel's series. A series
/// without one of the group's tags has Datadog's "N/A" for it.
let seriesOf (q: SeriesQuery) (results: Query.Engine.FormulaResult list) : Series list =
    [ for result in results do
          for line in result.Series ->
              let pairs = line.Tags |> List.map (fun tag -> tag.Key, defaultArg tag.Value Query.Series.notApplicable)

              let name, host =
                  match q.GroupBy, pairs with
                  | [], [ _, host ] -> host, Some host
                  | _ -> pairs |> List.map (fun (key, value) -> $"{key}:{value}") |> String.concat ", ", None

              { Name = name
                Host = host
                Tags = Map pairs
                Points = [ for point in line.Points -> point.Time, point.Value ] } ]

type LogSearch =
    { Tenant: TenantId
      /// Free text looked for in the message.
      Text: string
      Service: string
      Host: string
      Status: string
      Tags: TagFilter
      From: DateTimeOffset
      To: DateTimeOffset
      Limit: int }

let private logsWhere (bound: SqlParams) (q: LogSearch) : string =
    let scope =
        $" WHERE tenant_id = {bound.Add(tenantOf q.Tenant)}"
        + $" AND timestamp >= fromUnixTimestamp64Milli({bound.Add(SqlValue.Int64(q.From.ToUnixTimeMilliseconds()))})"
        + $" AND timestamp < fromUnixTimestamp64Milli({bound.Add(SqlValue.Int64(q.To.ToUnixTimeMilliseconds()))})"

    let equals (column: string) (value: string) =
        if value = "" then "" else $" AND {column} = {bound.Add(SqlValue.String value)}"

    let facets = equals "service" q.Service + equals "host" q.Host + equals "status" q.Status

    // hasToken lets the token index skip granules; position then confirms
    // the whole phrase, which the index cannot.
    let text =
        match q.Text.Trim() with
        | "" -> ""
        | phrase ->
            let tokens = messageTokens phrase |> List.map (fun token -> $" AND hasToken(message, {bound.Add(SqlValue.String token)})") |> String.concat ""
            tokens + $" AND position(message, {bound.Add(SqlValue.String phrase)}) > 0"

    let tags = if q.Tags = All then "" else $" AND ({Filter.sql bound q.Tags})"
    scope + facets + text + tags

/// Two queries: the newest matching logs, and how many match in all.
let searchLogs (q: LogSearch) : Sql * Sql =
    let limit =
        if q.Limit <= 0 then 200L
        elif q.Limit > 1000 then 1000L
        else int64 q.Limit

    let rowsBound = SqlParams()
    let rowsWhere = logsWhere rowsBound q
    let countBound = SqlParams()
    let countWhere = logsWhere countBound q

    { Text =
        "SELECT toUnixTimestamp64Milli(timestamp), host, service, source, status, message, tags FROM logs"
        + rowsWhere
        + $" ORDER BY timestamp DESC LIMIT {rowsBound.Add(SqlValue.Int64 limit)}"
      Parameters = rowsBound.All },
    { Text = "SELECT count() FROM logs" + countWhere
      Parameters = countBound.All }

type LogFacet =
    | ByService
    | ByHost
    | ByStatus

/// The fifty most common values of one facet in a time range, with counts.
let logFacet (tenant: TenantId) (from: DateTimeOffset) (until: DateTimeOffset) (facet: LogFacet) : Sql =
    let bound = SqlParams()

    let column =
        match facet with
        | ByService -> "service"
        | ByHost -> "host"
        | ByStatus -> "status"

    { Text =
        $"SELECT {column} AS v, count() AS c FROM logs WHERE tenant_id = {bound.Add(tenantOf tenant)}"
        + $" AND timestamp >= fromUnixTimestamp64Milli({bound.Add(SqlValue.Int64(from.ToUnixTimeMilliseconds()))})"
        + $" AND timestamp < fromUnixTimestamp64Milli({bound.Add(SqlValue.Int64(until.ToUnixTimeMilliseconds()))})"
        + " GROUP BY v ORDER BY c DESC, v LIMIT 50"
      Parameters = bound.All }
