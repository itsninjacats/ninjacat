use crate::stats::{MAD_TO_SIGMA, mad, mean};

/// Replaces each value by the mean of its segment, the segments found by binary segmentation:
/// split where it lowers the squared error most, and keep splitting each half while a split is
/// worth more than a penalty.
///
/// WARNING(undocumented): Datadog does not say how it finds segments. The penalty is BIC-like,
/// 2·σ²·ln n, with σ estimated from the MAD of consecutive differences (a level shift is one
/// large difference, so it barely moves σ), or from their standard deviation where that MAD is
/// zero. A segment is at least 2 points.
pub fn piecewise_constant(ys: &[f64]) -> Vec<f64> {
    let n = ys.len();
    if n < 4 {
        return vec![if n == 0 { 0.0 } else { mean(ys) }; n];
    }
    let diffs: Vec<f64> = ys.windows(2).map(|w| w[1] - w[0]).collect();

    // A mostly flat, integer-valued series (a goroutine count) has a MAD of differences of zero;
    // the penalty would vanish and every blip become a segment. Then σ comes from the standard
    // deviation instead.
    let mut sigma = MAD_TO_SIGMA * mad(&diffs) / 2f64.sqrt();
    if sigma == 0.0 {
        let m = mean(&diffs);
        sigma = mean(&diffs.iter().map(|d| (d - m) * (d - m)).collect::<Vec<_>>()).sqrt() / 2f64.sqrt();
    }
    let penalty = 2.0 * sigma * sigma * (n as f64).ln();

    // Prefix sums make any segment's squared error O(1).
    let mut s1 = vec![0.0; n + 1];
    let mut s2 = vec![0.0; n + 1];
    for (i, y) in ys.iter().enumerate() {
        s1[i + 1] = s1[i] + y;
        s2[i + 1] = s2[i] + y * y;
    }
    let segments = Segments { s1, s2, penalty };
    let mut out = vec![0.0; n];
    segments.split(0, n, &mut out);
    out
}

struct Segments {
    s1: Vec<f64>,
    s2: Vec<f64>,
    penalty: f64,
}

impl Segments {
    /// The squared error of [i, j) around its mean.
    fn sse(&self, i: usize, j: usize) -> f64 {
        let sum = self.s1[j] - self.s1[i];
        self.s2[j] - self.s2[i] - sum * sum / (j - i) as f64
    }

    fn split(&self, i: usize, j: usize, out: &mut [f64]) {
        let mut best: Option<(usize, f64)> = None;
        for c in i + 2..j.saturating_sub(1) {
            let cost = self.sse(i, c) + self.sse(c, j);
            if best.is_none_or(|(_, b)| cost < b) {
                best = Some((c, cost));
            }
        }
        match best {
            Some((c, cost)) if self.sse(i, j) - cost > self.penalty => {
                self.split(i, c, out);
                self.split(c, j, out);
            }
            _ => {
                let mean = (self.s1[j] - self.s1[i]) / (j - i) as f64;
                out[i..j].fill(mean);
            }
        }
    }
}

fn kurtosis(xs: &[f64]) -> f64 {
    let m = mean(xs);
    let v = mean(&xs.iter().map(|x| (x - m).powf(2.0)).collect::<Vec<_>>());
    if v == 0.0 { 0.0 } else { mean(&xs.iter().map(|x| (x - m).powf(4.0)).collect::<Vec<_>>()) / (v * v) }
}

fn roughness(xs: &[f64]) -> f64 {
    if xs.len() < 3 {
        return 0.0;
    }
    let d: Vec<f64> = xs.windows(2).map(|w| w[1] - w[0]).collect();
    let m = mean(&d);
    mean(&d.iter().map(|x| (x - m).powf(2.0)).collect::<Vec<_>>()).sqrt()
}

/// Trailing moving average over `w` points, the first `w - 1` over the points so far. The mean is
/// kept running, as MathNet's MovingStatistics keeps it.
pub fn moving_average(w: usize, ys: &[f64]) -> Vec<f64> {
    let mut m = 0.0;
    ys.iter()
        .enumerate()
        .map(|(i, &v)| {
            if i < w {
                m += (v - m) / (i + 1) as f64;
            } else {
                m += (v - ys[i - w]) / w as f64;
            }
            m
        })
        .collect()
}

/// The window autosmooth picks for these values.
///
/// Source: Datadog's "Auto Smoother" post (https://www.datadoghq.com/blog/auto-smoother-asap/).
/// Auto Smoother "is inspired by the ASAP (Automatic Smoothing for Attention Prioritization)
/// algorithm" (Rong & Bailis, Stanford): a moving average whose window is chosen from the
/// smoothed series' roughness ("the standard deviation of the first difference series") and
/// kurtosis ("the fourth standardized moment"), which keeps it from oversmoothing.
///
/// This is ASAP's own rule: of the windows up to `max_window`, the one with the least roughness
/// whose kurtosis stays at least the original's.
///
/// WARNING(undocumented): Datadog "weighs roughness and kurtosis differently than ASAP does" and
/// "uses a different optimization technique", without saying how. We use ASAP's weighting and an
/// exhaustive search over windows.
pub fn autosmooth_window(max_window: usize, ys: &[f64]) -> usize {
    let original = kurtosis(ys);
    let mut best = (1, f64::INFINITY);
    for w in 1..=max_window.min(ys.len() / 4).max(1) {
        let smoothed = moving_average(w, ys);
        let keeps_peaks = w == 1 || kurtosis(&smoothed) >= original;
        if !keeps_peaks {
            continue;
        }
        let r = roughness(&smoothed);
        if w == 1 || r < best.1 {
            best = (w, r);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn piecewise_constant_finds_a_level_shift() {
        let noisy = [10.1, 9.9, 10.0, 10.2, 9.8, 20.1, 19.9, 20.0, 20.2, 19.8];
        let rounded: Vec<f64> = piecewise_constant(&noisy).iter().map(|v| (v * 1000.0).round() / 1000.0).collect();
        assert_eq!(rounded, [10.0, 10.0, 10.0, 10.0, 10.0, 20.0, 20.0, 20.0, 20.0, 20.0]);
    }

    fn distinct(xs: &[f64]) -> usize {
        let mut v = xs.to_vec();
        v.dedup();
        v.len()
    }

    #[test]
    fn piecewise_constant_leaves_one_level_as_one_segment() {
        assert_eq!(distinct(&piecewise_constant(&[5.1, 4.9, 5.0, 5.2, 4.8, 5.1, 4.9, 5.0])), 1);
    }

    #[test]
    fn piecewise_constant_is_not_fooled_by_a_mostly_flat_integer_series() {
        // 37 with single blips, like a goroutine count: one segment, not one per blip.
        let ys = [37.0, 37.0, 38.0, 37.0, 37.0, 37.0, 39.0, 37.0, 37.0, 37.0, 37.0, 38.0, 37.0, 37.0];
        assert_eq!(distinct(&piecewise_constant(&ys)), 1);
    }

    #[test]
    fn autosmooth_smooths_noise_and_keeps_a_step() {
        // Alternating noise around 10, then a clear step up to 30.
        let ys: Vec<f64> =
            (0..80).map(|i| if i < 40 { 10.0 } else { 30.0 } + if i % 2 == 0 { 1.0 } else { -1.0 }).collect();
        let w = autosmooth_window(40, &ys);
        assert!(w > 1, "window {w}");
        let smoothed = moving_average(w, &ys);
        // Well inside each level the noise is gone, and the levels are kept.
        assert!((9.5..=10.5).contains(&smoothed[30]), "{}", smoothed[30]);
        assert!((29.5..=30.5).contains(&smoothed[75]), "{}", smoothed[75]);
    }

    #[test]
    fn moving_average_is_trailing() {
        let m = moving_average(2, &[1.0, 3.0, 5.0, 9.0]);
        assert_eq!(m, [1.0, 2.0, 4.0, 7.0]);
    }
}
