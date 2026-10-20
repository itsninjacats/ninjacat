pub fn median(xs: &[f64]) -> f64 {
    let mut s = xs.to_vec();
    s.sort_by(f64::total_cmp);
    let n = s.len();
    if n == 0 {
        f64::NAN
    } else if n % 2 == 1 {
        s[n / 2]
    } else {
        (s[n / 2 - 1] + s[n / 2]) / 2.0
    }
}

pub fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len() as f64
}

/// Population standard deviation.
pub fn sd(xs: &[f64]) -> f64 {
    let m = mean(xs);
    (xs.iter().map(|x| (x - m) * (x - m)).sum::<f64>() / xs.len() as f64).sqrt()
}

/// Median absolute deviation from the median.
pub fn mad(xs: &[f64]) -> f64 {
    let m = median(xs);
    median(&xs.iter().map(|x| (x - m).abs()).collect::<Vec<_>>())
}

/// MAD × 1.4826 estimates the standard deviation of normal data: the usual constant that makes
/// MAD "the robust analog for standard deviation".
pub const MAD_TO_SIGMA: f64 = 1.4826;
