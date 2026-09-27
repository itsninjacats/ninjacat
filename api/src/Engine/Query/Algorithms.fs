/// The statistics behind the algorithmic functions: plain functions over
/// arrays of numbers, with no idea of series, queries or time zones.
///
/// Datadog documents what these functions are for, and little of how they
/// work. Each choice made here is marked WARNING(undocumented) where it is
/// made.
module NinjaCat.Api.Engine.Query.Algorithms

open System

let median (xs: float[]) =
    if xs.Length = 0 then
        nan
    else
        let s = Array.sort xs
        let m = s.Length / 2
        if s.Length % 2 = 1 then s[m] else (s[m - 1] + s[m]) / 2.0

/// Median absolute deviation from the median.
let mad (xs: float[]) =
    let m = median xs
    median (xs |> Array.map (fun x -> abs (x - m)))

/// MAD × 1.4826 estimates the standard deviation of normal data: the usual
/// constant that makes MAD "the robust analog for standard deviation".
let private madToSigma = 1.4826

// --- regression ------------------------------------------------------------------

/// Least squares on (x, y): slope and intercept.
let ols (xs: float[]) (ys: float[]) : float * float =
    let n = float xs.Length
    let mx = Array.average xs
    let my = Array.average ys
    let sxx = Array.sumBy (fun x -> (x - mx) * (x - mx)) xs
    let sxy = Array.map2 (fun x y -> (x - mx) * (y - my)) xs ys |> Array.sum

    if n < 2.0 || sxx = 0.0 then 0.0, my else let b = sxy / sxx in b, my - b * mx

/// Weighted least squares, the step inside iteratively reweighted fitting.
let private wls (xs: float[]) (ys: float[]) (ws: float[]) : float * float =
    let sw = Array.sum ws
    let mx = Array.map2 (*) ws xs |> Array.sum |> fun s -> s / sw
    let my = Array.map2 (*) ws ys |> Array.sum |> fun s -> s / sw
    let sxx = Array.map2 (fun w x -> w * (x - mx) * (x - mx)) ws xs |> Array.sum
    let sxy = Array.map3 (fun w x y -> w * (x - mx) * (y - my)) ws xs ys |> Array.sum

    if sxx = 0.0 then 0.0, my else let b = sxy / sxx in b, my - b * mx

/// A line fitted under Huber loss, by iteratively reweighted least squares:
/// residuals within `k` scales count fully, larger ones with weight k / |r|,
/// so a spike bends the line far less than under least squares.
///
/// WARNING(undocumented): Datadog names the loss, not its tuning. k = 1.345
/// (95 % efficiency on normal data, the textbook value), the scale is the
/// MAD of the residuals, and fitting stops after 50 rounds or when the line
/// moves by less than 1e-9.
let huber (xs: float[]) (ys: float[]) : float * float =
    let k = 1.345

    let rec fit (b, a) round =
        let residuals = Array.map2 (fun x y -> y - (a + b * x)) xs ys
        let scale = madToSigma * mad residuals

        if scale = 0.0 || round = 50 then
            b, a
        else
            let ws = residuals |> Array.map (fun r -> let u = abs r / scale in if u <= k then 1.0 else k / u)
            let b', a' = wls xs ys ws

            if abs (b' - b) < 1e-9 && abs (a' - a) < 1e-9 then b', a' else fit (b', a') (round + 1)

    fit (ols xs ys) 0

// --- piecewise constant ----------------------------------------------------------

/// Replaces each value by the mean of its segment, the segments found by
/// binary segmentation: split where it lowers the squared error most, and
/// keep splitting each half while a split is worth more than a penalty.
///
/// WARNING(undocumented): Datadog does not say how it finds segments. The
/// penalty is BIC-like, 2·σ²·ln n, with σ estimated from the MAD of
/// consecutive differences — a level shift is one large difference, so it
/// barely moves σ — or from their standard deviation where that MAD is zero.
/// A segment is at least 2 points.
let piecewiseConstant (ys: float[]) : float[] =
    let n = ys.Length

    if n < 4 then
        Array.create n (if n = 0 then 0.0 else Array.average ys)
    else
        let diffs = ys |> Array.pairwise |> Array.map (fun (a, b) -> b - a)

        // A mostly flat, integer-valued series (a goroutine count) has a MAD
        // of differences of zero; the penalty would vanish and every blip
        // become a segment. Then σ comes from the standard deviation instead.
        let sigma =
            match madToSigma * mad diffs / sqrt 2.0 with
            | 0.0 ->
                let m = Array.average diffs
                sqrt (diffs |> Array.averageBy (fun d -> (d - m) * (d - m))) / sqrt 2.0
            | s -> s
        let penalty = 2.0 * sigma * sigma * log (float n)

        // Prefix sums make any segment's squared error O(1).
        let s1 = Array.scan (+) 0.0 ys
        let s2 = Array.scan (fun acc y -> acc + y * y) 0.0 ys

        let sse i j = // [i, j)
            let len = float (j - i)
            let sum = s1[j] - s1[i]
            s2[j] - s2[i] - sum * sum / len

        let out = Array.zeroCreate n

        let rec segment i j =
            let best =
                [ i + 2 .. j - 2 ]
                |> List.map (fun c -> c, sse i c + sse c j)
                |> List.sortBy snd
                |> List.tryHead

            match best with
            | Some(c, split) when sse i j - split > penalty ->
                segment i c
                segment c j
            | _ ->
                let mean = (s1[j] - s1[i]) / float (j - i)
                for t in i .. j - 1 do
                    out[t] <- mean

        segment 0 n
        out

// --- autosmooth ------------------------------------------------------------------

let private kurtosis (xs: float[]) =
    let m = Array.average xs
    let v = xs |> Array.averageBy (fun x -> (x - m) ** 2.0)
    if v = 0.0 then 0.0 else (xs |> Array.averageBy (fun x -> (x - m) ** 4.0)) / (v * v)

let private roughness (xs: float[]) =
    if xs.Length < 3 then
        0.0
    else
        let d = xs |> Array.pairwise |> Array.map (fun (a, b) -> b - a)
        let m = Array.average d
        sqrt (d |> Array.averageBy (fun x -> (x - m) ** 2.0))

/// Trailing moving average over `w` points.
let movingAverage (w: int) (ys: float[]) =
    ys |> Array.mapi (fun i _ -> ys[max 0 (i - w + 1) .. i] |> Array.average)

/// The window autosmooth picks for these values.
///
/// Source: Datadog's "Auto Smoother" post
/// (https://www.datadoghq.com/blog/auto-smoother-asap/). Auto Smoother "is
/// inspired by the ASAP (Automatic Smoothing for Attention Prioritization)
/// algorithm" (Rong & Bailis, Stanford): a moving average whose window is
/// chosen from the smoothed series' roughness — "the standard deviation of the
/// first difference series" — and kurtosis, "the fourth standardized moment",
/// which keeps it from oversmoothing.
///
/// This is ASAP's own rule: of the windows up to `maxWindow`, the one with the
/// least roughness whose kurtosis stays at least the original's.
///
/// WARNING(undocumented): Datadog "weighs roughness and kurtosis differently
/// than ASAP does" and "uses a different optimization technique", without
/// saying how. We use ASAP's weighting and an exhaustive search over windows.
let autosmoothWindow (maxWindow: int) (ys: float[]) : int =
    let original = kurtosis ys

    [ 1 .. max 1 (min maxWindow (ys.Length / 4)) ]
    |> List.map (fun w -> w, movingAverage w ys)
    |> List.filter (fun (w, s) -> w = 1 || kurtosis s >= original)
    |> List.minBy (fun (_, s) -> roughness s)
    |> fst

// --- outliers --------------------------------------------------------------------

/// Which rows (series, aligned on one time axis) are outliers.
///
/// DBSCAN, as the docs describe it: the median series is the per-time median;
/// each series' distance to it is Euclidean; the threshold is the median of
/// those distances "multiplied by a normalizing constant", and ε is that
/// threshold times `tolerance`. Series closer than ε to each other cluster
/// together; everything outside the largest cluster is an outlier.
///
/// WARNING(undocumented): the normalizing constant, and how "scaled" scales.
/// The constant is 1. Scaled DBSCAN floors the threshold at 1 % of the median
/// series' norm, so tightly packed series of large values do not turn
/// outliers over noise.
///
/// WARNING(undocumented): DBSCAN's minimum cluster size. It is 1 — every
/// series is a core point, so clusters are the groups of series linked by
/// distances within ε.
let dbscanOutliers (scaled: bool) (tolerance: float) (rows: float[][]) : bool[] =
    let n = rows.Length

    if n < 3 then
        Array.create n false
    else
        let times = rows[0].Length
        let med = Array.init times (fun t -> rows |> Array.map (fun r -> r[t]) |> median)
        let dist (a: float[]) (b: float[]) = Array.map2 (fun x y -> (x - y) * (x - y)) a b |> Array.sum |> sqrt
        let threshold = rows |> Array.map (dist med) |> median

        let threshold =
            if scaled then max threshold (0.01 * dist med (Array.zeroCreate times)) else threshold

        let eps = tolerance * threshold

        // Clusters: connected components of "within ε".
        let cluster = Array.create n -1
        let mutable next = 0

        for start in 0 .. n - 1 do
            if cluster[start] < 0 then
                let stack = Collections.Generic.Stack [ start ]
                cluster[start] <- next

                while stack.Count > 0 do
                    let i = stack.Pop()

                    for j in 0 .. n - 1 do
                        if cluster[j] < 0 && dist rows[i] rows[j] <= eps then
                            cluster[j] <- next
                            stack.Push j

                next <- next + 1

        let largest = cluster |> Array.countBy id |> Array.maxBy snd |> fst
        cluster |> Array.map ((<>) largest)

/// MAD, as the docs describe it: a point is an outlier when it is more than
/// `tolerance` deviations from the per-time median; a series is when more
/// than `percent` of its points are.
///
/// WARNING(undocumented): what a "deviation" is, and how "scaled" scales. A
/// deviation is MAD × 1.4826, the standard-deviation estimate. Scaled MAD
/// floors it at 1 % of |median|, for the same reason as scaled DBSCAN.
let madOutliers (scaled: bool) (tolerance: float) (percent: float) (rows: float option[][]) : bool[] =
    let n = rows.Length

    if n < 3 then
        Array.create n false
    else
        let times = rows[0].Length

        let outlying =
            Array.init times (fun t ->
                let present = rows |> Array.choose (fun r -> r[t])
                let med = median present
                let dev = madToSigma * mad present
                let dev = if scaled then max dev (0.01 * abs med) else dev
                rows |> Array.map (fun r -> r[t] |> Option.map (fun v -> abs (v - med) > tolerance * dev)))

        Array.init n (fun i ->
            let flags = outlying |> Array.choose (fun at -> at[i])
            flags.Length > 0 && 100.0 * float (flags |> Array.filter id |> Array.length) / float flags.Length > percent)
