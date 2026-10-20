//! The golden fixtures of the F# intake (`api/tests/Intake.Tests/Fixtures/go`): requests recorded
//! from the Go server, each with its answer and the rows it stored. Every fixture a ported host
//! serves is replayed through that host's router, in memory, and compared field by field;
//! `Fixtures/overrides.json` lists the accepted differences, as for the F# replay.
//!
//! Equal means equal in meaning: object key order does not matter, a number is a number however
//! it is written, a timestamp may lose digits past 100 ns, and a string that is itself JSON is
//! compared as JSON.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use http_body_util::BodyExt;
use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::routers::{api, app};
use ninjapaws_intake::server::{Intake, Routes, router};
use ninjapaws_storage::Sink;
use serde_json::Value;

/// The keys the Go tests used: one for handler tests, one for the tests of the host dispatch.
fn test_keys() -> Arc<Store> {
    let store = Store::default();
    store.publish([
        (
            keys::hash("00000000000000000000000000000001"),
            Key { id: "test".into(), name: "test".into(), tenant_id: "test".into() },
        ),
        (
            keys::hash("routes-test-key"),
            Key { id: "k1".into(), name: "routes-test".into(), tenant_id: "tenant-1".into() },
        ),
    ]);
    Arc::new(store)
}

fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../api/tests/Intake.Tests/Fixtures")
}

/// What api.<site> serves in the F# intake and this port does not yet: the CI Visibility
/// endpoints, ported with the CI hosts.
const NOT_PORTED_ON_API: [&str; 10] = [
    "/api/v2/libraries/tests/services/setting",
    "/api/v2/ci/tests/skippable",
    "/api/v2/ci/libraries/tests",
    "/api/v2/ci/libraries/tests/flaky",
    "/api/v2/test/libraries/test-management/tests",
    "/api/v2/git/repository/search_commits",
    "/api/v2/git/repository/packfile",
    "/api/v2/ci/pipeline/tags",
    "/api/v2/ci/pipeline/metrics",
    "/api/intake/ci/custom_spans",
];

/// A ported host's process as the fixtures need it: the key it asks for and its routes.
struct Process {
    scheme: Option<Scheme>,
    mounts: Vec<fn(&mut Routes)>,
}

fn process(scheme: Option<Scheme>, mount: fn(&mut Routes)) -> Option<Process> {
    Some(Process { scheme, mounts: vec![mount] })
}

/// The Go server's name for each group of routes, as the fixtures recorded which groups served a
/// request, and the router that serves that group here.
fn route_set(name: &str) -> Option<Process> {
    let agent = Some(Scheme::Agent);
    match name {
        "routeAPI" => process(agent, api::metrics_and_checks),
        "routeApp" => process(agent, app::metrics_v3),
        _ => None,
    }
}

/// The process an intake host name is sent to.
fn host_process(host: &str) -> Option<Process> {
    let agent = Some(Scheme::Agent);
    if host.starts_with("app.") || host.contains("-app.agent.") {
        return process(agent, app::mount);
    }
    if host.starts_with("api.") || host.contains("-flare.agent.") {
        return process(agent, api::mount);
    }
    None
}

/// The process that serves a fixture, or None for one no ported host serves.
fn process_for(fixture: &Value) -> Option<Process> {
    let routes: Vec<&str> = fixture["routes"].as_array()?.iter().filter_map(Value::as_str).collect();
    let request = &fixture["request"];
    let host = request["host"].as_str()?;
    let path = request["url"].as_str()?.split('?').next()?;

    // Most Go tests built one engine and sent to "example.com"; the ones that went through the
    // host dispatch carry a real intake host.
    if host != "example.com" {
        if NOT_PORTED_ON_API.contains(&path) {
            return None;
        }
        return host_process(host);
    }
    if routes.is_empty() {
        // The Go flare tests called its router alone; every host with a key serves it.
        return path.starts_with("/support/flare").then(|| Process { scheme: Some(Scheme::Agent), mounts: Vec::new() });
    }
    // The intake made of the named route sets, guarded as the first one is.
    let sets: Vec<Process> = routes.iter().map(|name| route_set(name)).collect::<Option<_>>()?;
    Some(Process { scheme: sets[0].scheme, mounts: sets.into_iter().flat_map(|set| set.mounts).collect() })
}

struct Answer {
    status: u16,
    headers: http::HeaderMap,
    body: Vec<u8>,
}

fn router_of(process: &Process, sink: Sink) -> tako::router::Router {
    let intake = Intake { sink, keys: test_keys(), ack_unknown: false, max_body_bytes: 64 * 1024 * 1024 };
    router(intake, process.scheme, |routes| {
        for mount in &process.mounts {
            mount(routes);
        }
    })
}

/// The fixture's request. Of two values of a header the later one counts, as the F# replay sets
/// them.
fn request_of(fixture: &Value) -> tako::types::Request {
    let request = &fixture["request"];
    let mut builder = http::Request::builder()
        .method(request["method"].as_str().unwrap())
        .uri(request["url"].as_str().unwrap())
        .header("host", request["host"].as_str().unwrap());
    for (name, values) in request["headers"].as_object().unwrap() {
        if let Some(value) = values.as_array().and_then(|v| v.last()).and_then(Value::as_str) {
            builder = builder.header(name, value);
        }
    }
    let body = base64::engine::general_purpose::STANDARD.decode(request["body_b64"].as_str().unwrap()).unwrap();
    builder.body(body.into()).unwrap()
}

async fn send(fixture: &Value, process: &Process) -> (Answer, Vec<ninjapaws_storage::Write>) {
    let (sink, capture) = Sink::capture();
    let router = router_of(process, sink);
    let response = router.dispatch(request_of(fixture)).await;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    (Answer { status, headers, body }, capture.writes())
}

fn strings(node: &Value) -> Vec<String> {
    node.as_array().map(|items| items.iter().filter_map(|i| i.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// A timestamp cut to 100 ns, or None when the text is not one.
fn timestamp_to_100ns(text: &str) -> Option<String> {
    let pattern = regex::Regex::new(r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(\.\d+)?Z$").unwrap();
    let found = pattern.captures(text)?;
    let digits = found.get(2).map_or("", |m| &m.as_str()[1..]);
    Some(format!("{}.{:0<7}", &found[1], &digits[..digits.len().min(7)]))
}

fn same_number(expected: &Value, actual: &Value) -> bool {
    if expected == actual {
        return true;
    }
    match (expected.as_f64(), actual.as_f64()) {
        (Some(a), Some(b)) => a == b || (a - b).abs() <= 1e-12 * a.abs().max(b.abs()),
        _ => false,
    }
}

fn json_container(text: &str) -> Option<Value> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') || trimmed.starts_with('[') { serde_json::from_str(text).ok() } else { None }
}

fn show(node: &Value) -> String {
    let text = node.to_string();
    if text.chars().count() > 160 { format!("{}…", text.chars().take(160).collect::<String>()) } else { text }
}

/// Every difference between `expected` (Go) and `actual` (Rust), as readable lines.
fn differences(ignored: &HashSet<String>, path: &str, expected: &Value, actual: &Value, found: &mut Vec<String>) {
    let is_ignored = |p: &str| ignored.contains(p) || ignored.contains(&format!("{p}/$b64"));
    if is_ignored(path) {
        return;
    }
    match (expected, actual) {
        (Value::Object(e), Value::Object(a)) => {
            for (key, value) in e {
                let at = format!("{path}/{key}");
                match a.get(key) {
                    Some(other) => differences(ignored, &at, value, other, found),
                    None if !is_ignored(&at) => found.push(format!("{at}: missing, Go has {}", show(value))),
                    None => {}
                }
            }
            for (key, value) in a {
                let at = format!("{path}/{key}");
                if !e.contains_key(key) && !is_ignored(&at) {
                    found.push(format!("{at}: not in Go, Rust has {}", show(value)));
                }
            }
        }
        (Value::Array(e), Value::Array(a)) => {
            if e.len() != a.len() {
                found.push(format!("{path}: Go has {} items, Rust has {}", e.len(), a.len()));
            } else {
                for (i, (x, y)) in e.iter().zip(a).enumerate() {
                    differences(ignored, &format!("{path}/{i}"), x, y, found);
                }
            }
        }
        (Value::Number(_), Value::Number(_)) => {
            if !same_number(expected, actual) {
                found.push(format!("{path}: Go {}, Rust {}", show(expected), show(actual)));
            }
        }
        (Value::String(e), Value::String(a)) if e != a => match (timestamp_to_100ns(e), timestamp_to_100ns(a)) {
            (Some(et), Some(at)) if et == at => {}
            _ => match (json_container(e), json_container(a)) {
                (Some(ej), Some(aj)) => differences(ignored, &format!("{path}~json"), &ej, &aj, found),
                _ => found.push(format!("{path}: Go {}, Rust {}", show(expected), show(actual))),
            },
        },
        (Value::String(_), Value::String(_)) | (Value::Null, Value::Null) => {}
        (Value::Bool(e), Value::Bool(a)) if e == a => {}
        _ => found.push(format!("{path}: Go {}, Rust {}", show(expected), show(actual))),
    }
}

/// The column names of an INSERT statement's column list.
fn insert_columns(insert: &str) -> Vec<String> {
    let opening = insert.find('(').unwrap();
    let closing = opening + insert[opening..].find(')').unwrap();
    insert[opening + 1..closing].split(',').map(|c| c.trim().to_string()).collect()
}

/// Runs the fixture's request; returns the differences from what Go did.
async fn run(id: &str, fixture: &Value, process: &Process, overrides: &Value) -> Vec<String> {
    let (response, writes) = send(fixture, process).await;
    let mut ignored: HashSet<String> = strings(&fixture["volatile"]).into_iter().collect();
    ignored.extend(strings(&overrides[id]["ignore"]));
    let mut found = Vec::new();
    let expected = &fixture["response"];

    // --- the response ---

    let expected_status = expected["status"].as_u64().unwrap() as u16;
    if expected_status != response.status {
        found.push(format!("/response/status: Go {expected_status}, Rust {}", response.status));
    }
    for (name, values) in expected["headers"].as_object().unwrap() {
        let path = format!("/response/headers/{name}");
        // A volatile header is listed by the path of its value, `…/<name>/0`.
        if name != "Content-Length" && !ignored.contains(&path) && !ignored.contains(&format!("{path}/0")) {
            let want = strings(values);
            let got: Vec<String> =
                response.headers.get_all(name.as_str()).iter().map(|v| v.to_str().unwrap().to_string()).collect();
            if want != got {
                found.push(format!("{path}: Go {want:?}, Rust {got:?}"));
            }
        }
    }
    let expected_body =
        base64::engine::general_purpose::STANDARD.decode(expected["body_b64"].as_str().unwrap()).unwrap();
    if !expected["body_json"].is_null() {
        let actual = serde_json::from_slice(&response.body)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&response.body).into()));
        differences(&ignored, "/response/body_json", &expected["body_json"], &actual, &mut found);
    } else if !ignored.contains("/response/body_b64") && expected_body != response.body {
        found.push(format!(
            "/response/body_b64: Go {} bytes, Rust {} bytes, and they differ",
            expected_body.len(),
            response.body.len()
        ));
    }

    // --- what was sent to storage ---

    // Null means the Go test ran without a capturing node: nothing to compare.
    let Some(sends) = fixture["sends"].as_array() else { return found };
    if sends.len() != writes.len() {
        let go: Vec<&str> = sends.iter().filter_map(|s| s["to"].as_str()).collect();
        let rust: Vec<&str> = writes.iter().map(|w| w.writer).collect();
        found.push(format!("/sends: Go wrote {} batches {go:?}, Rust wrote {} {rust:?}", sends.len(), writes.len()));
        return found;
    }
    for (i, (send, write)) in sends.iter().zip(&writes).enumerate() {
        let path = format!("/sends/{i}");
        if send["to"].as_str() != Some(write.writer) {
            found.push(format!("{path}/to: Go {}, Rust {}", send["to"], write.writer));
            continue;
        }
        let columns = insert_columns(send["insert"].as_str().unwrap());
        if columns != write.columns {
            found.push(format!("{path}/insert: Go columns {columns:?}, Rust columns {:?}", write.columns));
            continue;
        }
        let args: Vec<Value> = write
            .rows
            .iter()
            .map(|row| Value::Array(write.columns.iter().map(|c| row[*c].clone()).collect()))
            .collect();
        differences(&ignored, &format!("{path}/args"), &send["args"], &Value::Array(args), &mut found);
    }
    found
}

#[tokio::test]
async fn every_fixture_of_a_ported_host_is_answered_and_stored_as_it_says() {
    let root = fixtures_root();
    let overrides: Value = serde_json::from_slice(&std::fs::read(root.join("overrides.json")).unwrap()).unwrap();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(root.join("go"))
        .unwrap()
        .flat_map(|dir| std::fs::read_dir(dir.unwrap().path()).unwrap())
        .map(|f| f.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    paths.sort();

    let mut replayed = 0;
    let mut failures = Vec::new();
    for path in paths {
        let id = format!(
            "{}/{}",
            path.parent().unwrap().file_name().unwrap().to_str().unwrap(),
            path.file_stem().unwrap().to_str().unwrap()
        );
        let fixture: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let Some(process) = process_for(&fixture) else { continue };
        replayed += 1;
        let found = run(&id, &fixture, &process, &overrides).await;
        if !found.is_empty() {
            failures.push(format!("{id}\n  {}", found.join("\n  ")));
        }
    }
    assert!(replayed > 0, "no fixture found under {}", root.display());
    assert!(failures.is_empty(), "{} of {replayed} fixtures differ:\n{}", failures.len(), failures.join("\n"));
}

/// Every fixture's rows inserted through the real writers into a scratch database built from the
/// migrations, so each row type is checked against its table. Passes without asserting when
/// ClickHouse is not running.
#[tokio::test]
async fn every_fixture_row_goes_into_its_clickhouse_table() {
    let server = ninjapaws_storage::client_from_env().with_database("default");
    if server.query("SELECT 1").execute().await.is_err() {
        eprintln!("ClickHouse is not running: the round trip is skipped");
        return;
    }
    let database = format!("ninjapaws_roundtrip_{}", std::process::id());
    server.query(&format!("CREATE DATABASE {database}")).execute().await.unwrap();
    let client = ninjapaws_storage::client_from_env().with_database(&database);
    ninjapaws_storage::migrations::apply(&client).await.unwrap();

    let root = fixtures_root();
    let mut expected: std::collections::BTreeMap<&str, u64> = std::collections::BTreeMap::new();
    let sink = Sink::clickhouse(client.clone());
    for dir in std::fs::read_dir(root.join("go")).unwrap() {
        for file in std::fs::read_dir(dir.unwrap().path()).unwrap() {
            let fixture: Value = serde_json::from_slice(&std::fs::read(file.unwrap().path()).unwrap()).unwrap();
            let Some(process) = process_for(&fixture) else { continue };
            let (_, writes) = send(&fixture, &process).await;
            for write in writes {
                *expected.entry(write.table).or_default() += write.rows.len() as u64;
            }
            router_of(&process, sink.clone()).dispatch(request_of(&fixture)).await;
        }
    }
    sink.stop().await;
    server.query(&format!("DROP DATABASE {database}")).execute().await.unwrap();

    // Rows past a table's TTL vanish on the way in, so what is checked is that ClickHouse took
    // every batch, not what it kept.
    let mut wrong = Vec::new();
    for stats in sink.stats() {
        let rows = expected.get(stats.table).copied().unwrap_or_default();
        if stats.failed > 0 || stats.written != rows {
            wrong.push(format!(
                "{}: {rows} rows written, {} taken, {} failed",
                stats.table, stats.written, stats.failed
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

