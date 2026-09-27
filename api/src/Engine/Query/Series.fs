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
open NinjaCat.Api.Engine.Query.Compile

type Series =
    { GroupTags: string list
      /// Bucket start (unix ms) → value.
      Points: Map<int64, float> }

/// The value shown for a series that has no value for a `by` key. Datadog
/// lower-cases tag values on intake, so no real value can be "N/A".
let notApplicable = "N/A"

/// Groups rows by their group values; each group is one series.
///
/// A bucket that starts before `from` is dropped: it holds only the part of
/// its interval inside the window, so its value covers less time than every
/// other point. Datadog's recorded v1 response likewise starts at the first
/// bucket after `from`.
let fromRows (groupBy: string list) (fromMs: int64) (rows: Row list) : Series list =
    rows
    |> List.filter (fun r -> r.BucketMs >= fromMs)
    |> List.groupBy _.Groups
    |> List.map (fun (groups, rs) ->
        { GroupTags = List.map2 (fun k v -> $"""{k}:{defaultArg v notApplicable}""") groupBy groups
          Points = rs |> List.map (fun r -> r.BucketMs, r.Value) |> Map.ofList })

/// A formula's `limit`: rank the series by their mean, keep `count`.
///
/// Mean is our choice for ranking — the spec does not say what Datadog ranks
/// formula limits by. Without a count only the order applies.
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

/// Every output's series, laid out on one time axis.
let response (outputs: (int * Series list) list) : TimeseriesFormulaQueryResponse =
    let times =
        outputs
        |> Seq.collect (snd >> Seq.collect _.Points.Keys)
        |> Seq.distinct
        |> Seq.sort
        |> List.ofSeq

    let series, values =
        outputs
        |> List.collect (fun (queryIndex, series) ->
            series
            |> List.map (fun s ->
                { GroupTags = s.GroupTags; QueryIndex = queryIndex },
                times |> List.map s.Points.TryFind))
        |> List.unzip

    { Data =
        { Id = "0"
          Type = "timeseries_response"
          Attributes = { Series = series; Times = times; Values = values } } }
