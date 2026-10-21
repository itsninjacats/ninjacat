use crate::Band;
use crate::stats::{median, sd};

/// For each value, the range expected from the `window` values before it.
///
/// "Basic uses a simple lagging rolling quantile computation to determine the range of expected
/// values" (docs: monitors/types/anomaly), and "bounds can be interpreted as the standard
/// deviations for your algorithm" (docs: dashboards/functions/algorithms).
///
/// Standard deviation, not a robust stand-in for it: the MAD collapses to 0 when most of a window
/// is one value, as a goroutine count usually is, and the band with it.
///
/// WARNING(undocumented): which quantile centres the band. The median. A value needs at least 5
/// before it for a band.
pub fn band(window: usize, bounds: f64, values: &[f64]) -> Vec<Option<Band>> {
    (0..values.len())
        .map(|i| {
            let past = &values[i.saturating_sub(window)..i];
            if past.len() < 5 {
                return None;
            }
            let centre = median(past);
            let sd = sd(past);
            Some(Band { lower: centre - bounds * sd, upper: centre + bounds * sd })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_recent_level() {
        let values: Vec<f64> = (0..21).map(|i| if i % 2 == 0 { 11.0 } else { 9.0 }).collect();
        let b = band(60, 2.0, &values);
        assert_eq!(b[3], None);
        let b = b[20].unwrap();
        assert!((9.0..=11.0).contains(&((b.lower + b.upper) / 2.0)));
        assert!(b.upper > b.lower);
    }

    #[test]
    fn does_not_collapse_on_a_mostly_constant_integer_series() {
        let mut values = vec![37.0, 37.0, 38.0, 37.0, 39.0, 37.0, 37.0, 38.0, 37.0, 40.0, 37.0, 37.0];
        values.push(38.0);
        let b = band(60, 2.0, &values)[12].unwrap();
        assert!(b.lower < 37.0 && b.upper > 38.0, "{b:?}");
    }

    #[test]
    fn bounds_are_standard_deviations() {
        let mut values: Vec<f64> = (0..10).map(|i| if i % 2 == 0 { 8.0 } else { 12.0 }).collect();
        values.push(10.0);
        let b = band(60, 3.0, &values)[10].unwrap();
        assert!((b.lower - 4.0).abs() < 1e-9 && (b.upper - 16.0).abs() < 1e-9, "{b:?}");
    }
}
