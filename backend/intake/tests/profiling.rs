//! Tests of routers/profiling.rs beyond the golden fixtures, ported from the F# ProfilingTests.

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;

use base64::Engine;
use bytes::Bytes;
use chrono::{DateTime, SecondsFormat, Utc};
use http_body_util::BodyExt;
use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::json;
use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::routers::pprof::{self, Summary};
use ninjapaws_intake::routers::profiling;
use ninjapaws_intake::server::{Intake, Routes, router};
use ninjapaws_proto::perftools::profiles::{Function, Line, Location, Profile, Sample, ValueType};
use ninjapaws_storage::{Sink, Write as Batch};
use prost::Message;
use serde_json::Value;

const KEY: &str = "00000000000000000000000000000001";
const BOUNDARY: &str = "profiling-tests-boundary";

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

/// A small but real gzip-compressed pprof profile: one sample, one location, one function, a
/// sample type and a period, and no mapping at all — which is legitimate (a Go binary with inlined
/// symbols has none).
fn pprof_bytes() -> Vec<u8> {
    let profile = Profile {
        time_nanos: 1_700_000_000_000_000_000,
        duration_nanos: 10_000_000_000,
        period: 10_000_000,
        string_table: ["", "cpu", "nanoseconds", "main.main", "main.go"].map(String::from).to_vec(),
        sample_type: vec![ValueType { r#type: 1, unit: 2 }],
        period_type: Some(ValueType { r#type: 1, unit: 2 }),
        sample: vec![Sample { location_id: vec![1], value: vec![100], label: vec![] }],
        location: vec![Location {
            id: 1,
            address: 0x1000,
            line: vec![Line { function_id: 1, line: 42, ..Default::default() }],
            ..Default::default()
        }],
        function: vec![Function { id: 1, name: 3, system_name: 3, filename: 4, ..Default::default() }],
        ..Default::default()
    };
    gzip(&profile.encode_to_vec())
}

/// A multipart/form-data body of the given form fields, and its Content-Type.
fn multipart(parts: &[(&str, &[u8])]) -> (String, Vec<u8>) {
    let mut body = Vec::new();
    for (name, data) in parts {
        write!(body, "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n").unwrap();
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    write!(body, "--{BOUNDARY}--\r\n").unwrap();
    (format!("multipart/form-data; boundary={BOUNDARY}"), body)
}

/// One POST with the test key to a host's routes: the status, the body and what was written.
async fn post(mount: fn(&mut Routes), url: &str, content_type: &str, body: &[u8]) -> (u16, Vec<u8>, Vec<Batch>) {
    let store = Store::default();
    store.publish([(keys::hash(KEY), Key { id: "test".into(), name: "test".into(), tenant_id: "test".into() })]);
    let (sink, capture) = Sink::capture();
    let intake = Intake { sink, keys: Arc::new(store), ack_unknown: false, max_body_bytes: 64 * 1024 * 1024 };
    let router = router(intake, Some(Scheme::Agent), mount);
    let mut request = http::Request::builder().method("POST").uri(url).header("dd-api-key", KEY);
    if !content_type.is_empty() {
        request = request.header("content-type", content_type);
    }
    let response = router.dispatch(request.body(body.to_vec().into()).unwrap()).await;
    let status = response.status().as_u16();
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, body, capture.writes())
}

fn rows<'a>(writes: &'a [Batch], table: &str) -> Vec<&'a Value> {
    writes.iter().filter(|w| w.table == table).flat_map(|w| &w.rows).collect()
}

fn parts_of(parts: &[(&str, &[u8])]) -> BTreeMap<String, Bytes> {
    parts.iter().map(|(name, data)| (name.to_string(), Bytes::copy_from_slice(data))).collect()
}

fn time(text: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(text).unwrap().to_utc()
}

/// One part that is pprof, one that is not, and an event with a repeated tag key and a key no
/// column reads.
#[test]
fn a_profile_row_keeps_the_event_whole_and_describes_each_attachment() {
    let event = br#"{
        "family": "go", "version": "1.2.3", "runtime": "go1.23", "language": "go",
        "end": "2026-09-23T10:30:00Z",
        "tags_profiler": "service:my-svc,env:prod,also:x,also:y",
        "custom_field": "from a newer agent"
    }"#;
    let cpu = pprof_bytes();
    let parts = parts_of(&[("event", event), ("cpu.pprof", &cpu), ("notes.txt", b"not a profile")]);
    let profiles: BTreeMap<String, Summary> = ["cpu.pprof", "notes.txt"]
        .into_iter()
        .filter_map(|name| profiling::read_pprof(&parts[name]).map(|p| (name.to_string(), p)))
        .collect();
    let decoded = json::try_parse(event).ok();

    let row =
        profiling::profile_row("test-tenant", "profile", "dd-trace-go", "1.60.0", &parts, decoded.as_ref(), &profiles);

    assert_eq!(row.tenant_id, "test-tenant");
    assert_eq!(row.variant, "profile");
    assert_eq!((row.dd_evp_origin.as_str(), row.dd_evp_origin_version.as_str()), ("dd-trace-go", "1.60.0"));
    assert!(String::from_utf8_lossy(&row.event.0).contains("custom_field"));
    assert_eq!([&row.family, &row.version, &row.runtime, &row.language], ["go", "1.2.3", "go1.23", "go"]);
    // start was never sent: absent, not a zero time.
    assert_eq!(row.start_raw, "");
    assert_eq!(row.start_parsed, None);
    assert_eq!(row.end_raw, "2026-09-23T10:30:00Z");
    assert_eq!(row.end_parsed, Some(time("2026-09-23T10:30:00Z")));
    assert_eq!(row.tags_profiler["also"], ["x", "y"]);

    // Attachments in name order: cpu.pprof before notes.txt.
    assert_eq!(row.attach_name, ["cpu.pprof", "notes.txt"]);
    assert_eq!(row.attach_parsed, [1, 0]);
    assert_eq!(row.attach_sample_types, [vec!["cpu".to_string()], vec![]]);
    assert_eq!(row.attach_sample_units, [vec!["nanoseconds".to_string()], vec![]]);
    assert_eq!(row.attach_period_type, ["cpu/nanoseconds", ""]);
    assert_eq!(row.attach_mapping_count, [0, 0]);
    assert_eq!(row.attach_location_count, [1, 0]);
    assert_eq!(row.attach_function_count, [1, 0]);
    assert_eq!(row.attach_sample_count, [1, 0]);
    assert_eq!(row.attach_period, [10_000_000, 0]);
    assert_eq!(row.attach_bytes[1].0, b"not a profile");
    assert_eq!(row.attach_size, [cpu.len() as u64, 13]);
}

#[test]
fn a_profile_row_without_an_event_part_still_lists_the_attachments() {
    let parts = parts_of(&[("cpu.pprof", b"not actually pprof either")]);
    let row = profiling::profile_row("t", "profile-v1", "", "", &parts, None, &BTreeMap::new());

    assert!(row.event.0.is_empty());
    assert_eq!(row.family, "");
    assert!(row.tags_profiler.is_empty());
    assert_eq!(row.attach_name, ["cpu.pprof"]);
    assert_eq!(row.attach_parsed, [0]);
}

/// start and end are kept as the text on the wire next to the parsed time. The shared JSON reader
/// holds a number as a float, so `1.0e3` and `1.50` come back as the float writes them; the F#
/// server kept the literal.
#[test]
fn start_is_kept_as_the_text_it_had_on_the_wire() {
    let cases = [
        ("1758623400", "1758623400"),
        ("\"2025-09-23T10:30:00Z\"", "2025-09-23T10:30:00Z"),
        ("true", "true"),
        ("null", ""),
        // Anything else is kept as JSON, keys sorted.
        (r#"{"b":[1,"x",null,true,{"k":1.25}],"a":"s"}"#, r#"{"a":"s","b":[1,"x",null,true,{"k":1.25}]}"#),
        ("[]", "[]"),
        ("{}", "{}"),
    ];
    for (start, expected) in cases {
        let event = json::try_parse(format!("{{\"start\": {start}}}").as_bytes()).unwrap();
        let row = profiling::profile_row("t", "profile", "", "", &BTreeMap::new(), Some(&event), &BTreeMap::new());
        assert_eq!(row.start_raw, expected, "{start}");
    }
}

#[test]
fn timestamps_are_rfc3339_text_or_an_epoch_whose_magnitude_gives_the_unit() {
    let cases = [
        ("\"2025-09-23T10:30:00Z\"", "2025-09-23T10:30:00.0000000Z"),
        ("\"2025-09-23T10:30:00.5Z\"", "2025-09-23T10:30:00.5000000Z"),
        ("\"2025-09-23T12:30:00+02:00\"", "2025-09-23T10:30:00.0000000Z"),
        // A comma before the fraction, as ISO 8601 allows.
        ("\"2025-09-23T10:30:00,5Z\"", "2025-09-23T10:30:00.5000000Z"),
        // Digits past the seventh are cut off, not rounded up.
        ("\"2025-09-23T10:30:00.99999999Z\"", "2025-09-23T10:30:00.9999999Z"),
        ("\"2025-09-23T10:30:00.123456789012Z\"", "2025-09-23T10:30:00.1234567Z"),
        ("1758623400", "2025-09-23T10:30:00.0000000Z"),
        ("1758623400000", "2025-09-23T10:30:00.0000000Z"),
        ("1758623400000000", "2025-09-23T10:30:00.0000000Z"),
        ("1758623400000000000", "2025-09-23T10:30:00.0000000Z"),
        ("1758623400123456789", "2025-09-23T10:30:00.1234567Z"),
        ("1758623400123456", "2025-09-23T10:30:00.1234560Z"),
        ("0", ""),
        ("-5", ""),
        ("1758623400.5", ""),
        ("1e9", ""),
        ("\"not a time\"", ""),
        ("\"1758623400\"", ""),
        ("true", ""),
        // The first second of year 10000: kept at the limit.
        ("253402300800", "9999-12-31T23:59:59.0000000Z"),
    ];
    for (value, expected) in cases {
        let parsed = profiling::parse_timestamp(Some(&json::try_parse(value.as_bytes()).unwrap()));
        let shown = parsed.map(|t| t.to_rfc3339_opts(SecondsFormat::Nanos, true)).map(|t| {
            // As .NET's "o" writes it: seven digits of fraction.
            let (seconds, fraction) = t.trim_end_matches('Z').split_once('.').unwrap();
            format!("{seconds}.{}Z", &fraction[..7])
        });
        assert_eq!(shown.unwrap_or_default(), expected, "{value}");
    }
}

#[test]
fn an_absent_timestamp_is_no_time() {
    assert_eq!(profiling::parse_timestamp(None), None);
}

fn describe(summary: Option<Summary>) -> String {
    match summary {
        None => "rejected".into(),
        Some(s) => format!(
            "types={} units={} samples={} time={} duration={} period={}:{} mappings={} locations={} functions={}",
            s.sample_types.join(","),
            s.sample_units.join(","),
            s.sample_count,
            s.time_nanos,
            s.duration_nanos,
            s.period_type,
            s.period,
            s.mapping_count,
            s.location_count,
            s.function_count
        ),
    }
}

const FULL: &str = "types=cpu units=nanoseconds samples=1 time=1700000000000000000 duration=10000000000 period=cpu/nanoseconds:10000000 mappings=0 locations=1 functions=1";
const EMPTY: &str = "types= units= samples=0 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0";

/// Bytes (base64) and the verdict on them. The verdicts were produced by running
/// github.com/google/pprof/profile's ParseData on the same bytes; the ones with a comment are where
/// protobuf's own rules, which the generated parser follows, say otherwise.
const PPROF_VERDICTS: &[(&str, &str, &str)] = &[
    (
        "full",
        "CgQIARACEgQIARBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdvSICAqLHjn+fLF1CAyK+gJVoECAEQAmCAreIE",
        FULL,
    ),
    (
        "full gzip",
        "H4sIAAAAAAAA/wBmAJn/CgQIARACEgQIARBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdvSICAqLHjn+fLF1CAyK+gJVoECAEQAmCAreIEAwARwMXuZgAAAA==",
        FULL,
    ),
    ("only an empty string", "MgA=", EMPTY),
    ("empty input", "", "rejected"),
    ("gzip of nothing", "H4sIAAAAAAAA/wMAAAAAAAAAAAA=", "rejected"),
    ("no string table", "YAU=", "rejected"),
    ("first string not empty", "MgF4", "rejected"),
    (
        "two sample types one period",
        "CgQIARACCgQIAxAEMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nb2AH",
        "types=cpu,main.main units=nanoseconds,main.go samples=0 time=0 duration=0 period=/:7 mappings=0 locations=0 functions=0",
    ),
    (
        "sample with too many values",
        "CgQIARACEgcIARBkEMgBIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv",
        "rejected",
    ),
    (
        "sample without sample types",
        "EgIIASILCAEYgCAiBAgBECoqCAgBEAMYAyAEMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==",
        "rejected",
    ),
    (
        "sample at unknown location",
        "CgQIARACEgQICRBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv",
        "rejected",
    ),
    (
        "sample without locations",
        "CgQIARACEgIQZDIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28=",
        "types=cpu units=nanoseconds samples=1 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0",
    ),
    (
        "duplicate function ids",
        "KggIARADGAMgBCoICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv",
        "rejected",
    ),
    ("function id zero", "KggIABADGAMgBDIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28=", "rejected"),
    (
        "duplicate location ids",
        "IgsIARiAICIECAEQKiILCAEYgCAiBAgBECoqCAgBEAMYAyAEMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==",
        "rejected",
    ),
    (
        "location id zero",
        "IgsIABiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv",
        "rejected",
    ),
    (
        "line without function",
        "IgsIARiAICIECAAQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv",
        "rejected",
    ),
    (
        "line with unknown function",
        "IgsIARiAICIECAcQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv",
        "rejected",
    ),
    (
        "location without lines",
        "IgQIARAHMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==",
        "types= units= samples=0 time=0 duration=0 period=/:0 mappings=0 locations=1 functions=0",
    ),
    ("mapping id zero", "GgMQgCAyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv", "rejected"),
    ("duplicate mapping ids", "GgIIARoCCAEyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv", "rejected"),
    (
        "one mapping",
        "GgQIASgEMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==",
        "types= units= samples=0 time=0 duration=0 period=/:0 mappings=1 locations=0 functions=0",
    ),
    ("mapping file out of range", "GgQIASgJMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==", "rejected"),
    ("sample type out of range", "CgQIARAJMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==", "rejected"),
    (
        "negative string index",
        "Cg0IARD///////////8BMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==",
        "rejected",
    ),
    ("function name out of range", "KgQIARBjMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==", "rejected"),
    ("period type out of range", "WgQICRABMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==", "rejected"),
    ("drop frames out of range", "OAkyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv", "rejected"),
    ("comment out of range", "aAkyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv", "rejected"),
    ("default sample type out of range", "cAkyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv", "rejected"),
    ("doc url out of range", "eAkyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv", "rejected"),
    ("label key out of range", "CgQIARACEgYQARoCCAkyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv", "rejected"),
    (
        "label str out of range",
        "CgQIARACEggQARoECAEQCTIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28=",
        "rejected",
    ),
    (
        "label unit out of range",
        "CgQIARACEgoQARoGCAEYBSAJMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==",
        "rejected",
    ),
    (
        "label unit ignored beside str",
        "CgQIARACEgoQARoGCAEQAiAJMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==",
        "types=cpu units=nanoseconds samples=1 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0",
    ),
    // protobuf: of a field sent twice, the last one counts. Go's library took it for two profiles
    // glued together.
    (
        "time_nanos twice",
        "SAVIBjIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28=",
        "types= units= samples=0 time=6 duration=0 period=/:0 mappings=0 locations=0 functions=0",
    ),
    (
        "time_nanos zero then set",
        "SABIBjIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28=",
        "types= units= samples=0 time=6 duration=0 period=/:0 mappings=0 locations=0 functions=0",
    ),
    // A known field in another wire type: prost refuses it, as Go's library did. The F# server's
    // protobuf library skipped it as an unknown field.
    ("field of the wrong wire type", "SgFBMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==", "rejected"),
    ("unknown field of any type", "mgYBQZAGB7UGAQIDBDIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28=", EMPTY),
    // protobuf: a group is skipped like any unknown field. Go's library refused it.
    ("group wire type", "owakBjIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28=", EMPTY),
    // protobuf: a string is UTF-8, and the parser holds a profile to it. Go's library read strings
    // as bytes.
    ("strings that are not UTF-8", "CgQIARABMgAyAv/+", "rejected"),
    ("drop frames twice last wins", "OAk4ATIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28=", EMPTY),
    // protobuf: a message sent twice is merged field by field. Go's library kept the second alone.
    (
        "period type twice",
        "WgQIARACWgIIAzIAMgNjcHUyC25hbm9zZWNvbmRzMgltYWluLm1haW4yB21haW4uZ28=",
        "types= units= samples=0 time=0 duration=0 period=main.main/nanoseconds:0 mappings=0 locations=0 functions=0",
    ),
    (
        "packed and single values",
        "CgQIARACCgQIARACEgQSAgECEgQQARACMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbw==",
        "types=cpu,cpu units=nanoseconds,nanoseconds samples=2 time=0 duration=0 period=/:0 mappings=0 locations=0 functions=0",
    ),
    ("varint of eleven bytes", "YAFg/////////////wEyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv", "rejected"),
    // Ten bytes whose last one carries more than the 64th bit: prost refuses it as Go's library
    // did. The F# server's library dropped the extra bits and read -1.
    ("varint of ten bytes", "YP///////////38yADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdv", "rejected"),
    ("length past the end", "MgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nbzIJYQ==", "rejected"),
    (
        "gzip that breaks off",
        "H4sIAAAAAAAA/wBmAJn/CgQIARACEgQIARBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdvSICAqLHjn+fLF1CAyK+gJVoECAEQAmCArQ==",
        "rejected",
    ),
    (
        "gzip then garbage",
        "H4sIAAAAAAAA/wBmAJn/CgQIARACEgQIARBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdvSICAqLHjn+fLF1CAyK+gJVoECAEQAmCAreIEAwARwMXuZgAAAAECAw==",
        "rejected",
    ),
    (
        "two gzip members",
        "H4sIAAAAAAAA/wBmAJn/CgQIARACEgQIARBkIgsIARiAICIECAEQKioICAEQAxgDIAQyADIDY3B1MgtuYW5vc2Vjb25kczIJbWFpbi5tYWluMgdtYWluLmdvSICAqLHjn+fLF1CAyK+gJVoECAEQAmCAreIEAwARwMXuZgAAAB+LCAAAAAAAAP8AZgCZ/woECAEQAhIECAEQZCILCAEYgCAiBAgBECoqCAgBEAMYAyAEMgAyA2NwdTILbmFub3NlY29uZHMyCW1haW4ubWFpbjIHbWFpbi5nb0iAgKix45/nyxdQgMivoCVaBAgBEAJggK3iBAMAEcDF7mYAAAA=",
        "rejected",
    ),
    ("not a profile", "bm90IGEgcHJvZmlsZQ==", "rejected"),
    ("json", "eyJhIjoxfQ==", "rejected"),
    ("gzip of text", "H4sIAAAAAAAA/wANAPL/bm90IGEgcHJvZmlsZQMAdMNUDg0AAAA=", "rejected"),
];

#[test]
fn pprof_is_accepted_rejected_and_summarised() {
    let wrong: Vec<String> = PPROF_VERDICTS
        .iter()
        .filter_map(|(name, data, expected)| {
            let bytes = base64::engine::general_purpose::STANDARD.decode(data).unwrap();
            let got = describe(profiling::read_pprof(&bytes));
            (got != *expected).then(|| format!("{name}: expected {expected}, got {got}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_test_profile_is_one_the_reader_accepts() {
    assert_eq!(describe(profiling::read_pprof(&pprof_bytes())), FULL);
    assert_eq!(pprof::read(b""), None);
}

fn texts(entries: Option<Vec<&[u8]>>) -> Vec<String> {
    match entries {
        Some(found) => found.into_iter().map(|e| String::from_utf8_lossy(e).into_owned()).collect(),
        None => vec!["(none)".into()],
    }
}

#[test]
fn a_json_array_of_logs_gives_each_element_as_it_was_written() {
    let body = br#" [{"service":"svc-a","ddsource":"dd_debugger"}, {"service" : "svc-b", "n": 1.50},null,7] "#;
    assert_eq!(
        texts(profiling::decode_logs(body)),
        [r#"{"service":"svc-a","ddsource":"dd_debugger"}"#, r#"{"service" : "svc-b", "n": 1.50}"#, "null", "7"]
    );
}

/// The browser SDK's debugger track sends one object per line.
#[test]
fn ndjson_logs_give_one_entry_per_non_empty_line() {
    let body = b"\n{\"service\":\"svc-a\"}\r\n\n  {\"service\":\"svc-b\"} \t\n{broken\n";
    assert_eq!(texts(profiling::decode_logs(body)), [r#"{"service":"svc-a"}"#, r#"{"service":"svc-b"}"#, "{broken"]);
}

#[test]
fn a_body_that_is_neither_an_array_nor_ndjson_has_no_log_entries() {
    for body in ["not json at all", "   ", "", "[1, 2", "[1] trailing", "\"text\""] {
        assert!(profiling::decode_logs(body.as_bytes()).is_none(), "{body}");
    }
}

#[test]
fn an_empty_array_is_a_batch_with_no_entries_not_a_failure() {
    assert_eq!(profiling::decode_logs(b"[]"), Some(vec![]));
}

#[tokio::test]
async fn ndjson_logs_are_stored_one_row_per_line() {
    let body = b"{\"service\":\"svc-a\",\"b\":1,\"a\":2,\"a\":3}\nnot json\n";
    let (status, _, writes) =
        post(profiling::mount_debugger, "/api/v2/debugger?ddtags=env:prod,env:dev", "text/plain", body).await;

    assert_eq!(status, 202);
    let rows = rows(&writes, "debugger_logs");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["service"], "svc-a");
    assert_eq!(rows[0]["extra_keys"], serde_json::json!(["a", "b"]));
    assert_eq!(rows[0]["ddtags"]["env"], serde_json::json!(["prod", "dev"]));
    // A line that is not an object keeps its bytes and has no columns.
    assert_eq!(rows[1]["entry"], "not json");
    assert_eq!(rows[1]["service"], "");
    assert_eq!(rows[1]["extra_keys"], serde_json::json!([]));
    assert!(writes.iter().all(|w| w.table != "raw_payloads"));
}

/// Lines are trimmed of what Go's bytes.TrimSpace takes for white space, which is Unicode's.
#[test]
fn ndjson_lines_lose_unicode_white_space_at_both_ends() {
    let body = "{\"service\":\"a\"}\u{a0}\n\u{2003}{\"service\":\"b\"}\u{85}\n\u{b}{\"service\":\"c\"}\u{c}\n";
    assert_eq!(
        texts(profiling::decode_logs(body.as_bytes())),
        [r#"{"service":"a"}"#, r#"{"service":"b"}"#, r#"{"service":"c"}"#]
    );
}

/// Go's decoder puts U+FFFD where a string is not text and carries on. The entry itself is kept
/// byte for byte.
#[tokio::test]
async fn text_that_is_not_valid_unicode_is_replaced_not_refused() {
    let entry = [
        &b"{\"service\":\"a"[..],
        &[0xFF],
        &b"\",\"ddsource\":\"x\\ud83d\\n\\\"\\u00e9\",\"k\\udc00\":1,\"z\\ud83d\\ude00\":2}"[..],
    ]
    .concat();
    let body = [&b"["[..], &entry, b"]"].concat();
    let (status, _, writes) = post(profiling::mount_debugger, "/api/v2/debugger", "application/json", &body).await;

    assert_eq!(status, 202);
    let rows = rows(&writes, "debugger_logs");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["service"], "a\u{FFFD}");
    assert_eq!(rows[0]["ddsource"], "x\u{FFFD}\n\"\u{e9}");
    assert_eq!(rows[0]["extra_keys"], serde_json::json!(["k\u{FFFD}", "z\u{1F600}"]));
    assert_eq!(
        rows[0]["entry"]["$b64"],
        base64::engine::general_purpose::STANDARD.encode(&entry),
        "the entry is kept byte for byte"
    );
}

#[test]
fn a_profile_event_with_broken_text_is_still_read() {
    let event =
        [&b"{\"family\":\"g"[..], &[0xFF], b"o\",\"version\":\"\\ud800x\",\"start\":\"", &[0xFF], b"\"}"].concat();
    let decoded = json::try_parse(&event).ok();
    let row = profiling::profile_row(
        "t",
        "profile",
        "",
        "",
        &parts_of(&[("event", &event)]),
        decoded.as_ref(),
        &BTreeMap::new(),
    );

    assert_eq!(row.family, "g\u{FFFD}o");
    assert_eq!(row.version, "\u{FFFD}x");
    assert_eq!(row.start_raw, "\u{FFFD}");
    assert_eq!(row.start_parsed, None);
    assert_eq!(row.event.0, event);
}

/// The shared reader nests 128 deep (the F# one 512): an event inside that is still read.
#[test]
fn json_nested_deeply_is_still_read() {
    let deep = format!("{}{}", "[".repeat(100), "]".repeat(100));
    let event = json::try_parse(format!("{{\"family\": \"go\", \"start\": {deep}}}").as_bytes()).unwrap();
    let row = profiling::profile_row("t", "profile", "", "", &BTreeMap::new(), Some(&event), &BTreeMap::new());

    assert_eq!(row.family, "go");
    assert_eq!(row.start_raw, deep);
    assert_eq!(profiling::decode_logs(format!("[{{\"deep\": {deep}}}]").as_bytes()).map(|e| e.len()), Some(1));
}

#[tokio::test]
async fn logs_that_are_not_json_are_kept_raw() {
    let (status, _, writes) =
        post(profiling::mount_debugger, "/api/v2/debugger", "text/plain", b"not json at all").await;

    assert_eq!(status, 202);
    assert_eq!(writes.len(), 1);
    let raw = &writes[0].rows[0];
    assert_eq!((raw["intake"].as_str(), raw["reason"].as_str()), (Some("debugger"), Some("decode_error")));
    assert_eq!(raw["note"], "logs variant: not a JSON array or NDJSON, content-type text/plain");
}

#[tokio::test]
async fn a_diagnostics_entry_that_is_not_an_object_still_gets_its_row() {
    let (content_type, body) = multipart(&[(
        "event",
        br#"[{"service":"svc","timestamp":"2025-09-23T10:30:00Z","debugger":{"diagnostics":{"probeId":"p1"}}}, 42]"#,
    )]);
    let (_, _, writes) = post(profiling::mount_debugger, "/api/v2/debugger", &content_type, &body).await;

    let rows = rows(&writes, "debugger_diagnostics");
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["probe_id"], "p1");
    assert_eq!(rows[0]["timestamp"], "2025-09-23T10:30:00Z");
    // No exception object: both are absent, not empty strings.
    assert_eq!(rows[0]["exception_type"], Value::Null);
    assert_eq!(rows[0]["exception_message"], Value::Null);
    assert_eq!(rows[1]["message"], "42");
    assert_eq!(rows[1]["service"], "");
    assert_eq!(rows[1]["timestamp"], Value::Null);
}

#[test]
fn an_exception_without_a_message_is_an_empty_message_not_an_absent_one() {
    let message = br#"{"debugger":{"diagnostics":{"exception":{"type":"NPE"}}}}"#;
    let decoded = json::try_parse(message).ok();
    let row = profiling::diagnostic_row("t", Utc::now(), &Default::default(), message, decoded.as_ref());

    assert_eq!(row.exception_type.as_deref(), Some("NPE"));
    assert_eq!(row.exception_message.as_deref(), Some(""));
}

#[tokio::test]
async fn an_empty_diagnostics_batch_stores_nothing() {
    let (content_type, body) = multipart(&[("event", b"[]")]);
    let (status, _, writes) = post(profiling::mount_debugger, "/api/v2/debugger", &content_type, &body).await;

    assert_eq!(status, 202);
    assert!(writes.is_empty());
}

#[test]
fn a_symdb_row_keeps_both_spellings_of_the_upload() {
    let file = gzip(
        br#"{
            "service": "env-svc", "version": "9.9.9", "language": "go",
            "upload_id": "env-upload-1", "batch_num": 7, "final": true,
            "scopes": [{"scope_type": "package", "name": "main", "scopes": []}, null]
        }"#,
    );
    let event = br#"{
            "service": "evt-svc", "version": "1.0.0", "language": "go",
            "runtimeId": "rt-1", "uploadId": "evt-upload-1", "batchNum": "3",
            "final": false, "attachmentSize": 4096
        }"#;
    let symdb = profiling::decode_symdb_file(&file);
    let decoded = json::try_parse(event).ok();
    let row = profiling::symdb_upload_row("t", event, &file, "env:prod,team:x", decoded.as_ref(), &symdb);

    assert_eq!(row.event.0, event);
    assert_eq!(row.file.0, file);
    assert_eq!(row.ddtags["env"], ["prod"]);
    assert_eq!((row.service.as_str(), row.env_service.as_str()), ("evt-svc", "env-svc"));
    assert_eq!((row.upload_id.as_str(), row.env_upload_id.as_str()), ("evt-upload-1", "env-upload-1"));
    // A string loses its quotes; anything else keeps its JSON form.
    assert_eq!([&row.batch_num, &row.env_batch_num, &row.env_final], ["3", "7", "true"]);
    assert_eq!(row.runtime_id, "rt-1");
    assert_eq!(row.r#final, Some(0));
    assert_eq!(row.attachment_size, Some(4096));
    assert_eq!(row.scopes_ok, 1);
    assert_eq!(row.scope_count, 2);
    assert!(row.inflated_size > 0);
}

#[test]
fn missing_or_malformed_scopes_leave_the_envelope_standing() {
    for envelope in [
        r#"{"service": "s", "upload_id": "u"}"#,
        r#"{"service": "s", "scopes": null}"#,
        r#"{"service": "s", "scopes": {"not": "a list"}}"#,
        r#"{"service": "s", "scopes": [{"name": "ok"}, "not a scope"]}"#,
    ] {
        let symdb = profiling::decode_symdb_file(&gzip(envelope.as_bytes()));
        assert!(symdb.envelope.is_some(), "{envelope}");
        assert_eq!(symdb.scope_count, None, "{envelope}");
        assert_eq!(symdb.inflated_size, envelope.len());

        let row = profiling::symdb_upload_row("t", b"", b"", "", None, &symdb);
        assert_eq!(row.env_service, "s");
        assert_eq!(row.scopes_ok, 0);
    }
}

#[test]
fn a_file_that_is_not_gzip_keeps_its_bytes_and_nothing_else() {
    let garbage = b"not gzip at all";
    let symdb = profiling::decode_symdb_file(garbage);
    let row = profiling::symdb_upload_row("t", br#"{"service":"s"}"#, garbage, "", None, &symdb);

    assert_eq!(row.file.0, garbage);
    assert_eq!(row.scopes_ok, 0);
    assert_eq!(row.inflated_size, 0);
    assert_eq!(row.env_service, "");
    assert!(row.ddtags.is_empty());
}

#[test]
fn gzip_that_does_not_hold_a_json_object_still_reports_its_size() {
    let symdb = profiling::decode_symdb_file(&gzip(b"[1, 2, 3]"));
    assert!(symdb.envelope.is_none());
    assert_eq!(symdb.scope_count, None);
    assert_eq!(symdb.inflated_size, 9);
}

#[test]
fn a_gzip_stream_that_breaks_off_or_has_bytes_after_its_end_is_not_read() {
    let whole = gzip(br#"{"service": "s", "scopes": []}"#);
    assert!(profiling::decode_symdb_file(&whole).envelope.is_some());

    let broken = [whole[..whole.len() - 6].to_vec(), [&whole[..], &[9, 9]].concat(), vec![], vec![0x1F, 0x8B]];
    for file in broken {
        let symdb = profiling::decode_symdb_file(&file);
        assert!(symdb.envelope.is_none());
        assert_eq!(symdb.scope_count, None);
        assert_eq!(symdb.inflated_size, 0);
    }
}

/// uploadId and batchNum are identifiers: a producer that sends them as bare numbers must not lose
/// them.
#[test]
fn a_numeric_upload_id_is_kept_as_its_digits() {
    let event = br#"{"uploadId": 42, "batchNum": 12345678901234567890}"#;
    let decoded = json::try_parse(event).ok();
    let row = profiling::symdb_upload_row("t", event, b"", "", decoded.as_ref(), &profiling::decode_symdb_file(b""));

    assert_eq!(row.upload_id, "42");
    assert_eq!(row.batch_num, "12345678901234567890");
    assert_eq!(row.r#final, None);
    assert_eq!(row.attachment_size, None);
}

#[test]
fn an_attachment_size_that_is_not_a_whole_non_negative_number_is_absent() {
    for size in ["-1", "1.5", "\"4096\""] {
        let event = format!("{{\"attachmentSize\": {size}, \"final\": \"yes\"}}");
        let decoded = json::try_parse(event.as_bytes()).ok();
        let row = profiling::symdb_upload_row(
            "t",
            event.as_bytes(),
            b"",
            "",
            decoded.as_ref(),
            &profiling::decode_symdb_file(b""),
        );
        assert_eq!(row.attachment_size, None, "{size}");
        assert_eq!(row.r#final, None, "{size}");
    }
}

/// The six ident bytes elf_info reads, padded to the sixteen a real ident has.
fn fake_elf(class: u8, endianness: u8) -> Vec<u8> {
    [&[0x7F, b'E', b'L', b'F', class, endianness][..], &[0; 10]].concat()
}

#[test]
fn elf_info_reads_the_class_and_byte_order_off_the_ident_bytes() {
    assert_eq!(profiling::elf_info(&fake_elf(2, 1)), ("ELF64", "LE"));
    assert_eq!(profiling::elf_info(&fake_elf(1, 2)), ("ELF32", "BE"));
    assert_eq!(profiling::elf_info(&fake_elf(9, 0)), ("ELF?", "?"));
    assert_eq!(profiling::elf_info(b"not an elf file"), ("", ""));
    assert_eq!(profiling::elf_info(&[0x7F, b'E', b'L', b'F', 2]), ("", ""));
}

#[test]
fn a_symbol_upload_row_reads_the_metadata_and_keeps_the_parts_it_does_not_name() {
    let meta = br#"{
            "type": "elf_symbol_file", "arch": "amd64",
            "gnu_build_id": "abc123", "go_build_id": "",
            "file_hash": "deadbeef", "symbol_source": "agent",
            "origin": "profiler", "origin_version": "7.60.0",
            "filename": "myapp"
        }"#;
    let elf = fake_elf(2, 1);
    let parts =
        parts_of(&[("event", meta), ("elf_symbol_file", &elf), ("debug_extra", b"a part this router does not name")]);
    let decoded = json::try_parse(meta).ok();
    let row = profiling::symbol_upload_row("t", &parts, decoded.as_ref());

    assert_eq!(
        [
            &row.r#type,
            &row.arch,
            &row.gnu_build_id,
            &row.go_build_id,
            &row.file_hash,
            &row.symbol_source,
            &row.origin,
            &row.origin_version,
            &row.filename
        ],
        ["elf_symbol_file", "amd64", "abc123", "", "deadbeef", "agent", "profiler", "7.60.0", "myapp"]
    );
    assert_eq!(row.has_elf, 1);
    assert_eq!((row.elf_class.as_str(), row.elf_endianness.as_str()), ("ELF64", "LE"));
    assert_eq!(row.elf_size, 16);
    assert_eq!(row.elf.0, elf);
    assert_eq!(row.meta.0, meta);
    assert_eq!(row.other_parts.keys().collect::<Vec<_>>(), ["debug_extra"]);
    assert_eq!(row.other_parts["debug_extra"].0, b"a part this router does not name");
}

#[test]
fn a_symbol_upload_without_an_elf_part_says_so() {
    let meta = br#"{"type":"x"}"#;
    let decoded = json::try_parse(meta).ok();
    let row = profiling::symbol_upload_row("t", &parts_of(&[("event", meta)]), decoded.as_ref());

    assert_eq!(row.r#type, "x");
    assert_eq!(row.has_elf, 0);
    assert_eq!((row.elf_class.as_str(), row.elf_endianness.as_str()), ("", ""));
    assert_eq!(row.elf_size, 0);
    assert!(row.elf.0.is_empty());
    assert!(row.other_parts.is_empty());
}

#[tokio::test]
async fn the_old_profile_path_stores_the_same_row_under_its_own_variant() {
    let cpu = pprof_bytes();
    let (content_type, body) = multipart(&[("event", br#"{"family":"go"}"#), ("cpu.pprof", &cpu)]);
    let (status, answer, writes) = post(profiling::mount_profile, "/v1/input", &content_type, &body).await;

    assert_eq!(status, 202);
    assert_eq!(answer, b"{}");
    let rows = rows(&writes, "profiles");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["variant"], "profile-v1");
    assert_eq!(rows[0]["tenant_id"], "test");
    assert_eq!(rows[0]["attach_parsed"], "\u{1}");
}

#[tokio::test]
async fn a_repeated_part_name_keeps_the_last_part() {
    let (content_type, body) = multipart(&[("notes", b"first"), ("event", b"{}"), ("notes", b"second")]);
    let (_, _, writes) = post(profiling::mount_profile, "/api/v2/profile", &content_type, &body).await;

    let rows = rows(&writes, "profiles");
    assert_eq!(rows[0]["attach_name"], serde_json::json!(["notes"]));
    assert_eq!(rows[0]["attach_bytes"], serde_json::json!(["second"]));
}

/// The notes say why a body is not multipart. Three content types the F# server called "not a
/// media type" (`a/b/c`, `a b`, `multipart/`) are read by the shared reader as media types it does
/// not take for multipart, and are not listed.
#[tokio::test]
async fn a_profile_that_is_not_multipart_is_kept_raw_with_the_reason() {
    let cases = [
        ("", "not multipart, content-type (no content-type): the content type is not a media type"),
        ("text/plain", "not multipart, content-type text/plain: the content type is text/plain, not multipart"),
        (
            "TEXT/Plain ; charset=utf-8",
            "not multipart, content-type TEXT/Plain ; charset=utf-8: the content type is text/plain, not multipart",
        ),
        (
            "multipart/form-data",
            "not multipart, content-type multipart/form-data: the content type is multipart without a boundary",
        ),
        (
            "multipart/form-data; boundary=",
            "not multipart, content-type multipart/form-data; boundary=: the content type is multipart without a boundary",
        ),
    ];
    for (content_type, note) in cases {
        let (status, _, writes) =
            post(profiling::mount_profile, "/api/v2/profile", content_type, b"not multipart").await;
        assert_eq!(status, 202);
        let raws = rows(&writes, "raw_payloads");
        assert_eq!(raws.len(), 1, "{content_type}");
        assert_eq!((raws[0]["intake"].as_str(), raws[0]["reason"].as_str()), (Some("profiling"), Some("decode_error")));
        assert_eq!(raws[0]["note"], note);
        assert_eq!(raws[0]["body"], "not multipart");
    }
    // A boundary the body never reaches: the body breaks off.
    let (_, _, writes) =
        post(profiling::mount_profile, "/api/v2/profile", "multipart/form-data; boundary=x", b"not multipart").await;
    let note = rows(&writes, "raw_payloads")[0]["note"].as_str().unwrap().to_string();
    assert!(note.starts_with("not multipart, content-type multipart/form-data; boundary=x: "), "{note}");
}

#[tokio::test]
async fn a_quoted_boundary_and_a_boundary_in_other_letter_case_are_understood() {
    let (_, body) = multipart(&[("event", br#"{"family":"go"}"#)]);
    let content_type = format!("Multipart/Form-Data; Boundary=\"{BOUNDARY}\"");
    let (_, _, writes) = post(profiling::mount_profile, "/api/v2/profile", &content_type, &body).await;

    assert_eq!(rows(&writes, "profiles")[0]["family"], "go");
}

type Mount = fn(&mut Routes);

/// Half a submission is not stored as if it were a whole one.
#[tokio::test]
async fn a_multipart_body_that_breaks_off_is_kept_raw() {
    let cases: [(Mount, &str, &str, &str); 3] = [
        (profiling::mount_profile, "/api/v2/profile", "profiling", "not multipart, content-type "),
        (profiling::mount_debugger, "/api/v2/debugger", "debugger", "multipart, content-type "),
        (profiling::mount_sourcemap, "/api/v2/srcmap", "srcmap", "multipart, content-type "),
    ];
    let cpu = pprof_bytes();
    let (content_type, body) = multipart(&[("event", br#"{"family":"go"}"#), ("cpu.pprof", &cpu)]);
    let cut = &body[..body.len() - 39];
    for (mount, path, intake, prefix) in cases {
        let (status, _, writes) = post(mount, path, &content_type, cut).await;
        assert_eq!(status, 202);
        assert_eq!(writes.len(), 1, "{path}");
        let raw = &writes[0].rows[0];
        assert_eq!((raw["intake"].as_str(), raw["reason"].as_str()), (Some(intake), Some("decode_error")));
        assert!(raw["note"].as_str().unwrap().starts_with(&format!("{prefix}{content_type}: ")), "{}", raw["note"]);
        assert_eq!(raw["body_bytes"], cut.len());
    }
}

#[tokio::test]
async fn a_symbol_upload_that_is_not_multipart_is_kept_raw() {
    let (status, _, writes) =
        post(profiling::mount_sourcemap, "/api/v2/srcmap", "application/octet-stream", b"\x7fELF").await;

    assert_eq!(status, 202);
    let raws = rows(&writes, "raw_payloads");
    assert_eq!(raws[0]["intake"], "srcmap");
    assert_eq!(
        raws[0]["note"],
        "multipart, content-type application/octet-stream: the content type is application/octet-stream, not multipart"
    );
}

#[tokio::test]
async fn a_symbol_upload_whose_event_is_not_json_still_stores_the_file() {
    let elf = fake_elf(2, 2);
    let (content_type, body) = multipart(&[("event", b"not json"), ("elf_symbol_file", &elf)]);
    let (_, _, writes) = post(profiling::mount_sourcemap, "/api/v2/srcmap", &content_type, &body).await;

    let rows = rows(&writes, "symbol_uploads");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["meta"], "not json");
    assert_eq!(rows[0]["type"], "");
    assert_eq!(rows[0]["elf_endianness"], "BE");
}
