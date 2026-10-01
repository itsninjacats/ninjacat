/// What the panel asks of the telemetry tables: metric names, tag keys and
/// values, a plain series query, log search and its facets.
///
/// Everything here is pure: a request becomes `Sql` (text and bound values),
/// and the server runs it. A caller's text is never put into the SQL itself.
module NinjaCat.Api.Engine.Panel

open System
open System.Globalization
open System.Text.RegularExpressions

/// One tag key and the values it may have: values of one key are OR'd,
/// different keys are AND'd.
type TagFilter = { Key: string; Values: string list }

/// At most this many points per series, whatever step was asked for.
let maxPoints = 2000

// --- reading the request ---------------------------------------------------------

/// `tag=env:prod&tag=env:staging&tag=service:api` as filters, one per key.
/// Only the first colon splits, so a value keeps its own colons.
let parseTagFilters (raw: string list) : Result<TagFilter list, string> =
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

        Ok
            [ for key in keys ->
                  { Key = key
                    Values = pairs |> List.filter (fun (k, _) -> k = key) |> List.map snd } ]

let private durationUnits: Map<string, float> =
    Map
        [ "ns", 1e-9
          "us", 1e-6
          "µs", 1e-6
          "ms", 1e-3
          "s", 1.0
          "m", 60.0
          "h", 3600.0 ]

/// A duration as Go's time.ParseDuration reads it: a sign, then one or more
/// number-and-unit pairs ("-1h30m", "1.5h", "300ms").
let parseDuration (text: string) : Result<TimeSpan, string> =
    let m = Regex.Match(text, @"^([+-])?((?:\d+(?:\.\d*)?|\.\d+)(?:ns|us|µs|ms|s|m|h))+$")

    if not m.Success then
        Error $"invalid duration \"{text}\""
    else
        let seconds =
            Regex.Matches(text, @"(\d+(?:\.\d*)?|\.\d+)(ns|us|µs|ms|s|m|h)")
            |> Seq.sumBy (fun part -> Double.Parse(part.Groups[1].Value, CultureInfo.InvariantCulture) * durationUnits[part.Groups[2].Value])

        Ok(TimeSpan.FromSeconds(if m.Groups[1].Value = "-" then -seconds else seconds))

/// An offset from now: a Go duration, or days and weeks ("-7d", "-2w"),
/// which Go's durations stop short of.
let parseOffset (raw: string) : Result<TimeSpan, string> =
    let scaled (suffix: string) (unit: TimeSpan) =
        match Double.TryParse(raw.Substring(0, raw.Length - suffix.Length), NumberStyles.Float, CultureInfo.InvariantCulture) with
        | true, n -> Ok(unit * n)
        | false, _ -> Error $"bad offset \"{raw}\""

    if raw.EndsWith "w" then scaled "w" (TimeSpan.FromDays 7.0)
    elif raw.EndsWith "d" then scaled "d" (TimeSpan.FromDays 1.0)
    else parseDuration raw

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
        parseOffset raw |> Result.map (fun offset -> now + offset)
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

/// The bucket width: the one asked for, or about 300 points across the span;
/// widened to stay under maxPoints, never under a second, in whole seconds.
let resolveStep (raw: string) (span: TimeSpan) : TimeSpan =
    let asked =
        match parseDuration raw with
        | Ok d when d > TimeSpan.Zero -> d
        | _ -> span / 300.0

    let floor = span / float maxPoints
    let step = max (max asked floor) (TimeSpan.FromSeconds 1.0)
    TimeSpan.FromSeconds(Math.Round(step.TotalSeconds, MidpointRounding.AwayFromZero))

/// At most this many group-by keys: each multiplies the number of series.
let maxGroupByKeys = 4

/// Group-by keys without duplicates, sorted so a series is named the same
/// whatever order the keys were given in.
let normalizeGroupBy (keys: string list) : Result<string list, string> =
    if keys |> List.contains "" then
        Error "group-by key must not be empty"
    else
        let distinct = List.distinct keys

        if distinct.Length > maxGroupByKeys then
            Error $"too many group-by keys: {distinct.Length} given, at most {maxGroupByKeys} allowed"
        else
            Ok(List.sort distinct)

/// The words of a search text as ClickHouse's hasToken wants them: runs of
/// ASCII letters and digits.
let messageTokens (text: string) : string list =
    Regex.Matches(text, "[0-9a-zA-Z]+") |> Seq.map _.Value |> List.ofSeq

// --- SQL ------------------------------------------------------------------------

/// Collects bound values and hands out their placeholders.
type private Bound() =
    let values = ResizeArray<string * SqlValue>()

    member _.Add(value: SqlValue) : string =
        let name = $"p{values.Count}"
        values.Add((name, value))
        Sql.param name value

    member _.Parameters = List.ofSeq values

/// Tags with a materialised column of their own in `metrics`.
let private hotTagColumns = set [ "env"; "service"; "kube_namespace"; "kube_deployment"; "pod_name" ]

/// ` AND …` for each tag filter. A tag is a list of values, so a filter is a
/// membership test, not an equality. `useHotColumns` is for `metrics`, which
/// has columns for the common keys; `logs` does not.
let private tagFilterSql (bound: Bound) (filters: TagFilter list) (useHotColumns: bool) : Result<string, string> =
    if filters |> List.exists (fun f -> f.Key = "" || f.Values.IsEmpty) then
        Error "tag filter needs a key and at least one value"
    else
        filters
        |> List.map (fun f ->
            let hot = useHotColumns && hotTagColumns.Contains f.Key

            match hot, f.Values with
            // The key is one of five fixed names here, never the caller's text.
            | true, [ value ] -> $" AND {f.Key} = {bound.Add(SqlValue.String value)}"
            | true, values -> $" AND {f.Key} IN {bound.Add(SqlValue.StringArray values)}"
            | false, [ value ] -> $" AND has(tags[{bound.Add(SqlValue.String f.Key)}], {bound.Add(SqlValue.String value)})"
            | false, values -> $" AND hasAny(tags[{bound.Add(SqlValue.String f.Key)}], {bound.Add(SqlValue.StringArray values)})")
        |> String.concat ""
        |> Ok

let private tenantOf (TenantId tenant) = SqlValue.String tenant

let private boundedLimit (asked: int) (fallback: int) : SqlValue =
    SqlValue.Int64(int64 (if asked <= 0 || asked > 1000 then fallback else asked))

/// Metric names, optionally those containing `search`.
let listMetrics (tenant: TenantId) (search: string) (limit: int) : Sql =
    let bound = Bound()
    let where = $"tenant_id = {bound.Add(tenantOf tenant)}"
    let searchFilter = if search = "" then "" else $" AND positionCaseInsensitive(metric, {bound.Add(SqlValue.String search)}) > 0"

    { Text = $"SELECT DISTINCT metric FROM metrics WHERE {where}{searchFilter} ORDER BY metric LIMIT {bound.Add(boundedLimit limit 500)}"
      Parameters = bound.Parameters }

/// Hosts that reported, optionally for one metric.
let listHosts (tenant: TenantId) (metric: string) : Sql =
    let bound = Bound()
    let where = $"tenant_id = {bound.Add(tenantOf tenant)}"
    let metricFilter = if metric = "" then "" else $" AND metric = {bound.Add(SqlValue.String metric)}"

    { Text = $"SELECT DISTINCT host FROM metrics WHERE {where}{metricFilter} ORDER BY host LIMIT 1000"
      Parameters = bound.Parameters }

/// Tag keys seen, optionally on one metric.
let listTagKeys (tenant: TenantId) (metric: string) : Sql =
    let bound = Bound()
    let where = $"tenant_id = {bound.Add(tenantOf tenant)}"
    let metricFilter = if metric = "" then "" else $" AND metric = {bound.Add(SqlValue.String metric)}"

    { Text = $"SELECT DISTINCT arrayJoin(mapKeys(tags)) AS k FROM metrics WHERE {where}{metricFilter} ORDER BY k LIMIT 1000"
      Parameters = bound.Parameters }

/// Values of one tag key, optionally on one metric and containing `search`.
let listTagValues (tenant: TenantId) (metric: string) (key: string) (search: string) (limit: int) : Sql =
    let bound = Bound()
    let values = $"SELECT arrayJoin(tags[{bound.Add(SqlValue.String key)}]) AS v FROM metrics WHERE tenant_id = {bound.Add(tenantOf tenant)}"
    let metricFilter = if metric = "" then "" else $" AND metric = {bound.Add(SqlValue.String metric)}"
    let searchFilter = if search = "" then "" else $" WHERE positionCaseInsensitive(v, {bound.Add(SqlValue.String search)}) > 0"

    { Text = $"SELECT DISTINCT v FROM ({values}{metricFilter}){searchFilter} ORDER BY v LIMIT {bound.Add(boundedLimit limit 200)}"
      Parameters = bound.Parameters }

type SeriesQuery =
    { Tenant: TenantId
      Metric: string
      Hosts: string list
      From: DateTimeOffset
      To: DateTimeOffset
      Step: TimeSpan
      /// avg, min, max, sum or count; anything else is avg.
      Aggregation: string
      Tags: TagFilter list
      /// Tag keys to split series by; empty splits by host.
      GroupBy: string list }

/// The series query and its group-by keys as normalised. Each row is the
/// group values (one per key, or the host), the bucket in Unix seconds, and
/// the value; rows come ordered by group, then bucket.
let querySeries (q: SeriesQuery) : Result<Sql * string list, string> =
    if q.Metric = "" then
        Error "metric is required"
    else
        match normalizeGroupBy q.GroupBy with
        | Error e -> Error e
        | Ok groupKeys ->
            let bound = Bound()

            let aggregation =
                match q.Aggregation.ToLowerInvariant() with
                | "min"
                | "max"
                | "sum"
                | "count" as known -> known
                | _ -> "avg"

            let step = if q.Step < TimeSpan.FromSeconds 1.0 then TimeSpan.FromMinutes 1.0 else q.Step

            // A series belongs to every value its tag has: arrayJoin gives a
            // row per value.
            let groupColumns, groupAliases =
                if groupKeys.IsEmpty then
                    "host", "host"
                else
                    groupKeys |> List.mapi (fun i key -> $"arrayJoin(tags[{bound.Add(SqlValue.String key)}]) AS g{i}") |> String.concat ", ",
                    groupKeys |> List.mapi (fun i _ -> $"g{i}") |> String.concat ", "

            let select =
                $"SELECT {groupColumns}, toInt64(toUnixTimestamp(toStartOfInterval(timestamp, toIntervalSecond({bound.Add(SqlValue.Int64(int64 step.TotalSeconds))})))) AS bucket, {aggregation}(value) AS value"

            let where =
                $" FROM metrics WHERE tenant_id = {bound.Add(tenantOf q.Tenant)} AND metric = {bound.Add(SqlValue.String q.Metric)}"
                + $" AND timestamp >= fromUnixTimestamp64Milli({bound.Add(SqlValue.Int64(q.From.ToUnixTimeMilliseconds()))})"
                + $" AND timestamp < fromUnixTimestamp64Milli({bound.Add(SqlValue.Int64(q.To.ToUnixTimeMilliseconds()))})"

            let hostFilter = if q.Hosts.IsEmpty then "" else $" AND host IN {bound.Add(SqlValue.StringArray q.Hosts)}"

            match tagFilterSql bound q.Tags true with
            | Error e -> Error e
            | Ok tagFilter ->
                Ok(
                    { Text = $"{select}{where}{hostFilter}{tagFilter} GROUP BY {groupAliases}, bucket ORDER BY {groupAliases}, bucket"
                      Parameters = bound.Parameters },
                    groupKeys
                )

type LogSearch =
    { Tenant: TenantId
      /// Free text looked for in the message.
      Text: string
      Service: string
      Host: string
      Status: string
      Tags: TagFilter list
      From: DateTimeOffset
      To: DateTimeOffset
      Limit: int }

let private logsWhere (bound: Bound) (q: LogSearch) : Result<string, string> =
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

    tagFilterSql bound q.Tags false |> Result.map (fun tags -> scope + facets + text + tags)

/// Two queries: the newest matching logs, and how many match in all.
let searchLogs (q: LogSearch) : Result<Sql * Sql, string> =
    let limit =
        if q.Limit <= 0 then 200L
        elif q.Limit > 1000 then 1000L
        else int64 q.Limit

    let rowsBound = Bound()
    let countBound = Bound()

    match logsWhere rowsBound q, logsWhere countBound q with
    | Ok rowsWhere, Ok countWhere ->
        Ok(
            { Text =
                "SELECT toUnixTimestamp64Milli(timestamp), host, service, source, status, message, tags FROM logs"
                + rowsWhere
                + $" ORDER BY timestamp DESC LIMIT {rowsBound.Add(SqlValue.Int64 limit)}"
              Parameters = rowsBound.Parameters },
            { Text = "SELECT count() FROM logs" + countWhere
              Parameters = countBound.Parameters }
        )
    | Error e, _
    | _, Error e -> Error e

type LogFacet =
    | ByService
    | ByHost
    | ByStatus

/// The fifty most common values of one facet in a time range, with counts.
let logFacet (tenant: TenantId) (from: DateTimeOffset) (until: DateTimeOffset) (facet: LogFacet) : Sql =
    let bound = Bound()

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
      Parameters = bound.Parameters }
