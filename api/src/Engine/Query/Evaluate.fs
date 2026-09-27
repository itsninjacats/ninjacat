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

// --- the tree ---------------------------------------------------------------------

/// The queries a node reads, so only those go to ClickHouse.
let rec sources (node: Node) : int list =
    match node with
    | Fetch i -> [ i ]
    | Pointwise(_, inner)
    | Top(inner, _, _, _)
    | CountNonzero inner
    | CountNotNull inner
    | ExcludeNull inner -> sources inner

let rec eval (fetched: Map<int, Series list>) (node: Node) : Series list =
    match node with
    | Fetch i -> fetched[i]
    | Pointwise(fn, inner) -> eval fetched inner |> List.map (mapValues fn)
    | Top(inner, count, by, order) -> eval fetched inner |> top count by order
    // Every value that reaches here is finite (see `apply`), so "not null"
    // and "non-zero finite" need no further check.
    | CountNonzero inner -> eval fetched inner |> countAcross (fun v -> v <> 0.0)
    | CountNotNull inner -> eval fetched inner |> countAcross (fun _ -> true)
    | ExcludeNull inner -> eval fetched inner |> List.filter (hasNotApplicable >> not)
