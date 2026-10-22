//! process.<site> through its router, as the process-agent sends to it: what the golden fixtures
//! do not cover.

mod db;
mod programs;

use std::sync::Arc;

use base64::Engine;
use http_body_util::BodyExt;
use ninjapaws_intake::keys::Store;
use ninjapaws_intake::process_frame;
use ninjapaws_intake::raw;
use ninjapaws_intake::routers::process::REPLY_FRAME;
use ninjapaws_intake::server::{self, Body, Ctx, accepted};
use ninjapaws_proto::datadog::process_agent::{
    Addr, CollectorConnections, CollectorPod, CollectorProc, CollectorProcDiscovery, Connection, HttpAggregations,
    HttpMethod, HttpStats, Process, http_stats,
};
use prost::Message;
use serde_json::Value;
use tako::extractors::state::State;
use tako::router::Router;

const KEY: &str = "00000000000000000000000000000001";

struct Answer {
    status: u16,
    content_type: String,
    body: Vec<u8>,
}

/// The scratch database's client and a tenant of the test's own.
async fn begin() -> Option<(clickhouse::Client, String)> {
    Some((db::scratch_client().await?, db::tenant()))
}

/// Posts a frame as the test agent would.
async fn post(client: &clickhouse::Client, tenant: &str, path: &str, body: Vec<u8>) -> Answer {
    let request = http::Request::builder()
        .method("POST")
        .uri(path)
        .header("dd-api-key", KEY)
        .header("x-dd-processagentversion", "7.58.2")
        .body(body.into())
        .unwrap();
    let router = programs::router_for(programs::processes, client, KEY, tenant);
    let response = router.dispatch(request).await;
    let status = response.status().as_u16();
    let content_type =
        response.headers().get("content-type").map(|v| v.to_str().unwrap().to_string()).unwrap_or_default();
    let body = response.into_body().collect().await.unwrap().to_bytes().to_vec();
    Answer { status, content_type, body }
}

/// The tables the tenant has rows in, and how many.
async fn stored(client: &clickhouse::Client, tenant: &str) -> Vec<(String, u64)> {
    db::tables(client, tenant).await.into_iter().collect()
}

/// The tenant's one row of `table`.
async fn single(client: &clickhouse::Client, tenant: &str, table: &str) -> Value {
    let mut found = db::rows(client, table, tenant).await;
    assert_eq!(found.len(), 1, "{table}: {found:?}");
    found.remove(0)
}

/// The column's text is `expected`, by JSON meaning.
fn same_json(expected: &str, actual: &Value) {
    let parse = |text: &str| serde_json::from_str::<Value>(text).unwrap();
    assert_eq!(parse(expected), parse(actual.as_str().unwrap()), "{actual}");
}

fn bytes(base64: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(base64).unwrap()
}

/// A Blob column as JSON: its text when it is UTF-8, else `{"$b64": …}`.
fn blob(value: &Value) -> Vec<u8> {
    match value {
        Value::String(text) => text.clone().into_bytes(),
        other => bytes(other["$b64"].as_str().unwrap()),
    }
}

fn frame(name: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/frames").join(name);
    std::fs::read(path).unwrap()
}

fn proc_frame() -> Vec<u8> {
    let proc = CollectorProc {
        host_name: "host-a".into(),
        processes: vec![Process { pid: 1, ..Default::default() }],
        ..Default::default()
    };
    process_frame::encode(12, 0, &proc)
}

#[tokio::test]
async fn every_route_answers_with_the_reply_frame_as_protobuf() {
    let Some((client, tenant)) = begin().await else { return };
    let answer = post(&client, &tenant, "/api/v1/collector", proc_frame()).await;
    assert_eq!((answer.status, answer.content_type.as_str()), (200, "application/x-protobuf"));
    assert_eq!(answer.body, *REPLY_FRAME);
    assert_eq!(stored(&client, &tenant).await, [("process_snapshots".to_string(), 1), ("processes".to_string(), 1)]);
}

/// The stock agent image (7.83.2) compresses with Zstandard 0.5, encoding 2.
#[tokio::test]
async fn a_legacy_zstd_frame_from_a_real_agent_stores_its_processes() {
    let Some((client, tenant)) = begin().await else { return };
    let answer = post(&client, &tenant, "/api/v1/collector", frame("frame_agent_7_83_2.bin")).await;
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, *REPLY_FRAME);
    let snapshot = single(&client, &tenant, "process_snapshots").await;
    assert_eq!(
        (snapshot["host_name"].as_str(), snapshot["header_encoding"].as_u64()),
        (Some("nc-verify-agent-host"), Some(2))
    );
    let processes = db::rows(&client, "processes", &tenant).await;
    assert!(!processes.is_empty());
    assert!(db::rows(&client, "raw_payloads", &tenant).await.is_empty());
}

#[tokio::test]
async fn a_frame_whose_body_is_not_its_types_protobuf_is_kept_raw() {
    let Some((client, tenant)) = begin().await else { return };
    // Type 12 with a body that ends inside a field.
    let data = [process_frame::encode(12, 0, &CollectorProc::default()), vec![0x12, 0x7F, 0x41]].concat();
    post(&client, &tenant, "/api/v1/collector", data).await;
    let raw = single(&client, &tenant, "raw_payloads").await;
    assert_eq!(raw["reason"], "decode_error");
    assert!(raw["note"].as_str().unwrap().starts_with("collector: "));
}

/// tenant_id leads every table's ORDER BY, so a payload kept under no tenant is one no query finds.
#[tokio::test]
async fn without_a_tenant_a_payload_is_not_kept_and_the_sender_still_gets_its_answer() {
    let Some(client) = db::scratch_client().await else { return };
    // No route stores without a key; this one, made for the test, would.
    let unguarded = |router: &mut Router| {
        router.post("/unguarded", |ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body| async move {
            raw::store(&clickhouse, &ctx, "process", "decode_error", "test", &body).await.unwrap();
            accepted()
        });
    };
    let settings = server::Settings { ack_unknown: false, max_body_bytes: 1 << 20 };
    let router = programs::router_of(unguarded, client.clone(), Arc::new(Store::default()), settings);
    let request = http::Request::builder().method("POST").uri("/unguarded").body(proc_frame().into()).unwrap();
    assert_eq!(router.dispatch(request).await.status(), 202);
    assert!(db::rows(&client, "raw_payloads", "").await.is_empty());
}

#[tokio::test]
async fn every_route_keeps_a_frame_of_a_type_it_has_no_table_for() {
    let Some((client, _)) = begin().await else { return };
    let host = || "host-a".to_string();
    let cases = [
        (
            "/api/v1/container",
            53,
            process_frame::encode(53, 0, &CollectorProcDiscovery { host_name: host(), ..Default::default() }),
            "CollectorProcDiscovery",
        ),
        (
            "/api/v1/connections",
            12,
            process_frame::encode(12, 0, &CollectorProc { host_name: host(), ..Default::default() }),
            "CollectorProc",
        ),
        (
            "/api/v1/discovery",
            22,
            process_frame::encode(22, 0, &CollectorConnections { host_name: host(), ..Default::default() }),
            "CollectorConnections",
        ),
        // A Kubernetes resource: the same frame, meant for another intake.
        (
            "/api/v1/collector",
            41,
            process_frame::encode(41, 0, &CollectorPod { host_name: host(), ..Default::default() }),
            "CollectorPod",
        ),
    ];
    for (path, message_type, data, name) in cases {
        let tenant = db::tenant();
        let answer = post(&client, &tenant, path, data).await;
        assert_eq!(answer.status, 200);
        let raw = single(&client, &tenant, "raw_payloads").await;
        assert_eq!(raw["reason"], "unexpected_shape");
        assert_eq!(raw["note"], format!("{path} decoded to {name} (frame type {message_type})"));
        assert_eq!(stored(&client, &tenant).await, [("raw_payloads".to_string(), 1)]);
    }
}

// Frames encoded by agent-payload v5.0.207, and beside them the sub-messages in proto3's JSON
// mapping, as the protobuf library writes them.
const CONNECTIONS_FRAME: &str = "AwAWAAAAAAAAAAAAAAAAABIGaG9zdC1hGoYBCAGSAiUIAhIhCAEQ////////////ARgDIgQIChACIgQIAxABIgQIABAFkgINCP///////////wESANICMwgEEi8KBggcEgIIAgolCAESIQgBEP///////////wEYAyIECAoQAiIECAMQASIECAAQBdICBAgFEgDaAgwIERIICgYIARICGAlCCQoFY2lkLTISAEIeCgVjaWQtMRIVCgVjaWQtMRoD+/8BIgN0OjEoBDAJcl8BAgACMwEAAQADCDEwLjAuMC4xAgAMCDEwLjAuMC4yAQAIMTAuMC4wLjMBGAtleGFtcGxlLmNvbQtleGFtcGxlLm9yZxppbnRlcm5hbC5zdmMuY2x1c3Rlci5sb2NhbKoBDQoJY29ubnRyYWNrEgCqARMKBnRyYWNlchIJCAEQARjSCSAF+gEMCgoKCHN1Ym5ldC1h+gEA+gEVEhMKETAyOjQyOmFjOjExOjAwOjAykgITCghzdWJuZXQtYRABGAIiA3g6eZICAJoCBggBKAE4AcICCgoGaG9zdC1iEgDCAjYKBmhvc3QtYRIsCAUQBxoGaG9zdC1hMghlbnY6cHJvZDIDYTxiOAhAgICAgIAgSANQgOLPqgbyAikKBzEuMi4zLjQSHgoHMS4yLjMuNBIDYXdzGgl1cy1lYXN0LTEiA2E6Yg==";
const PROC_FRAME: &str = "AwAMAAAAAAAAAAAAAAAAABIGaG9zdC1hGk8QARosCAUQBxoGaG9zdC1hMghlbnY6cHJvZDIDYTxiOAhAgICAgIAgSANQgOLPqgbSARwyFAoSChAvdmFyL2xvZy9hcHAubG9nMgIKADIAUgYSAWO6AQA=";
const DNS_V1: &str = "AQIAAjMBAAEAAwgxMC4wLjAuMQIADAgxMC4wLjAuMgEACDEwLjAuMC4zARgLZXhhbXBsZS5jb20LZXhhbXBsZS5vcmcaaW50ZXJuYWwuc3ZjLmNsdXN0ZXIubG9jYWw=";

#[tokio::test]
async fn the_whole_sub_messages_of_a_connections_payload_are_protobufs_own_json() {
    let Some((client, tenant)) = begin().await else { return };
    post(&client, &tenant, "/api/v1/connections", bytes(CONNECTIONS_FRAME)).await;
    let payload = single(&client, &tenant, "connections_payloads").await;

    same_json(
        r#"{"cid-1":{"id":"cid-1","byteKey":"+/8B","tags":["t:1"],"tagIndex":4,"tagsModified":"9"},"cid-2":{}}"#,
        &payload["resolved_resources"],
    );
    same_json(
        r#"[{"alias":"subnet-a","tagIndex":1,"tagsModified":"2","tags":["x:y"]},{}]"#,
        &payload["route_metadata"],
    );
    same_json(
        r#"{"npmEnabled":true,"csmEnabled":true,"discoveryServiceMapEnabled":true}"#,
        &payload["agent_configuration"],
    );
    same_json(
        r#"{"host-a":{"id":"5","orgId":7,"name":"host-a","allTags":["env:prod","a<b"],"numCpus":8,"totalMemory":"1099511627776","tagIndex":3,"tagsModified":"1700000000"},"host-b":{}}"#,
        &payload["resolved_hosts_by_name"],
    );
    same_json(
        r#"{"1.2.3.4":{"ip":"1.2.3.4","cloudProvider":"aws","region":"us-east-1","tags":["a:b"]}}"#,
        &payload["resolved_public_ips"],
    );
    // Enums by name, and a result at its default left out like any field.
    same_json(
        r#"{"conntrack":{},
            "tracer":{"runtimeCompilationEnabled":true,"runtimeCompilationResult":"CompilationSuccess","runtimeCompilationDuration":"1234","kernelHeaderFetchResult":"DownloadSuccess"}}"#,
        &payload["compilation_telemetry"],
    );
    same_json(
        r#"[{"subnet":{"alias":"subnet-a"}},{},{"interface":{"HardwareAddr":"02:42:ac:11:00:02"}}]"#,
        &payload["routes"],
    );
    assert_eq!(payload["dns_names"], serde_json::json!(["example.com", "example.org", "internal.svc.cluster.local"]));
    assert_eq!(blob(&payload["encoded_dns"]), bytes(DNS_V1));

    let connection = single(&client, &tenant, "connections").await;
    assert_eq!(payload["payload_id"], connection["payload_id"]);

    // 18446744073709551615 is the largest uint64: a 64-bit integer is a string in protobuf's
    // JSON, so it cannot pass through a float.
    same_json(
        r#"{"-1":{},"2":{"dnsTimeouts":1,"dnsSuccessLatencySum":"18446744073709551615","dnsFailureLatencySum":"3","dnsCountByRcode":{"0":5,"10":2,"3":1}}}"#,
        &connection["dns_stats_by_domain"],
    );
    assert!(connection["dns_stats_by_domain"].as_str().unwrap().contains("18446744073709551615"));
    same_json(
        r#"{"4":{"dnsStatsByQueryType":{"1":{"dnsTimeouts":1,"dnsSuccessLatencySum":"18446744073709551615","dnsFailureLatencySum":"3","dnsCountByRcode":{"0":5,"10":2,"3":1}},"28":{"dnsTimeouts":2}}},"5":{}}"#,
        &connection["dns_stats_by_domain_by_query_type"],
    );
    same_json(
        r#"{"17":{"dnsStatsByQueryType":{"1":{"dnsFailureLatencySum":"9"}}}}"#,
        &connection["dns_stats_by_domain_offset_by_query_type"],
    );
}

#[tokio::test]
async fn a_connections_payload_with_nothing_in_it_stores_empty_strings_not_empty_json() {
    let Some((client, tenant)) = begin().await else { return };
    let conns = CollectorConnections { host_name: "host-a".into(), ..Default::default() };
    post(&client, &tenant, "/api/v1/connections", process_frame::encode(22, 0, &conns)).await;
    let payload = single(&client, &tenant, "connections_payloads").await;
    for column in [
        "resolved_resources",
        "route_metadata",
        "agent_configuration",
        "resolved_hosts_by_name",
        "resolved_public_ips",
        "compilation_telemetry",
        "routes",
    ] {
        assert_eq!(payload[column], "", "{column}");
    }
    assert_eq!(payload["conn_telemetry"], serde_json::json!({}));
    assert_eq!(payload["host_tags"], serde_json::json!({}));
    assert_eq!(payload["dns_names"], serde_json::json!([]));
    // The payload row is written even when there is no connection to go with it.
    assert_eq!(
        stored(&client, &tenant).await,
        [("connections_payloads".to_string(), 1), ("process_snapshots".to_string(), 1)]
    );
}

#[tokio::test]
async fn the_whole_sub_messages_of_a_process_are_protobufs_own_json() {
    let Some((client, tenant)) = begin().await else { return };
    post(&client, &tenant, "/api/v1/collector", bytes(PROC_FRAME)).await;
    let row = single(&client, &tenant, "processes").await;
    same_json(
        r#"{"id":"5","orgId":7,"name":"host-a","allTags":["env:prod","a<b"],"numCpus":8,"totalMemory":"1099511627776","tagIndex":3,"tagsModified":"1700000000"}"#,
        &row["process_host"],
    );
    // A log resource with a path, one without, and a resource of a kind this build does not know.
    same_json(r#"[{"logs":{"path":"/var/log/app.log"}},{"logs":{}},{}]"#, &row["service_resources"]);

    // A host that was sent but empty is an empty object, not "".
    let container = single(&client, &tenant, "containers").await;
    same_json("{}", &container["host_info"]);
}

#[tokio::test]
async fn what_system_probe_saw_inside_a_connection_becomes_stat_rows_that_point_back_at_it() {
    let Some((client, tenant)) = begin().await else { return };
    let mut endpoint = HttpStats { path: "/orders".into(), method: HttpMethod::Get as i32, ..Default::default() };
    endpoint
        .stats_by_status_code
        .insert(200, http_stats::Data { count: 1, first_latency_sample: 5e6, ..Default::default() });
    let aggregations = HttpAggregations { endpoint_aggregations: vec![endpoint] };
    let conns = CollectorConnections {
        host_name: "host-a".into(),
        connections: vec![Connection {
            pid: 7,
            laddr: Some(Addr { ip: "10.0.0.1".into(), port: 5000, ..Default::default() }),
            raddr: Some(Addr { ip: "10.0.0.2".into(), port: 80, ..Default::default() }),
            http_aggregations: aggregations.encode_to_vec(),
            // Not a DatabaseAggregations: logged, and the connection is stored all the same.
            database_aggregations: vec![0xff, 0xff, 0xff],
            ..Default::default()
        }],
        ..Default::default()
    };
    post(&client, &tenant, "/api/v1/connections", process_frame::encode(22, 0, &conns)).await;

    let connection = single(&client, &tenant, "connections").await;
    let stat = single(&client, &tenant, "connection_http_stats").await;
    assert_eq!(
        (&stat["path"], &stat["status_code"], &stat["count"]),
        (&Value::from("/orders"), &Value::from(200), &Value::from(1))
    );
    for column in ["payload_id", "timestamp", "host", "pid", "laddr_ip", "laddr_port", "raddr_ip", "raddr_port"] {
        assert_eq!(stat[column], connection[column], "{column}");
    }
    // The connection's database aggregations were not readable: no row of theirs.
    assert_eq!(
        stored(&client, &tenant).await,
        [
            ("connection_http_stats".to_string(), 1),
            ("connections".to_string(), 1),
            ("connections_payloads".to_string(), 1),
            ("process_snapshots".to_string(), 1),
        ]
    );
}

#[tokio::test]
async fn a_dns_buffer_that_cannot_be_read_through_still_stores_the_payload() {
    let Some((client, tenant)) = begin().await else { return };
    let conns = CollectorConnections {
        host_name: "host-a".into(),
        encoded_dns: vec![1, 0, 0, 0, 100, 0],
        connections: vec![Connection { pid: 1, ..Default::default() }],
        ..Default::default()
    };
    post(&client, &tenant, "/api/v1/connections", process_frame::encode(22, 0, &conns)).await;
    let payload = single(&client, &tenant, "connections_payloads").await;
    assert_eq!(payload["dns_names"], serde_json::json!([]));
    assert_eq!(blob(&payload["encoded_dns"]).len(), 6);
    single(&client, &tenant, "connections").await;
}
