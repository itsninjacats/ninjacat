//! Tests of the logs intake beyond the golden fixtures: the cases of the F# RealPayloadTests and
//! IntakeCoreTests for the logs host, and the F# TcpLogsTests for the agent's logs over TCP.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::{TimeDelta, TimeZone, Utc};
use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::routers::logs;
use ninjapaws_intake::server::{Intake, Process, router};
use ninjapaws_intake::tcp_logs::{self, FrameResult, Framing};
use ninjapaws_proto::pb;
use ninjapaws_storage::{Capture, Sink, Write as Batch};
use prost::Message;
use serde_json::Value;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

const KEY: &str = "00000000000000000000000000000001";

fn store() -> Arc<Store> {
    let store = Store::default();
    store.publish([(keys::hash(KEY), Key { id: "test".into(), name: "test".into(), tenant_id: "test".into() })]);
    Arc::new(store)
}

/// One POST of a body to the logs host with the test key: what was written.
async fn post(path: &str, body: &[u8]) -> Vec<Batch> {
    let (sink, capture) = Sink::capture();
    let intake = Intake { sink, keys: store(), ack_unknown: false, max_body_bytes: 64 * 1024 * 1024 };
    let request = http::Request::builder().method("POST").uri(path).header("dd-api-key", KEY);
    let response =
        router(intake, Some(Scheme::Agent), logs::mount).dispatch(request.body(body.to_vec().into()).unwrap()).await;
    assert_eq!(response.status(), 202);
    capture.writes()
}

fn rows<'a>(writes: &'a [Batch], table: &str) -> Vec<&'a Value> {
    writes.iter().filter(|w| w.table == table).flat_map(|w| &w.rows).collect()
}

#[tokio::test]
async fn logs_from_the_dotnet_tracer_which_have_no_message_field_become_log_rows() {
    let body = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/real/dd-trace-dotnet-3.54.0-logs.json"),
    )
    .unwrap();
    let writes = post("/api/v2/logs", &body).await;

    assert!(rows(&writes, "raw_payloads").is_empty());
    let rows = rows(&writes, "logs");
    assert_eq!(rows.len(), 2);
    let (started, refused) = (rows[0], rows[1]);
    assert_eq!(started["message"], "Now listening on: http://[::]:8080");
    assert_eq!(started["status"], "info");
    assert_eq!(started["service"], "ninjacat-api");
    assert_eq!(started["source"], "csharp");
    assert_eq!(started["host"], "9f06fd4497f5");
    let expected = Utc.with_ymd_and_hms(2026, 10, 1, 20, 43, 16).unwrap() + TimeDelta::nanoseconds(409_169_200);
    assert_eq!(started["timestamp"], expected.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true));
    assert_eq!(started["timestamp_source"], "compact_string");
    // What became a column is not repeated among the attributes.
    let attributes = started["attributes"].as_str().unwrap();
    assert!(!attributes.contains("\"@m\""));
    assert!(!attributes.contains("\"@t\""));
    assert!(!attributes.contains("dd_service"));
    assert!(attributes.contains("\"Category\":\"Microsoft.Hosting.Lifetime\""));

    assert_eq!(refused["status"], "warn");
    assert!(refused["attributes"].as_str().unwrap().contains("\"dd_trace_id\":\"6abec5e5000000002f9a60afb73b9615\""));
}

#[tokio::test]
async fn an_empty_object_among_the_logs_is_not_stored() {
    let writes = post("/api/v2/logs", br#"[{}, {"message":"kept"}]"#).await;
    assert!(rows(&writes, "raw_payloads").is_empty());
    assert_eq!(rows(&writes, "logs").iter().map(|row| row["message"].as_str().unwrap()).collect::<Vec<_>>(), ["kept"]);
}

/// The shared reader nests 128 deep (the F# one 512); the rule is the same on both sides of it.
#[tokio::test]
async fn a_body_nested_deeper_than_any_telemetry_is_kept_raw_and_one_inside_the_limit_is_stored() {
    let send = |depth: usize| async move {
        let deep = format!("{}{}", "[".repeat(depth), "]".repeat(depth));
        let writes = post("/api/v2/logs", format!(r#"[{{"message":"m","deep":{deep}}}]"#).as_bytes()).await;
        rows(&writes, "raw_payloads").iter().map(|raw| raw["reason"].as_str().unwrap().to_string()).collect::<Vec<_>>()
    };
    // Past the limit the body does not parse at all: kept whole, answered as usual.
    assert_eq!(send(228).await, ["decode_error"]);
    // Inside it: parsed, and writing it back into a column must not fail.
    assert!(send(100).await.is_empty());
}

fn proto_log(message: &str, timestamp: i64) -> Vec<u8> {
    pb::Log {
        message: message.into(),
        status: "info".into(),
        timestamp,
        hostname: "h1".into(),
        service: "svc".into(),
        source: "go".into(),
        tags: vec!["env:prod".into()],
    }
    .encode_to_vec()
}

/// `<key> <payload>` with its 4-byte length in front.
fn prefixed(key: &str, payload: &[u8]) -> Vec<u8> {
    let body = [format!("{key} ").as_bytes(), payload].concat();
    [&(body.len() as u32).to_be_bytes()[..], &body].concat()
}

fn handle(framing: Framing, frame: &[u8]) -> FrameResult {
    tcp_logs::handle_frame(&store(), "10.0.0.1:5000", framing, frame)
}

fn keyed(payload: &[u8]) -> Vec<u8> {
    [format!("{KEY} ").as_bytes(), payload].concat()
}

#[test]
fn a_key_and_a_space_at_the_start_is_the_line_framing_anything_else_the_default() {
    assert_eq!(tcp_logs::detect_framing(format!("{KEY} hello\n").as_bytes()), Framing::Lines);
    assert_eq!(tcp_logs::detect_framing(&prefixed(KEY, &proto_log("x", 0))), Framing::LengthPrefixed);
    assert_eq!(tcp_logs::detect_framing(b"short"), Framing::LengthPrefixed);
    // 32 characters that are not all hex.
    assert_eq!(tcp_logs::detect_framing(format!("zz{} hello", &KEY[2..]).as_bytes()), Framing::LengthPrefixed);
}

#[test]
fn frames_are_cut_out_as_they_complete() {
    let one = prefixed(KEY, &proto_log("a", 0));
    let stream = [&one[..], &one, &one[..10]].concat();
    let (frames, consumed) = tcp_logs::take_frames(Framing::LengthPrefixed, &stream).unwrap();
    assert_eq!(frames.len(), 2);
    assert_eq!(consumed, 2 * one.len());

    let (frames, consumed) = tcp_logs::take_frames(Framing::Lines, b"k one\nk two\nk thr").unwrap();
    assert_eq!(frames, [&b"k one"[..], b"k two"]);
    assert_eq!(consumed, 12);
}

#[test]
fn a_frame_declaring_more_than_the_limit_is_refused_unread() {
    assert!(tcp_logs::take_frames(Framing::LengthPrefixed, &[0x7f, 0xff, 0xff, 0xff]).is_err());
}

#[test]
fn a_protobuf_log_becomes_a_row_with_its_own_timestamp() {
    let FrameResult::LogLine(row) = handle(Framing::LengthPrefixed, &keyed(&proto_log("hello", 1790151330000))) else {
        panic!("not a log line")
    };
    assert_eq!(row.tenant_id, "test");
    assert_eq!(row.message, "hello");
    assert_eq!(row.host, "h1");
    assert_eq!(row.timestamp_source, "tcp_proto_ms");
    assert_eq!(row.timestamp, Utc.timestamp_millis_opt(1790151330000).unwrap());
    assert_eq!(row.tags["env"], ["prod"]);
}

#[test]
fn a_protobuf_log_without_a_timestamp_is_stamped_on_arrival() {
    let FrameResult::LogLine(row) = handle(Framing::LengthPrefixed, &keyed(&proto_log("hello", 0))) else {
        panic!("not a log line")
    };
    assert_eq!(row.timestamp_source, "arrival");
}

#[test]
fn bytes_that_are_not_a_protobuf_log_are_kept_raw_with_the_peer() {
    let FrameResult::Undecodable(row) = handle(Framing::LengthPrefixed, &keyed("ÿÿ not proto".as_bytes())) else {
        panic!("not kept raw")
    };
    assert_eq!(row.intake, "logs-tcp");
    assert_eq!(row.reason, "decode_error");
    assert_eq!(row.method, "TCP");
    assert_eq!(row.headers["Remote-Addr"], "10.0.0.1:5000");
    assert!(row.note.starts_with("length-prefixed proto: "));
}

#[test]
fn a_json_line_is_read_as_a_log_any_other_line_is_the_message() {
    let FrameResult::LogLine(row) =
        handle(Framing::Lines, &keyed(br#"{"message":"from json","service":"api","status":"error"}"#))
    else {
        panic!("not a log line")
    };
    assert_eq!((row.message.as_str(), row.service.as_str(), row.status.as_str()), ("from json", "api", "error"));

    let FrameResult::LogLine(row) = handle(Framing::Lines, &keyed(b"plain text, with spaces")) else {
        panic!("not a log line")
    };
    assert_eq!(row.message, "plain text, with spaces");
    assert_eq!(row.timestamp_source, "arrival");

    // JSON, but not shaped like a log: still the whole line.
    let FrameResult::LogLine(row) = handle(Framing::Lines, &keyed(br#"{"message":5}"#)) else {
        panic!("not a log line")
    };
    assert_eq!(row.message, r#"{"message":5}"#);
}

#[test]
fn an_unknown_key_closes_the_connection_a_frame_without_a_key_is_dropped() {
    assert!(matches!(handle(Framing::Lines, b"ffffffffffffffffffffffffffffffff hello"), FrameResult::UnknownKey));
    assert!(matches!(handle(Framing::Lines, b"nospacehere"), FrameResult::Nothing));
    assert!(matches!(handle(Framing::Lines, b""), FrameResult::Nothing));
}

/// A listener on a free port, as the logs binary starts it, and the rows it is handed.
async fn listening(tls: Option<tokio_rustls::TlsAcceptor>) -> (u16, Process, Arc<Capture>) {
    let (sink, capture) = Sink::capture();
    let process = Process { keys: store(), sink, stopping: CancellationToken::new(), tasks: TaskTracker::new() };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tcp_logs::serve(listener, tls, &process);
    (port, process, capture)
}

fn messages(capture: &Capture) -> Vec<String> {
    capture
        .writes()
        .iter()
        .filter(|w| w.table == "logs")
        .flat_map(|w| &w.rows)
        .map(|row| row["message"].as_str().unwrap().to_string())
        .collect()
}

/// Waits for the rows, then stops the process as the binary does and waits for its connections.
async fn rows_after(process: Process, capture: &Capture, expected: usize) -> Vec<String> {
    for _ in 0..250 {
        if messages(capture).len() >= expected {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    process.stopping.cancel();
    process.tasks.close();
    process.tasks.wait().await;
    messages(capture)
}

#[tokio::test]
async fn length_prefixed_frames_over_a_real_connection() {
    let (port, process, capture) = listening(None).await;
    let payload = [prefixed(KEY, &proto_log("one", 0)), prefixed(KEY, &proto_log("two", 0))].concat();
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    client.write_all(&payload).await.unwrap();
    drop(client);

    assert_eq!(rows_after(process, &capture, 2).await, ["one", "two"]);
}

#[tokio::test]
async fn lines_over_a_real_connection_and_rows_before_an_unknown_key_are_kept() {
    let (port, process, capture) = listening(None).await;
    let payload = format!("{KEY} first\n{KEY} second\nffffffffffffffffffffffffffffffff third\n{KEY} never read\n");
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    client.write_all(payload.as_bytes()).await.unwrap();

    assert_eq!(rows_after(process, &capture, 2).await, ["first", "second"]);
}

/// The agent holds its connection open: a stopping process is the one to leave, and what was read
/// is kept.
#[tokio::test]
async fn a_connection_held_open_is_flushed_when_the_process_stops() {
    let (port, process, capture) = listening(None).await;
    let mut client = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    client.write_all(format!("{KEY} held\n").as_bytes()).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(rows_after(process, &capture, 1).await, ["held"]);
    drop(client);
}

#[tokio::test]
async fn frames_over_tls() {
    use tokio_rustls::rustls::pki_types::{CertificateDer, ServerName};
    use tokio_rustls::rustls::{ClientConfig, RootCertStore};

    let certified = rcgen::generate_simple_self_signed(vec!["localhost".to_string()]).unwrap();
    let directory = std::env::temp_dir().join(format!("ninjapaws-tls-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let (cert_file, key_file) = (directory.join("cert.pem"), directory.join("key.pem"));
    std::fs::write(&cert_file, certified.cert.pem()).unwrap();
    std::fs::write(&key_file, certified.signing_key.serialize_pem()).unwrap();
    let acceptor = tcp_logs::tls_acceptor(cert_file.to_str().unwrap(), key_file.to_str().unwrap()).unwrap();
    std::fs::remove_dir_all(&directory).unwrap();

    let (port, process, capture) = listening(Some(acceptor)).await;
    let mut roots = RootCertStore::empty();
    roots.add(CertificateDer::from(certified.cert.der().to_vec())).unwrap();
    let connector = tokio_rustls::TlsConnector::from(Arc::new(
        ClientConfig::builder().with_root_certificates(roots).with_no_client_auth(),
    ));
    let tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut client = connector.connect(ServerName::try_from("localhost").unwrap(), tcp).await.unwrap();
    client.write_all(&prefixed(KEY, &proto_log("encrypted", 0))).await.unwrap();
    client.shutdown().await.unwrap();

    assert_eq!(rows_after(process, &capture, 1).await, ["encrypted"]);
}
