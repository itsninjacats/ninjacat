/// A service check monitor, `"postgres.can_connect".over("env:prod").by("host").last(3).count_by_status()`:
/// read, and evaluated from `check_runs`.
module NinjaCat.Api.Engine.Monitor.Check

open System
open System.Threading.Tasks
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.MetricQuery
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor.Definition
open NinjaCat.Api.Engine.Monitor.Evaluate

type private Arg =
    | Item of string
    | Count of float

type private Call = { Name: string; Args: Arg list }

module private Grammar =
    open FParsec

    type private P<'T> = Parser<'T, unit>

    let private ws: P<unit> = spaces

    // No escapes, as in the metric parser.
    let private quoted: P<string> =
        let inQuotes (q: char) = between (pchar q) (pchar q) (manySatisfy (fun c -> c <> q))
        inQuotes '\'' <|> inQuotes '"'

    let private word: P<string> = many1Satisfy (fun c -> isLetter c || isDigit c || c = '_') <?> "a name"

    let private arg: P<Arg> = (quoted |>> Item) <|> (Parser.number |>> Count)

    let private call: P<Call> =
        pchar '.' >>. ws >>. word .>> ws .>>. between (pchar '(' >>. ws) (pchar ')') (sepBy (arg .>> ws) (pchar ',' >>. ws))
        .>> ws
        |>> fun (name, args) -> { Name = name; Args = args }

    let read (text: string) : Result<string * Call list, string> =
        run (ws >>. quoted .>> ws .>>. many call .>> eof) text |> Parser.toResult

let problemsOf (result: Result<'T, string list>) : string list =
    match result with
    | Error problems -> problems
    | Ok _ -> []

let private collect (results: Result<'T, string> list) : Result<'T list, string list> =
    match results |> List.choose (fun r -> match r with Error e -> Some e | Ok _ -> None) with
    | [] -> Ok(results |> List.choose Result.toOption)
    | problems -> Error problems

let private texts (c: Call) : Result<string list, string list> =
    let items = c.Args |> List.choose (fun a -> match a with Item t -> Some(t.Trim()) | Count _ -> None)

    if items.Length <> c.Args.Length then Error [ $"{c.Name}() takes quoted strings" ]
    elif items.IsEmpty || List.exists String.IsNullOrEmpty items then Error [ $"{c.Name}() cannot be blank" ]
    else Ok items

let private tagFilter (call: string) (item: string) : Result<TagFilter, string> =
    Parser.parseTagFilter ("{" + item + "}") |> Result.mapError (fun e -> $"{call}(\"{item}\"): {e}")

let private known = [ "over"; "exclude"; "by"; "last"; "count_by_status" ]

let private shapeProblems (name: string) (calls: Call list) =
    let names = calls |> List.map _.Name

    [ if name.Trim() = "" then
          "the check name is empty"
      if List.contains "pct_by_status" names then
          "cluster alerts (pct_by_status) are not supported yet"
      else
          match List.tryLast calls with
          | Some { Name = "count_by_status"; Args = [] } -> ()
          | _ -> "a check monitor's query ends in .count_by_status()"
      for n in names |> List.distinct do
          if not (List.contains n known) && n <> "pct_by_status" then
              $".{n}() is not part of a check query, which takes over, exclude, by, last and count_by_status"
          elif names |> List.filter ((=) n) |> List.length > 1 then
              $".{n}() is given more than once" ]

/// WARNING(undocumented): what over() takes. Each item is a tag filter, a comma is AND, and a `*`
/// among tags is dropped; exclude() items are ANDed on as NOT.
let private overOf (call: Call option) =
    match call |> Option.map texts with
    | None -> Ok []
    | Some(Ok items) -> items |> List.filter (fun i -> i <> "*") |> List.map (tagFilter "over") |> collect
    | Some(Error e) -> Error e

let private excludeOf (call: Call option) =
    match call |> Option.map texts with
    | None -> Ok []
    | Some(Ok items) when List.contains "*" items -> Error [ "exclude() cannot take \"*\"" ]
    | Some(Ok items) -> items |> List.map (tagFilter "exclude" >> Result.map Not) |> collect
    | Some(Error e) -> Error e

// WARNING(undocumented): by("*") is one group per series, as Datadog's own monitors name
// {{host.name}} under it; no by() is one group. A `key:value` is refused rather than guessed at.
let private byOf (call: Call option) =
    match call |> Option.map texts with
    | None -> Ok []
    | Some(Ok [ "*" ]) -> Ok [ "*" ]
    | Some(Ok items) when List.contains "*" items -> Error [ "by() takes \"*\" alone or tag keys" ]
    | Some(Ok items) ->
        match items |> List.filter (fun i -> i.Contains ':') with
        | [] -> Ok(items |> List.distinct |> List.sort)
        | bad -> Error [ for i in bad -> $"by() takes tag keys; put {i} in over()" ]
    | Some(Error e) -> Error e

let private lastOf (call: Call option) =
    match call with
    | Some { Args = [ Count n ] } when n = Math.Floor n && n >= 1.0 && n <= 100.0 -> Ok(int n)
    | Some _ -> Error [ "last() takes a whole number of runs from 1 to 100" ]
    | None -> Error [ "last(N) is required: the number of runs the thresholds count in" ]

let query (text: string) : Result<CheckQuery, string list> =
    match Grammar.read text with
    | Error e -> Error [ e ]
    | Ok(name, calls) ->
        let find (n: string) = calls |> List.tryFind (fun c -> c.Name = n)

        match shapeProblems name calls, overOf (find "over"), excludeOf (find "exclude"), byOf (find "by"), lastOf (find "last") with
        | [], Ok over, Ok exclude, Ok by, Ok last ->
            Ok
                { Check = name
                  Filter = Parser.andOf (over @ exclude)
                  GroupBy = by
                  Last = last }
        | shape, over, exclude, by, last ->
            Error(shape @ problemsOf over @ problemsOf exclude @ problemsOf by @ problemsOf last)

/// No Data must fire before a silent group is forgotten (State.forgetAfter).
let longestNoData = TimeSpan.FromHours 48.0

let definition (text: string) (options: string) : Result<CheckDefinition, string list> =
    match query text, Options.parse options with
    | Ok q, Ok o ->
        let t = o.Thresholds

        let count (name: string) (value: float option) =
            [ match value with
              | Some v when v <> Math.Floor v || v < 1.0 || v > 100.0 ->
                  $"options.thresholds.{name} is a count of consecutive runs, a whole number from 1 to 100"
              | _ -> () ]

        // WARNING(undocumented): a missing threshold is 1 run.
        let critical, warning, ok = defaultArg t.Critical 1.0, defaultArg t.Warning 1.0, defaultArg t.Ok 1.0
        let largest = List.max [ critical; warning; ok; defaultArg t.Unknown 1.0 ]

        let problems =
            count "critical" t.Critical
            @ count "warning" t.Warning
            @ count "ok" t.Ok
            @ count "unknown" t.Unknown
            @ [ if float q.Last < largest then
                    $"last({q.Last}) must be at least the largest threshold ({largest}), as the thresholds count among the last runs"
                if t.CriticalRecovery.IsSome || t.WarningRecovery.IsSome then
                    "recovery thresholds are not available for check monitors; options.thresholds.ok counts the runs to resolve"
                if o.TriggerWindow.IsSome || o.RecoveryWindow.IsSome then
                    "options.threshold_windows is for anomaly monitors"
                if o.OnMissingData.IsSome then
                    "options.on_missing_data is not available for check monitors; use notify_no_data and no_data_timeframe"
                match o.NoDataTimeframe with
                | Some timeframe when timeframe > longestNoData ->
                    $"options.no_data_timeframe ({timeframe.TotalMinutes} minutes) must be at most 2880, as a silent check group is forgotten after 48 hours"
                | _ -> () ]

        match problems with
        | [] ->
            Ok
                { Query = q
                  Options =
                    { o with
                        Thresholds =
                            { t with
                                Critical = Some critical
                                Warning = Some warning
                                Ok = Some ok }
                        NotActedOn = o.NotActedOn @ [ if o.RequireFullWindow then "require_full_window" ] } }
        | problems -> Error problems
    | q, o -> Error(problemsOf q @ problemsOf o)

// --- reading the runs ------------------------------------------------------------------

/// How far back a tick reads: 8 runs of a check every 15 minutes, last(100) of one every 72 s, or
/// the no-data timeframe when longer, so a check run less often has its runs read.
let private lookback (c: CheckDefinition) =
    max (TimeSpan.FromHours 2.0) (defaultArg c.Options.NoDataTimeframe TimeSpan.Zero)

/// WARNING(undocumented): evaluation_delay moves the read back, with no minimum: a run is stored
/// or not, and one stored late still counts at the next tick.
let period (c: CheckDefinition) (at: DateTimeOffset) : Period =
    let until = at - c.Options.EvaluationDelay
    { From = until - lookback c; To = until }

let private millis (t: DateTimeOffset) = SqlValue.Int64(t.ToUnixTimeMilliseconds())

let private where (bound: SqlParams) (tenant: TenantId) (q: CheckQuery) (from: DateTimeOffset) (until: DateTimeOffset) =
    let (TenantId t) = tenant

    $"""tenant_id = {bound.Add("tenant", SqlValue.String t)} AND check_name = {bound.Add("check", SqlValue.String q.Check)}
      AND timestamp >= fromUnixTimestamp64Milli({bound.Add("from", millis from)})
      AND timestamp < fromUnixTimestamp64Milli({bound.Add("to", millis until)})
      AND ({Filter.sql Set.empty bound q.Filter})"""

/// A run's tags as `k:v`, or `k` for a tag without a value.
let private tagPairs =
    "arrayFlatten(arrayMap((k, vs) -> arrayMap(v -> if(v = '', toString(k), concat(toString(k), ':', toString(v))), vs), mapKeys(tags), mapValues(tags)))"

/// The newest runs of each series, a host and tag set, in Compile.Row's columns: `value` is the
/// run's status, `bucket_ms` its time. N of its known runs and N of its UNKNOWN ones, two at least,
/// as the live rule needs the gap between its newest two runs. A run stored twice is one run.
let sql (tenant: TenantId) (q: CheckQuery) (p: Period) : Sql =
    let bound = SqlParams()

    let groups =
        match q.GroupBy with
        | [ "*" ] -> $"arrayStringConcat(arraySort(arrayConcat(if(host = '', [], [concat('host:', toString(host))]), {tagPairs})), ',') AS g0, "
        | keys -> keys |> List.mapi (fun i k -> $"{Compile.groupSql Set.empty bound k} AS g{i}, ") |> String.concat ""

    { Text =
        $"""SELECT {groups}toInt64(toUnixTimestamp(timestamp)) * 1000 AS bucket_ms, toFloat64(CAST(status, 'Int8')) AS value, series_id
FROM (
    SELECT DISTINCT timestamp, host, tags, status, cityHash64(host, tags) AS series_id
    FROM check_runs
    WHERE {where bound tenant q p.From p.To}
    ORDER BY timestamp DESC
    LIMIT {max q.Last 2} BY series_id, status = 'UNKNOWN'
)"""
      Parameters = bound.All }

/// The newest runs of the check, newest first: time, host, status, message and tags as `k:v`.
let runs (tenant: TenantId) (q: CheckQuery) (from: DateTimeOffset) (until: DateTimeOffset) : Sql =
    let bound = SqlParams()

    { Text =
        $"""SELECT toInt64(toUnixTimestamp(timestamp)) * 1000, host, toString(status), message, {tagPairs}
FROM check_runs
WHERE {where bound tenant q from until}
ORDER BY timestamp DESC
LIMIT 100"""
      Parameters = bound.All }

// --- judging ---------------------------------------------------------------------------

[<Literal>]
let private StatusOk = 0

[<Literal>]
let private StatusWarning = 1

[<Literal>]
let private StatusCritical = 2

[<Literal>]
let private StatusUnknown = 3

/// WARNING(undocumented): a series counts while `until − newest ≤ 2 × gap + 1 min`, so a renamed
/// host or an old tag set stops counting; one with a single run in the window counts while it is
/// there, as a check run once a day does.
let private streakOf (last: int) (p: Period) (runs: Compile.Row list) : (Streak * int64) option =
    let runs = runs |> List.sortByDescending _.BucketMs |> List.distinctBy _.BucketMs
    let newest = runs.Head.BucketMs

    let live =
        match runs with
        | _ :: previous :: _ -> p.To.ToUnixTimeMilliseconds() - newest <= 2L * (newest - previous.BucketMs) + 60_000L
        | _ -> true

    if not live then
        None
    else
        let statuses = runs |> List.map (fun r -> int r.Value)
        let known = statuses |> List.filter ((<>) StatusUnknown) |> List.truncate last
        let leading (status: int) (among: int list) = among |> List.takeWhile ((=) status) |> List.length

        Some(
            { Latest = statuses.Head
              Critical = leading StatusCritical known
              Warning = leading StatusWarning known
              Ok = leading StatusOk known
              Unknown = leading StatusUnknown statuses
              OkSinceUnknown = leading StatusOk (List.truncate last statuses) },
            newest
        )

/// The rows of `sql` as an evaluation: one group per combination of the `by` values, or per series
/// for by("*"), each with its live series. A group with none is left out, so it goes silent.
let evaluation (q: CheckQuery) (p: Period) (maxGroups: int) (rows: Compile.Row list) : Result<Evaluation, string> =
    let pairsOf (values: string option list) =
        match q.GroupBy, values with
        | [ "*" ], [ Some series ] -> series.Split(',', StringSplitOptions.RemoveEmptyEntries) |> List.ofArray |> List.sort
        | keys, values -> List.map2 (fun k (v: string option) -> $"{k}:{v.Value}") keys values |> List.sort

    let groups =
        rows
        |> List.filter (fun r -> r.Groups |> List.forall Option.isSome)
        |> List.groupBy (fun r -> pairsOf r.Groups)

    if groups.Length > maxGroups then
        Error $"too_many_groups: the query yields {groups.Length} groups, at most {maxGroups} are evaluated"
    else
        Ok
            { Period = p
              Groups =
                [ for pairs, members in groups do
                      let live = members |> List.groupBy _.SeriesId |> List.choose (snd >> streakOf q.Last p)

                      if not live.IsEmpty then
                          { Key = (if pairs.IsEmpty then "*" else String.concat "," pairs)
                            Tags = pairs
                            Reading = Streaks(List.map fst live)
                            Recovery = None
                            LastPoint = Some(DateTimeOffset.FromUnixTimeMilliseconds(live |> List.map snd |> List.max)) } ] }

let evaluate
    (execute: Sql -> Task<Compile.Row list>)
    (tenant: TenantId)
    (maxGroups: int)
    (c: CheckDefinition)
    (at: DateTimeOffset)
    : Task<Result<Evaluation, string>> =
    task {
        let p = period c at
        let! rows = execute (sql tenant c.Query p)
        return evaluation c.Query p maxGroups rows
    }

let private severity (status: int) =
    match status with
    | StatusCritical -> 3
    | StatusWarning -> 2
    | StatusUnknown -> 1
    | _ -> 0

/// The worst newest status, as the group's value: CRITICAL, then WARNING, UNKNOWN and OK.
let worst (streaks: Streak list) : float =
    streaks |> List.map _.Latest |> List.maxBy severity |> float
