use std::collections::BTreeMap;

/// The season a model repeats over.
///
/// WARNING(undocumented): the step Datadog fits seasonal models at. We use its line-graph steps
/// for the matching windows, so a season is 60, 288 or 168 points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Seasonality {
    Hourly,
    Daily,
    Weekly,
}

impl Seasonality {
    pub fn step_ms(self) -> i64 {
        match self {
            Seasonality::Hourly => 60_000,
            Seasonality::Daily => 300_000,
            Seasonality::Weekly => 3_600_000,
        }
    }

    /// Points per season.
    pub fn period(self) -> usize {
        match self {
            Seasonality::Hourly => 60,
            Seasonality::Daily => 288,
            Seasonality::Weekly => 168,
        }
    }

    pub(crate) fn bucket(self, t: i64) -> i64 {
        t.div_euclid(self.step_ms()) * self.step_ms()
    }

    /// Counted from the epoch, so a profile fitted once lines up with any later window.
    pub(crate) fn phase(self, t: i64) -> usize {
        t.div_euclid(self.step_ms()).rem_euclid(self.period() as i64) as usize
    }

    /// The points before `to` as the model's input: the mean of each bucket at the model's step,
    /// from the first point's bucket on, gaps filled by linear interpolation. With the first
    /// bucket's time. None with fewer than `seasons` seasons or more than a fifth missing.
    ///
    /// WARNING(undocumented): how Datadog treats gaps in the history it fits. Short gaps are
    /// interpolated; mostly-empty history gives no model.
    pub(crate) fn resample(self, points: &[(i64, f64)], to: i64, seasons: usize) -> Option<(i64, Vec<f64>)> {
        let step = self.step_ms();
        let mut buckets = BTreeMap::new();
        for &(t, v) in points.iter().filter(|p| p.0 < to) {
            let b = buckets.entry(self.bucket(t)).or_insert((0.0, 0));
            b.0 += v;
            b.1 += 1;
        }
        let start = *buckets.keys().next()?;
        let len = ((to - start + step - 1) / step) as usize;
        if len < seasons * self.period() || (buckets.len() as f64) < 0.8 * len as f64 {
            return None;
        }

        let mut known = vec![None; len];
        for (t, (sum, n)) in buckets {
            known[((t - start) / step) as usize] = Some(sum / n as f64);
        }
        let mut y = vec![0.0; len];
        let mut before: Option<(usize, f64)> = None;
        for i in 0..len {
            match known[i] {
                Some(v) => {
                    y[i] = v;
                    before = Some((i, v));
                }
                None => {
                    let after = (i + 1..len).find_map(|j| known[j].map(|v| (j, v)));
                    y[i] = match (before, after) {
                        (Some((j0, v0)), Some((j1, v1))) => v0 + (v1 - v0) * (i - j0) as f64 / (j1 - j0) as f64,
                        (Some((_, v0)), None) => v0,
                        (None, Some((_, v1))) => v1,
                        (None, None) => 0.0,
                    };
                }
            }
        }
        Some((start, y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_averages_buckets_and_fills_gaps() {
        let s = Seasonality::Hourly;
        // Two points a bucket for 59 buckets, then bucket 10 left empty.
        let points: Vec<(i64, f64)> = (0..120)
            .map(|i| (i * 30_000, (i / 2) as f64))
            .filter(|p| p.0 / 60_000 != 10)
            .collect();
        let (start, y) = s.resample(&points, 60 * 60_000, 1).unwrap();
        assert_eq!(start, 0);
        assert_eq!(y.len(), 60);
        assert_eq!(y[9], 9.0);
        assert_eq!(y[10], 10.0);
        assert_eq!(y[11], 11.0);
    }

    #[test]
    fn resample_refuses_too_little_history() {
        let points: Vec<(i64, f64)> = (0..59).map(|i| (i * 60_000, 1.0)).collect();
        assert_eq!(Seasonality::Hourly.resample(&points, 59 * 60_000, 1), None);
    }
}
