module NinjaCat.Api.Engine.Tests.AlgorithmsTests

open Xunit
open NinjaCat.Api.Engine.Query.Algorithms

let private xs = Array.init 10 float

[<Fact>]
let ``ols recovers an exact line`` () =
    let slope, intercept = ols xs (xs |> Array.map (fun x -> 3.0 + 2.0 * x))
    Assert.Equal(2.0, slope, 9)
    Assert.Equal(3.0, intercept, 9)

[<Fact>]
let ``huber shrugs off a spike that bends least squares`` () =
    // A flat line at 5 with one spike to 105 at the end.
    let ys = Array.init 10 (fun i -> if i = 9 then 105.0 else 5.0)
    let olsSlope, _ = ols xs ys
    let huberSlope, huberIntercept = huber xs ys
    Assert.True(olsSlope > 5.0, $"least squares slope {olsSlope}")
    Assert.Equal(0.0, huberSlope, 6)
    Assert.Equal(5.0, huberIntercept, 6)

[<Fact>]
let ``piecewise constant finds a level shift`` () =
    let noisy = [| 10.1; 9.9; 10.0; 10.2; 9.8; 20.1; 19.9; 20.0; 20.2; 19.8 |]
    Assert.Equal<float[]>([| 10.0; 10.0; 10.0; 10.0; 10.0; 20.0; 20.0; 20.0; 20.0; 20.0 |], piecewiseConstant noisy |> Array.map (fun v -> round (v * 1000.0) / 1000.0))

[<Fact>]
let ``piecewise constant leaves one level as one segment`` () =
    let flat = [| 5.1; 4.9; 5.0; 5.2; 4.8; 5.1; 4.9; 5.0 |]
    Assert.Equal(1, piecewiseConstant flat |> Array.distinct |> Array.length)

[<Fact>]
let ``autosmooth smooths noise and keeps a step`` () =
    // Alternating noise around 10, then a clear step up to 30.
    let ys = Array.init 80 (fun i -> (if i < 40 then 10.0 else 30.0) + (if i % 2 = 0 then 1.0 else -1.0))
    let w = autosmoothWindow 40 ys
    Assert.True(w > 1, $"window {w}")
    let smoothed = movingAverage w ys
    // Well inside each level the noise is gone, and the levels are kept.
    Assert.InRange(smoothed[30], 9.5, 10.5)
    Assert.InRange(smoothed[75], 29.5, 30.5)

[<Fact>]
let ``dbscan picks out the series far from the rest`` () =
    let rows = [| [| 1.0; 1.0; 1.0 |]; [| 1.1; 0.9; 1.0 |]; [| 0.9; 1.1; 1.0 |]; [| 1.0; 1.0; 1.2 |]; [| 9.0; 9.0; 9.0 |] |]
    Assert.Equal<bool[]>([| false; false; false; false; true |], dbscanOutliers false 3.0 rows)

[<Fact>]
let ``mad flags a series only past its percentage`` () =
    // Series 3 is far off at one time out of four (25 %). The others vary a
    // little at every time, so the MAD is never zero.
    let rows =
        [| [| Some 1.0; Some 1.2; Some 0.8; Some 1.1 |]
           [| Some 1.2; Some 0.9; Some 1.1; Some 0.8 |]
           [| Some 0.8; Some 1.1; Some 1.0; Some 1.2 |]
           [| Some 1.1; Some 1.0; Some 50.0; Some 0.9 |] |]

    Assert.Equal<bool[]>([| false; false; false; true |], madOutliers false 3.0 20.0 rows)
    Assert.Equal<bool[]>([| false; false; false; false |], madOutliers false 3.0 30.0 rows)

[<Fact>]
let ``too few series have no outliers`` () =
    Assert.Equal<bool[]>([| false; false |], dbscanOutliers false 1.0 [| [| 1.0 |]; [| 100.0 |] |])

[<Fact>]
let ``unscaled MAD with identical peers flags any deviation`` () =
    // At a time where three series agree exactly, the MAD is zero, and a
    // fourth that differs at all is out by infinitely many MADs.
    let rows = [| [| Some 1.0 |]; [| Some 1.0 |]; [| Some 1.0 |]; [| Some 1.01 |] |]
    Assert.Equal<bool[]>([| false; false; false; true |], madOutliers false 3.0 0.0 rows)
    // Scaled MAD floors the deviation at 1 % of the median: 1.01 is within it.
    Assert.Equal<bool[]>([| false; false; false; false |], madOutliers true 3.0 0.0 rows)

[<Fact>]
let ``piecewise constant is not fooled by a mostly flat integer series`` () =
    // 37 with single blips, like a goroutine count: one segment, not one per blip.
    let ys = [| 37.0; 37.0; 38.0; 37.0; 37.0; 37.0; 39.0; 37.0; 37.0; 37.0; 37.0; 38.0; 37.0; 37.0 |]
    Assert.Equal(1, piecewiseConstant ys |> Array.distinct |> Array.length)

[<Fact>]
let ``the basic band follows the recent level`` () =
    // Around 10 (±1) for 20 points: the band for the next is centred near 10.
    let values = Array.init 21 (fun i -> 10.0 + (if i % 2 = 0 then 1.0 else -1.0))
    let band = basicBand 60 2.0 values
    Assert.Equal(None, band[3]) // under 5 points of history
    let lo, hi = band[20].Value
    Assert.InRange((lo + hi) / 2.0, 9.0, 11.0)
    Assert.True(hi - lo > 0.0)

[<Fact>]
let ``a linear forecast extends an exact line`` () =
    let xs = Array.init 20 (fun i -> float (i - 20) * 10.0) // the last 200 s
    let ys = xs |> Array.map (fun x -> 100.0 + 0.5 * x)
    for model in [ "default"; "simple"; "reactive" ] do
        let predicted = linearForecast model 1.0 xs ys [| 0.0; 60.0 |]
        Assert.Equal(100.0, predicted[0].Value, 6)
        Assert.Equal(130.0, predicted[1].Value, 6)
        Assert.Equal(predicted[0].Value, predicted[0].Lower, 6) // no residuals, no band
        Assert.Equal(predicted[0].Value, predicted[0].Upper, 6)

[<Fact>]
let ``the basic band does not collapse on a mostly constant integer series`` () =
    // Mostly 37, now and then 38–40, like a goroutine count. A 38 is normal.
    let history = [| 37.0; 37.0; 38.0; 37.0; 39.0; 37.0; 37.0; 38.0; 37.0; 40.0; 37.0; 37.0 |]
    let band = basicBand 60 2.0 (Array.append history [| 38.0 |])
    let lo, hi = band[history.Length].Value
    Assert.True(lo < 37.0 && hi > 38.0, $"band [{lo}, {hi}]")

[<Fact>]
let ``bounds are standard deviations`` () =
    // Past values 8 and 12 alternating: median 10, standard deviation 2.
    let past = Array.init 10 (fun i -> if i % 2 = 0 then 8.0 else 12.0)
    let band = basicBand 60 3.0 (Array.append past [| 10.0 |])
    let lo, hi = band[10].Value
    Assert.Equal(4.0, lo, 9)
    Assert.Equal(16.0, hi, 9)
