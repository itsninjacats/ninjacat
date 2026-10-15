/// STL and the airline SARIMA against statsmodels, on the fixed series in
/// Fixtures/ (see Fixtures/README.md for how they were made).
module NinjaCat.Api.Engine.Tests.ModelsTests

open System.IO
open System.Text.Json
open Xunit
open NinjaCat.Api.Engine.Query.Models

let private fixture name =
    JsonDocument.Parse(File.ReadAllText(Path.Combine(System.AppContext.BaseDirectory, "Fixtures", name))).RootElement

let private numbers (e: JsonElement) = [| for x in e.EnumerateArray() -> x.GetDouble() |]
let private maxDiff (a: float[]) (b: float[]) = Array.map2 (fun x y -> abs (x - y)) a b |> Array.max

[<Fact>]
let ``the STL port is statsmodels' STL`` () =
    let r = fixture "stl.json"
    let d = NinjaCat.Api.Engine.Query.Stl.decompose (NinjaCat.Api.Engine.Query.Stl.robustDefaults 24) (numbers (r.GetProperty "y"))
    // Measured 1e-13: the same algorithm, step for step.
    Assert.InRange(maxDiff d.Trend (numbers (r.GetProperty "trend")), 0.0, 1e-9)
    Assert.InRange(maxDiff d.Seasonal (numbers (r.GetProperty "seasonal")), 0.0, 1e-9)
    Assert.InRange(maxDiff d.Remainder (numbers (r.GetProperty "resid")), 0.0, 1e-9)

[<Fact>]
let ``STL with R's jumps stays close to it`` () =
    let r = fixture "stl.json"
    let d = stl 24 (numbers (r.GetProperty "y"))
    Assert.InRange(maxDiff d.Trend (numbers (r.GetProperty "trend")), 0.0, 0.01)
    Assert.InRange(maxDiff d.Seasonal (numbers (r.GetProperty "seasonal")), 0.0, 0.01)

[<Fact>]
let ``the airline fit agrees with statsmodels`` () =
    let r = fixture "sarima.json"
    let m = fitAirline 24 (numbers (r.GetProperty "y"))
    let fit = r.GetProperty "fit"
    // Conditional sum of squares against statsmodels' exact likelihood.
    // Measured: θ within 0.004, Θ within 0.002.
    Assert.InRange(abs (fit.GetProperty("theta").GetDouble() - m.Theta), 0.0, 0.01)
    Assert.InRange(abs (fit.GetProperty("Theta").GetDouble() - m.SeasonalTheta), 0.0, 0.01)
    Assert.InRange(m.Sigma2 / fit.GetProperty("sigma2").GetDouble(), 0.95, 1.05)

[<Fact>]
let ``with statsmodels' parameters, the forecast is statsmodels' forecast`` () =
    let r = fixture "sarima.json"
    let fit = r.GetProperty "fit"

    let m =
        { Season = 24
          Theta = fit.GetProperty("theta").GetDouble()
          SeasonalTheta = fit.GetProperty("Theta").GetDouble()
          Sigma2 = fit.GetProperty("sigma2").GetDouble() }

    let forecast = forecastAirline m (numbers (r.GetProperty "y")) 24
    let expected = r.GetProperty "forecast"
    Assert.InRange(maxDiff (Array.map fst forecast) (numbers (expected.GetProperty "mean")), 0.0, 1e-3)
    Assert.InRange(maxDiff (Array.map snd forecast) (numbers (expected.GetProperty "se")), 0.0, 1e-6)

[<Fact>]
let ``one-step predictions follow a level shift`` () =
    // A seasonal pattern, then everything 10 higher: the prediction catches
    // up within a few points, as agile must.
    let s = 12
    let y = Array.init (s * 10) (fun t -> sin (2.0 * System.Math.PI * float t / float s) + (if t >= s * 8 then 10.0 else 0.0))
    let p = predictions (fitAirline s y) y
    let t = s * 8 + 3
    Assert.InRange(abs (y[t] - p[t].Value), 0.0, 2.0)
