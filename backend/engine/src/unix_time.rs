//! Unix times and spans a client sends, kept to the span of stored times. (F#: `UnixTime.fs`.)
//!
//! Every stored time falls between the epoch and the end of 2105 (ClickHouse's DateTime), so a
//! range outside them finds nothing, which is Datadog's answer too, not an error. Clamped this far
//! inside what a date holds, a range can be shifted by a step or a season without overflowing.

use chrono::TimeDelta;

/// 2105-12-31T23:59:59Z.
pub const LATEST_MS: i64 = 4_291_747_199_000;

/// Seconds, as milliseconds, clamped to the stored times.
pub fn of_seconds(seconds: i64) -> i64 {
    seconds.clamp(0, LATEST_MS / 1000) * 1000
}

/// Milliseconds clamped to the stored times.
pub fn of_millis(ms: i64) -> i64 {
    ms.clamp(0, LATEST_MS)
}

/// A step or a shift a client asks for, kept to the span of stored times: a longer one finds what
/// that one does, or nothing.
///
/// Truncated to .NET's tick (100 ns), as `TimeSpan.FromMilliseconds` does. NaN, which .NET
/// refuses with an exception, is no span at all.
pub fn span_of_millis(ms: f64) -> TimeDelta {
    if ms.is_nan() {
        return TimeDelta::zero();
    }
    let ticks = (ms.clamp(-(LATEST_MS as f64), LATEST_MS as f64) * 10_000.0).trunc() as i64;
    TimeDelta::nanoseconds(ticks * 100)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_to_the_stored_times() {
        assert_eq!(of_seconds(-5), 0);
        assert_eq!(of_seconds(1_700_000_000), 1_700_000_000_000);
        assert_eq!(of_seconds(i64::MAX), LATEST_MS);
        assert_eq!(of_millis(i64::MIN), 0);
        assert_eq!(of_millis(i64::MAX), LATEST_MS);
        assert_eq!(span_of_millis(1.5), TimeDelta::microseconds(1500));
        assert_eq!(span_of_millis(f64::INFINITY), TimeDelta::milliseconds(LATEST_MS));
        assert_eq!(span_of_millis(f64::NEG_INFINITY), TimeDelta::milliseconds(-LATEST_MS));
    }
}
