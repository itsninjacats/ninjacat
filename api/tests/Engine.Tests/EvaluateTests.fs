module NinjaCat.Api.Engine.Tests.EvaluateTests

open Xunit
open NinjaCat.Api.Engine.Api.V2.Timeseries
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

// --- across series -------------------------------------------------------------------

let private s name (points: (int64 * float) list) = { GroupTags = [ name ]; Points = Map.ofList points }

let private names (out: Series list) = out |> List.map (_.GroupTags >> List.head)

/// Three series built so each ranking picks a different winner:
///   a: 1, 1, 10    max 10, mean 4,   sum 12, last 10, l2norm √102 ≈ 10.1
///   b: 5, 5, 5     max 5,  mean 5,   sum 15, last 5,  l2norm √75  ≈ 8.7
///   c: -9, 8, 0    max 8,  mean -1/3, sum -1, last 0,  l2norm √145 ≈ 12.0, min -9
let private abc =
    [ s "a" [ 0L, 1.0; 20000L, 1.0; 40000L, 10.0 ]
      s "b" [ 0L, 5.0; 20000L, 5.0; 40000L, 5.0 ]
      s "c" [ 0L, -9.0; 20000L, 8.0; 40000L, 0.0 ] ]

let private topOf n by order = eval (Map [ 0, abc ]) (Top(Fetch 0, n, by, order)) |> names

[<Fact>]
let ``each ranking picks its own winner`` () =
    Assert.Equal<string list>([ "a" ], topOf 1 ByMax Desc)
    Assert.Equal<string list>([ "b" ], topOf 1 ByMean Desc)
    Assert.Equal<string list>([ "b" ], topOf 1 BySum Desc)
    Assert.Equal<string list>([ "a" ], topOf 1 ByLast Desc)
    Assert.Equal<string list>([ "c" ], topOf 1 ByL2norm Desc)
    Assert.Equal<string list>([ "c" ], topOf 1 ByMin Asc)
    Assert.Equal<string list>([ "c" ], topOf 1 ByArea Asc) // signed: c's area is negative

[<Fact>]
let ``top keeps the order and the count`` () =
    Assert.Equal<string list>([ "b"; "a"; "c" ], topOf 5 ByMean Desc)
    Assert.Equal<string list>([ "c"; "a" ], topOf 2 ByMean Asc)

[<Fact>]
let ``a series with no points goes last either way`` () =
    let series = [ s "empty" []; s "one" [ 0L, 1.0 ] ]
    Assert.Equal<string list>([ "one"; "empty" ], eval (Map [ 0, series ]) (Top(Fetch 0, 5, ByMean, Asc)) |> names)
    Assert.Equal<string list>([ "one"; "empty" ], eval (Map [ 0, series ]) (Top(Fetch 0, 5, ByMean, Desc)) |> names)

[<Fact>]
let ``count_nonzero and count_not_null count series per time`` () =
    // At t=0: a=1, b=5, c=-9 → 3 non-zero. At 40000: a=10, b=5, c=0 → 2 non-zero, 3 not null.
    let nonzero = eval (Map [ 0, abc ]) (CountNonzero(Fetch 0)) |> List.exactlyOne
    let notNull = eval (Map [ 0, abc ]) (CountNotNull(Fetch 0)) |> List.exactlyOne
    Assert.Empty nonzero.GroupTags
    Assert.Equal<Map<int64, float>>(Map [ 0L, 3.0; 20000L, 3.0; 40000L, 2.0 ], nonzero.Points)
    Assert.Equal<Map<int64, float>>(Map [ 0L, 3.0; 20000L, 3.0; 40000L, 3.0 ], notNull.Points)

[<Fact>]
let ``count_not_null counts gaps out`` () =
    let series = [ s "a" [ 0L, 1.0; 20000L, 1.0 ]; s "b" [ 20000L, 0.0 ] ]
    let out = eval (Map [ 0, series ]) (CountNotNull(Fetch 0)) |> List.exactlyOne
    Assert.Equal<Map<int64, float>>(Map [ 0L, 1.0; 20000L, 2.0 ], out.Points)

[<Fact>]
let ``counting nothing is no series`` () =
    Assert.Empty(eval (Map [ 0, [] ]) (CountNonzero(Fetch 0)))

[<Fact>]
let ``exclude_null drops groups with any N/A`` () =
    let series =
        [ { GroupTags = [ "host:a"; "role:web" ]; Points = Map [ 0L, 1.0 ] }
          { GroupTags = [ "host:b"; "role:N/A" ]; Points = Map [ 0L, 1.0 ] }
          { GroupTags = [ "host:N/A"; "role:db" ]; Points = Map [ 0L, 1.0 ] } ]

    let kept = eval (Map [ 0, series ]) (ExcludeNull(Fetch 0))
    Assert.Equal<string list list>([ [ "host:a"; "role:web" ] ], kept |> List.map _.GroupTags)
