//! The Kubernetes and container hosts through their routers, as the cluster agent and the node
//! agent send to them: what the golden fixtures do not cover.

mod db;
mod programs;

use std::collections::BTreeMap;
use std::sync::Arc;

use http_body_util::BodyExt;
use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::process_frame;
use ninjapaws_intake::server::Settings;
use ninjapaws_proto::datadog::config::{Client, LatestConfigsRequest};
use ninjapaws_proto::datadog::contimage::{ContainerImage, ContainerImagePayload};
use ninjapaws_proto::datadog::process_agent::{
    Cluster, CollectorCluster, CollectorManifest, CollectorManifestCr, CollectorManifestCrd, Manifest,
};
use prost::Message;
use serde_json::{Value, json};
use tako::router::Router;

const KEY: &str = "00000000000000000000000000000001";

/// What one request answered, and where its rows are: under a tenant of its own.
struct Answer {
    status: u16,
    body: String,
    client: clickhouse::Client,
    tenant: String,
}

impl Answer {
    /// How many rows the request stored in each table.
    async fn tables(&self) -> BTreeMap<String, u64> {
        db::tables(&self.client, &self.tenant).await
    }

    async fn rows(&self, table: &str) -> Vec<Value> {
        db::rows(&self.client, table, &self.tenant).await
    }
}

/// The client of this binary's scratch database. None when ClickHouse is not running.
async fn scratch() -> Option<clickhouse::Client> {
    Some(db::client(&db::scratch().await?))
}

async fn send(
    client: &clickhouse::Client,
    mount: fn(&mut Router),
    method: &str,
    path: &str,
    key: Option<&str>,
    body: Vec<u8>,
) -> Answer {
    let tenant = db::tenant();
    let store = Store::default();
    store.publish([(keys::hash(KEY), Key { id: "k".into(), name: "test".into(), tenant_id: tenant.clone() })]);
    let settings = Settings { ack_unknown: false, max_body_bytes: 1 << 20 };
    let mut request = http::Request::builder().method(method).uri(path);
    if let Some(key) = key {
        request = request.header("dd-api-key", key);
    }
    let router = programs::router_of(mount, client.clone(), Arc::new(store), settings);
    let response = router.dispatch(request.body(body.into()).unwrap()).await;
    let status = response.status().as_u16();
    let body = String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    Answer { status, body, client: client.clone(), tenant }
}

async fn post(client: &clickhouse::Client, mount: fn(&mut Router), path: &str, body: Vec<u8>) -> Answer {
    send(client, mount, "POST", path, Some(KEY), body).await
}

/// The rows a request is expected to have stored, per table.
fn stored<const N: usize>(tables: [(&str, u64); N]) -> BTreeMap<String, u64> {
    tables.into_iter().map(|(table, n)| (table.to_string(), n)).collect()
}

// ---- remote configuration ----

const NOT_SERVED: &str = r#"{"error":"remote configuration not served"}"#;

#[tokio::test]
async fn a_configuration_poll_is_answered_404_so_the_agent_keeps_the_config_it_has() {
    let Some(client) = scratch().await else { return };
    let request = LatestConfigsRequest {
        hostname: "web-01".into(),
        agent_version: "7.60.0".into(),
        has_error: true,
        error: "apply failed".into(),
        products: vec!["APM_SAMPLING".into()],
        active_clients: vec![Client { id: "client-1".into(), is_agent: true, ..Default::default() }],
        ..Default::default()
    };
    let answer = post(&client, programs::remote_config, "/api/v0.1/configurations", request.encode_to_vec()).await;
    assert_eq!((answer.status, answer.body.as_str()), (404, NOT_SERVED));
    assert!(answer.tables().await.is_empty());
}

#[tokio::test]
async fn a_poll_that_is_not_protobuf_or_decodes_to_nothing_gets_the_same_404() {
    let Some(client) = scratch().await else { return };
    for body in [vec![0xff; 4], Vec::new()] {
        let answer = post(&client, programs::remote_config, "/api/v0.1/configurations", body).await;
        assert_eq!((answer.status, answer.body.as_str()), (404, NOT_SERVED));
        assert!(answer.tables().await.is_empty());
    }
}

#[tokio::test]
async fn the_org_and_status_lookups_are_answered_404() {
    let Some(client) = scratch().await else { return };
    for (method, path) in
        [("GET", "/api/v0.1/org"), ("POST", "/api/v0.1/org"), ("GET", "/api/v0.1/status"), ("POST", "/api/v0.1/status")]
    {
        let answer = send(&client, programs::remote_config, method, path, Some(KEY), Vec::new()).await;
        assert_eq!((answer.status, answer.body.as_str()), (404, r#"{"error":"not served"}"#), "{method} {path}");
        assert!(answer.tables().await.is_empty());
    }
}

#[tokio::test]
async fn remote_configuration_needs_an_api_key() {
    let Some(client) = scratch().await else { return };
    assert_eq!(send(&client, programs::remote_config, "GET", "/api/v0.1/org", None, Vec::new()).await.status, 403);
}

// ---- containers ----

#[tokio::test]
async fn skipped_images_are_counted_as_a_metric_of_the_intake_itself_then_the_rest_are_stored() {
    let Some(client) = scratch().await else { return };
    let image = |id: &str, name: &str| ContainerImage { id: id.into(), name: name.into(), ..Default::default() };
    let payload = ContainerImagePayload {
        host: "node-9".into(),
        images: vec![image("img-a", ""), image("", "x"), image("", "y")],
        ..Default::default()
    };
    let answer = post(&client, programs::containers, "/api/v2/contimage", payload.encode_to_vec()).await;

    assert_eq!(answer.status, 202);
    assert_eq!(answer.tables().await, stored([("container_images", 1), ("metrics", 1)]));
    // An event-driven COUNT: no sampling interval to divide by later.
    let points = answer.rows("metrics").await;
    let point = &points[0];
    assert_eq!(point["metric"], "ninjacat.intake.images.skipped");
    assert_eq!((point["metric_type"].as_str(), point["interval"].as_u64()), (Some("COUNT"), Some(0)));
    assert_eq!((point["tenant_id"].as_str(), point["host"].as_str()), (Some(answer.tenant.as_str()), Some("node-9")));
    assert_eq!((point["value"].as_f64(), point["source_type"].as_str()), (Some(2.0), Some("ninjacat")));
    assert_eq!(point["tags"], json!({ "reason": ["no_identity"] }));

    let images = answer.rows("container_images").await;
    assert_eq!(images[0]["image_key"], "img-a");
}

#[tokio::test]
async fn an_inventory_of_only_unidentifiable_images_stores_the_metric_and_no_rows() {
    let Some(client) = scratch().await else { return };
    let payload = ContainerImagePayload {
        images: vec![ContainerImage { name: "x".into(), ..Default::default() }],
        ..Default::default()
    };
    let answer = post(&client, programs::containers, "/api/v2/contimage", payload.encode_to_vec()).await;
    assert_eq!(answer.tables().await, stored([("metrics", 1)]));
}

// ---- kubeops ----

#[tokio::test]
async fn post_orchmanif_stores_a_crd_frames_manifests_with_the_wrappers_tags() {
    let Some(client) = scratch().await else { return };
    let inner = CollectorManifest {
        cluster_name: "prod".into(),
        manifests: vec![Manifest {
            uid: "u1".into(),
            kind: "CustomResourceDefinition".into(),
            content: b"kind: CRD".to_vec(),
            ..Default::default()
        }],
        ..Default::default()
    };
    let wrapper = CollectorManifestCrd { manifest: Some(inner), tags: vec!["source:crd".into()] };
    let frame = process_frame::encode(81, 0, &wrapper);
    let answer = post(&client, programs::kubeops, "/api/v2/orchmanif", frame).await;

    assert_eq!((answer.status, answer.body.as_str()), (202, "{}"));
    assert_eq!(answer.tables().await, stored([("k8s_manifests", 1)]));
    let rows = answer.rows("k8s_manifests").await;
    let row = &rows[0];
    assert_eq!(
        (row["tenant_id"].as_str(), row["cluster_name"].as_str(), row["uid"].as_str()),
        (Some(answer.tenant.as_str()), Some("prod"), Some("u1"))
    );
    assert_eq!(row["tag_sources"], json!({ "wrapper": ["source:crd"] }));
    assert_eq!(row["header_timestamp"], Value::Null);
}

#[tokio::test]
async fn a_cr_frame_with_nothing_inside_it_is_accepted_and_stores_nothing() {
    let Some(client) = scratch().await else { return };
    let frame = process_frame::encode(82, 0, &CollectorManifestCr::default());
    let answer = post(&client, programs::kubeops, "/api/v2/orchmanif", frame).await;
    assert_eq!(answer.status, 202);
    assert!(answer.tables().await.is_empty());
}

#[tokio::test]
async fn post_api_v1_orchestrator_the_legacy_path_stores_a_cluster_frame() {
    let Some(client) = scratch().await else { return };
    let collector = CollectorCluster {
        cluster_name: "prod".into(),
        cluster_id: "cid".into(),
        cluster: Some(Cluster { node_count: 3, ..Default::default() }),
        ..Default::default()
    };
    let frame = process_frame::encode(46, 1700000123, &collector);
    let answer = post(&client, programs::kubeops, "/api/v1/orchestrator", frame).await;

    assert_eq!(answer.status, 202);
    assert_eq!(answer.tables().await, stored([("k8s_cluster", 1)]));
    let rows = answer.rows("k8s_cluster").await;
    let row = &rows[0];
    assert_eq!(
        (row["tenant_id"].as_str(), row["cluster_id"].as_str(), row["node_count"].as_u64()),
        (Some(answer.tenant.as_str()), Some("cid"), Some(3))
    );
    assert_eq!(row["header_timestamp"], 1700000123);
}

#[tokio::test]
async fn one_bad_action_in_a_batch_is_kept_raw_while_the_rest_become_rows() {
    let Some(client) = scratch().await else { return };
    let body = br#"[{"action_id":"a-1"}, {"action_id": 5}, {"action_id":"a-3"}]"#.to_vec();
    let answer = post(&client, programs::kubeops, "/api/v2/kubeactions", body).await;

    assert_eq!(answer.status, 202);
    assert_eq!(answer.tables().await, stored([("k8s_actions", 2), ("raw_payloads", 1)]));
    let raws = answer.rows("raw_payloads").await;
    let raw = &raws[0];
    assert_eq!((raw["intake"].as_str(), raw["reason"].as_str()), (Some("kubeactions"), Some("decode_error")));
    assert_eq!(raw["note"], "[1].action_id: expected a string, got number");
    // The element's own JSON, as it was sent.
    assert_eq!(raw["body"], r#"{"action_id": 5}"#);
    let mut ids: Vec<Value> = answer.rows("k8s_actions").await.into_iter().map(|r| r["action_id"].clone()).collect();
    ids.sort_by_key(|id| id.as_str().unwrap().to_string());
    assert_eq!(ids, ["a-1", "a-3"]);
}

#[tokio::test]
async fn a_kubeactions_body_of_null_or_an_empty_array_stores_nothing() {
    let Some(client) = scratch().await else { return };
    for body in ["null", "[]"] {
        let answer = post(&client, programs::kubeops, "/api/v2/kubeactions", body.as_bytes().to_vec()).await;
        assert_eq!(answer.status, 202);
        assert!(answer.tables().await.is_empty(), "{body}");
    }
}

/// An image payload posted to the lifecycle route: field 3 of EventsPayload arrives
/// length-delimited. Go's library skipped the mistyped field and stored a junk event row; prost
/// refuses the message, and the body is kept whole (overrides.json names the fixture).
#[tokio::test]
async fn a_lifecycle_body_with_a_mistyped_field_is_kept_raw_not_stored_as_an_event() {
    let Some(client) = scratch().await else { return };
    use base64::Engine;
    let path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/go/TestPortProbeContainers__images_on_lifecycle/001.json");
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let body =
        base64::engine::general_purpose::STANDARD.decode(fixture["request"]["body_b64"].as_str().unwrap()).unwrap();
    let answer = post(&client, programs::containers, "/api/v2/contlcycle", body).await;
    assert_eq!(answer.status, 202);
    assert_eq!(answer.tables().await, stored([("raw_payloads", 1)]));
    assert_eq!(answer.rows("raw_payloads").await[0]["reason"], "decode_error");
}
