//! The telemetry and resources hosts through their routers, beyond the golden fixtures: what an
//! envelope's keys and headers become, and the note of a kept payload.

mod db;
mod programs;

use serde_json::Value;
use tako::router::Router;

const KEY: &str = "00000000000000000000000000000001";

/// What one request left behind: its status, and the tenant its rows are under.
struct Sent {
    status: u16,
    tenant: String,
}

/// Posts a body with a key of a tenant of its own to the host `mount` serves.
async fn post(
    client: &clickhouse::Client,
    mount: fn(&mut Router),
    path: &str,
    headers: &[(&str, &str)],
    body: &str,
) -> Sent {
    let tenant = db::tenant();
    let router = programs::router_for(mount, client, KEY, &tenant);
    let mut request = http::Request::builder().method("POST").uri(path).header("dd-api-key", KEY);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = router.dispatch(request.body(body.to_string().into()).unwrap()).await;
    Sent { status: response.status().as_u16(), tenant }
}

/// The rows the request stored in `table`.
async fn rows(client: &clickhouse::Client, sent: &Sent, table: &str) -> Vec<Value> {
    db::rows(client, table, &sent.tenant).await
}

async fn telemetry(client: &clickhouse::Client, body: &str) -> Vec<Value> {
    let sent = post(client, programs::telemetry, "/api/v2/apmtelemetry", &[], body).await;
    assert_eq!(sent.status, 202);
    rows(client, &sent, "apm_telemetry").await
}

#[tokio::test]
async fn the_producer_is_named_from_request_type_then_from_the_keys_only_one_producer_sets() {
    let Some(client) = db::scratch_client().await else { return };
    for (body, producer) in [
        (r#"{"request_type":"apm-onboarding-event"}"#, "trace-agent"),
        (r#"{"request_type":"apm-remote-config-event"}"#, "cluster-agent"),
        (r#"{"request_type":"agent-logs"}"#, "agent-telemetry"),
        (r#"{"request_type":"app-heartbeat"}"#, "tracer"),
        (r#"{"request_type":"agent-bsod","event_time":1}"#, "agent-telemetry"),
        (r#"{"request_type":"logs","origin":"installer","runtime_id":"r"}"#, "fleet-installer"),
        (r#"{"request_type":"logs","runtime_id":"r"}"#, "tracer"),
        (r#"{"request_type":"logs"}"#, "unknown"),
    ] {
        let found = telemetry(&client, body).await;
        assert_eq!(found.len(), 1, "{body}");
        assert_eq!(found[0]["producer"], producer, "{body}");
    }
}

#[tokio::test]
async fn the_proxys_headers_are_stored_and_a_malformed_application_block_lands_in_extra() {
    let Some(client) = db::scratch_client().await else { return };
    let sent = post(
        &client,
        programs::telemetry,
        "/api/v2/apmtelemetry",
        &[
            ("Via", "trace-agent 7.60.0"),
            ("DD-Agent-Hostname", "h1"),
            ("DD-Agent-Env", "prod"),
            ("Datadog-Container-ID", "c1"),
            ("X-Datadog-Container-Tags", "a:b"),
        ],
        r#"{"request_type":"app-started","application":[1,2],"future":{"x":1}}"#,
    )
    .await;
    let found = rows(&client, &sent, "apm_telemetry").await;
    assert_eq!(found.len(), 1);
    let row = &found[0];
    assert_eq!(row["via"], "trace-agent 7.60.0");
    assert_eq!(row["dd_agent_hostname"], "h1");
    assert_eq!(row["dd_agent_env"], "prod");
    assert_eq!(row["datadog_container_id"], "c1");
    assert_eq!(row["x_datadog_container_tags"], "a:b");
    assert_eq!(row["extra"], serde_json::json!({ "application": "[1,2]", "future": "{\"x\":1}" }));
}

#[tokio::test]
async fn a_batch_that_is_not_a_list_of_objects_is_kept_only_on_the_requests_row() {
    let Some(client) = db::scratch_client().await else { return };
    let not_a_list = telemetry(&client, r#"{"request_type":"message-batch","payload":{"request_type":"logs"}}"#).await;
    assert_eq!(not_a_list.len(), 1);
    assert_eq!(not_a_list[0]["payload"], r#"{"request_type":"logs"}"#);

    let not_objects =
        telemetry(&client, r#"{"request_type":"message-batch","payload":[{"request_type":"logs"},7]}"#).await;
    assert_eq!(not_objects.len(), 1);
    assert_eq!(not_objects[0]["batch_index"], Value::Null);

    let absent = telemetry(&client, r#"{"request_type":"message-batch"}"#).await;
    assert_eq!(absent.len(), 1);
    assert_eq!(absent[0]["payload"], "");
}

#[tokio::test]
async fn a_null_batch_entry_is_still_an_entry_with_nothing_in_it() {
    let Some(client) = db::scratch_client().await else { return };
    let mut found = telemetry(
        &client,
        r#"{"request_type":"message-batch","runtime_id":"r","payload":[null,{"request_type":"logs","payload":1}]}"#,
    )
    .await;
    // Rows come back in the table's order: the request's own row first, then its entries.
    found.sort_by_key(|row| row["batch_index"].as_i64());
    let column = |name: &str| found.iter().map(|row| row[name].clone()).collect::<Vec<_>>();
    assert_eq!(column("batch_index"), vec![Value::Null, 0.into(), 1.into()]);
    assert_eq!(column("request_type"), vec!["message-batch", "", "logs"]);
    assert_eq!(column("payload"), vec![r#"[null,{"request_type":"logs","payload":1}]"#, "", "1"]);
    assert_eq!(column("parent_request_type"), vec!["", "message-batch", "message-batch"]);
}

/// What is stored as JSON is the text the tracer sent, digits and spaces included.
#[tokio::test]
async fn the_payload_debug_times_and_extra_keep_the_text_as_sent() {
    let Some(client) = db::scratch_client().await else { return };
    let mut found = telemetry(
        &client,
        r#"{"request_type":"message-batch","tracer_time": 1.50 ,"debug":{ "x" : 1.0e3 },"future":[ 1.50 ],
            "payload":[ {"request_type":"logs","payload":{ "n" : 1.50 }} ]}"#,
    )
    .await;
    found.sort_by_key(|row| row["batch_index"].as_i64());
    assert_eq!(found[0]["tracer_time_raw"], "1.50");
    assert_eq!(found[0]["debug"], r#"{ "x" : 1.0e3 }"#);
    assert_eq!(found[0]["extra"], serde_json::json!({ "future": "[ 1.50 ]" }));
    assert_eq!(found[0]["payload"], r#"[ {"request_type":"logs","payload":{ "n" : 1.50 }} ]"#);
    assert_eq!(found[1]["payload"], r#"{ "n" : 1.50 }"#);
}

#[tokio::test]
async fn a_null_body_is_an_empty_envelope_and_still_a_row() {
    let Some(client) = db::scratch_client().await else { return };
    let found = telemetry(&client, "null").await;
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["request_type"], "");
    assert_eq!(found[0]["producer"], "unknown");
}

#[tokio::test]
async fn a_body_that_is_json_but_not_an_object_goes_to_raw_payloads() {
    let Some(client) = db::scratch_client().await else { return };
    let sent = post(&client, programs::telemetry, "/api/v2/apmtelemetry", &[], "[1,2,3]").await;
    assert!(rows(&client, &sent, "apm_telemetry").await.is_empty());
    let raw = rows(&client, &sent, "raw_payloads").await;
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0]["intake"], "apmtelemetry");
    assert_eq!(raw[0]["reason"], "decode_error");
    assert_eq!(raw[0]["note"], "expected an object, got array");
}

#[tokio::test]
async fn genresources_quotes_the_origin_headers_in_the_note() {
    let Some(client) = db::scratch_client().await else { return };
    let sent = post(
        &client,
        programs::resources,
        "/api/v2/genresources",
        &[("DD-EVP-ORIGIN", "a\"b\\c"), ("Content-Type", "application/x-protobuf")],
        "opaque",
    )
    .await;
    assert_eq!(sent.status, 202);
    let raw = rows(&client, &sent, "raw_payloads").await;
    assert_eq!(raw.len(), 1);
    assert_eq!(raw[0]["reason"], "no_schema");
    assert_eq!(
        raw[0]["note"],
        r#"DD-EVP-ORIGIN="a\"b\\c" DD-EVP-ORIGIN-VERSION="" Content-Type="application/x-protobuf""#
    );
}

/// A handler stores under the tenant of the key that admitted the request, and nothing else
/// checks for one: the route's middleware is the only guard. Every route of every intake program,
/// in its `main` and in the tests' copy, therefore names its key's middleware, but the probes and
/// the install host's anonymous registry, which store nothing.
#[test]
fn every_route_that_can_store_asks_for_a_key() {
    const ANONYMOUS: [&str; 6] = [
        "server::health",
        "server::ping",
        "install::handle_version_check",
        "install::handle_manifest",
        "install::handle_blob",
        "install::handle_btf",
    ];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("tests/programs/mod.rs")];
    for entry in std::fs::read_dir(root.join("../api/src/bin")).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        // The query process and the monitor service are directories, and neither they nor the
        // migrations serve an intake.
        if name.starts_with("ninjapaws_") && name.ends_with(".rs") && name != "ninjapaws_migrate.rs" {
            files.push(path);
        }
    }
    let methods = [".get(", ".post(", ".put(", ".patch(", ".delete(", ".route("];
    let mut routes = 0;
    let mut unguarded = Vec::new();
    for file in &files {
        let text = std::fs::read_to_string(file).unwrap();
        let code: String =
            text.lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
        for (at, _) in code.match_indices("router") {
            let statement = code[at + "router".len()..].split(';').next().unwrap_or_default();
            let rest: String = statement.split_whitespace().collect();
            if !methods.iter().any(|method| rest.starts_with(method)) {
                continue;
            }
            routes += 1;
            if !rest.contains(".middleware(auth::") && !ANONYMOUS.iter().any(|handler| rest.contains(handler)) {
                unguarded.push(format!("{}: router{rest}", file.display()));
            }
        }
    }
    assert!(routes > 200, "found only {routes} routes: the registrations are spelled differently now");
    assert!(unguarded.is_empty(), "routes without a key's middleware:\n{}", unguarded.join("\n"));
}
