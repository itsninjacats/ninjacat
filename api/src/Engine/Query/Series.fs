/// From ClickHouse rows to the response: series, the shared time axis, limits.
///
/// Gaps stay gaps. ClickHouse returns only buckets that had points, and a
/// series with nothing in a bucket gets null there — not zero, which would
/// draw a drop that never happened. Filling (`.fill()`, default_zero,
/// interpolation) will be decided here, in one place, when it exists.
module NinjaCat.Api.Engine.Query.Series

open System
open NinjaCat.Api.Engine.Api.V2.Timeseries
open NinjaCat.Api.Engine.Api.V2.Wire
open NinjaCat.Api.Engine.Query.Plan
open NinjaCat.Api.Engine.Query.Compile

type Series =
    { GroupTags: string list
      /// Bucket start (unix ms) → value.
      Points: Map<int64, float> }

/// The value shown for a series that has no value for a `by` key. Datadog
/// lower-cases tag values on intake, so no real value can be "N/A".
///
/// WARNING(undocumented): the exact group_tags string of an N/A group. The
/// docs name the group "N/A"; `key:N/A` is our spelling.
let notApplicable = "N/A"

/// Groups rows by their group values; each group is one series.
///
/// A bucket that starts before `from` is dropped: it holds only the part of
/// its interval inside the window, so its value covers less time than every
/// other point.
///
/// WARNING(undocumented): what Datadog does with a partial first bucket.
/// Its one recorded v1 response starts at the first bucket after `from`, and
/// we do the same.
let fromRows (groupBy: string list) (fromMs: int64) (rows: Row list) : Series list =
    rows
    |> List.filter (fun r -> r.BucketMs >= fromMs)
    |> List.groupBy _.Groups
    |> List.map (fun (groups, rs) ->
        { GroupTags = List.map2 (fun k v -> $"""{k}:{defaultArg v notApplicable}""") groupBy groups
          Points = rs |> List.map (fun r -> r.BucketMs, r.Value) |> Map.ofList })

// --- filling and combining, in F# -------------------------------------------------

/// Fills one series at `times` it has no point at, within the limit after its
/// previous real point. `times` and the series are both in time order, so one
/// pass with a cursor does it.
let private fillAt (method: FillMethod) (limitMs: int64) (times: int64[]) (points: Map<int64, float>) =
    let real = points |> Map.toArray
    let mutable i = 0 // real[i] is the first real point after the current time
    let filled = ResizeArray<int64 * float>()

    for t in times do
        while i < real.Length && fst real[i] <= t do
            i <- i + 1

        if not (points.ContainsKey t) && i > 0 then
            let tp, vp = real[i - 1]

            if t - tp <= limitMs then
                match method with
                | Last -> filled.Add(t, vp)
                | Zero -> filled.Add(t, 0.0)
                | Linear when i < real.Length ->
                    let tn, vn = real[i]
                    filled.Add(t, vp + (vn - vp) * float (t - tp) / float (tn - tp))
                // Nothing after the gap to interpolate towards.
                | Linear -> ()

    if filled.Count = 0 then points else Seq.fold (fun m (t, v) -> Map.add t v m) points filled

let private combineValues (agg: Aggregation) (values: float list) =
    match agg with
    | Avg -> List.average values
    | Sum -> List.sum values
    | Min -> List.min values
    | Max -> List.max values
    | Count -> float values.Length

/// Per-series rows → one series per group: each series filled, then the
/// series of a group combined bucket by bucket with the space aggregator.
///
/// Interpolation is "aligning several series together, to make it possible
/// to perform aggregation across sources" (docs: metrics/guide/
/// interpolation-the-fill-modifier-explained). So a series is filled only at
/// the times another series of its group has a real point, and a series alone
/// in its group is never filled.
///
/// WARNING(undocumented): whether Datadog also fills to align series of
/// different groups ("group queries for easier comparisons"). We do not.
///
/// WARNING(undocumented): a gap at the start of the window, whose previous
/// real point lies before `from`. The window's reads are not extended for it,
/// so it stays a gap.
let combine (groupBy: string list) (spaceAgg: Aggregation) (fill: Fill) (fromMs: int64) (rows: Row list) : Series list =
    rows
    |> List.filter (fun r -> r.BucketMs >= fromMs)
    |> List.groupBy _.Groups
    |> List.map (fun (groups, rs) ->
        let series =
            rs
            |> List.groupBy _.SeriesId
            |> List.map (fun (_, xs) -> xs |> List.map (fun r -> r.BucketMs, r.Value) |> Map.ofList)

        let times = series |> Seq.collect _.Keys |> Seq.distinct |> Seq.sort |> Array.ofSeq

        let filled =
            match fill with
            | FillWithin(method, limit) when series.Length > 1 ->
                series |> List.map (fillAt method (int64 limit * 1000L) times)
            | _ -> series

        { GroupTags = List.map2 (fun k v -> $"""{k}:{defaultArg v notApplicable}""") groupBy groups
          Points =
            times
            |> Array.choose (fun t ->
                match filled |> List.choose (Map.tryFind t) with
                | [] -> None
                | values -> Some(t, combineValues spaceAgg values))
            |> Map.ofArray })

/// The window's bucket starts: multiples of the step, from the first at or
/// after `fromMs`, up to `toMs`. ClickHouse's toStartOfInterval aligns
/// second-sized buckets the same way.
let grid (stepMs: int64) (fromMs: int64) (toMs: int64) : int64 seq =
    let first = (fromMs + stepMs - 1L) / stepMs * stepMs
    Seq.unfold (fun t -> if t < toMs then Some(t, t + stepMs) else None) first

/// A value of 0 in every bucket of `times` a series has none in.
let zeroFill (times: int64 seq) (s: Series) =
    { s with Points = times |> Seq.fold (fun m t -> if Map.containsKey t m then m else Map.add t 0.0 m) s.Points }

/// A formula's `limit`: rank the series by their mean, keep `count`. Without
/// a count only the order applies.
///
/// WARNING(undocumented): what Datadog ranks a formula's `limit` by. We
/// use the mean.
let limit (l: ParsedLimit option) (series: Series list) : Series list =
    match l with
    | None -> series
    | Some l ->
        let mean s =
            if s.Points.IsEmpty then Double.NegativeInfinity else s.Points.Values |> Seq.average

        let ordered =
            match l.Order with
            | Desc -> series |> List.sortByDescending mean
            | Asc -> series |> List.sortBy mean

        match l.Count with
        | Some n -> List.truncate n ordered
        | None -> ordered

/// What a series carries beside its values, when its formula asked.
type SeriesExtra =
    { /// Time → (lower, upper), within the window.
      Band: Map<int64, float * float> option
      /// Past the window: (time, value, lower, upper).
      Forecast: (int64 * float * float * float) list option }

let noExtra = { Band = None; Forecast = None }

/// Every output's series with their extras, laid out on one time axis.
let responseWith (outputs: (int * (Series * SeriesExtra) list) list) : TimeseriesFormulaQueryResponse =
    let flat = outputs |> List.collect (fun (qi, xs) -> xs |> List.map (fun (s, e) -> qi, s, e))

    let times =
        flat |> Seq.collect (fun (_, s, _) -> s.Points.Keys) |> Seq.distinct |> Seq.sort |> List.ofSeq

    let anyBand = flat |> List.exists (fun (_, _, e) -> e.Band.IsSome)
    let anyForecast = flat |> List.exists (fun (_, _, e) -> e.Forecast.IsSome)

    { Data =
        { Id = "0"
          Type = "timeseries_response"
          Attributes =
            { Series = flat |> List.map (fun (qi, s, _) -> { GroupTags = s.GroupTags; QueryIndex = qi })
              Times = times
              Values = flat |> List.map (fun (_, s, _) -> times |> List.map s.Points.TryFind)
              NinjacatBounds =
                if not anyBand then None
                else
                    flat
                    |> List.map (fun (_, _, e) ->
                        e.Band
                        |> Option.map (fun band ->
                            { Upper = times |> List.map (band.TryFind >> Option.map snd)
                              Lower = times |> List.map (band.TryFind >> Option.map fst) }))
                    |> Some
              NinjacatForecast =
                if not anyForecast then None
                else
                    flat
                    |> List.map (fun (_, _, e) ->
                        e.Forecast
                        |> Option.map (fun points ->
                            { Times = points |> List.map (fun (t, _, _, _) -> t)
                              Values = points |> List.map (fun (_, v, _, _) -> v)
                              Lower = points |> List.map (fun (_, _, lo, _) -> lo)
                              Upper = points |> List.map (fun (_, _, _, hi) -> hi) }))
                    |> Some } } }

/// Every output's series, laid out on one time axis.
let response (outputs: (int * Series list) list) : TimeseriesFormulaQueryResponse =
    outputs |> List.map (fun (qi, series) -> qi, series |> List.map (fun s -> s, noExtra)) |> responseWith
