/// Seasonal time-series models behind the agile and robust anomaly
/// algorithms and the seasonal forecast: STL decomposition and the airline
/// SARIMA. Plain functions over evenly spaced arrays with no gaps.
///
/// Datadog describes its robust algorithm as "a seasonal-trend decomposition"
/// and agile as "a robust version of the SARIMA algorithm", without saying
/// which ones. We take the textbook versions with their reference defaults,
/// and test them against statsmodels (tests/Engine.Tests/Fixtures).
module NinjaCat.Api.Engine.Query.Models

open System
open MathNet.Numerics.LinearAlgebra
open MathNet.Numerics.Optimization

// --- STL ------------------------------------------------------------------------------

/// STL as the robust algorithm uses it (see Stl.fs, a port of statsmodels').
type Stl = Stl.Decomposition

/// Robust STL with statsmodels' defaults, and R stl()'s jumps: each smoother
/// evaluated every ⌈width/10⌉ points and interpolated between, as Cleveland
/// et al. recommend.
///
/// The jumps are for speed: evaluating every point, a daily season (1728
/// points) takes ~270 ms per series; with jumps, a fraction of that, for a
/// seasonal component differing by under 1e-4 of its amplitude.
///
/// WARNING(undocumented): Datadog's decomposition and its settings. These are
/// the reference defaults of statsmodels and R.
let stl (period: int) (values: float[]) : Stl =
    let d = Stl.robustDefaults period
    let jump width = int (Math.Ceiling(float width / 10.0))

    Stl.decompose
        { d with
            SeasonalJump = jump d.Seasonal
            TrendJump = jump d.Trend
            LowPassJump = jump d.LowPass }
        values

// --- the airline SARIMA -----------------------------------------------------------------

/// SARIMA(0,1,1)(0,1,1)s — Box & Jenkins' "airline model", the textbook
/// default for seasonal series:
///
///   (1 − B)(1 − B^s) y_t = (1 + θB)(1 + ΘB^s) e_t
///
/// Two differences take out trend and season; two moving-average terms
/// correct by the last error (θ) and the error one season ago (Θ).
///
/// WARNING(undocumented): Datadog's SARIMA orders and fitting. These are the
/// airline orders, fitted by conditional sum of squares (R's arima() default
/// "CSS" part) with Nelder–Mead, θ and Θ kept in (−1, 1).
type Airline =
    { Season: int
      Theta: float
      SeasonalTheta: float
      /// Variance of the one-step errors.
      Sigma2: float }

/// The one-step errors e_t for given parameters, conditional on zero errors
/// before the first difference is defined.
let private errors (s: int) (theta: float) (seasonal: float) (y: float[]) : float[] =
    let e = Array.zeroCreate y.Length

    for t in s + 1 .. y.Length - 1 do
        let w = y[t] - y[t - 1] - y[t - s] + y[t - s - 1]
        e[t] <- w - theta * e[t - 1] - seasonal * e[t - s] - theta * seasonal * e[t - s - 1]

    e

/// Fits θ and Θ on `y` (at least two seasons and a point).
let fitAirline (s: int) (y: float[]) : Airline =
    // tanh keeps both parameters inside (−1, 1) without a constrained solver.
    let objective (v: Vector<float>) =
        errors s (tanh v[0]) (tanh v[1]) y |> Array.sumBy (fun x -> x * x)

    let result =
        NelderMeadSimplex(1e-10, 2000).FindMinimum(ObjectiveFunction.Value objective, Vector<float>.Build.Dense [| 0.0; 0.0 |])

    let theta, seasonal = tanh result.MinimizingPoint[0], tanh result.MinimizingPoint[1]
    let e = errors s theta seasonal y
    let used = y.Length - s - 1

    { Season = s
      Theta = theta
      SeasonalTheta = seasonal
      Sigma2 = (e |> Array.sumBy (fun x -> x * x)) / float (max 1 (used - 2)) }

/// One-step predictions ŷ_t: each point as the model expected it from the
/// points before. None where the model is not yet defined.
///
/// Robust filtering — the "robust" in Datadog's "robust version of the SARIMA
/// algorithm": an error fed back into the moving-average terms is clipped to
/// ±2σ (a Huber ψ), so one large surprise cannot swing the next predictions.
/// Without it, a model fitted on smooth data (θ near 1) answers a level shift
/// of +20 with predictions alternating ±20 around it for the rest of the
/// window. The shift itself is still followed at once: every prediction
/// starts from the last actual value.
///
/// WARNING(undocumented): how Datadog makes its SARIMA robust. Clipping at 2σ
/// is ours, a common choice for robust filters.
let predictions (m: Airline) (y: float[]) : float option[] =
    let s = m.Season
    let limit = 2.0 * sqrt m.Sigma2
    let clipped = Array.zeroCreate y.Length
    let p = Array.create y.Length None

    for t in s + 1 .. y.Length - 1 do
        let expected =
            y[t - 1] + y[t - s] - y[t - s - 1]
            + m.Theta * clipped[t - 1]
            + m.SeasonalTheta * clipped[t - s]
            + m.Theta * m.SeasonalTheta * clipped[t - s - 1]

        p[t] <- Some expected
        clipped[t] <- max -limit (min limit (y[t] - expected))

    p

/// The next `h` values after `y`, with their standard errors. Future errors
/// are zero, and the variance is σ²·Σψ², ψ the model's impulse response.
let forecastAirline (m: Airline) (y: float[]) (h: int) : (float * float)[] =
    let s = m.Season
    let n = y.Length
    let e = Array.append (errors s m.Theta m.SeasonalTheta y) (Array.zeroCreate h)
    let x = Array.append y (Array.zeroCreate h)

    for t in n .. n + h - 1 do
        x[t] <-
            x[t - 1] + x[t - s] - x[t - s - 1]
            + m.Theta * e[t - 1]
            + m.SeasonalTheta * e[t - s]
            + m.Theta * m.SeasonalTheta * e[t - s - 1]

    // ψ: the response of y to a single unit error at time 0.
    let psi = Array.zeroCreate h
    let ys = Array.zeroCreate (h + s + 2)
    let es = Array.zeroCreate (h + s + 2)
    let o = s + 1 // offset so indices t − s − 1 stay in range
    es[o] <- 1.0

    for j in 0 .. h - 1 do
        let t = o + j
        ys[t] <- ys[t - 1] + ys[t - s] - ys[t - s - 1] + es[t] + m.Theta * es[t - 1] + m.SeasonalTheta * es[t - s] + m.Theta * m.SeasonalTheta * es[t - s - 1]
        psi[j] <- ys[t]

    let cumulative = psi |> Array.scan (fun acc p -> acc + p * p) 0.0 |> Array.tail
    Array.init h (fun k -> x[n + k], sqrt (m.Sigma2 * cumulative[k]))
