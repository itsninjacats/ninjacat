/// Analysis: a parsed request becomes a plan — everything decided that the
/// SQL compiler and the evaluator need, and nothing left to guess.
///
/// This is where defaults are filled in (the step, the aggregators), where
/// formulas are resolved to the queries they name, and where anything the
/// engine cannot do yet is refused by name rather than half-answered.
///
/// What is supported so far:
///
///   avg|sum|min|max:metric{any tag filter} [by {keys}]
///     [.rollup(avg|sum|min|max|count[, seconds])] [.as_count() | .as_rate()]
///   formulas that are a single query name, with an optional limit
///
/// Everything else (functions, arithmetic, other modifiers, calendar rollups,
/// percentiles) gets a "not supported yet" naming the construct.
module NinjaCat.Api.Engine.Query.Plan

open System
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Api.V2.Timeseries

type Aggregation =
    | Avg
    | Sum
    | Min
    | Max
    /// The number of points: a rollup method only, never a space aggregator
    /// (`count:` is a distribution prefix).
    | Count

/// How one series' points in one bucket become one value.
///
/// Datadog: docs/dashboards/functions/rollup, metrics/custom_metrics/type_modifiers.
type TimeAggregation =
    /// `.rollup(method)`, or no modifier at all: avg, Datadog's "enforced
    /// default". Whether Datadog's backend applies `.as_count()` on its own to
    /// COUNT and RATE metrics is not documented — its UI appends it to the
    /// query text, so dashboard queries carry it explicitly. We compute what
    /// the query says.
    | Plain of Aggregation
    /// Events per bucket: COUNT sums, RATE sums value × interval, GAUGE is
    /// unaffected (avg).
    | AsCount
    /// Events per second: AsCount divided by the bucket width — for RATE,
    /// by the width floored to the metric's own interval, as Datadog does.
    | AsRate

type QueryPlan =
    { Metric: string
      Filter: TagFilter
      /// Tag keys, in the order of `by {...}`; `host` is one of them like any
      /// other, though it is stored in its own column.
      GroupBy: string list
      /// Across series, per bucket: the `avg:` in `avg:metric{*}`.
      SpaceAgg: Aggregation
      /// Within one series, per bucket.
      TimeAgg: TimeAggregation
      /// The bucket width for this query: `.rollup(_, seconds)`, or the
      /// request's step.
      Step: TimeSpan }

/// One series group in the response: a formula, or a query shown as is when
/// the request had no formulas.
type Output =
    {
        /// What the response calls `query_index`: the formula's position, or
        /// the query's when there are no formulas.
        QueryIndex: int
        /// Index into Plan.Queries.
        Source: int
        Limit: ParsedLimit option
    }

type Plan =
    { From: DateTimeOffset
      To: DateTimeOffset
      /// The request's bucket width: from `interval`, or Datadog's table. A
      /// query with its own `.rollup(_, seconds)` overrides it.
      Step: TimeSpan
      Queries: QueryPlan list
      Outputs: Output list }

// --- the step --------------------------------------------------------------------

/// Datadog's documented defaults for a line graph, window → bucket
/// (docs: dashboards/functions/rollup). Rows are "up to this window"; a window
/// between two rows takes the coarser row's step, which is our guess — Datadog
/// does not document the in-between.
let private defaultSteps =
    [ TimeSpan.FromMinutes 1.0, TimeSpan.FromSeconds 1.0
      TimeSpan.FromMinutes 5.0, TimeSpan.FromSeconds 2.0
      TimeSpan.FromMinutes 15.0, TimeSpan.FromSeconds 5.0
      TimeSpan.FromMinutes 30.0, TimeSpan.FromSeconds 10.0
      TimeSpan.FromHours 1.0, TimeSpan.FromSeconds 20.0
      TimeSpan.FromHours 4.0, TimeSpan.FromMinutes 1.0
      TimeSpan.FromDays 1.0, TimeSpan.FromMinutes 5.0
      TimeSpan.FromDays 7.0, TimeSpan.FromHours 1.0
      TimeSpan.FromDays 31.0, TimeSpan.FromHours 4.0
      TimeSpan.FromDays 366.0, TimeSpan.FromDays 1.0 ]

/// Datadog's cap on points per series for line and bar graphs.
let maxPoints = 1500

/// A requested width, honoured unless it would exceed maxPoints, and rounded
/// up to whole seconds — buckets are `toStartOfInterval` in seconds. Datadog
/// does the same to `interval` ("may override with a larger interval") and to
/// `.rollup(_, seconds)` ("up to a limit of 1,500 points").
let private fitted (window: TimeSpan) (requested: TimeSpan) =
    let floor = TimeSpan.FromTicks(window.Ticks / int64 maxPoints)
    TimeSpan.FromSeconds(Math.Ceiling(max (max requested floor).TotalSeconds 1.0))

let step (window: TimeSpan) (hint: TimeSpan option) : TimeSpan =
    match hint with
    | Some h -> fitted window h
    | None ->
        defaultSteps
        |> List.tryFind (fun (upTo, _) -> window <= upTo)
        |> Option.map snd
        |> Option.defaultValue (TimeSpan.FromDays 7.0)

// --- queries ---------------------------------------------------------------------

let private spaceAggregation =
    function
    | "avg" -> Some Avg
    | "sum" -> Some Sum
    | "min" -> Some Min
    | "max" -> Some Max
    | _ -> None

let private rollupMethod =
    function
    | "count" -> Some Count
    | m -> spaceAggregation m

// --- modifiers -------------------------------------------------------------------

/// What the modifiers ask for, before they are checked against each other.
type private Modifiers =
    { Method: Aggregation option
      Interval: TimeSpan option
      Mode: string option }

let private readModifiers (label: string) (mods: Modifier list) : Modifiers * string list =
    let apply (acc: Modifiers, errors: string list) (m: Modifier) =
        let fail msg = acc, errors @ [ $"{label}: {msg}" ]

        match m.Name, m.Args with
        | "rollup", method :: rest ->
            let methodName =
                match method with
                | Word w
                | Quoted w -> Some w
                | Num _ -> None

            match methodName |> Option.bind rollupMethod, rest with
            | None, _ -> fail ".rollup() method must be one of avg, sum, min, max, count"
            | Some agg, [] -> { acc with Method = Some agg }, errors
            | Some agg, [ Num seconds ] when seconds >= 1.0 ->
                { acc with Method = Some agg; Interval = Some(TimeSpan.FromSeconds seconds) }, errors
            | Some _, [ Num _ ] -> fail ".rollup() interval must be at least 1 second"
            // `.rollup(sum, daily, 12am, 'Europe/Paris')`: buckets on calendar
            // boundaries in a timezone, not fixed widths.
            | Some _, _ -> fail "calendar .rollup() (daily, weekly, monthly, alignment, timezone) is not supported yet"
        | "rollup", [] -> fail ".rollup() needs a method, e.g. .rollup(sum) or .rollup(sum, 60)"
        | ("as_count" | "as_rate") as mode, [] ->
            match acc.Mode with
            | Some other when other <> mode -> fail $".{mode}() cannot follow .{other}()"
            | _ -> { acc with Mode = Some mode }, errors
        | ("as_count" | "as_rate") as mode, _ -> fail $".{mode}() takes no arguments"
        | name, _ -> fail $".{name}() is not supported yet"

    mods |> List.fold apply ({ Method = None; Interval = None; Mode = None }, [])

let private timeAggregation (label: string) (m: Modifiers) : Result<TimeAggregation, string> =
    match m.Mode, m.Method with
    | None, method -> Ok(Plain(defaultArg method Avg))
    // as_count/as_rate set the time aggregator to sum; a rollup that says sum
    // agrees, one that says anything else contradicts it.
    | Some "as_count", (None | Some Sum) -> Ok AsCount
    | Some "as_rate", (None | Some Sum) -> Ok AsRate
    | Some mode, Some other -> Error $"{label}: .{mode}() with .rollup({(string other).ToLowerInvariant()}) is not supported yet"
    | Some mode, None -> Error $"{label}: unknown modifier .{mode}()"

/// Template variables (`{$env}`) belong to dashboards, which substitute them
/// before sending; one arriving here means nobody did.
let rec private templateVariables filter =
    match filter with
    | Bare v when v.StartsWith "$" -> [ v ]
    | Tag(_, v) when v.StartsWith "$" -> [ v ]
    | Not f -> templateVariables f
    | And fs
    | Or fs -> List.collect templateVariables fs
    | _ -> []

let private planQuery (window: TimeSpan) (requestStep: TimeSpan) (label: string) (q: MetricQuery) : Result<QueryPlan, string list> =
    let mods, modErrors = readModifiers label q.Modifiers

    let timeAgg = timeAggregation label mods

    let errors =
        [ match q.SpaceAgg with
          | Some a when (spaceAggregation a).IsNone -> $"{label}: aggregator '{a}:' is not supported yet"
          | _ -> ()
          yield! modErrors
          match timeAgg with
          | Error e -> e
          | Ok _ -> ()
          for v in templateVariables q.Filter do
              $"{label}: template variable '{v}' was not substituted" ]

    match errors, timeAgg with
    | [], Ok timeAgg ->
        Ok
            { Metric = q.Metric
              Filter = q.Filter
              GroupBy = q.GroupBy
              // No prefix: avg. Datadog's own default depends on the metric
              // type; for a gauge, avg is what it would pick.
              SpaceAgg = q.SpaceAgg |> Option.bind spaceAggregation |> Option.defaultValue Avg
              TimeAgg = timeAgg
              Step = mods.Interval |> Option.map (fitted window) |> Option.defaultValue requestStep }
    | _ -> Error errors

// --- formulas --------------------------------------------------------------------

let private planOutputs (req: ParsedTimeseriesRequest) : Result<Output list, string list> =
    match req.Formulas with
    | [] -> Ok(req.Queries |> List.mapi (fun i _ -> { QueryIndex = i; Source = i; Limit = None }))
    | formulas ->
        let byName =
            req.Queries
            |> List.indexed
            |> List.choose (fun (i, q) -> q.Name |> Option.map (fun n -> n, i))
            |> Map.ofList

        let results =
            formulas
            |> List.mapi (fun i f ->
                match f.Formula with
                | Leaf name ->
                    match byName.TryFind name with
                    | Some source -> Ok { QueryIndex = i; Source = source; Limit = f.Limit }
                    | None -> Error $"formulas[{i}]: no query is named '{name}'"
                | _ -> Error $"formulas[{i}]: only a single query name is supported yet, e.g. \"query1\"")

        match results |> List.choose (function Error e -> Some e | Ok _ -> None) with
        | [] -> Ok(results |> List.choose (function Ok o -> Some o | Error _ -> None))
        | errors -> Error errors

// --- the plan --------------------------------------------------------------------

let plan (req: ParsedTimeseriesRequest) : Result<Plan, string list> =
    let duplicates =
        req.Queries
        |> List.choose _.Name
        |> List.countBy id
        |> List.filter (fun (_, n) -> n > 1)
        |> List.map (fun (name, _) -> $"query name '{name}' is used more than once")

    let window = req.To - req.From
    let requestStep = step window req.Interval

    let queries =
        req.Queries
        |> List.mapi (fun i q ->
            let label = $"""queries[{i}]{q.Name |> Option.map (sprintf " (%s)") |> Option.defaultValue ""}"""
            planQuery window requestStep label q.Query)

    let queryErrors = queries |> List.collect (function Error es -> es | Ok _ -> [])
    let outputs = planOutputs req
    let outputErrors = match outputs with Error es -> es | Ok _ -> []

    match duplicates @ queryErrors @ outputErrors, outputs with
    | [], Ok outputs ->
        Ok
            { From = req.From
              To = req.To
              Step = requestStep
              Queries = queries |> List.choose (function Ok q -> Some q | Error _ -> None)
              Outputs = outputs }
    | errors, _ -> Error errors
