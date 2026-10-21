use argmin::core::{CostFunction, Executor, State};
use argmin::solver::neldermead::NelderMead;

/// SARIMA(0,1,1)(0,1,1)s — Box & Jenkins' "airline model", the textbook default for seasonal
/// series:
///
///   (1 − B)(1 − B^s) y_t = (1 + θB)(1 + ΘB^s) e_t
///
/// Two differences take out trend and season; two moving-average terms correct by the last
/// error (θ) and the error one season ago (Θ).
///
/// WARNING(undocumented): Datadog calls agile "a robust version of the SARIMA algorithm" without
/// orders or fitting. These are the airline orders, fitted by conditional sum of squares (R's
/// arima() "CSS") with Nelder–Mead, θ and Θ kept in (−1, 1).
#[derive(Clone, Debug)]
pub struct Airline {
    pub season: usize,
    pub theta: f64,
    pub seasonal_theta: f64,
    /// Variance of the one-step errors.
    pub sigma2: f64,
}

/// The one-step errors for given parameters, conditional on zero errors before the first
/// difference is defined.
fn errors(s: usize, theta: f64, seasonal: f64, y: &[f64]) -> Vec<f64> {
    let mut e = vec![0.0; y.len()];
    for t in s + 1..y.len() {
        let w = y[t] - y[t - 1] - y[t - s] + y[t - s - 1];
        e[t] = w - theta * e[t - 1] - seasonal * e[t - s] - theta * seasonal * e[t - s - 1];
    }
    e
}

struct SumOfSquares<'a> {
    season: usize,
    y: &'a [f64],
}

impl CostFunction for SumOfSquares<'_> {
    type Param = Vec<f64>;
    type Output = f64;

    // tanh keeps both parameters inside (−1, 1) without a constrained solver.
    fn cost(&self, v: &Vec<f64>) -> Result<f64, argmin::core::Error> {
        Ok(errors(self.season, v[0].tanh(), v[1].tanh(), self.y).iter().map(|e| e * e).sum())
    }
}

impl Airline {
    /// Fits θ and Θ on `y`. None with less than a season and two points, where there is no
    /// error to fit; the callers ask for two seasons or three.
    pub fn fit(s: usize, y: &[f64]) -> Option<Airline> {
        if y.len() < s + 2 {
            return None;
        }
        let simplex = vec![vec![0.0, 0.0], vec![0.1, 0.0], vec![0.0, 0.1]];
        let solver = NelderMead::new(simplex).with_sd_tolerance(1e-10).ok()?;
        let result = Executor::new(SumOfSquares { season: s, y }, solver)
            .configure(|state| state.max_iters(2000))
            .run()
            .ok()?;
        let best = result.state().get_best_param()?;
        let (theta, seasonal_theta) = (best[0].tanh(), best[1].tanh());

        let sum: f64 = errors(s, theta, seasonal_theta, y).iter().map(|e| e * e).sum();
        let used = y.len() - s - 1;
        Some(Airline { season: s, theta, seasonal_theta, sigma2: sum / used.saturating_sub(2).max(1) as f64 })
    }

    /// One-step predictions: each point as the model expected it from the points before. None
    /// where the model is not yet defined.
    ///
    /// The "robust" in "robust version of SARIMA": an error fed back into the moving-average
    /// terms is clipped to ±2σ (a Huber ψ), so one large surprise cannot swing the next
    /// predictions. Without it, a model fitted on smooth data (θ near 1) answers a level shift
    /// of +20 with predictions alternating ±20 around it. The shift itself is still followed at
    /// once: every prediction starts from the last actual value.
    ///
    /// WARNING(undocumented): how Datadog makes its SARIMA robust. Clipping at 2σ is ours, a
    /// common choice for robust filters.
    pub fn predictions(&self, y: &[f64]) -> Vec<Option<f64>> {
        let s = self.season;
        let limit = 2.0 * self.sigma2.sqrt();
        let mut clipped = vec![0.0; y.len()];
        let mut p = vec![None; y.len()];
        for t in s + 1..y.len() {
            let expected = y[t - 1] + y[t - s] - y[t - s - 1]
                + self.theta * clipped[t - 1]
                + self.seasonal_theta * clipped[t - s]
                + self.theta * self.seasonal_theta * clipped[t - s - 1];
            p[t] = Some(expected);
            clipped[t] = (y[t] - expected).clamp(-limit, limit);
        }
        p
    }
}

impl Airline {
    /// The next `h` values after `y`, with their standard errors. Future errors are zero, and
    /// the variance is σ²·Σψ², ψ the model's impulse response.
    pub fn forecast(&self, y: &[f64], h: usize) -> Vec<(f64, f64)> {
        let (s, theta, seasonal) = (self.season, self.theta, self.seasonal_theta);
        let n = y.len();
        let mut e = errors(s, theta, seasonal, y);
        e.resize(n + h, 0.0);
        let mut x = y.to_vec();
        x.resize(n + h, 0.0);
        for t in n..n + h {
            x[t] = x[t - 1] + x[t - s] - x[t - s - 1]
                + theta * e[t - 1]
                + seasonal * e[t - s]
                + theta * seasonal * e[t - s - 1];
        }

        // ψ: the response of y to a single unit error at time 0, offset so t − s − 1 stays in range.
        let o = s + 1;
        let mut ys = vec![0.0; h + s + 2];
        let mut es = vec![0.0; h + s + 2];
        es[o] = 1.0;
        let mut variance = 0.0;
        (0..h)
            .map(|k| {
                let t = o + k;
                ys[t] = ys[t - 1] + ys[t - s] - ys[t - s - 1]
                    + es[t]
                    + theta * es[t - 1]
                    + seasonal * es[t - s]
                    + theta * seasonal * es[t - s - 1];
                variance += ys[t] * ys[t];
                (x[n + k], (self.sigma2 * variance).sqrt())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{fixture, max_diff, numbers};

    #[test]
    fn agrees_with_statsmodels() {
        // Conditional sum of squares against statsmodels' exact likelihood.
        let r = fixture("sarima.json");
        let m = Airline::fit(24, &numbers(&r["y"])).unwrap();
        let fit = &r["fit"];
        assert!((fit["theta"].as_f64().unwrap() - m.theta).abs() < 0.01, "{m:?}");
        assert!((fit["Theta"].as_f64().unwrap() - m.seasonal_theta).abs() < 0.01, "{m:?}");
        assert!((0.95..1.05).contains(&(m.sigma2 / fit["sigma2"].as_f64().unwrap())), "{m:?}");
    }

    #[test]
    fn with_statsmodels_parameters_the_forecast_is_statsmodels_forecast() {
        let r = fixture("sarima.json");
        let fit = &r["fit"];
        let m = Airline {
            season: 24,
            theta: fit["theta"].as_f64().unwrap(),
            seasonal_theta: fit["Theta"].as_f64().unwrap(),
            sigma2: fit["sigma2"].as_f64().unwrap(),
        };
        let f = m.forecast(&numbers(&r["y"]), 24);
        let mean: Vec<f64> = f.iter().map(|p| p.0).collect();
        let se: Vec<f64> = f.iter().map(|p| p.1).collect();
        assert!(max_diff(&mean, &numbers(&r["forecast"]["mean"])) < 1e-3);
        assert!(max_diff(&se, &numbers(&r["forecast"]["se"])) < 1e-6);
    }

    #[test]
    fn predictions_follow_a_level_shift() {
        let s = 12;
        let y: Vec<f64> = (0..s * 10)
            .map(|t| (2.0 * std::f64::consts::PI * t as f64 / s as f64).sin() + if t >= s * 8 { 10.0 } else { 0.0 })
            .collect();
        let p = Airline::fit(s, &y).unwrap().predictions(&y);
        let t = s * 8 + 3;
        assert!((y[t] - p[t].unwrap()).abs() < 2.0);
    }
}
