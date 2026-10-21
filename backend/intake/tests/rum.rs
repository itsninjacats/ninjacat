//! Tests of routers/rum.rs beyond the golden fixtures, ported from the F# RumTests, and the browser
//! SDK's real payloads from RealPayloadTests.
//!
//! The RUM intake is the one surface whose clients are web pages and phones, and almost every
//! failure it can have is invisible from outside: a browser that loses a batch to CORS reports
//! success, an Android app that gets a 200 drops the batch, and a mobile batch can carry events
//! from hours ago.
//!
//! What the F# suite checked by replaying the fixtures recorded on Go's engine through the wrapper
//! with Go's peer, the golden replay here does for every one of them.

use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use chrono::Datelike;
use http_body_util::BodyExt;
use ninjapaws_intake::auth::{Admitted, Scheme};
use ninjapaws_intake::body::Part;
use ninjapaws_intake::json;
use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::routers::rum::{self, EventKind};
use ninjapaws_intake::server::{Ctx, Intake, query_value, router};
use ninjapaws_intake::wire::time;
use ninjapaws_storage::rows::rum::RumRequest;
use ninjapaws_storage::{Sink, Write as Batch};
use serde_json::{Value, json};
use tako::conn_info::ConnInfo;

const KEY: &str = "00000000000000000000000000000001";
/// The peer address every Go httptest request has.
const PEER: &str = "192.0.2.1:1234";
const BOUNDARY: &str = "rum-tests-boundary";

struct Answer {
    status: u16,
    headers: http::HeaderMap,
    body: Vec<u8>,
    writes: Vec<Batch>,
}

impl Answer {
    fn header(&self, name: &str) -> &str {
        self.headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default()
    }

    fn rows(&self, table: &str) -> Vec<&Value> {
        self.writes.iter().filter(|w| w.table == table).flat_map(|w| &w.rows).collect()
    }
}

/// browser-intake as it is served: the routes behind what sits above them, the peer a proxy it
/// trusts.
async fn serve(allowed_origins: &[String], method: &str, url: &str, headers: &[(&str, &str)], body: &[u8]) -> Answer {
    let store = Store::default();
    store.publish([(keys::hash(KEY), Key { id: "test".into(), name: "test".into(), tenant_id: "test".into() })]);
    let (sink, capture) = Sink::capture();
    let intake = Intake { sink, keys: Arc::new(store), ack_unknown: false, max_body_bytes: 64 * 1024 * 1024 };
    let gate = rum::above(
        router(intake, Some(Scheme::Browser), rum::mount),
        allowed_origins,
        rum::trusted_proxies("192.0.2.1").unwrap(),
    );
    let mut request = http::Request::builder().method(method).uri(url).header("host", "browser-intake.example.test");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut request: tako::types::Request = request.body(body.to_vec().into()).unwrap();
    request.extensions_mut().insert(ConnInfo::tcp(PEER.parse().unwrap()));
    let response = gate.dispatch(request).await;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    Answer { status, headers, body, writes: capture.writes() }
}

/// A POST with the test key in the header, as the mobile SDKs send it.
async fn post(url: &str, headers: &[(&str, &str)], body: &[u8]) -> Answer {
    let mut with_key = vec![("dd-api-key", KEY)];
    with_key.extend_from_slice(headers);
    serve(&[], "POST", url, &with_key, body).await
}

/// A request as the handlers see it once a key of tenant "t" has admitted it.
fn request_of(url: &str, headers: &[(&str, &str)]) -> Ctx {
    let mut builder = http::Request::builder().method("POST").uri(url);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let (mut parts, ()) = builder.body(()).unwrap().into_parts();
    parts.extensions.insert(Admitted(Key { id: "test".into(), name: "test".into(), tenant_id: "t".into() }));
    parts.extensions.insert(ConnInfo::tcp(PEER.parse().unwrap()));
    let (sink, _) = Sink::capture();
    Ctx { parts, intake: Arc::new(Intake { sink, keys: Arc::default(), ack_unknown: false, max_body_bytes: 0 }) }
}

fn sender() -> RumRequest {
    rum::request_info(&request_of("/api/v2/rum", &[]))
}

fn value(text: &str) -> Value {
    json::try_parse(text.as_bytes()).unwrap()
}

/// A multipart body of (name, file name, data); a part with a file name is sent as
/// application/octet-stream.
fn multipart(parts: &[(&str, &str, &[u8])]) -> (String, Vec<u8>) {
    let mut body = Vec::new();
    for (name, file_name, data) in parts {
        write!(body, "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"").unwrap();
        if !file_name.is_empty() {
            write!(body, "; filename=\"{file_name}\"\r\nContent-Type: application/octet-stream").unwrap();
        }
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    write!(body, "--{BOUNDARY}--\r\n").unwrap();
    (format!("multipart/form-data; boundary={BOUNDARY}"), body)
}

const ACTION: &str =
    r#"{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},"action":{"id":"a","type":"click"}}"#;

/// A view as a browser sends it: Web Vitals in the modern view.performance block with the
/// deprecated flat fields beside them.
const VIEW: &str = r#"{"type":"view","date":1591283924940,
        "application":{"id":"ac8218cf-498b-4d33-bd44-151095959547"},
        "session":{"id":"cacbf45c-3a05-48ce-b066-d76349460599","type":"user"},
        "view":{"id":"623d50fd-75cf-4025-97d2-e51ff94171f6","loading_type":"initial_load",
                "time_spent":245512755000,"largest_contentful_paint":1,"cumulative_layout_shift":0.9,
                "first_contentful_paint":420725000,"error":{"count":2},"frustration":{"count":9},
                "performance":{"cls":{"score":0.1},"lcp":{"timestamp":20000000}}},
        "_dd":{"document_version":9},"device":{"brand":"Apple"},"os":{"name":"iOS"},
        "feature_flags":{"feature_one":true}}"#;

#[tokio::test]
async fn the_browser_profilers_quota_check_is_admitted_with_the_key_in_dd_client_token() {
    let answer = serve(
        &[],
        "GET",
        "/api/v2/profiling/quota?session_id=s-1",
        &[("dd-client-token", KEY), ("origin", "https://shop.example")],
        b"",
    )
    .await;
    assert!(answer.writes.is_empty());
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, br#"{"data":{"attributes":{"admitted":true,"reason":"quota_ok"}}}"#);

    let refused =
        serve(&[], "GET", "/api/v2/profiling/quota?session_id=s-1", &[("dd-client-token", "not-a-key")], b"").await;
    assert_eq!(refused.status, 403);
}

#[test]
fn a_view_keeps_its_identity_its_version_and_the_sdks_date() {
    let row = rum::view_row(&sender(), "view", &value(VIEW), VIEW);

    assert_eq!(row.application_id, "ac8218cf-498b-4d33-bd44-151095959547");
    assert_eq!(row.view_id, "623d50fd-75cf-4025-97d2-e51ff94171f6");
    // The ReplacingMergeTree version.
    assert_eq!(row.document_version, 9);
    // Milliseconds, not seconds and not the arrival time.
    assert_eq!(row.date, time::from_unix_millis(1591283924940));
    // Averaging loading_time across initial_load and route_change means nothing, so the type
    // must survive.
    assert_eq!(row.view_loading_type, "initial_load");
    assert_eq!(row.view_time_spent, 245512755000);
    assert_eq!(row.error_count, Some(2));
    assert_eq!(row.frustration_count, Some(9));
    assert_eq!(row.device_brand, "Apple");
    assert_eq!(row.os_name, "iOS");
    assert!(row.feature_flags.contains("feature_one"));
    assert_eq!(row.event, VIEW);
}

#[test]
fn a_views_modern_vitals_win_over_the_deprecated_fields_which_remain_the_fallback() {
    let row = rum::view_row(&sender(), "view", &value(VIEW), VIEW);

    assert_eq!(row.lcp, Some(20000000));
    assert_eq!(row.cls, Some(0.1));
    // No view.performance.fcp in this event: the flat field answers.
    assert_eq!(row.fcp, Some(420725000));
    assert_eq!(row.inp, None);
}

#[test]
fn a_view_with_no_crash_object_has_not_measured_zero_crashes() {
    assert_eq!(rum::view_row(&sender(), "view", &value(VIEW), VIEW).crash_count, None);
}

#[tokio::test]
async fn a_view_update_lands_in_rum_views_under_the_same_identity() {
    let update = r#"{"type":"view_update","date":1712345678901,"application":{"id":"app"},"session":{"id":"s","type":"user"},"view":{"id":"v","url":"https://example.com/home","time_spent":245512755000},"_dd":{"document_version":2,"format_version":2}}"#;
    let answer = post("/api/v2/rum?ddsource=browser", &[], update.as_bytes()).await;

    assert_eq!(answer.status, 202);
    let rows = answer.rows("rum_views");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["event_type"], "view_update");
    assert_eq!(
        (&rows[0]["application_id"], &rows[0]["session_id"], &rows[0]["view_id"]),
        (&json!("app"), &json!("s"), &json!("v"))
    );
    // Without it a view_update can never win a merge.
    assert_eq!(rows[0]["document_version"], 2);
}

fn event_of(kind: &str, body: &str) -> ninjapaws_storage::rows::rum::RumEvent {
    let row = rum::event_row(&sender(), kind, &value(body), body);
    assert_eq!(row.event_type, kind);
    row
}

#[test]
fn an_action_carries_its_label_from_target_name_and_every_frustration_type() {
    let row = event_of(
        "action",
        r#"{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},
            "action":{"id":"a1","type":"click","target":{"name":"Buy now"},
                      "frustration":{"type":["rage_click","dead_click"]}}}"#,
    );

    assert_eq!(row.action_name, "Buy now");
    // One click can be classified twice.
    assert_eq!(row.action_frustration_types, ["rage_click", "dead_click"]);
    assert_eq!(row.error_message, "");
    assert_eq!(row.resource_url, "");
}

#[test]
fn a_crash_report_is_an_ordinary_error_with_is_crash_set() {
    let row = event_of(
        "error",
        r#"{"type":"error","date":1,"application":{"id":"app"},"session":{"id":"s"},
            "error":{"id":"e1","message":"boom","source":"source","is_crash":true,
                     "handling":"unhandled","fingerprint":"fp","stack":"at x"}}"#,
    );

    assert_eq!(row.error_is_crash, Some(true));
    assert_eq!(row.error_message, "boom");
    assert_eq!(row.error_fingerprint, "fp");
}

#[test]
fn an_error_that_never_mentions_is_crash_is_unknown_not_false() {
    let row = event_of(
        "error",
        r#"{"type":"error","date":1,"application":{"id":"app"},"session":{"id":"s"},
            "error":{"id":"e1","message":"boom","source":"source"}}"#,
    );
    assert_eq!(row.error_is_crash, None);
}

#[test]
fn a_resource_that_returned_0_bytes_is_not_a_resource_with_no_size() {
    let row = event_of(
        "resource",
        r#"{"type":"resource","date":1,"application":{"id":"app"},"session":{"id":"s"},
            "resource":{"id":"r1","type":"xhr","url":"https://x/y","method":"GET",
                        "status_code":204,"duration":1200,"size":0}}"#,
    );

    assert_eq!(row.resource_size, Some(0));
    assert_eq!(row.resource_status_code, Some(204));
    assert_eq!(row.long_task_duration, None);
}

#[test]
fn a_vital_keeps_the_sub_discriminator_that_picked_its_variant() {
    let row = event_of(
        "vital",
        r#"{"type":"vital","date":1,"application":{"id":"app"},"session":{"id":"s"},
            "vital":{"id":"v1","type":"duration","name":"checkout","duration":900}}"#,
    );

    assert_eq!(row.vital_type, "duration");
    assert_eq!(row.vital_name, "checkout");
    assert_eq!(row.vital_duration, Some(900));
}

/// Tags are a multiset: nothing forbids two tags sharing a key, and every SDK lets an app add its
/// own.
#[test]
fn an_event_keeps_repeated_tags() {
    let row = event_of(
        "action",
        r#"{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},
            "ddtags":"env:prod,team:web,team:payments,beta"}"#,
    );

    assert_eq!(row.ddtags.len(), 3);
    assert_eq!(row.ddtags["env"], ["prod"]);
    assert_eq!(row.ddtags["team"], ["web", "payments"]);
    assert_eq!(row.ddtags["beta"], [""]);
}

/// An SDK ships against a schema snapshot months older than ours and adds keys between releases.
#[test]
fn an_event_keeps_keys_no_schema_declares_and_64_bit_integers_keep_their_digits() {
    let row = event_of(
        "action",
        r#"{"type":"action","date":1,"application":{"id":"app"},"session":{"id":"s"},
            "action":{"id":"a1","type":"click","brand_new_field":42},
            "context":{"cart_value":9007199254740993,"nested":{"a":[1,"b"]}},
            "unknown_top_level":{"x":1}}"#,
    );

    // 2^53+1: through a float it would come back as ...992.
    assert!(row.context.contains("9007199254740993"));
    assert!(row.event.contains("unknown_top_level"));
    assert!(row.event.contains("brand_new_field"));
}

fn telemetry_event(telemetry: &str) -> String {
    format!(
        r#"{{"_dd":{{"format_version":2}},"type":"telemetry","date":1591284175342,"service":"sample-sdk","source":"browser","version":"1.2.3","application":{{"id":"ac8218cf"}},"session":{{"id":"cacbf45c"}},"view":{{"id":"623d50fd"}},"action":{{"id":"ae3a5d82"}},"experimental_features":["foo"],"telemetry":{telemetry}}}"#
    )
}

#[test]
fn an_error_telemetry_event_keeps_its_status_message_and_error() {
    let event = telemetry_event(
        r#"{"status":"error","message":"XHR error POST https://app.datadoghq.com/api/v1/logs-analytics/aggregate?type=rum","error":{"stack":"Failed to load","kind":"TypeError"},"custom":"property"}"#,
    );
    let row = rum::telemetry_row(&sender(), &value(&event), &event);

    assert_eq!(row.r#type, "telemetry");
    assert_eq!(row.status, "error");
    assert_eq!(row.message, "XHR error POST https://app.datadoghq.com/api/v1/logs-analytics/aggregate?type=rum");
    assert_eq!(row.error_stack, "Failed to load");
    assert_eq!(row.error_kind, "TypeError");
    assert_eq!((row.application_id.as_str(), row.session_id.as_str()), ("ac8218cf", "cacbf45c"));
    assert_eq!(row.experimental_features, ["foo"]);
}

/// The configuration object is the answer to "why is this app sending nothing?", so it is kept
/// whole.
#[test]
fn a_configuration_telemetry_event_keeps_the_whole_configuration() {
    let event =
        telemetry_event(r#"{"type":"configuration","configuration":{"session_sample_rate":100,"use_proxy":true}}"#);
    let row = rum::telemetry_row(&sender(), &value(&event), &event);

    assert_eq!(row.telemetry_type, "configuration");
    assert!(row.configuration.contains("session_sample_rate"));
    assert!(row.configuration.contains("use_proxy"));
}

#[test]
fn a_usage_telemetry_event_keeps_the_feature() {
    let event =
        telemetry_event(r#"{"type":"usage","usage":{"feature":"set-tracking-consent","tracking_consent":"granted"}}"#);
    let row = rum::telemetry_row(&sender(), &value(&event), &event);

    assert_eq!(row.telemetry_type, "usage");
    assert_eq!(row.usage_feature, "set-tracking-consent");
}

/// start/end are NANOSECONDS while `date` is milliseconds. Mixing the two would put a sample run
/// a million times too far from its event.
#[test]
fn a_timeseries_keeps_its_arrays_in_order_and_its_nanoseconds() {
    let event = r#"{"type":"timeseries","date":1591283924940,"application":{"id":"app"},
        "session":{"id":"s"},"source":"ios",
        "timeseries":{"id":"ts1","name":"memory","schema":"object-v2",
            "start":1591283924940000000,"end":1591283925940000000,
            "data":{"timestamps":[1591283924940000000,1591283925440000000],
                    "values":{"memory_footprint":[100,200]}}}}"#;
    let row = rum::timeseries_row(&sender(), &value(event), event);

    assert_eq!(
        (row.name.as_str(), row.timeseries_id.as_str(), row.timeseries_schema.as_str()),
        ("memory", "ts1", "object-v2")
    );
    assert_eq!(row.start, 1591283924940000000);
    assert_eq!(row.end, 1591283925940000000);
    assert_eq!(row.date, time::from_unix_millis(1591283924940));
    assert_eq!(row.timestamps, [1591283924940000000, 1591283925440000000]);
    assert!(row.values.contains("memory_footprint"));
}

/// The span format is hand-rolled per SDK with no published schema: ids are text of 64- and
/// 128-bit numbers, meta hides two nested objects among flat keys, and metrics are numbers.
#[test]
fn a_span_keeps_its_ids_as_text_and_gets_its_meta_flattened() {
    let span = r#"{"trace_id":"9007199254740993","span_id":2,"parent_id":"0",
        "name":"urlsession.request","service":"app","resource":"GET /x","type":"custom",
        "start":1591283924940000000,"duration":1500000,"error":1,
        "meta":{"_dd.source":"mobile","device":{"brand":"Apple","model":"iPhone"},
                "os":{"name":"iOS"},"tracer.version":"2.0.0","nulled":null},
        "metrics":{"_top_level":1,"_sampling_priority_v1":2}}"#;
    let row = rum::span_row(&sender(), "prod", "", &value(span), span);

    assert_eq!(row.trace_id, "9007199254740993");
    // An id sent as a JSON number keeps the digits it arrived with.
    assert_eq!(row.span_id, "2");
    assert_eq!(row.env, "prod");
    assert_eq!(row.start, 1591283924940000000);
    assert_eq!(row.duration_ns, 1500000);
    assert_eq!(row.error, 1);
    let meta: Vec<(&str, &str)> = row.meta.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(
        meta,
        [
            ("_dd.source", "mobile"),
            ("device.brand", "Apple"),
            ("device.model", "iPhone"),
            // Sent as null is not the same as not sent.
            ("nulled", ""),
            ("os.name", "iOS"),
            ("tracer.version", "2.0.0"),
        ]
    );
    assert_eq!(row.metrics["_sampling_priority_v1"], 2.0);
    assert_eq!(row.span, span);
}

#[test]
fn the_browsers_identity_is_read_from_the_query_string_and_the_key_never_becomes_durable() {
    let request = rum::request_info(&request_of(
        "/api/v2/rum?ddsource=browser&dd-api-key=k&dd-evp-origin=browser&dd-evp-origin-version=5.23.0&dd-request-id=req-1&batch_time=1591283924940&_dd.api=beacon",
        &[],
    ));

    assert_eq!(
        (request.ddsource.as_str(), request.evp_origin.as_str(), request.evp_origin_version.as_str()),
        ("browser", "browser", "5.23.0")
    );
    assert_eq!((request.request_id.as_str(), request.dd_api.as_str()), ("req-1", "beacon"));
    assert_eq!(request.batch_time, Some(time::from_unix_millis(1591283924940)));
    // A first attempt has no retry count: "retried zero times" is a different fact.
    assert_eq!(request.retry_count, None);
    assert_eq!(request.retry_after, None);
    assert_eq!(request.query_extra, "");
}

#[test]
fn a_browser_retry_is_read_from_dd_retry_count_and_dd_retry_after() {
    let request =
        rum::request_info(&request_of("/api/v2/rum?ddsource=browser&_dd.retry_count=2&_dd.retry_after=1000", &[]));
    assert_eq!(request.retry_count, Some(2));
    assert_eq!(request.retry_after, Some(1000));
}

#[test]
fn a_mobile_retry_is_the_same_two_facts_spelled_inside_ddtags_and_identity_comes_from_headers() {
    let request = rum::request_info(&request_of(
        "/api/v2/rum?ddsource=ios&ddtags=retry_count:3,retry_after:503",
        &[
            ("dd-evp-origin", "ios"),
            ("dd-evp-origin-version", "3.17.0"),
            ("dd-request-id", "req-2"),
            ("dd-idempotency-key", "sha1-of-body"),
        ],
    ));

    assert_eq!(
        (request.evp_origin.as_str(), request.evp_origin_version.as_str(), request.request_id.as_str()),
        ("ios", "3.17.0", "req-2")
    );
    // What tells a retry from a new batch.
    assert_eq!(request.idempotency_key, "sha1-of-body");
    assert_eq!(request.retry_count, Some(3));
    // The HTTP status that caused the retry, as the mobile SDKs send it.
    assert_eq!(request.retry_after, Some(503));
}

#[test]
fn a_negative_retry_count_and_an_unparsable_batch_time_are_absent_not_zero() {
    let request =
        rum::request_info(&request_of("/api/v2/rum?_dd.retry_count=-1&batch_time=abc&_dd.retry_after=%2B5", &[]));
    assert_eq!(request.retry_count, None);
    assert_eq!(request.batch_time, None);
    assert_eq!(request.retry_after, Some(5));
}

#[test]
fn the_compression_signal_is_kept_from_the_query_string_or_from_content_encoding() {
    for (url, content_encoding, expected) in [
        ("/api/v2/rum?ddsource=browser&dd-evp-encoding=deflate", "", "deflate"),
        ("/api/v2/rum?ddsource=android", "gzip", "gzip"),
        ("/api/v2/rum?ddsource=browser", "", ""),
    ] {
        let headers: Vec<(&str, &str)> =
            if content_encoding.is_empty() { vec![] } else { vec![("content-encoding", content_encoding)] };
        let request = rum::request_info(&request_of(url, &headers));
        assert_eq!(request.evp_encoding, expected);
        assert!(!request.query_extra.contains("dd-evp-encoding"));
    }
}

#[test]
fn a_parameter_with_no_column_is_kept_with_every_value_it_repeated() {
    let request =
        rum::request_info(&request_of("/api/v2/rum?ddsource=browser&brand_new=1&brand_new=2&api_key=zzz", &[]));
    assert_eq!(value(&request.query_extra), json!({ "brand_new": ["1", "2"] }));
}

#[test]
fn the_senders_address_is_the_peer_when_nothing_was_forwarded() {
    let request = rum::request_info(&request_of("/api/v2/rum", &[("user-agent", "Mozilla/5.0")]));
    assert_eq!(request.remote_addr, "192.0.2.1");
    assert_eq!(request.user_agent, "Mozilla/5.0");
}

/// The address a trusted proxy forwarded replaces the peer. X-Forwarded-For is read from the
/// right: the last entry is the one the trusted proxy itself wrote, the ones before it are the
/// client's claim.
#[tokio::test]
async fn the_address_forwarded_by_a_trusted_proxy_is_the_senders() {
    for (name, forwarded, expected) in [
        ("x-forwarded-for", "203.0.113.7", "203.0.113.7"),
        ("x-forwarded-for", "198.51.100.1, 203.0.113.7", "203.0.113.7"),
        // Not a header that is believed.
        ("x-real-ip", "198.51.100.9", "192.0.2.1"),
        ("x-forwarded-for", "203.0.113.7, not-an-address", "192.0.2.1"),
    ] {
        let url = format!("/api/v2/rum?ddsource=browser&dd-api-key={KEY}");
        let answer = serve(&[], "POST", &url, &[(name, forwarded)], ACTION.as_bytes()).await;
        assert_eq!(answer.status, 202);
        assert_eq!(answer.rows("rum_events")[0]["remote_addr"], expected, "{name}: {forwarded}");
    }
}

fn forwarded(target: &str) -> String {
    form_urlencoded::byte_serialize(target.as_bytes()).collect()
}

fn unwrapped(url: &str) -> http::Uri {
    let mut request: tako::types::Request = http::Request::builder().uri(url).body(Vec::new().into()).unwrap();
    rum::unwrap_forward(&mut request);
    request.uri().clone()
}

#[test]
fn a_forwarded_path_and_query_become_the_request() {
    let uri = unwrapped(&format!(
        "/rum-proxy?ddforward={}",
        forwarded("/api/v2/rum?ddsource=browser&dd-api-key=k&dd-request-id=r1")
    ));

    assert_eq!(uri.path(), "/api/v2/rum");
    assert_eq!(query_value(&uri, "ddsource"), "browser");
    assert_eq!(query_value(&uri, "dd-api-key"), "k");
    assert_eq!(query_value(&uri, "dd-request-id"), "r1");
    // Left in, it would be stored as an unknown parameter.
    assert!(!uri.query().unwrap().contains("ddforward"));
}

#[test]
fn an_outer_parameter_the_proxy_added_survives_the_merge_and_the_forwarded_one_wins_a_conflict() {
    let uri = unwrapped(&format!(
        "/rum-proxy?tenant=acme&ddsource=outer&ddforward={}",
        forwarded("/api/v2/logs?ddsource=browser")
    ));

    assert_eq!(uri.path(), "/api/v2/logs");
    assert_eq!(query_value(&uri, "tenant"), "acme");
    assert_eq!(query_value(&uri, "ddsource"), "browser");
}

#[test]
fn a_request_with_no_ddforward_or_one_that_is_not_a_path_is_left_as_it_came() {
    for url in [
        "/api/v2/rum?ddsource=browser",
        "/rum-proxy?ddforward=%3Fddsource%3Dbrowser",
        "/rum-proxy?ddforward=api%2Fv2%2Frum",
    ] {
        assert_eq!(unwrapped(url), url);
    }
}

/// The proxy-as-a-string form end to end: the SDK posts to its own path and everything that
/// matters, the key included, is inside ddforward.
#[tokio::test]
async fn a_batch_sent_through_ddforward_is_routed_admitted_and_stored() {
    let target = forwarded(&format!("/api/v2/rum?ddsource=browser&dd-api-key={KEY}&dd-evp-origin=browser"));
    let answer = serve(&[], "POST", &format!("/my-rum-proxy?ddforward={target}"), &[], ACTION.as_bytes()).await;

    assert_eq!(answer.status, 202);
    let rows = answer.rows("rum_events");
    assert_eq!(rows.len(), 1);
    assert_eq!((&rows[0]["ddsource"], &rows[0]["evp_origin"]), (&json!("browser"), &json!("browser")));
    assert_eq!(rows[0]["query_extra"], "");
}

/// A preflight for a path that is not served must be answered too, so the browser gets as far as
/// the POST and sees the 404.
#[tokio::test]
async fn a_preflight_is_answered_204_with_the_access_control_headers_and_needs_no_key() {
    for path in ["/api/v2/rum", "/api/v2/not-invented-yet"] {
        let answer = serve(
            &[],
            "OPTIONS",
            path,
            &[
                ("origin", "https://shop.example"),
                ("access-control-request-method", "POST"),
                ("access-control-request-headers", "content-type"),
            ],
            b"",
        )
        .await;

        assert_eq!(answer.status, 204, "{path}");
        assert!(answer.body.is_empty());
        assert_eq!(answer.header("access-control-allow-origin"), "https://shop.example");
        assert_eq!(answer.header("vary"), "Origin");
        assert!(answer.header("access-control-allow-methods").contains("POST"));
        assert!(answer.header("access-control-allow-headers").to_lowercase().contains("dd-api-key"));
        // Without it every request would preflight again.
        assert_eq!(answer.header("access-control-max-age"), "86400");
    }
}

/// Without the header on a 403 or a 404 the browser blocks the read, the SDK sees status 0 while
/// online, counts the batch as delivered and drops it.
#[tokio::test]
async fn every_answer_carries_the_origin_the_refusals_included() {
    for (method, url, status) in [
        ("POST", "/api/v2/rum?ddsource=browser&dd-api-key=wrong", 403),
        ("POST", "/api/v2/not-invented-yet", 404),
        ("GET", "/ping", 200),
    ] {
        let answer = serve(&[], method, url, &[("origin", "https://shop.example")], ACTION.as_bytes()).await;
        assert_eq!(answer.status, status, "{url}");
        assert_eq!(answer.header("access-control-allow-origin"), "https://shop.example", "{url}");
    }
}

#[tokio::test]
async fn a_request_with_no_origin_is_not_a_browser_and_gets_no_access_control_headers() {
    let answer = post("/api/v2/rum", &[], ACTION.as_bytes()).await;
    assert_eq!(answer.status, 202);
    assert!(answer.headers.keys().all(|name| !name.as_str().starts_with("access-control-") && name != "vary"));
}

#[test]
fn no_allowlist_an_empty_one_or_a_star_all_mean_any_origin() {
    for raw in ["", "*", " * ", " , "] {
        assert!(rum::parse_allowed_origins(raw).is_empty(), "{raw:?}");
    }
}

/// An allowlist is the operator's choice and is honoured exactly: an origin on it is allowed, one
/// that is not gets no header at all.
#[tokio::test]
async fn an_allowlist_admits_its_origins_and_gives_the_others_no_header() {
    let allowed = rum::parse_allowed_origins("https://shop.example, https://admin.example");
    assert_eq!(allowed, ["https://shop.example", "https://admin.example"]);
    for (origin, expected) in [
        ("https://shop.example", "https://shop.example"),
        ("https://admin.example", "https://admin.example"),
        ("https://evil.example", ""),
    ] {
        let answer = serve(
            &allowed,
            "OPTIONS",
            "/api/v2/rum",
            &[("origin", origin), ("access-control-request-method", "POST")],
            b"",
        )
        .await;
        assert_eq!(answer.status, 204);
        assert_eq!(answer.header("access-control-allow-origin"), expected);
    }
}

fn kind(event: &str) -> String {
    match rum::classify(&value(event)) {
        Ok(EventKind::View(event_type)) => format!("rum_views:{event_type}"),
        Ok(EventKind::Plain(event_type)) => format!("rum_events:{event_type}"),
        Ok(EventKind::Telemetry) => "rum_telemetry".into(),
        Ok(EventKind::Timeseries) => "rum_timeseries".into(),
        Err(unknown_type) => format!("unknown:{unknown_type}"),
    }
}

/// What the Go server did with each of these, recorded from it.
#[test]
fn an_events_table_is_decided_by_its_discriminator_fields() {
    let cases = [
        (r#"{"type":"view"}"#, "rum_views:view"),
        (r#"{"type":"view_update"}"#, "rum_views:view_update"),
        (r#"{"type":"transition"}"#, "rum_events:transition"),
        (r#"{"type":"long_task"}"#, "rum_events:long_task"),
        (r#"{"type":"resource"}"#, "rum_events:resource"),
        (r#"{"type":"vital","vital":{"type":"duration"}}"#, "rum_events:vital"),
        (r#"{"type":"vital","vital":{"type":"operation_step"}}"#, "rum_events:vital"),
        (r#"{"type":"vital","vital":{"type":"app_launch"}}"#, "rum_events:vital"),
        (r#"{"type":"vital","vital":{"type":"brand_new"}}"#, "unknown:vital"),
        (r#"{"type":"vital"}"#, "unknown:vital"),
        (r#"{"type":"vital","vital":"x"}"#, "unknown:vital"),
        (r#"{"type":"telemetry","telemetry":{"status":"error"}}"#, "rum_telemetry"),
        (r#"{"type":"telemetry","telemetry":{"type":"log","status":"debug"}}"#, "rum_telemetry"),
        (r#"{"type":"telemetry","telemetry":{"type":"configuration"}}"#, "rum_telemetry"),
        (r#"{"type":"telemetry","telemetry":{"type":"configuration","status":"error"}}"#, "rum_telemetry"),
        (r#"{"type":"telemetry","telemetry":{"type":"usage"}}"#, "rum_telemetry"),
        (r#"{"type":"telemetry","telemetry":{"type":"log","status":"warn"}}"#, "unknown:telemetry"),
        (r#"{"type":"telemetry","telemetry":{"type":"log"}}"#, "unknown:telemetry"),
        (r#"{"type":"telemetry","telemetry":{"type":"brand_new","status":"error"}}"#, "unknown:telemetry"),
        (r#"{"type":"telemetry","telemetry":{"type":null,"status":"error"}}"#, "unknown:telemetry"),
        (r#"{"type":"telemetry","telemetry":{"status":["error"]}}"#, "unknown:telemetry"),
        (r#"{"type":"telemetry"}"#, "unknown:telemetry"),
        (r#"{"type":"telemetry","telemetry":5}"#, "unknown:telemetry"),
        (r#"{"type":"timeseries","timeseries":{"name":"memory"}}"#, "rum_timeseries"),
        (r#"{"type":"timeseries","timeseries":{"name":"cpu"}}"#, "rum_timeseries"),
        (r#"{"type":"timeseries","timeseries":{"name":"battery"}}"#, "unknown:timeseries"),
        (r#"{"type":"timeseries"}"#, "unknown:timeseries"),
        (r#"{"type":"brand_new"}"#, "unknown:brand_new"),
        (r#"{"type":7}"#, "unknown:"),
        (r#"{"TYPE":"action"}"#, "unknown:"),
        ("{}", "unknown:"),
    ];
    for (event, expected) in cases {
        assert_eq!(kind(event), expected, "{event}");
    }
}

#[tokio::test]
async fn blank_lines_and_a_proxys_crlf_do_not_make_events() {
    let body = format!("\r\n{ACTION}\r\n\r\n  {ACTION}  \n\n");
    let answer = post("/api/v2/rum?ddsource=browser", &[], body.as_bytes()).await;

    assert_eq!(answer.status, 202);
    let rows = answer.rows("rum_events");
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|row| row["event"] == ACTION));
    assert!(answer.rows("raw_payloads").is_empty());
}

#[tokio::test]
async fn a_line_that_is_not_an_object_is_kept_raw_and_the_rest_of_the_batch_is_stored() {
    let body = format!("[1,2]\nnull\n{{bad\n{ACTION}");
    let answer = post("/api/v2/rum?ddsource=browser", &[], body.as_bytes()).await;

    assert_eq!(answer.status, 202);
    assert_eq!(answer.rows("rum_events").len(), 1);
    let raws = answer.rows("raw_payloads");
    assert_eq!(raws.iter().map(|raw| raw["body"].as_str().unwrap()).collect::<Vec<_>>(), ["[1,2]", "null", "{bad"]);
    assert!(raws.iter().all(|raw| raw["intake"] == "rum" && raw["reason"] == "decode_error"));
}

/// 202 always: a 500 would make every SDK retry the same batch forever.
#[tokio::test]
async fn a_date_beyond_year_9999_and_a_string_that_is_not_utf8_are_stored_not_answered_with_a_500() {
    let far_future = r#"{"type":"action","date":999999999999999999,"application":{"id":"far"}}"#;
    let broken_text = [&br#"{"type":"action","date":1,"application":{"id":" "#[..], &[0xff, 0xfe], b"\"}}"].concat();
    let body = [far_future.as_bytes(), b"\n", &broken_text].concat();
    let answer = post("/api/v2/rum?batch_time=999999999999999999", &[], &body).await;

    assert_eq!(answer.status, 202);
    let rows = answer.rows("rum_events");
    assert_eq!(rows.len(), 2);
    let date = chrono::DateTime::parse_from_rfc3339(rows[0]["date"].as_str().unwrap()).unwrap();
    assert_eq!(date.year(), 9999);
    assert!(rows[1]["application_id"].as_str().unwrap().contains('\u{FFFD}'));
}

#[tokio::test]
async fn the_agents_diagnose_sweep_and_an_empty_body_store_nothing() {
    for url in ["/api/v2/rum", "/api/v2/replay", "/api/v2/spans"] {
        let swept = post(url, &[("x-requested-with", "datadog-agent-diagnose")], ACTION.as_bytes()).await;
        let empty = post(url, &[], b"").await;
        assert_eq!((swept.status, empty.status), (202, 202));
        assert_eq!(swept.body, b"{}");
        assert!(swept.writes.is_empty() && empty.writes.is_empty());
    }
}

#[test]
fn a_replay_part_is_a_segment_or_a_resource() {
    for (part_name, meta_type, expected) in [
        ("segment", "", "segment"),
        ("file0", "", "segment"),
        ("image", "resource", "resource"),
        // A part named something else, classified by the metadata, or with nothing to go by.
        ("blob", "resource", "resource"),
        ("blob", "", "segment"),
    ] {
        let meta = if meta_type.is_empty() { json!({}) } else { json!({ "type": meta_type }) };
        assert_eq!(rum::replay_variant(part_name, &meta), expected, "{part_name}");
    }
}

#[tokio::test]
async fn a_browser_segment_is_stored_untouched_with_its_metadata() {
    let meta = r#"{"application":{"id":"app"},"session":{"id":"s"},"view":{"id":"v"},"source":"browser","start":1591283924940,"end":1591283925940,"records_count":12,"has_full_snapshot":true,"index_in_view":3,"creation_reason":"init","raw_segment_size":4096,"compressed_segment_size":512}"#;
    let segment = [0x78, 0x9c, 0x00, 0xff];
    let (content_type, body) = multipart(&[("segment", "sess-1", &segment), ("event", "blob", meta.as_bytes())]);
    let answer = post("/api/v2/replay?ddsource=browser", &[("content-type", &content_type)], &body).await;

    assert_eq!(answer.status, 202);
    let rows = answer.rows("rum_replay_segments");
    assert_eq!(rows.len(), 1);
    let row = rows[0];
    assert_eq!(
        (&row["variant"], &row["part_name"], &row["part_filename"]),
        (&json!("segment"), &json!("segment"), &json!("sess-1"))
    );
    assert_eq!(row["segment"], json!({ "$b64": "eJwA/w==" }));
    assert_eq!(row["image"], "");
    assert_eq!(
        [&row["application_id"], &row["session_id"], &row["view_id"], &row["source"]],
        [&json!("app"), &json!("s"), &json!("v"), &json!("browser")]
    );
    assert_eq!(row["start"], "2020-06-04T15:18:44.940Z");
    assert_eq!(row["index_in_view"], 3);
    assert_eq!(row["creation_reason"], "init");
    assert_eq!(row["event"], meta);
}

/// The mobile canvas variant repeats the part name; a map keyed by name would keep only the last
/// image and shuffle the pairing.
#[tokio::test]
async fn repeated_image_parts_each_get_a_row_paired_with_metadata_by_position() {
    let meta = r#"[{"type":"resource","application":{"id":"a0"}},{"type":"resource","application":{"id":"a1"}}]"#;
    let (content_type, body) =
        multipart(&[("image", "i0", b"img0"), ("image", "i1", b"img1"), ("event", "", meta.as_bytes())]);
    let answer = post("/api/v2/replay?ddsource=android", &[("content-type", &content_type)], &body).await;

    let rows = answer.rows("rum_replay_segments");
    assert_eq!(rows.iter().map(|row| row["application_id"].as_str().unwrap()).collect::<Vec<_>>(), ["a0", "a1"]);
    assert_eq!(rows.iter().map(|row| row["image"].as_str().unwrap()).collect::<Vec<_>>(), ["img0", "img1"]);
    assert!(rows.iter().all(|row| row["variant"] == "resource"));
    assert_eq!(rows[1]["event"], r#"{"type":"resource","application":{"id":"a1"}}"#);
}

#[tokio::test]
async fn a_blob_with_no_metadata_entry_is_still_stored() {
    let (content_type, body) =
        multipart(&[("file0", "file0", b"a"), ("file1", "file1", b"b"), ("event", "", br#"[{"view":{"id":"v0"}}]"#)]);
    let answer = post("/api/v2/replay", &[("content-type", &content_type)], &body).await;

    let rows = answer.rows("rum_replay_segments");
    assert_eq!(rows.iter().map(|row| row["view_id"].as_str().unwrap()).collect::<Vec<_>>(), ["v0", ""]);
    assert_eq!(rows[1]["event"], "");
}

#[tokio::test]
async fn an_event_part_that_is_neither_an_object_nor_an_array_is_kept_raw_and_the_blob_is_still_stored() {
    let (content_type, body) = multipart(&[("segment", "blob", b"zlib"), ("event", "", b"not json")]);
    let answer = post("/api/v2/replay", &[("content-type", &content_type)], &body).await;

    let raws = answer.rows("raw_payloads");
    assert_eq!(raws.len(), 1);
    assert_eq!((&raws[0]["reason"], &raws[0]["body"]), (&json!("decode_error"), &json!("not json")));
    let rows = answer.rows("rum_replay_segments");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["event"], "");
}

#[tokio::test]
async fn a_replay_body_that_is_not_multipart_is_kept_raw() {
    for (content_type, note) in [
        ("application/json", "replay body is not multipart: the content type is application/json, not multipart"),
        ("", "replay body is not multipart: the content type is not a media type"),
        ("multipart/form-data", "replay body is not multipart: the content type is multipart without a boundary"),
    ] {
        let headers: Vec<(&str, &str)> =
            if content_type.is_empty() { vec![] } else { vec![("content-type", content_type)] };
        let answer = post("/api/v2/replay", &headers, br#"{"a":1}"#).await;

        assert_eq!(answer.status, 202);
        let raws = answer.rows("raw_payloads");
        assert_eq!(raws.len(), 1);
        assert_eq!(
            [&raws[0]["intake"], &raws[0]["reason"], &raws[0]["note"]],
            [&json!("rum"), &json!("unexpected_shape"), &json!(note)]
        );
        assert!(answer.rows("rum_replay_segments").is_empty());
    }
}

#[tokio::test]
async fn a_multipart_body_that_breaks_off_is_kept_raw_whole_not_stored_in_part() {
    let (content_type, body) = multipart(&[("segment", "blob", b"zlib"), ("event", "", br#"{"view":{"id":"v"}}"#)]);
    let truncated = &body[..body.len() - 20];
    let answer = post("/api/v2/replay", &[("content-type", &content_type)], truncated).await;

    let raws = answer.rows("raw_payloads");
    assert_eq!(raws.len(), 1);
    assert_eq!(raws[0]["reason"], "unexpected_shape");
    assert_eq!(raws[0]["body_bytes"], truncated.len());
    assert!(answer.rows("rum_replay_segments").is_empty());
}

#[tokio::test]
async fn an_envelope_without_spans_and_a_span_that_is_not_an_object_are_kept_raw() {
    let body = [
        r#"{"env":"prod"}"#,
        r#"{"spans":{"a":1}}"#,
        r#"{"spans":["str",{"trace_id":"t1"}],"env":"prod","new_key":{"x":1}}"#,
    ]
    .join("\n");
    let answer = post("/api/v2/spans?ddsource=ios", &[], body.as_bytes()).await;

    assert_eq!(answer.status, 202);
    let raws: Vec<(&str, &str)> = answer
        .rows("raw_payloads")
        .iter()
        .map(|raw| (raw["reason"].as_str().unwrap(), raw["body"].as_str().unwrap()))
        .collect();
    assert_eq!(
        raws,
        [
            ("unexpected_shape", r#"{"env":"prod"}"#),
            ("decode_error", r#"{"spans":{"a":1}}"#),
            ("decode_error", "\"str\"")
        ]
    );
    let rows = answer.rows("rum_spans");
    assert_eq!(rows.len(), 1);
    assert_eq!((&rows[0]["trace_id"], &rows[0]["env"]), (&json!("t1"), &json!("prod")));
    assert_eq!(rows[0]["envelope_extra"], r#"{"new_key":{"x":1}}"#);
}

#[test]
fn a_spans_meta_keeps_numbers_booleans_and_arrays_as_text_and_its_metrics_keep_only_numbers() {
    let span = r#"{"trace_id":9007199254740993,"error":300,"meta":{"num":1.50,"yes":true,"arr":[1,"a"],"empty":{},"device":{"deep":{"er":"x"}}},"metrics":{"a":1,"b":2.5,"c":"x","d":null}}"#;
    let row = rum::span_row(&sender(), "", "", &value(span), span);

    assert_eq!(row.trace_id, "9007199254740993");
    // Go's int8 conversion wraps.
    assert_eq!(row.error, 44);
    let meta: Vec<(&str, &str)> = row.meta.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    assert_eq!(meta, [("arr", r#"[1,"a"]"#), ("device.deep.er", "x"), ("num", "1.50"), ("yes", "true")]);
    let metrics: Vec<(&str, f64)> = row.metrics.iter().map(|(k, v)| (k.as_str(), *v)).collect();
    assert_eq!(metrics, [("a", 1.0), ("b", 2.5)]);
}

#[test]
fn a_replay_row_puts_the_bytes_in_the_column_that_says_what_they_are() {
    let part = Part { name: "image".into(), data: Bytes::from_static(b"png"), ..Default::default() };
    let row = rum::replay_row(&sender(), &part, &json!({}), "{}");
    assert_eq!((row.variant.as_str(), row.image.0.as_slice(), row.segment.0.len()), ("resource", &b"png"[..], 0));
}

fn real(name: &str) -> Vec<u8> {
    std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real").join(name))
        .unwrap()
}

fn tally<'a>(names: impl Iterator<Item = &'a str>) -> String {
    let mut counts = std::collections::BTreeMap::new();
    for name in names {
        *counts.entry(name).or_insert(0) += 1;
    }
    counts.iter().map(|(name, count)| format!("{name}:{count}")).collect::<Vec<_>>().join(",")
}

#[tokio::test]
async fn a_batch_from_the_browser_sdk_becomes_views_and_events_with_nothing_left_raw() {
    for (file, views, events) in [
        ("browser-sdk-7.15.0-chromium.ndjson", 2, "action:3,error:2,long_task:1,resource:10"),
        ("browser-sdk-7.15.0-firefox.ndjson", 2, "action:3,error:2,resource:9"),
    ] {
        let answer =
            post("/api/v2/rum?ddsource=browser&dd-evp-origin=browser&dd-evp-origin-version=7.15.0", &[], &real(file))
                .await;
        assert_eq!(answer.status, 202);
        assert!(answer.rows("raw_payloads").is_empty(), "{file}");
        assert_eq!(answer.rows("rum_views").len(), views, "{file}");
        let rows = answer.rows("rum_events");
        assert_eq!(tally(rows.iter().map(|row| row["event_type"].as_str().unwrap())), events, "{file}");
        // What every event of a session carries, read from the real thing.
        for row in rows {
            assert_eq!(row["application_id"], "11111111-2222-3333-4444-555555555555");
            assert_ne!(row["session_id"], "");
            assert_ne!(row["view_id"], "");
        }
    }
}

#[tokio::test]
async fn the_browser_sdks_telemetry_becomes_telemetry_rows() {
    let answer = post("/api/v2/rum?ddsource=browser", &[], &real("browser-sdk-7.15.0-telemetry.ndjson")).await;
    assert!(answer.rows("raw_payloads").is_empty());
    let rows = answer.rows("rum_telemetry");
    assert_eq!(tally(rows.iter().map(|row| row["telemetry_type"].as_str().unwrap())), "configuration:1,usage:8");
}
