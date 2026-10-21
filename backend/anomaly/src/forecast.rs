use crate::airline::Airline;
use crate::{Band, Seasonality, linear};

pub use crate::linear::LinearModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forecast {
    Linear(LinearModel),
    Seasonal(Seasonality),
}

/// A predicted value with the band around it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Prediction {
    pub time: i64,
    pub value: f64,
    pub band: Band,
}

/// The value expected at each of `future`, `deviations` standard deviations either side.
///
/// `points` are the history the forecast learns from. Linear fits all of them: pass the
/// `history` Datadog's forecast() takes, the window by default. Seasonal "requires at least two
/// seasons of history"; six are read. Empty with too little history.
pub fn forecast(method: Forecast, points: &[(i64, f64)], future: &[i64], deviations: f64) -> Vec<Prediction> {
    let Some(&(last, _)) = points.last() else { return Vec::new() };
    match method {
        Forecast::Linear(model) => linear(model, points, last, future, deviations),
        Forecast::Seasonal(season) => {
            forecast_at(method, points, season.bucket(last) + season.step_ms(), future, deviations)
        }
    }
}

/// `forecast` made at `to`, the end of a window, as the query engine makes it: linear measures
/// time from `to`, and seasonal continues from the bucket holding `to`'s last instant, a gap at
/// the end of the history filled with its last value.
pub fn forecast_at(
    method: Forecast,
    points: &[(i64, f64)],
    to: i64,
    future: &[i64],
    deviations: f64,
) -> Vec<Prediction> {
    match method {
        Forecast::Linear(model) => linear(model, points, to, future, deviations),
        Forecast::Seasonal(season) => seasonal(season, points, to, future, deviations).unwrap_or_default(),
    }
}

fn linear(model: LinearModel, points: &[(i64, f64)], origin: i64, future: &[i64], deviations: f64) -> Vec<Prediction> {
    if points.len() < 2 {
        return Vec::new();
    }
    let seconds = |t: i64| (t - origin) as f64 / 1000.0;
    let xs: Vec<f64> = points.iter().map(|p| seconds(p.0)).collect();
    let ys: Vec<f64> = points.iter().map(|p| p.1).collect();
    let at: Vec<f64> = future.iter().map(|&t| seconds(t)).collect();
    linear::forecast(model, deviations, &xs, &ys, &at)
        .into_iter()
        .zip(future)
        .map(|((value, width), &time)| Prediction { time, value, band: Band { lower: value - width, upper: value + width } })
        .collect()
}

fn seasonal(season: Seasonality, points: &[(i64, f64)], to: i64, future: &[i64], deviations: f64) -> Option<Vec<Prediction>> {
    let step = season.step_ms();
    let last = season.bucket(to - 1);
    let (_, y) = season.resample(points, to, 2)?;
    let ahead = |t: i64| (season.bucket(t) - last) / step;
    let h = future.iter().map(|&t| ahead(t)).max().filter(|&h| h > 0)?;
    let predicted = Airline::fit(season.period(), &y)?.forecast(&y, h as usize);

    Some(
        future
            .iter()
            .filter(|&&t| ahead(t) > 0)
            .map(|&time| {
                let (value, se) = predicted[ahead(time) as usize - 1];
                let band = Band { lower: value - deviations * se, upper: value + deviations * se };
                Prediction { time, value, band }
            })
            .collect(),
    )
}
