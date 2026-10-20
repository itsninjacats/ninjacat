//! Small readings shared by the wire formats: Datadog's tags, text, times, varints.

use std::collections::BTreeMap;

use chrono::{DateTime, TimeZone, Utc};
use ninjapaws_storage::rows::Tags;
use regex::Regex;

/// Datadog's "key:value" tags.
pub mod tags {
    use super::*;

    /// Splits at the FIRST colon: values legitimately contain colons ("url:http://x"). A bare tag
    /// keeps its key with an empty value.
    pub fn split(tag: &str) -> (&str, &str) {
        tag.split_once(':').unwrap_or((tag, ""))
    }

    /// Key → every value seen, in arrival order. Tags are a multiset: two tags may share a key
    /// (kube_service:a and kube_service:b), and both must survive.
    pub fn to_multi_map<S: AsRef<str>>(tags: impl IntoIterator<Item = S>) -> Tags {
        let mut by_key = Tags::new();
        for tag in tags {
            let (key, value) = split(tag.as_ref());
            by_key.entry(key.to_string()).or_default().push(value.to_string());
        }
        by_key
    }

    /// For "key:value" lists whose keys are unique by contract (Kubernetes labels and
    /// annotations): the last value of a key wins.
    pub fn to_map<S: AsRef<str>>(pairs: impl IntoIterator<Item = S>) -> BTreeMap<String, String> {
        pairs
            .into_iter()
            .map(|pair| {
                let (key, value) = split(pair.as_ref());
                (key.to_string(), value.to_string())
            })
            .collect()
    }

    /// `ddtags` is one comma-separated string.
    pub fn split_ddtags(text: &str) -> Vec<String> {
        if text.is_empty() { Vec::new() } else { text.split(',').map(String::from).collect() }
    }
}

pub mod text {
    pub fn first_non_empty<'a>(values: &[&'a str]) -> &'a str {
        values.iter().find(|v| !v.is_empty()).copied().unwrap_or("")
    }

    /// Bytes as text; what is not UTF-8 becomes U+FFFD, as .NET's decoder does.
    pub fn utf8(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    /// ClickHouse has no Bool in these tables; flags are UInt8.
    pub fn flag(value: bool) -> u8 {
        u8::from(value)
    }

    /// A float as the text a column holds: the shortest digits that give the same float back,
    /// laid out as the columns have held them from the start (printf's %g): plain digits between
    /// 1e-4 and 1e6, d.ddde±XX outside.
    pub fn of_float(value: f64) -> String {
        if value.is_nan() {
            return "NaN".into();
        }
        if value.is_infinite() {
            return if value > 0.0 { "+Inf".into() } else { "-Inf".into() };
        }
        if value == 0.0 {
            return if value.is_sign_negative() { "-0".into() } else { "0".into() };
        }
        // `{:e}` is the shortest round-trip digits as d.ddde<power>.
        let scientific = format!("{:e}", value.abs());
        let (mantissa, power) = scientific.split_once('e').unwrap();
        let power: i32 = power.parse().unwrap();
        let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
        let sign = if value < 0.0 { "-" } else { "" };
        // The number is 0.<digits> × 10^point.
        let point = power + 1;
        if !(-4..6).contains(&power) {
            let rest = if digits.len() > 1 { format!(".{}", &digits[1..]) } else { String::new() };
            let power_sign = if power < 0 { "-" } else { "+" };
            format!("{sign}{}{rest}e{power_sign}{:02}", &digits[..1], power.abs())
        } else if point <= 0 {
            format!("{sign}0.{}{digits}", "0".repeat((-point) as usize))
        } else if point as usize >= digits.len() {
            format!("{sign}{digits}{}", "0".repeat(point as usize - digits.len()))
        } else {
            format!("{sign}{}.{}", &digits[..point as usize], &digits[point as usize..])
        }
    }

    pub fn of_float32(value: f32) -> String {
        if value.is_finite() {
            // The shortest digits of the f32, read back as the f64 they spell.
            of_float(value.to_string().parse::<f64>().unwrap())
        } else {
            of_float(f64::from(value))
        }
    }
}

/// Protobuf's variable-length integers, where they appear outside a message the generated code
/// reads: the dictionaries of metrics v3, the DNS buffers of a connections payload.
pub mod varint {
    /// The varint at `position`: its value and the bytes it took. None when the bytes run out, or
    /// it is longer than a varint can be.
    pub fn read(data: &[u8], position: usize) -> Option<(u64, usize)> {
        let mut rest = data.get(position..)?;
        let before = rest.len();
        let value = prost::encoding::decode_varint(&mut rest).ok()?;
        Some((value, before - rest.len()))
    }
}

pub mod time {
    use super::*;

    /// An RFC 3339 timestamp as UTC, or None: a 'T', seconds and a zone are required, a fraction
    /// is optional (after '.' or ',', as ISO 8601 allows), any offset up to 23:59. Digits past
    /// 100 ns are dropped, not rounded.
    pub fn try_rfc3339(text: &str) -> Option<DateTime<Utc>> {
        static PATTERN: std::sync::LazyLock<Regex> = std::sync::LazyLock::new(|| {
            Regex::new(r"^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?(?:Z|([+-])(\d{2}):(\d{2}))$")
                .unwrap()
        });
        let found = PATTERN.captures(text)?;
        let number = |i: usize| found.get(i).map_or(0, |m| m.as_str().parse::<i64>().unwrap_or(0));
        let (year, month, day) = (number(1), number(2), number(3));
        let (hour, minute, second) = (number(4), number(5), number(6));
        let (offset_hours, offset_minutes) = (number(9), number(10));
        if offset_hours >= 24 || offset_minutes >= 60 || hour >= 24 || minute >= 60 || second >= 60 || year < 1 {
            return None;
        }
        let date = chrono::NaiveDate::from_ymd_opt(year as i32, month as u32, day as u32)?;
        let ticks = found.get(7).map_or(0, |m| {
            let padded = format!("{:0<7}", m.as_str());
            padded[..7].parse::<i64>().unwrap_or(0)
        });
        let local = date.and_hms_opt(hour as u32, minute as u32, second as u32)?.and_utc();
        let sign = if found.get(8).is_some_and(|m| m.as_str() == "-") { -1 } else { 1 };
        let offset = chrono::TimeDelta::minutes(sign * (offset_hours * 60 + offset_minutes));
        let utc = local + chrono::TimeDelta::nanoseconds(ticks * 100) - offset;
        // A time whose UTC form falls outside years 1 to 9999 is kept at the limit.
        Some(utc.clamp(min_time(), max_time()))
    }

    fn min_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(1, 1, 1, 0, 0, 0).unwrap()
    }

    fn max_time() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(9999, 12, 31, 23, 59, 59).unwrap() + chrono::TimeDelta::nanoseconds(999_999_900)
    }

    /// A timestamp past what the server holds (year 9999) is kept at that limit: one absurd value
    /// in a payload must not fail the request.
    pub fn from_unix_seconds(seconds: i64) -> DateTime<Utc> {
        let limit = max_time().timestamp();
        let floor = min_time().timestamp();
        Utc.timestamp_opt(seconds.clamp(floor, limit), 0).unwrap()
    }

    pub fn from_unix_millis(millis: i64) -> DateTime<Utc> {
        let limit = max_time().timestamp() * 1000 + 999;
        let floor = min_time().timestamp() * 1000;
        Utc.timestamp_millis_opt(millis.clamp(floor, limit)).unwrap()
    }

    /// A timestamp off the wire, in seconds; the receive time when the sender left it out. Zero
    /// does not mean 1970, it means "not supplied", and a row stamped 1970 falls out of retention
    /// before anyone sees it.
    pub fn wire_seconds(seconds: i64) -> DateTime<Utc> {
        if seconds <= 0 { Utc::now() } else { from_unix_seconds(seconds) }
    }

    /// The same, for milliseconds.
    pub fn wire_millis(millis: i64) -> DateTime<Utc> {
        if millis <= 0 { Utc::now() } else { from_unix_millis(millis) }
    }

    /// A protobuf Timestamp as a time. None when it is absent or not a valid Timestamp: seconds
    /// outside the years 1 to 9999, or nanos outside one second.
    pub fn of_timestamp(timestamp: Option<&pbjson_types::Timestamp>) -> Option<DateTime<Utc>> {
        let t = timestamp?;
        if !(0..1_000_000_000).contains(&t.nanos) {
            return None;
        }
        let time = Utc.timestamp_opt(t.seconds, t.nanos as u32).single()?;
        (min_time()..=max_time()).contains(&time).then_some(time)
    }

    /// A time that may be absent: None unless `present` and positive.
    pub fn optional_seconds(present: bool, seconds: i64) -> Option<DateTime<Utc>> {
        (present && seconds > 0).then(|| from_unix_seconds(seconds))
    }
}

/// A metric type number as its name. Zero is UNSPECIFIED and not rare: Datadog's own
/// documentation example sends it. It is stored as sent; resolving it is a decision for query
/// time.
pub fn metric_type_name(value: i64) -> &'static str {
    match value {
        1 => "COUNT",
        2 => "RATE",
        3 => "GAUGE",
        _ => "UNSPECIFIED",
    }
}

/// Node counts per version, as the unsigned numbers the column holds. A negative count cannot be
/// real and would wrap to billions if cast.
pub fn version_spread(counts: impl IntoIterator<Item = (String, i32)>) -> BTreeMap<String, u32> {
    counts.into_iter().map(|(version, nodes)| (version, nodes.max(0) as u32)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_are_laid_out_as_g() {
        assert_eq!(text::of_float(1.0), "1");
        assert_eq!(text::of_float(0.1), "0.1");
        assert_eq!(text::of_float(123456.0), "123456");
        assert_eq!(text::of_float(1234567.0), "1.234567e+06");
        assert_eq!(text::of_float(0.0001), "0.0001");
        assert_eq!(text::of_float(0.00001), "1e-05");
        assert_eq!(text::of_float(-2.5), "-2.5");
        assert_eq!(text::of_float(f64::NAN), "NaN");
    }

    #[test]
    fn rfc3339_takes_a_zone_and_keeps_100ns() {
        let t = time::try_rfc3339("2026-09-23T10:15:30.123456789+02:00").unwrap();
        assert_eq!(t.to_rfc3339_opts(chrono::SecondsFormat::Nanos, true), "2026-09-23T08:15:30.123456700Z");
        assert!(time::try_rfc3339("2026-09-23 10:15:30Z").is_none());
        assert!(time::try_rfc3339("2026-02-30T10:15:30Z").is_none());
    }

    #[test]
    fn tags_split_at_the_first_colon_and_keep_repeats() {
        let map = tags::to_multi_map(["url:http://x", "a:1", "a:2", "bare"]);
        assert_eq!(map["url"], vec!["http://x"]);
        assert_eq!(map["a"], vec!["1", "2"]);
        assert_eq!(map["bare"], vec![""]);
    }
}
