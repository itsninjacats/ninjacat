//! The latency summaries of APM stats and Data Streams.
//!
//! The agent's concentrator and the tracers build these with sketches-go and ship them as
//! protobuf, so this follows sketches-go (ddsketch.FromProto and the few questions the intake asks
//! a sketch). NOT the metric sketches: those follow the agent's own quantile package
//! (ddsketch.rs), and the two cannot be mixed. APM values are span durations in nanoseconds; Data
//! Streams latencies are seconds.

use std::collections::BTreeMap;

use ninjapaws_proto::test::{DdSketch, IndexMapping, Store as SentStore, index_mapping::Interpolation};
use ninjapaws_storage::rows::traces::{SketchState, SketchSummary};
use prost::Message;

/// One of a sketch's two bin stores, answering what sketches-go's DenseStore answers. Not a dense
/// array: two far-apart indexes in a hostile sketch would ask for gigabytes.
struct Store {
    bins: BTreeMap<i64, f64>,
    total: f64,
}

impl Store {
    fn add(&mut self, index: i64, count: f64) {
        if count != 0.0 {
            *self.bins.entry(index).or_default() += count;
            self.total += count;
        }
    }

    fn is_empty(&self) -> bool {
        self.total == 0.0
    }

    fn min_index(&self) -> i64 {
        self.bins.keys().next().copied().unwrap_or(i64::MAX)
    }

    fn max_index(&self) -> i64 {
        self.bins.keys().next_back().copied().unwrap_or(i64::MIN)
    }

    /// The bins that hold something, by ascending index.
    fn filled(&self) -> Vec<(i64, f64)> {
        self.bins.iter().filter(|(_, count)| **count > 0.0).map(|(index, count)| (*index, *count)).collect()
    }
}

fn store(sent: Option<&SentStore>) -> Store {
    let mut filled = Store { bins: BTreeMap::new(), total: 0.0 };
    if let Some(sent) = sent {
        // By index, so the totals are the same sums whatever order the map was sent in.
        let mut bins: Vec<(&i32, &f64)> = sent.bin_counts.iter().collect();
        bins.sort_by_key(|(index, _)| **index);
        for (index, count) in bins {
            filled.add(i64::from(*index), *count);
        }
        for (i, count) in sent.contiguous_bin_counts.iter().enumerate() {
            filled.add(i as i64 + i64::from(sent.contiguous_bin_index_offset), *count);
        }
    }
    filled
}

/// Rebuilds a float from its exponent and its significand in [1, 2).
fn build_float(exponent: f64, significand_plus_one: f64) -> f64 {
    let exponent_bits = (((exponent as i64).wrapping_add(1023) << 52) as u64) & 0x7FF0_0000_0000_0000;
    let significand_bits = significand_plus_one.to_bits() & 0x000F_FFFF_FFFF_FFFF;
    f64::from_bits(exponent_bits | significand_bits)
}

/// The value a bin index stands for, or why the mapping is unusable.
///
/// Three mappings exist: the exact logarithm, and two that approximate it from the float's binary
/// form (linear and cubic interpolation).
fn index_to_value(mapping: Option<&IndexMapping>) -> Result<Box<dyn Fn(i64) -> f64>, String> {
    let Some(mapping) = mapping else { return Err("the sketch has no index mapping".into()) };
    let gamma = mapping.gamma;
    let offset = mapping.index_offset;
    let interpolation = Interpolation::try_from(mapping.interpolation);
    match interpolation {
        Ok(Interpolation::None | Interpolation::Linear | Interpolation::Cubic) if gamma <= 1.0 => {
            Err("the sketch's gamma must be greater than 1".into())
        }
        Ok(Interpolation::None) => {
            let multiplier = 1.0 / gamma.ln();
            let accuracy = 1.0 - 2.0 / (1.0 + gamma);
            Ok(Box::new(move |index| ((index as f64 - offset) / multiplier).exp() * (1.0 + accuracy)))
        }
        Ok(Interpolation::Linear) => {
            let multiplier = 1.0 / gamma.log2();
            let accuracy = 1.0 - 2.0 / (1.0 + gamma.log2().exp());
            Ok(Box::new(move |index| {
                let x = (index as f64 - offset) / multiplier;
                let exponent = x.floor();
                build_float(exponent, x - exponent + 1.0) * (1.0 + accuracy)
            }))
        }
        Ok(Interpolation::Cubic) => {
            let multiplier = 1.0 / gamma.log2();
            let accuracy = 1.0 - 2.0 / (1.0 + (0.7 * gamma.log2()).exp());
            // The cubic's coefficients are A = 6/35, B = -3/5, C = 10/7; the constants below are
            // sketches-go's expressions of them.
            let d0 = -459.0 / 1225.0;
            Ok(Box::new(move |index| {
                let x = (index as f64 - offset) / multiplier;
                let exponent = x.floor();
                let d1 = 5454.0 / 6125.0 - 972.0 / 1225.0 * (x - exponent);
                let p = ((d1 - (d1 * d1 - 4.0 * d0 * d0 * d0).sqrt()) / 2.0).cbrt();
                let significand_plus_one = -(-0.6 + p + d0 / p) / (18.0 / 35.0) + 1.0;
                build_float(exponent, significand_plus_one) * (1.0 + accuracy)
            }))
        }
        _ => Err(format!("interpolation not supported: {}", mapping.interpolation)),
    }
}

/// Turns one summary into the columns the tables keep: the bytes, the state, and the numbers when
/// it really decoded. Bytes that do not decode are a state with its reason, never a reason to
/// store nothing.
pub fn summary(raw: &[u8]) -> SketchSummary {
    let blank = SketchSummary {
        raw: raw.to_vec(),
        state: SketchState::Absent,
        count: None,
        sum: None,
        min: None,
        max: None,
        bin_keys: Vec::new(),
        bin_counts: Vec::new(),
    };
    if raw.is_empty() {
        return blank;
    }
    let sketch = match DdSketch::decode(raw) {
        Ok(sketch) => sketch,
        Err(e) => return SketchSummary { state: SketchState::Undecodable(format!("undecodable: {e}")), ..blank },
    };
    let value = match index_to_value(sketch.mapping.as_ref()) {
        Ok(value) => value,
        // Bytes that arrived and are not a DDSketch stay in the raw column: usually version skew,
        // and this is the evidence.
        Err(problem) => {
            return SketchSummary { state: SketchState::Undecodable(format!("not a DDSketch: {problem}")), ..blank };
        }
    };
    let positive = store(sketch.positive_values.as_ref());
    let negative = store(sketch.negative_values.as_ref());
    let zero_count = sketch.zero_count;
    if zero_count == 0.0 && positive.is_empty() && negative.is_empty() {
        // A valid sketch with no values: not the same as no sketch.
        return SketchSummary { state: SketchState::Empty, ..blank };
    }

    let positive_bins = positive.filled();
    // sketches-go counts the zero bin into the sum as 0 × count, which is not 0 for an infinite
    // count.
    let mut sum = if zero_count != 0.0 { 0.0 * zero_count } else { 0.0 };
    for (index, count) in &positive_bins {
        sum += value(*index) * count;
    }
    for (index, count) in negative.filled() {
        sum += -value(index) * count;
    }
    let min = if !negative.is_empty() {
        Some(-value(negative.max_index()))
    } else if zero_count > 0.0 {
        Some(0.0)
    } else if !positive.is_empty() {
        Some(value(positive.min_index()))
    } else {
        None
    };
    let max = if !positive.is_empty() {
        Some(value(positive.max_index()))
    } else if zero_count > 0.0 {
        Some(0.0)
    } else if !negative.is_empty() {
        Some(-value(negative.min_index()))
    } else {
        None
    };

    // Only the positive store is broken out: these are durations and sizes. The raw bytes keep
    // the rest.
    SketchSummary {
        state: SketchState::Ok,
        count: Some(zero_count + positive.total + negative.total),
        sum: Some(sum),
        min,
        max,
        bin_keys: positive_bins.iter().map(|(index, _)| *index as i32).collect(),
        bin_counts: positive_bins.iter().map(|(_, count)| *count).collect(),
        ..blank
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The sketch Go's ddsketch.NewDefaultDDSketch(0.01) builds from 1e6, 2e6 and 3e6: the bins and
    /// the numbers below are the Go library's.
    pub(crate) fn go_sketch() -> Vec<u8> {
        DdSketch {
            mapping: Some(IndexMapping { gamma: (1.0 + 0.01) / (1.0 - 0.01), ..Default::default() }),
            positive_values: Some(SentStore {
                bin_counts: HashMap::from([(690, 1.0), (725, 1.0), (745, 1.0)]),
                ..Default::default()
            }),
            ..Default::default()
        }
        .encode_to_vec()
    }

    fn close(expected: f64, actual: Option<f64>) {
        let actual = actual.unwrap();
        assert!((expected - actual).abs() < 1e-6, "{expected} != {actual}");
    }

    #[test]
    fn a_sketch_summary_has_the_numbers_sketches_go_computes() {
        let raw = go_sketch();
        let summary = summary(&raw);
        assert_eq!(summary.state, SketchState::Ok);
        assert_eq!(summary.raw, raw);
        assert_eq!(summary.count, Some(3.0));
        // sketches-go gives 5987460.634366452, 994912.7844253893 and 2988992.7847295585: the same
        // to twelve digits, where the sketch itself promises two.
        close(5987460.634366452, summary.sum);
        close(994912.7844253893, summary.min);
        close(2988992.7847295585, summary.max);
        assert_eq!(summary.bin_keys, vec![690, 725, 745]);
        assert_eq!(summary.bin_counts, vec![1.0, 1.0, 1.0]);
    }

    #[test]
    fn no_sketch_an_empty_sketch_and_bytes_that_are_not_one_are_three_different_states() {
        assert_eq!(summary(&[]).state, SketchState::Absent);

        // A valid sketch that measured nothing; its bytes are kept.
        let raw = DdSketch {
            mapping: Some(IndexMapping { gamma: 1.02, ..Default::default() }),
            positive_values: Some(SentStore::default()),
            ..Default::default()
        }
        .encode_to_vec();
        let empty = summary(&raw);
        assert_eq!(empty.state, SketchState::Empty);
        assert_eq!(empty.count, None);
        assert_eq!(empty.raw, raw);

        let garbage = summary(b"not a sketch");
        assert!(matches!(garbage.state, SketchState::Undecodable(_)));
        assert_eq!(garbage.raw, b"not a sketch");
        assert_eq!(garbage.count, None);

        // A sketch without its mapping cannot be read either.
        let unmapped = summary(&DdSketch { zero_count: 4.0, ..Default::default() }.encode_to_vec());
        assert_eq!(unmapped.state, SketchState::Undecodable("not a DDSketch: the sketch has no index mapping".into()));
    }
}
