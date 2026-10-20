//! api.<site> / app.<site>: the main forwarder.
//!
//!   agent config: dd_url / DD_DD_URL
//!
//! The one intake carrying several unrelated signals, because it predates Datadog's split into
//! per-product intakes. Where the rows go:
//!
//!   /api/v1/series, /api/v2/series        metrics
//!   /api/beta/sketches                    sketches + agent_batch_metadata
//!   /api/v1/distribution_points           sketches
//!   /api/v1/check_run                     check_runs
//!   /api/v1/events                        events
//!   /intake/  events variant              events
//!             agent_checks variant        agent_checks + external_host_tags
//!             gohai/systemStats variant   hosts
//!             resources alone             process_groups
//!   /api/v1/metadata                      agent_metadata
//!   /api/v2/intake-key                    delegated_auth_requests
//!   /api/v2/profiles/symbols/query        symbol_queries
//!   Private Action Runner                 runner_enrollments, runner_dequeues,
//!                                         runner_task_updates, runner_heartbeats,
//!                                         action_connections
//!   GET /api/v1/query, GET /api/v2/validate, the runner's health-check:
//!                                         nothing; they are questions, not data
//!
//! The rule for every handler: what has a column is stored, what cannot be decoded goes to
//! raw_payloads.

use std::collections::BTreeMap;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use http::{HeaderValue, StatusCode};
use ninjapaws_proto::datadog::agentpayload::CommonMetadata;
use ninjapaws_storage::column;
use ninjapaws_storage::fnv;
use ninjapaws_storage::rows::agent::{
    AgentBatchMetadata, AgentCheck, AgentMetadata, DelegatedAuth, ExternalHostTags, ProcessGroup,
};
use ninjapaws_storage::rows::checks::{CheckRun, CheckStatus};
use ninjapaws_storage::rows::events::Event;
use ninjapaws_storage::rows::hosts::Host;
use ninjapaws_storage::rows::metrics::{MetricPoint, SketchRow};
use ninjapaws_storage::rows::profiling::SymbolQuery;
use ninjapaws_storage::rows::runner::{
    ActionConnection, RunnerDequeue, RunnerEnrollment, RunnerHeartbeat, RunnerTaskUpdate,
};
use ninjapaws_storage::rows::{Extra, Tags};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tako::types::Response;
use uuid::Uuid;

use super::api_payloads::*;
use crate::body;
use crate::ddsketch;
use crate::json;
use crate::server::{Ctx, Routes, accepted, json, json_as, status};
use crate::wire::{metric_type_name, tags, text, time};

/// What app.<site> and api.<site> both serve.
pub fn metrics_and_checks(routes: &mut Routes) {
    routes.post("/intake/", handle_intake);
    routes.post("/api/v1/series", handle_series_v1);
    routes.post("/api/v2/series", handle_series_v2);
    routes.post("/api/v1/check_run", handle_check_run);
    routes.post("/api/v1/metadata", handle_metadata);
    routes.post("/api/beta/sketches", handle_sketches);

    // Not sent by any agent: API clients only.
    routes.post("/api/v1/events", handle_events);
    routes.post("/api/v1/distribution_points", handle_distribution_points);

    routes.post("/api/v2/intake-key", handle_intake_key);
    routes.post("/api/v2/profiles/symbols/query", handle_symbols_query);
    // The agent sends GET; POST gets the same answer.
    routes.post("/api/v2/validate", handle_opm_validate);

    routes.post("/api/unstable/on_prem_runners", handle_runner_enroll);
    routes.post("/api/unstable/on_prem_runners/api_key_only", handle_runner_enroll);
    routes.post("/api/v2/on-prem-management-service/workflow-tasks/dequeue", handle_runner_dequeue);
    routes.post("/api/v2/on-prem-management-service/workflow-tasks/publish-task-update", handle_runner_task_update);
    routes.post("/api/v2/on-prem-management-service/workflow-tasks/heartbeat", handle_runner_heartbeat);
    routes.post("/api/v2/actions/connections", handle_action_connections);

    routes.get("/api/v1/query", handle_query);
    routes.get("/api/v2/validate", handle_opm_validate);
    routes.get("/api/v2/on-prem-management-service/runner/health-check", handle_runner_health_check);
}

/// api.<site>, <version>-flare.agent.<site>. The CI Visibility endpoints Datadog also puts on
/// this host are not ported yet.
pub fn mount(routes: &mut Routes) {
    metrics_and_checks(routes);
}

/// The reply the agent expects after a batch of metrics. It reads the body: an empty object would
/// be an API error to it.
fn ack_series() -> Response {
    json(StatusCode::ACCEPTED, &json!({ "errors": [] }))
}

fn status_ok() -> Response {
    json(StatusCode::OK, &json!({ "status": "ok" }))
}

/// The keys of a map for a note: sorted, the first eight, then how many more.
fn key_list<V>(values: &BTreeMap<String, V>) -> String {
    let keys: Vec<&str> = values.keys().map(String::as_str).collect();
    const LIMIT: usize = 8;
    if keys.is_empty() {
        "(none)".into()
    } else if keys.len() <= LIMIT {
        keys.join(", ")
    } else {
        format!("{} ... {} more", keys[..LIMIT].join(", "), keys.len() - LIMIT)
    }
}

/// For an `extra` column: every value JSON-encoded, so one column takes a string, a number and an
/// object alike, and a string that looks like a number stays a string.
fn extra_encoded(skip: &[&str], values: &BTreeMap<String, Value>) -> Extra {
    values
        .iter()
        .filter(|(name, _)| !skip.contains(&name.as_str()))
        .map(|(k, v)| (k.clone(), json::compact(v)))
        .collect()
}

/// The members of an object as the texts they were sent as, by name.
type Sent<'t> = BTreeMap<String, &'t str>;

/// A member's text as sent, "" when it was not sent.
fn sent_text(sent: &Sent, name: &str) -> String {
    sent.get(name).map(|t| t.to_string()).unwrap_or_default()
}

/// For an `extra` column of a document kept as it came: the members not in `skip`, each as the
/// text it was sent as.
fn extra_sent(skip: &[&str], text: &str) -> Extra {
    json::raw::other_members(&skip.iter().copied().collect(), text)
}

/// Folds a nested object's extras into the outer ones under a prefix, so two levels of one
/// document share a column without colliding.
pub(crate) fn merge_extra(mut outer: Extra, prefix: &str, nested: Extra) -> Extra {
    for (name, value) in nested {
        outer.insert(format!("{prefix}{name}"), value);
    }
    outer
}

/// A float timestamp as whole seconds: the fraction dropped, and NaN or anything an i64 cannot
/// hold becomes the minimum, which `time::wire_seconds` then reads as "not supplied".
fn whole_seconds(value: f64) -> i64 {
    if value.is_nan() || value >= i64::MAX as f64 || value < i64::MIN as f64 { i64::MIN } else { value as i64 }
}

// ---------------------------------------------------------------------------
// Metrics
// ---------------------------------------------------------------------------

/// /api/v2/series: protobuf from the agent, JSON from the public API's clients, both the same
/// shape.
///
/// The host is a resource of type "host"; every other resource type (the agent writes one more,
/// "device") goes into the row's resources map, so a device series stays distinguishable from a
/// host one.
pub async fn handle_series_v2(ctx: Ctx, body: Bytes) -> Response {
    if ctx.is_sweep() {
        return ack_series();
    }
    let decoded = if body::is_json(ctx.header("content-type"), &body) {
        parse_series_v2_json(&body)
    } else {
        parse_series_v2_protobuf(&body)
    };
    match decoded {
        Err(error) => ctx.raw("series", "decode_error", &error, &body),
        Ok(payload) => {
            let mut points = Vec::new();
            for series in &payload.series {
                let host =
                    series.resources.iter().find(|r| r.r#type == "host").map(|r| r.name.clone()).unwrap_or_default();
                let resources = series
                    .resources
                    .iter()
                    .filter(|r| r.r#type != "host")
                    .map(|r| (r.r#type.clone(), r.name.clone()))
                    .collect();
                let origin = series.metadata.as_ref().and_then(|m| m.origin).unwrap_or_default();
                let tags = tags::to_multi_map(&series.tags);
                for point in &series.points {
                    points.push(MetricPoint {
                        host: host.clone(),
                        metric_type: metric_type_name(i64::from(series.r#type)).into(),
                        source_type: series.source_type_name.clone(),
                        unit: series.unit.clone(),
                        interval: series.interval as u32,
                        tags: tags.clone(),
                        origin_product: origin.origin_product,
                        origin_category: origin.origin_category,
                        origin_service: origin.origin_service,
                        resources: BTreeMap::clone(&resources),
                        ..MetricPoint::new(
                            ctx.tenant(),
                            time::wire_seconds(point.timestamp),
                            &series.metric,
                            point.value,
                        )
                    });
                }
            }
            ctx.write(points);
        }
    }
    ack_series()
}

/// The v1 wire's type word; an unknown word stays UNSPECIFIED rather than being guessed at.
pub fn normalize_type_word(word: &str) -> &'static str {
    match word.to_lowercase().as_str() {
        "count" => "COUNT",
        "rate" => "RATE",
        "gauge" => "GAUGE",
        _ => "UNSPECIFIED",
    }
}

/// /api/v1/series: the older public API, JSON only. The agent's v1 encoder also writes device,
/// source_type_name and unit, which the API does not have: the last two have columns, device goes
/// into the resources map under the name v2 uses for it, anything else the API does not have into
/// `extra`.
pub async fn handle_series_v1(ctx: Ctx, body: Bytes) -> Response {
    if ctx.is_sweep() {
        return ack_series();
    }
    match parse_series_v1(&body) {
        Err(error) => ctx.raw("series", "decode_error", &error, &body),
        Ok(payload) => {
            if !payload.additional.is_empty() {
                // A key beside `series` belongs to the batch, not to a point: copying it onto
                // every row would multiply it. The body is kept instead, once.
                ctx.raw(
                    "series",
                    "unexpected_shape",
                    &format!("v1 payload carried undeclared top-level keys: {}", key_list(&payload.additional)),
                    &body,
                );
            }
            for (_, item, why) in &payload.rejected {
                ctx.raw("series", "unexpected_shape", why, item.as_bytes());
            }

            let mut points = Vec::new();
            for (series, at) in payload.series.iter().zip(&payload.positions) {
                let interval = series.interval.map_or(0, |seconds| seconds as u32);
                let resources = match json::lenient_string(series.additional.get("device")).as_str() {
                    "" => BTreeMap::new(),
                    device => BTreeMap::from([("device".to_string(), device.to_string())]),
                };
                let extra = extra_encoded(&["device", "source_type_name", "unit"], &series.additional);
                let tags = tags::to_multi_map(&series.tags);

                for (j, point) in series.points.iter().enumerate() {
                    match point.pair.get(..2) {
                        Some([Some(timestamp), Some(value)]) => points.push(MetricPoint {
                            host: series.host.clone(),
                            metric_type: normalize_type_word(&series.kind).into(),
                            source_type: json::lenient_string(series.additional.get("source_type_name")),
                            unit: json::lenient_string(series.additional.get("unit")),
                            interval,
                            tags: tags.clone(),
                            resources: resources.clone(),
                            extra: extra.clone(),
                            ..MetricPoint::new(
                                ctx.tenant(),
                                time::wire_seconds(whole_seconds(*timestamp)),
                                &series.metric,
                                *value,
                            )
                        }),
                        // The series is stored through its other points; the pair is the
                        // smallest thing worth keeping.
                        _ => ctx.raw(
                            "series",
                            "unexpected_shape",
                            &format!("series[{at}].points[{j}]: expected a [timestamp, value] pair of numbers"),
                            point.raw.as_bytes(),
                        ),
                    }
                }
            }
            ctx.write(points);
        }
    }
    ack_series()
}

/// Who sent a batch, as opposed to what was in it: agent version, timezone, the agent's clock and
/// both of its IPs. The api_key in the same block is a credential and is not carried over. An
/// all-empty block is not stored.
fn store_batch_metadata(ctx: &Ctx, intake: &str, sender: &CommonMetadata) {
    let empty = sender.agent_version.is_empty()
        && sender.timezone.is_empty()
        && sender.current_epoch == 0.0
        && sender.internal_ip.is_empty()
        && sender.public_ip.is_empty();
    if !empty {
        ctx.write(vec![AgentBatchMetadata {
            tenant_id: ctx.tenant().into(),
            received_at: Utc::now(),
            intake: intake.into(),
            agent_version: sender.agent_version.clone(),
            timezone: sender.timezone.clone(),
            current_epoch: sender.current_epoch,
            internal_ip: sender.internal_ip.clone(),
            public_ip: sender.public_ip.clone(),
        }]);
    }
}

/// /api/beta/sketches: DDSketch histograms the agent has already built. One row per dogsketch,
/// not per metric: each is its own point in time.
///
/// A sketch may also carry `distributions`, the pre-DDSketch encoding (raw values and ranks). The
/// current agent never writes it and it cannot become a bucket row without inventing numbers, so
/// such a sketch is kept raw, whole.
pub async fn handle_sketches(ctx: Ctx, body: Bytes) -> Response {
    if ctx.is_sweep() {
        return ack_series();
    }
    let decoded = if body::is_json(ctx.header("content-type"), &body) {
        parse_sketches_json(&body)
    } else {
        parse_sketches_protobuf(&body)
    };
    match decoded {
        Err(error) => ctx.raw("sketches", "decode_error", &error, &body),
        Ok(payload) => {
            let mut rows = Vec::new();
            for sketch in &payload.sketches {
                if !sketch.distributions.is_empty() {
                    ctx.raw(
                        "sketches",
                        "no_schema",
                        &format!("legacy pre-DDSketch Distribution for {} has no row shape", sketch.metric),
                        json::proto::message(Some(sketch)).as_bytes(),
                    );
                }
                let origin = sketch.metadata.as_ref().and_then(|m| m.origin).unwrap_or_default();
                let tags = tags::to_multi_map(&sketch.tags);
                for dogsketch in &sketch.dogsketches {
                    rows.push(SketchRow {
                        tenant_id: ctx.tenant().into(),
                        timestamp: time::wire_seconds(dogsketch.ts),
                        metric: sketch.metric.clone(),
                        host: sketch.host.clone(),
                        tags: tags.clone(),
                        count: dogsketch.cnt as u64,
                        min: dogsketch.min,
                        max: dogsketch.max,
                        avg: dogsketch.avg,
                        sum: dogsketch.sum,
                        bucket_keys: dogsketch.k.clone(),
                        bucket_counts: dogsketch.n.clone(),
                        origin_product: origin.origin_product,
                        origin_category: origin.origin_category,
                        origin_service: origin.origin_service,
                        ..Default::default()
                    });
                }
            }
            ctx.write(rows);
            store_batch_metadata(&ctx, "sketches", &payload.metadata.unwrap_or_default());
        }
    }
    ack_series()
}

/// A distribution point's time and values. The pair is heterogeneous: of its elements the last
/// timestamp and the last value list count. No values, no point.
pub fn distribution_point(items: &[DistributionItem]) -> Option<(DateTime<Utc>, Vec<f64>)> {
    let mut timestamp = 0.0;
    let mut values: &[f64] = &[];
    for item in items {
        match item {
            DistributionItem::Timestamp(seconds) => timestamp = *seconds,
            DistributionItem::Values(list) => values = list,
            DistributionItem::Absent | DistributionItem::Unfit => {}
        }
    }
    if values.is_empty() { None } else { Some((time::wire_seconds(whole_seconds(timestamp)), values.to_vec())) }
}

/// /api/v1/distribution_points. The agent never calls this: it sends finished sketches. Clients
/// send RAW VALUES here, so the sketch is built on our side, with the agent's own bucketing so both
/// sources merge by key.
///
/// The raw values are not kept: a distribution exists to be merged with the agent's sketches,
/// which arrive already bucketed. What does not fit is kept whole: a body with no series list, an
/// element that is not a series, a pair that could not be split.
pub async fn handle_distribution_points(ctx: Ctx, body: Bytes) -> Response {
    if ctx.is_sweep() {
        return ack_series();
    }
    match parse_distribution_points(&body) {
        Err(error) => ctx.raw("distribution_points", "decode_error", &error, &body),
        Ok(payload) => {
            if !payload.additional.is_empty() {
                ctx.raw(
                    "distribution_points",
                    "unexpected_shape",
                    &format!("payload carried undeclared top-level keys: {}", key_list(&payload.additional)),
                    &body,
                );
            }
            for (_, item, why) in &payload.rejected {
                ctx.raw("distribution_points", "unexpected_shape", why, item.as_bytes());
            }

            let mut rows = Vec::new();
            for (series, at) in payload.series.iter().zip(&payload.positions) {
                if !series.kind.is_empty() && series.kind != "distribution" {
                    // Noted, and the points still become rows: a typo in that one word must not
                    // cost a host its distribution.
                    ctx.raw(
                        "distribution_points",
                        "unexpected_shape",
                        &format!("series[{at}].type: expected \"distribution\", got \"{}\"", series.kind),
                        series.raw.as_bytes(),
                    );
                }
                let extra = extra_encoded(&[], &series.additional);
                let tags = tags::to_multi_map(&series.tags);

                for (j, point) in series.points.iter().enumerate() {
                    let unfit = point.items.contains(&DistributionItem::Unfit);
                    match distribution_point(&point.items) {
                        Some((timestamp, values)) if !unfit => {
                            let sketch = ddsketch::build(&values);
                            rows.push(SketchRow {
                                tenant_id: ctx.tenant().into(),
                                timestamp,
                                metric: series.metric.clone(),
                                host: series.host.clone(),
                                tags: tags.clone(),
                                count: sketch.stats.count as u64,
                                min: sketch.stats.min,
                                max: sketch.stats.max,
                                avg: sketch.stats.avg,
                                sum: sketch.stats.sum,
                                bucket_keys: sketch.keys,
                                bucket_counts: sketch.counts,
                                extra: extra.clone(),
                                ..Default::default()
                            });
                        }
                        _ => ctx.raw(
                            "distribution_points",
                            "unexpected_shape",
                            &format!("series[{at}].points[{j}]: expected a [timestamp, [values]] pair"),
                            point.raw.as_bytes(),
                        ),
                    }
                }
            }
            ctx.write(rows);
        }
    }
    ack_series()
}

// ---------------------------------------------------------------------------
// Service checks
// ---------------------------------------------------------------------------

/// /api/v1/check_run. A check answers "is this thing working", not "how much of it is there";
/// monitors of the "service is down" kind are built on these.
pub async fn handle_check_run(ctx: Ctx, body: Bytes) -> Response {
    if ctx.is_sweep() {
        return ack_series();
    }
    match parse_check_runs(&body) {
        Err(error) => ctx.raw("check_run", "decode_error", &error, &body),
        Ok(checks) => {
            for (_, item, why) in &checks.rejected {
                ctx.raw("check_run", "unexpected_shape", why, item.as_bytes());
            }
            let rows = checks
                .runs
                .iter()
                .map(|run| CheckRun {
                    tenant_id: ctx.tenant().into(),
                    timestamp: time::wire_seconds(run.timestamp),
                    check_name: run.check.clone(),
                    host: run.host_name.clone(),
                    status: CheckStatus::of(i64::from(run.status)),
                    message: run.message.clone(),
                    tags: tags::to_multi_map(&run.tags),
                    extra: extra_encoded(&[], &run.additional),
                })
                .collect();
            ctx.write(rows);
        }
    }
    ack_series()
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Anything outside Datadog's vocabulary is coerced rather than rejected: dropping an event
/// because someone invented an alert type would lose the one thing we were asked to remember.
/// What the sender wrote is kept beside the coerced value.
pub fn coerce_alert_type(raw: &str) -> String {
    let word = raw.to_lowercase();
    if EVENT_ALERT_TYPES.contains(&word.as_str()) { word } else { "info".into() }
}

pub fn coerce_priority(raw: &str) -> String {
    let word = raw.to_lowercase();
    if EVENT_PRIORITIES.contains(&word.as_str()) { word } else { "normal".into() }
}

/// A new event id, and the numeric form the API hands back to the caller: the id's first eight
/// bytes, shifted to stay inside an i64.
fn new_event_id() -> (Uuid, u64) {
    let id = Uuid::new_v4();
    (id, id.as_u64_pair().0 >> 1)
}

/// POST /api/v1/events. Unlike every other endpoint this one MUST return a body: the client reads
/// the reply into EventCreateResponse and keeps the id.
pub async fn handle_events(ctx: Ctx, body: Bytes) -> Response {
    if ctx.is_sweep() {
        return json(StatusCode::ACCEPTED, &json!({ "status": "ok" }));
    }
    let events = match parse_events(&body) {
        Err(error) => {
            ctx.raw("events", "decode_error", &error, &body);
            return json(StatusCode::BAD_REQUEST, &json!({ "status": "error", "error": error }));
        }
        Ok(events) if events.is_empty() => {
            return json(StatusCode::BAD_REQUEST, &json!({ "status": "error", "error": "no event with a title" }));
        }
        Ok(events) => events,
    };

    // Every event gets an id; the reply carries the first one's.
    let ids: Vec<(Uuid, u64)> = events.iter().map(|_| new_event_id()).collect();
    let rows = events
        .iter()
        .zip(&ids)
        .map(|(event, (id, number))| Event {
            tenant_id: ctx.tenant().into(),
            timestamp: time::wire_seconds(event.date_happened),
            event_id: *id,
            event_id_num: *number,
            title: event.title.clone(),
            text: event.text.clone(),
            host: event.host.clone(),
            alert_type: coerce_alert_type(&event.alert_type),
            priority: coerce_priority(event.priority.as_deref().unwrap_or_default()),
            aggregation_key: event.aggregation_key.clone(),
            source_type_name: event.source_type_name.clone(),
            device_name: event.device_name.clone(),
            tags: tags::to_multi_map(event.tags.iter().flatten()),
            event_type: String::new(),
            related_event_id: event.related_event_id,
            alert_type_raw: event.alert_type.clone(),
            priority_raw: event.priority.clone().unwrap_or_default(),
            extra: extra_encoded(&[], &event.additional),
        })
        .collect();
    ctx.write(rows);

    let first = &events[0];
    let first_id = ids[0].1;
    let reply = json!({
        "status": "ok",
        "event": {
            "id": first_id,
            "id_str": first_id.to_string(),
            "title": first.title,
            "text": first.text,
            "date_happened": time::wire_seconds(first.date_happened).timestamp(),
            "host": first.host,
            "alert_type": coerce_alert_type(&first.alert_type),
            "priority": coerce_priority(first.priority.as_deref().unwrap_or_default()),
            "tags": first.tags,
            "source_type_name": first.source_type_name,
            "device_name": first.device_name,
            "url": "",
        }
    });
    json(StatusCode::ACCEPTED, &reply)
}

// ---------------------------------------------------------------------------
// /intake/: the Agent v5 endpoint
// ---------------------------------------------------------------------------

/// One agent event (pkg/metrics/event.Event) as the shared events row. The agent's names differ
/// from the public API's for the same three things: msg_title, msg_text and timestamp against
/// title, text and date_happened.
///
/// `source` is the key the agent grouped the event under and stands in when the event does not
/// repeat it; likewise the payload's host stands in for an event with none of its own.
pub fn intake_event_row(tenant: &str, source: &str, payload_host: &str, event: &Value) -> Event {
    let fields = json::members(event);
    let (id, number) = new_event_id();
    let alert_raw = json::lenient_string(fields.get("alert_type"));
    let priority_raw = json::lenient_string(fields.get("priority"));
    let event_tags: Vec<String> = match fields.get("tags") {
        Some(Value::Array(list)) => list.iter().filter_map(json::string_of).collect(),
        _ => Vec::new(),
    };
    Event {
        tenant_id: tenant.into(),
        timestamp: time::wire_seconds(json::lenient_int64(fields.get("timestamp"))),
        event_id: id,
        event_id_num: number,
        title: json::lenient_string(fields.get("msg_title")),
        text: json::lenient_string(fields.get("msg_text")),
        host: text::first_non_empty(&[&json::lenient_string(fields.get("host")), payload_host]).into(),
        alert_type: coerce_alert_type(&alert_raw),
        priority: coerce_priority(&priority_raw),
        aggregation_key: json::lenient_string(fields.get("aggregation_key")),
        source_type_name: text::first_non_empty(&[&json::lenient_string(fields.get("source_type_name")), source])
            .into(),
        device_name: String::new(),
        tags: tags::to_multi_map(&event_tags),
        event_type: json::lenient_string(fields.get("event_type")),
        related_event_id: None,
        alert_type_raw: alert_raw,
        priority_raw,
        extra: extra_encoded(
            &[
                "msg_title",
                "msg_text",
                "timestamp",
                "host",
                "alert_type",
                "priority",
                "aggregation_key",
                "source_type_name",
                "event_type",
                "tags",
            ],
            &fields,
        ),
    }
}

/// `events` as source → its events, or what does not fit that shape: an object of lists of
/// objects. A null event is an empty one.
fn events_by_source(events: &Value) -> Result<BTreeMap<String, Vec<Value>>, String> {
    match events {
        Value::Null => Ok(BTreeMap::new()),
        Value::Object(sources) => {
            for (source, list) in sources {
                match list {
                    Value::Null => {}
                    Value::Array(items) => {
                        if let Some((i, event)) = items.iter().enumerate().find(|(_, e)| !e.is_object() && !e.is_null())
                        {
                            return Err(format!("events.{source}[{i}]: expected an object, got {}", json::kind(event)));
                        }
                    }
                    other => return Err(format!("events.{source}: expected a list, got {}", json::kind(other))),
                }
            }
            Ok(json::members(events)
                .into_iter()
                .map(|(source, list)| (source, list.as_array().cloned().unwrap_or_default()))
                .collect())
        }
        other => Err(format!("events: expected an object, got {}", json::kind(other))),
    }
}

/// The events variant: {"events": {<source>: [...]}, "internalHostname"}. The rows go to the same
/// table as /api/v1/events.
fn intake_events(ctx: &Ctx, envelope: &BTreeMap<String, Value>, sent: &Sent) {
    let events = &envelope["events"];
    match events_by_source(events) {
        Err(error) => ctx.raw("intake", "unexpected_shape", &error, sent_text(sent, "events").as_bytes()),
        Ok(by_source) => {
            let host = json::lenient_string(envelope.get("internalHostname"));
            // By source in sorted order, so two replays of one payload give the same rows.
            let rows = by_source
                .iter()
                .flat_map(|(source, list)| {
                    list.iter().map(|event| intake_event_row(ctx.tenant(), source, &host, event))
                })
                .collect();
            ctx.write(rows);
        }
    }
}

/// One positional agent_checks entry: 0 check name, 1 source type, 2 instance id, 3 status,
/// 4 message. A shorter array is normal, which is why status is optional: a missing position 3
/// must not read as 0, which means OK. Anything after position 4 is kept as a JSON array, so the
/// positions survive.
pub fn agent_check_row(
    tenant: &str,
    at: DateTime<Utc>,
    host: &str,
    version: &str,
    uuid: &str,
    meta: &str,
    positional: &[Value],
) -> AgentCheck {
    let text = |i: usize| positional.get(i).map(|v| json::lenient_string(Some(v))).unwrap_or_default();
    AgentCheck {
        tenant_id: tenant.into(),
        received_at: at,
        hostname: host.into(),
        agent_version: version.into(),
        uuid: uuid.into(),
        check_name: text(0),
        source_type: text(1),
        instance_id: text(2),
        status: positional.get(3).and_then(json::int64_of),
        message: text(4),
        positional_extra: if positional.len() > 5 { json::compact_array(&positional[5..]) } else { String::new() },
        meta: meta.into(),
    }
}

/// The agent_checks variant (the V5 collector's payload): check statuses as positional arrays,
/// and external_host_tags, [[hostname, {source: [tags]}]]: tags for hosts the agent is NOT
/// running on, which is why they get a table of their own.
fn intake_agent_checks(ctx: &Ctx, envelope: &BTreeMap<String, Value>, sent: &Sent) {
    let elements = |name: &str| sent.get(name).map(|t| json::raw::elements(t)).unwrap_or_default();
    let check_texts = elements("agent_checks");
    let tag_texts = elements("external_host_tags");
    let checks = json::lenient_items(envelope.get("agent_checks"));
    let external_tags: Vec<Option<(String, Tags)>> = json::lenient_items(envelope.get("external_host_tags"))
        .iter()
        .enumerate()
        .map(|(i, entry)| match entry.as_array() {
            Some(pair) if pair.len() == 2 => {
                Some((json::lenient_string(Some(&pair[0])), json::lenient_string_lists(Some(&pair[1]))))
            }
            _ => {
                ctx.raw(
                    "intake",
                    "unexpected_shape",
                    &format!("external_host_tags[{i}] is not a [hostname, tags] pair"),
                    tag_texts.get(i).copied().unwrap_or_default().as_bytes(),
                );
                None
            }
        })
        .collect();

    let now = Utc::now();
    let host = json::lenient_string(envelope.get("internalHostname"));
    let version = json::lenient_string(envelope.get("agentVersion"));
    let uuid = json::lenient_string(envelope.get("uuid"));
    let meta = sent_text(sent, "meta");
    let mut check_rows = Vec::new();
    for (i, check) in checks.iter().enumerate() {
        match check.as_array() {
            Some(positional) => {
                check_rows.push(agent_check_row(ctx.tenant(), now, &host, &version, &uuid, &meta, positional))
            }
            None => ctx.raw(
                "intake",
                "unexpected_shape",
                &format!("agent_checks[{i}] is not a positional array"),
                check_texts.get(i).copied().unwrap_or_default().as_bytes(),
            ),
        }
    }
    ctx.write(check_rows);

    // One row per SOURCE: a host carries several at once, and flattening them would lose which
    // of them said what.
    let tag_rows = external_tags
        .into_iter()
        .flatten()
        .flat_map(|(described_host, by_source)| {
            by_source.into_iter().map(move |(source, list)| ExternalHostTags {
                tenant_id: ctx.tenant().into(),
                received_at: now,
                host: described_host.clone(),
                source,
                tags: tags::to_multi_map(&list),
            })
        })
        .collect();
    ctx.write(tag_rows);
}

/// The envelope keys host_row reads by name; the rest go to intake_extra. apiKey is in the list so
/// that a credential cannot reach a column by way of a catch-all.
const HOST_KNOWN_KEYS: [&str; 20] = [
    "apiKey",
    "internalHostname",
    "agentVersion",
    "os",
    "uuid",
    "agent-flavor",
    "python",
    "gohai",
    "host-tags",
    "systemStats",
    "meta",
    "network",
    "logs",
    "install-method",
    "proxy-info",
    "otlp",
    "container-meta",
    "fips_mode",
    "fips_proxy_enabled",
    "resources",
];

/// The gohai sections with columns of their own. gohai.network is not one of them: the envelope's
/// own `network` (the host's addresses) holds the column of that name, and gohai's interface
/// inventory keeps its name in gohai_extra.
const GOHAI_KNOWN_SECTIONS: [&str; 4] = ["platform", "cpu", "memory", "filesystem"];

/// The hosts row of an /intake/ host envelope.
///
///   internalHostname, agentVersion, os   columns
///   gohai (a STRING holding JSON)        platform, cpu, memory: columns;
///                                        filesystem: its JSON; the rest: gohai_extra
///   host-tags                            every source → host_tags; the
///                                        "system" source also fills tags
///   meta, network, logs, otlp, resources their JSON text, as sent
///   systemStats, install-method, proxy-info, container-meta
///                                        flat maps, each value its JSON text as sent
///   apiKey                               the real key: never stored
///   anything else                        intake_extra
#[allow(clippy::too_many_arguments)]
pub fn host_row(
    tenant: &str,
    hostname: &str,
    agent_version: &str,
    os_name: &str,
    seen_at: DateTime<Utc>,
    envelope: &BTreeMap<String, Value>,
    envelope_text: &str,
    gohai: &BTreeMap<String, Value>,
    gohai_text: &str,
    host_tags: BTreeMap<String, Vec<String>>,
) -> Host {
    // gohai reports every value as a string, even numbers. A section that is absent or shaped
    // differently is left empty rather than failing the row.
    let section = |name: &str| gohai.get(name).and_then(json::string_map_of).unwrap_or_default();
    let sent = json::raw::members(envelope_text);
    let gohai_sent = json::raw::members(gohai_text);
    let object_map = |name: &str| sent.get(name).map(|t| extra_sent(&[], t)).unwrap_or_default();
    Host {
        tenant_id: tenant.into(),
        host: hostname.into(),
        seen_at,
        agent_version: agent_version.into(),
        os: os_name.into(),
        platform: section("platform"),
        cpu: section("cpu"),
        memory: section("memory"),
        tags: tags::to_multi_map(host_tags.get("system").into_iter().flatten()),
        uuid: json::lenient_string(envelope.get("uuid")),
        agent_flavor: json::lenient_string(envelope.get("agent-flavor")),
        python_version: json::lenient_string(envelope.get("python")),
        meta: sent_text(&sent, "meta"),
        network: sent_text(&sent, "network"),
        filesystem: sent_text(&gohai_sent, "filesystem"),
        logs: sent_text(&sent, "logs"),
        otlp: sent_text(&sent, "otlp"),
        system_stats: object_map("systemStats"),
        install_method: object_map("install-method"),
        proxy_info: object_map("proxy-info"),
        container_meta: object_map("container-meta"),
        fips_mode: json::flag(envelope.get("fips_mode")),
        fips_proxy_enabled: json::flag(envelope.get("fips_proxy_enabled")),
        host_tags,
        gohai_extra: extra_sent(&GOHAI_KNOWN_SECTIONS, gohai_text),
        intake_extra: extra_sent(&HOST_KNOWN_KEYS, envelope_text),
        resources: sent_text(&sent, "resources"),
    }
}

/// The host metadata variant. The payload is loose and shifts between agent versions, so fields
/// are read by name.
fn intake_host(ctx: &Ctx, envelope: &BTreeMap<String, Value>, envelope_text: &str) {
    let hostname = json::lenient_string(envelope.get("internalHostname"));
    if hostname.is_empty() {
        return;
    }
    // The text of gohai's own document, which the envelope carries as a string.
    let gohai_text = json::lenient_string(envelope.get("gohai"));
    let gohai = match gohai_text.as_str() {
        "" => BTreeMap::new(),
        inner => json::try_parse(inner.as_bytes()).map(|sections| json::members(&sections)).unwrap_or_default(),
    };
    let row = host_row(
        ctx.tenant(),
        &hostname,
        &json::lenient_string(envelope.get("agentVersion")),
        &json::lenient_string(envelope.get("os")),
        Utc::now(),
        envelope,
        envelope_text,
        &gohai,
        &gohai_text,
        json::lenient_string_lists(envelope.get("host-tags")),
    );
    ctx.write(vec![row]);
}

/// One row of a resources snapshot, as gohai writes it:
/// [usernames, cpu %, memory %, vms, rss, name, number of pids] (pkg/gohai/processes). None when
/// it is not that.
fn process_group(tenant: &str, host: &str, at: DateTime<Utc>, fields: &Value) -> Option<ProcessGroup> {
    let field = fields.as_array().filter(|f| f.len() == 7)?;
    let usernames = json::string_of(&field[0])?;
    let cpu = json::float_of(&field[1])?;
    let mem = json::float_of(&field[2])?;
    let name = json::string_of(&field[5])?;
    let unsigned = |i: usize| field[i].as_u64().unwrap_or(0);
    Some(ProcessGroup {
        tenant_id: tenant.into(),
        timestamp: at,
        host: host.into(),
        name,
        // The agent joins them with commas.
        usernames: if usernames.is_empty() { Vec::new() } else { usernames.split(',').map(String::from).collect() },
        process_count: unsigned(6) as u32,
        cpu_pct: cpu,
        mem_pct: mem,
        vms: unsigned(3),
        rss: unsigned(4),
    })
}

/// {"meta":{"host":…},"processes":{"snaps":[[unix seconds, [row, …]], …]}}: the processes of the
/// host grouped by name, sent with the host metadata. Whatever of it is not that shape keeps the
/// body raw, once.
fn intake_resources(ctx: &Ctx, body: &[u8], resources: &Value) {
    let host = json::at(&["meta", "host"], resources).and_then(json::string_of).unwrap_or_default();
    let Some(snaps) = json::at(&["processes", "snaps"], resources).and_then(Value::as_array) else {
        ctx.raw("intake", "unexpected_shape", "resources without processes.snaps", body);
        return;
    };
    let mut rows = Vec::new();
    let mut unread = 0;
    for snap in snaps {
        let groups = match snap.as_array() {
            Some(pair) if pair.len() == 2 && pair[1].is_array() => {
                json::int64_of(&pair[0]).map(|seconds| (time::from_unix_seconds(seconds), &pair[1]))
            }
            _ => None,
        };
        match groups {
            None => unread += 1,
            Some((at, list)) => {
                for fields in list.as_array().into_iter().flatten() {
                    match process_group(ctx.tenant(), &host, at, fields) {
                        Some(row) => rows.push(row),
                        None => unread += 1,
                    }
                }
            }
        }
    }
    if unread > 0 {
        ctx.raw(
            "intake",
            "unexpected_shape",
            &format!("resources: {unread} snapshots or rows are not the resources check's shape"),
            body,
        );
    }
    ctx.write(rows);
}

/// Four producers share /intake/ and nothing but the path; they are told apart by their top-level
/// keys, in this order:
///
///   events                 agent events, the shutdown event among them
///   agent_checks           the V5 collector: check statuses, external host tags
///   gohai or systemStats   host metadata
///   resources alone        the resources check's process groups
fn decode_intake(ctx: &Ctx, body: &[u8]) {
    let root = match json::try_parse(body) {
        Err(error) => return ctx.raw("intake", "decode_error", &error, body),
        Ok(root) if !root.is_object() && !root.is_null() => {
            return ctx.raw("intake", "decode_error", &format!("expected an object, got {}", json::kind(&root)), body);
        }
        Ok(root) => root,
    };
    let envelope = json::members(&root);
    let text = json::raw::text(body);
    let sent = json::raw::members(&text);
    if envelope.contains_key("events") {
        intake_events(ctx, &envelope, &sent);
    } else if envelope.contains_key("agent_checks") {
        intake_agent_checks(ctx, &envelope, &sent);
    } else if envelope.contains_key("gohai") || envelope.contains_key("systemStats") {
        intake_host(ctx, &envelope, &text);
    } else if envelope.contains_key("resources") && envelope.len() == 1 {
        intake_resources(ctx, body, &envelope["resources"]);
    } else {
        ctx.raw(
            "intake",
            "unexpected_shape",
            &format!("no known /intake/ variant key, keys: {}", key_list(&envelope)),
            body,
        );
    }
}

pub async fn handle_intake(ctx: Ctx, body: Bytes) -> Response {
    if !ctx.is_sweep() {
        decode_intake(&ctx, &body);
    }
    status_ok()
}

// ---------------------------------------------------------------------------
// /api/v1/metadata
// ---------------------------------------------------------------------------

/// The variant keys of /api/v1/metadata, one per producer (comp/metadata/*).
const METADATA_VARIANTS: [&str; 13] = [
    "agent_metadata",
    "host_metadata",
    "check_metadata",
    "logs_metadata",
    "files_metadata",
    "system_probe_metadata",
    "security_agent_metadata",
    "signing_metadata",
    "host_system_info_metadata",
    "host_gpu_metadata",
    "ha_agent_metadata",
    "datadog_cluster_agent_metadata",
    "clustercheck_metadata",
];

const METADATA_ENVELOPE_KEYS: [&str; 5] = ["hostname", "clustername", "cluster_id", "timestamp", "uuid"];

fn store_metadata(ctx: &Ctx, body: &[u8]) {
    let root = match json::try_parse(body) {
        Err(error) => return ctx.raw("metadata", "decode_error", &error, body),
        Ok(root) if !root.is_object() && !root.is_null() => {
            return ctx.raw(
                "metadata",
                "decode_error",
                &format!("expected an object, got {}", json::kind(&root)),
                body,
            );
        }
        Ok(root) => root,
    };
    let envelope = json::members(&root);
    let found: Vec<&str> = METADATA_VARIANTS.iter().copied().filter(|v| envelope.contains_key(*v)).collect();
    if found.is_empty() {
        // A producer we have never seen, or a variant key renamed between releases.
        return ctx.raw(
            "metadata",
            "no_schema",
            &format!("no known /api/v1/metadata variant key, keys: {}", key_list(&envelope)),
            body,
        );
    }
    let timestamp = json::lenient_int64(json::field("timestamp", &root));
    let now = Utc::now();
    // Every key that is neither the shared envelope nor a variant of this request:
    // clustercheck_status and its sibling among them.
    let skip: Vec<&str> = METADATA_ENVELOPE_KEYS.iter().copied().chain(found.iter().copied()).collect();
    let text = json::raw::text(body);
    let sent = json::raw::members(&text);
    let extra = extra_sent(&skip, &text);

    // One row per variant key present: `variant` stays a real filter.
    let rows = found
        .iter()
        .map(|variant| AgentMetadata {
            tenant_id: ctx.tenant().into(),
            received_at: now,
            variant: variant.to_string(),
            hostname: json::lenient_string(json::field("hostname", &root)),
            cluster_name: json::lenient_string(json::field("clustername", &root)),
            cluster_id: json::lenient_string(json::field("cluster_id", &root)),
            timestamp: (timestamp > 0).then(|| time::from_unix_seconds(timestamp)),
            uuid: json::lenient_string(json::field("uuid", &root)),
            payload: sent_text(&sent, variant),
            envelope_extra: extra.clone(),
        })
        .collect();
    ctx.write(rows);
}

/// /api/v1/metadata: the inventory payloads. Each is an envelope of hostname, timestamp and uuid
/// plus ONE variant key; the cluster-agent's carry clustername and cluster_id instead of hostname.
/// None of the variants has a published type, so each is stored as its JSON under its key.
pub async fn handle_metadata(ctx: Ctx, body: Bytes) -> Response {
    if !ctx.is_sweep() {
        store_metadata(&ctx, &body);
    }
    status_ok()
}

// ---------------------------------------------------------------------------
// Endpoints the agent reads from: each needs a real answer, not a 202
// ---------------------------------------------------------------------------

/// GET /api/v2/validate: the trace-agent asks which org its key belongs to (pkg/trace/api/opm.go)
/// and wants {"data":{"id":"<org UUID>"}}. It hashes the id into the Org Propagation Marker that
/// tracers attach to the trace context they pass on, so the id must be the same on every call:
/// here it is derived from the tenant. Two installations that both use the tenant "default"
/// therefore share a marker. After four failures the agent leaves the marker unset for the life of
/// the process.
pub async fn handle_opm_validate(ctx: Ctx, _: Bytes) -> Response {
    let digest = Sha256::digest(format!("ninjacat-org:{}", ctx.tenant()).as_bytes());
    // The bytes as .NET's Guid read them, mixed-endian, as the F# server answered.
    let id = Uuid::from_bytes_le(digest[..16].try_into().unwrap());
    json(StatusCode::OK, &json!({ "data": { "id": id.to_string() } }))
}

/// The SHA-256 of a delegated-auth proof, hex. The proof never leaves here.
fn proof_fingerprint(proof: &str) -> String {
    if proof.is_empty() {
        String::new()
    } else {
        Sha256::digest(proof.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
    }
}

/// POST /api/v2/intake-key: the delegated-auth exchange. The agent sends an empty body with
/// `Authorization: Delegated <proof>` and reads {"data":{"attributes":{"api_key":"…"}}}; an empty
/// api_key is an error on its side. The key that admitted the request is what it gets back,
/// whether it came in the header or in the query. The proof IS the credential, so only its
/// fingerprint is stored.
pub async fn handle_intake_key(ctx: Ctx, _: Bytes) -> Response {
    let authorization = ctx.header("authorization");
    let (scheme, proof) = authorization.split_once(' ').unwrap_or((authorization, ""));
    // The key the request was admitted with, as the authentication left it.
    if let Some(key_id) = ctx.key_id() {
        ctx.write(vec![DelegatedAuth {
            tenant_id: ctx.tenant().into(),
            at: Utc::now(),
            scheme: scheme.into(),
            proof_fingerprint: proof_fingerprint(proof),
            api_key_id: key_id.into(),
        }]);
    }
    let query_key = ctx.query("api_key");
    let admitted_with = text::first_non_empty(&[ctx.header("dd-api-key"), &query_key]);
    json(StatusCode::OK, &json!({ "data": { "attributes": { "api_key": admitted_with } } }))
}

/// GET /api/v1/query, for the cluster-agent's External Metrics Provider. Nothing is queryable here
/// yet, so the answer is a well-formed EMPTY result: it marks each metric invalid on the agent's
/// side, which is the truth, where a 202 would count as an API error.
pub async fn handle_query(ctx: Ctx, _: Bytes) -> Response {
    json(
        StatusCode::OK,
        &json!({ "status": "ok", "res_type": "time_series", "query": ctx.query("query"), "series": [] }),
    )
}

/// A single-resource JSON:API document, {"data":{"type","id","attributes"}}, read no further than
/// the envelope: the concrete types belong to the agent.
struct JsonApiResource {
    id: String,
    attributes: BTreeMap<String, Value>,
    /// The attributes object as it was sent, "" for none.
    attributes_text: String,
}

impl JsonApiResource {
    fn text(&self, name: &str) -> String {
        json::lenient_string(self.attributes.get(name))
    }

    /// An attribute's JSON text as sent; "" when it was not sent.
    fn sent(&self, name: &str) -> String {
        json::raw::member(name, &self.attributes_text).unwrap_or_default().into()
    }
}

/// None when the body is not a JSON:API document.
fn try_json_api(body: &[u8]) -> Option<JsonApiResource> {
    let root = json::try_parse_object(body)?;
    let data = root.get("data").filter(|d| d.is_object() || d.is_null())?;
    let fields = json::members(data);
    let text = json::raw::text(body);
    let data_text = json::raw::member("data", &text).unwrap_or_default();
    Some(JsonApiResource {
        id: json::lenient_string(fields.get("id")),
        attributes: json::lenient_members(fields.get("attributes")),
        attributes_text: json::raw::member("attributes", data_text).unwrap_or_default().into(),
    })
}

/// The request's JSON:API resource. A body that is not one is kept raw, with `note`, and gives
/// None.
fn decode_json_api(ctx: &Ctx, body: &[u8], intake: &str, note: &str) -> Option<JsonApiResource> {
    let resource = try_json_api(body);
    if resource.is_none() {
        ctx.raw(intake, "decode_error", note, body);
    }
    resource
}

/// POST /api/v2/profiles/symbols/query, from the host profiler's symbol uploader. The reply lists
/// the build ids already held, and the uploader sends only what is missing; it reads the reply as
/// a JSON:API array, so anything else stops all uploads. We hold nothing, so the array is empty.
/// The question is kept: these build ids are the profiled binaries of the fleet.
pub async fn handle_symbols_query(ctx: Ctx, body: Bytes) -> Response {
    if let Some(resource) = decode_json_api(&ctx, &body, "symbols", "not a JSON:API document") {
        ctx.write(vec![SymbolQuery {
            tenant_id: ctx.tenant().into(),
            at: Utc::now(),
            arch: resource.text("arch"),
            build_ids: json::lenient_strings(resource.attributes.get("buildIds")).unwrap_or_default(),
            resource: text::utf8(&body),
        }]);
    }
    json(StatusCode::OK, &json!({ "data": [] }))
}

// ---------------------------------------------------------------------------
// Private Action Runner
// ---------------------------------------------------------------------------
//
// A pull loop steered entirely by our answers: enrollment hands out the identity every later
// request is signed with, dequeue hands out work, health-check hands out timing. Its request
// types are internal to the agent, so decoding stops at the JSON:API envelope and the attribute
// keys.

/// The numeric org_id the runner insists on, derived from the tenant (FNV-64a): stable across
/// restarts without a table, never zero, inside the i64 the runner parses it into.
pub fn tenant_org_id(tenant: &str) -> i64 {
    ((fnv::hash64(tenant.as_bytes()) >> 1) as i64) | 1
}

/// POST /api/unstable/on_prem_runners and its api_key_only variant.
///
/// The runner builds urn:dd:apps:on-prem-runner:<region>:<org_id>:<runner_id> from the reply,
/// splits it on ":" and signs every later request with the pair, so runner_id must not contain a
/// colon. The status must be exactly 200 (anything else is retried forever) and the reply's type
/// must be "createRunnerResponse".
pub async fn handle_runner_enroll(ctx: Ctx, body: Bytes) -> Response {
    let Some(resource) = decode_json_api(&ctx, &body, "par", "enroll: not a JSON:API document") else {
        return status(StatusCode::BAD_REQUEST);
    };
    let modes = json::lenient_strings(resource.attributes.get("runner_modes"));
    let runner_id = Uuid::new_v4().to_string();
    let org_id = tenant_org_id(ctx.tenant());
    ctx.write(vec![RunnerEnrollment {
        tenant_id: ctx.tenant().into(),
        runner_id: runner_id.clone(),
        enrolled_at: Utc::now(),
        name: resource.text("runner_name"),
        modes: modes.clone().unwrap_or_default(),
        host: resource.text("runner_host"),
        public_key_pem: resource.text("public_key_pem"),
        agent_hostname: resource.text("agent_hostname"),
        orch_cluster_id: resource.text("orch_cluster_id"),
        agent_flavor: resource.text("agent_flavor"),
        org_id,
        attributes: column::json_object(&json::compact_object(&resource.attributes)),
    }]);
    let reply = json!({
        "data": {
            "type": "createRunnerResponse",
            "id": runner_id,
            "attributes": {
                "runner_id": runner_id,
                "org_id": org_id,
                "runner_modes": modes,
                "agent_hostname": resource.text("agent_hostname"),
                "orch_cluster_id": resource.text("orch_cluster_id"),
                "agent_flavor": resource.text("agent_flavor"),
            }
        }
    });
    // JSON:API's own media type.
    json_as(StatusCode::OK, "application/vnd.api+json", &reply)
}

/// The runner's poll for work. An EMPTY 200 means "no task" and is the normal idle answer: a body
/// would be parsed as a task, and any other status is an error on the runner's side. Every poll is
/// also the only liveness signal a runner with no tasks produces.
pub async fn handle_runner_dequeue(ctx: Ctx, body: Bytes) -> Response {
    if let Some(resource) = decode_json_api(&ctx, &body, "par", "dequeue: not a JSON:API document") {
        // Both timestamps stay strings: their format is undocumented and an unparseable value
        // must not become 1970.
        ctx.write(vec![RunnerDequeue {
            tenant_id: ctx.tenant().into(),
            at: Utc::now(),
            runner_started_at: resource.text("runner_started_at"),
            last_task_received_at: resource.text("last_task_received_at"),
            version: ctx.header("x-datadog-onprem-version").into(),
            modes: ctx.header("x-datadog-onprem-modes").into(),
        }]);
    }
    status(StatusCode::OK)
}

/// publish-task-update: the outcome of a task. The document id is "succeed_task" or "fail_task";
/// `payload.outputs` is the action's own result, arbitrary JSON of any size. The runner accepts
/// any status.
pub async fn handle_runner_task_update(ctx: Ctx, body: Bytes) -> Response {
    if let Some(resource) = decode_json_api(&ctx, &body, "par", "task-update: not a JSON:API document") {
        let payload = json::lenient_members(resource.attributes.get("payload"));
        let payload_text = resource.sent("payload");
        // A numeric enum on the runner's side, and a succeeded task sends none: absent must not
        // become a 0 that reads as an error code.
        let error_code = match payload.get("error_code") {
            Some(Value::Null) => Some(0),
            Some(code) => json::int64_of(code),
            None => None,
        };
        ctx.write(vec![RunnerTaskUpdate {
            tenant_id: ctx.tenant().into(),
            at: Utc::now(),
            outcome: resource.id.clone(),
            task_id: resource.text("task_id"),
            action_fqn: resource.text("action_fqn"),
            job_id: resource.text("job_id"),
            client: resource.sent("client"),
            branch: json::lenient_string(payload.get("branch")),
            outputs: json::raw::member("outputs", &payload_text).unwrap_or_default().into(),
            error_code,
            error_details: json::lenient_string(payload.get("error_details")),
            api_error: json::lenient_string(payload.get("api_error")),
            extra: merge_extra(
                extra_sent(&["task_id", "client", "action_fqn", "job_id", "payload"], &resource.attributes_text),
                "payload.",
                extra_sent(&["branch", "outputs", "error_code", "error_details", "api_error"], &payload_text),
            ),
        }]);
    }
    accepted()
}

/// The per-task heartbeat. It must be answered with exactly 200: a 404 tells the runner the job is
/// gone and it stops heartbeating. The rows answer "when did this job stop making progress": a job
/// that hangs never sends a task update.
pub async fn handle_runner_heartbeat(ctx: Ctx, body: Bytes) -> Response {
    if let Some(resource) = decode_json_api(&ctx, &body, "par", "heartbeat: not a JSON:API document") {
        ctx.write(vec![RunnerHeartbeat {
            tenant_id: ctx.tenant().into(),
            at: Utc::now(),
            task_id: resource.text("task_id"),
            action_fqn: resource.text("action_fqn"),
            job_id: resource.text("job_id"),
            client: resource.sent("client"),
        }]);
    }
    json(StatusCode::OK, &json!({}))
}

/// GET runner/health-check. The runner ignores the body and reads X-Server-Time; with no
/// X-Retry-After-Ms it polls at its default interval.
pub async fn handle_runner_health_check(_: Ctx, _: Bytes) -> Response {
    let mut response = json(StatusCode::OK, &json!({}));
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    response.headers_mut().insert("x-server-time", HeaderValue::from_str(&now).unwrap());
    response
}

/// POST /api/v2/actions/connections: a connection a runner may use to reach a third-party system.
/// SENSITIVE: `integration.credentials` holds secrets and is stored verbatim, because a connection
/// without them cannot be used. It must never be logged.
pub async fn handle_action_connections(ctx: Ctx, body: Bytes) -> Response {
    if let Some(resource) = decode_json_api(&ctx, &body, "par", "connections: not a JSON:API document") {
        let integration = json::lenient_members(resource.attributes.get("integration"));
        let integration_text = resource.sent("integration");
        let connection_tags = json::lenient_strings(resource.attributes.get("tags")).unwrap_or_default();
        ctx.write(vec![ActionConnection {
            tenant_id: ctx.tenant().into(),
            at: Utc::now(),
            name: resource.text("name"),
            runner_id: resource.text("runner_id"),
            tags: tags::to_multi_map(&connection_tags),
            integration_type: json::lenient_string(integration.get("type")),
            credentials: json::raw::member("credentials", &integration_text).unwrap_or_default().into(),
            extra: merge_extra(
                extra_sent(&["name", "runner_id", "tags", "integration"], &resource.attributes_text),
                "integration.",
                extra_sent(&["type", "credentials"], &integration_text),
            ),
        }]);
    }
    accepted()
}
