//! The Datadog agent's DDSketch bucketing.
//!
//! Sketches built here must land in the same buckets as the ones the agent sends: bucket counts
//! are merged by key across hosts, and a key that means 47.3 ms here and 12 ms there gives
//! percentiles that look plausible and are wrong.
//!
//! Ported from DataDog/datadog-agent, pkg/util/quantile/config.go. NOT the DDSketch of the paper
//! (and of the sketches libraries): that one derives gamma differently and has no index offset,
//! so its keys cannot be merged with the agent's.
//!
//! Copyright 2016-present Datadog, Inc. Licensed under the Apache License 2.0; see
//! THIRD_PARTY_NOTICES.md.

use std::collections::BTreeMap;
use std::sync::LazyLock;

/// The guaranteed relative error: 1/128, about 0.78 %.
pub const EPS: f64 = 1.0 / 128.0;

/// The smallest value with a key of its own; anything smaller is key 0.
pub const MIN: f64 = 1e-9;

/// The ratio between consecutive bucket boundaries.
pub const GAMMA: f64 = 1.0 + 2.0 * EPS;

static GAMMA_LN: LazyLock<f64> = LazyLock::new(|| (2.0 * EPS).ln_1p());

/// Shifts keys so the smallest supported value gets key 1.
pub static BIAS: LazyLock<i32> = LazyLock::new(|| -((MIN.ln() / *GAMMA_LN).floor() as i32) + 1);

/// The bucket a value belongs to.
///
/// Rounds to NEAREST, not down: gamma^(key - bias) is the middle of the bucket, not its floor. The
/// agent's own comment says otherwise; its code does this.
pub fn key(value: f64) -> i32 {
    if value < 0.0 {
        -key(-value)
    } else if value == 0.0 || value < MIN {
        0
    } else {
        (value.ln() / *GAMMA_LN).round_ties_even() as i32 + *BIAS
    }
}

/// The representative value of a bucket: its middle on the logarithmic scale.
pub fn value(k: i32) -> f64 {
    if k == 0 {
        0.0
    } else if k < 0 {
        -value(-k)
    } else {
        GAMMA.powi(k - *BIAS)
    }
}

/// The summary that travels with the buckets.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stats {
    pub count: i64,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub sum: f64,
}

/// Raw measurements as agent-compatible buckets: keys ascending, counts in matching order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sketch {
    pub keys: Vec<i32>,
    pub counts: Vec<u32>,
    pub stats: Stats,
}

pub fn build(values: &[f64]) -> Sketch {
    if values.is_empty() {
        return Sketch::default();
    }
    let mut tally: BTreeMap<i32, u32> = BTreeMap::new();
    for v in values {
        *tally.entry(key(*v)).or_default() += 1;
    }
    let sum: f64 = values.iter().sum();
    Sketch {
        keys: tally.keys().copied().collect(),
        counts: tally.values().copied().collect(),
        stats: Stats {
            count: values.len() as i64,
            min: values.iter().copied().fold(f64::INFINITY, f64::min),
            max: values.iter().copied().fold(f64::NEG_INFINITY, f64::max),
            avg: sum / values.len() as f64,
            sum,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_agents_bias_and_keys() {
        assert_eq!(*BIAS, 1338);
        assert_eq!(key(1.0), 1338);
        assert_eq!(key(0.0), 0);
        assert_eq!(key(-1.0), -1338);
        assert!((value(key(100.0)) / 100.0 - 1.0).abs() < EPS);
    }
}
