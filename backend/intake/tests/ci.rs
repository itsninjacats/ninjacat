//! Tests of routers/ci_visibility.rs, ci_webhook.rs and install.rs beyond the golden fixtures: the
//! decoders' edge cases, and the columns that keep a JSON value as the text it arrived in.

mod programs;

use std::collections::BTreeMap;
use std::io::Write;
use std::sync::Arc;

use base64::Engine;
use http_body_util::BodyExt;
use ninjapaws_intake::json;
use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::routers::ci_visibility::{
    AgentHeaders, Value, coverage_entries_msgpack, decode_any, read_config_request, test_event_row,
};
use ninjapaws_intake::server::Intake;
use ninjapaws_storage::{Sink, Write as Batch};
use serde_json::{Value as Json, json};
use tako::router::Router;

const KEY: &str = "00000000000000000000000000000001";
const BOUNDARY: &str = "frontier";

struct Answer {
    status: u16,
    headers: http::HeaderMap,
    body: Vec<u8>,
    writes: Vec<Batch>,
}

impl Answer {
    fn rows(&self, table: &str) -> Vec<&Json> {
        self.writes.iter().filter(|w| w.table == table).flat_map(|w| &w.rows).collect()
    }

    /// The one row written to `table`.
    fn row(&self, table: &str) -> &Json {
        let rows = self.rows(table);
        assert_eq!(rows.len(), 1, "{table}: {rows:?}");
        rows[0]
    }

    fn text(&self) -> String {
        String::from_utf8(self.body.clone()).unwrap()
    }
}

async fn serve(mount: fn(&mut Router), method: &str, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Answer {
    let store = Store::default();
    store.publish([(keys::hash(KEY), Key { id: "test".into(), name: "test".into(), tenant_id: "test".into() })]);
    let (sink, capture) = Sink::capture();
    let intake = Intake {
        sink,
        keys: Arc::new(store),
        ack_unknown: false,
        max_body_bytes: 64 * 1024 * 1024,
        stopping: Default::default(),
        tasks: Default::default(),
    };
    let router = programs::router_of(mount, intake);
    let mut request = http::Request::builder().method(method).uri(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = router.dispatch(request.body(body.to_vec().into()).unwrap()).await;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    Answer { status, headers, body, writes: capture.writes() }
}

/// A POST with the test key to a host that asks for the agent's key.
async fn post(mount: fn(&mut Router), url: &str, headers: &[(&str, &str)], body: &[u8]) -> Answer {
    let mut with_key = vec![("dd-api-key", KEY)];
    with_key.extend_from_slice(headers);
    serve(mount, "POST", url, &with_key, body).await
}

async fn post_json(mount: fn(&mut Router), url: &str, body: &str) -> Answer {
    post(mount, url, &[("content-type", "application/json")], body.as_bytes()).await
}

/// A multipart body of (name, content type, data).
fn multipart(parts: &[(&str, &str, &[u8])]) -> (String, Vec<u8>) {
    let mut body = Vec::new();
    for (name, content_type, data) in parts {
        write!(body, "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"").unwrap();
        if *name == "packfile" {
            write!(body, "; filename=\"objects/pack-1.pack\"").unwrap();
        }
        if !content_type.is_empty() {
            write!(body, "\r\nContent-Type: {content_type}").unwrap();
        }
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    write!(body, "--{BOUNDARY}--\r\n").unwrap();
    (format!("multipart/form-data; boundary={BOUNDARY}"), body)
}

async fn coverage(parts: &[(&str, &str, &[u8])]) -> Answer {
    let (content_type, body) = multipart(parts);
    post(programs::ci_test_coverage, "/api/v2/citestcov", &[("content-type", &content_type)], &body).await
}

fn value(text: &str) -> Value {
    match decode_any("application/json", text.as_bytes()) {
        (_, Ok(value)) => value,
        (_, Err(e)) => panic!("{e}"),
    }
}

fn number(literal: &str) -> Value {
    Value::Number(literal.into())
}

fn string(text: &str) -> Value {
    Value::String(text.into())
}

fn texts(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

/// A column that holds bytes, as a capture spells it: its text when it is UTF-8.
fn blob(column: &Json) -> Vec<u8> {
    match column {
        Json::String(text) => text.as_bytes().to_vec(),
        other => base64::engine::general_purpose::STANDARD.decode(other["$b64"].as_str().unwrap()).unwrap(),
    }
}

// ---- the decoded value ----

#[test]
fn a_float_where_text_was_expected_is_written_in_the_layout_the_columns_hold() {
    let cases = [
        (1234567.0, "1.234567e+06"),
        (100000.0, "100000"),
        (1e6, "1e+06"),
        (1e20, "1e+20"),
        (1e21, "1e+21"),
        (123456789012345678.0, "1.2345678901234568e+17"),
        (0.0001, "0.0001"),
        (0.00001, "1e-05"),
        (0.000123, "0.000123"),
        (1.5, "1.5"),
        (120.0, "120"),
        (-0.0, "-0"),
        (f64::NAN, "NaN"),
        (f64::INFINITY, "+Inf"),
        (f64::NEG_INFINITY, "-Inf"),
        (1e-7, "1e-07"),
        (5e-324, "5e-324"),
        (1.7976931348623157e308, "1.7976931348623157e+308"),
        (12345.678, "12345.678"),
        (-12345.678, "-12345.678"),
        (1e100, "1e+100"),
        (1.5e-10, "1.5e-10"),
    ];
    for (number, expected) in cases {
        assert_eq!(Value::Float(number).text(), expected, "{number}");
    }
    let singles = [
        (0.1f32, "0.1"),
        (1234567.0, "1.234567e+06"),
        (1e6, "1e+06"),
        (16777216.0, "1.6777216e+07"),
        (3.4e38, "3.4e+38"),
    ];
    for (number, expected) in singles.into_iter().chain([(1e-5, "1e-05")]) {
        assert_eq!(Value::Float32(number).text(), expected, "{number}");
    }
}

#[test]
fn text_renders_every_wire_type_exactly() {
    assert_eq!(Value::Null.text(), "");
    assert_eq!(string("s").text(), "s");
    assert_eq!(Value::Bytes(b"raw".to_vec()).text(), "raw");
    assert_eq!(number("1.50").text(), "1.50");
    assert_eq!(Value::Bool(true).text(), "true");
    assert_eq!(Value::Bool(false).text(), "false");
    assert_eq!(Value::Int(-5).text(), "-5");
    assert_eq!(Value::UInt(u64::MAX).text(), "18446744073709551615");
    assert_eq!(value(r#"[1, "a"]"#).text(), r#"[1,"a"]"#);
    assert_eq!(value(r#"{"a": {"b": null}}"#).text(), r#"{"a":{"b":null}}"#);
}

#[test]
fn an_id_is_never_read_through_a_float() {
    assert_eq!(Value::UInt(u64::MAX).to_u64(), u64::MAX);
    assert_eq!(Value::Int(7).to_u64(), 7);
    assert_eq!(Value::Int(-1).to_u64(), 0);
    // 2^53 + 1: the first integer a float cannot hold.
    assert_eq!(number("9007199254740993").to_u64(), 9007199254740993);
    for zero in [number("1.0"), number("1e3"), number("-1"), string("+42"), string(" 42")] {
        assert_eq!(zero.to_u64(), 0, "{zero:?}");
    }
    assert_eq!(string("42").to_u64(), 42);
    assert_eq!(Value::Float(120.0).to_u64(), 120);
    for zero in [120.5, -1.0, 9007199254740992.0, f64::NAN] {
        assert_eq!(Value::Float(zero).to_u64(), 0, "{zero}");
    }
    assert_eq!(Value::Float32(5.0).to_u64(), 0);
    assert_eq!(Value::Bool(true).to_u64(), 0);
    assert_eq!(Value::Null.to_u64(), 0);
}

#[test]
fn a_signed_integer_follows_the_same_rules() {
    assert_eq!(Value::Int(-7).to_i64(), -7);
    assert_eq!(Value::UInt(i64::MAX as u64).to_i64(), i64::MAX);
    assert_eq!(Value::UInt(u64::MAX).to_i64(), 0);
    assert_eq!(number("1654698415668011500").to_i64(), 1654698415668011500);
    assert_eq!(number("1e3").to_i64(), 0);
    assert_eq!(number("9223372036854775808").to_i64(), 0);
    assert_eq!(string("-5").to_i64(), -5);
    assert_eq!(string("+5").to_i64(), 5);
    assert_eq!(string("5 ").to_i64(), 0);
    assert_eq!(Value::Float(-120.0).to_i64(), -120);
    assert_eq!(Value::Float(0.5).to_i64(), 0);
    assert_eq!(Value::Float(-9007199254740992.0).to_i64(), 0);
    assert_eq!(Value::Bytes(vec![1]).to_i64(), 0);
    assert_eq!(Value::Null.try_i64(), None);
    assert_eq!(number("0").try_i64(), Some(0));
    assert_eq!(string("not a number").try_i64(), Some(0));
}

#[test]
fn a_metric_is_read_from_every_numeric_form() {
    assert_eq!(Value::Float(1.5).to_float(), 1.5);
    assert_eq!(Value::Float32(1.5).to_float(), 1.5);
    assert_eq!(Value::Int(-3).to_float(), -3.0);
    assert_eq!(Value::UInt(3).to_float(), 3.0);
    assert_eq!(number("42.5").to_float(), 42.5);
    assert_eq!(number("1e999").to_float(), f64::INFINITY);
    assert_eq!(string("3").to_float(), 3.0);
    assert_eq!(string(" 3").to_float(), 0.0);
    assert_eq!(string("abc").to_float(), 0.0);
    assert_eq!(Value::Bool(true).to_float(), 0.0);
    // Infinity and NaN in words.
    assert_eq!(string("inf").to_float(), f64::INFINITY);
    assert_eq!(string("+Inf").to_float(), f64::INFINITY);
    assert_eq!(string("-Infinity").to_float(), f64::NEG_INFINITY);
    assert!(string("NaN").to_float().is_nan());
}

#[test]
fn a_bare_string_counts_as_a_list_of_one() {
    assert_eq!(value(r#"["linux", 7]"#).strings(), vec!["linux", "7"]);
    assert_eq!(string("linux").strings(), vec!["linux"]);
    assert!(string("").strings().is_empty());
    assert!(value("[]").strings().is_empty());
    assert!(number("7").strings().is_empty());
}

#[test]
fn maps_render_each_value_and_ignore_what_is_not_an_object() {
    let object = value(r#"{"a": "x", "b": 7, "c": null, "d": {"e": 1.50}}"#);
    assert_eq!(object.string_map(), texts(&[("a", "x"), ("b", "7"), ("c", ""), ("d", r#"{"e":1.50}"#)]));
    let floats: BTreeMap<String, f64> =
        [("a", 0.0), ("b", 7.0), ("c", 0.0), ("d", 0.0)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
    assert_eq!(object.float_map(), floats);
    assert!(value("[1]").string_map().is_empty());
    assert!(Value::Null.float_map().is_empty());
    assert!(object.has("c"));
    assert!(!object.has("z"));
    assert_eq!(*string("not an object").field("a"), Value::Null);
}

#[test]
fn to_json_keeps_every_digit_and_says_nothing_for_null() {
    assert_eq!(Value::Null.to_json(), "");
    let msgpack = Value::Object(
        [
            ("id", Value::UInt(u64::MAX)),
            ("n", Value::Int(-1)),
            ("bitmap", Value::Bytes(vec![1, 2, 3])),
            ("half", Value::Float(0.5)),
            ("none", Value::Null),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect(),
    );
    assert_eq!(msgpack.to_json(), r#"{"bitmap":"AQID","half":0.5,"id":18446744073709551615,"n":-1,"none":null}"#);
    assert_eq!(
        value(r#"{"id": 9007199254740993, "price": 1.50}"#).to_json(),
        r#"{"id":9007199254740993,"price":1.50}"#
    );
    assert_eq!(Value::Ext(5, vec![1]).to_json(), r#"{"Data":"AQ==","Type":5}"#);
}

#[test]
fn a_value_json_cannot_express_gives_no_text_at_all() {
    let bad =
        Value::Object([("ok".to_string(), Value::Float(1.0)), ("bad".to_string(), Value::Float(f64::NAN))].into());
    assert_eq!(bad.to_json(), "");
}

#[test]
fn unknown_keeps_the_keys_nothing_names_as_json() {
    let found = value(r#"{"known": 1, "new": {"nested": true}, "text": "t", "nothing": null}"#).unknown(&["known"]);
    assert_eq!(found, texts(&[("new", r#"{"nested":true}"#), ("text", "\"t\""), ("nothing", "")]));
    assert!(string("x").unknown(&[]).is_empty());
}

#[test]
fn a_repeated_key_keeps_its_last_value_in_both_formats() {
    assert_eq!(value(r#"{"a": 1, "a": 2}"#).field_text("a"), "2");
    // {"a": 1, "a": 2}
    let body = [0x82, 0xa1, b'a', 0x01, 0xa1, b'a', 0x02];
    match decode_any("application/msgpack", &body) {
        ("msgpack", Ok(value)) => assert_eq!(value.field_text("a"), "2"),
        other => panic!("decoded {other:?}"),
    }
}

#[test]
fn content_type_picks_the_decoder_and_the_first_byte_when_it_says_nothing() {
    let json_body = br#"{"a":1}"#;
    let msgpack_body = [0x81, 0xa1, b'a', 0x01];
    assert_eq!(decode_any("application/json", json_body).0, "json");
    assert_eq!(decode_any("application/msgpack", &msgpack_body).0, "msgpack");
    assert_eq!(decode_any("", json_body).0, "json");
    assert_eq!(decode_any("text/plain", b"  \n[1]").0, "json");
    assert_eq!(decode_any("", &msgpack_body).0, "msgpack");
    // A JSON body labelled msgpack is read as msgpack: '{' is the integer 123.
    assert_eq!(decode_any("application/msgpack", json_body), ("msgpack", Ok(Value::UInt(123))));
}

#[test]
fn what_follows_the_first_json_value_is_not_looked_at() {
    assert_eq!(value(r#"{"a":1} trailing"#).field_text("a"), "1");
    assert!(decode_any("application/json", b"  ").1.is_err());
    assert!(decode_any("application/json", b"[1,2").1.is_err());
}

#[test]
fn json_holding_half_a_surrogate_pair_or_bytes_that_are_not_utf8_still_decodes() {
    // Half a surrogate pair written as an escape, and a byte that is not UTF-8: both are read as
    // U+FFFD.
    let body = [
        &br#"{"half":"x\ud83dy","low":"\udc00","pair":"\ud83d\ude00","slash":"\\ud83d","k\ud800":1,"byte":"z"#[..],
        &[0xff],
        b"\"}",
    ]
    .concat();
    let value = decode_any("application/json", &body).1.unwrap();
    assert_eq!(value.field_text("half"), "x\u{fffd}y");
    assert_eq!(value.field_text("low"), "\u{fffd}");
    assert_eq!(value.field_text("pair"), "\u{1f600}");
    assert_eq!(value.field_text("slash"), "\\ud83d");
    assert_eq!(value.field_text("k\u{fffd}"), "1");
    assert_eq!(value.field_text("byte"), "z\u{fffd}");

    // Nested deeper than .NET's default limit of 64.
    let deep = format!("{{\"deep\":{}{}}}", "[".repeat(80), "]".repeat(80));
    assert!(decode_any("application/json", deep.as_bytes()).1.is_ok());
}

/// `around` with its `{}` replaced by lists nested until the whole body is `depth` levels deep,
/// and four megabytes of text at the bottom.
fn deep(depth: usize, around: &str) -> String {
    let (before, after) = around.split_once("{}").unwrap();
    let levels = depth - before.matches(['{', '[']).count();
    format!("{before}{}\"{}\"{}{after}", "[".repeat(levels), "x".repeat(4 << 20), "]".repeat(levels))
}

/// Bodies as deep as the parser goes, each a few megabytes: a decoder reads them a fixed number
/// of times, not once per level, so a sender cannot buy seconds of CPU with nesting.
#[tokio::test]
async fn a_deeply_nested_body_is_decoded_in_linear_time() {
    // serde_json takes 127 levels and refuses the 128th.
    const DEEPEST: usize = 127;
    let cycle = r#"{"events":[{"type":"test","content":{"meta":{"x":{}}}}]}"#;
    assert!(json::try_parse(deep(DEEPEST + 1, cycle).as_bytes()).is_err());

    let started = std::time::Instant::now();
    let answer = post_json(programs::ci_test_cycle, "/api/v2/citestcycle", &deep(DEEPEST, cycle)).await;
    assert_eq!(answer.rows("ci_test_events").len(), 1);
    let answer = webhook("/api/v2/webhook", &[], &deep(DEEPEST, r#"[{"level":"job","parameters":{"x":{}}}]"#)).await;
    assert_eq!(answer.rows("ci_webhook_events").len(), 1);
    let settings = deep(DEEPEST, r#"{"data":{"attributes":{"configurations":{}}}}"#);
    let answer = post_json(programs::api, "/api/v2/libraries/tests/services/setting", &settings).await;
    assert_eq!(answer.rows("ci_settings_requests").len(), 1);
    let tags = deep(DEEPEST, r#"{"data":{"type":"t","attributes":{"tags":{}}}}"#);
    let answer = post_json(programs::api, "/api/v2/ci/pipeline/tags", &tags).await;
    assert_eq!(answer.rows("ci_pipeline_events").len(), 1);
    // Read once per level, each 4 MB body would be over a hundred passes.
    assert!(started.elapsed() < std::time::Duration::from_secs(10), "{:?}", started.elapsed());
}

// ---- citestcov ----

#[test]
fn a_coverages_array_header_longer_than_the_document_does_not_decode() {
    // {"version": 2, "coverages": <array32 announcing 2^32-2 elements>}
    let forged = [&[0x82, 0xa7][..], b"version", &[0x02, 0xa9], b"coverages", &[0xdd, 0xff, 0xff, 0xff, 0xfe]].concat();
    assert!(coverage_entries_msgpack(&forged).is_err());
}

#[test]
fn each_coverage_entry_keeps_its_own_bytes() {
    let first = [&[0x81, 0xa7][..], b"span_id", &[0x07]].concat();
    let second = [&[0x81, 0xad][..], b"test_suite_id", &[0x09]].concat();
    // {"extra": [1, 2], "version": "2", "coverages": [first, second]}
    let body = [
        &[0x83, 0xa5][..],
        b"extra",
        &[0x92, 0x01, 0x02, 0xa7],
        b"version",
        &[0xa1, b'2', 0xa9],
        b"coverages",
        &[0x92],
        &first,
        &second,
    ]
    .concat();
    let (version, entries) = coverage_entries_msgpack(&body).unwrap();
    // An envelope key nobody knows is skipped, and a version that changed type still reads.
    assert_eq!(version, 2);
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].raw, first);
    assert_eq!(entries[1].raw, second);
    assert_eq!(entries[0].value.field("span_id").to_u64(), 7);
}

#[test]
fn a_coverage_envelope_whose_key_is_not_a_string_does_not_decode() {
    // {1: 2}
    let error = coverage_entries_msgpack(&[0x81, 0x01, 0x02]).unwrap_err();
    assert!(error.starts_with("envelope key 0: "), "{error}");
}

#[tokio::test]
async fn a_truncated_coverage_upload_is_kept_raw_not_stored_in_part() {
    let (content_type, body) = multipart(&[
        ("event", "application/json", br#"{"dummy": true}"#),
        ("coveragex", "application/json", br#"{"version":2,"coverages":[{"test_suite_id":9}]}"#),
    ]);
    // Cut inside the closing boundary: both parts are whole, the body is not.
    let cut = &body[..body.len() - 9];
    let answer = post(programs::ci_test_coverage, "/api/v2/citestcov", &[("content-type", &content_type)], cut).await;
    assert_eq!(answer.status, 202);
    assert!(answer.rows("ci_coverage").is_empty());
    let raw = answer.row("raw_payloads");
    assert_eq!((raw["intake"].as_str(), raw["reason"].as_str()), (Some("citestcov"), Some("decode_error")));
    let note = raw["note"].as_str().unwrap();
    assert!(note.starts_with("multipart, content-type multipart/form-data; boundary=frontier: "), "{note}");
}

#[tokio::test]
async fn a_json_coverage_payload_of_the_wrong_shape_is_kept_raw() {
    for payload in ["[1,2]", r#"{"version":2,"coverages":{"not":"an array"}}"#] {
        let answer = coverage(&[("coverage1", "application/json", payload.as_bytes())]).await;
        assert_eq!(answer.status, 202);
        assert!(answer.rows("ci_coverage").is_empty(), "{payload}");
        assert_eq!(answer.row("raw_payloads")["reason"], "decode_error");
    }
}

#[tokio::test]
async fn a_json_coverage_version_that_cannot_be_read_is_0_and_costs_no_coverage() {
    for (version, expected) in [("2", 2), ("\"2\"", 2), ("2.0", 0), ("99999999999999999999", 0), ("\"two\"", 0)] {
        let payload = format!(r#"{{"version":{version},"coverages":[{{}}]}}"#);
        let answer = coverage(&[("c", "application/json", payload.as_bytes())]).await;
        assert!(answer.rows("raw_payloads").is_empty(), "{version}");
        assert_eq!(answer.row("ci_coverage")["payload_version"], expected, "{version}");
    }
}

#[tokio::test]
async fn a_json_coverage_entry_keeps_the_base64_text_of_its_bitmap_and_its_unknown_keys() {
    let payload = br#"{"version":2,"coverages":[{"test_session_id":1,"span_id":0,"files":[{"filename":"a.go","bitmap":"AQID"}],"later":[1]}]}"#;
    let answer = coverage(&[("coverage1", "", payload)]).await;
    let row = answer.row("ci_coverage");
    assert_eq!(row["payload_version"], 2);
    assert_eq!(row["raw_format"], "json");
    // Zero is a legal span id, and it was sent.
    assert_eq!(row["span_id"], 0);
    assert_eq!(row["files_bitmap"], json!(["AQID"]));
    assert_eq!(row["extra"], json!({ "later": "[1]" }));
    assert_eq!(row["event"], "");
}

#[tokio::test]
async fn a_json_coverage_entry_is_stored_as_the_text_it_arrived_in() {
    let entry = r#"{ "test_suite_id" : 9,  "files": [ {"filename": "a.go", "weight": 1.50} ] }"#;
    let payload = format!(r#"{{"version":2,"coverages":[ {entry} , {{"span_id":1}}]}}"#);
    let answer =
        coverage(&[("event", "application/json", b"{}"), ("coverage1", "application/json", payload.as_bytes())]).await;
    let rows = answer.rows("ci_coverage");
    assert_eq!(rows.len(), 2);
    assert_eq!(blob(&rows[0]["raw"]), entry.as_bytes());
    assert_eq!(blob(&rows[1]["raw"]), br#"{"span_id":1}"#);
    assert_eq!(rows[0]["suite_id"], 9);
    assert_eq!(rows[0]["event"], "{}");
}

// ---- citestcycle ----

#[tokio::test]
async fn an_empty_test_cycle_body_is_answered_and_nothing_is_stored() {
    let answer =
        post(programs::ci_test_cycle, "/api/v2/citestcycle", &[("content-type", "application/msgpack")], b"").await;
    assert_eq!(answer.status, 202);
    assert!(answer.writes.is_empty());
}

#[tokio::test]
async fn events_that_are_not_maps_keep_the_body_raw_once_the_others_become_rows() {
    let body = r#"{"version":3,"events":[5,{"type":"test","version":2,"content":{"service":"s","meta":{"test.is_new":" YES "}}},"x"]}"#;
    let answer = post_json(programs::ci_test_cycle, "/api/v2/citestcycle", body).await;
    assert_eq!(answer.status, 202);
    let writers: Vec<&str> = answer.writes.iter().map(|w| w.writer).collect();
    assert_eq!(writers, vec!["storage_raw_payloads", "storage_ci_test_events"]);
    assert_eq!(answer.row("raw_payloads")["note"], "2 of 3 events are not maps: 0, 2");
    let row = answer.row("ci_test_events");
    assert_eq!(row["payload_version"], 3);
    assert_eq!(row["event_version"], 2);
    assert_eq!(row["service"], "s");
    assert_eq!(row["test_is_new"], 1);
    assert_eq!(row["test_is_retry"], Json::Null);
}

#[tokio::test]
async fn an_events_content_keeps_each_number_as_it_was_written() {
    let body =
        r#"{"events":[{"type":"test","content":{ "span_id" : 12345678901234567890, "metrics": {"cpu": 1.50} }}]}"#;
    let answer = post_json(programs::ci_test_cycle, "/api/v2/citestcycle", body).await;
    let row = answer.row("ci_test_events");
    assert_eq!(row["span_id"], 12345678901234567890u64);
    // Re-written with sorted keys and no spaces; the literals stay.
    assert_eq!(row["content"], r#"{"metrics":{"cpu":1.50},"span_id":12345678901234567890}"#);
}

#[test]
fn a_source_line_is_taken_from_metrics_first_then_from_meta() {
    let row = |content: &str| {
        let event = value(&format!(r#"{{"type":"test","content":{content}}}"#));
        test_event_row("t", chrono::DateTime::UNIX_EPOCH, &AgentHeaders::default(), 1, &BTreeMap::new(), &event)
    };
    let both = row(r#"{"meta":{"test.source.start":"7"},"metrics":{"test.source.start":120.9}}"#);
    assert_eq!((both.test_source_start, both.test_source_end), (Some(120), None));
    let meta_only = row(r#"{"meta":{"test.source.start":" 7 ","test.source.end":"seven"}}"#);
    assert_eq!((meta_only.test_source_start, meta_only.test_source_end), (Some(7), None));
}

#[tokio::test]
async fn the_agents_own_headers_win_over_the_fallbacks() {
    let body = br#"{"events":[{"type":"test","content":{}}]}"#;
    let answer = post(
        programs::ci_test_cycle,
        "/api/v2/citestcycle",
        &[
            ("content-type", "application/json"),
            ("x-datadog-container-id", "invented"),
            ("x-datadog-agentversion", "7.90.0"),
            ("via", "trace-agent 7.83.0"),
        ],
        body,
    )
    .await;
    let row = answer.row("ci_test_events");
    assert_eq!(row["container_id"], "invented");
    assert_eq!(row["agent_version"], "7.90.0");

    let via_other = post(
        programs::ci_test_cycle,
        "/api/v2/citestcycle",
        &[("content-type", "application/json"), ("via", "1.1 varnish")],
        body,
    )
    .await;
    assert_eq!(via_other.row("ci_test_events")["agent_version"], "");
}

// ---- what a tracer asks api.<site> ----

#[test]
fn a_question_keeps_what_decoded_when_a_field_has_the_wrong_type() {
    let (request, problem) = read_config_request(br#"{"data":{"id":5,"type":"x","attributes":{"service":"s"}}}"#);
    assert_eq!((request.id.as_str(), request.r#type.as_str()), ("", "x"));
    // The attributes are not read under a broken envelope.
    assert_eq!(request.service, "");
    assert_eq!(problem.map(|p| p.0), Some("envelope"));

    let (request, problem) = read_config_request(
        br#"{"data":{"id":"a","attributes":{"service":5,"env":"ci","configurations":{"a": 1},"page_info":{"page_state":"p"}}}}"#,
    );
    assert_eq!(request.id, "a");
    assert_eq!(request.service, "");
    assert_eq!(request.env, "ci");
    assert_eq!(request.configurations, r#"{"a": 1}"#);
    assert_eq!(request.page_state, "p");
    assert_eq!(problem.map(|p| p.0), Some("attributes"));
}

#[test]
fn a_questions_members_are_read_by_exact_name_and_null_is_the_same_as_absent() {
    let (request, problem) = read_config_request(
        br#"{"DATA":{"id":"upper"},"data":{"id":"exact","Attributes":{"service":"x"},"attributes":{"service":"s"}}}"#,
    );
    assert_eq!((request.id.as_str(), request.service.as_str()), ("exact", "s"));
    assert!(problem.is_none());

    // null is no document, and no error.
    let (request, problem) = read_config_request(b"null");
    assert_eq!(request.id, "");
    assert!(problem.is_none());

    let (request, problem) =
        read_config_request(br#"{"data":{"id":null,"attributes":{"configurations":null,"page_info":null}}}"#);
    assert_eq!(request.id, "");
    // Kept as JSON text, so the null that was sent stays.
    assert_eq!(request.configurations, "null");
    assert!(problem.is_none());

    for body in ["[1]", "\"text\"", r#"{"data":[1,2]}"#, r#"{"data":{"id":"x"}} trailing"#] {
        let (request, problem) = read_config_request(body.as_bytes());
        assert_eq!(request.id, "", "{body}");
        assert_eq!(problem.map(|p| p.0), Some("envelope"), "{body}");
    }

    let (_, problem) = read_config_request(br#"{"data":{"attributes":[1]}}"#);
    assert_eq!(problem, Some(("attributes", "data.attributes: expected an object, got array".to_string())));
}

#[test]
fn of_a_repeated_key_the_last_one_counts() {
    let (request, problem) = read_config_request(br#"{"data":{"id":"a","id":5}}"#);
    assert_eq!(request.id, "");
    assert_eq!(problem, Some(("envelope", "data.id: expected a string, got number".to_string())));

    let (request, problem) = read_config_request(br#"{"data":{"id":"a","id":null}}"#);
    assert_eq!(request.id, "");
    assert!(problem.is_none());

    let (request, problem) = read_config_request(br#"{"data":{"id":"a"},"data":{"type":"t"}}"#);
    assert_eq!((request.id.as_str(), request.r#type.as_str()), ("", "t"));
    assert!(problem.is_none());

    let (request, _) = read_config_request(
        br#"{"data":{"attributes":{"configurations":{"a":1}}},"data":{"attributes":{"configurations":{"b":2}}}}"#,
    );
    assert_eq!(request.configurations, r#"{"b":2}"#);
}

#[tokio::test]
async fn the_configurations_are_stored_as_the_text_they_arrived_in() {
    let configurations = r#"{ "os.platform" : "linux",  "weight": 1.50 }"#;
    let body = format!(r#"{{"data":{{"id":"a","attributes":{{"service":"s","configurations":{configurations}}}}}}}"#);
    let answer = post_json(programs::api, "/api/v2/libraries/tests/services/setting", &body).await;
    let row = answer.row("ci_settings_requests");
    assert_eq!(row["configurations"], configurations);
    assert_eq!(row["request_body"], body.as_str());
}

#[tokio::test]
async fn an_id_that_needs_escaping_is_echoed_as_valid_json() {
    let answer = post_json(programs::api, "/api/v2/ci/libraries/tests", r#"{"data":{"id":"a\"b\\c"}}"#).await;
    let answer_json: Json = serde_json::from_slice(&answer.body).unwrap();
    assert_eq!(answer_json["data"]["id"], "a\"b\\c");
    assert_eq!(answer.row("ci_settings_requests")["response_body"], answer.text());
}

#[tokio::test]
async fn a_commit_search_that_does_not_decode_still_gets_an_empty_json_answer() {
    for body in [r#"{"data":"#, r#"{"data":{"id":"a"}}"#, r#"{"data":[5]}"#, r#"{"data":[{"id":7}]}"#, r#"{"meta":[]}"#]
    {
        let answer = post_json(programs::api, "/api/v2/git/repository/search_commits", body).await;
        assert_eq!(answer.status, 200);
        assert_eq!(answer.headers["content-type"], "application/json; charset=utf-8");
        assert_eq!(answer.text(), r#"{"data":[],"meta":{"repository_url":""}}"#);
        assert!(answer.rows("git_commits").is_empty());
        assert_eq!(answer.row("raw_payloads")["intake"], "gitmeta", "{body}");
    }
}

#[tokio::test]
async fn a_commit_without_an_id_is_not_stored() {
    let body = r#"{"data":[{"id":"aaa"},null,{"type":"commit"},{"id":""}],"meta":{"repository_url":"r"}}"#;
    let answer = post_json(programs::api, "/api/v2/git/repository/search_commits", body).await;
    assert_eq!(answer.status, 200);
    assert_eq!(answer.row("git_commits")["sha"], "aaa");
    assert!(answer.rows("raw_payloads").is_empty());
}

#[tokio::test]
async fn a_packfile_upload_that_is_not_multipart_is_kept_raw_and_still_answered_204() {
    let answer = post(
        programs::api,
        "/api/v2/git/repository/packfile",
        &[("content-type", "application/octet-stream")],
        b"PACK",
    )
    .await;
    assert_eq!(answer.status, 204);
    assert!(answer.body.is_empty());
    let raw = answer.row("raw_payloads");
    assert_eq!(raw["intake"], "gitmeta");
    assert_eq!(
        raw["note"],
        "packfile multipart, content-type application/octet-stream: the content type is application/octet-stream, not multipart"
    );
    assert!(answer.rows("git_packfiles").is_empty());
}

async fn packfile(parts: &[(&str, &str, &[u8])]) -> Answer {
    let (content_type, body) = multipart(parts);
    post(programs::api, "/api/v2/git/repository/packfile", &[("content-type", &content_type)], &body).await
}

#[tokio::test]
async fn a_packfile_upload_without_a_packfile_part_is_kept_raw_and_stores_no_pack() {
    let answer = packfile(&[("pushedSha", "application/json", br#"{"data":{"id":"abc"}}"#)]).await;
    assert_eq!(answer.status, 204);
    let raw = answer.row("raw_payloads");
    assert_eq!(
        (raw["reason"].as_str(), raw["note"].as_str()),
        (Some("unexpected_shape"), Some("packfile upload without a packfile part"))
    );
    assert!(answer.rows("git_packfiles").is_empty());
    assert!(answer.rows("git_commits").is_empty());
}

#[tokio::test]
async fn a_pushed_sha_part_that_does_not_decode_is_kept_beside_the_pack() {
    let pushed_sha = br#"{"data":{"id":"abc"},"meta":5}"#;
    let answer = packfile(&[
        ("pushedSha", "application/json", pushed_sha),
        ("packfile", "application/octet-stream", &[0xff, 0x00]),
        ("note", "", b"hello"),
    ])
    .await;
    assert_eq!(answer.status, 204);
    let pack = answer.row("git_packfiles");
    // What did decode is kept: the sha was readable, the repository was not.
    assert_eq!(pack["pushed_sha"], "abc");
    assert_eq!(pack["repository_url"], "");
    assert_eq!(pack["filename"], "objects/pack-1.pack");
    assert_eq!(blob(&pack["packfile"]), vec![0xff, 0x00]);
    assert_eq!(
        pack["other_parts"],
        json!({ "pushedSha.undecodable": std::str::from_utf8(pushed_sha).unwrap(), "note": "hello" })
    );
    assert_eq!(answer.row("git_commits")["packfile_id"], pack["packfile_id"]);
}

#[tokio::test]
async fn a_repeated_pushed_sha_part_changes_only_what_it_sets() {
    let answer = packfile(&[
        ("pushedSha", "application/json", br#"{"data":{"id":"one"},"meta":{"repository_url":"r1"}}"#),
        ("pushedSha", "application/json", br#"{"data":{"type":"commit"}}"#),
        ("packfile", "application/octet-stream", b"PACK"),
    ])
    .await;
    let pack = answer.row("git_packfiles");
    assert_eq!(pack["pushed_sha"], "one");
    assert_eq!(pack["repository_url"], "r1");
    assert_eq!(pack["other_parts"], json!({}));
    assert_eq!(answer.row("git_commits")["sha"], "one");
}

// ---- datadog-ci ----

#[tokio::test]
async fn measures_fill_the_metrics_column_only_when_metrics_are_absent() {
    let body = r#"{"data":{"type":"t","attributes":{"metrics":{"a":1},"measures":{"b":2},"ci_level":"2"}}}"#;
    let answer = post_json(programs::api, "/api/intake/ci/custom_spans", body).await;
    let row = answer.row("ci_pipeline_events");
    assert_eq!(row["metrics"], json!({ "a": 1.0 }));
    assert_eq!(row["ci_level"], 2);
    // "measures" is a named key, so it is not repeated in extra.
    assert_eq!(row["extra"], json!({}));
}

#[tokio::test]
async fn a_datadog_ci_call_whose_attributes_are_not_an_object_is_still_a_row() {
    for body in [r#"{"data":{"type":"t","attributes":[1,2]}}"#, "null", r#"{"data":null}"#] {
        let answer = post_json(programs::api, "/api/v2/ci/pipeline/tags", body).await;
        assert_eq!(answer.status, 202);
        let row = answer.row("ci_pipeline_events");
        assert_eq!(row["kind"], "tag");
        assert_eq!(row["body"], body);
        assert_eq!(row["ci_level"], Json::Null);
        assert_eq!(row["tags"], json!({}));
    }
}

#[tokio::test]
async fn a_datadog_ci_envelope_of_the_wrong_shape_is_kept_raw() {
    for body in ["[1]", r#"{"data":5}"#, r#"{"data":{"type":7}}"#] {
        let answer = post_json(programs::api, "/api/v2/ci/pipeline/metrics", body).await;
        assert_eq!(answer.status, 202);
        assert!(answer.rows("ci_pipeline_events").is_empty());
        assert_eq!(answer.row("raw_payloads")["intake"], "cipipeline");
    }
}

#[tokio::test]
async fn a_datadog_ci_calls_attributes_are_stored_as_the_text_they_arrived_in() {
    let attributes = r#"{ "tags" : {"team": "ci"},  "metrics": {"binary.size": 1.50} }"#;
    let body = format!(r#"{{"data":{{"type":"ci_custom_metric","attributes":{attributes}}}}}"#);
    let answer = post_json(programs::api, "/api/v2/ci/pipeline/metrics", &body).await;
    let row = answer.row("ci_pipeline_events");
    assert_eq!(row["attributes"], attributes);
    assert_eq!(row["metrics"], json!({ "binary.size": 1.5 }));
}

// ---- webhook-intake ----

async fn webhook(url: &str, headers: &[(&str, &str)], body: &str) -> Answer {
    let mut with_key = vec![("dd-api-key", KEY), ("content-type", "application/json")];
    with_key.extend_from_slice(headers);
    serve(programs::ci_webhook, "POST", url, &with_key, body.as_bytes()).await
}

#[tokio::test]
async fn a_bare_null_is_an_empty_batch() {
    let answer = webhook("/api/v2/webhook", &[], "null").await;
    assert_eq!(answer.status, 202);
    assert!(answer.writes.is_empty());
}

#[tokio::test]
async fn an_empty_webhook_body_is_answered_and_nothing_is_stored() {
    let answer = webhook("/api/v2/webhook/", &[], "").await;
    assert_eq!(answer.status, 202);
    assert!(answer.writes.is_empty());
}

#[tokio::test]
async fn an_element_that_is_not_an_object_goes_raw_the_others_become_rows() {
    let answer = webhook("/api/v2/webhook", &[], r#"[null, 7, {"level":"job","name":"x"}]"#).await;
    assert_eq!(answer.status, 202);
    let raw = answer.rows("raw_payloads");
    assert_eq!(raw.iter().map(|row| row["body"].as_str().unwrap()).collect::<Vec<_>>(), vec!["null", "7"]);
    for row in raw {
        assert_eq!(row["intake"], "webhook");
        assert_eq!(row["reason"], "unexpected_shape");
        assert_eq!(row["note"], "element is not a JSON object");
    }
    assert_eq!(answer.row("ci_webhook_events")["level"], "job");
    // Raw elements are written as they are met, the rows after the loop.
    assert_eq!(answer.writes.last().unwrap().writer, "storage_ci_webhook_events");
}

#[tokio::test]
async fn a_bare_string_or_number_is_a_batch_of_one_element_that_is_not_an_object() {
    let answer = webhook("/api/v2/webhook", &[], "\"pipeline\"").await;
    assert!(answer.rows("ci_webhook_events").is_empty());
    assert_eq!(answer.row("raw_payloads")["reason"], "unexpected_shape");
}

#[tokio::test]
async fn the_elements_own_provider_wins_over_the_header_the_header_over_the_shape() {
    let answer = webhook(
        "/api/v2/webhook",
        &[("dd-ci-provider-name", "  Jenkins ")],
        r#"[{"provider":"GitLab","object_kind":"build"},{"object_kind":"build"}]"#,
    )
    .await;
    let rows = answer.rows("ci_webhook_events");
    assert_eq!(rows.iter().map(|r| r["provider"].as_str().unwrap()).collect::<Vec<_>>(), vec!["gitlab", "jenkins"]);
    // No "level", so GitLab's object_kind is the level.
    assert_eq!(rows.iter().map(|r| r["level"].as_str().unwrap()).collect::<Vec<_>>(), vec!["build", "build"]);

    let answer = webhook("/api/v2/webhook", &[], r#"[{"object_kind":null},{"provider":""}]"#).await;
    let rows = answer.rows("ci_webhook_events");
    assert_eq!(rows.iter().map(|r| r["provider"].as_str().unwrap()).collect::<Vec<_>>(), vec!["gitlab", "unknown"]);
}

#[tokio::test]
async fn a_numeric_id_keeps_every_digit_and_absent_is_not_false() {
    let body = r#"{"level":"pipeline","trace_id":5678901234567890123,"span_id":1.50,"payload_version":"2",
            "queue_time":1500,"partial_retry":"false","is_manual":false,"unique_id":true}"#;
    let answer = webhook("/api/v2/webhook", &[], body).await;
    let row = answer.row("ci_webhook_events");
    assert_eq!(row["trace_id"], "5678901234567890123");
    assert_eq!(row["span_id"], "1.50");
    assert_eq!(row["unique_id"], "true");
    assert_eq!(row["payload_version"], 2);
    assert_eq!(row["queue_time_ms"], 1500);
    // A string is not a bool: the provider did not say.
    assert_eq!(row["partial_retry"], Json::Null);
    assert_eq!(row["is_manual"], 0);
    // The element as it was written, spaces and the literal 1.50 included.
    assert_eq!(row["body"], body);
}

#[tokio::test]
async fn each_element_of_a_batch_is_stored_as_the_text_it_arrived_in() {
    let first = r#"{ "level" : "job",  "queue_time": 1.50 }"#;
    let answer = webhook("/api/v2/webhook", &[], &format!("[ {first} ,\n {{\"level\":\"stage\"}} ]")).await;
    let rows = answer.rows("ci_webhook_events");
    assert_eq!(rows[0]["body"], first);
    assert_eq!(rows[1]["body"], r#"{"level":"stage"}"#);
}

#[tokio::test]
async fn a_time_that_is_not_rfc_3339_keeps_its_text_and_has_no_parse() {
    let answer =
        webhook("/api/v2/webhook", &[], r#"{"start":"2026-09-23 10:00:00","end":"2026-09-23T10:06:30.250+02:00"}"#)
            .await;
    let row = answer.row("ci_webhook_events");
    assert_eq!(row["start_raw"], "2026-09-23 10:00:00");
    assert_eq!(row["start_parsed"], Json::Null);
    assert_eq!(row["end_parsed"], "2026-09-23T08:06:30.250Z");
}

#[tokio::test]
async fn a_single_label_or_tag_sent_unwrapped_still_counts() {
    let answer = webhook(
        "/api/v2/webhook",
        &[],
        r#"{"node":{"labels":"linux"},"tags":"team:ci","parameters":{"N":3,"deep":{"a":1}}}"#,
    )
    .await;
    let row = answer.row("ci_webhook_events");
    assert_eq!(row["node_labels"], json!(["linux"]));
    assert_eq!(row["tags"], json!({ "team": ["ci"] }));
    assert_eq!(row["parameters"], json!({ "N": "3", "deep": r#"{"a":1}"# }));
}

#[tokio::test]
async fn the_delivery_id_falls_back_from_gitlab_to_github_to_datadogs_request_id() {
    async fn delivery(headers: &[(&str, &str)]) -> String {
        let answer = webhook("/api/v2/webhook", headers, r#"{"level":"job"}"#).await;
        answer.row("ci_webhook_events")["delivery_id"].as_str().unwrap().to_string()
    }
    assert_eq!(delivery(&[]).await, "");
    assert_eq!(delivery(&[("dd-request-id", "dd")]).await, "dd");
    assert_eq!(delivery(&[("dd-request-id", "dd"), ("x-github-delivery", "gh")]).await, "gh");
    assert_eq!(
        delivery(&[("dd-request-id", "dd"), ("x-github-delivery", "gh"), ("x-gitlab-event-uuid", "gl")]).await,
        "gl"
    );
}

#[tokio::test]
async fn the_synthetics_pollers_answer_does_not_depend_on_its_query() {
    let answer = serve(
        programs::synthetics_poller,
        "GET",
        "/api/unstable/synthetics/agents/tests?agent_hostname=h",
        &[("dd-api-key", KEY)],
        b"",
    )
    .await;
    assert_eq!(answer.status, 200);
    assert_eq!(answer.text(), r#"{"tests":[]}"#);
    assert!(answer.writes.is_empty());
}

// ---- install, eudm-intake, llmobs-intake ----

async fn install(method: &str, url: &str) -> Answer {
    serve(programs::install, method, url, &[], b"x").await
}

#[tokio::test]
async fn an_unhosted_blob_is_the_registrys_own_blob_unknown_with_no_api_key_asked() {
    let answer = install("GET", "/v2/datadog-agent/blobs/sha256:abc").await;
    assert_eq!(answer.status, 404);
    assert_eq!(answer.headers["docker-distribution-api-version"], "registry/2.0");
    let body: Json = serde_json::from_slice(&answer.body).unwrap();
    // go-containerregistry parses the code, so the exact string matters.
    assert_eq!(
        body,
        json!({ "errors": [{ "code": "BLOB_UNKNOWN", "message": "ninjacat does not host packages: datadog-agent@sha256:abc" }] })
    );
    assert!(answer.writes.is_empty());
}

#[tokio::test]
async fn a_host_that_asks_for_no_key_takes_no_upload_and_answers_no_key_check() {
    // What every keyed host also serves (a flare, /api/v1/validate) is not served here: it would
    // be a body anyone could make the intake read.
    let flare = install("POST", "/support/flare").await;
    // 405, not 404: the path exists here only as the HEAD probe.
    assert_eq!(flare.status, 405);
    assert!(flare.writes.is_empty());
    assert_eq!(install("GET", "/api/v1/validate").await.status, 404);
}

#[tokio::test]
async fn a_btf_archive_is_not_hosted_so_system_probe_falls_back_to_its_next_source() {
    let answer = install("GET", "/btfs/ubuntu/22.04/x86_64/5.15.0-91-generic.btf.tar.xz").await;
    assert_eq!(answer.status, 404);
    assert_eq!(answer.text(), r#"{"errors":["BTF archives are not hosted here"]}"#);
    assert!(!answer.headers.contains_key("docker-distribution-api-version"));
    assert!(answer.writes.is_empty());
}

#[tokio::test]
async fn an_llmobs_body_sent_by_the_diagnose_sweep_is_a_probe_not_a_payload() {
    let answer =
        post(programs::llm_obs, "/api/v2/llmobs", &[("x-requested-with", "datadog-agent-diagnose")], br#"{"span":{}}"#)
            .await;
    assert_eq!(answer.status, 202);
    assert!(answer.writes.is_empty());
}

#[tokio::test]
async fn the_note_of_an_aiusage_payload_quotes_the_proxys_origin_headers() {
    let answer = post(
        programs::ai_usage,
        "/api/v2/aiusage",
        &[
            ("dd-evp-origin", "ai \"prompt\" logger"),
            ("dd-evp-origin-version", "1.2.3"),
            ("content-type", "application/json"),
        ],
        br#"{"prompt":"hi"}"#,
    )
    .await;
    assert_eq!(
        answer.row("raw_payloads")["note"],
        r#"DD-EVP-ORIGIN="ai \"prompt\" logger" DD-EVP-ORIGIN-VERSION="1.2.3" Content-Type="application/json""#
    );
}
