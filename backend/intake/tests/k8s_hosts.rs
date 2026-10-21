//! The Kubernetes and container hosts through their routers, as the cluster agent and the node
//! agent send to them: what the golden fixtures do not cover.

use std::sync::Arc;

use http_body_util::BodyExt;
use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::process_frame;
use ninjapaws_intake::routers::{config, containers, kubeops};
use ninjapaws_intake::server::{Intake, Routes, router};
use ninjapaws_proto::datadog::config::{Client, LatestConfigsRequest};
use ninjapaws_proto::datadog::contimage::{ContainerImage, ContainerImagePayload};
use ninjapaws_proto::datadog::process_agent::{
    Cluster, CollectorCluster, CollectorManifest, CollectorManifestCr, CollectorManifestCrd, Manifest,
};
use ninjapaws_storage::{Sink, Write};
use prost::Message;
use serde_json::{Value, json};

const KEY: &str = "00000000000000000000000000000001";
const TENANT: &str = "test";

struct Answer {
    status: u16,
    body: String,
    writes: Vec<Write>,
}

async fn send(mount: fn(&mut Routes), method: &str, path: &str, key: Option<&str>, body: Vec<u8>) -> Answer {
    let store = Store::default();
    store.publish([(keys::hash(KEY), Key { id: "k".into(), name: "test".into(), tenant_id: TENANT.into() })]);
    let (sink, capture) = Sink::capture();
    let intake = Intake { sink, keys: Arc::new(store), ack_unknown: false, max_body_bytes: 1 << 20 };
    let mut request = http::Request::builder().method(method).uri(path);
    if let Some(key) = key {
        request = request.header("dd-api-key", key);
    }
    let response = router(intake, Some(Scheme::Agent), mount).dispatch(request.body(body.into()).unwrap()).await;
    let status = response.status().as_u16();
    let body = String::from_utf8(response.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap();
    Answer { status, body, writes: capture.writes() }
}

async fn post(mount: fn(&mut Routes), path: &str, body: Vec<u8>) -> Answer {
    send(mount, "POST", path, Some(KEY), body).await
}

fn writers(answer: &Answer) -> Vec<&str> {
    answer.writes.iter().map(|w| w.writer).collect()
}

fn rows<'a>(answer: &'a Answer, writer: &str) -> Vec<&'a Value> {
    answer.writes.iter().filter(|w| w.writer == writer).flat_map(|w| &w.rows).collect()
}

// ---- remote configuration ----

const NOT_SERVED: &str = r#"{"error":"remote configuration not served"}"#;

#[tokio::test]
async fn a_configuration_poll_is_answered_404_so_the_agent_keeps_the_config_it_has() {
    let request = LatestConfigsRequest {
        hostname: "web-01".into(),
        agent_version: "7.60.0".into(),
        has_error: true,
        error: "apply failed".into(),
        products: vec!["APM_SAMPLING".into()],
        active_clients: vec![Client { id: "client-1".into(), is_agent: true, ..Default::default() }],
        ..Default::default()
    };
    let answer = post(config::mount, "/api/v0.1/configurations", request.encode_to_vec()).await;
    assert_eq!((answer.status, answer.body.as_str()), (404, NOT_SERVED));
    assert!(answer.writes.is_empty());
}

#[tokio::test]
async fn a_poll_that_is_not_protobuf_or_decodes_to_nothing_gets_the_same_404() {
    for body in [vec![0xff; 4], Vec::new()] {
        let answer = post(config::mount, "/api/v0.1/configurations", body).await;
        assert_eq!((answer.status, answer.body.as_str()), (404, NOT_SERVED));
        assert!(answer.writes.is_empty());
    }
}

#[tokio::test]
async fn the_org_and_status_lookups_are_answered_404() {
    for (method, path) in
        [("GET", "/api/v0.1/org"), ("POST", "/api/v0.1/org"), ("GET", "/api/v0.1/status"), ("POST", "/api/v0.1/status")]
    {
        let answer = send(config::mount, method, path, Some(KEY), Vec::new()).await;
        assert_eq!((answer.status, answer.body.as_str()), (404, r#"{"error":"not served"}"#), "{method} {path}");
        assert!(answer.writes.is_empty());
    }
}

#[tokio::test]
async fn remote_configuration_needs_an_api_key() {
    assert_eq!(send(config::mount, "GET", "/api/v0.1/org", None, Vec::new()).await.status, 403);
}

// ---- containers ----

#[tokio::test]
async fn skipped_images_are_counted_as_a_metric_of_the_intake_itself_then_the_rest_are_stored() {
    let image = |id: &str, name: &str| ContainerImage { id: id.into(), name: name.into(), ..Default::default() };
    let payload = ContainerImagePayload {
        host: "node-9".into(),
        images: vec![image("img-a", ""), image("", "x"), image("", "y")],
        ..Default::default()
    };
    let answer = post(containers::mount, "/api/v2/contimage", payload.encode_to_vec()).await;

    assert_eq!(answer.status, 202);
    assert_eq!(writers(&answer), ["storage_metrics", "storage_container_images"]);
    // An event-driven COUNT: no sampling interval to divide by later.
    let point = rows(&answer, "storage_metrics")[0];
    assert_eq!(point["metric"], "ninjacat.intake.images.skipped");
    assert_eq!((point["metric_type"].as_str(), point["interval"].as_u64()), (Some("COUNT"), Some(0)));
    assert_eq!((point["tenant_id"].as_str(), point["host"].as_str()), (Some(TENANT), Some("node-9")));
    assert_eq!((point["value"].as_f64(), point["source_type"].as_str()), (Some(2.0), Some("ninjacat")));
    assert_eq!(point["tags"], json!({ "reason": ["no_identity"] }));

    let images = rows(&answer, "storage_container_images");
    assert_eq!(images.len(), 1);
    assert_eq!((images[0]["image_key"].as_str(), images[0]["tenant_id"].as_str()), (Some("img-a"), Some(TENANT)));
}

#[tokio::test]
async fn an_inventory_of_only_unidentifiable_images_stores_the_metric_and_no_rows() {
    let payload = ContainerImagePayload {
        images: vec![ContainerImage { name: "x".into(), ..Default::default() }],
        ..Default::default()
    };
    let answer = post(containers::mount, "/api/v2/contimage", payload.encode_to_vec()).await;
    assert_eq!(writers(&answer), ["storage_metrics"]);
}

// ---- kubeops ----

#[tokio::test]
async fn post_orchmanif_stores_a_crd_frames_manifests_with_the_wrappers_tags() {
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
    let answer = post(kubeops::mount, "/api/v2/orchmanif", process_frame::encode(81, 0, &wrapper)).await;

    assert_eq!((answer.status, answer.body.as_str()), (202, "{}"));
    assert_eq!(writers(&answer), ["storage_k8s_manifests"]);
    let row = rows(&answer, "storage_k8s_manifests")[0];
    assert_eq!(
        (row["tenant_id"].as_str(), row["cluster_name"].as_str(), row["uid"].as_str()),
        (Some(TENANT), Some("prod"), Some("u1"))
    );
    assert_eq!(row["tag_sources"], json!({ "wrapper": ["source:crd"] }));
    assert_eq!(row["header_timestamp"], Value::Null);
}

#[tokio::test]
async fn a_cr_frame_with_nothing_inside_it_is_accepted_and_stores_nothing() {
    let answer =
        post(kubeops::mount, "/api/v2/orchmanif", process_frame::encode(82, 0, &CollectorManifestCr::default())).await;
    assert_eq!(answer.status, 202);
    assert!(answer.writes.is_empty());
}

#[tokio::test]
async fn post_api_v1_orchestrator_the_legacy_path_stores_a_cluster_frame() {
    let collector = CollectorCluster {
        cluster_name: "prod".into(),
        cluster_id: "cid".into(),
        cluster: Some(Cluster { node_count: 3, ..Default::default() }),
        ..Default::default()
    };
    let answer = post(kubeops::mount, "/api/v1/orchestrator", process_frame::encode(46, 1700000123, &collector)).await;

    assert_eq!(answer.status, 202);
    assert_eq!(writers(&answer), ["storage_k8s_cluster"]);
    let row = rows(&answer, "storage_k8s_cluster")[0];
    assert_eq!(
        (row["tenant_id"].as_str(), row["cluster_id"].as_str(), row["node_count"].as_u64()),
        (Some(TENANT), Some("cid"), Some(3))
    );
    assert_eq!(row["header_timestamp"], 1700000123);
}

#[tokio::test]
async fn one_bad_action_in_a_batch_is_kept_raw_while_the_rest_become_rows() {
    let body = br#"[{"action_id":"a-1"}, {"action_id": 5}, {"action_id":"a-3"}]"#.to_vec();
    let answer = post(kubeops::mount, "/api/v2/kubeactions", body).await;

    assert_eq!(answer.status, 202);
    assert_eq!(writers(&answer), ["storage_raw_payloads", "storage_k8s_actions"]);
    let raw = rows(&answer, "storage_raw_payloads")[0];
    assert_eq!((raw["intake"].as_str(), raw["reason"].as_str()), (Some("kubeactions"), Some("decode_error")));
    assert_eq!(raw["note"], "[1].action_id: expected a string, got number");
    // The element's own JSON, as it was sent.
    assert_eq!(raw["body"], r#"{"action_id": 5}"#);
    let ids: Vec<&Value> = rows(&answer, "storage_k8s_actions").iter().map(|r| &r["action_id"]).collect();
    assert_eq!(ids, ["a-1", "a-3"]);
}

#[tokio::test]
async fn a_kubeactions_body_of_null_or_an_empty_array_stores_nothing() {
    for body in ["null", "[]"] {
        let answer = post(kubeops::mount, "/api/v2/kubeactions", body.as_bytes().to_vec()).await;
        assert_eq!(answer.status, 202);
        assert!(answer.writes.is_empty(), "{body}");
    }
}

/// An image payload posted to the lifecycle route: field 3 of EventsPayload arrives
/// length-delimited. Go and Google.Protobuf skip the mistyped field and stored a junk event row;
/// prost refuses the message, and the body is kept whole (overrides.json names the fixture).
#[tokio::test]
async fn a_lifecycle_body_with_a_mistyped_field_is_kept_raw_not_stored_as_an_event() {
    use base64::Engine;
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/go/TestPortProbeContainers__images_on_lifecycle/001.json"
    );
    let fixture: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let body =
        base64::engine::general_purpose::STANDARD.decode(fixture["request"]["body_b64"].as_str().unwrap()).unwrap();
    let answer = post(containers::mount, "/api/v2/contlcycle", body).await;
    assert_eq!(answer.status, 202);
    assert_eq!(writers(&answer), vec!["storage_raw_payloads"]);
    assert_eq!(rows(&answer, "storage_raw_payloads")[0]["reason"], "decode_error");
}
