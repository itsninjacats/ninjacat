module NinjaCat.Api.Intake.Tests.DDSketchTests

open System
open Xunit
open NinjaCat.Api.Intake

/// Keys the agent itself produces for these values.
[<Theory>]
[<InlineData(11.9, 1498)>]
[<InlineData(12.1, 1499)>]
[<InlineData(47.3, 1587)>]
[<InlineData(890.0, 1776)>]
[<InlineData(1150.0, 1793)>]
let ``keys match the agent's`` (value: float, key: int) = Assert.Equal(key, DDSketch.key value)

[<Fact>]
let ``constants are the agent's`` () =
    Assert.Equal(1.015625, DDSketch.gamma)
    Assert.Equal(1338, DDSketch.bias)

[<Fact>]
let ``a bucket's value is its middle, not its floor`` () =
    let k = DDSketch.key 1150.0
    Assert.True(DDSketch.value k > 1150.0)
    let lo, hi = DDSketch.bounds k
    Assert.True(1150.0 >= lo && 1150.0 < hi)

[<Fact>]
let ``relative error stays within eps and bounds contain the value`` () =
    let mutable v = 0.001

    while v < 1e6 do
        let k = DDSketch.key v
        Assert.True(abs (DDSketch.value k - v) / v <= DDSketch.eps, $"value {v}")
        let lo, hi = DDSketch.bounds k
        Assert.True(v >= lo && v < hi, $"{v} outside bounds of {k}")
        v <- v * 1.3

[<Fact>]
let ``key and value round-trip`` () =
    for k in 1200..7..1999 do
        Assert.Equal(k, DDSketch.key (DDSketch.value k))

[<Fact>]
let ``build tallies values into sorted buckets`` () =
    let values = [| 11.9; 12.1; 12.0; 12.2; 47.3; 47.0; 46.9; 48.1; 51.0; 890.0; 895.0; 1150.0 |]
    let keys, counts, stats = DDSketch.build values

    Assert.Equal(keys.Length, counts.Length)
    Assert.Equal<int32[]>(Array.sort keys, keys)
    Assert.Equal(keys.Length, (Array.distinct keys).Length)
    Assert.Equal(12L, stats.Count)
    Assert.Equal(12u, Array.sum counts)
    Assert.Equal(11.9, stats.Min)
    Assert.Equal(1150.0, stats.Max)
    Assert.True(abs (stats.Sum - 3223.5) < 1e-9)
    Assert.True(abs (stats.Avg - 268.625) < 1e-9)

[<Fact>]
let ``build of nothing is empty`` () =
    let keys, counts, stats = DDSketch.build [||]
    Assert.Empty keys
    Assert.Empty counts
    Assert.Equal(0L, stats.Count)
