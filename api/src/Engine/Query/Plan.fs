/// Analysis: a parsed request becomes a plan — everything decided that the
/// SQL compiler and the evaluator need, and nothing left to guess.
///
/// This is where defaults are filled in (the step, the aggregators), where
/// formulas are resolved to the queries they name, and where anything the
/// engine cannot do yet is refused by name rather than half-answered.
///
/// What is supported so far — the simple path:
///
///   avg|sum|min|max:metric{any tag filter} [by {keys}]
///   formulas that are a single query name, with an optional limit
///
/// Everything else (functions, arithmetic, modifiers, percentiles) gets a
/// "not supported yet" naming the construct.
module NinjaCat.Api.Engine.Query.Plan

open System
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Api.V2.Timeseries

type Aggregation =
    | Avg
    | Sum
    | Min
    | Max

type QueryPlan =
    { Metric: string
      Filter: TagFilter
      /// Tag keys, in the order of `by {...}`; `host` is one of them like any
      /// other, though it is stored in its own column.
      GroupBy: string list
      /// Across series, per bucket: the `avg:` in `avg:metric{*}`.
      SpaceAgg: Aggregation
      /// Within one series, per bucket. Always avg until `.rollup()` exists:
      /// that is Datadog's default for gauges, the only type handled yet.
      TimeAgg: Aggregation }

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
      /// The bucket width, shared by every query so their series line up.
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

/// With an `interval`, Datadog treats it as a hint it "may override with a
/// larger interval": we honour it unless it would exceed maxPoints, and round
/// it up to whole seconds — buckets are `toStartOfInterval` in seconds.
let step (window: TimeSpan) (hint: TimeSpan option) : TimeSpan =
    let wholeSeconds (t: TimeSpan) = TimeSpan.FromSeconds(Math.Ceiling(max t.TotalSeconds 1.0))

    match hint with
    | Some h ->
        let floor = TimeSpan.FromTicks(window.Ticks / int64 maxPoints)
        wholeSeconds (max h floor)
    | None ->
        defaultSteps
        |> List.tryFind (fun (upTo, _) -> window <= upTo)
        |> Option.map snd
        |> Option.defaultValue (TimeSpan.FromDays 7.0)

// --- queries ---------------------------------------------------------------------

let private aggregation =
    function
    | "avg" -> Some Avg
    | "sum" -> Some Sum
    | "min" -> Some Min
    | "max" -> Some Max
    | _ -> None

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

let private planQuery (label: string) (q: MetricQuery) : Result<QueryPlan, string list> =
    let errors =
        [ match q.SpaceAgg with
          | Some a when (aggregation a).IsNone -> $"{label}: aggregator '{a}:' is not supported yet"
          | _ -> ()
          for m in q.Modifiers do
              $"{label}: .{m.Name}() is not supported yet"
          for v in templateVariables q.Filter do
              $"{label}: template variable '{v}' was not substituted" ]

    if errors.IsEmpty then
        Ok
            { Metric = q.Metric
              Filter = q.Filter
              GroupBy = q.GroupBy
              // No prefix: avg. Datadog's own default depends on the metric
              // type; with gauges only, avg is what it would pick.
              SpaceAgg = q.SpaceAgg |> Option.bind aggregation |> Option.defaultValue Avg
              TimeAgg = Avg }
    else
        Error errors

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

    let queries =
        req.Queries
        |> List.mapi (fun i q ->
            let label = $"""queries[{i}]{q.Name |> Option.map (sprintf " (%s)") |> Option.defaultValue ""}"""
            planQuery label q.Query)

    let queryErrors = queries |> List.collect (function Error es -> es | Ok _ -> [])
    let outputs = planOutputs req
    let outputErrors = match outputs with Error es -> es | Ok _ -> []

    match duplicates @ queryErrors @ outputErrors, outputs with
    | [], Ok outputs ->
        Ok
            { From = req.From
              To = req.To
              Step = step (req.To - req.From) req.Interval
              Queries = queries |> List.choose (function Ok q -> Some q | Error _ -> None)
              Outputs = outputs }
    | errors, _ -> Error errors
