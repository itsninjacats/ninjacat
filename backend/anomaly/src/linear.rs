use crate::stats::{MAD_TO_SIGMA, mad};

/// Least squares on (x, y): slope and intercept.
pub fn ols(xs: &[f64], ys: &[f64]) -> (f64, f64) {
    wls(xs, ys, &vec![1.0; xs.len()])
}

/// Weighted least squares, the step inside iteratively reweighted fitting.
fn wls(xs: &[f64], ys: &[f64], ws: &[f64]) -> (f64, f64) {
    let sw: f64 = ws.iter().sum();
    let mx = ws.iter().zip(xs).map(|(w, x)| w * x).sum::<f64>() / sw;
    let my = ws.iter().zip(ys).map(|(w, y)| w * y).sum::<f64>() / sw;
    let (mut sxx, mut sxy) = (0.0, 0.0);
    for ((w, x), y) in ws.iter().zip(xs).zip(ys) {
        sxx += w * (x - mx) * (x - mx);
        sxy += w * (x - mx) * (y - my);
    }
    if xs.len() < 2 || sxx == 0.0 {
        (0.0, my)
    } else {
        let b = sxy / sxx;
        (b, my - b * mx)
    }
}

/// A line fitted under Huber loss, by iteratively reweighted least squares: residuals within
/// `k` scales count fully, larger ones with weight k / |r|, so a spike bends the line far less
/// than under least squares.
///
/// WARNING(undocumented): Datadog names the loss, not its tuning. k = 1.345 (95 % efficiency on
/// normal data, the textbook value), the scale is the MAD of the residuals, and fitting stops
/// after 50 rounds or when the line moves by less than 1e-9.
pub fn huber(xs: &[f64], ys: &[f64]) -> (f64, f64) {
    let k = 1.345;
    let (mut b, mut a) = ols(xs, ys);
    for _ in 0..50 {
        let residuals = residuals(xs, ys, b, a);
        let scale = MAD_TO_SIGMA * mad(&residuals);
        if scale == 0.0 {
            break;
        }
        let ws: Vec<f64> = residuals
            .iter()
            .map(|r| {
                let u = r.abs() / scale;
                if u <= k { 1.0 } else { k / u }
            })
            .collect();
        let (b2, a2) = wls(xs, ys, &ws);
        let settled = (b2 - b).abs() < 1e-9 && (a2 - a).abs() < 1e-9;
        (b, a) = (b2, a2);
        if settled {
            break;
        }
    }
    (b, a)
}

fn residuals(xs: &[f64], ys: &[f64], slope: f64, intercept: f64) -> Vec<f64> {
    xs.iter().zip(ys).map(|(x, y)| y - (intercept + slope * x)).collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinearModel {
    Default,
    Simple,
    Reactive,
}

/// The value at each of `future`, with the band's half-width, from (x, y) history.
///
/// WARNING(undocumented): how the three models differ in practice. We read the docs'
/// descriptions as: simple fits Huber over the whole history; default fits Huber over its most
/// recent half ("adjusts to the most recent trend ... robust to recent noise"); reactive fits
/// least squares over its most recent fifth ("extrapolates recent behavior ... at the risk of
/// overfitting"). The band is `deviations` robust standard deviations of the fit's residuals,
/// the same width at every horizon.
pub fn forecast(model: LinearModel, deviations: f64, xs: &[f64], ys: &[f64], future: &[f64]) -> Vec<(f64, f64)> {
    let n = xs.len();
    let from = match model {
        LinearModel::Simple => 0,
        LinearModel::Reactive => n.saturating_sub((n / 5).max(2)),
        LinearModel::Default => n.saturating_sub((n / 2).max(2)),
    };
    let (fx, fy) = (&xs[from..], &ys[from..]);
    let (slope, intercept) = match model {
        LinearModel::Reactive => ols(fx, fy),
        _ => huber(fx, fy),
    };
    let width = deviations * MAD_TO_SIGMA * mad(&residuals(fx, fy, slope, intercept));
    future.iter().map(|x| (intercept + slope * x, width)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ols_recovers_an_exact_line() {
        let xs: Vec<f64> = (0..10).map(f64::from).collect();
        let ys: Vec<f64> = xs.iter().map(|x| 3.0 + 2.0 * x).collect();
        let (b, a) = ols(&xs, &ys);
        assert!((b - 2.0).abs() < 1e-9 && (a - 3.0).abs() < 1e-9);
    }

    #[test]
    fn huber_shrugs_off_a_spike_that_bends_least_squares() {
        let xs: Vec<f64> = (0..10).map(f64::from).collect();
        let ys: Vec<f64> = (0..10).map(|i| if i == 9 { 105.0 } else { 5.0 }).collect();
        assert!(ols(&xs, &ys).0 > 5.0);
        let (b, a) = huber(&xs, &ys);
        assert!(b.abs() < 1e-6 && (a - 5.0).abs() < 1e-6, "{b} {a}");
    }

    #[test]
    fn every_model_extends_an_exact_line() {
        let xs: Vec<f64> = (0..20).map(|i| (i - 20) as f64 * 10.0).collect();
        let ys: Vec<f64> = xs.iter().map(|x| 100.0 + 0.5 * x).collect();
        for model in [LinearModel::Default, LinearModel::Simple, LinearModel::Reactive] {
            let p = forecast(model, 1.0, &xs, &ys, &[0.0, 60.0]);
            assert!((p[0].0 - 100.0).abs() < 1e-6 && (p[1].0 - 130.0).abs() < 1e-6, "{model:?}");
            assert!(p[0].1.abs() < 1e-6, "{model:?}: no residuals, no band");
        }
    }
}
