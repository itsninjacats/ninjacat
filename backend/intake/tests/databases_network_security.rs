//! The database, network device, security and event platform hosts through their routers: what
//! the golden fixtures do not cover, and the payloads recorded from real senders
//! (`backend/intake/tests/fixtures/real`).

use std::path::Path;
use std::sync::Arc;

use http_body_util::BodyExt;
use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::keys::{self, Key, Store};
use ninjapaws_intake::routers::{dbm, evp, ndm, security};
use ninjapaws_intake::server::{Intake, Routes, router};
use ninjapaws_proto::datadog::agentdiscovery::{AgentDiscoveryPayload, AgentDiscoveryPayloadBatch};
use ninjapaws_proto::datadog::sbom::{SbomEntity, SbomPayload};
use ninjapaws_storage::{Sink, Write};
use prost::Message;
use serde_json::Value;

const KEY: &str = "00000000000000000000000000000001";
const TENANT: &str = "test";

struct Answer {
    status: u16,
    body: String,
    writes: Vec<Write>,
}

impl Answer {
    fn writers(&self) -> Vec<&str> {
        self.writes.iter().map(|w| w.writer).collect()
    }

    /// Every row written to the table, in order.
    fn rows(&self, table: &str) -> Vec<&Value> {
        self.writes.iter().filter(|w| w.table == table).flat_map(|w| &w.rows).collect()
    }

    fn raw(&self) -> Vec<&Value> {
        self.rows("raw_payloads")
    }
}

async fn send(mount: fn(&mut Routes), scheme: Scheme, path: &str, headers: &[(&str, &str)], body: Vec<u8>) -> Answer {
    let store = Store::default();
    store.publish([(keys::hash(KEY), Key { id: "test".into(), name: "test".into(), tenant_id: TENANT.into() })]);
    let (sink, capture) = Sink::capture();
    let intake = Intake { sink, keys: Arc::new(store), ack_unknown: false, max_body_bytes: 64 * 1024 * 1024 };
    let router = router(intake, Some(scheme), mount);
    let mut request = http::Request::builder().method("POST").uri(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = router.dispatch(request.body(body.into()).unwrap()).await;
    let status = response.status().as_u16();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    Answer { status, body: String::from_utf8_lossy(&body).into(), writes: capture.writes() }
}

/// A POST with the test key, answered 202 and `{}`.
async fn post(mount: fn(&mut Routes), path: &str, body: impl Into<Vec<u8>>) -> Answer {
    let answer = send(mount, Scheme::Agent, path, &[("dd-api-key", KEY)], body.into()).await;
    assert_eq!((answer.status, answer.body.as_str()), (202, "{}"), "{path}");
    answer
}

fn real(name: &str) -> Vec<u8> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real");
    std::fs::read(root.join(name)).unwrap()
}

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

// --- databases ---

#[tokio::test]
async fn what_a_real_dbm_integration_sends_is_stored_event_for_event_with_no_key_left_undecoded() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/real");
    let tracks = ["databasequery", "dbmactivity", "dbmhealth", "dbmmetadata", "dbmmetrics", "dbmcolumnstatistics"];
    let mut files: Vec<String> =
        std::fs::read_dir(root).unwrap().map(|f| f.unwrap().file_name().to_string_lossy().into_owned()).collect();
    files.sort();
    let mut checked = 0;
    for file in files {
        let Some(track) = tracks.iter().find(|track| file.ends_with(&format!("-{track}.json"))) else { continue };
        let body = real(&file);
        let answer = post(dbm::mount, &format!("/api/v2/{track}"), body.clone()).await;
        assert!(answer.raw().is_empty(), "{file}");
        let rows = answer.rows("dbm_events");
        let sent: Vec<Value> = serde_json::from_slice(&body).unwrap();
        assert_eq!(rows.len(), sent.len(), "{file}");
        for (event, row) in sent.iter().zip(&rows) {
            assert_eq!(text(&row["track"]), *track, "{file}");
            assert_eq!(row["undecoded_keys"], Value::Array(Vec::new()), "{file}");
            // A plan is always recognised as one: its signature is read whenever the check
            // collected a plan at all.
            if let Some(plan) = event.get("db").and_then(|db| db.get("plan")).filter(|plan| plan.is_object()) {
                let expected = plan["signature"].as_str().map(|s| Value::String(s.into())).unwrap_or(Value::Null);
                assert_eq!(row["plan_signature"], expected, "{file}");
            } else if event.get("db").is_some() {
                assert_eq!(row["plan_signature"], Value::Null, "{file}");
            }
        }
        checked += 1;
    }
    assert!(checked >= 30, "only {checked} recorded DBM files found");
}

#[tokio::test]
async fn an_oracle_plan_is_a_list_of_steps_and_they_are_counted() {
    let answer =
        post(dbm::mount, "/api/v2/databasequery", real("oracle-check-agent-7.84.0-oracle-23-databasequery.json")).await;
    let rows = answer.rows("dbm_events");
    let plan = rows.iter().find(|row| row["dbm_type"] == "plan").unwrap();
    assert_eq!(plan["plan_definition_steps"], 2);
    assert!(plan["plan_signature"].is_string());
}

#[tokio::test]
async fn a_postgres_plan_is_explain_output_in_a_string_or_null_when_none_could_be_collected() {
    let answer = post(dbm::mount, "/api/v2/databasequery", real("postgres-check-23.12.0-databasequery.json")).await;
    let mut shown: Vec<(String, bool)> = answer
        .rows("dbm_events")
        .iter()
        .map(|row| (text(&row["dbm_type"]).to_string(), row["plan_signature"].is_string()))
        .collect();
    shown.sort();
    assert_eq!(shown, [("fqt".into(), false), ("plan".into(), false), ("plan".into(), true)]);
}

#[tokio::test]
async fn the_agents_empty_dbm_probe_stores_nothing() {
    for body in ["{}", "[]", ""] {
        assert!(post(dbm::mount, "/api/v2/dbmmetrics", body).await.writes.is_empty(), "{body}");
    }
}

#[tokio::test]
async fn a_bare_dbm_object_is_a_batch_of_one_stored_with_its_text_as_sent() {
    let body = " {\"host\": \"db1\",\n  \"status\": \"ok\"}\n";
    let answer = post(dbm::mount, "/api/v2/dbmhealth", body).await;
    let rows = answer.rows("dbm_events");
    assert_eq!(rows.len(), 1);
    assert_eq!(text(&rows[0]["event"]), body.trim());
    assert_eq!(text(&rows[0]["tenant_id"]), TENANT);
}

// --- network devices ---

#[tokio::test]
async fn what_the_agents_snmp_check_learns_about_a_device_becomes_devices_interfaces_and_addresses() {
    let answer = post(ndm::mount, "/api/v2/ndm", real("agent-7.84.0-ndm.json")).await;
    assert!(answer.raw().is_empty());
    let devices = answer.rows("ndm_devices");
    assert_eq!(devices.len(), 1);
    assert_eq!(
        [text(&devices[0]["name"]), text(&devices[0]["sys_object_id"]), text(&devices[0]["integration"])],
        ["lab-switch-1", "1.3.6.1.4.1.8072.3.2.10", "snmp"]
    );
    assert_eq!(answer.rows("ndm_interfaces").len(), 2);
    assert_eq!(answer.rows("ndm_ip_addresses").len(), 2);

    // What has no table of its own is kept by kind: the exporter NetFlow came from, the device
    // scan's status and OIDs, the check's diagnosis.
    let mut kinds: Vec<&str> = answer.rows("ndm_metadata_objects").iter().map(|o| text(&o["kind"])).collect();
    kinds.sort();
    let mut tally: Vec<(&str, usize)> = Vec::new();
    for kind in kinds {
        match tally.last_mut() {
            Some((last, count)) if *last == kind => *count += 1,
            _ => tally.push((kind, 1)),
        }
    }
    assert_eq!(tally, [("device_oid", 4), ("diagnosis", 1), ("netflow_exporter", 1), ("scan_status", 1)]);
}

#[tokio::test]
async fn traps_arrive_with_the_names_the_agents_own_trap_database_gave_them_or_as_bare_oids() {
    let answer = post(ndm::mount, "/api/v2/ndmtraps", real("agent-7.84.0-ndmtraps.json")).await;
    assert!(answer.raw().is_empty());
    let traps = answer.rows("snmp_traps");
    let mut shown: Vec<[&str; 3]> = traps
        .iter()
        .map(|t| [text(&t["snmp_trap_oid"]), text(&t["snmp_trap_name"]), text(&t["snmp_trap_mib"])])
        .collect();
    shown.sort();
    assert_eq!(
        shown,
        [
            ["1.3.6.1.4.1.99999.0.7", "", ""],
            ["1.3.6.1.6.3.1.1.5.1", "coldStart", "START-MIB"],
            ["1.3.6.1.6.3.1.1.5.3", "linkDown", "IF-MIB"]
        ]
    );
    // The agent resolves a known variable to its name and its enum to a word.
    let link_down = traps.iter().find(|t| t["snmp_trap_name"] == "linkDown").unwrap();
    assert_eq!(link_down["enriched"]["ifOperStatus"], "\"down\"");
    // A vendor trap it has no definition for keeps its variables by OID.
    let unknown = traps.iter().find(|t| t["snmp_trap_name"] == "").unwrap();
    assert_eq!(unknown["var_oids"], serde_json::json!(["1.3.6.1.4.1.99999.1.1", "1.3.6.1.4.1.99999.1.2"]));
    assert_eq!(unknown["var_values"], serde_json::json!(["\"fan tray 2 failed\"", "42"]));
    assert_eq!(unknown["enriched"], serde_json::json!({}));
}

#[tokio::test]
async fn netflow_v5_records_the_agent_aggregated_become_flows() {
    let answer = post(ndm::mount, "/api/v2/ndmflow", real("agent-7.84.0-ndmflow.json")).await;
    assert!(answer.raw().is_empty());
    let flows = answer.rows("netflow_flows");
    assert_eq!(flows.len(), 4);
    assert!(flows.iter().all(|f| f["flow_type"] == "netflow5"));
    let dns = flows.iter().find(|f| f["destination_port"] == "53").unwrap();
    assert_eq!(
        [text(&dns["source_ip"]), text(&dns["destination_ip"]), text(&dns["ip_protocol"])],
        ["10.1.0.7", "8.8.8.8", "UDP"]
    );
}

#[tokio::test]
async fn a_traceroute_the_agent_ran_becomes_one_network_path_with_its_hops() {
    let answer = post(ndm::mount, "/api/v2/netpath", real("agent-7.84.0-netpath.json")).await;
    assert!(answer.raw().is_empty());
    let paths = answer.rows("network_paths");
    assert_eq!(paths.len(), 1);
    let path = paths[0];
    assert_eq!((text(&path["protocol"]), text(&path["destination_hostname"])), ("TCP", "app.ninjacat.lab"));
    assert_eq!(path["destination_port"], 8080);
    // Three traceroute runs, one hop each: the two containers share a network.
    assert_eq!(path["run_ids"].as_array().unwrap().len(), 3);
    assert!(path["hop_ttls"].as_array().unwrap().iter().all(|ttls| ttls.as_array().unwrap().len() == 1));
}

#[tokio::test]
async fn one_value_of_the_wrong_type_refuses_the_whole_batch_of_flows_and_the_body_is_kept_raw() {
    let body = r#"[{"type":"netflow9","bytes":1},{"type":"netflow9","bytes":"many"}]"#;
    let answer = post(ndm::mount, "/api/v2/ndmflow", body).await;
    assert!(answer.rows("netflow_flows").is_empty());
    let raw = answer.raw();
    assert_eq!(raw.len(), 1);
    assert_eq!(
        [text(&raw[0]["intake"]), text(&raw[0]["reason"]), text(&raw[0]["body"])],
        ["ndmflow", "decode_error", body]
    );
}

#[tokio::test]
async fn an_ndm_entry_with_a_value_of_the_wrong_type_is_kept_raw_and_the_entries_beside_it_are_stored() {
    let good = r#"{"namespace":"default","devices":[{"id":"default:10.0.0.1","status":1}]}"#;
    let bad = r#"{"namespace":"default","devices":[{"id":17}]}"#;
    let answer = post(ndm::mount, "/api/v2/ndm", format!("[{good},{bad}]")).await;
    let devices = answer.rows("ndm_devices");
    assert_eq!(devices.len(), 1);
    assert_eq!((text(&devices[0]["device_id"]), text(&devices[0]["status"])), ("default:10.0.0.1", "reachable"));
    let raw = answer.raw();
    assert_eq!(raw.len(), 1);
    assert_eq!((text(&raw[0]["intake"]), text(&raw[0]["reason"])), ("ndm", "decode_error"));
    assert!(text(&raw[0]["note"]).starts_with("entry 1: "));
    assert_eq!(text(&raw[0]["body"]), bad);
}

#[tokio::test]
async fn an_entry_without_a_trap_is_an_unexpected_shape_a_broken_trap_a_decode_error() {
    let answer = post(
        ndm::mount,
        "/api/v2/ndmtraps",
        r#"[{"other":1},{"trap":{"timestamp":"soon"}},{"trap":{"snmpTrapOID":"1.3"}},7]"#,
    )
    .await;
    let traps = answer.rows("snmp_traps");
    assert_eq!(traps.len(), 1);
    assert_eq!(traps[0]["snmp_trap_oid"], "1.3");
    let raw = answer.raw();
    let reasons: Vec<&str> = raw.iter().map(|r| text(&r["reason"])).collect();
    assert_eq!(reasons, ["unexpected_shape", "decode_error", "decode_error"]);
    assert_eq!(text(&raw[0]["note"]), "entry 0: no trap object");
    assert!(text(&raw[1]["note"]).starts_with("entry 1: trap: "));
    assert_eq!(text(&raw[1]["body"]), r#"{"trap":{"timestamp":"soon"}}"#);
    assert!(text(&raw[2]["note"]).starts_with("entry 3: "));
}

/// A batch entry kept raw is the text that was sent, numbers and spacing included.
#[tokio::test]
async fn an_entry_kept_raw_is_the_text_as_sent() {
    let bad = r#"{"namespace" : "x", "n": 1.50, "devices":[{"id":17}]}"#;
    let answer = post(ndm::mount, "/api/v2/ndm", format!("[ {bad} ]")).await;
    assert_eq!(text(&answer.raw()[0]["body"]), bad);

    let bad = r#"{"n": 1.50, "configs": 7}"#;
    let answer = post(ndm::mount, "/api/v2/ndmconfig", format!("[ {bad} ]")).await;
    assert_eq!(text(&answer.raw()[0]["body"]), bad);

    let bad = r#"{"other" : 1E3}"#;
    let answer = post(ndm::mount, "/api/v2/ndmtraps", format!("[ {bad} ]")).await;
    assert_eq!(text(&answer.raw()[0]["body"]), bad);

    let answer = post(evp::mount_synthetics_results, "/api/v2/synthetics", r#"[ {"test":{"id":"t"}} , 1.50 ]"#).await;
    assert_eq!(text(&answer.raw()[0]["body"]), "1.50");
}

#[tokio::test]
async fn an_address_that_is_not_one_refuses_the_batch_of_paths() {
    let answer =
        post(ndm::mount, "/api/v2/netpath", r#"[{"traceroute":{"runs":[{"hops":[{"ip_address":"nowhere"}]}]}}]"#).await;
    assert!(answer.rows("network_paths").is_empty());
    let raw = answer.raw();
    assert_eq!((text(&raw[0]["intake"]), text(&raw[0]["reason"])), ("netpath", "decode_error"));
}

// --- security ---

const BOUNDARY: &str = "security-tests-boundary";

/// A multipart/form-data body of the given form fields.
fn multipart(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, data) in parts {
        body.extend(format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes());
        body.extend(*data);
        body.extend(b"\r\n");
    }
    body.extend(format!("--{BOUNDARY}--\r\n").as_bytes());
    body
}

async fn post_dump(content_type: &str, body: Vec<u8>) -> Answer {
    let mut headers = vec![("dd-api-key", KEY)];
    if !content_type.is_empty() {
        headers.push(("content-type", content_type));
    }
    let answer = send(security::mount_cws, Scheme::Agent, "/api/v2/secdump", &headers, body).await;
    assert_eq!((answer.status, answer.body.as_str()), (202, "{}"));
    answer
}

#[tokio::test]
async fn a_dump_with_a_header_only_is_stored_without_nodes() {
    let content_type = format!("multipart/form-data; boundary={BOUNDARY}");
    let answer = post_dump(&content_type, multipart(&[("event", br#"{"host":"h1"}"#)])).await;
    assert_eq!(answer.writers(), ["storage_cws_activity_dumps"]);
    assert_eq!(answer.rows("cws_activity_dumps")[0]["header_host"], "h1");
}

#[tokio::test]
async fn a_dump_request_with_nothing_to_decode_is_kept_raw() {
    let body = multipart(&[("event", b"[1]"), ("dump", b""), ("other", b"x")]);
    for (content_type, note) in [
        ("application/json".to_string(), "content-type is not multipart/form-data"),
        (String::new(), "content-type is not multipart/form-data"),
        ("multipart/form-data".to_string(), "content-type is not multipart/form-data"),
        (format!("multipart/form-data; boundary={BOUNDARY}"), "neither multipart part decoded"),
    ] {
        let answer = post_dump(&content_type, body.clone()).await;
        assert_eq!(answer.writers(), ["storage_raw_payloads"], "{content_type}");
        let raw = answer.raw();
        assert_eq!(
            [text(&raw[0]["intake"]), text(&raw[0]["reason"]), text(&raw[0]["note"])],
            ["secdump", "decode_error", note]
        );
        assert_eq!(text(&raw[0]["body"]).as_bytes(), body.as_slice());
    }
}

#[tokio::test]
async fn each_track_stores_its_envelopes_under_its_own_name() {
    let body = r#"[{"hostname":"h1","message":"{\"kind\":\"remediation\"}"}]"#;
    for (mount, path, track) in [
        (security::mount_runtime_security as fn(&mut Routes), "/api/v2/secruntime", "secruntime"),
        (security::mount_runtime_security, "/api/v2/secinfo", "secinfo"),
        (security::mount_cspm, "/api/v2/compliance", "compliance"),
    ] {
        let answer = post(mount, path, body).await;
        let rows = answer.rows("security_events");
        assert_eq!(
            [text(&rows[0]["track"]), text(&rows[0]["tenant_id"]), text(&rows[0]["event_kind"])],
            [track, TENANT, "remediation"]
        );
    }
}

#[tokio::test]
async fn a_batch_that_is_not_a_list_of_envelopes_is_kept_raw_under_its_track() {
    for (body, note) in [
        (r#"{"hostname":"h1"}"#, "the body is not a list of envelopes: expected a list, got object"),
        ("\"text\"", "the body is not a list of envelopes: expected a list, got string"),
        (r#"[{"hostname":"h1"}, 7]"#, "the body is not a list of envelopes: entry 1 is not a JSON object"),
    ] {
        let answer = post(security::mount_cspm, "/api/v2/compliance", body).await;
        assert_eq!(answer.writers(), ["storage_raw_payloads"]);
        let raw = answer.raw();
        assert_eq!(
            [text(&raw[0]["intake"]), text(&raw[0]["reason"]), text(&raw[0]["note"])],
            ["compliance", "unexpected_shape", note]
        );
    }
    let answer = post(security::mount_runtime_security, "/api/v2/secinfo", "not json").await;
    let raw = answer.raw();
    assert_eq!((text(&raw[0]["intake"]), text(&raw[0]["reason"])), ("secinfo", "unexpected_shape"));
    assert!(text(&raw[0]["note"]).starts_with("the body is not a list of envelopes: "));
}

#[tokio::test]
async fn an_empty_batch_or_a_probe_stores_nothing() {
    for body in ["null", "[]", "", "{}"] {
        assert!(post(security::mount_runtime_security, "/api/v2/secruntime", body).await.writes.is_empty(), "{body}");
    }
}

/// Go's decoder puts U+FFFD where a string is not text and carries on.
#[tokio::test]
async fn an_envelope_with_text_that_is_not_valid_unicode_keeps_its_row() {
    let body = [
        &b"[{\"hostname\":\"h"[..],
        &[0xff],
        br#"","service":"s\ud800","k\udc00":{"a\ud800":1},"#,
        br#""message":{"title":"t\ud800","k\udc00":2,"agent":{"rule_id":"r\ud800"}}}]"#,
    ]
    .concat();
    let answer = post(security::mount_runtime_security, "/api/v2/secruntime", body).await;
    let rows = answer.rows("security_events");
    let row = rows[0];
    assert_eq!(
        [text(&row["hostname"]), text(&row["service"]), text(&row["title"]), text(&row["rule_id"])],
        ["h\u{fffd}", "s\u{fffd}", "t\u{fffd}", "r\u{fffd}"]
    );
    assert_eq!(row["message_decoded"], 1);
    // Raw JSON kept in a column is the repaired text: half a pair in it would stop the next reader
    // of that column.
    assert_eq!(text(&row["extra"]["k\u{fffd}"]), r#"{"a\ufffd":1}"#);
}

#[tokio::test]
async fn an_sbom_that_is_not_protobuf_or_has_no_entities_is_kept_raw() {
    let answer = post(security::mount_sbom, "/api/v2/sbom", "definitely not protobuf").await;
    let raw = answer.raw();
    assert_eq!(answer.writers(), ["storage_raw_payloads"]);
    assert_eq!((text(&raw[0]["intake"]), text(&raw[0]["reason"])), ("sbom", "decode_error"));
    assert_ne!(text(&raw[0]["note"]), "");

    // Protobuf reads almost any bytes as some message: a payload without entities is taken for
    // another type's.
    let body = SbomPayload { version: 1, host: "h3".into(), ..Default::default() }.encode_to_vec();
    let answer = post(security::mount_sbom, "/api/v2/sbom", body).await;
    let raw = answer.raw();
    assert_eq!((text(&raw[0]["reason"]), text(&raw[0]["note"])), ("unexpected_shape", "decoded to zero entities"));
}

#[tokio::test]
async fn an_entity_without_components_writes_only_the_entity() {
    let body = SbomPayload {
        version: 1,
        host: "h1".into(),
        entities: vec![SbomEntity { id: "e".into(), ..Default::default() }],
        ..Default::default()
    }
    .encode_to_vec();
    assert_eq!(post(security::mount_sbom, "/api/v2/sbom", body).await.writers(), ["storage_sbom_entities"]);
}

fn hex(text: &str) -> Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}

#[tokio::test]
async fn bytes_that_are_not_a_scanner_result_are_kept_raw_the_note_saying_what_was_there() {
    for (body, reason, note) in [
        ("08010802", "unexpected_shape", "not an SdsResultPayload: no resource and no results; 1:varintx2"),
        // A known field in another wire type (2, the timestamp, as bytes): prost refuses the
        // message.
        (
            "0801080212036162631a00",
            "decode_error",
            "protobuf SdsResultPayload: failed to decode Protobuf message: SdsResultPayload.timestamp: invalid wire type: LengthDelimited (expected Varint); 1:varintx2 2:bytes 3:bytes",
        ),
        ("08", "decode_error", ""),
        ("706c61696e2074657874", "decode_error", ""),
    ] {
        let answer = post(security::mount_sds, "/api/v2/sdsresult", hex(body)).await;
        assert_eq!(answer.writers(), ["storage_raw_payloads"], "{body}");
        let raw = answer.raw();
        assert_eq!((text(&raw[0]["intake"]), text(&raw[0]["reason"])), ("sds", reason), "{body}");
        if note.is_empty() {
            assert!(text(&raw[0]["note"]).starts_with("protobuf SdsResultPayload: "), "{body}");
        } else {
            assert_eq!(text(&raw[0]["note"]), note);
        }
    }
    assert!(post(security::mount_sds, "/api/v2/sdsresult", Vec::new()).await.writes.is_empty());
}

/// What the agent's data security check sent after scanning three tables of a Postgres: one with
/// planted e-mail addresses and a card number, one with nothing to find, one that does not exist.
async fn scanned(file: &str) -> Answer {
    let answer = post(security::mount_sds, "/api/v2/sdsresult", real(file)).await;
    assert!(answer.raw().is_empty());
    assert_eq!((answer.rows("sds_scans").len(), answer.rows("sds_results").len()), (1, 1));
    answer
}

#[tokio::test]
async fn a_real_scan_that_found_sensitive_data_says_which_rule_matched_in_which_column() {
    let answer = scanned("agent-7.84.0-sdsresult-matches.bin").await;
    let scan = answer.rows("sds_scans")[0];
    assert_eq!(
        [text(&scan["resource_type"]), text(&scan["resource_name"]), text(&scan["scanning_source"])],
        ["postgres_table", "lab-instance.shop.public.users", "agent"]
    );
    assert_eq!(scan["rule_ids"], serde_json::json!(["email", "visa-card"]));
    assert!(scan["timestamp"].is_string());

    let result = answer.rows("sds_results")[0];
    assert_eq!(
        [
            text(&result["location_kind"]),
            text(&result["database_name"]),
            text(&result["schema_name"]),
            text(&result["table_name"]),
            text(&result["path"])
        ],
        ["postgres_table", "shop", "public", "users", ""]
    );
    assert_eq!(result["scanned_column_names"], serde_json::json!(["email", "name", "note"]));
    assert_eq!(result["scanned_column_types"], serde_json::json!(["text", "varchar", "text"]));
    assert_eq!(result["scanned_row_count"], 3);
    assert_eq!(
        [text(&result["task_id"]), text(&result["sub_task_id"]), text(&result["task_status"])],
        ["task-lab-1", "sub-users", "SUCCESS"]
    );

    let mut matches: Vec<(String, String, String, i64, i64)> = answer
        .rows("sds_matches")
        .iter()
        .map(|m| {
            (
                text(&m["kind"]).into(),
                text(&m["rule_id"]).into(),
                text(&m["column_name"]).into(),
                m["count_matched_rows"].as_i64().unwrap(),
                m["count_matches"].as_i64().unwrap(),
            )
        })
        .collect();
    matches.sort();
    let expected = [
        ("table_match", "email", "email", 2, 2),
        ("table_match", "email", "note", 1, 2),
        ("table_match", "visa-card", "note", 1, 1),
    ]
    .map(|(k, r, c, rows, n)| (k.to_string(), r.to_string(), c.to_string(), rows, n));
    assert_eq!(matches, expected);
}

#[tokio::test]
async fn a_real_scan_that_found_nothing_still_records_the_table_it_scanned() {
    let answer = scanned("agent-7.84.0-sdsresult-clean.bin").await;
    let result = answer.rows("sds_results")[0];
    assert_eq!((text(&result["table_name"]), text(&result["task_status"])), ("orders", "SUCCESS"));
    assert_eq!(result["table_match_count"], 0);
    assert!(answer.rows("sds_matches").is_empty());
}

#[tokio::test]
async fn a_real_scan_that_failed_carries_the_scanners_reason() {
    let answer = scanned("agent-7.84.0-sdsresult-failed.bin").await;
    let result = answer.rows("sds_results")[0];
    assert_eq!((text(&result["table_name"]), text(&result["task_status"])), ("no_such_table", "ERROR"));
    assert!(text(&result["failure_reason"]).contains("relation \"no_such_table\" does not exist"));
    assert!(answer.rows("sds_matches").is_empty());
}

// --- the event platform ---

#[tokio::test]
async fn every_event_platform_route_answers_an_empty_body_with_202_and_stores_nothing() {
    for (mount, path) in [
        (evp::mount_agent_discovery as fn(&mut Routes), "/api/v2/agentdiscovery"),
        (evp::mount_agent_health, "/api/v2/agenthealth"),
        (evp::mount_event_management, "/api/v2/events"),
        (evp::mount_software_inventory, "/api/v2/softinv"),
        (evp::mount_synthetics_results, "/api/v2/synthetics"),
    ] {
        assert!(post(mount, path, Vec::new()).await.writes.is_empty(), "{path}");
    }
    for path in ["/api/v1/lineage", "/api/v2/query-actions"] {
        let answer = send(evp::mount_data_obs, Scheme::Lineage, path, &[("dd-api-key", KEY)], Vec::new()).await;
        assert_eq!(answer.status, 202);
        assert!(answer.writes.is_empty(), "{path}");
    }
}

#[tokio::test]
async fn a_discovery_batch_is_stored_a_body_that_is_not_protobuf_kept_raw_and_an_empty_batch_dropped() {
    let batch = AgentDiscoveryPayloadBatch {
        host_id: "host-1".into(),
        payloads: vec![AgentDiscoveryPayload { integration: "postgres".into(), ..Default::default() }],
    };
    let answer = post(evp::mount_agent_discovery, "/api/v2/agentdiscovery", batch.encode_to_vec()).await;
    let rows = answer.rows("agent_discovery");
    assert_eq!([text(&rows[0]["tenant_id"]), text(&rows[0]["host_id"])], [TENANT, "host-1"]);

    let answer = post(evp::mount_agent_discovery, "/api/v2/agentdiscovery", "definitely not protobuf").await;
    let raw = answer.raw();
    assert_eq!((text(&raw[0]["intake"]), text(&raw[0]["reason"])), ("agentdiscovery", "decode_error"));

    // Protobuf skips unknown fields, so a payload meant for another endpoint decodes into an empty
    // batch: that is logged, not stored.
    let empty = AgentDiscoveryPayloadBatch { host_id: "host-1".into(), ..Default::default() };
    assert!(post(evp::mount_agent_discovery, "/api/v2/agentdiscovery", empty.encode_to_vec()).await.writes.is_empty());
}

#[tokio::test]
async fn a_health_report_is_stored_report_first_then_its_issues() {
    let body = r#"{"schema_version": "1.0", "host": {"hostname": "web-1"},
        "issues": {"b": {"id": "b", "severity": 1, "extra": {"pid": 9007199254740993}}, "a": {"id": "a"}}}"#;
    let answer = post(evp::mount_agent_health, "/api/v2/agenthealth", body).await;
    assert_eq!(answer.writers(), ["storage_agent_health_reports", "storage_agent_health_issues"]);
    let report = answer.rows("agent_health_reports")[0];
    assert_eq!((text(&report["tenant_id"]), text(&report["host"])), (TENANT, "web-1"));
    assert_eq!(report["issue_count"], 2);
    let issues = answer.rows("agent_health_issues");
    let keys: Vec<&str> = issues.iter().map(|i| text(&i["issue_key"])).collect();
    assert_eq!(keys, ["a", "b"]);
    assert!(issues.iter().all(|i| i["report_id"] == report["report_id"] && i["received_at"] == report["received_at"]));
    assert_eq!(text(&issues[1]["severity"]), "ISSUE_SEVERITY_LOW");
    assert_eq!(text(&issues[1]["extra"]), r#"{"pid":9007199254740993}"#);

    let answer = post(evp::mount_agent_health, "/api/v2/agenthealth", r#"{"service": "datadog-agent"}"#).await;
    assert_eq!(answer.writers(), ["storage_agent_health_reports"]);

    let body = r#"{"issues": {"a": {"tags": "x"}}}"#;
    let answer = post(evp::mount_agent_health, "/api/v2/agenthealth", body).await;
    let raw = answer.raw();
    assert_eq!(
        [text(&raw[0]["intake"]), text(&raw[0]["reason"]), text(&raw[0]["body"])],
        ["agenthealth", "decode_error", body]
    );
    assert!(answer.rows("agent_health_reports").is_empty());
}

#[tokio::test]
async fn an_event_is_stored_and_a_body_that_is_not_one_object_kept_raw() {
    let body = r#"{"data":{"attributes":{"title":"a\ud800","attributes":{}}}}"#;
    let answer = post(evp::mount_event_management, "/api/v2/events", body).await;
    assert_eq!(answer.writers(), ["storage_event_management_events"]);
    let rows = answer.rows("event_management_events");
    assert_eq!((text(&rows[0]["tenant_id"]), text(&rows[0]["title"])), (TENANT, "a\u{fffd}"));

    for body in [r#"[{"data": {}}]"#, "\"text\" ", r#"{"data": "#] {
        let answer = post(evp::mount_event_management, "/api/v2/events", body).await;
        let raw = answer.raw();
        assert_eq!(
            [text(&raw[0]["intake"]), text(&raw[0]["reason"]), text(&raw[0]["note"]), text(&raw[0]["body"])],
            ["event-management", "decode_error", "body is not a JSON object", body]
        );
    }

    let answer = post(evp::mount_event_management, "/api/v2/events", r#"{"data": {"type": "event"}}"#).await;
    let raw = answer.raw();
    assert_eq!(
        (text(&raw[0]["reason"]), text(&raw[0]["note"])),
        ("unexpected_shape", "data.attributes is missing or not an object")
    );
}

#[tokio::test]
async fn synthetics_results_are_stored_and_a_bad_item_kept_raw() {
    let body = r#"[{"test": {"id": "t-1"}}, 42, {"test": {"id": "t-2"}}]"#;
    let answer = post(evp::mount_synthetics_results, "/api/v2/synthetics", body).await;
    assert_eq!(answer.writers(), ["storage_raw_payloads", "storage_synthetics_results"]);
    let rows = answer.rows("synthetics_results");
    let ids: Vec<&str> = rows.iter().map(|r| text(&r["test_id"])).collect();
    assert_eq!(ids, ["t-1", "t-2"]);
    assert_eq!(rows[0]["received_at"], rows[1]["received_at"]);
    let raw = answer.raw();
    assert_eq!(
        [text(&raw[0]["intake"]), text(&raw[0]["reason"]), text(&raw[0]["note"]), text(&raw[0]["body"])],
        ["synthetics", "decode_error", "item is not a JSON object", "42"]
    );
}

#[tokio::test]
async fn a_lineage_event_sent_with_a_bearer_key_is_stored_with_the_querys_api_version_and_the_via_header() {
    let bearer = format!("Bearer {KEY}");
    let headers = [("authorization", bearer.as_str()), ("via", "trace-agent 7.83.0")];
    let body = br#"{"eventType": "COMPLETE", "run": {"runId": "run-1"}}"#.to_vec();
    let answer =
        send(evp::mount_data_obs, Scheme::Lineage, "/api/v1/lineage?api-version=2", &headers, body.clone()).await;
    assert_eq!(answer.status, 202);
    assert_eq!(answer.writers(), ["storage_openlineage_events"]);
    let row = answer.rows("openlineage_events")[0];
    assert_eq!(
        [text(&row["tenant_id"]), text(&row["run_id"]), text(&row["api_version"]), text(&row["via"])],
        [TENANT, "run-1", "2", "trace-agent 7.83.0"]
    );

    let refused = send(evp::mount_data_obs, Scheme::Lineage, "/api/v1/lineage", &[], body).await;
    assert_eq!(refused.status, 403);
    assert!(refused.writes.is_empty());
}

#[tokio::test]
async fn a_lineage_batch_stores_every_event_and_keeps_an_item_that_is_not_an_object_raw() {
    let body = br#"[{"eventType": "START"}, "text", {"eventType": "COMPLETE"}]"#.to_vec();
    let answer = send(evp::mount_data_obs, Scheme::Lineage, "/api/v1/lineage", &[("dd-api-key", KEY)], body).await;
    let types: Vec<&str> = answer.rows("openlineage_events").iter().map(|r| text(&r["event_type"])).collect();
    assert_eq!(types, ["START", "COMPLETE"]);
    let raw = answer.raw();
    assert_eq!(
        [text(&raw[0]["intake"]), text(&raw[0]["reason"]), text(&raw[0]["body"])],
        ["lineage", "decode_error", "\"text\""]
    );
}

#[tokio::test]
async fn a_batched_body_that_is_not_json_is_kept_raw_and_one_of_null_holds_no_items() {
    for (mount, scheme, path, intake) in [
        (evp::mount_software_inventory as fn(&mut Routes), Scheme::Agent, "/api/v2/softinv", "softinv"),
        (evp::mount_synthetics_results, Scheme::Agent, "/api/v2/synthetics", "synthetics"),
        (evp::mount_data_obs, Scheme::Lineage, "/api/v1/lineage", "lineage"),
        (evp::mount_data_obs, Scheme::Lineage, "/api/v2/query-actions", "query-actions"),
    ] {
        let body = br#"[{"a": 1}, "#.to_vec();
        let answer = send(mount, scheme, path, &[("dd-api-key", KEY)], body).await;
        assert_eq!(answer.writers(), ["storage_raw_payloads"], "{path}");
        let raw = answer.raw();
        assert_eq!(
            [text(&raw[0]["intake"]), text(&raw[0]["reason"]), text(&raw[0]["note"])],
            [intake, "unexpected_shape", "body is not a JSON array or object"]
        );
        let answer = send(mount, scheme, path, &[("dd-api-key", KEY)], b"null".to_vec()).await;
        assert!(answer.writes.is_empty(), "{path}");
    }
}

#[tokio::test]
async fn a_query_action_entry_is_stored_whole_with_its_sorted_keys_and_the_senders_origin_headers() {
    let headers = [("dd-api-key", KEY), ("dd-evp-origin", "python-integration"), ("dd-evp-origin-version", "1.2.3")];
    let body = br#"[{"b": 9007199254740993, "a": {"z": 15, "y": null}}, {"only": "<tag> & more"}, null]"#.to_vec();
    let answer = send(evp::mount_data_obs, Scheme::Lineage, "/api/v2/query-actions", &headers, body).await;
    let rows = answer.rows("query_action_results");
    assert_eq!(rows.len(), 3);
    assert_eq!(text(&rows[0]["result"]), r#"{"a":{"y":null,"z":15},"b":9007199254740993}"#);
    assert_eq!(rows[0]["keys"], serde_json::json!(["a", "b"]));
    assert_eq!(text(&rows[1]["result"]), r#"{"only":"<tag> & more"}"#);
    // A null entry is stored as null with no keys.
    assert_eq!((text(&rows[2]["result"]), &rows[2]["keys"]), ("null", &serde_json::json!([])));
    for row in rows {
        assert_eq!(
            [text(&row["tenant_id"]), text(&row["dd_evp_origin"]), text(&row["dd_evp_origin_version"])],
            [TENANT, "python-integration", "1.2.3"]
        );
    }

    // A bare object is a batch of one, and a repeated key keeps its last value.
    let answer = send(
        evp::mount_data_obs,
        Scheme::Lineage,
        "/api/v2/query-actions",
        &[("dd-api-key", KEY)],
        br#"{"a": 1, "a": 2}"#.to_vec(),
    )
    .await;
    let rows = answer.rows("query_action_results");
    assert_eq!((text(&rows[0]["result"]), text(&rows[0]["dd_evp_origin"])), (r#"{"a":2}"#, ""));
}
