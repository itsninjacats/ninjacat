//! The golden fixtures (`backend/intake/tests/fixtures/go`): requests recorded from the Go
//! server, each with its answer and the rows it stored. Every fixture a ported host serves is
//! replayed through that host's router into a scratch ClickHouse database; its answer and the rows
//! read back from each table are compared field by field with Go's. `fixtures/overrides.json`
//! lists the accepted differences, each with its reason.
//!
//! Equal means equal in meaning: object key order does not matter, a number is a number however
//! it is written, a timestamp may lose digits past 100 ns or past what its column keeps, and a
//! string that is itself JSON is compared as JSON. A table's rows are matched in any order: a
//! table keeps them in its sort order, and which batch carried a row is not stored.

mod db;
mod programs;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock};

use base64::Engine;
use http_body_util::BodyExt;
use ninjapaws_intake::auth;
use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::routers::{flare, rum};
use ninjapaws_intake::server::Settings;
use regex::Regex;
use serde_json::Value;
use tako::conn_info::ConnInfo;
use tako::router::Router;

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A ported host's process as the fixtures need it: its routes, and what its binary puts above
/// them.
struct Process {
    program: fn(&mut Router),
    above: Option<fn(Router) -> Router>,
}

fn process(program: fn(&mut Router)) -> Option<Process> {
    Some(Process { program, above: None })
}

/// The peer of every Go httptest request.
const PEER: &str = "192.0.2.1:1234";

/// browser-intake as its binary serves it, the peer every recorded request comes from being a
/// proxy it trusts, as an operator would name theirs.
fn browser() -> Option<Process> {
    let above: fn(Router) -> Router = |router| {
        let trusted = rum::trusted_proxies("192.0.2.1").unwrap();
        rum::above(router, &[], trusted)
    };
    Some(Process { program: programs::browser, above: Some(above) })
}

/// The Go server's name for each group of routes, as the fixtures recorded which groups served a
/// request, and the router that serves that group here.
fn route_set(name: &str) -> Option<Process> {
    match name {
        "routeAPI" => process(programs::api),
        "routeApp" => process(programs::app),
        "routeTrace" => process(programs::trace),
        "routeTelemetry" => process(programs::telemetry),
        "routeResources" => process(programs::resources),
        "routeLogs" => process(programs::logs),
        "routeProfile" => process(programs::profile),
        "routeDebugger" => process(programs::debugger),
        "routeSourcemap" => process(programs::sourcemap),
        "routeRUM" => browser(),
        "routeProcess" => process(programs::processes),
        "routeKubeops" => process(programs::kubeops),
        "routeContainers" => process(programs::containers),
        "routeConfig" => process(programs::remote_config),
        "routeDBM" => process(programs::dbm),
        "routeNDM" => process(programs::ndm),
        "routeCWS" => process(programs::cws),
        "routeRuntimeSecurity" => process(programs::runtime_security),
        "routeCSPM" => process(programs::cspm),
        "routeSBOM" => process(programs::sbom),
        "routeSDS" => process(programs::sds),
        "routeAgentDiscovery" => process(programs::agent_discovery),
        "routeAgentHealth" => process(programs::agent_health),
        "routeEventManagement" => process(programs::event_management),
        "routeSoftwareInventory" => process(programs::software_inventory),
        "routeSynthetics" => process(programs::synthetics_results),
        "routeDataObs" => process(programs::data_obs),
        "routeCIVisibilityAPI" => process(programs::api),
        "routeCITestCycle" => process(programs::ci_test_cycle),
        "routeCITestCov" => process(programs::ci_test_coverage),
        "routeCIWebhook" => process(programs::ci_webhook),
        "routeSyntheticsAgent" => process(programs::synthetics_poller),
        "routeAIUsage" => process(programs::ai_usage),
        "routeLLMObs" => process(programs::llm_obs),
        "routeInstall" => process(programs::install),
        _ => None,
    }
}

/// The process an intake host name is sent to.
fn host_process(host: &str) -> Option<Process> {
    // The OCI registry, the BTF download and the HEAD probe are anonymous.
    if host == "install.datadoghq.com" || host == "install.datad0g.com" {
        return process(programs::install);
    }
    if host.starts_with("app.") || host.contains("-app.agent.") {
        return process(programs::app);
    }
    if host.starts_with("api.") || host.contains("-flare.agent.") {
        return process(programs::api);
    }
    if host.starts_with("trace.agent.") {
        return process(programs::trace);
    }
    if host.starts_with("instrumentation-telemetry-intake.") {
        return process(programs::telemetry);
    }
    if host.starts_with("resources-intake.") {
        return process(programs::resources);
    }
    if host.starts_with("http-intake.logs.") || host.starts_with("agent-http-intake.logs.") {
        return process(programs::logs);
    }
    if host.starts_with("intake.profile.") {
        return process(programs::profile);
    }
    if host.starts_with("debugger-intake.") {
        return process(programs::debugger);
    }
    if host.starts_with("sourcemap-intake.") {
        return process(programs::sourcemap);
    }
    if host.starts_with("browser-intake.") {
        return browser();
    }
    if host.starts_with("process.") {
        return process(programs::processes);
    }
    if host.starts_with("orchestrator.") || host.starts_with("kubeops-intake.") {
        return process(programs::kubeops);
    }
    if host.starts_with("contlcycle-intake.") || host.starts_with("contimage-intake.") {
        return process(programs::containers);
    }
    if host.starts_with("config.") {
        return process(programs::remote_config);
    }
    let on = |prefix: &str| host.starts_with(prefix);
    if on("dbm-metrics-intake.") {
        return process(programs::dbm);
    }
    if on("ndm-intake.") || on("snmp-traps-intake.") || on("ndmflow-intake.") || on("netpath-intake.") {
        return process(programs::ndm);
    }
    if on("cws-intake.") {
        return process(programs::cws);
    }
    if on("runtime-security-http-intake.logs.") {
        return process(programs::runtime_security);
    }
    if on("cspm-intake.") {
        return process(programs::cspm);
    }
    if on("sbom-intake.") {
        return process(programs::sbom);
    }
    if on("sds-intake.") {
        return process(programs::sds);
    }
    if on("agentdiscovery-intake.") {
        return process(programs::agent_discovery);
    }
    if on("agenthealth-intake.") {
        return process(programs::agent_health);
    }
    if on("event-management-intake.") {
        return process(programs::event_management);
    }
    if on("softinv-intake.") {
        return process(programs::software_inventory);
    }
    if on("http-synthetics.") {
        return process(programs::synthetics_results);
    }
    if on("data-obs-intake.") {
        return process(programs::data_obs);
    }
    if on("intake.synthetics.") {
        return process(programs::synthetics_poller);
    }
    if on("citestcycle-intake.") {
        return process(programs::ci_test_cycle);
    }
    if on("citestcov-intake.") {
        return process(programs::ci_test_coverage);
    }
    if on("webhook-intake.") {
        return process(programs::ci_webhook);
    }
    if on("eudm-intake.") {
        return process(programs::ai_usage);
    }
    if on("llmobs-intake.") {
        return process(programs::llm_obs);
    }
    None
}

/// The process that serves a fixture, or None for one no ported host serves.
fn process_for(fixture: &Value) -> Option<Process> {
    let routes: Vec<&str> = fixture["routes"].as_array()?.iter().filter_map(Value::as_str).collect();
    let request = &fixture["request"];
    let host = request["host"].as_str()?;
    let path = request["url"].as_str()?.split('?').next()?;
    // A Go test that built an engine around a handler of its own (`func1`) names no route set; it
    // is a test of its own instead.
    if routes.iter().any(|name| !name.starts_with("route")) {
        return None;
    }

    // Most Go tests built one engine and sent to "example.com"; the ones that went through the
    // host dispatch carry a real intake host.
    if host != "example.com" {
        return host_process(host);
    }
    if routes.is_empty() {
        // The Go flare tests called its router alone; every host with a key serves it.
        return path.starts_with("/support/flare").then_some(Process { program: flare_only, above: None });
    }
    match routes[..] {
        [name] => route_set(name),
        _ => None,
    }
}

/// The flare routes alone, as every program with a key registers them.
fn flare_only(router: &mut Router) {
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);
}

/// Every fixture file: those recorded by the Go server's own tests (`go/`), and those recorded
/// from the requests in `trace/recorded_requests_test.go.txt` (`trace/`), payloads its tests never
/// sent through HTTP: the idx wire format, real sketches, every msgp error, unknown Data Streams
/// fields.
fn fixture_paths(root: &Path) -> Vec<PathBuf> {
    ["go", "trace"]
        .iter()
        .flat_map(|set| std::fs::read_dir(root.join(set)).unwrap())
        .map(|dir| dir.unwrap().path())
        .filter(|dir| dir.is_dir())
        .flat_map(|dir| std::fs::read_dir(dir).unwrap())
        .map(|f| f.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect()
}

struct Answer {
    status: u16,
    headers: http::HeaderMap,
    body: Vec<u8>,
}

fn router_of(process: &Process, clickhouse: clickhouse::Client) -> Router {
    let settings = Settings { ack_unknown: false, max_body_bytes: 64 * 1024 * 1024 };
    let router = programs::router_of(process.program, clickhouse, test_keys(), settings);
    match process.above {
        Some(above) => above(router),
        None => router,
    }
}

/// The fixture's request. Of two values of a header the later one counts, as the recorded replay set
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
    let mut request: tako::types::Request = builder.body(body.into()).unwrap();
    request.extensions_mut().insert(ConnInfo::tcp(PEER.parse().unwrap()));
    request
}

async fn send(fixture: &Value, process: &Process, clickhouse: &clickhouse::Client) -> Answer {
    let response = router_of(process, clickhouse.clone()).dispatch(request_of(fixture)).await;
    let status = response.status().as_u16();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    Answer { status, headers, body }
}

fn strings(node: &Value) -> Vec<String> {
    node.as_array().map(|items| items.iter().filter_map(|i| i.as_str().map(String::from)).collect()).unwrap_or_default()
}

/// Compiled once: every expected row is compared with many stored ones.
static TIMESTAMP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2})(?:\.(\d+))?Z$").unwrap());

/// A timestamp cut to 100 ns, or None when the text is not one.
fn timestamp_to_100ns(text: &str) -> Option<String> {
    let found = TIMESTAMP.captures(text)?;
    let digits = found.get(2).map_or("", |m| m.as_str());
    Some(format!("{}.{:0<7}", &found[1], &digits[..digits.len().min(7)]))
}

/// Go's time as its column keeps it: cut to the fraction digits the stored text has (none for
/// DateTime, 3 for DateTime64(3), 9 for DateTime64(9)), and a time outside the column's range
/// held at the bound the row's serialiser clamps it to.
fn same_stored_time(expected: &str, stored: &str) -> bool {
    let (Some(e), Some(s)) = (TIMESTAMP.captures(expected), TIMESTAMP.captures(stored)) else {
        return false;
    };
    let digits = s.get(2).map_or(0, |m| m.as_str().len());
    let cut = |found: &regex::Captures| {
        let fraction = found.get(2).map_or("", |m| m.as_str());
        format!("{}.{:0<digits$}", &found[1], &fraction[..fraction.len().min(digits)])
    };
    let (expected, stored) = (cut(&e), cut(&s));
    if expected == stored {
        return true;
    }
    let (earliest, latest) = match digits {
        0 => ("1970-01-01T00:00:00.", "2105-12-31T23:59:59."),
        _ => ("1900-01-01T00:00:00.", "2262-04-11T23:47:16."),
    };
    let earliest = format!("{earliest:0<w$}", w = 20 + digits);
    let latest = format!("{latest:0<w$}", w = 20 + digits);
    (expected < earliest && stored == earliest) || (expected > latest && stored == latest)
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

/// Text as the list of its bytes, as a Vec<u8> serializes.
fn bytes_of(text: &str) -> Vec<Option<u64>> {
    text.bytes().map(|b| Some(u64::from(b))).collect()
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
            _ if same_stored_time(e, a) => {}
            _ => match (json_container(e), json_container(a)) {
                (Some(ej), Some(aj)) => differences(ignored, &format!("{path}~json"), &ej, &aj, found),
                _ => found.push(format!("{path}: Go {}, Rust {}", show(expected), show(actual))),
            },
        },
        (Value::String(_), Value::String(_)) | (Value::Null, Value::Null) => {}
        // Go's recorder wrote a []byte (an Array(UInt8) column) as its text.
        (Value::String(e), Value::Array(a)) if a.iter().map(Value::as_u64).collect::<Vec<_>>() == bytes_of(e) => {}
        (Value::Bool(e), Value::Bool(a)) if e == a => {}
        _ => found.push(format!("{path}: Go {}, Rust {}", show(expected), show(actual))),
    }
}

/// The table an INSERT statement writes to.
fn insert_table(insert: &str) -> String {
    let rest = insert.trim_start().trim_start_matches("INSERT INTO").trim_start();
    rest.split(|c: char| c.is_whitespace() || c == '(').next().unwrap().trim_matches('`').to_string()
}

/// The column names of an INSERT statement's column list. A name the statement quoted (`group`, a
/// keyword) is the same column unquoted.
fn insert_columns(insert: &str) -> Vec<String> {
    let opening = insert.find('(').unwrap();
    let closing = opening + insert[opening..].find(')').unwrap();
    insert[opening + 1..closing].split(',').map(|c| c.trim().trim_matches('`').to_string()).collect()
}

/// The type of every column of every table, by (table, column).
async fn column_types(clickhouse: &clickhouse::Client) -> HashMap<(String, String), String> {
    let sql = "SELECT table, name, type FROM system.columns WHERE database = currentDatabase()";
    let columns = clickhouse.query(sql).fetch_all::<(String, String, String)>().await.unwrap();
    columns.into_iter().map(|(table, name, kind)| ((table, name), kind)).collect()
}

/// How many rows each table holds, the empty ones left out.
async fn tables_with_rows(clickhouse: &clickhouse::Client) -> BTreeMap<String, u64> {
    let sql = "SELECT table, sum(rows) FROM system.parts WHERE database = currentDatabase() AND active GROUP BY table";
    clickhouse.query(sql).fetch_all::<(String, u64)>().await.unwrap().into_iter().collect()
}

/// The rows of `table`, each as the array of `columns`, spelled as Go's recorder wrote its args.
async fn stored_rows(
    clickhouse: &clickhouse::Client,
    types: &HashMap<(String, String), String>,
    table: &str,
    columns: &[String],
) -> Vec<Value> {
    let kinds: Vec<&str> =
        columns.iter().map(|c| types.get(&(table.to_string(), c.clone())).map_or("", String::as_str)).collect();
    let select: Vec<String> = columns
        .iter()
        .zip(&kinds)
        .map(|(c, kind)| match kind.starts_with("JSON") {
            // A JSON column comes back as the object; Go's recorder has its text.
            true => format!("toJSONString(`{c}`)"),
            false => format!("`{c}`"),
        })
        .collect();
    let sql = format!("SELECT {} FROM `{table}`", select.join(", "));
    let mut rows = db::json_lines(clickhouse.query(&sql), "JSONCompactEachRow").await;
    for row in &mut rows {
        for (value, kind) in row.as_array_mut().unwrap().iter_mut().zip(&kinds) {
            if kind.starts_with("Map(") && kind.ends_with("Float64)") {
                float_map_as_go(value);
            }
        }
    }
    rows
}

/// NaN and the infinities of a Map(String, Float64) as ClickHouse writes them ("nan", "inf"),
/// spelled as a row's serde JSON does.
fn float_map_as_go(map: &mut Value) {
    for value in map.as_object_mut().into_iter().flat_map(|m| m.values_mut()) {
        let go = match value.as_str() {
            Some("nan") => "NaN",
            Some("inf") => "+Inf",
            Some("-inf") => "-Inf",
            _ => continue,
        };
        *value = Value::String(go.into());
    }
}

/// Runs the fixture's request; returns the differences from what Go did.
async fn run(
    id: &str,
    fixture: &Value,
    process: &Process,
    overrides: &Value,
    clickhouse: &clickhouse::Client,
    types: &HashMap<(String, String), String>,
) -> Vec<String> {
    // Each fixture starts from empty tables, so what is read back is what its request stored.
    for table in tables_with_rows(clickhouse).await.keys() {
        if table != "schema_migrations" {
            clickhouse.query(&format!("TRUNCATE TABLE `{table}`")).execute().await.unwrap();
        }
    }
    let response = send(fixture, process, clickhouse).await;
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

    // --- what was stored ---

    // Null means the Go test ran without a capturing node: nothing to compare. An override that
    // ignores "/sends" says this intake stores something else on purpose; a test of its own pins
    // what.
    let Some(sends) = fixture["sends"].as_array() else {
        return found;
    };
    if ignored.contains("/sends") {
        return found;
    }
    // Go's rows by table, each with its own path in the fixture, so the overrides apply to it.
    let mut columns_of: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut expected: BTreeMap<String, Vec<(String, &Value)>> = BTreeMap::new();
    for (i, send) in sends.iter().enumerate() {
        let insert = send["insert"].as_str().unwrap();
        let table = insert_table(insert);
        let columns = insert_columns(insert);
        if *columns_of.entry(table.clone()).or_insert_with(|| columns.clone()) != columns {
            found.push(format!("/sends/{i}/insert: Go wrote two column lists into one table"));
        }
        let rows = expected.entry(table).or_default();
        for (j, row) in send["args"].as_array().unwrap().iter().enumerate() {
            rows.push((format!("/sends/{i}/args/{j}"), row));
        }
    }
    let mut stored = tables_with_rows(clickhouse).await;
    stored.retain(|table, _| !db::DERIVED.contains(&table.as_str()));
    let go: BTreeMap<&str, u64> = expected.iter().map(|(t, rows)| (t.as_str(), rows.len() as u64)).collect();
    let rust: BTreeMap<&str, u64> = stored.iter().map(|(t, n)| (t.as_str(), *n)).collect();
    if go != rust {
        found.push(format!("/sends: Go stored {go:?}, Rust stored {rust:?}"));
    }
    for (table, rows) in &expected {
        if !stored.contains_key(table) {
            continue;
        }
        let mut unused: Vec<Option<Value>> =
            stored_rows(clickhouse, types, table, &columns_of[table]).await.into_iter().map(Some).collect();
        let n = unused.len();
        for (position, (path, want)) in rows.iter().enumerate() {
            // The first unused stored row equal to Go's, looked for from its own position on; else
            // the closest one, whose differences are reported.
            let mut best: Option<(usize, Vec<String>)> = None;
            for k in (0..n).map(|k| (k + position) % n) {
                let Some(candidate) = &unused[k] else { continue };
                let mut diffs = Vec::new();
                differences(&ignored, path, want, candidate, &mut diffs);
                let equal = diffs.is_empty();
                if best.as_ref().is_none_or(|(_, b)| diffs.len() < b.len()) {
                    best = Some((k, diffs));
                }
                if equal {
                    break;
                }
            }
            match best {
                Some((k, diffs)) => {
                    unused[k] = None;
                    found.extend(diffs);
                }
                None => found.push(format!("{path}: no stored row of {table} left to match")),
            }
        }
    }
    found
}

#[tokio::test]
async fn every_fixture_of_a_ported_host_is_answered_and_stored_as_it_says() {
    let Some(database) = db::scratch().await else { return };
    let clickhouse = db::client(&database);
    let types = column_types(&clickhouse).await;
    let root = fixtures_root();
    let overrides: Value = serde_json::from_slice(&std::fs::read(root.join("overrides.json")).unwrap()).unwrap();
    let mut paths = fixture_paths(&root);
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
        let Some(process) = process_for(&fixture) else {
            continue;
        };
        replayed += 1;
        let found = run(&id, &fixture, &process, &overrides, &clickhouse, &types).await;
        if !found.is_empty() {
            failures.push(format!("{id}\n  {}", found.join("\n  ")));
        }
    }
    assert!(replayed > 0, "no fixture found under {}", root.display());
    assert!(failures.is_empty(), "{} of {replayed} fixtures differ:\n{}", failures.len(), failures.join("\n"));
}
