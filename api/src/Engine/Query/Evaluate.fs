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
/// Our choices where Datadog's docs are silent:
///   - log of zero or a negative number is a gap: JSON cannot carry -Infinity
///     or NaN, and neither is a value to draw.
///   - round() rounds halves away from zero (2.5 → 3, -2.5 → -3), as people
///     expect, not to even as .NET does by default.
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
/// nothing to rank by, and goes last whichever the direction.
///
/// `area` is the sum of value × bucket width, in seconds. Buckets are equal
/// within one query, so it orders series the way the true area would.
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
/// Our choices where Datadog's docs are silent, or odd:
///   - Diff-like functions work on consecutive points, so a gap is bridged:
///     the diff after a gap is against the last point before it.
///   - monotonic_diff keeps zero (a counter that did not move), and a
///     negative delta — a counter reset — becomes a gap.
///   - median_N and rollingavg_N are trailing windows: each point looks at
///     itself and the N-1 before it, as a live graph must.
///   - integral follows the docs literally: "the cumulative sum of
///     [time delta] x [value delta] over all consecutive pairs of points".
///     That makes the integral of a constant zero, which suggests the docs
///     mean something else; kept literal until checked against Datadog.
///   - ewma starts at the first point's value, then
///     s = alpha·v + (1 - alpha)·s, alpha = 2 / (span + 1): "twice the
///     weighted average age", the usual span definition.
let private alongTime (fn: TimewiseFn) (stepMs: int64) (s: Series) : Series =
    let pts = s.Points |> Map.toArray
    let seconds (dtMs: int64) = float dtMs / 1000.0

    let deltas (f: float -> float -> float option) =
        pts |> Array.pairwise |> Array.choose (fun ((t0, v0), (t1, v1)) -> f (seconds (t1 - t0)) (v1 - v0) |> Option.map (fun r -> t1, r))

    let window n (f: float[] -> float) =
        pts |> Array.mapi (fun i (t, _) -> t, pts[max 0 (i - n + 1) .. i] |> Array.map snd |> f)

    let median (xs: float[]) =
        let sorted = Array.sort xs
        let m = sorted.Length / 2
        if sorted.Length % 2 = 1 then sorted[m] else (sorted[m - 1] + sorted[m]) / 2.0

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
        | Median n -> window n median
        | RollingAvg n -> window n Array.average

    { s with Points = Map.ofArray out }

let private trim (fromMs: int64) (series: Series list) =
    series |> List.map (fun s -> { s with Points = s.Points |> Map.filter (fun t _ -> t >= fromMs) })

// --- the tree ---------------------------------------------------------------------

/// The queries a node reads, so only those go to ClickHouse.
let rec sources (node: Node) : int list =
    match node with
    | Fetch i -> [ i ]
    | Pointwise(_, inner)
    | Top(inner, _, _, _)
    | CountNonzero inner
    | CountNotNull inner
    | ExcludeNull inner
    | Timewise(_, inner) -> sources inner

/// What evaluation reads: each query's series, and each query's bucket width.
type Fetched =
    { Series: Map<int, Series list>
      StepMs: Map<int, int64> }

/// Evaluates `node`, keeping points from `fromMs` on.
///
/// A time-wise function asks its input for earlier points — its lookback —
/// and trims its own output back to `fromMs`, so nothing above it sees the
/// history: `top` ranks, and the response shows, the visible window only.
let rec evalFrom (fetched: Fetched) (fromMs: int64) (node: Node) : Series list =
    match node with
    | Fetch i -> fetched.Series[i] |> trim fromMs
    | Pointwise(fn, inner) -> evalFrom fetched fromMs inner |> List.map (mapValues fn)
    | Top(inner, count, by, order) -> evalFrom fetched fromMs inner |> top count by order
    // Every value that reaches here is finite (see `apply`), so "not null"
    // and "non-zero finite" need no further check.
    | CountNonzero inner -> evalFrom fetched fromMs inner |> countAcross (fun v -> v <> 0.0)
    | CountNotNull inner -> evalFrom fetched fromMs inner |> countAcross (fun _ -> true)
    | ExcludeNull inner -> evalFrom fetched fromMs inner |> List.filter (hasNotApplicable >> not)
    | Timewise(fn, inner) ->
        // One source below: arithmetic, the only way to join two, is not in yet.
        let stepMs = fetched.StepMs[List.head (sources inner)]
        // Saturating: `fromMs` may already be the lowest there is.
        let back = int64 (lookback fn) * stepMs
        let innerFrom = if fromMs < Int64.MinValue + back then Int64.MinValue else fromMs - back

        evalFrom fetched innerFrom inner |> List.map (alongTime fn stepMs) |> trim fromMs

/// Evaluates with every point the queries returned; for tests of single
/// functions, where there is no window to trim to.
let eval (series: Map<int, Series list>) (node: Node) : Series list =
    evalFrom { Series = series; StepMs = series |> Map.map (fun _ _ -> 20000L) } Int64.MinValue node
