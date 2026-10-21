//! Six single-purpose hosts of the event platform.
//!
//!   agentdiscovery-intake.<site>     POST /api/v2/agentdiscovery   protobuf
//!   agenthealth-intake.<site>        POST /api/v2/agenthealth      JSON
//!   event-management-intake.<site>   POST /api/v2/events           JSON
//!   softinv-intake.<site>            POST /api/v2/softinv          JSON
//!   http-synthetics.<site>           POST /api/v2/synthetics       JSON
//!   data-obs-intake.<site>           POST /api/v1/lineage          JSON
//!                                    POST /api/v2/query-actions    JSON
//!
//!   agent config: none for the event platform tracks (DD_SITE only); agenthealth is a plain HTTP
//!   client under `dd_url`, lineage is the trace-agent's reverse proxy under
//!   `ol_proxy_config.dd_url`.
//!
//! How the event platform frames a body decides what a handler expects: protobuf and stream tracks
//! carry ONE message per request (agentdiscovery, events); the other JSON tracks are batched into
//! an array (softinv, synthetics, query-actions). The agent's `diagnose` sends an EMPTY body down
//! every track, so an empty body is a probe, not a fault.
//!
//! Only agentdiscovery and agenthealth have published types. The rest is read by its documented
//! field names, and every other key is kept as JSON.
//!
//! A body that does not decode, or a batch item that is not an object, goes to raw_payloads; a
//! body that became rows never does.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use ninjapaws_proto::datadog::agentdiscovery::{AgentDiscoveryConfigFilePayloadFormat, AgentDiscoveryPayloadBatch};
use ninjapaws_proto::datadog::healthplatform::{HealthReport, Issue, IssueSeverity, IssueState};
use ninjapaws_storage::Table;
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::evp::{
    AgentDiscovery, AgentHealthIssue, AgentHealthReport, EventManagementEvent, HostSoftware, OpenLineageEvent,
    QueryActionResult, SyntheticsResult,
};
use prost::Message;
use serde_json::Value;
use tako::types::Response;
use uuid::Uuid;

use super::api::merge_extra;
use crate::json;
use crate::server::{Ctx, Routes, accepted};
use crate::wire::{enum_name, tags, text, time};

/// agentdiscovery-intake.<site>
pub fn mount_agent_discovery(routes: &mut Routes) {
    routes.post("/api/v2/agentdiscovery", handle_agent_discovery);
}

/// agenthealth-intake.<site>
pub fn mount_agent_health(routes: &mut Routes) {
    routes.post("/api/v2/agenthealth", handle_agent_health);
}

/// event-management-intake.<site>
pub fn mount_event_management(routes: &mut Routes) {
    routes.post("/api/v2/events", handle_event_management);
}

/// softinv-intake.<site>
pub fn mount_software_inventory(routes: &mut Routes) {
    routes.post("/api/v2/softinv", handle_software_inventory);
}

/// http-synthetics.<site>: where the results come back.
pub fn mount_synthetics_results(routes: &mut Routes) {
    routes.post("/api/v2/synthetics", handle_synthetics);
}

/// data-obs-intake.<site>
pub fn mount_data_obs(routes: &mut Routes) {
    routes.post("/api/v1/lineage", handle_open_lineage);
    routes.post("/api/v2/query-actions", handle_query_actions);
}

// The JSON tracks are read generically, with the readers of `json` that note nothing: a value is
// an `Option<&Value>`, None when the key is absent or null.

fn names(list: &[&'static str]) -> BTreeSet<&'static str> {
    list.iter().copied().collect()
}

/// An object, or null, which stands for an object with no members.
fn is_object_or_null(value: &Value) -> bool {
    value.is_object() || value.is_null()
}

fn object_of(value: Option<&Value>) -> Option<&Value> {
    value.filter(|v| v.is_object())
}

/// A body as one JSON object (or null): the first value, whatever follows it.
pub fn parse_object(body: &[u8]) -> Option<Value> {
    json::try_parse_first(body).ok().filter(is_object_or_null)
}

/// A batched track: each item becomes rows or is kept raw, so one bad item never takes the good
/// ones with it.
fn handle_batch<T: Table>(
    ctx: &Ctx,
    body: &[u8],
    intake: &str,
    to_rows: impl Fn(DateTime<Utc>, &Value) -> Vec<T>,
) -> Response {
    if !body.is_empty() {
        match json::try_parse_list(body) {
            Err(_) => ctx.raw(intake, "unexpected_shape", "body is not a JSON array or object", body),
            Ok(items) => {
                let now = Utc::now();
                let text = json::raw::text(body);
                let mut rows = Vec::new();
                for (item, item_text) in items.iter().zip(json::raw::list(&text)) {
                    if is_object_or_null(item) {
                        rows.extend(to_rows(now, item));
                    } else {
                        ctx.raw(intake, "decode_error", "item is not a JSON object", item_text.as_bytes());
                    }
                }
                ctx.write(rows);
            }
        }
    }
    accepted()
}

/// One row per payload.
pub fn agent_discovery_rows(
    tenant: &str,
    received_at: DateTime<Utc>,
    batch: &AgentDiscoveryPayloadBatch,
) -> Vec<AgentDiscovery> {
    batch
        .payloads
        .iter()
        .map(|payload| AgentDiscovery {
            tenant_id: tenant.into(),
            received_at,
            host_id: batch.host_id.clone(),
            integration: payload.integration.clone(),
            runtime: payload.runtime.clone(),
            runtime_id: payload.runtime_id.clone(),
            ingestion_timestamp: time::of_timestamp(payload.ingestion_timestamp.as_ref()),
            config_paths: payload.config_files.iter().map(|file| file.path.clone()).collect(),
            config_contents: payload.config_files.iter().map(|file| Blob(file.content.clone())).collect(),
            config_truncated: payload.config_files.iter().map(|file| text::flag(file.truncated)).collect(),
            config_formats: payload
                .config_files
                .iter()
                .map(|file| enum_name(file.payload_format, AgentDiscoveryConfigFilePayloadFormat::as_str_name))
                .collect(),
            // Values too, not only names: see the migration for why.
            env_var_names: payload.env_vars.iter().map(|var| var.name.clone()).collect(),
            env_var_values: payload.env_vars.iter().map(|var| var.value.clone()).collect(),
        })
        .collect()
}

/// agentdiscovery-intake: which integration config files and env vars each agent runtime sees.
/// One AgentDiscoveryPayloadBatch per request.
pub async fn handle_agent_discovery(ctx: Ctx, body: Bytes) -> Response {
    if !body.is_empty() {
        match AgentDiscoveryPayloadBatch::decode(&body[..]) {
            Err(e) => ctx.raw("agentdiscovery", "decode_error", &e.to_string(), &body),
            // Protobuf skips fields it does not know, so a payload meant for another endpoint
            // decodes into an empty batch. Not a decode error, so it is not kept raw.
            Ok(batch) if batch.payloads.is_empty() => {
                tracing::warn!("[agentdiscovery] decoded to zero payloads ({} bytes) — wrong payload type?", body.len())
            }
            Ok(batch) => ctx.write(agent_discovery_rows(ctx.tenant(), Utc::now(), &batch)),
        }
    }
    accepted()
}

/// A HealthReport as the agent sends it: the protobuf message written as JSON by Go's
/// encoding/json, so keys are snake_case and enums are numbers. Protobuf's JSON parser reads both.
///
/// An issue's extra is a protobuf Struct, whose numbers are float64s. The parser refuses an integer
/// a float64 cannot hold exactly instead of rounding it, which would refuse the whole report over
/// one id; so every number under extra is made a float first. The extra column is taken from the
/// body as sent (health_issues_raw), where the digits survive.
pub fn decode_health_report(body: &[u8]) -> Result<HealthReport, String> {
    let mut report = json::try_parse(body)?;
    if let Some(Value::Object(issues)) = report.get_mut("issues") {
        for extra in issues.values_mut().filter_map(|issue| issue.get_mut("extra")) {
            as_floats(extra);
        }
    }
    serde_json::from_value(report).map_err(|e| e.to_string())
}

fn as_floats(value: &mut Value) {
    match value {
        Value::Number(number) => {
            if let Some(float) = number.as_f64().and_then(serde_json::Number::from_f64) {
                *number = float;
            }
        }
        Value::Array(items) => items.iter_mut().for_each(as_floats),
        Value::Object(members) => members.values_mut().for_each(as_floats),
        _ => {}
    }
}

/// The body's `issues` object, read generically. Protobuf's Struct holds every number as a
/// float64, so an issue's extra is taken from here instead, where a number keeps its digits.
pub fn health_issues_raw(body: &[u8]) -> Option<Value> {
    object_of(json::child("issues", parse_object(body).as_ref())).cloned()
}

/// Extra as protobuf decoded it: every number has been through a float64.
fn struct_json(extra: Option<&pbjson_types::Struct>) -> String {
    extra.and_then(|e| serde_json::to_value(e).ok()).map(|value| json::compact_sorted(&value)).unwrap_or_default()
}

fn issue_extra(extra: Option<&pbjson_types::Struct>, raw_issue: Option<&Value>) -> String {
    match raw_issue.and_then(|issue| issue.get("extra")) {
        Some(Value::Null) => String::new(),
        Some(sent) => json::compact_sorted(sent),
        None => struct_json(extra),
    }
}

fn health_issue_row(
    tenant: &str,
    received_at: DateTime<Utc>,
    report_id: Uuid,
    key: &str,
    issue: &Issue,
    raw_issue: Option<&Value>,
) -> AgentHealthIssue {
    // An absent sub-message reads as an empty one, except where its absence is itself the
    // information.
    let remediation = issue.remediation.clone().unwrap_or_default();
    let script = remediation.script.as_ref();
    let lifecycle = issue.persisted_issue.as_ref();

    AgentHealthIssue {
        tenant_id: tenant.into(),
        received_at,
        report_id,
        issue_key: key.into(),
        id: issue.id.clone(),
        issue_name: issue.issue_name.clone(),
        title: issue.title.clone(),
        description: issue.description.clone(),
        category: issue.category.clone(),
        location: issue.location.clone(),
        severity: enum_name(issue.severity, IssueSeverity::as_str_name),
        detected_at: issue.detected_at.clone(),
        detected_at_parsed: time::try_rfc3339(&issue.detected_at),
        source: issue.source.clone(),
        extra: issue_extra(issue.extra.as_ref(), raw_issue),
        remediation_summary: remediation.summary.clone(),
        remediation_step_order: remediation.steps.iter().map(|step| step.order).collect(),
        remediation_step_text: remediation.steps.iter().map(|step| step.text.clone()).collect(),
        script_language: script.map(|s| s.language.clone()).unwrap_or_default(),
        script_language_version: script.map(|s| s.language_version.clone()).unwrap_or_default(),
        script_filename: script.map(|s| s.filename.clone()).unwrap_or_default(),
        script_requires_root: script.map(|s| text::flag(s.requires_root)),
        script_content: script.map(|s| s.content.clone()).unwrap_or_default(),
        tags: tags::to_multi_map(&issue.tags),
        persisted_state: lifecycle.map(|l| enum_name(l.state, IssueState::as_str_name)).unwrap_or_default(),
        first_seen: lifecycle.map(|l| l.first_seen.clone()).unwrap_or_default(),
        last_seen: lifecycle.map(|l| l.last_seen.clone()).unwrap_or_default(),
        resolved_at: lifecycle.and_then(|l| l.resolved_at.clone()),
        issue_type: issue.issue_type.clone(),
    }
}

/// The report's row and one row per issue, in the order of the issue keys so that two ingests of
/// the same report give the same rows. `raw_issues` is what health_issues_raw found, used only for
/// extra.
pub fn health_report_rows(
    tenant: &str,
    received_at: DateTime<Utc>,
    report_id: Uuid,
    report: &HealthReport,
    raw_issues: Option<&Value>,
) -> (AgentHealthReport, Vec<AgentHealthIssue>) {
    let host = report.host.clone().unwrap_or_default();
    let report_row = AgentHealthReport {
        tenant_id: tenant.into(),
        received_at,
        report_id,
        schema_version: report.schema_version.clone(),
        event_type: report.event_type.clone(),
        emitted_at: report.emitted_at.clone(),
        emitted_at_parsed: time::try_rfc3339(&report.emitted_at),
        service: report.service.clone(),
        host: host.hostname,
        agent_version: host.agent_version,
        par_ids: host.par_ids,
        issue_count: report.issues.len() as u32,
    };
    let mut keys: Vec<&String> = report.issues.keys().collect();
    keys.sort();
    let issue_rows = keys
        .into_iter()
        .map(|key| {
            let raw_issue = object_of(json::child(key, raw_issues));
            health_issue_row(tenant, received_at, report_id, key, &report.issues[key], raw_issue)
        })
        .collect();
    (report_row, issue_rows)
}

/// agenthealth-intake: issues the agent found with its own setup, one HealthReport per request.
/// Not an event platform track.
pub async fn handle_agent_health(ctx: Ctx, body: Bytes) -> Response {
    if !body.is_empty() {
        match decode_health_report(&body) {
            Err(e) => ctx.raw("agenthealth", "decode_error", &e, &body),
            Ok(report) => {
                // The wire has no report id; this one joins the two tables.
                let raw_issues = health_issues_raw(&body);
                let (report_row, issue_rows) =
                    health_report_rows(ctx.tenant(), Utc::now(), Uuid::new_v4(), &report, raw_issues.as_ref());
                ctx.write(vec![report_row]);
                ctx.write(issue_rows);
            }
        }
    }
    accepted()
}

/// The label in raw_payloads. Not "events": /api/v1/events on api.<site> already has that one, and
/// the two tracks are unrelated.
const EVENT_MANAGEMENT_INTAKE: &str = "event-management";

static EVENT_ATTRIBUTE_KEYS: LazyLock<BTreeSet<&str>> = LazyLock::new(|| {
    names(&[
        "host",
        "title",
        "category",
        "integration_id",
        "message",
        "timestamp",
        "tags",
        "aggregation_key",
        "attributes",
        "system-notable-events",
    ])
});
static EVENT_DATA_KEYS: LazyLock<BTreeSet<&str>> = LazyLock::new(|| names(&["type", "attributes"]));
static EVENT_ENVELOPE_KEYS: LazyLock<BTreeSet<&str>> = LazyLock::new(|| names(&["data"]));

/// One JSON:API-like envelope:
///
///   {"data": {"type": "event", "attributes": {host, title, category, integration_id, message,
///             timestamp, tags, aggregation_key, attributes: {...},
///             "system-notable-events": {event_type}}}}
///
/// The inner `attributes` is each producer's own: {status, priority, custom} for notable events,
/// {changed_resource, author, ...} for change events.
///
/// None when data.attributes is missing or not an object: the one thing the envelope must have.
pub fn event_management_row(
    tenant: &str,
    received_at: DateTime<Utc>,
    envelope: &Value,
) -> Option<EventManagementEvent> {
    let top = Some(envelope);
    let data = object_of(json::child("data", top));
    let attrs = Some(object_of(json::child("attributes", data))?);
    let timestamp = json::text(json::child("timestamp", attrs));

    // Undeclared keys of all three levels land in one map: data's own (a JSON:API "id") and the
    // envelope's ("meta") under a prefix.
    let extra = json::other_members(&EVENT_ATTRIBUTE_KEYS, json::compact_sorted, attrs);
    let extra = merge_extra(extra, "data.", json::other_members(&EVENT_DATA_KEYS, json::compact_sorted, data));
    let extra = merge_extra(extra, "top.", json::other_members(&EVENT_ENVELOPE_KEYS, json::compact_sorted, top));

    let inner = match object_of(json::child("attributes", attrs)) {
        Some(object) => json::compact_sorted(object),
        None => "{}".into(),
    };

    Some(EventManagementEvent {
        tenant_id: tenant.into(),
        received_at,
        data_type: json::text(json::child("type", data)),
        host: json::text(json::child("host", attrs)),
        title: json::text(json::child("title", attrs)),
        category: json::text(json::child("category", attrs)),
        integration_id: json::text(json::child("integration_id", attrs)),
        message: json::text(json::child("message", attrs)),
        timestamp_parsed: time::try_rfc3339(&timestamp),
        timestamp,
        tags: tags::to_multi_map(json::text_list(json::child("tags", attrs))),
        aggregation_key: json::text(json::child("aggregation_key", attrs)),
        notable_event_type: json::text(json::child("event_type", json::child("system-notable-events", attrs))),
        attributes: inner,
        outer_extra: extra,
    })
}

/// event-management-intake: notable events, logon duration, anomaly notifications. A stream track,
/// so one envelope per request.
pub async fn handle_event_management(ctx: Ctx, body: Bytes) -> Response {
    if !body.is_empty() {
        match parse_object(&body) {
            None => ctx.raw(EVENT_MANAGEMENT_INTAKE, "decode_error", "body is not a JSON object", &body),
            Some(envelope) => match event_management_row(ctx.tenant(), Utc::now(), &envelope) {
                None => ctx.raw(
                    EVENT_MANAGEMENT_INTAKE,
                    "unexpected_shape",
                    "data.attributes is missing or not an object",
                    &body,
                ),
                Some(row) => ctx.write(vec![row]),
            },
        }
    }
    accepted()
}

static SOFTWARE_ENTRY_KEYS: LazyLock<BTreeSet<&str>> = LazyLock::new(|| {
    names(&[
        "software_type",
        "name",
        "version",
        "publisher",
        "deployment_status",
        "deployment_time",
        "product_code",
        "is_64_bit",
        "install_paths",
    ])
});
static SOFTWARE_PAYLOAD_KEYS: LazyLock<BTreeSet<&str>> = LazyLock::new(|| names(&["hostname", "host_software"]));
static HOST_SOFTWARE_KEYS: LazyLock<BTreeSet<&str>> = LazyLock::new(|| names(&["software"]));

/// One row per software entry of a {hostname, host_software: {software: [...]}} payload. An entry
/// that is not an object is dropped.
pub fn host_software_rows(tenant: &str, received_at: DateTime<Utc>, payload: &Value) -> Vec<HostSoftware> {
    let top = Some(payload);
    let host_software = json::child("host_software", top);
    let payload_extra = merge_extra(
        json::other_members(&SOFTWARE_PAYLOAD_KEYS, json::compact_sorted, top),
        "host_software.",
        json::other_members(&HOST_SOFTWARE_KEYS, json::compact_sorted, host_software),
    );
    let Some(Value::Array(entries)) = json::child("software", host_software) else { return Vec::new() };
    entries
        .iter()
        .filter(|item| item.is_object())
        .map(|item| {
            let entry = Some(item);
            let deployment_time = json::text(json::child("deployment_time", entry));
            HostSoftware {
                tenant_id: tenant.into(),
                received_at,
                hostname: json::text(json::child("hostname", top)),
                software_type: json::text(json::child("software_type", entry)),
                name: json::text(json::child("name", entry)),
                version: json::text(json::child("version", entry)),
                publisher: json::text(json::child("publisher", entry)),
                deployment_status: json::text(json::child("deployment_status", entry)),
                deployment_time_parsed: time::try_rfc3339(&deployment_time),
                deployment_time,
                product_code: json::text(json::child("product_code", entry)),
                is_64_bit: json::flag(json::child("is_64_bit", entry)),
                install_paths: json::text_list(json::child("install_paths", entry)),
                extra: json::other_members(&SOFTWARE_ENTRY_KEYS, json::compact_sorted, entry),
                payload_extra: payload_extra.clone(),
            }
        })
        .collect()
}

/// softinv-intake: installed packages per host, batched.
pub async fn handle_software_inventory(ctx: Ctx, body: Bytes) -> Response {
    handle_batch(&ctx, &body, "softinv", |now, item| host_software_rows(ctx.tenant(), now, item))
}

static SYNTHETICS_KEYS: LazyLock<BTreeSet<&str>> =
    LazyLock::new(|| names(&["test", "location", "result", "_dd", "enrichment", "v"]));

static SYNTHETICS_RESULT_KEYS: LazyLock<BTreeSet<&str>> = LazyLock::new(|| {
    names(&[
        "id",
        "initialId",
        "status",
        "runType",
        "duration",
        "testStartedAt",
        "testFinishedAt",
        "testTriggeredAt",
        "assertions",
        "failure",
        "config",
        "netstats",
        "netpath",
    ])
});

/// One test result:
///
///   {test: {id, name, type, subType, version}, location: {id, name, displayName},
///    result: {id, initialId, status, runType, duration, testStartedAt, testFinishedAt,
///             testTriggeredAt, assertions: [{type, operator, expected, actual, valid}],
///             failure: {code, message}, config: {request: {host, port, ...}},
///             netstats: {packetsSent, packetsReceived, packetLossPercentage, jitter, latency,
///             hops}, netpath},
///    _dd: {...}, enrichment, v}
///
/// netpath is the same type netpath-intake carries.
pub fn synthetics_result_row(tenant: &str, received_at: DateTime<Utc>, item: &Value) -> SyntheticsResult {
    let top = Some(item);
    let test = json::child("test", top);
    let location = json::child("location", top);
    let result = json::child("result", top);
    let netstats = json::child("netstats", result);

    // An assertion that is not an object still takes its place in the arrays.
    let assertions = json::lenient_items(json::child("assertions", result));
    let per_assertion =
        |read: &dyn Fn(Option<&Value>) -> String| -> Vec<String> { assertions.iter().map(|a| read(Some(a))).collect() };
    let failure = object_of(json::child("failure", result));
    let started = json::text(json::child("testStartedAt", result));
    let finished = json::text(json::child("testFinishedAt", result));
    let triggered = json::text(json::child("testTriggeredAt", result));

    SyntheticsResult {
        tenant_id: tenant.into(),
        received_at,
        test_id: json::text(json::child("id", test)),
        test_name: json::text(json::child("name", test)),
        test_type: json::text(json::child("type", test)),
        test_subtype: json::text(json::child("subType", test)),
        test_version: json::text(json::child("version", test)),
        location_id: json::text(json::child("id", location)),
        location_name: json::text(json::child("name", location)),
        location_display_name: json::text(json::child("displayName", location)),
        result_id: json::text(json::child("id", result)),
        result_initial_id: json::text(json::child("initialId", result)),
        status: json::text(json::child("status", result)),
        run_type: json::text(json::child("runType", result)),
        duration: json::text(json::child("duration", result)),
        test_started_at_parsed: time::try_rfc3339(&started),
        test_started_at: started,
        test_finished_at_parsed: time::try_rfc3339(&finished),
        test_finished_at: finished,
        test_triggered_at_parsed: time::try_rfc3339(&triggered),
        test_triggered_at: triggered,
        assertion_type: per_assertion(&|a| json::text(json::child("type", a))),
        assertion_operator: per_assertion(&|a| json::text(json::child("operator", a))),
        assertion_expected: per_assertion(&|a| json::compact_sorted_or_empty(json::child("expected", a))),
        assertion_actual: per_assertion(&|a| json::compact_sorted_or_empty(json::child("actual", a))),
        assertion_valid: assertions
            .iter()
            .map(|a| text::flag(matches!(json::child("valid", Some(a)), Some(Value::Bool(true)))))
            .collect(),
        // The failure object being there at all is the signal.
        failure_code: failure.map(|_| json::text(json::child("code", failure))),
        failure_message: failure.map(|_| json::text(json::child("message", failure))),
        config: json::compact_sorted_or_empty(json::child("config", result)),
        netstats_packets_sent: json::child("packetsSent", netstats).and_then(json::int64_of),
        netstats_packets_received: json::child("packetsReceived", netstats).and_then(json::int64_of),
        netstats_packet_loss_percentage: json::child("packetLossPercentage", netstats).and_then(json::float_of),
        netstats_jitter: json::child("jitter", netstats).and_then(json::float_of),
        netstats_latency: json::child("latency", netstats).and_then(json::float_of),
        netstats_hops: json::child("hops", netstats).and_then(json::int64_of),
        netpath: json::compact_sorted_or_empty(json::child("netpath", result)),
        dd: json::compact_sorted_or_empty(json::child("_dd", top)),
        enrichment: json::compact_sorted_or_empty(json::child("enrichment", top)),
        v: json::text(json::child("v", top)),
        extra: merge_extra(
            json::other_members(&SYNTHETICS_KEYS, json::compact_sorted, top),
            "result.",
            json::other_members(&SYNTHETICS_RESULT_KEYS, json::compact_sorted, result),
        ),
    }
}

/// http-synthetics: results of network tests the agent ran on the server's behalf, batched.
pub async fn handle_synthetics(ctx: Ctx, body: Bytes) -> Response {
    handle_batch(&ctx, &body, "synthetics", |now, item| vec![synthetics_result_row(ctx.tenant(), now, item)])
}

static LINEAGE_KEYS: LazyLock<BTreeSet<&str>> =
    LazyLock::new(|| names(&["eventType", "eventTime", "producer", "schemaURL", "run", "job", "inputs", "outputs"]));

/// An inputs[] or outputs[] list as three index-aligned arrays.
struct Datasets {
    namespaces: Vec<String>,
    names: Vec<String>,
    facets: Vec<String>,
}

fn datasets(value: Option<&Value>) -> Datasets {
    let items = match value {
        Some(Value::Array(items)) => items.as_slice(),
        _ => &[],
    };
    Datasets {
        namespaces: items.iter().map(|d| json::text(json::child("namespace", Some(d)))).collect(),
        names: items.iter().map(|d| json::text(json::child("name", Some(d)))).collect(),
        facets: items.iter().map(|d| json::compact_sorted_or_empty(json::child("facets", Some(d)))).collect(),
    }
}

/// Field names follow the published OpenLineage spec (RunEvent).
pub fn open_lineage_event_row(
    tenant: &str,
    received_at: DateTime<Utc>,
    api_version: &str,
    via: &str,
    event: &Value,
) -> OpenLineageEvent {
    let top = Some(event);
    let run = json::child("run", top);
    let job = json::child("job", top);
    let event_time = json::text(json::child("eventTime", top));
    let inputs = datasets(json::child("inputs", top));
    let outputs = datasets(json::child("outputs", top));

    OpenLineageEvent {
        tenant_id: tenant.into(),
        received_at,
        event_type: json::text(json::child("eventType", top)),
        event_time_parsed: time::try_rfc3339(&event_time),
        event_time,
        producer: json::text(json::child("producer", top)),
        schema_url: json::text(json::child("schemaURL", top)),
        run_id: json::text(json::child("runId", run)),
        run_facets: json::compact_sorted_or_empty(json::child("facets", run)),
        job_namespace: json::text(json::child("namespace", job)),
        job_name: json::text(json::child("name", job)),
        job_facets: json::compact_sorted_or_empty(json::child("facets", job)),
        input_namespace: inputs.namespaces,
        input_name: inputs.names,
        input_facets: inputs.facets,
        output_namespace: outputs.namespaces,
        output_name: outputs.names,
        output_facets: outputs.facets,
        api_version: api_version.into(),
        via: via.into(),
        extra: json::other_members(&LINEAGE_KEYS, json::compact_sorted, top),
    }
}

/// OpenLineage RunEvents, forwarded as they are by the trace-agent's reverse proxy, with
/// ?api-version=2 when the agent is told to add it. The transport posts one RunEvent per request;
/// an array is accepted in case a client batches.
pub async fn handle_open_lineage(ctx: Ctx, body: Bytes) -> Response {
    let api_version = ctx.query("api-version");
    let via = ctx.header("via").to_string();
    handle_batch(&ctx, &body, "lineage", |now, event| {
        vec![open_lineage_event_row(ctx.tenant(), now, &api_version, &via, event)]
    })
}

/// Results of Data Observability query actions, batched. They come from Python integrations and
/// no type documents them, so each entry is stored whole.
pub async fn handle_query_actions(ctx: Ctx, body: Bytes) -> Response {
    let origin = ctx.header("dd-evp-origin").to_string();
    let origin_version = ctx.header("dd-evp-origin-version").to_string();
    handle_batch(&ctx, &body, "query-actions", |now, entry| {
        let keys = match entry {
            Value::Object(object) => {
                let mut keys: Vec<String> = object.keys().cloned().collect();
                keys.sort();
                keys
            }
            _ => Vec::new(),
        };
        vec![QueryActionResult {
            tenant_id: ctx.tenant().into(),
            received_at: now,
            result: json::compact_sorted(entry),
            keys,
            dd_evp_origin: origin.clone(),
            dd_evp_origin_version: origin_version.clone(),
        }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ninjapaws_proto::datadog::agentdiscovery::{
        AgentDiscoveryConfigFile, AgentDiscoveryEnvVar, AgentDiscoveryPayload,
    };
    use ninjapaws_proto::datadog::healthplatform::{HostInfo, PersistedIssue, Remediation, RemediationStep, Script};

    fn now() -> DateTime<Utc> {
        "2026-09-23T10:30:00Z".parse().unwrap()
    }

    fn at(text: &str) -> DateTime<Utc> {
        text.parse().unwrap()
    }

    fn parse(text: &str) -> Value {
        parse_object(text.as_bytes()).expect("the test's JSON is an object")
    }

    fn texts(pairs: &[(&str, &str)]) -> ninjapaws_storage::rows::Extra {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn discovery_batch() -> AgentDiscoveryPayloadBatch {
        let postgres = AgentDiscoveryPayload {
            integration: "postgres".into(),
            runtime: "python".into(),
            runtime_id: "rt-1".into(),
            ingestion_timestamp: Some(pbjson_types::Timestamp { seconds: now().timestamp() - 60, nanos: 0 }),
            config_files: vec![
                AgentDiscoveryConfigFile {
                    path: "/etc/datadog-agent/conf.d/postgres.yaml".into(),
                    content: b"instances:\n  - host: db\n    password: hunter2\n".to_vec(),
                    truncated: false,
                    payload_format: AgentDiscoveryConfigFilePayloadFormat::PayloadFormatYaml as i32,
                },
                AgentDiscoveryConfigFile {
                    path: "/etc/x.conf".into(),
                    content: vec![0xff, 0xfe],
                    truncated: true,
                    payload_format: 99,
                },
            ],
            env_vars: vec![
                AgentDiscoveryEnvVar { name: "PGPASSWORD".into(), value: "hunter2".into() },
                AgentDiscoveryEnvVar { name: "PGHOST".into(), value: "db".into() },
            ],
        };
        let redis = AgentDiscoveryPayload {
            integration: "redis".into(),
            runtime: "go".into(),
            runtime_id: "rt-2".into(),
            ..Default::default()
        };
        AgentDiscoveryPayloadBatch { host_id: "host-1".into(), payloads: vec![postgres, redis] }
    }

    #[test]
    fn a_discovery_batch_gives_one_row_per_payload_config_files_as_aligned_arrays() {
        let rows = agent_discovery_rows("acme", now(), &discovery_batch());
        assert_eq!(rows.len(), 2);
        let postgres = &rows[0];
        assert_eq!((postgres.tenant_id.as_str(), postgres.received_at), ("acme", now()));
        assert_eq!(
            [postgres.host_id.as_str(), &postgres.integration, &postgres.runtime, &postgres.runtime_id],
            ["host-1", "postgres", "python", "rt-1"]
        );
        assert_eq!(postgres.ingestion_timestamp, Some(at("2026-09-23T10:29:00Z")));
        assert_eq!(postgres.config_paths, ["/etc/datadog-agent/conf.d/postgres.yaml", "/etc/x.conf"]);
        // A config file that is not UTF-8 keeps its bytes.
        assert_eq!(postgres.config_contents[1], Blob(vec![0xff, 0xfe]));
        assert_eq!(postgres.config_truncated, [0, 1]);
        // A format the schema does not know is stored as its number.
        assert_eq!(postgres.config_formats, ["PAYLOAD_FORMAT_YAML", "99"]);
        // Values are kept, not only names, in wire order.
        assert_eq!(postgres.env_var_names, ["PGPASSWORD", "PGHOST"]);
        assert_eq!(postgres.env_var_values, ["hunter2", "db"]);

        let redis = &rows[1];
        assert_eq!(redis.host_id, "host-1");
        assert_eq!(redis.ingestion_timestamp, None);
        assert!(redis.config_paths.is_empty() && redis.env_var_names.is_empty());
    }

    fn health_report() -> HealthReport {
        let extra = serde_json::from_value(serde_json::json!({ "probe": "tcp", "attempts": 3.0 })).unwrap();
        let resolved = Issue {
            id: "z-issue".into(),
            issue_name: "disk_full".into(),
            title: "Disk full".into(),
            severity: IssueSeverity::High as i32,
            extra: Some(extra),
            tags: vec!["env:prod".into(), "env:staging".into()],
            persisted_issue: Some(PersistedIssue {
                state: IssueState::Resolved as i32,
                resolved_at: Some("2026-09-23T09:00:00Z".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        // No persisted issue at all.
        let unresolved = Issue {
            id: "a-issue".into(),
            issue_name: "cert_expiring".into(),
            severity: IssueSeverity::Medium as i32,
            ..Default::default()
        };
        HealthReport {
            schema_version: "1.0".into(),
            event_type: "health_report".into(),
            emitted_at: "2026-09-23T10:29:00Z".into(),
            service: "logs-agent".into(),
            host: Some(HostInfo { hostname: "web-1".into(), par_ids: vec!["par-1".into()], ..Default::default() }),
            issues: [("z-issue".to_string(), resolved), ("a-issue".to_string(), unresolved)].into(),
        }
    }

    fn report_id() -> Uuid {
        Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap()
    }

    #[test]
    fn a_health_report_row_carries_its_host_its_issue_count_and_the_parsed_emit_time() {
        let (row, issues) = health_report_rows("acme", now(), report_id(), &health_report(), None);
        assert_eq!((row.tenant_id.as_str(), row.received_at, row.report_id), ("acme", now(), report_id()));
        assert_eq!(
            [row.schema_version.as_str(), &row.event_type, &row.service, &row.host],
            ["1.0", "health_report", "logs-agent", "web-1"]
        );
        assert_eq!(row.par_ids, ["par-1"]);
        assert_eq!(row.agent_version, None);
        assert_eq!(row.issue_count, 2);
        assert_eq!(row.emitted_at_parsed, Some(at("2026-09-23T10:29:00Z")));

        // Sorted by key, each with the report's id.
        let keys: Vec<&str> = issues.iter().map(|i| i.issue_key.as_str()).collect();
        assert_eq!(keys, ["a-issue", "z-issue"]);
        assert!(issues.iter().all(|i| i.report_id == report_id()));

        let no_lifecycle = &issues[0];
        assert_eq!(no_lifecycle.resolved_at, None);
        assert_eq!(no_lifecycle.persisted_state, "");
        assert_eq!(no_lifecycle.severity, "ISSUE_SEVERITY_MEDIUM");
        assert_eq!(no_lifecycle.extra, "");
        assert_eq!(no_lifecycle.script_requires_root, None);
        assert!(no_lifecycle.remediation_step_order.is_empty());

        let with_lifecycle = &issues[1];
        assert_eq!(with_lifecycle.resolved_at.as_deref(), Some("2026-09-23T09:00:00Z"));
        assert_eq!(with_lifecycle.persisted_state, "ISSUE_STATE_RESOLVED");
        assert_eq!(with_lifecycle.severity, "ISSUE_SEVERITY_HIGH");
        assert_eq!(with_lifecycle.tags["env"], ["prod", "staging"]);
        // What protobuf decoded: a Struct's numbers are floats, and serde writes them so.
        assert_eq!(with_lifecycle.extra, r#"{"attempts":3.0,"probe":"tcp"}"#);
    }

    #[test]
    fn a_health_report_without_a_host_stores_empty_host_fields() {
        let (row, issues) = health_report_rows("acme", now(), report_id(), &HealthReport::default(), None);
        assert_eq!(row.host, "");
        assert_eq!(row.agent_version, None);
        assert!(row.par_ids.is_empty());
        assert_eq!(row.issue_count, 0);
        assert_eq!(row.emitted_at_parsed, None);
        assert!(issues.is_empty());
    }

    #[test]
    fn a_remediations_steps_and_script_are_stored_and_a_script_that_needs_no_root_says_so() {
        let remediation = Remediation {
            summary: "free some space".into(),
            steps: vec![
                RemediationStep { order: 1, text: "find big files".into() },
                RemediationStep { order: 2, text: "delete them".into() },
            ],
            script: Some(Script {
                language: "bash".into(),
                language_version: "5+".into(),
                filename: "clean.sh".into(),
                content: "rm -rf /tmp/x".into(),
                requires_root: false,
            }),
        };
        let report = HealthReport {
            issues: [("disk".to_string(), Issue { remediation: Some(remediation), ..Default::default() })].into(),
            ..Default::default()
        };
        let (_, issues) = health_report_rows("acme", now(), report_id(), &report, None);
        let row = &issues[0];
        assert_eq!(row.remediation_summary, "free some space");
        assert_eq!(row.remediation_step_order, [1, 2]);
        assert_eq!(row.remediation_step_text, ["find big files", "delete them"]);
        assert_eq!(
            [row.script_language.as_str(), &row.script_language_version, &row.script_filename, &row.script_content],
            ["bash", "5+", "clean.sh", "rm -rf /tmp/x"]
        );
        assert_eq!(row.script_requires_root, Some(0));
    }

    fn decoded(body: &str) -> HealthReport {
        decode_health_report(body.as_bytes()).expect("the report decodes")
    }

    // Extra is a protobuf Struct, which holds every number as a float64. An id above 2^53 survives
    // only because extra is taken from the raw JSON.
    #[test]
    fn an_issues_extra_keeps_a_number_above_2_53_digit_for_digit() {
        let body = r#"{"issues": {"conn-refused": {"id": "conn-refused",
                       "extra": {"probe": "tcp", "probe_pid": 9007199254740993}}}}"#;
        let raw = health_issues_raw(body.as_bytes());
        let (_, issues) = health_report_rows("acme", now(), report_id(), &decoded(body), raw.as_ref());
        assert_eq!(issues[0].id, "conn-refused");
        assert_eq!(issues[0].extra, r#"{"probe":"tcp","probe_pid":9007199254740993}"#);
    }

    #[test]
    fn a_health_report_decodes_snake_case_keys_and_enums_sent_as_numbers() {
        let report = decoded(
            r#"{
                "schema_version": "1.0", "event_type": "health_report", "emitted_at": "2026-09-23T10:29:00Z",
                "service": "datadog-agent", "unknown_key": {"ignored": true},
                "host": {"hostname": "web-1", "agent_version": "7.83.0", "par_ids": ["p1", "p2"]},
                "issues": {
                    "disk": {
                        "id": "disk", "issue_name": "disk_full", "title": "Disk full", "description": "d",
                        "category": "storage", "location": "core-agent", "severity": 3,
                        "detected_at": "2026-09-23T10:00:00.5Z", "source": "logs", "tags": ["env:prod", "team:a"],
                        "issue_type": "disk_full",
                        "remediation": {
                            "summary": "s", "steps": [{"order": 2, "text": "t"}],
                            "script": {"language": "bash", "language_version": "5", "filename": "f.sh",
                                       "requires_root": true, "content": "c"}
                        },
                        "persisted_issue": {"state": 4, "first_seen": "f", "last_seen": "l", "resolved_at": "r"}
                    }
                }
            }"#,
        );
        let (row, issues) = health_report_rows("acme", now(), report_id(), &report, None);
        assert_eq!(row.agent_version.as_deref(), Some("7.83.0"));
        assert_eq!(row.par_ids, ["p1", "p2"]);
        let issue = &issues[0];
        assert_eq!(
            [issue.issue_key.as_str(), &issue.issue_name, &issue.category, &issue.location],
            ["disk", "disk_full", "storage", "core-agent"]
        );
        assert_eq!(issue.severity, "ISSUE_SEVERITY_HIGH");
        assert_eq!(issue.detected_at_parsed, Some(at("2026-09-23T10:00:00.5Z")));
        assert_eq!(issue.tags["team"], ["a"]);
        assert_eq!(issue.remediation_step_order, [2]);
        assert_eq!(issue.script_requires_root, Some(1));
        assert_eq!(issue.persisted_state, "ISSUE_STATE_ACTIVE");
        assert_eq!((issue.first_seen.as_str(), issue.last_seen.as_str()), ("f", "l"));
        assert_eq!(issue.resolved_at.as_deref(), Some("r"));
    }

    #[test]
    fn a_health_reports_keys_are_the_schemas_names_in_either_spelling_and_null_leaves_a_field_empty() {
        let report = decoded(
            r#"{"schemaVersion": "1.0", "host": {"hostname": "h", "agent_version": ""}, "not_in_the_schema": 1,
                "issues": {"b": {"severity": 2, "extra": null, "tags": ["x"],
                                 "remediation": {"steps": [{"order": 1, "text": "t"}], "script": null},
                                 "persisted_issue": {"state": "ISSUE_STATE_ACTIVE", "resolved_at": null}}}}"#,
        );
        let raw = health_issues_raw(br#"{"issues": {"b": {"extra": null}}}"#);
        let (row, issues) = health_report_rows("acme", now(), report_id(), &report, raw.as_ref());
        assert_eq!((row.schema_version.as_str(), row.host.as_str()), ("1.0", "h"));
        // Sent, though empty: not the same as never sent.
        assert_eq!(row.agent_version.as_deref(), Some(""));
        let issue = &issues[0];
        assert_eq!(issue.extra, "");
        assert_eq!(issue.remediation_step_order, [1]);
        assert_eq!(issue.script_requires_root, None);
        assert_eq!(issue.persisted_state, "ISSUE_STATE_ACTIVE");
        assert_eq!(issue.resolved_at, None);
    }

    /// pbjson refuses an enum number the schema does not have, so the report is kept raw rather
    /// than stored with the number as the severity's name.
    #[test]
    fn a_severity_the_schema_does_not_have_refuses_the_report() {
        assert!(decode_health_report(br#"{"issues": {"b": {"severity": 7}}}"#).is_err());
    }

    #[test]
    fn a_health_report_with_a_value_of_the_wrong_type_is_refused_whole() {
        for body in [
            "[1]",
            "\"text\" ",
            r#"{"schema_version": 5}"#,
            r#"{"host": []}"#,
            r#"{"host": {"par_ids": "p1"}}"#,
            r#"{"issues": []}"#,
            r#"{"issues": {"a": 1}}"#,
            // Protobuf's JSON has no null inside a map or a list.
            r#"{"issues": {"a": null}}"#,
            r#"{"issues": {"a": {"tags": [null]}}}"#,
            "null",
            r#"{"issues": {"a": {"severity": 4294967296}}}"#,
            r#"{"issues": {"a": {"tags": "x"}}}"#,
            r#"{"issues": {"a": {"tags": [1]}}}"#,
            r#"{"issues": {"a": {"extra": 5}}}"#,
            r#"{"issues": {"a": {"extra": [1]}}}"#,
            r#"{"issues": {"a": {"remediation": {"steps": [1]}}}}"#,
            r#"{"issues": {"a": {"remediation": {"script": {"requires_root": "yes"}}}}}"#,
            r#"{"schema_version": "#,
        ] {
            assert!(decode_health_report(body.as_bytes()).is_err(), "{body}");
        }
    }

    const EVENT_ENVELOPE: &str = r#"{
        "data": {
            "type": "event",
            "id": "evt-123",
            "attributes": {
                "host": "web-1",
                "title": "High CPU",
                "category": "performance",
                "integration_id": "system",
                "message": "cpu above 90%",
                "timestamp": "2026-09-23T10:30:00Z",
                "tags": ["env:prod", "env:staging"],
                "aggregation_key": "cpu-web-1",
                "system-notable-events": {"event_type": "anomaly"},
                "attributes": {"status": "warn", "priority": "high"},
                "unknown_field": {"nested": true}
            }
        },
        "meta": {"page": 1}
    }"#;

    fn event_row(text: &str) -> EventManagementEvent {
        event_management_row("acme", now(), &parse(text)).expect("the envelope gives a row")
    }

    #[test]
    fn an_event_envelopes_documented_attributes_become_columns() {
        let row = event_row(EVENT_ENVELOPE);
        assert_eq!((row.tenant_id.as_str(), row.received_at), ("acme", now()));
        assert_eq!(
            [row.data_type.as_str(), &row.host, &row.title, &row.category, &row.integration_id, &row.message],
            ["event", "web-1", "High CPU", "performance", "system", "cpu above 90%"]
        );
        assert_eq!(row.timestamp, "2026-09-23T10:30:00Z");
        assert_eq!(row.timestamp_parsed, Some(now()));
        assert_eq!(row.aggregation_key, "cpu-web-1");
        assert_eq!(row.tags["env"], ["prod", "staging"]);
        // The outer attributes and the producer's own inner "attributes" object must not be
        // confused, and the hyphenated key must be read as it is spelled.
        assert_eq!(row.notable_event_type, "anomaly");
        assert_eq!(row.attributes, r#"{"priority":"high","status":"warn"}"#);
        // Undeclared keys of all three levels land in outer_extra, prefixed by level.
        assert_eq!(
            row.outer_extra,
            texts(&[
                ("unknown_field", r#"{"nested":true}"#),
                ("data.id", "\"evt-123\""),
                ("top.meta", r#"{"page":1}"#)
            ])
        );
    }

    #[test]
    fn an_event_envelope_without_a_data_attributes_object_gives_no_row() {
        for text in [
            r#"{"data": {"type": "event"}}"#,
            r#"{"data": {"type": "event", "attributes": "text"}}"#,
            r#"{"data": []}"#,
            "{}",
            "null",
        ] {
            assert!(event_management_row("acme", now(), &parse(text)).is_none(), "{text}");
        }
    }

    #[test]
    fn an_event_without_inner_attributes_stores_an_empty_object_for_them() {
        let row = event_row(r#"{"data": {"attributes": {"title": "t"}}}"#);
        assert_eq!(row.attributes, "{}");
        assert_eq!((row.data_type.as_str(), row.timestamp.as_str(), row.timestamp_parsed), ("", "", None));
        assert!(row.tags.is_empty() && row.outer_extra.is_empty());
    }

    #[test]
    fn event_values_that_are_not_strings_are_stored_as_sent() {
        let row = event_row(
            r#"{"data": {"type": 7, "attributes": {
                "host": 9007199254740993, "title": true, "message": {"b": 1, "a": [15, null]},
                "timestamp": 1758622200, "tags": "env:prod", "aggregation_key": null,
                "system-notable-events": "not-an-object", "extra_null": null}}}"#,
        );
        assert_eq!([row.data_type.as_str(), &row.host, &row.title], ["7", "9007199254740993", "true"]);
        assert_eq!(row.message, r#"{"a":[15,null],"b":1}"#);
        assert_eq!((row.timestamp.as_str(), row.timestamp_parsed), ("1758622200", None));
        // A single tag sent as a bare string is a list of one.
        assert_eq!(row.tags["env"], ["prod"]);
        assert_eq!((row.aggregation_key.as_str(), row.notable_event_type.as_str()), ("", ""));
        // An undeclared key whose value is null is kept as it was sent.
        assert_eq!(row.outer_extra["extra_null"], "null");
    }

    #[test]
    fn a_body_is_read_up_to_the_end_of_its_first_json_value() {
        let envelope = parse_object(br#"{"data": {"attributes": {"title": "first"}}} {"data": 1}"#).unwrap();
        assert_eq!(event_management_row("acme", now(), &envelope).unwrap().title, "first");
        assert!(parse_object(br#"[{"data": {}}]"#).is_none());
        assert!(parse_object(b"\"text\" ").is_none());
        assert!(parse_object(br#"{"data": "#).is_none());
    }

    const SOFTWARE_PAYLOAD: &str = r#"{
        "hostname": "web-1",
        "extra_payload_field": "seen",
        "host_software": {
            "collected_at": "2026-09-23T10:00:00Z",
            "software": [
                {"software_type": "package", "name": "curl", "version": "7.88.1",
                 "publisher": "curl", "deployment_status": "installed",
                 "deployment_time": "2026-09-20T00:00:00Z", "product_code": "curl",
                 "is_64_bit": true, "install_paths": ["/usr/bin/curl"]},
                {"software_type": "package", "name": "mystery",
                 "vendor_specific_field": "x"},
                "not-an-object"
            ]
        }
    }"#;

    #[test]
    fn a_software_payload_gives_one_row_per_entry_and_drops_an_entry_that_is_not_an_object() {
        let rows = host_software_rows("acme", now(), &parse(SOFTWARE_PAYLOAD));
        let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["curl", "mystery"]);
        assert!(rows.iter().all(|r| r.hostname == "web-1" && r.tenant_id == "acme" && r.received_at == now()));

        let curl = &rows[0];
        assert_eq!(
            [curl.software_type.as_str(), &curl.version, &curl.publisher, &curl.deployment_status, &curl.product_code],
            ["package", "7.88.1", "curl", "installed", "curl"]
        );
        assert_eq!(curl.deployment_time_parsed, Some(at("2026-09-20T00:00:00Z")));
        assert_eq!(curl.is_64_bit, Some(1));
        assert_eq!(curl.install_paths, ["/usr/bin/curl"]);
        assert!(curl.extra.is_empty());

        // Unknown bitness is common and real: false would claim 32-bit.
        let mystery = &rows[1];
        assert_eq!((mystery.is_64_bit, mystery.deployment_time_parsed), (None, None));
        assert!(mystery.install_paths.is_empty());
        assert_eq!(mystery.extra, texts(&[("vendor_specific_field", "\"x\"")]));

        // The payload's undeclared keys repeat onto every row, host_software's own under a prefix.
        for row in &rows {
            assert_eq!(
                row.payload_extra,
                texts(&[
                    ("extra_payload_field", "\"seen\""),
                    ("host_software.collected_at", "\"2026-09-23T10:00:00Z\"")
                ])
            );
        }
    }

    #[test]
    fn software_bitness_is_read_only_from_a_json_bool() {
        let rows = host_software_rows(
            "acme",
            now(),
            &parse(
                r#"{"host_software": {"software": [{"is_64_bit": false}, {"is_64_bit": "true"}, {"is_64_bit": 1}]}}"#,
            ),
        );
        let bitness: Vec<Option<u8>> = rows.iter().map(|r| r.is_64_bit).collect();
        assert_eq!(bitness, [Some(0), None, None]);
    }

    #[test]
    fn a_software_payload_without_a_software_list_gives_no_rows() {
        for text in [
            r#"{"hostname": "web-1"}"#,
            r#"{"hostname": "web-1", "host_software": {"software": []}}"#,
            r#"{"hostname": "web-1", "host_software": {"software": {"name": "curl"}}}"#,
            r#"{"hostname": "web-1", "host_software": "text"}"#,
            "null",
        ] {
            assert!(host_software_rows("acme", now(), &parse(text)).is_empty(), "{text}");
        }
    }

    const SYNTHETICS_RESULT: &str = r#"{
        "test": {"id": "t-1", "name": "check google", "type": "api", "subType": "http", "version": "1"},
        "location": {"id": "loc-1", "name": "aws:eu-west-1", "displayName": "EU West"},
        "result": {
            "id": "r-1", "initialId": "r-1", "status": "passed", "runType": "scheduled",
            "duration": 123.45,
            "testStartedAt": "2026-09-23T10:30:00Z",
            "testFinishedAt": "2026-09-23T10:30:01Z",
            "assertions": [
                {"type": "statusCode", "operator": "is", "expected": 200, "actual": 200, "valid": true},
                {"type": "responseTime", "operator": "lessThan", "expected": 1000, "actual": 1500, "valid": false}
            ],
            "netstats": {"packetsSent": 9007199254740993, "packetsReceived": 10, "packetLossPercentage": 0.5,
                "jitter": 1.2, "latency": 42.5, "hops": 3}
        },
        "_dd": {"origin": "synthetics"},
        "v": 5
    }"#;

    fn synthetics(text: &str) -> SyntheticsResult {
        synthetics_result_row("acme", now(), &parse(text))
    }

    #[test]
    fn a_synthetics_results_test_location_and_result_fields_become_columns() {
        let row = synthetics(SYNTHETICS_RESULT);
        assert_eq!(
            [row.test_id.as_str(), &row.test_name, &row.test_type, &row.test_subtype, &row.test_version],
            ["t-1", "check google", "api", "http", "1"]
        );
        assert_eq!(
            [row.location_id.as_str(), &row.location_name, &row.location_display_name],
            ["loc-1", "aws:eu-west-1", "EU West"]
        );
        assert_eq!(
            [row.result_id.as_str(), &row.result_initial_id, &row.status, &row.run_type, &row.duration],
            ["r-1", "r-1", "passed", "scheduled", "123.45"]
        );
        assert_eq!(row.test_started_at_parsed, Some(now()));
        assert_eq!(row.test_finished_at_parsed, Some(at("2026-09-23T10:30:01Z")));
        assert_eq!((row.test_triggered_at.as_str(), row.test_triggered_at_parsed), ("", None));
        assert_eq!((row.dd.as_str(), row.v.as_str()), (r#"{"origin":"synthetics"}"#, "5"));
        assert_eq!([row.config.as_str(), &row.netpath, &row.enrichment], ["", "", ""]);
        // Every key here is a documented one.
        assert!(row.extra.is_empty());

        // Assertions keep their order and count as five aligned arrays.
        assert_eq!(row.assertion_type, ["statusCode", "responseTime"]);
        assert_eq!(row.assertion_operator, ["is", "lessThan"]);
        assert_eq!(row.assertion_expected, ["200", "1000"]);
        assert_eq!(row.assertion_actual, ["200", "1500"]);
        assert_eq!(row.assertion_valid, [1, 0]);

        // A counter one above 2^53 survives only if it never passes through a float.
        assert_eq!(row.netstats_packets_sent, Some(9007199254740993));
        assert_eq!((row.netstats_packets_received, row.netstats_hops), (Some(10), Some(3)));
        assert_eq!(
            (row.netstats_packet_loss_percentage, row.netstats_jitter, row.netstats_latency),
            (Some(0.5), Some(1.2), Some(42.5))
        );
        // The failure object being there at all is the signal.
        assert_eq!((row.failure_code, row.failure_message), (None, None));
    }

    #[test]
    fn an_assertion_that_is_not_an_object_still_takes_its_place_in_the_arrays() {
        let row = synthetics(
            r#"{"result": {"assertions": ["text", {"type": "t", "expected": {"b": 1, "a": "x"}, "valid": "true"}]}}"#,
        );
        assert_eq!(row.assertion_type, ["", "t"]);
        assert_eq!(row.assertion_operator, ["", ""]);
        assert_eq!(row.assertion_expected, ["", r#"{"a":"x","b":1}"#]);
        assert_eq!(row.assertion_actual, ["", ""]);
        // Only a JSON true is valid.
        assert_eq!(row.assertion_valid, [0, 0]);
    }

    #[test]
    fn a_netstat_that_is_not_a_number_of_the_columns_kind_stores_null() {
        let row = synthetics(
            r#"{"result": {"netstats": {"packetsSent": 3.0, "packetsReceived": "10", "hops": 1e3,
                "packetLossPercentage": "0.5", "jitter": "1.2", "latency": 7}}}"#,
        );
        assert_eq!((row.netstats_packets_sent, row.netstats_packets_received, row.netstats_hops), (None, None, None));
        assert_eq!((row.netstats_packet_loss_percentage, row.netstats_jitter), (None, None));
        assert_eq!(row.netstats_latency, Some(7.0));
    }

    #[test]
    fn a_synthetics_result_with_a_failure_object_stores_its_code_and_message() {
        let row = synthetics(r#"{"result":{"failure":{"code":"TIMEOUT","message":"no response"}}}"#);
        assert_eq!(
            (row.failure_code.as_deref(), row.failure_message.as_deref()),
            (Some("TIMEOUT"), Some("no response"))
        );
        let empty = synthetics(r#"{"result":{"failure":{}}}"#);
        assert_eq!((empty.failure_code.as_deref(), empty.failure_message.as_deref()), (Some(""), Some("")));
    }

    #[test]
    fn an_undeclared_key_directly_on_the_result_lands_in_extra_under_a_prefix() {
        let row = synthetics(r#"{"result":{"status":"passed","region":"eu-west-1"},"region":"top"}"#);
        assert_eq!(row.extra, texts(&[("result.region", "\"eu-west-1\""), ("region", "\"top\"")]));
    }

    #[test]
    fn synthetics_config_netpath_and_enrichment_are_kept_as_json_with_sorted_keys() {
        let row = synthetics(
            r#"{"result": {"duration": "12ms", "config": {"request": {"port": 443, "host": "example.com"}},
                            "netpath": {"hops": [{"ttl": 1}], "id": 9007199254740993}},
                "enrichment": [1, "two"]}"#,
        );
        assert_eq!(row.duration, "12ms");
        assert_eq!(row.config, r#"{"request":{"host":"example.com","port":443}}"#);
        assert_eq!(row.netpath, r#"{"hops":[{"ttl":1}],"id":9007199254740993}"#);
        assert_eq!(row.enrichment, r#"[1,"two"]"#);
    }

    const LINEAGE_EVENT: &str = r#"{
        "eventType": "COMPLETE",
        "eventTime": "2026-09-23T10:30:00Z",
        "producer": "https://example.com/spark",
        "schemaURL": "https://openlineage.io/spec/1-0-5/OpenLineage.json",
        "run": {"runId": "run-1", "facets": {"parent": {"run": {"runId": "run-0"}}}},
        "job": {"namespace": "etl", "name": "load_orders", "facets": {"sql": {"query": "SELECT 1"}}},
        "inputs": [{"namespace": "warehouse", "name": "raw_orders", "facets": {"schema": {"fields": []}}}],
        "outputs": [
            {"namespace": "warehouse", "name": "orders"},
            {"namespace": "warehouse", "name": "orders_summary"}
        ],
        "custom_extension_field": "seen"
    }"#;

    #[test]
    fn an_open_lineage_events_run_job_and_facets_are_stored_facets_as_opaque_json() {
        let row = open_lineage_event_row("acme", now(), "2", "trace-agent/7", &parse(LINEAGE_EVENT));
        assert_eq!((row.event_type.as_str(), row.event_time_parsed), ("COMPLETE", Some(now())));
        assert_eq!(row.producer, "https://example.com/spark");
        assert_eq!(row.schema_url, "https://openlineage.io/spec/1-0-5/OpenLineage.json");
        assert_eq!(row.run_id, "run-1");
        assert_eq!(row.run_facets, r#"{"parent":{"run":{"runId":"run-0"}}}"#);
        assert_eq!((row.job_namespace.as_str(), row.job_name.as_str()), ("etl", "load_orders"));
        assert_eq!(row.job_facets, r#"{"sql":{"query":"SELECT 1"}}"#);
        assert_eq!((row.api_version.as_str(), row.via.as_str()), ("2", "trace-agent/7"));
        assert_eq!(row.extra, texts(&[("custom_extension_field", "\"seen\"")]));

        // Inputs and outputs are independent lists, each with its own alignment.
        assert_eq!(
            (row.input_namespace.as_slice(), row.input_name.as_slice()),
            (&["warehouse".to_string()][..], &["raw_orders".to_string()][..])
        );
        assert_eq!(row.input_facets, [r#"{"schema":{"fields":[]}}"#]);
        assert_eq!(row.output_namespace, ["warehouse", "warehouse"]);
        assert_eq!(row.output_name, ["orders", "orders_summary"]);
        assert_eq!(row.output_facets, ["", ""]);
    }

    #[test]
    fn an_open_lineage_event_with_nothing_in_it_stores_empty_columns() {
        let row =
            open_lineage_event_row("acme", now(), "", "", &parse(r#"{"inputs": ["text"], "outputs": {"name": "x"}}"#));
        assert_eq!((row.event_type.as_str(), row.event_time_parsed, row.run_facets.as_str()), ("", None, ""));
        // A dataset that is not an object still takes its place.
        assert_eq!(
            (row.input_namespace.as_slice(), row.input_facets.as_slice()),
            (&[String::new()][..], &[String::new()][..])
        );
        assert!(row.output_name.is_empty() && row.extra.is_empty());
    }
}
