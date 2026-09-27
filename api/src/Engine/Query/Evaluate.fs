/// Evaluates a formula's Node over the series its queries returned.
///
/// Functions run after time and space aggregation, on the series the response
/// will show (docs: "Most of the functions are applied after time and space
/// aggregation"). A value a function cannot produce becomes a gap — absent
/// from the series, null in the response.
module NinjaCat.Api.Engine.Query.Evaluate

open System
open NinjaCat.Api.Engine.Query.Plan
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

/// The queries a node reads, so only those go to ClickHouse.
let rec sources (node: Node) : int list =
    match node with
    | Fetch i -> [ i ]
    | Pointwise(_, inner) -> sources inner

let rec eval (fetched: Map<int, Series list>) (node: Node) : Series list =
    match node with
    | Fetch i -> fetched[i]
    | Pointwise(fn, inner) -> eval fetched inner |> List.map (mapValues fn)
