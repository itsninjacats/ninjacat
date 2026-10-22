use std::collections::BTreeSet;

use crate::stats::{MAD_TO_SIGMA, mad, median};

/// The names Datadog's outliers() takes: 'dbscan', 'scaledbscan', 'mad', 'scaledmad'. `percent`
/// is MAD's fourth argument.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outliers {
    Dbscan,
    ScaledDbscan,
    Mad { percent: f64 },
    ScaledMad { percent: f64 },
}

/// Which of `series` are outliers among them. Each series is `(time, value)` points, oldest
/// first; they are lined up on the times any of them has. Fewer than three series have none.
pub fn outliers(algorithm: Outliers, tolerance: f64, series: &[Vec<(i64, f64)>]) -> Vec<bool> {
    let times: BTreeSet<i64> = series.iter().flat_map(|s| s.iter().map(|p| p.0)).collect();
    let rows: Vec<Vec<Option<f64>>> = series
        .iter()
        .map(|s| times.iter().map(|t| s.binary_search_by_key(t, |p| p.0).ok().map(|i| s[i].1)).collect())
        .collect();

    match algorithm {
        Outliers::Mad { percent } => by_mad(false, tolerance, percent, &rows),
        Outliers::ScaledMad { percent } => by_mad(true, tolerance, percent, &rows),
        Outliers::Dbscan | Outliers::ScaledDbscan => {
            // A series missing a time is placed at that time's median, so a gap is neither near
            // nor far from the others.
            let medians: Vec<f64> = (0..times.len()).map(|t| median(&present(&rows, t))).collect();
            let filled: Vec<Vec<f64>> = rows
                .iter()
                .map(|r| r.iter().zip(&medians).map(|(v, m)| v.unwrap_or(*m)).collect())
                .collect();
            by_dbscan(algorithm == Outliers::ScaledDbscan, tolerance, &filled)
        }
    }
}

fn present(rows: &[Vec<Option<f64>>], t: usize) -> Vec<f64> {
    rows.iter().filter_map(|r| r[t]).collect()
}

fn distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f64>().sqrt()
}

/// DBSCAN, as the docs describe it: the median series is the per-time median; each series'
/// distance to it is Euclidean; the threshold is the median of those distances "multiplied by a
/// normalizing constant", and ε is that threshold times `tolerance`. Series closer than ε to
/// each other cluster together; everything outside the largest cluster is an outlier.
///
/// WARNING(undocumented): the normalizing constant, and how "scaled" scales. The constant is 1.
/// Scaled DBSCAN floors the threshold at 1 % of the median series' norm, so tightly packed
/// series of large values do not turn outliers over noise.
///
/// WARNING(undocumented): DBSCAN's minimum cluster size. It is 1 — every series is a core
/// point, so clusters are the groups of series linked by distances within ε.
fn by_dbscan(scaled: bool, tolerance: f64, rows: &[Vec<f64>]) -> Vec<bool> {
    let n = rows.len();
    if n < 3 {
        return vec![false; n];
    }
    let times = rows[0].len();
    let med: Vec<f64> = (0..times).map(|t| median(&rows.iter().map(|r| r[t]).collect::<Vec<_>>())).collect();
    let mut threshold = median(&rows.iter().map(|r| distance(&med, r)).collect::<Vec<_>>());
    if scaled {
        threshold = threshold.max(0.01 * distance(&med, &vec![0.0; times]));
    }
    let eps = tolerance * threshold;

    // Clusters: connected components of "within ε".
    let mut cluster = vec![usize::MAX; n];
    let mut sizes = Vec::new();
    for start in 0..n {
        if cluster[start] != usize::MAX {
            continue;
        }
        let id = sizes.len();
        let mut size = 1;
        cluster[start] = id;
        let mut stack = vec![start];
        while let Some(i) = stack.pop() {
            for j in 0..n {
                if cluster[j] == usize::MAX && distance(&rows[i], &rows[j]) <= eps {
                    cluster[j] = id;
                    size += 1;
                    stack.push(j);
                }
            }
        }
        sizes.push(size);
    }
    // The first of the largest.
    let largest = (0..sizes.len()).rev().max_by_key(|&c| sizes[c]).unwrap_or(0);
    cluster.iter().map(|&c| c != largest).collect()
}

/// MAD, as the docs describe it: a point is an outlier when it is more than `tolerance`
/// deviations from the per-time median; a series is when more than `percent` of its points are.
///
/// WARNING(undocumented): what a "deviation" is, and how "scaled" scales. A deviation is
/// MAD × 1.4826, the standard-deviation estimate. Scaled MAD floors it at 1 % of |median|, for
/// the same reason as scaled DBSCAN.
fn by_mad(scaled: bool, tolerance: f64, percent: f64, rows: &[Vec<Option<f64>>]) -> Vec<bool> {
    let n = rows.len();
    if n < 3 {
        return vec![false; n];
    }
    let times = rows[0].len();
    let mut outlying = vec![0usize; n];
    let mut seen = vec![0usize; n];
    for t in 0..times {
        let values = present(rows, t);
        let med = median(&values);
        let mut dev = MAD_TO_SIGMA * mad(&values);
        if scaled {
            dev = dev.max(0.01 * med.abs());
        }
        for (i, row) in rows.iter().enumerate() {
            if let Some(v) = row[t] {
                seen[i] += 1;
                if (v - med).abs() > tolerance * dev {
                    outlying[i] += 1;
                }
            }
        }
    }
    (0..n).map(|i| seen[i] > 0 && 100.0 * outlying[i] as f64 / seen[i] as f64 > percent).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Rows on the times 0, 1, 2, …
    fn series(rows: &[&[f64]]) -> Vec<Vec<(i64, f64)>> {
        rows.iter().map(|r| r.iter().enumerate().map(|(t, v)| (t as i64, *v)).collect()).collect()
    }

    #[test]
    fn dbscan_picks_out_the_series_far_from_the_rest() {
        let s = series(&[&[1.0, 1.0, 1.0], &[1.1, 0.9, 1.0], &[0.9, 1.1, 1.0], &[1.0, 1.0, 1.2], &[9.0, 9.0, 9.0]]);
        assert_eq!(outliers(Outliers::Dbscan, 3.0, &s), [false, false, false, false, true]);
    }

    #[test]
    fn mad_flags_a_series_only_past_its_percentage() {
        // Series 3 is far off at one time out of four (25 %).
        let s = series(&[&[1.0, 1.2, 0.8, 1.1], &[1.2, 0.9, 1.1, 0.8], &[0.8, 1.1, 1.0, 1.2], &[1.1, 1.0, 50.0, 0.9]]);
        assert_eq!(outliers(Outliers::Mad { percent: 20.0 }, 3.0, &s), [false, false, false, true]);
        assert_eq!(outliers(Outliers::Mad { percent: 30.0 }, 3.0, &s), [false; 4]);
    }

    #[test]
    fn too_few_series_have_no_outliers() {
        assert_eq!(outliers(Outliers::Dbscan, 1.0, &series(&[&[1.0], &[100.0]])), [false, false]);
    }

    #[test]
    fn unscaled_mad_with_identical_peers_flags_any_deviation() {
        let s = series(&[&[1.0], &[1.0], &[1.0], &[1.01]]);
        assert_eq!(outliers(Outliers::Mad { percent: 0.0 }, 3.0, &s), [false, false, false, true]);
        // Scaled MAD floors the deviation at 1 % of the median: 1.01 is within it.
        assert_eq!(outliers(Outliers::ScaledMad { percent: 0.0 }, 3.0, &s), [false; 4]);
    }

    #[test]
    fn a_gap_is_neither_near_nor_far() {
        let mut s = series(&[&[1.0, 1.0, 1.0], &[1.1, 0.9, 1.0], &[0.9, 1.1, 1.0], &[9.0, 9.0, 9.0]]);
        s[1].remove(1);
        assert_eq!(outliers(Outliers::Dbscan, 3.0, &s), [false, false, false, true]);
    }
}
