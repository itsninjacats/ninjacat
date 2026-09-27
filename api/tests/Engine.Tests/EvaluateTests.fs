module NinjaCat.Api.Engine.Tests.EvaluateTests

open Xunit
open NinjaCat.Api.Engine.Query.Plan
open NinjaCat.Api.Engine.Query.Series
open NinjaCat.Api.Engine.Query.Evaluate

let private series (values: float list) =
    { GroupTags = [ "host:a" ]; Points = values |> List.mapi (fun i v -> int64 i, v) |> Map.ofList }

/// The values after applying one function to [-2.5; -1; 0; 1; 2.5; 100], in
/// time order; None where the function left a gap.
let private through fn =
    let input = series [ -2.5; -1.0; 0.0; 1.0; 2.5; 100.0 ]
    let out = eval (Map [ 0, [ input ] ]) (Pointwise(fn, Fetch 0)) |> List.exactlyOne
    [ 0L .. 5L ] |> List.map out.Points.TryFind

[<Fact>]
let ``abs, ceil, floor`` () =
    Assert.Equal<float option list>([ Some 2.5; Some 1.0; Some 0.0; Some 1.0; Some 2.5; Some 100.0 ], through Abs)
    Assert.Equal<float option list>([ Some -2.0; Some -1.0; Some 0.0; Some 1.0; Some 3.0; Some 100.0 ], through Ceil)
    Assert.Equal<float option list>([ Some -3.0; Some -1.0; Some 0.0; Some 1.0; Some 2.0; Some 100.0 ], through Floor)

[<Fact>]
let ``round takes halves away from zero`` () =
    Assert.Equal<float option list>([ Some -3.0; Some -1.0; Some 0.0; Some 1.0; Some 3.0; Some 100.0 ], through (Round 0))

[<Fact>]
let ``round to decimals`` () =
    let out = eval (Map [ 0, [ series [ 3.14159 ] ] ]) (Pointwise(Round 2, Fetch 0)) |> List.exactlyOne
    Assert.Equal(3.14, out.Points[0L])

[<Fact>]
let ``log of zero or less is a gap`` () =
    Assert.Equal<float option list>([ None; None; None; Some 0.0; Some(log10 2.5); Some 2.0 ], through Log10)
    Assert.Equal<float option list>(
        [ None; None; None; Some 0.0; Some(System.Math.Log2 2.5); Some(System.Math.Log2 100.0) ],
        through Log2
    )

[<Fact>]
let ``clamp raises or lowers to the threshold`` () =
    Assert.Equal<float option list>([ Some 0.0; Some 0.0; Some 0.0; Some 1.0; Some 2.5; Some 100.0 ], through (ClampMin 0.0))
    Assert.Equal<float option list>([ Some -2.5; Some -1.0; Some 0.0; Some 1.0; Some 2.5; Some 10.0 ], through (ClampMax 10.0))

[<Fact>]
let ``cutoff removes beyond the threshold and keeps it equal`` () =
    Assert.Equal<float option list>([ None; None; Some 0.0; Some 1.0; Some 2.5; Some 100.0 ], through (CutoffMin 0.0))
    Assert.Equal<float option list>([ Some -2.5; Some -1.0; Some 0.0; Some 1.0; Some 2.5; None ], through (CutoffMax 2.5))

[<Fact>]
let ``gaps stay gaps and every series is mapped`` () =
    let a = { GroupTags = [ "host:a" ]; Points = Map [ 0L, -1.0; 20L, -2.0 ] }
    let b = { GroupTags = [ "host:b" ]; Points = Map [ 20L, -3.0 ] }
    let out = eval (Map [ 0, [ a; b ] ]) (Pointwise(Abs, Fetch 0))
    Assert.Equal<Map<int64, float>>(Map [ 0L, 1.0; 20L, 2.0 ], out[0].Points)
    Assert.Equal<Map<int64, float>>(Map [ 20L, 3.0 ], out[1].Points)

[<Fact>]
let ``nested functions apply inside out`` () =
    // abs(log10(x)) on 0.01 → log10 = -2 → abs = 2
    let out = eval (Map [ 0, [ series [ 0.01 ] ] ]) (Pointwise(Abs, Pointwise(Log10, Fetch 0))) |> List.exactlyOne
    Assert.Equal(2.0, out.Points[0L], 12)

[<Fact>]
let ``sources lists the queries a node reads`` () =
    Assert.Equal<int list>([ 3 ], sources (Pointwise(Abs, Pointwise(Floor, Fetch 3))))
