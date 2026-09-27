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
///   formulas: query names, wrapped in functions that act on each value
///   (abs, log2, log10, ceil, floor, round, clamp_*, cutoff_*) or across
///   series (top, top10_mean-style shorthands, count_nonzero, count_not_null,
///   exclude_null) or along time (cumsum, integral, diff, monotonic_diff,
///   derivative, per_second/minute/hour, throughput, ewma_*, median_*,
///   rollingavg_*), with an optional limit
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
    /// default".
    ///
    /// WARNING(undocumented): whether Datadog's backend applies `.as_count()`
    /// on its own to COUNT and RATE metrics with no modifier. Its UI appends
    /// it to the query text, so dashboard queries carry it explicitly. We
    /// compute what the query says: avg.
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
      Step: TimeSpan
      /// Buckets to fetch before `from`, for functions that look back — a
      /// diff at the first visible point needs the point before it.
      Lookback: int }

/// One series group in the response: a formula, or a query shown as is when
/// the request had no formulas.
/// A function that maps each value on its own, independent of its
/// neighbours and of other series (docs: dashboards/functions/arithmetic,
/// exclusion).
type PointwiseFn =
    | Abs
    | Log2
    | Log10
    | Ceil
    | Floor
    /// `round(q)` or `round(q, decimals)`.
    | Round of decimals: int
    /// Values below the threshold are raised to it.
    | ClampMin of float
    /// Values above the threshold are lowered to it.
    | ClampMax of float
    /// Values below the threshold are removed; equal ones stay.
    | CutoffMin of float
    /// Values above the threshold are removed; equal ones stay.
    | CutoffMax of float

/// What `top` ranks series by (docs: dashboards/functions/rank).
type RankBy =
    | ByMax
    | ByMean
    | ByMin
    | BySum
    /// The series' latest value.
    | ByLast
    /// sqrt of the sum of squares: "the norm of the timeseries, which is
    /// always positive".
    | ByL2norm
    /// "Signed area under the curve, which can be negative".
    | ByArea

/// A function along one series' time axis: each output point depends on
/// earlier points of the same series (docs: dashboards/functions/rate,
/// arithmetic, smoothing, beta).
type TimewiseFn =
    /// Running sum "over the visible time window" — no lookback, by the docs.
    | Cumsum
    /// Running sum of Δt (seconds) × Δvalue over consecutive points, as the
    /// docs define it word for word.
    /// FIXME(integral): probably a docs error — see Evaluate.fs.
    | Integral
    /// Value minus the previous point's.
    | Diff
    /// Diff, only where it is not negative.
    | MonotonicDiff
    /// Diff divided by Δt in seconds.
    | Derivative
    | PerSecond
    | PerMinute
    | PerHour
    /// Each value divided by the bucket width in seconds.
    | Throughput
    /// Exponentially weighted moving average, alpha = 2 / (span + 1).
    | Ewma of span: int
    /// Median of the last `span` points.
    | Median of span: int
    /// Mean of the last `span` points.
    | RollingAvg of span: int

/// Buckets of history a function needs before its first output point.
///
/// WARNING(undocumented): how much history Datadog reads for these, and
/// whether at all. The amounts below are ours.
///
///   diff-like   1: the point before the first.
///   median, rollingavg   span - 1: a full window at the first point.
///   ewma   2 × span: by then the weight left on the start is
///          (1 - 2/(span+1))^(2·span) ≈ e^-4, under 2 %.
///   cumsum, integral, throughput   0: the docs make cumsum and integral
///          start at the visible window; throughput is per point.
let lookback =
    function
    | Cumsum
    | Integral
    | Throughput -> 0
    | Diff
    | MonotonicDiff
    | Derivative
    | PerSecond
    | PerMinute
    | PerHour -> 1
    | Median n
    | RollingAvg n -> n - 1
    | Ewma n -> 2 * n

/// What a formula computes: its names resolved to queries, every function
/// checked against what it takes. A function that exists here has arguments
/// of the right number and type — the evaluator never has to check.
type Node =
    /// The series one query returned; the index is into Plan.Queries.
    | Fetch of source: int
    | Pointwise of PointwiseFn * Node
    /// Keep `count` series, ranked by `by`, in `order` (desc = the largest).
    | Top of Node * count: int * by: RankBy * order: SortOrder
    /// One series: at each time, how many series have a non-zero value.
    | CountNonzero of Node
    /// One series: at each time, how many series have a value at all.
    | CountNotNull of Node
    /// Drop the series whose group has an N/A tag value.
    | ExcludeNull of Node
    | Timewise of TimewiseFn * Node

type Output =
    {
        /// What the response calls `query_index`: the formula's position, or
        /// the query's when there are no formulas.
        QueryIndex: int
        Node: Node
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
/// (docs: dashboards/functions/rollup). Rows are "up to this window".
///
/// WARNING(undocumented): the step for a window between two rows (say 2 h).
/// It takes the coarser row's step.
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

/// A requested width, honoured unless it would exceed maxPoints. Datadog
/// widens both `interval` ("may override with a larger interval") and
/// `.rollup(_, seconds)` ("up to a limit of 1,500 points").
///
/// WARNING(undocumented): by how much Datadog widens. We use the smallest
/// whole second that fits, and round a sub-second width up — buckets are
/// `toStartOfInterval` in seconds.
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
              // WARNING(undocumented): the space aggregator for a query with no
              // prefix. Datadog's default depends on the metric type; we use
              // avg, what it would pick for a gauge.
              SpaceAgg = q.SpaceAgg |> Option.bind spaceAggregation |> Option.defaultValue Avg
              TimeAgg = timeAgg
              Step = mods.Interval |> Option.map (fitted window) |> Option.defaultValue requestStep
              Lookback = 0 }
    | _ -> Error errors

// --- formulas --------------------------------------------------------------------

/// The functions that take one series argument and act on each value, by the
/// shape of their other arguments.
let private pointwiseFunctions: Map<string, Literal list -> Result<PointwiseFn, string>> =
    let none fn name =
        name,
        function
        | [] -> Ok fn
        | _ -> Error $"{name}() takes one argument: {name}(query)"

    let threshold ctor name =
        name,
        function
        | [ Num t ] -> Ok(ctor t)
        | _ -> Error $"{name}() takes a query and a number: {name}(query, 100)"

    Map
        [ none Abs "abs"
          none Log2 "log2"
          none Log10 "log10"
          none Ceil "ceil"
          none Floor "floor"
          "round",
          (function
          | [] -> Ok(Round 0)
          | [ Num d ] when d >= 0.0 && d <= 15.0 && d = Math.Floor d -> Ok(Round(int d))
          | _ -> Error "round() takes a query and optionally a whole number of decimals from 0 to 15: round(query, 2)")
          threshold ClampMin "clamp_min"
          threshold ClampMax "clamp_max"
          threshold CutoffMin "cutoff_min"
          threshold CutoffMax "cutoff_max" ]

/// Time-wise functions by name. The numbered forms are exactly the ones
/// documented; `ewma()` is ewma_20 and `median()` is median_3, as the docs say.
let private timewiseFunctions: Map<string, TimewiseFn> =
    Map
        [ "cumsum", Cumsum
          "integral", Integral
          "diff", Diff
          "monotonic_diff", MonotonicDiff
          "derivative", Derivative
          "per_second", PerSecond
          "per_minute", PerMinute
          "per_hour", PerHour
          "throughput", Throughput
          for n in [ 1; 3; 5; 7; 10; 20 ] do
              $"ewma_{n}", Ewma n
          "ewma", Ewma 20
          for n in [ 3; 5; 7; 9 ] do
              $"median_{n}", Median n
          "median", Median 3
          for n in [ 5; 13; 21; 29 ] do
              $"rollingavg_{n}", RollingAvg n ]

/// A plain argument after the series one, as the analysis sees it. In a
/// formula a bare word (`mean`) parses as a query name — the parser cannot
/// know better — so here, where the function says a word is wanted, a name
/// becomes a word again.
let private asLiteral (arg: Arg<string>) : Literal option =
    match arg with
    | Value(Number n) -> Some(Num n)
    | Value(Leaf w) -> Some(Word w)
    | Lit l -> Some l
    | _ -> None

let private rankBy =
    function
    | "max" -> Some ByMax
    | "mean" -> Some ByMean
    | "min" -> Some ByMin
    | "sum" -> Some BySum
    | "last" -> Some ByLast
    | "l2norm" -> Some ByL2norm
    | "area" -> Some ByArea
    | _ -> None

/// `top(q, 10, 'mean', 'desc')`.
///
/// WARNING(undocumented): defaults for top()'s arguments. All four are
/// required; Datadog's editor always writes them out.
let private topArgs (literals: Literal list) : Result<int * RankBy * SortOrder, string> =
    let word =
        function
        | Word w
        | Quoted w -> Some w
        | Num _ -> None

    match literals with
    | [ Num n; by; dir ] ->
        match int n, word by |> Option.bind rankBy, word dir with
        | limit, _, _ when float limit <> n || not (List.contains limit [ 5; 10; 25; 50; 100 ]) ->
            Error "top() limit must be one of 5, 10, 25, 50, 100"
        | _, None, _ -> Error "top() ranks by one of 'max', 'mean', 'min', 'sum', 'last', 'l2norm', 'area'"
        | limit, Some by, Some "desc" -> Ok(limit, by, Desc)
        | limit, Some by, Some "asc" -> Ok(limit, by, Asc)
        | _ -> Error "top() direction must be 'asc' or 'desc'"
    | _ -> Error "top() takes a query, a limit, a ranking and a direction: top(query, 10, 'mean', 'desc')"

/// `top5_mean`, `bottom10_min`… — the documented shorthands:
/// [top, bottom][5, 10, 15, 20]_[mean, min, max, last, area, l2norm].
///
/// WARNING(undocumented): whether bare `top10()` exists and what it ranks
/// by. It is not accepted.
let private topShorthand (name: string) : (int * RankBy * SortOrder) option =
    let m = Text.RegularExpressions.Regex.Match(name, "^(top|bottom)(5|10|15|20)_(mean|min|max|last|area|l2norm)$")

    if m.Success then
        rankBy m.Groups[3].Value
        |> Option.map (fun by -> int m.Groups[2].Value, by, (if m.Groups[1].Value = "top" then Desc else Asc))
    else
        None

/// A formula's expression tree → Node, with every problem in it named.
let rec private resolve (label: string) (byName: Map<string, int>) (expr: Formula) : Result<Node, string list> =
    match expr with
    | Leaf name ->
        match byName.TryFind name with
        | Some i -> Ok(Fetch i)
        | None -> Error [ $"{label}: no query is named '{name}'" ]
    | Number _
    | Neg _
    | Binary _ -> Error [ $"{label}: arithmetic in formulas is not supported yet" ]
    | Call(name, args) ->
        let inner, rest =
            match args with
            | Value inner :: rest -> Some inner, rest
            | _ -> None, []

        match inner with
        | None when args.IsEmpty -> Error [ $"{label}: {name}() needs a query to act on" ]
        | None -> Error [ $"{label}: the first argument of {name}() must be a query" ]
        | Some inner ->
            let literals = rest |> List.map asLiteral

            // The function itself, from its name and plain arguments, before
            // the inner expression is resolved.
            let build: Result<Node -> Node, string> =
                if not (List.forall Option.isSome literals) then
                    Error $"{name}() takes only numbers and words after the query"
                else
                    let literals = List.choose id literals

                    match name, literals with
                    | _ when pointwiseFunctions.ContainsKey name ->
                        // Pointwise thresholds are numbers, never words.
                        if literals |> List.exists (function Num _ -> false | _ -> true) then
                            Error $"{name}() takes only numbers after the query"
                        else
                            pointwiseFunctions[name] literals |> Result.map (fun fn node -> Pointwise(fn, node))
                    | "top", _ -> topArgs literals |> Result.map (fun (n, by, dir) node -> Top(node, n, by, dir))
                    | ("count_nonzero" | "count_nonzero_finite"), [] -> Ok CountNonzero
                    | "count_not_null", [] -> Ok CountNotNull
                    | "exclude_null", [] -> Ok ExcludeNull
                    | _, [] when timewiseFunctions.ContainsKey name ->
                        let fn = timewiseFunctions[name]
                        Ok(fun node -> Timewise(fn, node))
                    | _, _ when timewiseFunctions.ContainsKey name -> Error $"{name}() takes one argument: {name}(query)"
                    | ("count_nonzero" | "count_nonzero_finite" | "count_not_null" | "exclude_null"), _ ->
                        Error $"{name}() takes one argument: {name}(query)"
                    | _ ->
                        match topShorthand name, literals with
                        | Some(n, by, dir), [] -> Ok(fun node -> Top(node, n, by, dir))
                        | Some _, _ -> Error $"{name}() takes one argument: {name}(query)"
                        | None, _ -> Error $"function '{name}' is not supported yet"

            match resolve label byName inner, build with
            | Ok inner, Ok build -> Ok(build inner)
            | inner, build ->
                Error(
                    (match inner with Error es -> es | Ok _ -> [])
                    @ (match build with Error e -> [ $"{label}: {e}" ] | Ok _ -> [])
                )

let private planOutputs (req: ParsedTimeseriesRequest) : Result<Output list, string list> =
    match req.Formulas with
    | [] -> Ok(req.Queries |> List.mapi (fun i _ -> { QueryIndex = i; Node = Fetch i; Limit = None }))
    | formulas ->
        let byName =
            req.Queries
            |> List.indexed
            |> List.choose (fun (i, q) -> q.Name |> Option.map (fun n -> n, i))
            |> Map.ofList

        let results =
            formulas
            |> List.mapi (fun i f ->
                resolve $"formulas[{i}]" byName f.Formula
                |> Result.map (fun node -> { QueryIndex = i; Node = node; Limit = f.Limit }))

        match results |> List.collect (function Error es -> es | Ok _ -> []) with
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
        // The deepest history any output asks of each query.
        let rec needs (extra: int) (node: Node) : (int * int) list =
            match node with
            | Fetch i -> [ i, extra ]
            | Timewise(fn, inner) -> needs (extra + lookback fn) inner
            | Pointwise(_, inner)
            | Top(inner, _, _, _)
            | CountNonzero inner
            | CountNotNull inner
            | ExcludeNull inner -> needs extra inner

        let lookbacks =
            outputs
            |> List.collect (_.Node >> needs 0)
            |> List.groupBy fst
            |> List.map (fun (i, ns) -> i, ns |> List.map snd |> List.max)
            |> Map.ofList

        Ok
            { From = req.From
              To = req.To
              Step = requestStep
              Queries =
                queries
                |> List.choose (function Ok q -> Some q | Error _ -> None)
                |> List.mapi (fun i q -> { q with Lookback = defaultArg (lookbacks.TryFind i) 0 })
              Outputs = outputs }
    | errors, _ -> Error errors
