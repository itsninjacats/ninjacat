//! The algorithmic functions of the metric query language: `anomalies()` and `forecast()` over one
//! series, `outliers()` over a group of them.
//!
//! Times are milliseconds since the epoch; points are `(time, value)`, oldest first.
//!
//! ```
//! use ninjapaws_anomaly::{Anomalies, Direction, Seasonality, anomalies};
//!
//! # let points: Vec<(i64, f64)> = (0..240).map(|m| (m * 60_000, (m % 60) as f64)).collect();
//! # let from = 200 * 60_000;
//! // `points` reach three seasons back from the window starting at `from`.
//! let checked = anomalies(Anomalies::Agile(Seasonality::Hourly), &points, from, 2.0);
//! for p in checked.iter().filter(|p| p.is_anomaly(Direction::Both)) {
//!     println!("{} = {} outside [{}, {}]", p.time, p.value, p.band.lower, p.band.upper);
//! }
//! ```

mod airline;
mod anomalies;
mod basic;
mod forecast;
mod linear;
mod outliers;
mod seasonality;
mod smooth;
mod stats;
mod stl;

pub use anomalies::{Anomalies, LAG, Model, Point, anomalies};
pub use forecast::{Forecast, LinearModel, Prediction, forecast, forecast_at};
pub use outliers::{Outliers, outliers};
pub use linear::{huber, ols};
pub use seasonality::Seasonality;
pub use smooth::{autosmooth_window, moving_average, piecewise_constant};
pub use stats::median;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Both,
    Above,
    Below,
}

/// The range a point is expected in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub lower: f64,
    pub upper: f64,
}

impl Band {
    pub fn is_anomaly(&self, value: f64, direction: Direction) -> bool {
        match direction {
            Direction::Both => value < self.lower || value > self.upper,
            Direction::Above => value > self.upper,
            Direction::Below => value < self.lower,
        }
    }
}

#[cfg(test)]
mod fixture {
    use serde_json::Value;

    /// statsmodels' reference outputs (tests/fixtures/README.md).
    pub fn fixture(name: &str) -> Value {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/");
        serde_json::from_str(&std::fs::read_to_string(format!("{path}{name}")).unwrap()).unwrap()
    }

    pub fn numbers(v: &Value) -> Vec<f64> {
        v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
    }

    pub fn max_diff(a: &[f64], b: &[f64]) -> f64 {
        a.iter().zip(b).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
    }
}
