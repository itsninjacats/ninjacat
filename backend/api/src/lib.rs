//! What the read-side programs (`ninjapaws_internal`, `ninjapaws_monitors`) and
//! `ninjapaws_migrate` share: the ClickHouse reader and their settings.
pub mod reads;

use std::env;

use chrono::TimeDelta;
use ninjapaws_engine::query::plan::{DEFAULT_RETENTION, Retention};

pub fn var_or(name: &str, default: &str) -> String {
    env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

/// `:8081`, `*:8081` and `0.0.0.0:8081` are every interface, as the settings have always spelled it.
pub fn listen_on(addr: &str) -> String {
    match addr.rsplit_once(':') {
        Some(("" | "*", port)) => format!("0.0.0.0:{port}"),
        _ => addr.to_string(),
    }
}

/// The metrics retention, as `ninjapaws_migrate` sets the TTLs and the query planners read them.
pub fn metrics_retention() -> Result<Retention, String> {
    retention(&var_or("NINJACAT_METRICS_RETENTION_DAYS", ""), &var_or("NINJACAT_METRICS_ROLLUP_RETENTION_DAYS", ""))
}

/// A step of whole minutes reads the rollup whatever its age, so a rollup kept shorter than the
/// raw points would answer nothing where the raw table still holds them.
fn retention(raw: &str, rollup: &str) -> Result<Retention, String> {
    let raw = days("NINJACAT_METRICS_RETENTION_DAYS", raw, DEFAULT_RETENTION.raw)?;
    let rollup = days("NINJACAT_METRICS_ROLLUP_RETENTION_DAYS", rollup, DEFAULT_RETENTION.rollup)?;
    if rollup < raw {
        return Err(format!(
            "NINJACAT_METRICS_ROLLUP_RETENTION_DAYS ({}) must not be shorter than NINJACAT_METRICS_RETENTION_DAYS ({})",
            rollup.num_days(),
            raw.num_days()
        ));
    }
    Ok(Retention { raw, rollup })
}

/// A whole number of days, up to a century. A set value outside 1–36500 stops the start: falling
/// back to the default could set a shorter TTL than meant, and ClickHouse would delete the
/// difference.
fn days(name: &str, value: &str, default: TimeDelta) -> Result<TimeDelta, String> {
    match value {
        "" => Ok(default),
        raw => match raw.trim().parse::<i64>() {
            Ok(n) if (1..=36500).contains(&n) => Ok(TimeDelta::days(n)),
            _ => Err(format!("{name} must be a whole number of days from 1 to 36500, got \"{raw}\"")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn days_of(retention: Result<Retention, String>) -> Result<(i64, i64), String> {
        retention.map(|r| (r.raw.num_days(), r.rollup.num_days()))
    }

    #[test]
    fn unset_is_the_default() {
        assert_eq!(days_of(retention("", "")), Ok((30, 400)));
    }

    #[test]
    fn a_set_value_is_whole_days_from_one_to_a_century() {
        assert_eq!(days_of(retention(" 7 ", "36500")), Ok((7, 36500)));
        assert_eq!(days_of(retention("1", "1")), Ok((1, 1)));
        for bad in ["0", "-3", "36501", "1.5", "7d", "a week"] {
            let error = retention(bad, "").err().unwrap_or_else(|| panic!("{bad} was taken"));
            assert_eq!(
                error,
                format!(
                    "NINJACAT_METRICS_RETENTION_DAYS must be a whole number of days from 1 to 36500, got \"{bad}\""
                )
            );
        }
        assert!(retention("", "0").is_err_and(|e| e.starts_with("NINJACAT_METRICS_ROLLUP_RETENTION_DAYS must")));
    }

    #[test]
    fn the_rollup_is_kept_no_shorter_than_the_raw_points() {
        assert_eq!(days_of(retention("90", "90")), Ok((90, 90)));
        assert_eq!(
            retention("90", "60").err().as_deref(),
            Some(
                "NINJACAT_METRICS_ROLLUP_RETENTION_DAYS (60) must not be shorter than NINJACAT_METRICS_RETENTION_DAYS (90)"
            )
        );
        // The default rollup is checked against a longer raw retention too.
        assert!(retention("500", "").is_err());
    }
}
