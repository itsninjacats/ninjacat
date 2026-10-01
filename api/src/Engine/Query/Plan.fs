/// A parsed request becomes a plan: defaults filled in, formula names resolved
/// to queries, every function's arguments checked. Anything the engine cannot
/// do yet is refused here by name, so later stages never have to check.
module NinjaCat.Api.Engine.Query.Plan

open System
open System.Text.RegularExpressions
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Api.V2.Timeseries

// --- types -----------------------------------------------------------------------

type Aggregation =
    | Avg
    | Sum
    | Min
    | Max
    /// Rollup only: `count:` as a prefix belongs to distributions.
    | Count

/// How one series' points in one bucket become one value.
type TimeAggregation =
    /// `.rollup(method)`, or avg when there is no modifier.
    ///
    /// WARNING(undocumented): whether Datadog applies `.as_count()` on its own
    /// to COUNT and RATE metrics. Its UI writes it into the query, so we only
    /// do what the query says.
    | Plain of Aggregation
    /// COUNT sums, RATE sums value × interval, GAUGE averages.
    | AsCount
    /// AsCount divided by the bucket width (for RATE, the width floored to the
    /// metric's own interval).
    | AsRate

type FillMethod =
    | Linear
    | Last
    | Zero

type Fill =
    | NoFill
    | FillWithin of FillMethod * limitSeconds: int

type QueryPlan =
    { Metric: string
      Filter: TagFilter
      GroupBy: string list
      SpaceAgg: Aggregation
      TimeAgg: TimeAggregation
      Step: TimeSpan
      /// Buckets read before `from`, for functions that look back.
      Lookback: int
      /// Anything but NoFill makes ClickHouse return series one by one, for
      /// F# to interpolate and combine.
      Fill: Fill }

type PointwiseFn =
    | Abs
    | Log2
    | Log10
    | Ceil
    | Floor
    | Round of decimals: int
    | ClampMin of float
    | ClampMax of float
    | CutoffMin of float
    | CutoffMax of float

type ArithOp =
    | Plus
    | Minus
    | Times
    | Divide
    | Minimum
    | Maximum
    | Power

/// What `top` ranks series by.
type RankBy =
    | ByMax
    | ByMean
    | ByMin
    | BySum
    | ByLast
    | ByL2norm
    | ByArea

type TimewiseFn =
    | Cumsum
    /// FIXME(integral): implemented as the docs define it, which is probably a
    /// docs error — see Evaluate.fs.
    | Integral
    | Diff
    | MonotonicDiff
    | Derivative
    | PerSecond
    | PerMinute
    | PerHour
    | Throughput
    | Ewma of span: int
    | Median of span: int
    | RollingAvg of span: int
    | Autosmooth
    | TrendLine
    | RobustTrend
    | PiecewiseConstant

/// Buckets of history a time-wise function needs before its first point.
///
/// WARNING(undocumented): how much history Datadog reads for these. Ours: one
/// point for diffs, a full window for median and rollingavg, 2 × span for
/// ewma (the start's weight is then under 2 %), none where the docs say the
/// function starts at the visible window.
let lookback (fn: TimewiseFn) : int =
    match fn with
    | Cumsum
    | Integral
    | Throughput
    | TrendLine
    | RobustTrend
    | PiecewiseConstant -> 0
    | Diff
    | MonotonicDiff
    | Derivative
    | PerSecond
    | PerMinute
    | PerHour -> 1
    | Median span
    | RollingAvg span -> span - 1
    | Ewma span -> 2 * span
    | Autosmooth -> 2 * 20

type OutlierAlgorithm =
    | Dbscan of scaled: bool
    | Mad of scaled: bool * percent: float

/// A formula with its names resolved to queries.
type Node =
    /// The series one query returned; the index is into Plan.Queries.
    | Fetch of source: int
    | Pointwise of PointwiseFn * Node
    | Top of Node * count: int * by: RankBy * order: SortOrder
    | CountNonzero of Node
    | CountNotNull of Node
    | ExcludeNull of Node
    | Timewise of TimewiseFn * Node
    /// The inner node over data `offsetMs` away (negative: the past), drawn at
    /// the present window's times.
    | Shift of offsetMs: int64 * Node
    | DefaultZero of Node
    | Outliers of Node * OutlierAlgorithm * tolerance: float
    | Constant of float
    /// `time()`: each point's own time, in seconds.
    | TimeOfPoint
    | Arith of ArithOp * Node * Node

type LinearModel =
    | DefaultModel
    | SimpleModel
    | ReactiveModel

/// A seasonal model's period, and the step it is fitted at.
///
/// WARNING(undocumented): the step Datadog fits seasonal models at. We use its
/// line-graph steps for the matching windows, so a season is 60, 288 or 168
/// points.
type Seasonality =
    | Hourly
    | Daily
    | Weekly

    member s.Step =
        match s with
        | Hourly -> TimeSpan.FromMinutes 1.0
        | Daily -> TimeSpan.FromMinutes 5.0
        | Weekly -> TimeSpan.FromHours 1.0

    member s.Length =
        match s with
        | Hourly -> TimeSpan.FromHours 1.0
        | Daily -> TimeSpan.FromDays 1.0
        | Weekly -> TimeSpan.FromDays 7.0

    /// Points per season.
    member s.Period = int (s.Length / s.Step)

/// The `metrics` table's TTL (schema/migrations/0001_initial.sql).
/// When retention becomes configurable, read it here.
let rawRetention = TimeSpan.FromDays 30.0

/// History a seasonal model reads back from `to`: six seasons, as Datadog's
/// "up to six weeks".
///
/// WARNING(undocumented): raw retention caps us at whole seasons within 30
/// days, so weekly gets four weeks, not six.
let seasonalHistory (s: Seasonality) : TimeSpan =
    let wanted = s.Length * 6.0
    let wholeSeasonsKept = rawRetention.Ticks / s.Length.Ticks
    let available = TimeSpan.FromTicks(wholeSeasonsKept * s.Length.Ticks)
    if wanted < available then wanted else available

type AnomalyAlgorithm =
    | Basic
    | Robust of Seasonality
    | Agile of Seasonality

type ForecastMethod =
    | LinearForecast of LinearModel
    | SeasonalForecast of Seasonality

/// What a formula returns beside its series. Only a formula's outermost
/// function can ask for one.
type Extra =
    | NoExtra
    | Anomalies of AnomalyAlgorithm * bounds: float
    | Forecast of deviations: float * method: ForecastMethod * history: TimeSpan option * horizon: TimeSpan option

type Output =
    { /// The formula's position, or the query's when there are no formulas.
      QueryIndex: int
      Node: Node
      Limit: ParsedLimit option
      Extra: Extra }

/// Buckets before each point that the basic anomaly band is computed from.
///
/// WARNING(undocumented): the size of Datadog's "lagging rolling quantile"
/// window. Ours is 60 buckets.
let anomalyWindow = 60

type Plan =
    { From: DateTimeOffset
      To: DateTimeOffset
      Step: TimeSpan
      Queries: QueryPlan list
      Outputs: Output list }

// --- small helpers ---------------------------------------------------------------

/// A word argument, quoted or not.
let private word (literal: Literal) : string option =
    match literal with
    | Word w
    | Quoted w -> Some w
    | Num _ -> None

let private errorsIn (results: Result<'a, string list> list) : string list =
    results
    |> List.collect (fun result ->
        match result with
        | Error errors -> errors
        | Ok _ -> [])

// --- the step --------------------------------------------------------------------

/// Datadog's default steps for a line graph: (window up to, step).
///
/// WARNING(undocumented): a window between two rows takes the coarser row.
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

/// Datadog's cap on points per series.
let maxPoints = 1500

/// A requested step, widened when the window would hold more than maxPoints.
///
/// WARNING(undocumented): how much Datadog widens by. We take the smallest
/// whole second that fits; buckets are whole seconds in ClickHouse.
let private fitted (window: TimeSpan) (requested: TimeSpan) : TimeSpan =
    let smallestAllowed = TimeSpan.FromTicks(window.Ticks / int64 maxPoints)
    let seconds = max requested.TotalSeconds smallestAllowed.TotalSeconds
    TimeSpan.FromSeconds(Math.Ceiling(max seconds 1.0))

let step (window: TimeSpan) (hint: TimeSpan option) : TimeSpan =
    match hint with
    | Some requested -> fitted window requested
    | None ->
        match defaultSteps |> List.tryFind (fun (upTo, _) -> window <= upTo) with
        | Some(_, step) -> step
        | None -> TimeSpan.FromDays 7.0

// --- queries ---------------------------------------------------------------------

let private spaceAggregation (name: string) : Aggregation option =
    match name with
    | "avg" -> Some Avg
    | "sum" -> Some Sum
    | "min" -> Some Min
    | "max" -> Some Max
    | _ -> None

let private rollupMethod (name: string) : Aggregation option =
    match name with
    | "count" -> Some Count
    | _ -> spaceAggregation name

/// What a query's modifiers asked for, before they are checked together.
type private Modifiers =
    { Method: Aggregation option
      Interval: TimeSpan option
      /// "as_count" or "as_rate".
      Mode: string option
      Fill: Fill option }

let private readRollup (sofar: Modifiers) (args: Literal list) : Result<Modifiers, string> =
    match args with
    | [] -> Error ".rollup() needs a method, e.g. .rollup(sum) or .rollup(sum, 60)"
    | method :: rest ->
        match word method |> Option.bind rollupMethod, rest with
        | None, _ -> Error ".rollup() method must be one of avg, sum, min, max, count"
        | Some method, [] -> Ok { sofar with Method = Some method }
        | Some method, [ Num seconds ] when seconds >= 1.0 ->
            Ok { sofar with Method = Some method; Interval = Some(TimeSpan.FromSeconds seconds) }
        | Some _, [ Num _ ] -> Error ".rollup() interval must be at least 1 second"
        | Some _, _ -> Error "calendar .rollup() (daily, weekly, monthly, alignment, timezone) is not supported yet"

let private readFill (sofar: Modifiers) (args: Literal list) : Result<Modifiers, string> =
    match args with
    | [] -> Error ".fill() needs a method, e.g. .fill(zero) or .fill(linear, 300)"
    | method :: rest ->
        let limit =
            match rest with
            | [] -> Ok 300
            | [ Num seconds ] when seconds >= 1.0 && seconds = Math.Floor seconds -> Ok(int seconds)
            | _ -> Error ".fill() limit must be a whole number of seconds, e.g. .fill(linear, 300)"

        let methodError = Error ".fill() method must be one of null, zero, last, linear"

        match word method, limit with
        | Some "null", _ -> Ok { sofar with Fill = Some NoFill }
        | Some "linear", Ok limit -> Ok { sofar with Fill = Some(FillWithin(Linear, limit)) }
        | Some "last", Ok limit -> Ok { sofar with Fill = Some(FillWithin(Last, limit)) }
        | Some "zero", Ok limit -> Ok { sofar with Fill = Some(FillWithin(Zero, limit)) }
        | Some("linear" | "last" | "zero"), Error e -> Error e
        | _ -> methodError

let private readModifier (sofar: Modifiers) (m: Modifier) : Result<Modifiers, string> =
    match m.Name with
    | "rollup" -> readRollup sofar m.Args
    | "fill" -> readFill sofar m.Args
    | "as_count"
    | "as_rate" ->
        match m.Args, sofar.Mode with
        | _ :: _, _ -> Error $".{m.Name}() takes no arguments"
        | [], Some other when other <> m.Name -> Error $".{m.Name}() cannot follow .{other}()"
        | [], _ -> Ok { sofar with Mode = Some m.Name }
    | name -> Error $".{name}() is not supported yet"

let private readModifiers (label: string) (modifiers: Modifier list) : Modifiers * string list =
    let mutable read = { Method = None; Interval = None; Mode = None; Fill = None }
    let errors = ResizeArray<string>()

    for m in modifiers do
        match readModifier read m with
        | Ok updated -> read <- updated
        | Error e -> errors.Add $"{label}: {e}"

    read, List.ofSeq errors

let private timeAggregation (label: string) (m: Modifiers) : Result<TimeAggregation, string> =
    match m.Mode, m.Method with
    | None, None -> Ok(Plain Avg)
    | None, Some method -> Ok(Plain method)
    // as_count and as_rate sum within the bucket; a rollup other than sum
    // contradicts them.
    | Some "as_count", (None | Some Sum) -> Ok AsCount
    | Some "as_rate", (None | Some Sum) -> Ok AsRate
    | Some mode, Some other -> Error $"{label}: .{mode}() with .rollup({(string other).ToLowerInvariant()}) is not supported yet"
    | Some mode, None -> Error $"{label}: unknown modifier .{mode}()"

/// `{$env}` left in a filter: a dashboard should have substituted it.
let rec private templateVariables (filter: TagFilter) : string list =
    match filter with
    | Bare v when v.StartsWith "$" -> [ v ]
    | Tag(_, v) when v.StartsWith "$" -> [ v ]
    | Not inner -> templateVariables inner
    | And filters
    | Or filters -> List.collect templateVariables filters
    | _ -> []

let private planQuery (window: TimeSpan) (requestStep: TimeSpan) (label: string) (q: MetricQuery) : Result<QueryPlan, string list> =
    let modifiers, modifierErrors = readModifiers label q.Modifiers
    let timeAgg = timeAggregation label modifiers

    let aggregatorErrors =
        match q.SpaceAgg with
        | Some a when (spaceAggregation a).IsNone -> [ $"{label}: aggregator '{a}:' is not supported yet" ]
        | _ -> []

    let timeAggErrors =
        match timeAgg with
        | Error e -> [ e ]
        | Ok _ -> []

    let templateErrors =
        templateVariables q.Filter
        |> List.map (fun v -> $"{label}: template variable '{v}' was not substituted")

    match aggregatorErrors @ modifierErrors @ timeAggErrors @ templateErrors, timeAgg with
    | [], Ok timeAgg ->
        // WARNING(undocumented): with no prefix, Datadog picks the space
        // aggregator by metric type. We use avg, its choice for a gauge.
        let spaceAgg =
            match q.SpaceAgg |> Option.bind spaceAggregation with
            | Some agg -> agg
            | None -> Avg

        let step =
            match modifiers.Interval with
            | Some interval -> fitted window interval
            | None -> requestStep

        // Linear up to 5 minutes by default; as_count/as_rate turn it off.
        // WARNING(undocumented): the docs keep it on for gauges there. The
        // metric type is known only per row, so we turn it off for all.
        let fill =
            match modifiers.Fill, timeAgg with
            | Some fill, _ -> fill
            | None, (AsCount | AsRate) -> NoFill
            | None, Plain _ -> FillWithin(Linear, 300)

        Ok
            { Metric = q.Metric
              Filter = q.Filter
              GroupBy = q.GroupBy
              SpaceAgg = spaceAgg
              TimeAgg = timeAgg
              Step = step
              Lookback = 0
              Fill = fill }
    | errors, _ -> Error errors

// --- function arguments ------------------------------------------------------------

let private pointwiseFunction (name: string) (args: Literal list) : Result<PointwiseFn, string> =
    let numbers = args |> List.choose (fun a -> match a with Num n -> Some n | _ -> None)

    if numbers.Length <> args.Length then
        Error $"{name}() takes only numbers after the query"
    else
        match name, numbers with
        | "abs", [] -> Ok Abs
        | "log2", [] -> Ok Log2
        | "log10", [] -> Ok Log10
        | "ceil", [] -> Ok Ceil
        | "floor", [] -> Ok Floor
        | ("abs" | "log2" | "log10" | "ceil" | "floor"), _ -> Error $"{name}() takes one argument: {name}(query)"
        | "round", [] -> Ok(Round 0)
        | "round", [ d ] when d >= 0.0 && d <= 15.0 && d = Math.Floor d -> Ok(Round(int d))
        | "round", _ -> Error "round() takes a query and optionally a whole number of decimals from 0 to 15: round(query, 2)"
        | "clamp_min", [ t ] -> Ok(ClampMin t)
        | "clamp_max", [ t ] -> Ok(ClampMax t)
        | "cutoff_min", [ t ] -> Ok(CutoffMin t)
        | "cutoff_max", [ t ] -> Ok(CutoffMax t)
        | _ -> Error $"{name}() takes a query and a number: {name}(query, 100)"

/// Only the numbered forms the docs list; `ewma` alone is ewma_20 and `median`
/// alone is median_3, as the docs say.
let private timewiseFunction (name: string) : TimewiseFn option =
    match name with
    | "cumsum" -> Some Cumsum
    | "integral" -> Some Integral
    | "diff" -> Some Diff
    | "monotonic_diff" -> Some MonotonicDiff
    | "derivative" -> Some Derivative
    | "per_second" -> Some PerSecond
    | "per_minute" -> Some PerMinute
    | "per_hour" -> Some PerHour
    | "throughput" -> Some Throughput
    | "ewma" -> Some(Ewma 20)
    | "ewma_1" -> Some(Ewma 1)
    | "ewma_3" -> Some(Ewma 3)
    | "ewma_5" -> Some(Ewma 5)
    | "ewma_7" -> Some(Ewma 7)
    | "ewma_10" -> Some(Ewma 10)
    | "ewma_20" -> Some(Ewma 20)
    | "median" -> Some(Median 3)
    | "median_3" -> Some(Median 3)
    | "median_5" -> Some(Median 5)
    | "median_7" -> Some(Median 7)
    | "median_9" -> Some(Median 9)
    | "rollingavg_5" -> Some(RollingAvg 5)
    | "rollingavg_13" -> Some(RollingAvg 13)
    | "rollingavg_21" -> Some(RollingAvg 21)
    | "rollingavg_29" -> Some(RollingAvg 29)
    | "autosmooth" -> Some Autosmooth
    | "trend_line" -> Some TrendLine
    | "robust_trend" -> Some RobustTrend
    | "piecewise_constant" -> Some PiecewiseConstant
    | _ -> None

let private rankBy (name: string) : RankBy option =
    match name with
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
/// WARNING(undocumented): top()'s defaults. We require all four arguments, as
/// Datadog's editor always writes them.
type private Ranking =
    { Limit: int
      By: RankBy
      Order: SortOrder }

let private topArgs (args: Literal list) : Result<Ranking, string> =
    match args with
    | [ Num n; by; direction ] ->
        let limit = int n

        if float limit <> n || not (List.contains limit [ 5; 10; 25; 50; 100 ]) then
            Error "top() limit must be one of 5, 10, 25, 50, 100"
        else
            match word by |> Option.bind rankBy, word direction with
            | None, _ -> Error "top() ranks by one of 'max', 'mean', 'min', 'sum', 'last', 'l2norm', 'area'"
            | Some by, Some "desc" -> Ok { Limit = limit; By = by; Order = Desc }
            | Some by, Some "asc" -> Ok { Limit = limit; By = by; Order = Asc }
            | Some _, _ -> Error "top() direction must be 'asc' or 'desc'"
    | _ -> Error "top() takes a query, a limit, a ranking and a direction: top(query, 10, 'mean', 'desc')"

/// `top5_mean`, `bottom10_min`…: the documented
/// [top|bottom][5|10|15|20]_[mean|min|max|last|area|l2norm].
///
/// WARNING(undocumented): bare `top10()`. Not accepted.
let private topShorthand (name: string) : Ranking option =
    let m = Regex.Match(name, "^(top|bottom)(5|10|15|20)_(mean|min|max|last|area|l2norm)$")

    if not m.Success then
        None
    else
        let order = if m.Groups[1].Value = "top" then Desc else Asc
        let count = int m.Groups[2].Value

        match rankBy m.Groups[3].Value with
        | Some by -> Some { Limit = count; By = by; Order = order }
        | None -> None

/// `outliers(q, 'dbscan', 3)` or `outliers(q, 'mad', 3, 20)`. Algorithm names
/// ignore case: Datadog's own examples write both 'DBSCAN' and 'dbscan'.
let private outlierArgs (args: Literal list) : Result<OutlierAlgorithm * float, string> =
    let usage = "outliers() takes a query, an algorithm, a tolerance and for MAD a percentage: outliers(query, 'dbscan', 3) or outliers(query, 'mad', 3, 20)"
    let algorithm (literal: Literal) = word literal |> Option.map (fun w -> w.ToLowerInvariant())

    match args with
    | [ alg; Num tolerance ] when tolerance > 0.0 ->
        match algorithm alg with
        | Some "dbscan" -> Ok(Dbscan false, tolerance)
        | Some "scaledbscan" -> Ok(Dbscan true, tolerance)
        | Some("mad" | "scaledmad") -> Error "outliers() with MAD needs a percentage: outliers(query, 'mad', 3, 20)"
        | _ -> Error usage
    | [ alg; Num tolerance; Num percent ] when tolerance > 0.0 && percent >= 0.0 && percent <= 100.0 ->
        match algorithm alg with
        | Some "mad" -> Ok(Mad(false, percent), tolerance)
        | Some "scaledmad" -> Ok(Mad(true, percent), tolerance)
        | _ -> Error usage
    | _ -> Error usage

/// `calendar_shift`'s "-1d", "-2w", "-1mo", as milliseconds from `from`.
/// The same wall-clock time n units back in that zone, so "-1d" across a DST
/// change is 23 or 25 hours.
let private calendarShift (spec: string) (timezone: string) (from: DateTimeOffset) : Result<int64, string> =
    let m = Regex.Match(spec, "^-(\\d+)(d|w|mo)$")

    let zone =
        try
            Ok(TimeZoneInfo.FindSystemTimeZoneById timezone)
        with _ ->
            Error $"calendar_shift() timezone '{timezone}' is not a known IANA zone"

    match m.Success, zone with
    | false, _ -> Error $"calendar_shift() shift must look like \"-1d\", \"-2w\" or \"-1mo\", got \"{spec}\""
    | true, Error e -> Error e
    | true, Ok zone ->
        let n = int m.Groups[1].Value
        let local = TimeZoneInfo.ConvertTime(from, zone).DateTime

        let shifted =
            match m.Groups[2].Value with
            | "d" -> local.AddDays(float -n)
            | "w" -> local.AddDays(float (-7 * n))
            | _ -> local.AddMonths(-n)

        let back = DateTimeOffset(shifted, zone.GetUtcOffset shifted)
        Ok(back.ToUnixTimeMilliseconds() - from.ToUnixTimeMilliseconds())

/// A shift function's offset in milliseconds.
///
/// WARNING(undocumented): a calendar shift is computed once, at `from`. Over a
/// window crossing a month end or a DST change, later points drift from their
/// exact calendar counterparts.
let private shiftOffset (from: DateTimeOffset) (name: string) (args: Literal list) : Result<int64, string> =
    match name, args with
    | "timeshift", [ Num seconds ] when seconds <> 0.0 && seconds = Math.Floor seconds -> Ok(int64 seconds * 1000L)
    | "timeshift", _ -> Error "timeshift() takes a query and a whole, non-zero number of seconds: timeshift(query, -3600)"
    | "hour_before", [] -> Ok -3_600_000L
    | "day_before", [] -> Ok -86_400_000L
    | "week_before", [] -> Ok -604_800_000L
    // Datadog deprecated month_before in favour of calendar_shift "-1mo".
    | "month_before", [] -> calendarShift "-1mo" "UTC" from
    | "calendar_shift", [ (Quoted spec | Word spec) ] -> calendarShift spec "UTC" from
    | "calendar_shift", [ (Quoted spec | Word spec); (Quoted zone | Word zone) ] -> calendarShift spec zone from
    | "calendar_shift", _ ->
        Error "calendar_shift() takes a query, a shift and optionally a timezone: calendar_shift(query, \"-1w\", \"Europe/Paris\")"
    | _ -> Error $"{name}() takes one argument: {name}(query)"

// --- formulas --------------------------------------------------------------------

/// An argument after the query, as a plain literal. The parser reads a bare
/// word in a formula (`mean`) as a query name; here it is a word again.
let private asLiteral (arg: Arg<string>) : Literal option =
    match arg with
    | Value(Number n) -> Some(Num n)
    | Value(Leaf w) -> Some(Word w)
    | Lit l -> Some l
    | Value _
    | Named _ -> None

/// A function applied to its already resolved first argument.
let private applyFunction (from: DateTimeOffset) (name: string) (args: Literal list) (inner: Node) : Result<Node, string> =
    let oneArgument = $"{name}() takes one argument: {name}(query)"

    match name with
    | "abs"
    | "log2"
    | "log10"
    | "ceil"
    | "floor"
    | "round"
    | "clamp_min"
    | "clamp_max"
    | "cutoff_min"
    | "cutoff_max" ->
        match pointwiseFunction name args with
        | Ok fn -> Ok(Pointwise(fn, inner))
        | Error e -> Error e
    | "top" ->
        match topArgs args with
        | Ok ranking -> Ok(Top(inner, ranking.Limit, ranking.By, ranking.Order))
        | Error e -> Error e
    | "outliers" ->
        match outlierArgs args with
        | Ok(algorithm, tolerance) -> Ok(Outliers(inner, algorithm, tolerance))
        | Error e -> Error e
    | "timeshift"
    | "hour_before"
    | "day_before"
    | "week_before"
    | "month_before"
    | "calendar_shift" ->
        match shiftOffset from name args with
        | Ok offset -> Ok(Shift(offset, inner))
        | Error e -> Error e
    | "count_nonzero"
    | "count_nonzero_finite" -> if args.IsEmpty then Ok(CountNonzero inner) else Error oneArgument
    | "count_not_null" -> if args.IsEmpty then Ok(CountNotNull inner) else Error oneArgument
    | "exclude_null" -> if args.IsEmpty then Ok(ExcludeNull inner) else Error oneArgument
    | "default_zero" -> if args.IsEmpty then Ok(DefaultZero inner) else Error oneArgument
    | "anomalies"
    | "forecast" -> Error $"{name}() must be the outermost function of a formula"
    | _ ->
        match timewiseFunction name, topShorthand name with
        | Some fn, _ -> if args.IsEmpty then Ok(Timewise(fn, inner)) else Error oneArgument
        | None, Some ranking -> if args.IsEmpty then Ok(Top(inner, ranking.Limit, ranking.By, ranking.Order)) else Error oneArgument
        | None, None -> Error $"function '{name}' is not supported yet"

let private arith (op: ArithOp) (left: Result<Node, string list>) (right: Result<Node, string list>) : Result<Node, string list> =
    match left, right with
    | Ok l, Ok r -> Ok(Arith(op, l, r))
    | Error l, Error r -> Error(l @ r)
    | Error e, Ok _
    | Ok _, Error e -> Error e

let rec private resolve (from: DateTimeOffset) (label: string) (byName: Map<string, int>) (expr: Formula) : Result<Node, string list> =
    let resolveInner = resolve from label byName

    match expr with
    | Leaf name ->
        match byName.TryFind name with
        | Some index -> Ok(Fetch index)
        | None -> Error [ $"{label}: no query is named '{name}'" ]
    | Number n -> Ok(Constant n)
    | Neg inner -> arith Times (Ok(Constant -1.0)) (resolveInner inner)
    | Binary(Add, l, r) -> arith Plus (resolveInner l) (resolveInner r)
    | Binary(Sub, l, r) -> arith Minus (resolveInner l) (resolveInner r)
    | Binary(Mul, l, r) -> arith Times (resolveInner l) (resolveInner r)
    | Binary(Div, l, r) -> arith Divide (resolveInner l) (resolveInner r)
    | Call("minimum", [ Value l; Value r ]) -> arith Minimum (resolveInner l) (resolveInner r)
    | Call("maximum", [ Value l; Value r ]) -> arith Maximum (resolveInner l) (resolveInner r)
    | Call("pow", [ Value l; Value r ]) -> arith Power (resolveInner l) (resolveInner r)
    | Call(("minimum" | "maximum" | "pow") as name, _) -> Error [ $"{label}: {name}() takes two operands: {name}(a, b)" ]
    | Call("time", []) -> Ok TimeOfPoint
    | Call("time", _) -> Error [ $"{label}: time() takes no arguments" ]
    | Call(name, []) -> Error [ $"{label}: {name}() needs a query to act on" ]
    | Call(name, Value inner :: rest) ->
        let literals = rest |> List.map asLiteral

        if literals |> List.exists Option.isNone then
            Error [ $"{label}: {name}() takes only numbers and words after the query" ]
        else
            match resolveInner inner with
            | Error errors -> Error errors
            | Ok innerNode ->
                match applyFunction from name (List.choose id literals) innerNode with
                | Ok node -> Ok node
                | Error e -> Error [ $"{label}: {e}" ]
    | Call(name, _) -> Error [ $"{label}: the first argument of {name}() must be a query" ]

// --- anomalies and forecast --------------------------------------------------------

/// `4h`, `30m`, `1w`: a whole number and s, m, h, d or w.
let private duration (text: string) : TimeSpan option =
    let m = Regex.Match(text, "^(\\d+)(s|m|h|d|w)$")

    if not m.Success then
        None
    else
        let n = float m.Groups[1].Value

        let span =
            match m.Groups[2].Value with
            | "s" -> TimeSpan.FromSeconds n
            | "m" -> TimeSpan.FromMinutes n
            | "h" -> TimeSpan.FromHours n
            | "d" -> TimeSpan.FromDays n
            | _ -> TimeSpan.FromDays(7.0 * n)

        if span > TimeSpan.Zero then Some span else None

let private namedArg (name: string) (named: (string * Literal) list) : Literal option =
    named |> List.tryFind (fun (n, _) -> n = name) |> Option.map snd

/// Positional arguments after the query, and the `name=value` ones.
let private splitArgs (rest: Arg<string> list) : Literal option list * (string * Literal) list =
    let positional =
        rest
        |> List.choose (fun arg ->
            match arg with
            | Named _ -> None
            | other -> Some(asLiteral other))

    let named =
        rest
        |> List.choose (fun arg ->
            match arg with
            | Named(n, v) -> Some(n, v)
            | _ -> None)

    positional, named

/// "By default, the robust and agile algorithms use weekly seasonality."
///
/// WARNING(undocumented): forecast()'s default seasonality. Weekly, as for
/// anomalies().
/// WARNING(undocumented): `timezone` is accepted and ignored; our seasons are
/// fixed-length, so across DST they are off by an hour.
let private seasonality (named: (string * Literal) list) : Result<Seasonality, string> =
    let invalid (got: string) = Error $"seasonality must be 'hourly', 'daily' or 'weekly', got '{got}'"

    match namedArg "seasonality" named with
    | None -> Ok Weekly
    | Some(Num n) -> invalid (string n)
    | Some(Word s | Quoted s) ->
        match s with
        | "hourly" -> Ok Hourly
        | "daily" -> Ok Daily
        | "weekly" -> Ok Weekly
        | other -> invalid other

/// anomalies(q, 'basic' | 'agile' | 'robust', bounds, name=value…)
///
/// WARNING(undocumented): `direction`, `alert_window`, `interval` and
/// `count_default_zero` are for monitors. Accepted here and ignored: a graph
/// draws the whole band.
let private anomaliesArgs (rest: Arg<string> list) : Result<Extra, string> =
    let allowed = [ "direction"; "alert_window"; "interval"; "count_default_zero"; "seasonality"; "timezone" ]
    let positional, named = splitArgs rest

    match named |> List.tryFind (fun (n, _) -> not (List.contains n allowed)) with
    | Some(n, _) -> Error $"anomalies() has no argument '{n}'"
    | None ->
        match positional with
        | [ Some alg; Some(Num bounds) ] when bounds > 0.0 ->
            match word alg with
            | Some "basic" -> Ok(Anomalies(Basic, bounds))
            | Some "robust" ->
                match seasonality named with
                | Ok s -> Ok(Anomalies(Robust s, bounds))
                | Error e -> Error e
            | Some "agile" ->
                match seasonality named with
                | Ok s -> Ok(Anomalies(Agile s, bounds))
                | Error e -> Error e
            | _ -> Error "anomalies() algorithm must be 'basic', 'agile' or 'robust'"
        | _ -> Error "anomalies() takes a query, an algorithm and bounds: anomalies(query, 'basic', 2)"

let private linearModel (named: (string * Literal) list) : Result<LinearModel, string> =
    match namedArg "model" named |> Option.bind word with
    | None
    | Some "default" -> Ok DefaultModel
    | Some "simple" -> Ok SimpleModel
    | Some "reactive" -> Ok ReactiveModel
    | Some m -> Error $"forecast() model must be 'default', 'simple' or 'reactive', got '{m}'"

let private durationArg (name: string) (named: (string * Literal) list) : Result<TimeSpan option, string> =
    match namedArg name named with
    | None -> Ok None
    | Some value ->
        match word value |> Option.bind duration with
        | Some d -> Ok(Some d)
        | None -> Error $"forecast() {name} must be a duration like '4h', '3d' or '1w'"

/// forecast(q, 'linear' | 'seasonal', deviations, model=, history=, horizon=,
/// seasonality=, timezone=)
///
/// `horizon` is ours: Datadog takes it from the monitor (`next_1w`) and does
/// not document one for graphs. `interval` is accepted and ignored.
let private forecastArgs (rest: Arg<string> list) : Result<Extra, string> =
    let allowed = [ "model"; "history"; "horizon"; "seasonality"; "timezone"; "interval" ]
    let positional, named = splitArgs rest
    let unknown = named |> List.tryFind (fun (n, _) -> not (List.contains n allowed))

    match unknown, linearModel named, durationArg "history" named, durationArg "horizon" named with
    | Some(n, _), _, _, _ -> Error $"forecast() has no argument '{n}'"
    | None, Error e, _, _
    | None, _, Error e, _
    | None, _, _, Error e -> Error e
    | None, Ok model, Ok history, Ok horizon ->
        match positional with
        | [ Some alg; Some(Num deviations) ] when deviations >= 0.0 ->
            match word alg with
            | Some "linear" -> Ok(Forecast(deviations, LinearForecast model, history, horizon))
            | Some "seasonal" ->
                match seasonality named with
                | Ok s -> Ok(Forecast(deviations, SeasonalForecast s, history, horizon))
                | Error e -> Error e
            | _ -> Error "forecast() algorithm must be 'linear' or 'seasonal'"
        | _ -> Error "forecast() takes a query, an algorithm and deviations: forecast(query, 'linear', 1)"

/// A formula's outermost function may be anomalies() or forecast(); below it
/// everything is an ordinary Node.
let private resolveOutput (from: DateTimeOffset) (label: string) (byName: Map<string, int>) (expr: Formula) : Result<Node * Extra, string list> =
    match expr with
    | Call(("anomalies" | "forecast") as name, Value inner :: rest) ->
        let extra = if name = "anomalies" then anomaliesArgs rest else forecastArgs rest

        match resolve from label byName inner, extra with
        | Ok node, Ok extra -> Ok(node, extra)
        | Error errors, Ok _ -> Error errors
        | Ok _, Error e -> Error [ $"{label}: {e}" ]
        | Error errors, Error e -> Error(errors @ [ $"{label}: {e}" ])
    | Call(("anomalies" | "forecast"), _) -> Error [ $"{label}: the first argument must be a query" ]
    | _ ->
        match resolve from label byName expr with
        | Ok node -> Ok(node, NoExtra)
        | Error errors -> Error errors

// --- grouping ----------------------------------------------------------------------

/// The `by` keys of a node's series. Numbers and time() have no series at all.
type private Grouping =
    | NoSeries
    | GroupedBy of Set<string>

/// Arithmetic needs both sides grouped by the same tags ("all queries must be
/// grouped by the same", docs: dashboards/querying). An ungrouped side, a
/// single series or a number, applies to every series of the other.
let rec private grouping (req: ParsedTimeseriesRequest) (node: Node) : Result<Grouping, string> =
    let needsSeries (inner: Node) =
        match grouping req inner with
        | Ok NoSeries -> Error "a function needs a query inside it, not only numbers"
        | other -> other

    match node with
    | Fetch i -> Ok(GroupedBy(set req.Queries[i].Query.GroupBy))
    | Constant _
    | TimeOfPoint -> Ok NoSeries
    | CountNonzero inner
    | CountNotNull inner ->
        // One series out, whatever went in.
        match needsSeries inner with
        | Ok _ -> Ok(GroupedBy Set.empty)
        | Error e -> Error e
    | Pointwise(_, inner)
    | Top(inner, _, _, _)
    | ExcludeNull inner
    | Timewise(_, inner)
    | Shift(_, inner)
    | DefaultZero inner
    | Outliers(inner, _, _) -> needsSeries inner
    | Arith(_, l, r) ->
        match grouping req l, grouping req r with
        | Error e, _
        | _, Error e -> Error e
        | Ok NoSeries, Ok other
        | Ok other, Ok NoSeries -> Ok other
        | Ok(GroupedBy a), Ok(GroupedBy b) when a.IsEmpty -> Ok(GroupedBy b)
        | Ok(GroupedBy a), Ok(GroupedBy b) when b.IsEmpty || a = b -> Ok(GroupedBy a)
        | Ok(GroupedBy a), Ok(GroupedBy b) ->
            let show (keys: Set<string>) = keys |> Seq.map (fun k -> $"'{k}'") |> String.concat ", "
            Error $"arithmetic needs both sides grouped by the same tags, got {show a} and {show b}"

let private planOutput (req: ParsedTimeseriesRequest) (byName: Map<string, int>) (index: int) (formula: ParsedFormula) : Result<Output, string list> =
    let label = $"formulas[{index}]"

    match resolveOutput req.From label byName formula.Formula with
    | Error errors -> Error errors
    | Ok(node, extra) ->
        match grouping req node with
        | Error e -> Error [ $"{label}: {e}" ]
        | Ok NoSeries -> Error [ $"{label}: a formula needs at least one query, not only numbers" ]
        | Ok(GroupedBy _) ->
            Ok
                { QueryIndex = index
                  Node = node
                  Limit = formula.Limit
                  Extra = extra }

let private planOutputs (req: ParsedTimeseriesRequest) : Result<Output list, string list> =
    match req.Formulas with
    | [] ->
        // No formulas: each query is shown as it is.
        Ok(
            req.Queries
            |> List.mapi (fun i _ ->
                { QueryIndex = i
                  Node = Fetch i
                  Limit = None
                  Extra = NoExtra })
        )
    | formulas ->
        let byName =
            req.Queries
            |> List.indexed
            |> List.choose (fun (i, q) ->
                match q.Name with
                | Some name -> Some(name, i)
                | None -> None)
            |> Map.ofList

        let outputs = formulas |> List.mapi (planOutput req byName)

        match errorsIn outputs with
        | [] -> Ok(outputs |> List.choose Result.toOption)
        | errors -> Error errors

// --- lookback --------------------------------------------------------------------

/// Buckets before the window that `node` reads from query `source`, or None
/// when the node does not read that query.
let rec private historyFrom (source: int) (node: Node) : int option =
    match node with
    | Fetch i -> if i = source then Some 0 else None
    | Timewise(fn, inner) -> historyFrom source inner |> Option.map (fun n -> n + lookback fn)
    | Pointwise(_, inner)
    | Top(inner, _, _, _)
    | CountNonzero inner
    | CountNotNull inner
    | ExcludeNull inner
    | Shift(_, inner)
    | DefaultZero inner
    | Outliers(inner, _, _) -> historyFrom source inner
    | Arith(_, l, r) ->
        match historyFrom source l, historyFrom source r with
        | Some a, Some b -> Some(max a b)
        | Some a, None
        | None, Some a -> Some a
        | None, None -> None
    | Constant _
    | TimeOfPoint -> None

/// The first query a node reads, whose step an extra's history is counted in.
let rec private firstSource (node: Node) : int option =
    match node with
    | Fetch i -> Some i
    | Pointwise(_, inner)
    | Top(inner, _, _, _)
    | CountNonzero inner
    | CountNotNull inner
    | ExcludeNull inner
    | Timewise(_, inner)
    | Shift(_, inner)
    | DefaultZero inner
    | Outliers(inner, _, _) -> firstSource inner
    | Arith(_, l, r) ->
        match firstSource l with
        | Some i -> Some i
        | None -> firstSource r
    | Constant _
    | TimeOfPoint -> None

/// Buckets before the window that an output's extra reads.
let private extraHistory (window: TimeSpan) (step: TimeSpan) (extra: Extra) : int =
    match extra with
    | NoExtra -> 0
    // Seasonal models fetch their own history at their own step; the band
    // still needs the lagging residuals before the window.
    | Anomalies _ -> anomalyWindow
    | Forecast(_, SeasonalForecast _, _, _) -> 0
    | Forecast(_, LinearForecast _, history, _) ->
        let beyondWindow = (defaultArg history window) - window
        if beyondWindow <= TimeSpan.Zero then 0 else int (Math.Ceiling(beyondWindow / step))

let private queryLookback (window: TimeSpan) (queries: QueryPlan list) (outputs: Output list) (source: int) : int =
    let needed =
        outputs
        |> List.choose (fun output ->
            match historyFrom source output.Node with
            | None -> None
            | Some buckets ->
                let step =
                    match firstSource output.Node with
                    | Some i -> queries[i].Step
                    | None -> queries[source].Step

                Some(buckets + extraHistory window step output.Extra))

    if needed.IsEmpty then 0 else List.max needed

// --- the plan --------------------------------------------------------------------

let plan (req: ParsedTimeseriesRequest) : Result<Plan, string list> =
    let window = req.To - req.From
    let requestStep = step window req.Interval

    let duplicateNames =
        req.Queries
        |> List.choose _.Name
        |> List.countBy id
        |> List.filter (fun (_, count) -> count > 1)
        |> List.map (fun (name, _) -> $"query name '{name}' is used more than once")

    let queries =
        req.Queries
        |> List.mapi (fun i q ->
            let label =
                match q.Name with
                | Some name -> $"queries[{i}] ({name})"
                | None -> $"queries[{i}]"

            planQuery window requestStep label q.Query)

    let outputs = planOutputs req

    let outputErrors =
        match outputs with
        | Error errors -> errors
        | Ok _ -> []

    match duplicateNames @ errorsIn queries @ outputErrors, outputs with
    | [], Ok outputs ->
        let queries = queries |> List.choose Result.toOption

        Ok
            { From = req.From
              To = req.To
              Step = requestStep
              Queries = queries |> List.mapi (fun i q -> { q with Lookback = queryLookback window queries outputs i })
              Outputs = outputs }
    | errors, _ -> Error errors
