//! What a row's field becomes in its column, for the types serde alone cannot say. A serializer
//! that is not human-readable is the driver's RowBinary; a human-readable one (JSON) gets the
//! value as the golden fixtures and a log line spell it.

use base64::Engine;
use chrono::{DateTime, SecondsFormat, TimeZone, Utc};
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

fn rfc3339(time: &DateTime<Utc>) -> String {
    time.to_rfc3339_opts(SecondsFormat::AutoSi, true)
}

/// A time inside ClickHouse's own range for the column. One value outside it would otherwise fail
/// the whole batch it travels in.
fn clamp(time: &DateTime<Utc>, earliest: DateTime<Utc>, latest: DateTime<Utc>) -> DateTime<Utc> {
    (*time).clamp(earliest, latest)
}

fn utc(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, second).unwrap()
}

/// `DateTime('UTC')`: whole seconds, 1970 to 2105.
pub mod secs {
    use super::*;

    pub fn serialize<S: Serializer>(time: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return serializer.serialize_str(&rfc3339(time));
        }
        let inside = clamp(time, DateTime::UNIX_EPOCH, utc(2105, 12, 31, 23, 59, 59));
        serializer.serialize_u32(inside.timestamp() as u32)
    }

    struct Secs<'a>(&'a DateTime<Utc>);
    impl Serialize for Secs<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serialize(self.0, serializer)
        }
    }

    /// `Nullable(DateTime('UTC'))`.
    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(time: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error> {
            time.as_ref().map(Secs).serialize(serializer)
        }
    }

    /// `Array(Nullable(DateTime('UTC')))`.
    pub mod options {
        use super::*;

        pub fn serialize<S: Serializer>(times: &[Option<DateTime<Utc>>], serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_seq(times.iter().map(|time| time.as_ref().map(Secs)))
        }
    }
}

/// `DateTime64(3, 'UTC')`: milliseconds, 1900 to 2262.
pub mod millis {
    use super::*;

    pub fn serialize<S: Serializer>(time: &DateTime<Utc>, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return serializer.serialize_str(&rfc3339(time));
        }
        let inside = clamp(time, utc(1900, 1, 1, 0, 0, 0), utc(2262, 4, 11, 23, 47, 16));
        serializer.serialize_i64(inside.timestamp_millis())
    }

    struct Millis<'a>(&'a DateTime<Utc>);
    impl Serialize for Millis<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serialize(self.0, serializer)
        }
    }

    /// `Nullable(DateTime64(3, 'UTC'))`.
    pub mod option {
        use super::*;

        pub fn serialize<S: Serializer>(time: &Option<DateTime<Utc>>, serializer: S) -> Result<S::Ok, S::Error> {
            time.as_ref().map(Millis).serialize(serializer)
        }
    }

    /// `Array(Nullable(DateTime64(3, 'UTC')))`.
    pub mod options {
        use super::*;

        pub fn serialize<S: Serializer>(times: &[Option<DateTime<Utc>>], serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_seq(times.iter().map(|time| time.as_ref().map(Millis)))
        }
    }
}

/// `Map(String, Float64)`. JSON has no NaN or infinity, so as JSON those are the text `NaN`,
/// `+Inf` and `-Inf`.
pub mod float_map {
    use std::collections::BTreeMap;

    use super::*;

    struct Float(f64);
    impl Serialize for Float {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            match self.0 {
                v if v.is_nan() => serializer.serialize_str("NaN"),
                v if v == f64::INFINITY => serializer.serialize_str("+Inf"),
                v if v == f64::NEG_INFINITY => serializer.serialize_str("-Inf"),
                v => serializer.serialize_f64(v),
            }
        }
    }

    pub fn serialize<S: Serializer>(values: &BTreeMap<String, f64>, serializer: S) -> Result<S::Ok, S::Error> {
        if !serializer.is_human_readable() {
            return values.serialize(serializer);
        }
        serializer.collect_map(values.iter().map(|(k, v)| (k, Float(*v))))
    }
}

/// `DateTime64(9, 'UTC')`: nanoseconds since the epoch, written as the number they are. The
/// field is that number, as the wire gives it: an i64 spans 1677 to 2262, inside the column's
/// range.
pub mod nanos {
    use super::*;

    fn time(nanos: i64) -> DateTime<Utc> {
        DateTime::from_timestamp_nanos(nanos)
    }

    pub fn serialize<S: Serializer>(nanos: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return serializer.serialize_str(&rfc3339(&time(*nanos)));
        }
        serializer.serialize_i64(*nanos)
    }

    /// `Array(DateTime64(9, 'UTC'))`.
    pub mod list {
        use super::*;

        pub fn serialize<S: Serializer>(list: &[i64], serializer: S) -> Result<S::Ok, S::Error> {
            struct Nanos(i64);
            impl Serialize for Nanos {
                fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                    super::serialize(&self.0, serializer)
                }
            }
            serializer.collect_seq(list.iter().map(|n| Nanos(*n)))
        }
    }
}

/// A `String` column that holds bytes: a body, an archive. As JSON it is its text when it is
/// UTF-8, else `{"$b64": …}`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Blob(pub Vec<u8>);

impl Serialize for Blob {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if !serializer.is_human_readable() {
            return serializer.serialize_bytes(&self.0);
        }
        match std::str::from_utf8(&self.0) {
            Ok(text) => serializer.serialize_str(text),
            Err(_) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("$b64", &base64::engine::general_purpose::STANDARD.encode(&self.0))?;
                map.end()
            }
        }
    }
}

/// A `JSON` column: the object's text, `{}` for none, since a JSON column must hold an object.
pub fn json_object(text: &str) -> String {
    if text.is_empty() { "{}".into() } else { text.into() }
}

/// An `Array(UInt8)` column of flags, one per element of a parallel array. As JSON it is spelled
/// as bytes are, like [`Blob`].
pub mod uint8_array {
    use super::*;

    pub fn serialize<S: Serializer>(values: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            return Blob(values.to_vec()).serialize(serializer);
        }
        values.serialize(serializer)
    }
}
