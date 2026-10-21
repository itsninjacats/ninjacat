//! Durations as people write them: in a query, a URL or a setting.

use std::sync::LazyLock;

use chrono::TimeDelta;
use regex::Regex;

const PAIR: &str = r"([0-9]+(?:\.[0-9]*)?|\.[0-9]+)(ns|us|µs|ms|s|m|h|d|w)";

/// A final newline is taken, as it always has been.
static WHOLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^([+-])?(?:{PAIR})+\n?$")).unwrap());
static PAIRS: LazyLock<Regex> = LazyLock::new(|| Regex::new(PAIR).unwrap());

/// TimeSpan's range, in its 100 ns ticks: a longer duration was an overflow in .NET.
const TICKS_PER_SECOND: f64 = 10_000_000.0;

fn seconds(unit: &str) -> f64 {
    match unit {
        "ns" => 1e-9,
        "us" | "µs" => 1e-6,
        "ms" => 1e-3,
        "s" => 1.0,
        "m" => 60.0,
        "h" => 3600.0,
        "d" => 86400.0,
        _ => 604800.0,
    }
}

/// A sign, then one or more number-and-unit pairs: "15s", "-1h30m", "1.5h", "300ms", "7d", "2w".
pub fn parse(text: &str) -> Result<TimeDelta, String> {
    let Some(whole) = WHOLE.captures(text) else {
        return Err(format!("invalid duration \"{text}\""));
    };
    let total: f64 =
        PAIRS.captures_iter(text).map(|pair| pair[1].parse::<f64>().unwrap_or(f64::INFINITY) * seconds(&pair[2])).sum();
    let signed = if whole.get(1).is_some_and(|sign| sign.as_str() == "-") { -total } else { total };
    let ticks = signed * TICKS_PER_SECOND;
    if ticks.is_nan() || ticks > i64::MAX as f64 || ticks < i64::MIN as f64 {
        return Err(format!("duration \"{text}\" is too long"));
    }
    let ticks = ticks.trunc() as i64;
    Ok(TimeDelta::seconds(ticks / 10_000_000) + TimeDelta::nanoseconds(ticks % 10_000_000 * 100))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_every_unit_and_sign() {
        assert_eq!(parse("15s"), Ok(TimeDelta::seconds(15)));
        assert_eq!(parse("-1h30m"), Ok(TimeDelta::seconds(-5400)));
        assert_eq!(parse("+1.5h"), Ok(TimeDelta::seconds(5400)));
        assert_eq!(parse("300ms"), Ok(TimeDelta::milliseconds(300)));
        assert_eq!(parse("7d"), Ok(TimeDelta::days(7)));
        assert_eq!(parse("2w"), Ok(TimeDelta::weeks(2)));
        assert_eq!(parse("8µs"), Ok(TimeDelta::microseconds(8)));
        // Truncated to ticks as TimeSpan.FromSeconds does: 5e-6 * 1e7 is 49.99….
        assert_eq!(parse("5us"), Ok(TimeDelta::nanoseconds(4900)));
        assert_eq!(parse(".5s"), Ok(TimeDelta::milliseconds(500)));
        assert_eq!(parse("1.s"), Ok(TimeDelta::seconds(1)));
    }

    #[test]
    fn refuses_what_is_not_a_duration() {
        assert_eq!(parse("15"), Err("invalid duration \"15\"".to_string()));
        assert_eq!(parse("1h 30m"), Err("invalid duration \"1h 30m\"".to_string()));
        assert_eq!(parse(""), Err("invalid duration \"\"".to_string()));
    }

    #[test]
    fn too_long_for_a_timespan() {
        assert_eq!(parse("99999999999w"), Err("duration \"99999999999w\" is too long".to_string()));
        assert!(parse("10000000d").is_ok());
    }
}
