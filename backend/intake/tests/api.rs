//! What api.<site> stores as the JSON text it was sent: the golden fixtures compare JSON by
//! meaning, so they would not see a value written back with other digits or spacing.

mod programs;

use std::sync::Arc;

use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::server::Intake;
use ninjapaws_storage::Sink;
use serde_json::Value;

const KEY: &str = "00000000000000000000000000000001";

/// The rows each writer got from one request.
async fn post(path: &str, body: &str) -> Vec<(&'static str, Value)> {
    let store = Store::default();
    store.publish([(keys::hash(KEY), Key { id: "k".into(), name: "test".into(), tenant_id: "test".into() })]);
    let (sink, capture) = Sink::capture();
    let intake = Intake {
        sink,
        keys: Arc::new(store),
        ack_unknown: false,
        max_body_bytes: 1 << 20,
        stopping: Default::default(),
        tasks: Default::default(),
    };
    let request = http::Request::builder()
        .method("POST")
        .uri(path)
        .header("dd-api-key", KEY)
        .header("content-type", "application/json")
        .body(body.to_string().into())
        .unwrap();
    let response = programs::router_of(programs::api, intake).dispatch(request).await;
    assert!(response.status().is_success(), "{path}: {}", response.status());
    capture.writes().into_iter().flat_map(|w| w.rows.into_iter().map(move |row| (w.writer, row))).collect()
}

fn row<'a>(rows: &'a [(&str, Value)], writer: &str) -> &'a Value {
    &rows.iter().find(|(w, _)| *w == writer).unwrap_or_else(|| panic!("no {writer} row")).1
}

#[tokio::test]
async fn a_host_envelope_keeps_its_sections_as_sent() {
    let gohai = r#"{\"platform\":{\"os\":\"linux\"},\"filesystem\": [ {\"kb_size\": 1.50} ],\"extra_section\":{ \"n\" : 1.0e3 }}"#;
    let body = format!(
        r#"{{"internalHostname":"h1","gohai":"{gohai}","meta":{{ "hostname" : "h1", "load": 1.50 }},
            "network": null,"systemStats":{{ "cpuCores": 8.0 }},"future": [ 1.50 ]}}"#
    );
    let rows = post("/intake/", &body).await;
    let host = row(&rows, "storage_hosts");
    assert_eq!(host["meta"], r#"{ "hostname" : "h1", "load": 1.50 }"#);
    assert_eq!(host["network"], "null");
    assert_eq!(host["filesystem"], r#"[ {"kb_size": 1.50} ]"#);
    assert_eq!(host["system_stats"]["cpuCores"], "8.0");
    assert_eq!(host["gohai_extra"]["extra_section"], r#"{ "n" : 1.0e3 }"#);
    assert_eq!(host["intake_extra"]["future"], "[ 1.50 ]");
}

#[tokio::test]
async fn a_metadata_variant_and_the_envelopes_other_keys_are_kept_as_sent() {
    let rows = post(
        "/api/v1/metadata",
        r#"{"hostname":"h1","timestamp":1790151330,"agent_metadata":{ "ratio" : 1.50 },"clustercheck_status": [ 1.0e3 ]}"#,
    )
    .await;
    let metadata = row(&rows, "storage_agent_metadata");
    assert_eq!(metadata["payload"], r#"{ "ratio" : 1.50 }"#);
    assert_eq!(metadata["envelope_extra"]["clustercheck_status"], "[ 1.0e3 ]");
}

#[tokio::test]
async fn an_agent_check_list_keeps_its_meta_and_an_unreadable_entry_as_sent() {
    let rows = post(
        "/intake/",
        r#"{"internalHostname":"h1","meta":{ "v": 1.50 },"agent_checks":[["ntp","ntp","",0,""], { "not": 1.50 }]}"#,
    )
    .await;
    assert_eq!(row(&rows, "storage_agent_checks")["meta"], r#"{ "v": 1.50 }"#);
    assert_eq!(row(&rows, "storage_raw_payloads")["body"], r#"{ "not": 1.50 }"#);
}

#[tokio::test]
async fn a_v1_series_keeps_a_point_it_could_not_read_as_sent() {
    let rows =
        post("/api/v1/series", r#"{"series":[{"metric":"m","points":[[1790151330, 1.50],[ "x" , 1.50 ]]}]}"#).await;
    // The pair of strings is no pair of numbers: the series is refused whole, as sent.
    assert_eq!(
        row(&rows, "storage_raw_payloads")["body"],
        r#"{"metric":"m","points":[[1790151330, 1.50],[ "x" , 1.50 ]]}"#
    );
}

#[tokio::test]
async fn a_runner_task_update_keeps_its_client_outputs_and_extra_as_sent() {
    let rows = post(
        "/api/v2/on-prem-management-service/workflow-tasks/publish-task-update",
        r#"{"data":{"id":"succeed_task","attributes":{"task_id":"t1","client":{ "v" : 1.50 },"other": [ 1.0e3 ],
            "payload":{"outputs":{ "result" : 1.50 },"more": 2.50 }}}}"#,
    )
    .await;
    let update = row(&rows, "storage_runner_task_updates");
    assert_eq!(update["client"], r#"{ "v" : 1.50 }"#);
    assert_eq!(update["outputs"], r#"{ "result" : 1.50 }"#);
    assert_eq!(update["extra"]["other"], "[ 1.0e3 ]");
    assert_eq!(update["extra"]["payload.more"], "2.50");
}

#[tokio::test]
async fn an_action_connection_keeps_its_credentials_as_sent() {
    let rows = post(
        "/api/v2/actions/connections",
        r#"{"data":{"attributes":{"name":"c","integration":{"type":"http","credentials":{ "token" : "s", "n": 1.50 }}}}}"#,
    )
    .await;
    assert_eq!(row(&rows, "storage_action_connections")["credentials"], r#"{ "token" : "s", "n": 1.50 }"#);
}
