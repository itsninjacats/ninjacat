//! Times in JSON as the F# server wrote them (System.Text.Json): RFC 3339 with the fraction of a
//! second only when there is one, trailing zeros dropped. Use with `#[serde(serialize_with)]`.

use chrono::{DateTime, Utc};
use serde::Serializer;

/// `2026-10-20T10:00:00.5Z`: a UTC date, as the panel's endpoints write theirs.
pub fn utc(ms: i64) -> String {
    format!("{}Z", without_zone(ms))
}

/// `2026-10-20T10:00:00.5+00:00`: a time with its offset, as the monitors' endpoints write theirs.
pub fn offset(ms: i64) -> String {
    format!("{}+00:00", without_zone(ms))
}

fn without_zone(ms: i64) -> String {
    let t = DateTime::<Utc>::from_timestamp_millis(ms).unwrap_or_default();
    let fraction = ms.rem_euclid(1000);
    let seconds = t.format("%Y-%m-%dT%H:%M:%S");
    if fraction == 0 {
        seconds.to_string()
    } else {
        format!("{seconds}.{}", format!("{fraction:03}").trim_end_matches('0'))
    }
}

pub fn as_utc<S: Serializer>(ms: &i64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&utc(*ms))
}

pub fn as_offset<S: Serializer>(ms: &i64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&offset(*ms))
}

/// For an `Option<i64>` that is skipped when None.
pub fn as_offset_option<S: Serializer>(ms: &Option<i64>, s: S) -> Result<S::Ok, S::Error> {
    match ms {
        Some(ms) => as_offset(ms, s),
        None => s.serialize_none(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fraction_only_when_there_is_one() {
        assert_eq!(utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(offset(1_760_000_000_500), "2025-10-09T08:53:20.5+00:00");
        assert_eq!(utc(1_760_000_000_123), "2025-10-09T08:53:20.123Z");
    }
}
