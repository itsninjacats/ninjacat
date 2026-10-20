use stlrs::StlResult;

/// Robust STL with statsmodels' defaults (seasonal 7; trend the next odd number ≥
/// 1.5·period / (1 − 1.5/7); low-pass the next odd number > period; degrees 1; 2 inner and 15
/// robustness iterations), and with `jumps` R stl()'s: each smoother evaluated every ⌈width/10⌉
/// points and interpolated between, as Cleveland et al. recommend. The jumps are for speed; a
/// daily season is 1728 points of history.
///
/// WARNING(undocumented): Datadog describes its robust algorithm as "a seasonal-trend
/// decomposition" without saying which. These are the reference defaults of statsmodels and R.
pub fn decompose(period: usize, y: &[f64], jumps: bool) -> Option<StlResult<f64>> {
    let odd = |x: usize| if x.is_multiple_of(2) { x + 1 } else { x };
    let seasonal = 7;
    let trend = odd((1.5 * period as f64 / (1.0 - 1.5 / seasonal as f64)).ceil() as usize);
    let low_pass = odd(period + 1);
    let jump = |width: usize| if jumps { width.div_ceil(10) } else { 1 };

    stlrs::params()
        .seasonal_length(seasonal)
        .trend_length(trend)
        .low_pass_length(low_pass)
        .seasonal_degree(1)
        .trend_degree(1)
        .low_pass_degree(1)
        .seasonal_jump(jump(seasonal))
        .trend_jump(jump(trend))
        .low_pass_jump(jump(low_pass))
        .inner_loops(2)
        .outer_loops(15)
        .robust(true)
        .fit(y, period)
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixture::{fixture, max_diff, numbers};

    #[test]
    fn is_statsmodels_stl() {
        let r = fixture("stl.json");
        let d = decompose(24, &numbers(&r["y"]), false).unwrap();
        assert!(max_diff(d.trend(), &numbers(&r["trend"])) < 1e-9);
        assert!(max_diff(d.seasonal(), &numbers(&r["seasonal"])) < 1e-9);
        assert!(max_diff(d.remainder(), &numbers(&r["resid"])) < 1e-9);
    }

    #[test]
    fn with_jumps_stays_close_to_it() {
        let r = fixture("stl.json");
        let d = decompose(24, &numbers(&r["y"]), true).unwrap();
        assert!(max_diff(d.trend(), &numbers(&r["trend"])) < 0.01);
        assert!(max_diff(d.seasonal(), &numbers(&r["seasonal"])) < 0.01);
    }
}
