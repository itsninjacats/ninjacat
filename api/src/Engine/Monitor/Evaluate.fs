/// One evaluation of a metric monitor: when it runs, the window it reads, and
/// one value per group, through the engine. The tick time `at` is passed in;
/// it is the evaluation's identity, and nothing here reads the clock.
///
/// Arithmetic follows Datadog's order. Without `.as_count()` the formula runs
/// per bucket and the time aggregator reduces the window after it, which is
/// the timeseries path. With it each query is reduced first and the formula
/// runs on the reduced values, which is the scalar path.
module NinjaCat.Api.Engine.Monitor.Evaluate

open System
open System.Security.Cryptography
open System.Text
open System.Threading.Tasks
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Api.V2.Scalar
open NinjaCat.Api.Engine.Api.V2.Timeseries
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor.Definition

/// The read window, `[From, To)`.
type Period = { From: DateTimeOffset; To: DateTimeOffset }

type Reading =
    /// The value the thresholds are compared with.
    | Judged of float
    /// The group is in the result, but nothing in this window is judged.
    | Skipped of reason: string

type GroupResult =
    { /// `host:a,role:web`: the sorted `k:v` pairs; `*` for a simple alert.
      Key: string
      Tags: string list
      Reading: Reading
      /// The newest point the group had in the window. None: the engine kept
      /// the series (a function's lookback held points) but the window has
      /// none, which is no data.
      LastPoint: DateTimeOffset option }

/// An evaluation that judged. A group absent from it had no data.
type Evaluation =
    { Period: Period
      Groups: GroupResult list }

let simpleGroup = "*"

// --- when ------------------------------------------------------------------------------

/// How far back from `to` the evaluation reads.
let private reach (q: MonitorQuery) : TimeSpan =
    match q.Detection with
    // WARNING(undocumented): a forecast without history reads as far back as
    // it looks ahead.
    | Forecast f -> defaultArg f.History q.Window.Length
    | Threshold
    | Change _
    | Anomaly _
    | Outlier _ -> q.Window.Length

/// Every minute for windows under a day, every 10 minutes up to two days,
/// every 30 above.
let frequency (q: MonitorQuery) : TimeSpan =
    let judged =
        match q.Detection with
        // WARNING(undocumented): an anomaly monitor is timed by its
        // alert_window; its outer window only sizes graphs.
        | Anomaly a -> a.AlertWindow.Length
        | Threshold
        | Change _
        | Forecast _
        | Outlier _ -> reach q

    if judged < TimeSpan.FromHours 24.0 then TimeSpan.FromMinutes 1.0
    elif judged <= TimeSpan.FromHours 48.0 then TimeSpan.FromMinutes 10.0
    else TimeSpan.FromMinutes 30.0

/// Where in the first 20 s after each tick a monitor runs, so monitors on the
/// same frequency do not all query at once. Stable across processes, as
/// string.GetHashCode is not.
let offset (monitor: string) : TimeSpan =
    let hash = SHA256.HashData(Encoding.UTF8.GetBytes monitor)
    TimeSpan.FromMilliseconds(float (BitConverter.ToUInt32(hash, 0) % 20_000u))

/// The next tick: `at`, a multiple of the frequency on the wall clock, and
/// when it fires, `at` plus the monitor's offset. It fires after `now` and
/// comes after `previous`, the tick last evaluated, even when a timer fired
/// early or the clock stepped back: a group gets one transition per `at`. A
/// missed tick is not made up.
let nextTick (monitor: string) (frequency: TimeSpan) (previous: DateTimeOffset option) (now: DateTimeOffset) : DateTimeOffset * DateTimeOffset =
    let delay = offset monitor
    let f = frequency.Ticks

    let after =
        match previous with
        | Some p when p > now - delay -> p
        | _ -> now - delay

    let at = DateTimeOffset(after.UtcTicks / f * f + f, TimeSpan.Zero)
    at, at + delay

// --- the window and its step -----------------------------------------------------------

let private rollupInterval (q: MonitorQuery) : TimeSpan option =
    match Parse.leaves q.Measured |> List.choose Plan.rollupInterval with
    | [] -> None
    | intervals -> Some(List.max intervals)

/// The smallest step of whole seconds, at least `step`, that divides the
/// shift; `step` when the shift is shorter. Buckets sit on the epoch, so only
/// then does a shifted bucket land on a current one.
let private dividing (shift: TimeSpan) (step: TimeSpan) : TimeSpan =
    let shiftSeconds = int64 shift.TotalSeconds
    let least = int64 (Math.Ceiling step.TotalSeconds)

    match Seq.tryFind (fun s -> shiftSeconds % s = 0L) (seq { least..shiftSeconds }) with
    | Some seconds -> TimeSpan.FromSeconds(float seconds)
    | None -> step

/// An explicit `.rollup()` interval, then an anomaly's or forecast's
/// `interval`, otherwise 15 s; widened, as the engine widens any step, to fit
/// 1500 points in the window, and for a change to divide its shift.
///
/// WARNING(undocumented): 15 s, so one agent point falls in one bucket.
let private stepOf (q: MonitorQuery) : TimeSpan =
    let rollup = rollupInterval q

    let hint =
        match rollup, q.Detection with
        | Some interval, _ -> interval
        | None, Anomaly { Interval = Some interval }
        | None, Forecast { Interval = Some interval } -> interval
        | None, _ -> TimeSpan.FromSeconds 15.0

    let fitted = Plan.step (reach q) (Some hint)

    match rollup, q.Detection with
    | None, Change c -> dividing c.Shift.Length fitted
    | _ -> fitted

let private floorTo (step: TimeSpan) (t: DateTimeOffset) =
    let stepMs = int64 step.TotalMilliseconds
    DateTimeOffset.FromUnixTimeMilliseconds(t.ToUnixTimeMilliseconds() / stepMs * stepMs)

let private request (q: MonitorQuery) (p: Period) (step: TimeSpan) : ParsedTimeseriesRequest =
    let queries, formula = Engine.split q.Expression

    let formulas =
        match q.Detection with
        | Change c -> [ formula; Call("timeshift", [ Value formula; Value(Number(-c.Shift.Length.TotalSeconds)) ]) ]
        | Threshold
        | Anomaly _
        | Forecast _
        | Outlier _ -> [ formula ]

    { From = p.From
      To = p.To
      Interval = Some step
      Queries = queries
      Formulas = formulas |> List.map (fun f -> { Formula = f; Limit = None }) }

/// The window a tick reads, and its step: `to = floor(at − max(evaluation_delay,
/// 30 s), step)`, `from = to − window`.
///
/// WARNING(undocumented): the 30 s floor. It covers the agent's 15 s flush
/// and the intake's batching.
let period (retention: Plan.Retention) (d: Definition) (at: DateTimeOffset) : Period * TimeSpan =
    let q = d.Query
    let delay = max d.Options.EvaluationDelay (TimeSpan.FromSeconds 30.0)

    let within (step: TimeSpan) =
        let until = floorTo step (at - delay)
        { From = until - reach q; To = until }

    let step = stepOf q
    let p = within step

    // A read past the raw retention, lookback and shift included, makes the
    // engine run whole minutes; the window is aligned to the step it runs.
    match Plan.plan retention at (request q p step) with
    | Ok plan when plan.Step <> step -> within plan.Step, plan.Step
    | Ok _
    | Error _ -> p, step

// --- groups and values -----------------------------------------------------------------

type private Line =
    { Tags: Engine.Tag list
      Points: (DateTimeOffset * float) list }

/// The group a series belongs to; None for an N/A group, which Datadog drops.
/// A series with several values of a `by` tag is already one series per value
/// (Compile.groupSql), so it is judged in each group.
///
/// WARNING(undocumented): Datadog's template docs suggest it may join the
/// values into one group instead.
let private groupOf (q: MonitorQuery) (tags: Engine.Tag list) : (string * string list) option =
    if tags |> List.exists (fun t -> t.Value.IsNone) then
        None
    else
        match tags |> List.map (fun t -> $"{t.Key}:{t.Value.Value}") |> List.sort with
        | [] when q.GroupBy.IsEmpty -> Some(simpleGroup, [])
        // A series without tags in a multi alert is no group of it:
        // default_zero over no series gives one.
        | [] -> None
        | pairs -> Some(String.concat "," pairs, pairs)

let private reduce (agg: TimeAggregator) (values: float list) : float =
    match agg with
    | Avg -> List.average values
    | Sum -> List.sum values
    | Min -> List.min values
    | Max -> List.max values

/// Datadog's require_full_window buckets: a minute for windows counted in
/// minutes, ten for hours, an hour for days. Never finer than the step.
///
/// WARNING(undocumented): weeks and months take the hour of days.
let private fullWindowBucket (w: Window) (step: TimeSpan) : TimeSpan =
    let bucket =
        match w.Unit with
        | Minutes -> TimeSpan.FromMinutes 1.0
        | Hours -> TimeSpan.FromMinutes 10.0
        | Days
        | Weeks
        | Months -> TimeSpan.FromHours 1.0

    max bucket step

/// Full: the first bucket has data and at most 3 are empty.
let private isFull (bucket: TimeSpan) (p: Period) (times: DateTimeOffset list) : bool =
    let count = int (Math.Ceiling((p.To - p.From) / bucket))
    let filled = times |> List.map (fun t -> int ((t - p.From) / bucket)) |> set
    filled.Contains 0 && count - filled.Count <= 3

/// One group's points reduced to its value. A bucket whose arithmetic gave no
/// number is left out; a window with nothing left is Skipped.
///
/// WARNING(undocumented): Skipped, not No Data, when nothing is left. One
/// Datadog page says No Data.
let private readingOf (d: Definition) (p: Period) (step: TimeSpan) (points: (DateTimeOffset * float) list) : Reading =
    let finite = points |> List.filter (snd >> Double.IsFinite)
    let values = List.map snd finite
    let bucket = fullWindowBucket d.Query.Window step

    // WARNING(undocumented): an empty `.as_count()` or `.as_rate()` bucket
    // reads 0, zero-filled by the engine, so the rule sees no gaps there; on
    // the as_count path the window is one value and is not checked at all.
    if d.Options.RequireFullWindow && not d.Query.AsCount && not (isFull bucket p (List.map fst finite)) then
        Skipped "require_full_window: the window is not full"
    elif values.IsEmpty then
        Skipped "no bucket in the window has a value"
    else
        Judged(reduce d.Query.Aggregator values)

/// Per bucket, the change from the shifted value: `a − b`, or `(a − b) / b × 100`.
/// A bucket with no shifted value has no change.
let private changeOf (measure: ChangeMeasure) (current: Line) (previous: Line option) : (DateTimeOffset * float) list =
    let before =
        match previous with
        | Some line -> Map.ofList line.Points
        | None -> Map.empty

    [ for t, a in current.Points do
          match before.TryFind t, measure with
          | Some b, Absolute -> t, a - b
          | Some b, Percent -> t, (a - b) / b * 100.0
          | None, _ -> () ]

// --- running ---------------------------------------------------------------------------

let private timeOf (ms: int64) = DateTimeOffset.FromUnixTimeMilliseconds ms

let private scalarAggregator (agg: TimeAggregator) : ScalarAggregator =
    match agg with
    | Avg -> ScalarAvg
    | Sum -> ScalarSum
    | Min -> ScalarMin
    | Max -> ScalarMax

let private timeseries execute tenant retention at (req: ParsedTimeseriesRequest) : Task<Result<Line list list, string list>> =
    task {
        match! Engine.parsed execute tenant retention at (req.Formulas |> List.mapi (fun i _ -> string i)) req with
        | Error problems -> return Error problems
        | Ok results ->
            return
                Ok
                    [ for r in results ->
                          [ for s in r.Series -> { Tags = s.Tags; Points = s.Points |> List.map (fun p -> p.Time, p.Value) } ] ]
    }

/// Each output's series as one value: each query reduced by the time
/// aggregator first, the formula after.
let private scalar execute tenant retention at (agg: TimeAggregator) (req: ParsedTimeseriesRequest) : Task<Result<Line list list, string list>> =
    task {
        match Plan.plan retention at req with
        | Error problems -> return Error problems
        | Ok plan ->
            match! Execute.evaluateScalar execute tenant plan (plan.Queries |> List.map (fun _ -> scalarAggregator agg)) with
            | Error problems -> return Error problems
            | Ok outputs ->
                return
                    Ok
                        [ for series in outputs ->
                              [ for s in series ->
                                    { Tags = Engine.tags s.GroupTags
                                      Points = s.Points |> Map.toList |> List.map (fun (t, v) -> timeOf t, v) } ] ]
    }

/// Evaluates `d` at the tick `at`. `execute` runs the engine's SQL; the
/// caller passes ClickHouse, tests a list. Error is the evaluation's error:
/// the engine refused the request, or the query yields more than `maxGroups`
/// groups. Nothing is truncated.
let evaluate
    (execute: Sql -> Task<Compile.Row list>)
    (tenant: TenantId)
    (retention: Plan.Retention)
    (maxGroups: int)
    (d: Definition)
    (at: DateTimeOffset)
    : Task<Result<Evaluation, string>> =
    task {
        let q = d.Query
        let p, step = period retention d at

        let detection =
            match q.Detection with
            | Threshold -> Ok()
            | Change _ when q.AsCount -> Ok()
            | Change c when c.Shift.Length.Ticks % step.Ticks = 0L -> Ok()
            | Change c ->
                Error
                    $"change over {c.Shift.Text}: the step is {step.TotalSeconds} s, which does not divide the shift, so no bucket has one to compare with"
            | Anomaly _ -> Error "anomaly monitors are not evaluated yet"
            | Forecast _ -> Error "forecast monitors are not evaluated yet"
            | Outlier _ -> Error "outlier monitors are not evaluated yet"

        match detection with
        | Error e -> return Error e
        | Ok() ->
            let req = request q p step

            let! outputs =
                if q.AsCount then
                    scalar execute tenant retention at q.Aggregator req
                else
                    timeseries execute tenant retention at req

            match outputs with
            | Error problems -> return Error(String.concat "; " problems)
            | Ok outputs ->
                let byGroup (lines: Line list) =
                    lines |> List.choose (fun line -> groupOf q line.Tags |> Option.map (fun (key, tags) -> key, (tags, line)))

                let current = byGroup outputs.Head |> List.distinctBy fst

                if current.Length > maxGroups then
                    return Error $"too_many_groups: the query yields {current.Length} groups, at most {maxGroups} are evaluated"
                else
                    let points =
                        match q.Detection, outputs with
                        | Change c, [ _; shifted ] ->
                            let previous = byGroup shifted |> List.map (fun (key, (_, line)) -> key, line) |> Map.ofList
                            fun key (line: Line) -> changeOf c.Measure line (previous.TryFind key)
                        | _ -> fun _ (line: Line) -> line.Points

                    // WARNING(undocumented): on the as_count path the one
                    // point stands for a window whose empty buckets read 0, so
                    // the newest real bucket is unknown; the last one is taken.
                    // A group that stops reporting there goes to No Data up to
                    // a window late.
                    let lastPoint (line: Line) =
                        match line.Points with
                        | [] -> None
                        | _ when q.AsCount -> Some(p.To - step)
                        | pts -> Some(pts |> List.map fst |> List.max)

                    return
                        Ok
                            { Period = p
                              Groups =
                                [ for key, (tags, line) in current ->
                                      { Key = key
                                        Tags = tags
                                        Reading = readingOf d p step (points key line)
                                        LastPoint = lastPoint line } ] }
    }
