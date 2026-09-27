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
///     [.fill(null|zero|last|linear[, seconds])]
///   formulas: query names, wrapped in functions that act on each value
///   (abs, log2, log10, ceil, floor, round, clamp_*, cutoff_*) or across
///   series (top, top10_mean-style shorthands, count_nonzero, count_not_null,
///   exclude_null) or along time (cumsum, integral, diff, monotonic_diff,
///   derivative, per_second/minute/hour, throughput, ewma_*, median_*,
///   rollingavg_*, autosmooth, trend_line, robust_trend, piecewise_constant),
///   picked out by outliers(dbscan|scaledbscan|mad|scaledmad), combined by
///   + - * /, minimum, maximum, pow, time() and numbers, shifted in time (timeshift, hour/day/week/month_before,
///   calendar_shift), with an optional limit
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

/// How gaps in one series are filled before series are combined
/// (docs: metrics/guide/interpolation-the-fill-modifier-explained).
type FillMethod =
    /// "linear interpolation up to X seconds after real samples"
    | Linear
    /// "Replicates the last sample value up to X secs"
    | Last
    /// "Inserts 0 where the interpolation is needed up to X secs"
    | Zero

type Fill =
    /// `.fill(null)`, and the default under `.as_count()` / `.as_rate()`.
    | NoFill
    | FillWithin of FillMethod * limitSeconds: int

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
      Lookback: int
      /// Interpolation before space aggregation. Anything but NoFill makes
      /// ClickHouse return series one by one, and F# combine them.
      Fill: Fill }

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

/// What arithmetic combines two operands with (docs: dashboards/querying):
/// "+, -, /, *, minimum() and maximum()", and pow() (docs: arithmetic).
type ArithOp =
    | Plus
    | Minus
    | Times
    | Divide
    | Minimum
    | Maximum
    | Power

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
    /// EWMA with a span picked per series (see Evaluate.fs).
    | Autosmooth
    /// "Fit an ordinary least squares regression line through the metric
    /// values" — over the visible window.
    | TrendLine
    /// "Fit a robust regression trend line using Huber loss."
    | RobustTrend
    /// "Approximate the metric with a piecewise function composed of
    /// constant-valued segments."
    | PiecewiseConstant

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
    | Throughput
    | TrendLine
    | RobustTrend
    | PiecewiseConstant -> 0
    | Autosmooth -> 2 * 20
    | Diff
    | MonotonicDiff
    | Derivative
    | PerSecond
    | PerMinute
    | PerHour -> 1
    | Median n
    | RollingAvg n -> n - 1
    | Ewma n -> 2 * n

/// How outliers() compares series (docs: monitors/types/outlier).
type OutlierAlgorithm =
    /// Clusters series around the median series; outside the largest
    /// cluster is an outlier.
    | Dbscan of scaled: bool
    /// Per time, distance from the median in MADs; a series with more than
    /// `percent` of its points that far is an outlier.
    | Mad of scaled: bool * percent: float

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
    /// The inner node over data from `offsetMs` away (negative: the past),
    /// drawn at the present window's times. Shifts nest by adding up.
    | Shift of offsetMs: int64 * Node
    /// Every bucket of the window that a series has no value in becomes 0.
    | DefaultZero of Node
    /// Keep only the series outliers() finds.
    | Outliers of Node * OutlierAlgorithm * tolerance: float
    /// A number in a formula: `* 100`.
    | Constant of float
    /// `time()`: each point's own time, in seconds.
    | TimeOfPoint
    /// Two operands combined point by point, their series matched by group.
    | Arith of ArithOp * Node * Node

/// forecast()'s `model=` for the linear algorithm (docs: monitors/types/forecasts).
type LinearModel =
    /// "Adjusts to the most recent trend and extrapolates data while being
    /// robust to recent noise."
    | DefaultModel
    /// "Does a robust linear regression through the entire history."
    | SimpleModel
    /// "Extrapolates recent behavior better at the risk of overfitting to
    /// noise, spikes, or dips."
    | ReactiveModel

/// How an anomaly band finds its expected value (docs: monitors/types/anomaly).
/// A seasonal model's period, and the step it is fitted at.
///
/// WARNING(undocumented): the resolution Datadog fits seasonal models at. We
/// use its own default line-graph steps for the matching windows — an hour at
/// 1 min, a day at 5 min, a week at 1 h — so a season is 60, 288 or 168
/// points: enough to shape the season, few enough to fit fast.
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

    /// Points per season at the model's step.
    member s.Period = int (s.Length / s.Step)

/// How long raw points are kept: the `metrics` table's TTL.
///
/// Seasonal history cannot reach further back than this. It mirrors the TTL in
/// server/schema/migrations/0001_initial.sql; when retention becomes
/// configurable, this is the one place to read it from.
let rawRetention = TimeSpan.FromDays 30.0

/// History a seasonal model reads, back from `to`: "up to six weeks" (six
/// seasons), as much of it as raw retention holds.
///
/// WARNING(undocumented): Datadog reads six weeks; raw retention caps us at
/// 30 days, so weekly seasonality gets four weeks at most. Longer history
/// needs metrics_1m, which has no tags yet.
let seasonalHistory (s: Seasonality) =
    let wanted = s.Length * 6.0
    let available = TimeSpan.FromTicks((rawRetention.Ticks / s.Length.Ticks) * s.Length.Ticks)
    if wanted < available then wanted else available

type AnomalyAlgorithm =
    /// "A simple lagging rolling quantile computation."
    | Basic
    /// "A seasonal-trend decomposition algorithm, it is stable and
    /// predictions remain constant even through long-lasting anomalies."
    | Robust of Seasonality
    /// "A robust version of the SARIMA algorithm, it incorporates the
    /// immediate past into its predictions" and "quickly adjusts to metric
    /// level shifts."
    | Agile of Seasonality

/// How forecast() predicts.
type ForecastMethod =
    | LinearForecast of LinearModel
    /// The airline SARIMA on the seasonal history.
    | SeasonalForecast of Seasonality

/// What a formula adds beside its series, outside `values`. Only the
/// outermost function of a formula can ask for it.
type Extra =
    | NoExtra
    /// anomalies(q, algorithm, bounds): a band of expected values.
    | Anomalies of AnomalyAlgorithm * bounds: float
    /// forecast(q, algorithm, deviations): predicted values past the window,
    /// with a band. Horizon defaults to the window's length; linear history
    /// too.
    | Forecast of deviations: float * method: ForecastMethod * history: TimeSpan option * horizon: TimeSpan option

type Output =
    {
        /// What the response calls `query_index`: the formula's position, or
        /// the query's when there are no formulas.
        QueryIndex: int
        Node: Node
        Limit: ParsedLimit option
        Extra: Extra
    }

/// Buckets of history before each point that the basic anomaly band is
/// computed from.
///
/// WARNING(undocumented): Datadog's "lagging rolling quantile" window. It is 60
/// buckets: at the default steps, from a minute (1 s buckets) to a few hours.
let anomalyWindow = 60

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
      Mode: string option
      Fill: Fill option }

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
        | "fill", method :: rest ->
            let methodName =
                match method with
                | Word w
                | Quoted w -> Some w
                | Num _ -> None

            let limit =
                match rest with
                | [] -> Ok 300
                | [ Num s ] when s >= 1.0 && s = Math.Floor s -> Ok(int s)
                | _ -> Error ".fill() limit must be a whole number of seconds, e.g. .fill(linear, 300)"

            match methodName, limit with
            | Some "null", _ -> { acc with Fill = Some NoFill }, errors
            | Some m, Ok limit when List.contains m [ "linear"; "last"; "zero" ] ->
                let method = match m with "linear" -> Linear | "last" -> Last | _ -> Zero
                { acc with Fill = Some(FillWithin(method, limit)) }, errors
            | Some _, Error e -> fail e
            | _ -> fail ".fill() method must be one of null, zero, last, linear"
        | "fill", [] -> fail ".fill() needs a method, e.g. .fill(zero) or .fill(linear, 300)"
        | name, _ -> fail $".{name}() is not supported yet"

    mods |> List.fold apply ({ Method = None; Interval = None; Mode = None; Fill = None }, [])

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
              Lookback = 0
              // "The default interpolation for all metric types is linear
              // and performed up to five minutes after real samples";
              // as_count() and as_rate() disable it.
              //
              // WARNING(undocumented): the docs say as_count/as_rate disable
              // interpolation "except for Gauge types". The type is known
              // only per series, in the data, so under those modifiers no
              // series is interpolated, gauges included.
              Fill =
                match mods.Fill, timeAgg with
                | Some fill, _ -> fill
                | None, (AsCount | AsRate) -> NoFill
                | None, Plain _ -> FillWithin(Linear, 300) }
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
              $"rollingavg_{n}", RollingAvg n
          "autosmooth", Autosmooth
          "trend_line", TrendLine
          "robust_trend", RobustTrend
          "piecewise_constant", PiecewiseConstant ]

/// `outliers(q, 'dbscan', 3)` or `outliers(q, 'mad', 3, 20)`. Algorithm names
/// are matched case-insensitively: Datadog's own examples write both 'DBSCAN'
/// and 'dbscan'.
let private outlierArgs (literals: Literal list) : Result<OutlierAlgorithm * float, string> =
    let usage = "outliers() takes a query, an algorithm, a tolerance and for MAD a percentage: outliers(query, 'dbscan', 3) or outliers(query, 'mad', 3, 20)"

    let name =
        function
        | Word w
        | Quoted w -> Some(w.ToLowerInvariant())
        | Num _ -> None

    match literals with
    | [ alg; Num tol ] when tol > 0.0 ->
        match name alg with
        | Some "dbscan" -> Ok(Dbscan false, tol)
        | Some "scaledbscan" -> Ok(Dbscan true, tol)
        | Some("mad" | "scaledmad") -> Error "outliers() with MAD needs a percentage: outliers(query, 'mad', 3, 20)"
        | _ -> Error usage
    | [ alg; Num tol; Num pct ] when tol > 0.0 && pct >= 0.0 && pct <= 100.0 ->
        match name alg with
        | Some "mad" -> Ok(Mad(false, pct), tol)
        | Some "scaledmad" -> Ok(Mad(true, pct), tol)
        | _ -> Error usage
    | _ -> Error usage

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

/// Joins two resolved operands, collecting both sides' errors.
let private combine2 (label: string) (op: ArithOp) (l: Result<Node, string list>) (r: Result<Node, string list>) =
    match l, r with
    | Ok l, Ok r -> Ok(Arith(op, l, r))
    | l, r -> Error((match l with Error es -> es | Ok _ -> []) @ (match r with Error es -> es | Ok _ -> []))

/// A formula's expression tree → Node, with every problem in it named.
/// `calendar_shift`'s "-1d", "-2w", "-1mo": a negative whole number and a unit.
let private calendarShift (spec: string) (timezone: string) (from: DateTimeOffset) : Result<int64, string> =
    let m = Text.RegularExpressions.Regex.Match(spec, "^-(\\d+)(d|w|mo)$")

    let zone =
        try
            Ok(TimeZoneInfo.FindSystemTimeZoneById timezone)
        with _ ->
            Error $"calendar_shift() timezone '{timezone}' is not a known IANA zone"

    match m.Success, zone with
    | false, _ -> Error $"calendar_shift() shift must look like \"-1d\", \"-2w\" or \"-1mo\", got \"{spec}\""
    | _, Error e -> Error e
    | true, Ok zone ->
        let n = int m.Groups[1].Value
        let local = TimeZoneInfo.ConvertTime(from, zone)

        let shifted =
            match m.Groups[2].Value with
            | "d" -> local.DateTime.AddDays(float -n)
            | "w" -> local.DateTime.AddDays(float (-7 * n))
            | _ -> local.DateTime.AddMonths(-n)

        // The same wall-clock time, n units back, in that zone — so "-1d"
        // across a DST change is 23 or 25 hours, as a calendar day is.
        let back = DateTimeOffset(shifted, zone.GetUtcOffset shifted)
        Ok(back.ToUnixTimeMilliseconds() - from.ToUnixTimeMilliseconds())

/// The shift functions, as a fixed offset from the request's `from`.
///
/// WARNING(undocumented): how a calendar shift maps points inside the
/// window. The offset is computed once, at `from`, and applied to every point:
/// over a window spanning a month end or a DST change, points drift from their
/// exact calendar counterparts by the difference.
let private shiftOffset (from: DateTimeOffset) (name: string) (literals: Literal list) : Result<int64, string> option =
    let fixedShift seconds = Some(Ok(int64 seconds * 1000L))

    match name, literals with
    | "timeshift", [ Num seconds ] when seconds <> 0.0 && seconds = Math.Floor seconds -> Some(Ok(int64 seconds * 1000L))
    | "timeshift", _ -> Some(Error "timeshift() takes a query and a whole, non-zero number of seconds: timeshift(query, -3600)")
    | "hour_before", [] -> fixedShift -3600
    | "day_before", [] -> fixedShift -86400
    | "week_before", [] -> fixedShift -604800
    // Deprecated by Datadog in favour of calendar_shift "-1mo".
    | "month_before", [] -> Some(calendarShift "-1mo" "UTC" from)
    | ("hour_before" | "day_before" | "week_before" | "month_before"), _ -> Some(Error $"{name}() takes one argument: {name}(query)")
    | "calendar_shift", [ (Quoted spec | Word spec) ] -> Some(calendarShift spec "UTC" from)
    | "calendar_shift", [ (Quoted spec | Word spec); (Quoted zone | Word zone) ] -> Some(calendarShift spec zone from)
    | "calendar_shift", _ -> Some(Error "calendar_shift() takes a query, a shift and optionally a timezone: calendar_shift(query, \"-1w\", \"Europe/Paris\")")
    | _ -> None

let rec private resolve (from: DateTimeOffset) (label: string) (byName: Map<string, int>) (expr: Formula) : Result<Node, string list> =
    match expr with
    | Leaf name ->
        match byName.TryFind name with
        | Some i -> Ok(Fetch i)
        | None -> Error [ $"{label}: no query is named '{name}'" ]
    | Number n -> Ok(Constant n)
    | Neg inner -> resolve from label byName inner |> Result.map (fun n -> Arith(Times, Constant -1.0, n))
    | Binary(op, l, r) ->
        let op =
            match op with
            | Add -> Plus
            | Sub -> Minus
            | Mul -> Times
            | Div -> Divide

        combine2 label op (resolve from label byName l) (resolve from label byName r)
    | Call(("minimum" | "maximum" | "pow") as name, args) ->
        match args with
        | [ Value l; Value r ] ->
            let op = match name with "minimum" -> Minimum | "maximum" -> Maximum | _ -> Power
            combine2 label op (resolve from label byName l) (resolve from label byName r)
        | _ -> Error [ $"{label}: {name}() takes two operands: {name}(a, b)" ]
    | Call("time", []) -> Ok TimeOfPoint
    | Call("time", _) -> Error [ $"{label}: time() takes no arguments" ]
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
                    | "default_zero", [] -> Ok DefaultZero
                    | "outliers", _ -> outlierArgs literals |> Result.map (fun (alg, tol) node -> Outliers(node, alg, tol))
                    | ("anomalies" | "forecast"), _ -> Error $"{name}() must be the outermost function of a formula"
                    | "default_zero", _ -> Error "default_zero() takes one argument: default_zero(query)"
                    | _, [] when timewiseFunctions.ContainsKey name ->
                        let fn = timewiseFunctions[name]
                        Ok(fun node -> Timewise(fn, node))
                    | _, _ when timewiseFunctions.ContainsKey name -> Error $"{name}() takes one argument: {name}(query)"
                    | _ when (shiftOffset from name literals).IsSome ->
                        (shiftOffset from name literals).Value |> Result.map (fun offset node -> Shift(offset, node))
                    | ("count_nonzero" | "count_nonzero_finite" | "count_not_null" | "exclude_null"), _ ->
                        Error $"{name}() takes one argument: {name}(query)"
                    | _ ->
                        match topShorthand name, literals with
                        | Some(n, by, dir), [] -> Ok(fun node -> Top(node, n, by, dir))
                        | Some _, _ -> Error $"{name}() takes one argument: {name}(query)"
                        | None, _ -> Error $"function '{name}' is not supported yet"

            match resolve from label byName inner, build with
            | Ok inner, Ok build -> Ok(build inner)
            | inner, build ->
                Error(
                    (match inner with Error es -> es | Ok _ -> [])
                    @ (match build with Error e -> [ $"{label}: {e}" ] | Ok _ -> [])
                )

/// `4h`, `30m`, `1w`: a whole number and s, m, h, d or w.
let private duration (text: string) : TimeSpan option =
    let m = Text.RegularExpressions.Regex.Match(text, "^(\\d+)(s|m|h|d|w)$")

    if not m.Success then
        None
    else
        let n = float m.Groups[1].Value

        match m.Groups[2].Value with
        | "s" -> Some(TimeSpan.FromSeconds n)
        | "m" -> Some(TimeSpan.FromMinutes n)
        | "h" -> Some(TimeSpan.FromHours n)
        | "d" -> Some(TimeSpan.FromDays n)
        | _ -> Some(TimeSpan.FromDays(7.0 * n))
        |> Option.filter (fun t -> t > TimeSpan.Zero)

let private word =
    function
    | Word w
    | Quoted w -> Some w
    | Num _ -> None

/// The arguments after the query: positional literals and named ones apart.
/// In a formula a bare word is a query name — a word again here.
let private splitArgs (rest: Arg<string> list) =
    let positional = rest |> List.choose (function Named _ -> None | a -> Some(asLiteral a))
    let named = rest |> List.choose (function Named(n, v) -> Some(n, v) | _ -> None)
    positional, named

/// The named arguments shared by the seasonal algorithms.
///
/// "By default, the robust and agile algorithms use weekly seasonality"
/// (docs: dashboards/functions/algorithms).
///
/// WARNING(undocumented): forecast()'s default seasonality. Weekly, as for
/// anomalies().
///
/// WARNING(undocumented): `timezone`. It is accepted and has no effect: the
/// models run at a fixed period, so across a DST change a daily or weekly
/// season is off by the hour Datadog would shift it by.
let private seasonality (named: (string * Literal) list) : Result<Seasonality, string> =
    match named |> List.tryFind (fst >> (=) "seasonality") |> Option.map snd |> Option.bind word |> Option.defaultValue "weekly" with
    | "hourly" -> Ok Hourly
    | "daily" -> Ok Daily
    | "weekly" -> Ok Weekly
    | s -> Error $"seasonality must be 'hourly', 'daily' or 'weekly', got '{s}'"

/// anomalies(q, 'basic' | 'agile' | 'robust', bounds, name=value…)
///
/// WARNING(undocumented): what the monitor-oriented named arguments do on a
/// graph. `direction`, `alert_window`, `interval` and `count_default_zero` are
/// accepted, as in Datadog's own queries, and have no effect here: the band is
/// drawn at the query's step, both sides.
let private anomaliesArgs (from: DateTimeOffset) (rest: Arg<string> list) : Result<Extra, string> =
    let known = set [ "direction"; "alert_window"; "interval"; "count_default_zero"; "seasonality"; "timezone" ]
    let positional, named = splitArgs rest

    match positional, named |> List.tryFind (fun (n, _) -> not (known.Contains n)) with
    | _, Some(n, _) -> Error $"anomalies() has no argument '{n}'"
    | [ Some alg; Some(Num bounds) ], _ when bounds > 0.0 ->
        match word alg with
        | Some "basic" -> Ok(Anomalies(Basic, bounds))
        | Some "robust" -> seasonality named |> Result.map (fun s -> Anomalies(Robust s, bounds))
        | Some "agile" -> seasonality named |> Result.map (fun s -> Anomalies(Agile s, bounds))
        | _ -> Error "anomalies() algorithm must be 'basic', 'agile' or 'robust'"
    | _ -> Error "anomalies() takes a query, an algorithm and bounds: anomalies(query, 'basic', 2)"

/// forecast(q, 'linear' | 'seasonal', deviations, model=, history=, horizon=,
/// seasonality=, timezone=)
///
/// `horizon` is ours. Datadog takes the horizon from where forecast() is
/// used — a monitor's `next_1w` — and does not document it for graphs.
let private forecastArgs (from: DateTimeOffset) (window: TimeSpan) (rest: Arg<string> list) : Result<Extra, string> =
    let positional, named = splitArgs rest
    let ignored = set [ "interval" ]

    let model =
        match named |> List.tryFind (fst >> (=) "model") |> Option.map snd |> Option.bind word with
        | None
        | Some "default" -> Ok DefaultModel
        | Some "simple" -> Ok SimpleModel
        | Some "reactive" -> Ok ReactiveModel
        | Some m -> Error $"forecast() model must be 'default', 'simple' or 'reactive', got '{m}'"

    let span name =
        match named |> List.tryFind (fst >> (=) name) |> Option.map snd with
        | None -> Ok None
        | Some v ->
            match word v |> Option.bind duration with
            | Some d -> Ok(Some d)
            | None -> Error $"forecast() {name} must be a duration like '4h', '3d' or '1w'"

    let allowed = [ "model"; "history"; "horizon"; "seasonality"; "timezone" ]
    let unknown = named |> List.tryFind (fun (n, _) -> not (List.contains n allowed || ignored.Contains n))

    match positional, unknown, model, span "history", span "horizon" with
    | _, Some(n, _), _, _, _ -> Error $"forecast() has no argument '{n}'"
    | [ Some alg; Some(Num dev) ], None, Ok model, Ok history, Ok horizon when dev >= 0.0 ->
        match word alg with
        | Some "linear" -> Ok(Forecast(dev, LinearForecast model, history, horizon))
        | Some "seasonal" -> seasonality named |> Result.map (fun s -> Forecast(dev, SeasonalForecast s, history, horizon))
        | _ -> Error "forecast() algorithm must be 'linear' or 'seasonal'"
    | _, _, Error e, _, _
    | _, _, _, Error e, _
    | _, _, _, _, Error e -> Error e
    | _ -> Error "forecast() takes a query, an algorithm and deviations: forecast(query, 'linear', 1)"

/// A formula's outermost function may add an extra; everything below it is a
/// Node like any other.
let private resolveOutput (from: DateTimeOffset) (window: TimeSpan) (label: string) (byName: Map<string, int>) (expr: Formula) : Result<Node * Extra, string list> =
    let withExtra (args: Arg<string> list) (read: Arg<string> list -> Result<Extra, string>) =
        match args with
        | Value inner :: rest ->
            match resolve from label byName inner, read rest with
            | Ok node, Ok extra -> Ok(node, extra)
            | node, extra ->
                Error(
                    (match node with Error es -> es | Ok _ -> [])
                    @ (match extra with Error e -> [ $"{label}: {e}" ] | Ok _ -> [])
                )
        | _ -> Error [ $"{label}: the first argument must be a query" ]

    match expr with
    | Call("anomalies", args) -> withExtra args (anomaliesArgs from)
    | Call("forecast", args) -> withExtra args (forecastArgs from window)
    | _ -> resolve from label byName expr |> Result.map (fun node -> node, NoExtra)

/// The `by` keys a node's series carry: Some keys for series (empty for one
/// ungrouped series), None for a number or time(), which have no series.
///
/// Checks what arithmetic may combine: "all queries must be grouped by the
/// same" tags (docs: dashboards/querying). One side ungrouped — a single
/// series, or a number — applies to every series of the other.
let rec private groupKeys (req: ParsedTimeseriesRequest) (node: Node) : Result<Set<string> option, string> =
    let keysOf = groupKeys req

    /// A function over series needs series under it, not only numbers.
    let series (name: string) inner =
        keysOf inner
        |> Result.bind (function
            | None -> Error $"{name} needs a query inside it, not only numbers"
            | keys -> Ok keys)

    match node with
    | Fetch i -> Ok(Some(set req.Queries[i].Query.GroupBy))
    | Constant _
    | TimeOfPoint -> Ok None
    | CountNonzero inner
    | CountNotNull inner -> series "count_nonzero()/count_not_null()" inner |> Result.map (fun _ -> Some Set.empty)
    | Pointwise(_, inner)
    | Top(inner, _, _, _)
    | ExcludeNull inner
    | Timewise(_, inner)
    | Shift(_, inner)
    | DefaultZero inner
    | Outliers(inner, _, _) -> series "a function" inner
    | Arith(_, l, r) ->
        match keysOf l, keysOf r with
        | Error e, _
        | _, Error e -> Error e
        | Ok None, Ok k
        | Ok k, Ok None -> Ok k
        | Ok(Some a), Ok(Some b) when a.IsEmpty -> Ok(Some b)
        | Ok(Some a), Ok(Some b) when b.IsEmpty || a = b -> Ok(Some a)
        | Ok(Some a), Ok(Some b) ->
            let show (k: Set<string>) = k |> Seq.map (sprintf "'%s'") |> String.concat ", "
            Error $"arithmetic needs both sides grouped by the same tags, got {show a} and {show b}"

let private planOutputs (req: ParsedTimeseriesRequest) : Result<Output list, string list> =
    match req.Formulas with
    | [] -> Ok(req.Queries |> List.mapi (fun i _ -> { QueryIndex = i; Node = Fetch i; Limit = None; Extra = NoExtra }))
    | formulas ->
        let byName =
            req.Queries
            |> List.indexed
            |> List.choose (fun (i, q) -> q.Name |> Option.map (fun n -> n, i))
            |> Map.ofList

        let results =
            formulas
            |> List.mapi (fun i f ->
                resolveOutput req.From (req.To - req.From) $"formulas[{i}]" byName f.Formula
                |> Result.bind (fun (node, extra) ->
                    match groupKeys req node with
                    | Error e -> Error [ $"formulas[{i}]: {e}" ]
                    | Ok None -> Error [ $"formulas[{i}]: a formula needs at least one query, not only numbers" ]
                    | Ok(Some _) -> Ok { QueryIndex = i; Node = node; Limit = f.Limit; Extra = extra }))

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
            | Shift(_, inner)
            | DefaultZero inner
            | Outliers(inner, _, _) -> needs extra inner
            | Pointwise(_, inner)
            | Top(inner, _, _, _)
            | CountNonzero inner
            | CountNotNull inner
            | ExcludeNull inner -> needs extra inner
            | Arith(_, l, r) -> needs extra l @ needs extra r
            | Constant _
            | TimeOfPoint -> []

        let planned = queries |> List.map (function Ok q -> Some q | Error _ -> None)

        let rec firstSource node =
            match node with
            | Fetch i -> Some i
            | Pointwise(_, n) | Top(n, _, _, _) | CountNonzero n | CountNotNull n | ExcludeNull n
            | Timewise(_, n) | Shift(_, n) | DefaultZero n | Outliers(n, _, _) -> firstSource n
            | Arith(_, l, r) -> firstSource l |> Option.orElse (firstSource r)
            | Constant _
            | TimeOfPoint -> None

        // History an output's extra reads before the window, in buckets of
        // its query's step: the anomaly window, or forecast history beyond
        // the window itself.
        let extraLookback (o: Output) =
            let step = firstSource o.Node |> Option.bind (fun i -> planned[i]) |> Option.map _.Step |> Option.defaultValue requestStep

            match o.Extra with
            | NoExtra -> 0
            // Seasonal models read their history separately, at their own step;
            // the window itself still needs the lagging residuals before it.
            | Anomalies _ -> anomalyWindow
            | Forecast(_, SeasonalForecast _, _, _) -> 0
            | Forecast(_, _, history, _) ->
                let beyond = (defaultArg history window) - window
                if beyond <= TimeSpan.Zero then 0 else int (Math.Ceiling(beyond / step))

        let lookbacks =
            outputs
            |> List.collect (fun o -> needs (extraLookback o) o.Node)
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
