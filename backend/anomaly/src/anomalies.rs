use crate::airline::Airline;
use crate::{Band, Direction, Seasonality, basic, stats, stl};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anomalies {
    Basic,
    Agile(Seasonality),
    Robust(Seasonality),
}

/// A point of the window with the band expected for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub time: i64,
    pub value: f64,
    pub band: Band,
}

impl Point {
    pub fn is_anomaly(&self, direction: Direction) -> bool {
        self.band.is_anomaly(self.value, direction)
    }
}

/// How many points before each one the bands look at.
pub const LAG: usize = 60;

/// What an algorithm learned from a series' history. Fitting is the costly half, so a monitor
/// can fit once and `check` every evaluation.
#[derive(Clone, Debug)]
pub struct Model(Fitted);

#[derive(Clone, Debug)]
enum Fitted {
    Basic,
    /// The trend's last level and the season's latest cycle, by phase.
    Robust { season: Seasonality, level: f64, profile: Vec<f64>, sigma: f64 },
    Agile { season: Seasonality, airline: Airline },
}

impl Model {
    /// Fits on the points before `from` (the window's start). None while there is too little
    /// history: three seasons for the seasonal algorithms, with at most a fifth missing.
    pub fn fit(algorithm: Anomalies, points: &[(i64, f64)], from: i64) -> Option<Model> {
        let fitted = match algorithm {
            Anomalies::Basic => Fitted::Basic,
            Anomalies::Robust(season) => {
                let (start, y) = season.resample(points, season.bucket(from), 3)?;
                let d = stl::decompose(season.period(), &y, true)?;
                let mut profile = vec![0.0; season.period()];
                for i in y.len() - season.period()..y.len() {
                    profile[season.phase(start + i as i64 * season.step_ms())] = d.seasonal()[i];
                }
                Fitted::Robust {
                    season,
                    level: *d.trend().last()?,
                    profile,
                    sigma: stats::sd(d.remainder()),
                }
            }
            Anomalies::Agile(season) => {
                let (_, y) = season.resample(points, season.bucket(from), 3)?;
                Fitted::Agile { season, airline: Airline::fit(season.period(), &y)? }
            }
        };
        Some(Model(fitted))
    }

    /// `fit` on a history read at the model's step, for the window from `from` to `to`, as the
    /// query engine fits: agile's three seasons, and its fifth missing at most, are counted up to
    /// `to`, and a gap just before the window is interpolated towards the window's own values;
    /// the airline is then fitted on the buckets before the window's first.
    pub fn fit_window(algorithm: Anomalies, history: &[(i64, f64)], from: i64, to: i64) -> Option<Model> {
        match algorithm {
            Anomalies::Basic | Anomalies::Robust(_) => Model::fit(algorithm, history, from),
            Anomalies::Agile(season) => {
                let (start, y) = season.resample(history, to, 3)?;
                let fit_length = usize::try_from((season.bucket(from) - start) / season.step_ms()).ok()?;
                if fit_length < 3 * season.period() {
                    return None;
                }
                let airline = Airline::fit(season.period(), &y[..fit_length.min(y.len())])?;
                Some(Model(Fitted::Agile { season, airline }))
            }
        }
    }

    /// `check` with agile's expectations run over `recent`, the series at the model's step up to
    /// `to`, rather than over the bucket means of `points`. None when the model expects nothing:
    /// agile without `recent`, or with too little of it (two seasons).
    pub fn check_with(
        &self,
        recent: Option<&[(i64, f64)]>,
        to: i64,
        points: &[(i64, f64)],
        from: i64,
        bounds: f64,
    ) -> Option<Vec<Point>> {
        match &self.0 {
            Fitted::Basic | Fitted::Robust { .. } => Some(self.check(points, from, bounds)),
            Fitted::Agile { season, airline } => {
                let (start, y) = season.resample(recent?, to, 2)?;
                let predicted = airline.predictions(&y);
                let expected = |t: i64| {
                    let i = (season.bucket(t) - start) / season.step_ms();
                    usize::try_from(i).ok().and_then(|i| predicted.get(i).copied().flatten())
                };
                Some(seasonal_band(*season, bounds, airline.sigma2.sqrt(), points, from, expected))
            }
        }
    }

    /// Each point at or after `from` that the model has a band for. `points` reach back far
    /// enough for the bands: `LAG` points before `from`, and for agile two seasons, since it
    /// expects each point from those before it.
    pub fn check(&self, points: &[(i64, f64)], from: i64, bounds: f64) -> Vec<Point> {
        match &self.0 {
            Fitted::Basic => {
                let values: Vec<f64> = points.iter().map(|p| p.1).collect();
                basic::band(LAG, bounds, &values)
                    .into_iter()
                    .zip(points)
                    .filter_map(|(band, &(time, value))| Some(Point { time, value, band: band? }))
                    .filter(|p| p.time >= from)
                    .collect()
            }
            Fitted::Robust { season, level, profile, sigma } => {
                seasonal_band(*season, bounds, *sigma, points, from, |t| Some(level + profile[season.phase(t)]))
            }
            Fitted::Agile { season, airline } => {
                let Some(&(last, _)) = points.last() else { return Vec::new() };
                let Some((start, y)) = season.resample(points, season.bucket(last) + season.step_ms(), 2) else {
                    return Vec::new();
                };
                let predicted = airline.predictions(&y);
                let expected = |t: i64| {
                    let i = (season.bucket(t) - start) / season.step_ms();
                    usize::try_from(i).ok().and_then(|i| predicted.get(i).copied().flatten())
                };
                seasonal_band(*season, bounds, airline.sigma2.sqrt(), points, from, expected)
            }
        }
    }
}

/// `Model::fit` and `Model::check` in one go.
pub fn anomalies(algorithm: Anomalies, points: &[(i64, f64)], from: i64, bounds: f64) -> Vec<Point> {
    match Model::fit(algorithm, points, from) {
        Some(model) => model.check(points, from, bounds),
        None => Vec::new(),
    }
}

/// `bounds` standard deviations around the expected values: the model's spread, from its fit on
/// history, so a lasting anomaly in the window cannot widen the band around itself; combined
/// with the spread inside a model bucket, for points finer than the model (20 s points around a
/// 5-minute expectation), measured from each point's own bucket mean, which a level shift does
/// not move.
///
/// WARNING(undocumented): how Datadog widens a band for points finer than its model.
/// σ² = σ²(model) + σ²(within bucket), the latter over the `LAG` points before each.
fn seasonal_band(
    season: Seasonality,
    bounds: f64,
    model_sigma: f64,
    points: &[(i64, f64)],
    from: i64,
    expected: impl Fn(i64) -> Option<f64>,
) -> Vec<Point> {
    let mut sums = std::collections::HashMap::new();
    for &(t, v) in points {
        let s = sums.entry(season.bucket(t)).or_insert((0.0, 0));
        s.0 += v;
        s.1 += 1;
    }
    let within: Vec<f64> = points
        .iter()
        .map(|&(t, v)| {
            let (sum, n) = sums[&season.bucket(t)];
            v - sum / n as f64
        })
        .collect();

    points
        .iter()
        .enumerate()
        .filter(|(_, p)| p.0 >= from)
        .filter_map(|(i, &(time, value))| {
            let e = expected(time)?;
            let recent = &within[i.saturating_sub(LAG)..i];
            let within_sigma = if recent.len() >= 5 {
                (recent.iter().map(|d| d * d).sum::<f64>() / recent.len() as f64).sqrt()
            } else {
                0.0
            };
            let sigma = (model_sigma * model_sigma + within_sigma * within_sigma).sqrt();
            let band = Band { lower: e - bounds * sigma, upper: e + bounds * sigma };
            Some(Point { time, value, band })
        })
        .collect()
}
