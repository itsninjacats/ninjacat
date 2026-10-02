/// Evaluates a formula's Node over the series its queries returned.
///
/// Functions run after time and space aggregation, on the series the response
/// will show (docs: "Most of the functions are applied after time and space
/// aggregation"). A value a function cannot produce becomes a gap — absent
/// from the series, null in the response.
module NinjaCat.Api.Engine.Query.Evaluate

open System
open NinjaCat.Api.Engine.Query.Plan
open NinjaCat.Api.Engine.Api.V2.Timeseries
open NinjaCat.Api.Engine.Query.Series

/// One value through one function; None where there is no value to show.
///
/// WARNING(undocumented): log of zero or a negative number. It is a gap:
/// JSON cannot carry -Infinity or NaN, and neither is a value to draw.
///
/// WARNING(undocumented): how round() treats halves. It rounds them away
/// from zero (2.5 → 3, -2.5 → -3), as people expect, not to even as .NET
/// does by default.
let private apply (fn: PointwiseFn) (v: float) : float option =
    let finite x = if Double.IsFinite x then Some x else None

    match fn with
    | Abs -> Some(abs v)
    | Log2 -> finite (Math.Log2 v)
    | Log10 -> finite (Math.Log10 v)
    | Ceil -> Some(ceil v)
    | Floor -> Some(floor v)
    | Round decimals -> Some(Math.Round(v, decimals, MidpointRounding.AwayFromZero))
    | ClampMin t -> Some(max v t)
    | ClampMax t -> Some(min v t)
    // "The cutoff functions do not replace values that are equal to the
    // threshold value." (docs: dashboards/functions/exclusion)
    | CutoffMin t -> if v < t then None else Some v
    | CutoffMax t -> if v > t then None else Some v

let private mapValues (fn: PointwiseFn) (s: Series) =
    { s with
        Points =
            s.Points
            |> Map.toSeq
            |> Seq.choose (fun (t, v) -> apply fn v |> Option.map (fun r -> t, r))
            |> Map.ofSeq }

// --- across series -------------------------------------------------------------

/// The number a series is ranked by. None for a series with no points: it has
/// nothing to rank by.
///
/// WARNING(undocumented): where top() puts a series with no points. It goes
/// last, whichever the direction.
///
/// WARNING(undocumented): how `area` is integrated. It is the sum of
/// value × bucket width, in seconds. Buckets are equal within one query, so
/// it orders series the way the true area would.
let private rank (by: RankBy) (s: Series) : float option =
    if s.Points.IsEmpty then
        None
    else
        let values = s.Points.Values |> List.ofSeq

        match by with
        | ByMax -> Some(List.max values)
        | ByMin -> Some(List.min values)
        | ByMean -> Some(List.average values)
        | BySum -> Some(List.sum values)
        | ByLast -> Some(s.Points |> Seq.last |> _.Value)
        | ByL2norm -> Some(values |> List.sumBy (fun v -> v * v) |> sqrt)
        | ByArea ->
            match List.ofSeq s.Points.Keys with
            | [ _ ] -> Some(List.head values)
            | keys ->
                let width = float (keys[1] - keys[0]) / 1000.0
                Some(List.sum values * width)

/// WARNING(undocumented): how top() breaks ties. The sort is stable over the
/// series' group order, which is alphabetical (the SQL orders by group).
let private top (count: int) (by: RankBy) (order: SortOrder) (series: Series list) =
    let ranked, empty = series |> List.partition (fun s -> (rank by s).IsSome)

    let ordered =
        match order with
        | Desc -> ranked |> List.sortByDescending (rank by >> Option.get)
        | Asc -> ranked |> List.sortBy (rank by >> Option.get)

    ordered @ empty |> List.truncate count

/// One series with no group: at each time any input series has a point, how
/// many of them pass `counts`. A time with no point in any series stays a gap
/// (the docs count "tag values with non-null values at each point").
let private countAcross (counts: float -> bool) (series: Series list) : Series list =
    let times = series |> Seq.collect _.Points.Keys |> Seq.distinct

    let points =
        times
        |> Seq.map (fun t ->
            t,
            series
            |> List.sumBy (fun s ->
                match s.Points.TryFind t with
                | Some v when counts v -> 1.0
                | _ -> 0.0))
        |> Map.ofSeq

    if points.IsEmpty then [] else [ { GroupTags = []; Points = points } ]

let private hasNotApplicable (s: Series) =
    s.GroupTags |> List.exists (fun t -> t.EndsWith($":{notApplicable}"))

// --- along time ------------------------------------------------------------------

/// One function over one series' points, in time order.
///
/// WARNING(undocumented): how diff-like functions handle a gap. They work
/// on consecutive points, so the diff after a gap is against the last point
/// before it.
///
/// WARNING(undocumented): monotonic_diff at zero and below. It keeps zero
/// (a counter that did not move); a negative delta — a counter reset — is a
/// gap.
///
/// WARNING(undocumented): whether median_N and rollingavg_N windows are
/// trailing or centred. They are trailing: each point looks at itself and the
/// N-1 before it, as a live graph must.
///
/// WARNING(undocumented): how ewma starts. At the first point's value, then
/// s = alpha·v + (1 - alpha)·s, alpha = 2 / (span + 1): "twice the weighted
/// average age", the usual span definition.
///
/// FIXME(integral): the docs define integral as "the cumulative sum of
/// [time delta] x [value delta] over all consecutive pairs of points", and
/// that is what this does. It makes the integral of a constant zero, so the
/// docs likely mean [time delta] x [value] — the area under the curve.
/// Unverified: nobody here has a Datadog account to compare against. The fix
/// is the `Integral` case below: `dt * dv` → `dt * v1`.
let private alongTime (fn: TimewiseFn) (stepMs: int64) (s: Series) : Series =
    let pts = s.Points |> Map.toArray
    let seconds (dtMs: int64) = float dtMs / 1000.0

    let deltas (f: float -> float -> float option) =
        pts |> Array.pairwise |> Array.choose (fun ((t0, v0), (t1, v1)) -> f (seconds (t1 - t0)) (v1 - v0) |> Option.map (fun r -> t1, r))

    let window n (f: float[] -> float) =
        pts |> Array.mapi (fun i (t, _) -> t, pts[max 0 (i - n + 1) .. i] |> Array.map snd |> f)

    let out =
        match fn with
        | Cumsum -> pts |> Array.scan (fun (_, acc) (t, v) -> t, acc + v) (0L, 0.0) |> Array.tail
        | Integral ->
            match pts with
            | [||] -> [||]
            | _ ->
                let first = fst pts[0], 0.0
                let steps = deltas (fun dt dv -> Some(dt * dv))
                Array.append [| first |] (steps |> Array.scan (fun (_, acc) (t, x) -> t, acc + x) first |> Array.tail)
        | Diff -> deltas (fun _ dv -> Some dv)
        | MonotonicDiff -> deltas (fun _ dv -> if dv >= 0.0 then Some dv else None)
        | Derivative
        | PerSecond -> deltas (fun dt dv -> Some(dv / dt))
        | PerMinute -> deltas (fun dt dv -> Some(dv / dt * 60.0))
        | PerHour -> deltas (fun dt dv -> Some(dv / dt * 3600.0))
        | Throughput -> pts |> Array.map (fun (t, v) -> t, v / seconds stepMs)
        | Ewma span ->
            let alpha = 2.0 / (float span + 1.0)

            match pts with
            | [||] -> [||]
            | _ -> pts |> Array.tail |> Array.scan (fun (_, e) (t, v) -> t, alpha * v + (1.0 - alpha) * e) pts[0]
        | Median n -> window n Algorithms.median
        | RollingAvg n -> window n Array.average
        | TrendLine
        | RobustTrend when pts.Length > 0 ->
            let t0 = fst pts[0]
            let xs = pts |> Array.map (fun (t, _) -> seconds (t - t0))
            let ys = pts |> Array.map snd
            let slope, intercept = (if fn = TrendLine then Algorithms.ols else Algorithms.huber) xs ys
            Array.map2 (fun (t, _) x -> t, intercept + slope * x) pts xs
        | PiecewiseConstant -> Array.map2 (fun (t, _) v -> t, v) pts (Algorithms.piecewiseConstant (Array.map snd pts))
        // Autosmooth picks one window across all series; see evalShifted.
        | Autosmooth -> pts
        | TrendLine
        | RobustTrend -> [||]

    { s with Points = Map.ofArray out }

let private trim (fromMs: int64) (series: Series list) =
    series |> List.map (fun s -> { s with Points = s.Points |> Map.filter (fun t _ -> t >= fromMs) })

// --- the tree ---------------------------------------------------------------------

/// What a node reads: which query, and shifted by how much. The same query
/// may be read at several offsets (`a` and `week_before(a)` side by side);
/// each is one trip to ClickHouse.
let rec fetches (node: Node) : (int * int64) list =
    let rec go (shift: int64) node =
        match node with
        | Fetch i -> [ i, shift ]
        | Shift(offset, inner) -> go (shift + offset) inner
        | Pointwise(_, inner)
        | Top(inner, _, _, _)
        | CountNonzero inner
        | CountNotNull inner
        | ExcludeNull inner
        | Timewise(_, inner)
        | DefaultZero inner
        | Outliers(inner, _, _) -> go shift inner
        | Arith(_, l, r) -> go shift l @ go shift r
        | Constant _
        | TimeOfPoint -> []

    go 0L node

// --- arithmetic ---------------------------------------------------------------------

/// One operation on two values; None where it has no finite answer.
///
/// WARNING(undocumented): division by zero. It is a gap, not ±Infinity, which
/// JSON cannot carry and a graph cannot draw.
let private arith (op: ArithOp) (a: float) (b: float) : float option =
    let r =
        match op with
        | Plus -> a + b
        | Minus -> a - b
        | Times -> a * b
        | Divide -> if b = 0.0 then nan else a / b
        | Minimum -> min a b
        | Maximum -> max a b
        | Power -> Math.Pow(a, b)

    if Double.IsFinite r then Some r else None

/// An operand of arithmetic: series, or a value per time with no series of
/// its own — a number, time(), or arithmetic on those.
type private Operand =
    | Many of Series list
    | Scalar of (int64 -> float)

/// Point by point where both have a value — a gap on either side is a gap in
/// the result.
///
/// WARNING(undocumented): operands at different steps (one query with
/// `.rollup(sum, 60)`, the other at the request's 20 s). Their times rarely
/// meet, and most points become gaps; Datadog does not say how it aligns them.
let private zipSeries (op: ArithOp) (a: Series) (b: Series) tags =
    { GroupTags = tags
      Points =
        a.Points
        |> Seq.choose (fun p ->
            b.Points.TryFind p.Key |> Option.bind (fun bv -> arith op p.Value bv) |> Option.map (fun r -> p.Key, r))
        |> Map.ofSeq }

let private withScalar (f: float -> float -> float option) (s: Series) =
    { s with Points = s.Points |> Seq.choose (fun p -> f (float p.Key) p.Value |> Option.map (fun r -> p.Key, r)) |> Map.ofSeq }

/// Two operands combined, series matched by group:
///   - a number or time() applies to every series;
///   - one ungrouped series (a query with no `by`) applies to every series of
///     the other side;
///   - otherwise series pair up by their group tags, as a set, so `by {a, b}`
///     meets `by {b, a}`.
///
/// WARNING(undocumented): a group on one side only. It is dropped — there is
/// nothing to combine it with.
let private combineOperands (op: ArithOp) (left: Operand) (right: Operand) : Operand =
    let ungrouped =
        function
        | [ s ] when List.isEmpty s.GroupTags -> Some s
        | _ -> None

    match left, right with
    | Scalar f, Scalar g -> Scalar(fun t -> match arith op (f t) (g t) with Some r -> r | None -> nan)
    | Scalar f, Many b -> Many(b |> List.map (withScalar (fun t v -> arith op (f (int64 t)) v)))
    | Many a, Scalar g -> Many(a |> List.map (withScalar (fun t v -> arith op v (g (int64 t)))))
    | Many a, Many b ->
        match ungrouped a, ungrouped b with
        | Some sa, _ -> Many(b |> List.map (fun sb -> zipSeries op sa sb sb.GroupTags))
        | _, Some sb -> Many(a |> List.map (fun sa -> zipSeries op sa sb sa.GroupTags))
        | None, None ->
            let byGroup = b |> List.map (fun s -> set s.GroupTags, s) |> Map.ofList

            Many(
                a
                |> List.choose (fun sa -> byGroup.TryFind(set sa.GroupTags) |> Option.map (fun sb -> zipSeries op sa sb sa.GroupTags))
            )

/// What evaluation reads.
type Fetched =
    {
        /// Each (query, offset)'s series, with times already moved to the
        /// present window.
        Series: Map<int * int64, Series list>
        /// Each query's bucket width.
        StepMs: Map<int, int64>
        /// The window's end, for functions that fill it to the end.
        ToMs: int64
    }

/// Evaluates `node`, keeping points from `fromMs` on.
///
/// A time-wise function asks its input for earlier points — its lookback —
/// and trims its own output back to `fromMs`, so nothing above it sees the
/// history: `top` ranks, and the response shows, the visible window only.
let rec private evalShifted (fetched: Fetched) (shift: int64) (fromMs: int64) (node: Node) : Series list =
    let evalFrom = evalShifted fetched shift

    match node with
    | Fetch i -> fetched.Series[(i, shift)] |> trim fromMs
    | Shift(offset, inner) -> evalShifted fetched (shift + offset) fromMs inner
    // "Fills empty time intervals using the value 0" — every bucket of the
    // window, after time and space aggregation (docs: dashboards/functions/
    // interpolation).
    //
    // WARNING(undocumented): default_zero over a query with no series in the
    // window. It gives one series without tags, all zeros, so a sparse metric
    // reads as 0 rather than as nothing.
    //
    // WARNING(undocumented): the docs say default_zero fills "with
    // interpolation" where interpolation is enabled. Interpolation has already
    // run before space aggregation here; default_zero fills what is left with 0.
    // WARNING(undocumented): what outliers() returns. On a Datadog graph it
    // colours the outlying series and greys the rest; the v2 response has
    // nowhere to carry that mark. It returns the outlying series only — what a
    // monitor alerts on.
    | Outliers(inner, algorithm, tolerance) ->
        let series = evalFrom fromMs inner |> Array.ofList
        let times = series |> Seq.collect _.Points.Keys |> Seq.distinct |> Seq.sort |> Array.ofSeq
        let rows = series |> Array.map (fun s -> times |> Array.map s.Points.TryFind)

        let flags =
            match algorithm with
            | Mad(scaled, percent) -> Algorithms.madOutliers scaled tolerance percent rows
            | Dbscan scaled ->
                // A series missing a time is placed at that time's median, so
                // a gap is neither near nor far from the others.
                let medians = Array.init times.Length (fun t -> rows |> Array.choose (fun r -> r[t]) |> Algorithms.median)
                rows |> Array.map (Array.mapi (fun t v -> defaultArg v medians[t])) |> Algorithms.dbscanOutliers scaled tolerance

        Array.zip series flags |> Array.filter snd |> Array.map fst |> List.ofArray
    | Arith _
    | Constant _
    | TimeOfPoint ->
        let rec operand node =
            match node with
            | Constant c -> Scalar(fun _ -> c)
            // Times are milliseconds; time() is seconds.
            | TimeOfPoint -> Scalar(fun t -> float t / 1000.0)
            | Arith(op, l, r) -> combineOperands op (operand l) (operand r)
            | other -> Many(evalFrom fromMs other)

        match operand node with
        | Many series -> series
        // The plan refuses a formula made only of numbers.
        | Scalar _ -> []
    | DefaultZero inner ->
        let stepMs = fetched.StepMs[fetches inner |> List.head |> fst]
        let times = grid stepMs fromMs fetched.ToMs |> Array.ofSeq

        match evalFrom fromMs inner with
        | [] -> [ zeroFill times { GroupTags = []; Points = Map.empty } ]
        | series -> series |> List.map (zeroFill times)
    | Pointwise(fn, inner) -> evalFrom fromMs inner |> List.map (mapValues fn)
    | Top(inner, count, by, order) -> evalFrom fromMs inner |> top count by order
    // Every value that reaches here is finite (see `apply`), so "not null"
    // and "non-zero finite" need no further check.
    | CountNonzero inner -> evalFrom fromMs inner |> countAcross (fun v -> v <> 0.0)
    | CountNotNull inner -> evalFrom fromMs inner |> countAcross (fun _ -> true)
    | ExcludeNull inner -> evalFrom fromMs inner |> List.filter (hasNotApplicable >> not)
    | Timewise(fn, inner) ->
        // One source below: arithmetic, the only way to join two, is not in yet.
        let stepMs = fetched.StepMs[fetches inner |> List.head |> fst]
        // Saturating: `fromMs` may already be the lowest there is.
        let back = int64 (lookback fn) * stepMs
        let innerFrom = if fromMs < Int64.MinValue + back then Int64.MinValue else fromMs - back

        let series = evalFrom innerFrom inner

        match fn with
        // "When you apply Auto Smoother to several timeseries using a group
        // by query, the same window size is applied on all the timeseries"
        // (https://www.datadoghq.com/blog/auto-smoother-asap/).
        //
        // WARNING(undocumented): how that one window is chosen. Each series
        // gets its own ASAP window, and all use the median of them. The
        // window is capped at the lookback autosmooth reads, so the first
        // visible point always has a full window behind it.
        | Autosmooth ->
            let values = series |> List.map (fun s -> s.Points.Values |> Array.ofSeq) |> List.filter (fun v -> v.Length > 0)

            match values with
            | [] -> series |> trim fromMs
            | _ ->
                let w =
                    values
                    |> List.map (Algorithms.autosmoothWindow (lookback Autosmooth) >> float)
                    |> Array.ofList
                    |> Algorithms.median
                    |> int

                series
                |> List.map (fun s ->
                    let pts = s.Points |> Map.toArray
                    let smoothed = Algorithms.movingAverage w (Array.map snd pts)
                    { s with Points = Array.map2 (fun (t, _) v -> t, v) pts smoothed |> Map.ofArray })
                |> trim fromMs
        | _ -> series |> List.map (alongTime fn stepMs) |> trim fromMs

let evalFrom (fetched: Fetched) (fromMs: int64) (node: Node) : Series list = evalShifted fetched 0L fromMs node

/// Evaluates with every point the queries returned; for tests of single
/// functions, where there is no window to trim to.
let eval (series: Map<int, Series list>) (node: Node) : Series list =
    evalFrom
        { Series = series |> Map.toSeq |> Seq.map (fun (i, s) -> (i, 0L), s) |> Map.ofSeq
          StepMs = series |> Map.map (fun _ _ -> 20000L)
          ToMs = Int64.MaxValue }
        Int64.MinValue
        node
