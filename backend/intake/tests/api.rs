//! What api.<site> stores as the JSON text it was sent: the golden fixtures compare JSON by
//! meaning, so they would not see a value written back with other digits or spacing.

mod db;
mod programs;

use serde_json::Value;

const KEY: &str = "00000000000000000000000000000001";

/// A test's client and its tenant.
struct Scratch {
    client: clickhouse::Client,
    tenant: String,
}

/// Posts a body to the api host with a key of a tenant of the test's own. None when ClickHouse is
/// not running.
async fn post(path: &str, body: &str) -> Option<Scratch> {
    let client = db::scratch_client().await?;
    let tenant = db::tenant();
    let router = programs::router_for(programs::api, &client, KEY, &tenant);
    let request = http::Request::builder()
        .method("POST")
        .uri(path)
        .header("dd-api-key", KEY)
        .header("content-type", "application/json")
        .body(body.to_string().into())
        .unwrap();
    let response = router.dispatch(request).await;
    assert!(response.status().is_success(), "{path}: {}", response.status());
    Some(Scratch { client, tenant })
}

/// The tenant's only row of `table`.
async fn row(found: &Scratch, table: &str) -> Value {
    let mut rows = db::rows(&found.client, table, &found.tenant).await;
    assert_eq!(rows.len(), 1, "{table}");
    rows.remove(0)
}

#[tokio::test]
async fn a_host_envelope_keeps_its_sections_as_sent() {
    let gohai = r#"{\"platform\":{\"os\":\"linux\"},\"filesystem\": [ {\"kb_size\": 1.50} ],\"extra_section\":{ \"n\" : 1.0e3 }}"#;
    let body = format!(
        r#"{{"internalHostname":"h1","gohai":"{gohai}","meta":{{ "hostname" : "h1", "load": 1.50 }},
            "network": null,"systemStats":{{ "cpuCores": 8.0 }},"future": [ 1.50 ]}}"#
    );
    let Some(found) = post("/intake/", &body).await else { return };
    let host = row(&found, "hosts").await;
    assert_eq!(host["meta"], r#"{ "hostname" : "h1", "load": 1.50 }"#);
    assert_eq!(host["network"], "null");
    assert_eq!(host["filesystem"], r#"[ {"kb_size": 1.50} ]"#);
    assert_eq!(host["system_stats"]["cpuCores"], "8.0");
    assert_eq!(host["gohai_extra"]["extra_section"], r#"{ "n" : 1.0e3 }"#);
    assert_eq!(host["intake_extra"]["future"], "[ 1.50 ]");
}

#[tokio::test]
async fn a_metadata_variant_and_the_envelopes_other_keys_are_kept_as_sent() {
    let Some(found) = post(
        "/api/v1/metadata",
        r#"{"hostname":"h1","timestamp":1790151330,"agent_metadata":{ "ratio" : 1.50 },"clustercheck_status": [ 1.0e3 ]}"#,
    )
    .await
    else {
        return;
    };
    let metadata = row(&found, "agent_metadata").await;
    assert_eq!(metadata["payload"], r#"{ "ratio" : 1.50 }"#);
    assert_eq!(metadata["envelope_extra"]["clustercheck_status"], "[ 1.0e3 ]");
}

#[tokio::test]
async fn an_agent_check_list_keeps_its_meta_and_an_unreadable_entry_as_sent() {
    let Some(found) = post(
        "/intake/",
        r#"{"internalHostname":"h1","meta":{ "v": 1.50 },"agent_checks":[["ntp","ntp","",0,""], { "not": 1.50 }]}"#,
    )
    .await
    else {
        return;
    };
    assert_eq!(row(&found, "agent_checks").await["meta"], r#"{ "v": 1.50 }"#);
    assert_eq!(row(&found, "raw_payloads").await["body"], r#"{ "not": 1.50 }"#);
}

#[tokio::test]
async fn a_v1_series_keeps_a_point_it_could_not_read_as_sent() {
    let Some(found) =
        post("/api/v1/series", r#"{"series":[{"metric":"m","points":[[1790151330, 1.50],[ "x" , 1.50 ]]}]}"#).await
    else {
        return;
    };
    // The pair of strings is no pair of numbers: the series is refused whole, as sent.
    assert_eq!(
        row(&found, "raw_payloads").await["body"],
        r#"{"metric":"m","points":[[1790151330, 1.50],[ "x" , 1.50 ]]}"#
    );
}

#[tokio::test]
async fn a_runner_task_update_keeps_its_client_outputs_and_extra_as_sent() {
    let Some(found) = post(
        "/api/v2/on-prem-management-service/workflow-tasks/publish-task-update",
        r#"{"data":{"id":"succeed_task","attributes":{"task_id":"t1","client":{ "v" : 1.50 },"other": [ 1.0e3 ],
            "payload":{"outputs":{ "result" : 1.50 },"more": 2.50 }}}}"#,
    )
    .await
    else {
        return;
    };
    let update = row(&found, "runner_task_updates").await;
    assert_eq!(update["client"], r#"{ "v" : 1.50 }"#);
    assert_eq!(update["outputs"], r#"{ "result" : 1.50 }"#);
    assert_eq!(update["extra"]["other"], "[ 1.0e3 ]");
    assert_eq!(update["extra"]["payload.more"], "2.50");
}

#[tokio::test]
async fn an_action_connection_keeps_its_credentials_as_sent() {
    let Some(found) = post(
        "/api/v2/actions/connections",
        r#"{"data":{"attributes":{"name":"c","integration":{"type":"http","credentials":{ "token" : "s", "n": 1.50 }}}}}"#,
    )
    .await
    else {
        return;
    };
    assert_eq!(row(&found, "action_connections").await["credentials"], r#"{ "token" : "s", "n": 1.50 }"#);
}
