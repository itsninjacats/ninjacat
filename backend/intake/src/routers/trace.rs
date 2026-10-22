//! trace.agent.<site>: the trace-agent.
//!
//!   agent config: apm_config.apm_dd_url
//!
//! A separate binary from the node agent, which is why dd_url does not move it.
//!
//!   /api/v0.2/traces               -> spans                    one row per span
//!   /api/v0.2/stats                -> apm_stats                one row per grouped stat
//!   /api/v0.1/pipeline_stats       -> dsm_pipeline_stats       one row per StatsPoint
//!                                     dsm_backlogs             one row per Backlog
//!                                     dsm_bucket_transactions  one row per bucket with blobs
//!   /api/v2/data_streams_messages  -> dsm_messages             one row per array element
//!
//! A body that does not decode goes to raw_payloads under the intake "trace".
//! Not handled: /api/v2/apmtelemetry.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use base64::Engine;
use chrono::{DateTime, Utc};
use http::StatusCode;
use ninjapaws_proto::datadog::trace::{
    AgentPayload, AttributeAnyValue, AttributeArrayValue, ClientGroupedStats, ClientStatsBucket, ClientStatsPayload,
    Span, StatsPayload, TracerPayload, attribute_any_value::AttributeAnyValueType,
    attribute_array_value::AttributeArrayValueType, idx,
};
use ninjapaws_storage as storage;
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::traces::{self as rows, SketchState, TraceRoot};
use prost::Message;
use serde_json::{Map, Value};
use tako::extractors::state::State;
use tako::types::Response;

use super::trace_sketch;
use crate::json;
use crate::msgpack::{self, MsgFields, MsgValue};
use crate::raw;
use crate::server::{Body, Ctx, accepted, json, storage_failed};
use crate::wire::{tags, text};

/// The label of everything on this host in raw_payloads.
const INTAKE: &str = "trace";

// --- JSON for the *_json columns ---

/// A number, or one of three words in its place: JSON has numbers, but no NaN and no infinity.
/// The attributes beside a NaN are kept, and a reader can still tell what was sent.
fn json_double(value: f64) -> Value {
    if value.is_nan() {
        Value::from("NaN")
    } else if value == f64::INFINITY {
        Value::from("+Inf")
    } else if value == f64::NEG_INFINITY {
        Value::from("-Inf")
    } else {
        Value::from(value)
    }
}

/// Bytes as base64, the only portable way to put them in JSON.
fn json_bytes(bytes: &[u8]) -> Value {
    Value::from(base64::engine::general_purpose::STANDARD.encode(bytes))
}

/// An attribute value tagged with its wire type: {"int":5}. The tag is the point: a bare JSON
/// value would turn 5, 5.0 and "5" into the same thing.
fn tagged(tag: &str, value: Value) -> Value {
    Value::Object(Map::from_iter([(tag.to_string(), value)]))
}

/// A JSON object of the entries, keys sorted, so two identical attribute sets are the same text.
fn object(entries: BTreeMap<String, Value>) -> Value {
    Value::Object(entries.into_iter().collect())
}

// --- /api/v0.2/traces -> spans ---

fn sorted<V: Clone>(map: &HashMap<String, V>) -> BTreeMap<String, V> {
    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Splits an idx 128-bit trace id into (high, low). The wire carries big-endian bytes; a short id
/// is left-padded and a long one keeps its last sixteen bytes, rather than dropping an id that
/// could be read.
pub fn split_trace_id(bytes: &[u8]) -> (u64, u64) {
    let mut padded = [0u8; 16];
    if bytes.len() >= 16 {
        padded.copy_from_slice(&bytes[bytes.len() - 16..]);
    } else {
        padded[16 - bytes.len()..].copy_from_slice(bytes);
    }
    let (high, low) = padded.split_at(8);
    (u64::from_be_bytes(high.try_into().unwrap()), u64::from_be_bytes(low.try_into().unwrap()))
}

/// The hex upper half of a 128-bit trace id, from meta["_dd.p.tid"]. Anything unparseable is 0,
/// and stays in meta where a reader can still see it.
pub fn parse_hex64(text: &str) -> u64 {
    // Hex digits only: from_str_radix would also take a sign.
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return 0;
    }
    u64::from_str_radix(text, 16).unwrap_or(0)
}

fn v04_array_value(value: &AttributeArrayValue) -> Value {
    match AttributeArrayValueType::try_from(value.r#type) {
        Ok(AttributeArrayValueType::StringValue) => tagged("string", Value::from(value.string_value.clone())),
        Ok(AttributeArrayValueType::BoolValue) => tagged("bool", Value::from(value.bool_value)),
        Ok(AttributeArrayValueType::IntValue) => tagged("int", Value::from(value.int_value)),
        Ok(AttributeArrayValueType::DoubleValue) => tagged("double", json_double(value.double_value)),
        Err(_) => Value::Null,
    }
}

/// AttributeAnyValue is a hand-rolled union: `type` says which field is meaningful, and the
/// others are zero, not absent.
fn v04_value(value: &AttributeAnyValue) -> Value {
    match AttributeAnyValueType::try_from(value.r#type) {
        Ok(AttributeAnyValueType::StringValue) => tagged("string", Value::from(value.string_value.clone())),
        Ok(AttributeAnyValueType::BoolValue) => tagged("bool", Value::from(value.bool_value)),
        Ok(AttributeAnyValueType::IntValue) => tagged("int", Value::from(value.int_value)),
        Ok(AttributeAnyValueType::DoubleValue) => tagged("double", json_double(value.double_value)),
        Ok(AttributeAnyValueType::ArrayValue) => {
            let items = value.array_value.as_ref().map(|a| a.values.iter().map(v04_array_value).collect());
            tagged("array", Value::Array(items.unwrap_or_default()))
        }
        Err(_) => Value::Null,
    }
}

/// A v0.4 span event's typed attributes, in the same tagged form as idx.
fn v04_event_attributes(attributes: &HashMap<String, AttributeAnyValue>) -> String {
    if attributes.is_empty() {
        return String::new();
    }
    json::compact(&object(attributes.iter().map(|(k, v)| (k.clone(), v04_value(v))).collect()))
}

/// A v0.4 span link's plain string attributes, in the tagged form too, so the link_attributes
/// column has one shape across both wire formats.
fn v04_link_attributes(attributes: &HashMap<String, String>) -> String {
    if attributes.is_empty() {
        return String::new();
    }
    json::compact(&object(
        attributes.iter().map(|(k, v)| (k.clone(), tagged("string", Value::from(v.clone())))).collect(),
    ))
}

fn v04_span_row(chunk: &rows::Span, span: &Span) -> rows::Span {
    let meta = sorted(&span.meta);
    rows::Span {
        service: span.service.clone(),
        name: span.name.clone(),
        resource: span.resource.clone(),
        span_type: span.r#type.clone(),
        trace_id: span.trace_id,
        // v0.4 has no field for the upper half of a 128-bit trace id: it travels as hex in meta.
        // Parsed so both formats fill the same pair of columns, and left in meta as it arrived.
        trace_id_high: parse_hex64(meta.get("_dd.p.tid").map_or("", String::as_str)),
        span_id: span.span_id,
        parent_id: span.parent_id,
        start: span.start,
        duration_ns: span.duration,
        error: span.error,
        // v0.4 has no kind field either; the tracer puts it in meta.
        kind: meta.get("span.kind").cloned().unwrap_or_default(),
        metrics: sorted(&span.metrics),
        meta_struct: span.meta_struct.iter().map(|(k, v)| (k.clone(), Blob(v.clone()))).collect(),
        meta,
        link_trace_id: span.span_links.iter().map(|l| l.trace_id).collect(),
        link_trace_id_high: span.span_links.iter().map(|l| l.trace_id_high).collect(),
        link_span_id: span.span_links.iter().map(|l| l.span_id).collect(),
        link_attributes: span.span_links.iter().map(|l| v04_link_attributes(&l.attributes)).collect(),
        link_tracestate: span.span_links.iter().map(|l| l.tracestate.clone()).collect(),
        link_flags: span.span_links.iter().map(|l| l.flags).collect(),
        event_time: span.span_events.iter().map(|e| e.time_unix_nano as i64).collect(),
        event_name: span.span_events.iter().map(|e| e.name.clone()).collect(),
        event_attributes: span.span_events.iter().map(|e| v04_event_attributes(&e.attributes)).collect(),
        ..chunk.clone()
    }
}

fn v04_tracer_rows(agent: &rows::Span, tracer: &TracerPayload, found: &mut Vec<rows::Span>) {
    // The agent sets container_debug only when resolving the container tags gave it trouble, so
    // absent stays NULL: a zero latency would read as "the lookup was instant".
    let debug = tracer.container_debug.as_ref();
    let tracer_row = rows::Span {
        wire_format: "v04".into(),
        container_id: tracer.container_id.clone(),
        language: tracer.language_name.clone(),
        language_version: tracer.language_version.clone(),
        tracer_version: tracer.tracer_version.clone(),
        runtime_id: tracer.runtime_id.clone(),
        tracer_env: tracer.env.clone(),
        tracer_hostname: tracer.hostname.clone(),
        app_version: tracer.app_version.clone(),
        tracer_tags: sorted(&tracer.tags),
        container_debug_error: debug.map(|d| d.error.clone()),
        container_debug_latency_ms: debug.map(|d| d.latency_ms),
        container_debug_was_buffered: debug.map(|d| text::flag(d.was_buffered)),
        container_debug_buffer_ms: debug.map(|d| d.buffer_ms),
        container_debug_buffer_eviction_reason: debug.map(|d| d.buffer_eviction_reason.clone()),
        ..agent.clone()
    };
    for chunk in &tracer.chunks {
        // v0.4 has no sampling_mechanism field; the same information is in meta["_dd.p.dm"],
        // kept in the meta map untouched.
        let chunk_row = rows::Span {
            priority: chunk.priority,
            origin: chunk.origin.clone(),
            dropped_trace: text::flag(chunk.dropped_trace),
            chunk_tags: sorted(&chunk.tags),
            ..tracer_row.clone()
        };
        found.extend(chunk.spans.iter().map(|span| v04_span_row(&chunk_row, span)));
    }
}

/// Resolves a reference into an idx payload's string table. Index 0 is the empty string and means
/// "unset"; a reference past the end is a broken payload and resolves to empty, so one bad
/// reference does not cost the batch.
pub fn string_at(strings: &[String], index: u32) -> &str {
    match strings.get(index as usize) {
        Some(found) if index != 0 => found,
        _ => "",
    }
}

/// How far a nested attribute is followed. AnyValue is recursive on the wire and nothing bounds
/// it; real tracers nest two or three levels.
pub const ATTRIBUTE_MAX_DEPTH: usize = 32;

fn idx_value(strings: &[String], value: Option<&idx::AnyValue>, depth: usize) -> Value {
    use idx::any_value::Value as Any;
    if depth >= ATTRIBUTE_MAX_DEPTH {
        // Truncated, and said so: an empty value would read as "the tracer sent nothing".
        return tagged("_depth_exceeded", Value::from(ATTRIBUTE_MAX_DEPTH));
    }
    match value.and_then(|v| v.value.as_ref()) {
        Some(Any::StringValueRef(index)) => tagged("string", Value::from(string_at(strings, *index))),
        Some(Any::BoolValue(flag)) => tagged("bool", Value::from(*flag)),
        Some(Any::IntValue(number)) => tagged("int", Value::from(*number)),
        Some(Any::DoubleValue(number)) => tagged("double", json_double(*number)),
        Some(Any::BytesValue(bytes)) => tagged("bytes", json_bytes(bytes)),
        Some(Any::ArrayValue(array)) => {
            tagged("array", array.values.iter().map(|item| idx_value(strings, Some(item), depth + 1)).collect())
        }
        // A list, not an object: two entries may share a key, and the order is the tracer's.
        Some(Any::KeyValueList(list)) => tagged(
            "kvlist",
            list.key_values
                .iter()
                .map(|entry| {
                    let mut pair = Map::new();
                    pair.insert("key".into(), Value::from(string_at(strings, entry.key)));
                    pair.insert("value".into(), idx_value(strings, entry.value.as_ref(), depth + 1));
                    Value::Object(pair)
                })
                .collect(),
        ),
        // Absent, or a kind of value this build does not know: null, so the key still shows the
        // attribute existed.
        None => Value::Null,
    }
}

/// The attributes by reference. The map arrives unordered, so where two references resolve to
/// the same name, the higher reference wins.
fn by_reference(attributes: &HashMap<u32, idx::AnyValue>) -> Vec<(u32, &idx::AnyValue)> {
    let mut found: Vec<(u32, &idx::AnyValue)> = attributes.iter().map(|(k, v)| (*k, v)).collect();
    found.sort_by_key(|(key, _)| *key);
    found
}

/// An idx attribute map as JSON, every value tagged with its wire type:
/// {"k":{"string":"v"},"n":{"int":5}}. "" when there are none.
pub fn idx_attributes_json(strings: &[String], attributes: &HashMap<u32, idx::AnyValue>) -> String {
    if attributes.is_empty() {
        return String::new();
    }
    let by_name = by_reference(attributes)
        .into_iter()
        .map(|(key, value)| (string_at(strings, key).to_string(), idx_value(strings, Some(value), 0)))
        .collect();
    json::compact(&object(by_name))
}

/// The scalar attributes as plain strings, for the tracer_tags and chunk_tags columns. The others
/// survive in the matching *_attributes_json column.
fn idx_scalar_attributes(strings: &[String], attributes: &HashMap<u32, idx::AnyValue>) -> BTreeMap<String, String> {
    use idx::any_value::Value as Any;
    let mut found = BTreeMap::new();
    for (key, value) in by_reference(attributes) {
        let key = string_at(strings, key).to_string();
        match &value.value {
            Some(Any::StringValueRef(index)) => found.insert(key, string_at(strings, *index).to_string()),
            Some(Any::BoolValue(flag)) => found.insert(key, flag.to_string()),
            Some(Any::IntValue(number)) => found.insert(key, number.to_string()),
            Some(Any::DoubleValue(number)) => found.insert(key, text::of_float(*number)),
            _ => None,
        };
    }
    found
}

/// What an idx span's typed attributes become in the three v0.4 maps.
struct Projection {
    meta: BTreeMap<String, String>,
    metrics: BTreeMap<String, f64>,
    meta_struct: BTreeMap<String, Blob>,
}

/// Splits an idx span's typed attributes into the three v0.4 maps, so a query written for v0.4
/// reads both formats: strings and bools to meta, numbers to metrics, bytes to meta_struct.
///
/// An int goes to metrics as a float, which loses precision above 2^53; attributes_json keeps the
/// exact value beside it.
fn idx_project_attributes(strings: &[String], attributes: &HashMap<u32, idx::AnyValue>) -> Projection {
    use idx::any_value::Value as Any;
    let mut found = Projection { meta: BTreeMap::new(), metrics: BTreeMap::new(), meta_struct: BTreeMap::new() };
    for (key, value) in by_reference(attributes) {
        let key = string_at(strings, key).to_string();
        match &value.value {
            Some(Any::StringValueRef(index)) => {
                found.meta.insert(key, string_at(strings, *index).to_string());
            }
            Some(Any::BoolValue(flag)) => {
                found.meta.insert(key, flag.to_string());
            }
            Some(Any::IntValue(number)) => {
                found.metrics.insert(key, *number as f64);
            }
            Some(Any::DoubleValue(number)) => {
                found.metrics.insert(key, *number);
            }
            Some(Any::BytesValue(bytes)) => {
                found.meta_struct.insert(key, Blob(bytes.clone()));
            }
            _ => {}
        }
    }
    found
}

/// The idx SpanKind as the lowercase OTel name v0.4 tracers put in meta["span.kind"].
/// Unspecified is empty, as an absent meta key is.
fn idx_span_kind(kind: i32) -> &'static str {
    match idx::SpanKind::try_from(kind) {
        Ok(idx::SpanKind::Internal) => "internal",
        Ok(idx::SpanKind::Server) => "server",
        Ok(idx::SpanKind::Client) => "client",
        Ok(idx::SpanKind::Producer) => "producer",
        Ok(idx::SpanKind::Consumer) => "consumer",
        _ => "",
    }
}

fn idx_span_row(chunk: &rows::Span, strings: &[String], span: &idx::Span) -> rows::Span {
    let projection = idx_project_attributes(strings, &span.attributes);
    let link_ids: Vec<(u64, u64)> = span.links.iter().map(|link| split_trace_id(&link.trace_id)).collect();
    rows::Span {
        service: string_at(strings, span.service_ref).into(),
        name: string_at(strings, span.name_ref).into(),
        resource: string_at(strings, span.resource_ref).into(),
        span_type: string_at(strings, span.type_ref).into(),
        span_id: span.span_id,
        parent_id: span.parent_id,
        start: span.start as i64,
        duration_ns: span.duration as i64,
        // v0.4 ships an int32 flag and idx a bool; the column keeps the v0.4 width so one query
        // reads both.
        error: i32::from(span.error),
        kind: idx_span_kind(span.kind).into(),
        span_env: string_at(strings, span.env_ref).into(),
        span_version: string_at(strings, span.version_ref).into(),
        component: string_at(strings, span.component_ref).into(),
        meta: projection.meta,
        metrics: projection.metrics,
        meta_struct: projection.meta_struct,
        attributes_json: idx_attributes_json(strings, &span.attributes),
        link_trace_id: link_ids.iter().map(|(_, low)| *low).collect(),
        link_trace_id_high: link_ids.iter().map(|(high, _)| *high).collect(),
        link_span_id: span.links.iter().map(|l| l.span_id).collect(),
        link_attributes: span.links.iter().map(|l| idx_attributes_json(strings, &l.attributes)).collect(),
        link_tracestate: span.links.iter().map(|l| string_at(strings, l.tracestate_ref).to_string()).collect(),
        link_flags: span.links.iter().map(|l| l.flags).collect(),
        event_time: span.events.iter().map(|e| e.time as i64).collect(),
        event_name: span.events.iter().map(|e| string_at(strings, e.name_ref).to_string()).collect(),
        event_attributes: span.events.iter().map(|e| idx_attributes_json(strings, &e.attributes)).collect(),
        ..chunk.clone()
    }
}

/// One v1.0 (idx) tracer payload. Every string in this format is an index into the payload's own
/// table, so nothing is readable until resolved.
fn idx_tracer_rows(agent: &rows::Span, tracer: &idx::TracerPayload, found: &mut Vec<rows::Span>) {
    let strings = &tracer.strings;
    let debug = tracer.container_debug.as_ref();
    let tracer_row = rows::Span {
        wire_format: "idx".into(),
        container_id: string_at(strings, tracer.container_id_ref).into(),
        language: string_at(strings, tracer.language_name_ref).into(),
        language_version: string_at(strings, tracer.language_version_ref).into(),
        tracer_version: string_at(strings, tracer.tracer_version_ref).into(),
        runtime_id: string_at(strings, tracer.runtime_id_ref).into(),
        tracer_env: string_at(strings, tracer.env_ref).into(),
        tracer_hostname: string_at(strings, tracer.hostname_ref).into(),
        app_version: string_at(strings, tracer.app_version_ref).into(),
        tracer_tags: idx_scalar_attributes(strings, &tracer.attributes),
        tracer_attributes_json: idx_attributes_json(strings, &tracer.attributes),
        container_debug_error: debug.map(|d| string_at(strings, d.error_ref).into()),
        container_debug_latency_ms: debug.map(|d| d.latency_ms),
        container_debug_was_buffered: debug.map(|d| text::flag(d.was_buffered)),
        container_debug_buffer_ms: debug.map(|d| d.buffer_ms),
        container_debug_buffer_eviction_reason: debug.map(|d| string_at(strings, d.buffer_eviction_reason_ref).into()),
        ..agent.clone()
    };
    for chunk in &tracer.chunks {
        // v1.0 moved the trace id to the chunk, as the full 128 bits.
        let (high, low) = split_trace_id(&chunk.trace_id);
        let chunk_row = rows::Span {
            priority: chunk.priority,
            origin: string_at(strings, chunk.origin_ref).into(),
            dropped_trace: text::flag(chunk.dropped_trace),
            sampling_mechanism: chunk.sampling_mechanism,
            chunk_tags: idx_scalar_attributes(strings, &chunk.attributes),
            chunk_attributes_json: idx_attributes_json(strings, &chunk.attributes),
            trace_id_high: high,
            trace_id: low,
            ..tracer_row.clone()
        };
        found.extend(chunk.spans.iter().map(|span| idx_span_row(&chunk_row, strings, span)));
    }
}

/// Flattens a whole AgentPayload into span rows, both wire shapes, in wire order.
///
/// The agent, tracer and chunk are copied onto every row rather than normalized away: "which
/// agent version shipped this span" is the question asked when a trace looks wrong, and it should
/// not take two joins.
pub fn span_rows(tenant: &str, received_at: DateTime<Utc>, payload: &AgentPayload) -> Vec<rows::Span> {
    let agent = rows::Span {
        tenant_id: tenant.into(),
        received_at,
        agent_hostname: payload.host_name.clone(),
        agent_env: payload.env.clone(),
        agent_version: payload.agent_version.clone(),
        target_tps: payload.target_tps,
        error_tps: payload.error_tps,
        rare_sampler_enabled: text::flag(payload.rare_sampler_enabled),
        agent_tags: sorted(&payload.tags),
        ..Default::default()
    };
    let mut found = Vec::new();
    for tracer in &payload.tracer_payloads {
        v04_tracer_rows(&agent, tracer, &mut found);
    }
    for tracer in &payload.idx_tracer_payloads {
        idx_tracer_rows(&agent, tracer, &mut found);
    }
    found
}

/// POST /api/v0.2/traces: a protobuf AgentPayload.
///
/// What arrives is a sample: the agent runs its samplers before this writer. The complete counts
/// are in the stats endpoint. The agent parses the answer (rate_by_service feeds its priority
/// sampler), so it keeps exactly this shape.
pub async fn handle_traces(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    let answer = json(StatusCode::OK, &serde_json::json!({ "rate_by_service": {} }));
    if ctx.is_sweep() {
        return answer;
    }
    match AgentPayload::decode(&body[..]) {
        Err(e) => {
            raw::store(&clickhouse, &ctx, INTAKE, "decode_error", &format!("protobuf AgentPayload: {e}"), &body).await
        }
        // Protobuf carries no type marker and skips unknown fields, so a body meant for another
        // endpoint decodes "successfully" into an empty payload. Neither list being filled is the
        // one signal.
        Ok(payload) if payload.tracer_payloads.is_empty() && payload.idx_tracer_payloads.is_empty() => {
            raw::store(
                &clickhouse,
                &ctx,
                INTAKE,
                "unexpected_shape",
                "AgentPayload decoded with no TracerPayloads and no IdxTracerPayloads",
                &body,
            )
            .await
        }
        Ok(payload) => {
            if let Err(e) = storage::insert(&clickhouse, &span_rows(ctx.tenant(), Utc::now(), &payload)).await {
                return storage_failed(e);
            }
        }
    }
    answer
}

// --- /api/v0.2/stats -> apm_stats ---

// The trace-agent writes this payload with msgp: a map keyed by the Go field name at every level.
// A field of the wrong type refuses the payload, which then goes to raw_payloads with the first
// such field named; the fields are read in the struct's order, so that is the first one met.

fn group_of(f: &MsgFields) -> ClientGroupedStats {
    ClientGroupedStats {
        service: f.string("Service"),
        name: f.string("Name"),
        resource: f.string("Resource"),
        http_status_code: f.uint32("HTTPStatusCode"),
        r#type: f.string("Type"),
        db_type: f.string("DBType"),
        hits: f.uint64("Hits"),
        errors: f.uint64("Errors"),
        duration: f.uint64("Duration"),
        ok_summary: f.bytes("OkSummary"),
        error_summary: f.bytes("ErrorSummary"),
        synthetics: f.bool("Synthetics"),
        top_level_hits: f.uint64("TopLevelHits"),
        span_kind: f.string("SpanKind"),
        is_trace_root: f.int32("IsTraceRoot"),
        grpc_status_code: f.string("GRPCStatusCode"),
        http_method: f.string("HTTPMethod"),
        http_endpoint: f.string("HTTPEndpoint"),
        // The one key that is not the field's name.
        service_source: f.string("srv_src"),
        peer_tags: f.strings("PeerTags"),
        span_derived_primary_tags: f.strings("SpanDerivedPrimaryTags"),
        additional_metric_tags: f.strings("AdditionalMetricTags"),
    }
}

fn bucket_of(f: &MsgFields) -> ClientStatsBucket {
    ClientStatsBucket {
        start: f.uint64("Start"),
        duration: f.uint64("Duration"),
        agent_time_shift: f.int64("AgentTimeShift"),
        stats: f.maps("Stats").iter().map(group_of).collect(),
    }
}

fn client_of(f: &MsgFields) -> ClientStatsPayload {
    ClientStatsPayload {
        hostname: f.string("Hostname"),
        env: f.string("Env"),
        version: f.string("Version"),
        lang: f.string("Lang"),
        tracer_version: f.string("TracerVersion"),
        runtime_id: f.string("RuntimeID"),
        sequence: f.uint64("Sequence"),
        agent_aggregation: f.string("AgentAggregation"),
        service: f.string("Service"),
        container_id: f.string("ContainerID"),
        git_commit_sha: f.string("GitCommitSha"),
        image_tag: f.string("ImageTag"),
        process_tags_hash: f.uint64("ProcessTagsHash"),
        process_tags: f.string("ProcessTags"),
        stats: f.maps("Stats").iter().map(bucket_of).collect(),
        tags: f.strings("Tags"),
    }
}

/// Reads a msgpack StatsPayload.
pub fn decode_stats_payload(body: &[u8]) -> Result<StatsPayload, String> {
    let root = msgpack::decode(body)?;
    // The agent always sends the map; nil here is not an empty payload.
    if root == MsgValue::Nil {
        return Err("expected a map, got nil".into());
    }
    let problems = RefCell::new(Vec::new());
    let f = MsgFields::new(&problems, "", &root);
    let payload = StatsPayload {
        agent_hostname: f.string("AgentHostname"),
        agent_env: f.string("AgentEnv"),
        agent_version: f.string("AgentVersion"),
        client_computed: f.bool("ClientComputed"),
        split_payload: f.bool("SplitPayload"),
        stats: f.maps("Stats").iter().map(client_of).collect(),
    };
    match problems.into_inner().into_iter().next() {
        None => Ok(payload),
        Some(problem) => Err(problem),
    }
}

/// Flattens a StatsPayload into one row per grouped stat. A summary that does not decode is
/// logged and still stored: "undecodable" is a state the table records.
pub fn stat_rows(tenant: &str, received_at: DateTime<Utc>, payload: &StatsPayload) -> Vec<rows::ApmStat> {
    let summary_of = |group: &ClientGroupedStats, column: &str, raw: &[u8]| {
        let summary = trace_sketch::summary(raw);
        if let SketchState::Undecodable(error) = &summary.state {
            tracing::warn!("[apm-stats] {} {} {column} {} B: {error}", group.service, group.name, raw.len());
        }
        summary
    };
    let mut found = Vec::new();
    for client in &payload.stats {
        // The one flat "key:value" list in the trace protocol: a tag key may repeat, and a plain
        // map would keep whichever came last.
        let client_tags = tags::to_multi_map(&client.tags);
        for bucket in &client.stats {
            for group in &bucket.stats {
                let ok = summary_of(group, "ok_summary", &group.ok_summary);
                let error = summary_of(group, "error_summary", &group.error_summary);
                found.push(rows::ApmStat {
                    tenant_id: tenant.into(),
                    received_at,
                    agent_hostname: payload.agent_hostname.clone(),
                    agent_env: payload.agent_env.clone(),
                    agent_version: payload.agent_version.clone(),
                    client_computed: text::flag(payload.client_computed),
                    split_payload: text::flag(payload.split_payload),
                    client_hostname: client.hostname.clone(),
                    client_env: client.env.clone(),
                    client_version: client.version.clone(),
                    client_lang: client.lang.clone(),
                    client_tracer_version: client.tracer_version.clone(),
                    client_runtime_id: client.runtime_id.clone(),
                    client_sequence: client.sequence,
                    client_agent_aggregation: client.agent_aggregation.clone(),
                    client_service: client.service.clone(),
                    client_container_id: client.container_id.clone(),
                    client_tags: client_tags.clone(),
                    client_git_commit_sha: client.git_commit_sha.clone(),
                    client_image_tag: client.image_tag.clone(),
                    client_process_tags: client.process_tags.clone(),
                    client_process_tags_hash: client.process_tags_hash,
                    bucket_start: bucket.start as i64,
                    bucket_duration_ns: bucket.duration,
                    agent_time_shift_ns: bucket.agent_time_shift,
                    service: group.service.clone(),
                    name: group.name.clone(),
                    resource: group.resource.clone(),
                    http_status_code: group.http_status_code,
                    span_type: group.r#type.clone(),
                    db_type: group.db_type.clone(),
                    hits: group.hits,
                    errors: group.errors,
                    duration_ns: group.duration,
                    synthetics: text::flag(group.synthetics),
                    top_level_hits: group.top_level_hits,
                    span_kind: group.span_kind.clone(),
                    peer_tags: group.peer_tags.clone(),
                    is_trace_root: TraceRoot::of(group.is_trace_root),
                    grpc_status_code: group.grpc_status_code.clone(),
                    http_method: group.http_method.clone(),
                    http_endpoint: group.http_endpoint.clone(),
                    service_source: group.service_source.clone(),
                    span_derived_primary_tags: group.span_derived_primary_tags.clone(),
                    additional_metric_tags: group.additional_metric_tags.clone(),
                    ok_summary: Blob(ok.raw),
                    ok_summary_state: ok.state,
                    ok_count: ok.count,
                    ok_sum: ok.sum,
                    ok_min: ok.min,
                    ok_max: ok.max,
                    ok_bin_keys: ok.bin_keys,
                    ok_bin_counts: ok.bin_counts,
                    error_summary: Blob(error.raw),
                    error_summary_state: error.state,
                    error_count: error.count,
                    error_sum: error.sum,
                    error_min: error.min,
                    error_max: error.max,
                    error_bin_keys: error.bin_keys,
                    error_bin_counts: error.bin_counts,
                });
            }
        }
    }
    found
}

/// POST /api/v0.2/stats: msgpack, not protobuf.
///
/// What arrives is NOT a sample: the agent feeds every trace to its concentrator before the
/// sampler runs, so these counts are complete even when 99% of the spans were thrown away.
///
/// The Android SDK from 3.14 may post msgpack here that is not a StatsPayload; it fails the decode
/// and goes to raw_payloads.
pub async fn handle_stats(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    if !ctx.is_sweep() {
        match decode_stats_payload(&body) {
            Err(problem) => {
                raw::store(
                    &clickhouse,
                    &ctx,
                    INTAKE,
                    "decode_error",
                    &format!("msgpack StatsPayload: {problem}"),
                    &body,
                )
                .await
            }
            Ok(payload) => {
                if let Err(e) = storage::insert(&clickhouse, &stat_rows(ctx.tenant(), Utc::now(), &payload)).await {
                    return storage_failed(e);
                }
            }
        }
    }
    json(StatusCode::OK, &serde_json::json!({}))
}

// --- /api/v0.1/pipeline_stats -> dsm_pipeline_stats, dsm_backlogs, dsm_bucket_transactions ---
//
// The schema is dd-trace-go's, not the agent's: the trace-agent only proxies this request.
// dd-trace-go generates its codec with msgp and no field tags, so the wire is a map whose keys are
// its Go field names. Every level keeps the keys it does not know, with their values: dd-trace-go
// adds fields with no version negotiation, and the payload that shows a new one arrives once.

#[derive(Clone, Debug, PartialEq)]
pub struct DsmBacklog {
    pub tags: Vec<String>,
    pub value: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DsmPoint {
    pub edge_tags: Vec<String>,
    pub hash: u64,
    pub parent_hash: u64,
    pub pathway_latency: Vec<u8>,
    pub edge_latency: Vec<u8>,
    pub payload_size: Vec<u8>,
    pub timestamp_type: String,
    pub unknown: BTreeMap<String, MsgValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DsmBucket {
    pub start: u64,
    pub duration: u64,
    pub points: Vec<DsmPoint>,
    pub backlogs: Vec<DsmBacklog>,
    pub transactions: Vec<u8>,
    pub transaction_checkpoint_ids: Vec<u8>,
    pub unknown: BTreeMap<String, MsgValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DsmPayload {
    pub env: String,
    pub service: String,
    pub tracer_version: String,
    pub lang: String,
    pub version: String,
    pub process_tags: Vec<String>,
    pub product_mask: u64,
    pub buckets: Vec<DsmBucket>,
    pub unknown: BTreeMap<String, MsgValue>,
}

fn dsm_backlog(f: &MsgFields) -> DsmBacklog {
    DsmBacklog { tags: f.strings("Tags"), value: f.int64("Value") }
}

const DSM_POINT_KEYS: [&str; 7] =
    ["EdgeTags", "Hash", "ParentHash", "PathwayLatency", "EdgeLatency", "PayloadSize", "TimestampType"];

fn dsm_point(f: &MsgFields) -> DsmPoint {
    DsmPoint {
        edge_tags: f.strings("EdgeTags"),
        hash: f.uint64("Hash"),
        parent_hash: f.uint64("ParentHash"),
        pathway_latency: f.bytes("PathwayLatency"),
        edge_latency: f.bytes("EdgeLatency"),
        payload_size: f.bytes("PayloadSize"),
        timestamp_type: f.string("TimestampType"),
        unknown: f.unknown(&BTreeSet::from(DSM_POINT_KEYS)),
    }
}

// "Ids", not "IDs": dd-trace-go spells the field that way to match the Java tracer's wire format.
const DSM_BUCKET_KEYS: [&str; 6] =
    ["Start", "Duration", "Stats", "Backlogs", "Transactions", "TransactionCheckpointIds"];

fn dsm_bucket(f: &MsgFields) -> DsmBucket {
    DsmBucket {
        start: f.uint64("Start"),
        duration: f.uint64("Duration"),
        points: f.maps("Stats").iter().map(dsm_point).collect(),
        backlogs: f.maps("Backlogs").iter().map(dsm_backlog).collect(),
        transactions: f.bytes("Transactions"),
        transaction_checkpoint_ids: f.bytes("TransactionCheckpointIds"),
        unknown: f.unknown(&BTreeSet::from(DSM_BUCKET_KEYS)),
    }
}

const DSM_PAYLOAD_KEYS: [&str; 8] =
    ["Env", "Service", "TracerVersion", "Lang", "Version", "ProcessTags", "ProductMask", "Stats"];

/// Reads one msgpack datastreams.StatsPayload.
pub fn decode_dsm_payload(body: &[u8]) -> Result<DsmPayload, String> {
    let root = msgpack::decode(body)?;
    let problems = RefCell::new(Vec::new());
    let f = MsgFields::new(&problems, "", &root);
    let payload = DsmPayload {
        env: f.string("Env"),
        service: f.string("Service"),
        tracer_version: f.string("TracerVersion"),
        lang: f.string("Lang"),
        version: f.string("Version"),
        process_tags: f.strings("ProcessTags"),
        product_mask: f.uint64("ProductMask"),
        buckets: f.maps("Stats").iter().map(dsm_bucket).collect(),
        unknown: f.unknown(&BTreeSet::from(DSM_PAYLOAD_KEYS)),
    };
    match problems.into_inner().into_iter().next() {
        None => Ok(payload),
        Some(problem) => Err(problem),
    }
}

/// A value of an unknown field as JSON; bytes become base64.
fn unknown_json(value: &MsgValue) -> Value {
    match value {
        MsgValue::Nil => Value::Null,
        MsgValue::Bool(flag) => Value::from(*flag),
        MsgValue::Int(number) => Value::from(*number),
        MsgValue::UInt(number) => Value::from(*number),
        // The float32's own shortest digits, not those of the float64 it widens to.
        MsgValue::Float32(number) if number.is_finite() => Value::from(number.to_string().parse::<f64>().unwrap()),
        MsgValue::Float32(number) => json_double(f64::from(*number)),
        MsgValue::Float(number) => json_double(*number),
        MsgValue::Str(text) => Value::from(text.clone()),
        // An empty bin is null: there are no bytes to write.
        MsgValue::Bin(bytes) if bytes.is_empty() => Value::Null,
        MsgValue::Bin(bytes) => json_bytes(bytes),
        MsgValue::Array(items) => items.iter().map(unknown_json).collect(),
        MsgValue::Map(entries) => object(entries.iter().map(|(k, v)| (k.clone(), unknown_json(v))).collect()),
        MsgValue::Ext(kind, data) => {
            let mut ext = Map::new();
            ext.insert("Data".into(), if data.is_empty() { Value::Null } else { json_bytes(data) });
            ext.insert("Type".into(), Value::from(*kind));
            Value::Object(ext)
        }
    }
}

/// The four request headers the body does not repeat. The trace-agent adds three of them as it
/// proxies the request, and they hold the only container identity a pipeline payload has.
#[derive(Clone, Debug, Default)]
pub struct DsmHeaders {
    pub via: String,
    pub additional_tags: String,
    pub container_tags: String,
    pub content_encoding: String,
}

fn prefixed<'a>(
    prefix: &'a str,
    unknown: &'a BTreeMap<String, MsgValue>,
) -> impl Iterator<Item = (String, Value)> + 'a {
    unknown.iter().map(move |(key, value)| (format!("{prefix}{key}"), unknown_json(value)))
}

/// What one decoded payload fans out into, one list per table.
#[derive(Debug, Default)]
pub struct DsmRows {
    pub points: Vec<rows::DsmPipelineStat>,
    pub backlogs: Vec<rows::DsmBacklog>,
    pub transactions: Vec<rows::DsmBucketTransactions>,
}

/// Fans one decoded payload out into the three tables it feeds.
pub fn dsm_rows(tenant: &str, received_at: DateTime<Utc>, payload: &DsmPayload, headers: &DsmHeaders) -> DsmRows {
    let mut found = DsmRows::default();
    for bucket in &payload.buckets {
        let bucket_start = bucket.start as i64;
        for point in &bucket.points {
            // Unknown keys from all three levels ride the point row, prefixed by where they were
            // found: one column pair for the whole payload keeps the schema still while
            // dd-trace-go grows fields.
            let unknown: BTreeMap<String, Value> = prefixed("payload.", &payload.unknown)
                .chain(prefixed("bucket.", &bucket.unknown))
                .chain(prefixed("point.", &point.unknown))
                .collect();
            let pathway = trace_sketch::summary(&point.pathway_latency);
            let edge = trace_sketch::summary(&point.edge_latency);
            let size = trace_sketch::summary(&point.payload_size);
            found.points.push(rows::DsmPipelineStat {
                tenant_id: tenant.into(),
                received_at,
                env: payload.env.clone(),
                service: payload.service.clone(),
                tracer_version: payload.tracer_version.clone(),
                lang: payload.lang.clone(),
                version: payload.version.clone(),
                process_tags: payload.process_tags.clone(),
                product_mask: payload.product_mask,
                bucket_start,
                bucket_duration_ns: bucket.duration,
                edge_tags: point.edge_tags.clone(),
                hash: point.hash,
                parent_hash: point.parent_hash,
                timestamp_type: point.timestamp_type.clone(),
                pathway_latency: Blob(pathway.raw),
                pathway_latency_state: pathway.state,
                pathway_count: pathway.count,
                pathway_sum: pathway.sum,
                pathway_min: pathway.min,
                pathway_max: pathway.max,
                pathway_bin_keys: pathway.bin_keys,
                pathway_bin_counts: pathway.bin_counts,
                edge_latency: Blob(edge.raw),
                edge_latency_state: edge.state,
                edge_count: edge.count,
                edge_sum: edge.sum,
                edge_min: edge.min,
                edge_max: edge.max,
                edge_bin_keys: edge.bin_keys,
                edge_bin_counts: edge.bin_counts,
                payload_size: Blob(size.raw),
                payload_size_state: size.state,
                payload_size_count: size.count,
                payload_size_sum: size.sum,
                payload_size_min: size.min,
                payload_size_max: size.max,
                payload_size_bin_keys: size.bin_keys,
                payload_size_bin_counts: size.bin_counts,
                via: headers.via.clone(),
                additional_tags: headers.additional_tags.clone(),
                container_tags: headers.container_tags.clone(),
                content_encoding: headers.content_encoding.clone(),
                unknown_keys: unknown.keys().cloned().collect(),
                unknown_json: if unknown.is_empty() { String::new() } else { json::compact(&object(unknown)) },
            });
        }
        for backlog in &bucket.backlogs {
            found.backlogs.push(rows::DsmBacklog {
                tenant_id: tenant.into(),
                received_at,
                env: payload.env.clone(),
                service: payload.service.clone(),
                tracer_version: payload.tracer_version.clone(),
                lang: payload.lang.clone(),
                version: payload.version.clone(),
                process_tags: payload.process_tags.clone(),
                product_mask: payload.product_mask,
                bucket_start,
                bucket_duration_ns: bucket.duration,
                tags: backlog.tags.clone(),
                value: backlog.value,
            });
        }
        // A table of their own because these hang off the bucket: a bucket can carry them with
        // no stats points at all.
        if !bucket.transactions.is_empty() || !bucket.transaction_checkpoint_ids.is_empty() {
            found.transactions.push(rows::DsmBucketTransactions {
                tenant_id: tenant.into(),
                received_at,
                env: payload.env.clone(),
                service: payload.service.clone(),
                tracer_version: payload.tracer_version.clone(),
                lang: payload.lang.clone(),
                version: payload.version.clone(),
                process_tags: payload.process_tags.clone(),
                product_mask: payload.product_mask,
                bucket_start,
                bucket_duration_ns: bucket.duration,
                transactions: Blob(bucket.transactions.clone()),
                transaction_checkpoint_ids: Blob(bucket.transaction_checkpoint_ids.clone()),
            });
        }
    }
    found
}

/// POST /api/v0.1/pipeline_stats: Data Streams Monitoring stats straight from the tracer. The
/// trace-agent is a reverse proxy here: it adds Via, X-Datadog-Additional-Tags and
/// X-Datadog-Container-Tags and forwards the body untouched.
pub async fn handle_pipeline_stats(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    if !ctx.is_sweep() {
        match decode_dsm_payload(&body) {
            Err(problem) => {
                raw::store(
                    &clickhouse,
                    &ctx,
                    INTAKE,
                    "decode_error",
                    &format!("msgpack datastreams.StatsPayload: {problem}"),
                    &body,
                )
                .await
            }
            Ok(payload) => {
                let headers = DsmHeaders {
                    via: ctx.header("Via").into(),
                    additional_tags: ctx.header("X-Datadog-Additional-Tags").into(),
                    container_tags: ctx.header("X-Datadog-Container-Tags").into(),
                    // The body is decompressed and the header left, so this is what the tracer
                    // put on the wire.
                    content_encoding: ctx.header("Content-Encoding").into(),
                };
                let found = dsm_rows(ctx.tenant(), Utc::now(), &payload, &headers);
                if let Err(e) = storage::insert(&clickhouse, &found.points).await {
                    return storage_failed(e);
                }
                if let Err(e) = storage::insert(&clickhouse, &found.backlogs).await {
                    return storage_failed(e);
                }
                if let Err(e) = storage::insert(&clickhouse, &found.transactions).await {
                    return storage_failed(e);
                }
            }
        }
    }
    accepted()
}

// --- /api/v2/data_streams_messages -> dsm_messages ---

/// A message's top-level key names, sorted. The one thing extractable without guessing at
/// meaning, and enough to turn "what does this track send?" into a GROUP BY. A message that is
/// not an object has none.
pub fn message_keys(message: &[u8]) -> Vec<String> {
    match serde_json::from_slice::<BTreeMap<String, serde::de::IgnoredAny>>(message) {
        Ok(keys) => keys.into_keys().collect(),
        Err(_) => Vec::new(),
    }
}

/// One row per message, with its place in the batch.
///
/// The producer of this track is not in the open agent repository, so no shape is invented: a
/// message is stored as the JSON text it arrived as.
pub fn dsm_message_rows(
    tenant: &str,
    received_at: DateTime<Utc>,
    messages: Vec<&[u8]>,
    origin: &str,
    origin_version: &str,
    encoding: &str,
) -> Vec<rows::DsmMessage> {
    messages
        .into_iter()
        .enumerate()
        .map(|(position, message)| rows::DsmMessage {
            tenant_id: tenant.into(),
            received_at,
            position: position as u32,
            keys: message_keys(message),
            message: Blob(message.to_vec()),
            dd_evp_origin: origin.into(),
            dd_evp_origin_version: origin_version.into(),
            content_encoding: encoding.into(),
        })
        .collect()
}

/// POST /api/v2/data_streams_messages. Not the trace-agent at all: an Event Platform track that
/// happens to live on this host. JSON, batched by the forwarder into one array.
pub async fn handle_data_streams_messages(
    ctx: Ctx,
    State(clickhouse): State<clickhouse::Client>,
    body: Body,
) -> Response {
    match json::try_parse_element_bytes(&body) {
        Err(problem) => {
            raw::store(
                &clickhouse,
                &ctx,
                INTAKE,
                "unexpected_shape",
                &format!("body is not the JSON array the forwarder sends: {problem}"),
                &body,
            )
            .await
        }
        Ok(messages) => {
            let found = dsm_message_rows(
                ctx.tenant(),
                Utc::now(),
                messages,
                ctx.header("Dd-Evp-Origin"),
                ctx.header("Dd-Evp-Origin-Version"),
                ctx.header("Content-Encoding"),
            );
            if let Err(e) = storage::insert(&clickhouse, &found).await {
                return storage_failed(e);
            }
        }
    }
    accepted()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routers::trace_sketch::tests::go_sketch;
    use chrono::TimeZone;
    use ninjapaws_proto::datadog::trace::{AttributeArray, ContainerDebug, SpanEvent, SpanLink, TraceChunk, Trilean};

    fn received_at() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 3, 4, 5, 6, 7).unwrap()
    }

    /// The value of attribute `key` under its type tag, as JSON text.
    fn tagged_text(json: &str, key: &str, tag: &str) -> String {
        let attribute: Value = serde_json::from_str(json).unwrap();
        attribute[key][tag].to_string()
    }

    fn v04_payload(tracer: TracerPayload) -> AgentPayload {
        AgentPayload { tracer_payloads: vec![tracer], ..Default::default() }
    }

    fn tracer_of(spans: Vec<Span>) -> TracerPayload {
        TracerPayload { chunks: vec![TraceChunk { spans, ..Default::default() }], ..Default::default() }
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    // --- /api/v0.2/traces, v0.4 ---

    #[test]
    fn every_span_row_carries_its_agent_tracer_and_chunk() {
        let root = Span {
            service: "checkout".into(),
            name: "flask.request".into(),
            resource: "GET /pay".into(),
            r#type: "web".into(),
            trace_id: 111,
            span_id: 222,
            start: 1_700_000_000_000_000_123,
            duration: 5_000_000,
            meta: HashMap::from([
                ("span.kind".into(), "server".into()),
                ("_dd.p.tid".into(), "67890abcdef01234".into()),
            ]),
            metrics: HashMap::from([("_sampling_priority_v1".into(), 1.0)]),
            ..Default::default()
        };
        let child = Span {
            service: "checkout".into(),
            name: "postgres.query".into(),
            resource: "SELECT 1".into(),
            r#type: "db".into(),
            trace_id: 111,
            span_id: 333,
            parent_id: 222,
            start: 1_700_000_000_001_000_000,
            duration: 900_000,
            error: 1,
            ..Default::default()
        };
        let chunk = TraceChunk {
            priority: 1,
            origin: "lambda".into(),
            tags: HashMap::from([("_dd.p.dm".into(), "-1".into())]),
            spans: vec![root, child],
            ..Default::default()
        };
        let tracer = TracerPayload {
            container_id: "abc123".into(),
            language_name: "python".into(),
            language_version: "3.12.1".into(),
            tracer_version: "2.9.0".into(),
            runtime_id: "0f4c-runtime".into(),
            env: "prod".into(),
            hostname: "pod-1".into(),
            app_version: "1.4.0".into(),
            tags: HashMap::from([("team".into(), "payments".into())]),
            chunks: vec![chunk],
            ..Default::default()
        };
        let payload = AgentPayload {
            host_name: "ip-10-0-0-1".into(),
            env: "prod".into(),
            agent_version: "7.58.2".into(),
            target_tps: 10.0,
            error_tps: 10.0,
            rare_sampler_enabled: true,
            tags: HashMap::from([("cluster".into(), "eu-1".into())]),
            tracer_payloads: vec![tracer],
            ..Default::default()
        };

        let rows = span_rows("t1", received_at(), &payload);
        assert_eq!(rows.len(), 2);
        for row in &rows {
            assert_eq!(row.tenant_id, "t1");
            assert_eq!(row.wire_format, "v04");
            assert_eq!(row.agent_hostname, "ip-10-0-0-1");
            assert_eq!(row.agent_version, "7.58.2");
            assert_eq!(row.rare_sampler_enabled, 1);
            assert_eq!(row.target_tps, 10.0);
            assert_eq!(row.language, "python");
            assert_eq!(row.runtime_id, "0f4c-runtime");
            assert_eq!(row.container_id, "abc123");
            assert_eq!(row.priority, 1);
            assert_eq!(row.origin, "lambda");
            assert_eq!(row.chunk_tags["_dd.p.dm"], "-1");
            // container_debug was absent: NULL, not a zero that reads as "instant".
            assert_eq!(row.container_debug_error, None);
            assert_eq!(row.container_debug_latency_ms, None);
            assert_eq!(row.container_debug_was_buffered, None);
        }
        assert_eq!(rows[0].parent_id, 0);
        assert_eq!(rows[1].parent_id, 222);
        // The upper half of the trace id comes out of meta, and stays in meta.
        assert_eq!(rows[0].trace_id_high, 0x67890abcdef01234);
        assert_eq!(rows[0].meta["_dd.p.tid"], "67890abcdef01234");
        assert_eq!(rows[1].trace_id_high, 0);
        assert_eq!(rows[0].kind, "server");
        assert_eq!(rows[0].start, 1_700_000_000_000_000_123);
        assert_eq!(rows[1].error, 1);
    }

    #[test]
    fn a_payload_without_spans_gives_no_rows() {
        let empty = TracerPayload { chunks: vec![TraceChunk::default()], ..Default::default() };
        let mut payload = v04_payload(empty);
        payload.tracer_payloads.push(tracer_of(vec![Span { service: "s".into(), span_id: 1, ..Default::default() }]));
        let rows = span_rows("t1", received_at(), &payload);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].service, "s");
        assert!(span_rows("t1", received_at(), &AgentPayload::default()).is_empty());
    }

    #[test]
    fn container_debug_is_stored_when_present_a_zero_latency_included() {
        let mut tracer = tracer_of(vec![Span { span_id: 1, ..Default::default() }]);
        tracer.container_debug = Some(ContainerDebug {
            error: "context deadline exceeded".into(),
            latency_ms: 0,
            was_buffered: true,
            buffer_ms: 250,
            buffer_eviction_reason: "timeout".into(),
        });
        let row = &span_rows("t1", received_at(), &v04_payload(tracer))[0];
        assert_eq!(row.container_debug_error.as_deref(), Some("context deadline exceeded"));
        assert_eq!(row.container_debug_latency_ms, Some(0));
        assert_eq!(row.container_debug_was_buffered, Some(1));
        assert_eq!(row.container_debug_buffer_ms, Some(250));
        assert_eq!(row.container_debug_buffer_eviction_reason.as_deref(), Some("timeout"));
    }

    #[test]
    fn links_and_events_keep_their_wire_order_and_their_types() {
        let any = |kind: AttributeAnyValueType| AttributeAnyValue { r#type: kind as i32, ..Default::default() };
        let item = |kind: AttributeArrayValueType| AttributeArrayValue { r#type: kind as i32, ..Default::default() };
        let frames = AttributeArray {
            values: vec![
                AttributeArrayValue { string_value: "a".into(), ..item(AttributeArrayValueType::StringValue) },
                AttributeArrayValue { int_value: 7, ..item(AttributeArrayValueType::IntValue) },
            ],
        };
        let exception = SpanEvent {
            time_unix_nano: 1_700_000_000_000_000_000,
            name: "exception".into(),
            attributes: HashMap::from([
                ("code".into(), AttributeAnyValue { int_value: 42, ..any(AttributeAnyValueType::IntValue) }),
                (
                    "message".into(),
                    AttributeAnyValue { string_value: "boom".into(), ..any(AttributeAnyValueType::StringValue) },
                ),
                ("ratio".into(), AttributeAnyValue { double_value: 0.5, ..any(AttributeAnyValueType::DoubleValue) }),
                ("retry".into(), AttributeAnyValue { bool_value: true, ..any(AttributeAnyValueType::BoolValue) }),
                (
                    "frames".into(),
                    AttributeAnyValue { array_value: Some(frames), ..any(AttributeAnyValueType::ArrayValue) },
                ),
            ]),
        };
        let span = Span {
            span_id: 1,
            span_links: vec![
                SpanLink {
                    trace_id: 10,
                    trace_id_high: 99,
                    span_id: 11,
                    tracestate: "dd=s:1".into(),
                    flags: 0x80000001,
                    attributes: HashMap::from([("rel".into(), "follows".into())]),
                },
                SpanLink { trace_id: 20, span_id: 21, ..Default::default() },
            ],
            span_events: vec![
                exception,
                SpanEvent { time_unix_nano: 1_700_000_000_000_000_001, name: "log".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        let row = &span_rows("t1", received_at(), &v04_payload(tracer_of(vec![span])))[0];

        assert_eq!(row.link_trace_id, vec![10, 20]);
        assert_eq!(row.link_trace_id_high, vec![99, 0]);
        assert_eq!(row.link_flags, vec![0x80000001, 0]);
        assert_eq!(row.link_attributes, vec![r#"{"rel":{"string":"follows"}}"#.to_string(), String::new()]);
        assert_eq!(row.event_name, vec!["exception", "log"]);
        assert_eq!(row.event_time[1], 1_700_000_000_000_000_001);

        // The tag names the wire type: 42 as {"int":42} and 0.5 as {"double":0.5} stay apart,
        // which bare JSON numbers would not.
        let attributes = &row.event_attributes[0];
        assert_eq!(tagged_text(attributes, "code", "int"), "42");
        assert_eq!(tagged_text(attributes, "ratio", "double"), "0.5");
        assert_eq!(tagged_text(attributes, "retry", "bool"), "true");
        assert_eq!(tagged_text(attributes, "message", "string"), "\"boom\"");
        assert_eq!(tagged_text(attributes, "frames", "array"), r#"[{"string":"a"},{"int":7}]"#);
        assert_eq!(row.event_attributes[1], "");
    }

    #[test]
    fn meta_struct_bytes_arrive_untouched() {
        let raw = vec![0x81, 0xa3, b'k', b'e', b'y', 0x00, 0xff];
        let span =
            Span { span_id: 1, meta_struct: HashMap::from([("appsec".into(), raw.clone())]), ..Default::default() };
        let row = &span_rows("t1", received_at(), &v04_payload(tracer_of(vec![span])))[0];
        assert_eq!(row.meta_struct["appsec"], Blob(raw));
    }

    #[test]
    fn the_upper_half_of_a_trace_id_is_read_as_hex_and_is_0_when_it_is_not() {
        for (text, expected) in [
            ("67890abcdef01234", 0x67890abcdef01234),
            ("FFFFFFFFFFFFFFFF", u64::MAX),
            ("00000000000000000000000000000001F", 31),
            ("", 0),
            ("not hex", 0),
            ("+1", 0),
            ("1ffffffffffffffff", 0),
        ] {
            assert_eq!(parse_hex64(text), expected, "{text}");
        }
    }

    // --- /api/v0.2/traces, idx (v1.0) ---

    fn idx_string(index: u32) -> idx::AnyValue {
        idx::AnyValue { value: Some(idx::any_value::Value::StringValueRef(index)) }
    }

    fn idx_any(value: idx::any_value::Value) -> idx::AnyValue {
        idx::AnyValue { value: Some(value) }
    }

    #[test]
    fn an_idx_payload_is_resolved_against_its_string_table() {
        use idx::any_value::Value as Any;
        // Index 0 is the empty string; a reference of 0 means "unset".
        let table = strings(&[
            "",
            "checkout",
            "flask.request",
            "GET /pay",
            "web",
            "python",
            "3.12.1",
            "2.9.0",
            "runtime-1",
            "prod",
            "pod-1",
            "1.4.0",
            "http.method",
            "GET",
            "retries",
            "payload",
            "tags",
            "a",
            "dd=s:1",
            "exception",
        ]);
        let trace_id = vec![0x67, 0x89, 0x0a, 0xbc, 0xde, 0xf0, 0x12, 0x34, 0, 0, 0, 0, 0, 0, 0, 0x6f];
        let span = idx::Span {
            service_ref: 1,
            name_ref: 2,
            resource_ref: 3,
            type_ref: 4,
            span_id: 333,
            parent_id: 222,
            start: 1_700_000_000_000_000_123,
            duration: 5_000_000,
            error: true,
            kind: idx::SpanKind::Server as i32,
            env_ref: 9,
            version_ref: 11,
            attributes: HashMap::from([
                (12, idx_string(13)),
                (14, idx_any(Any::IntValue(3))),
                (15, idx_any(Any::BytesValue(vec![0x00, 0x01]))),
                (16, idx_any(Any::ArrayValue(idx::ArrayValue { values: vec![idx_string(17)] }))),
            ]),
            links: vec![idx::SpanLink {
                trace_id: trace_id.clone(),
                span_id: 11,
                tracestate_ref: 18,
                flags: 1,
                ..Default::default()
            }],
            events: vec![idx::SpanEvent { time: 1_700_000_000_000_000_000, name_ref: 19, ..Default::default() }],
            ..Default::default()
        };
        let chunk =
            idx::TraceChunk { priority: 2, trace_id, sampling_mechanism: 4, spans: vec![span], ..Default::default() };
        let list = idx::KeyValueList { key_values: vec![idx::KeyValue { key: 12, value: Some(idx_string(13)) }] };
        let tracer = idx::TracerPayload {
            strings: table,
            language_name_ref: 5,
            language_version_ref: 6,
            tracer_version_ref: 7,
            runtime_id_ref: 8,
            env_ref: 9,
            hostname_ref: 10,
            app_version_ref: 11,
            attributes: HashMap::from([(16, idx_any(Any::KeyValueList(list)))]),
            chunks: vec![chunk],
            ..Default::default()
        };
        let payload =
            AgentPayload { host_name: "agent-host".into(), idx_tracer_payloads: vec![tracer], ..Default::default() };

        let rows = span_rows("t1", received_at(), &payload);
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.wire_format, "idx");
        assert_eq!(row.agent_hostname, "agent-host");
        assert_eq!(row.service, "checkout");
        assert_eq!(row.name, "flask.request");
        assert_eq!(row.resource, "GET /pay");
        assert_eq!(row.span_type, "web");
        assert_eq!(row.container_id, "");
        assert_eq!(row.component, "");
        assert_eq!(row.trace_id_high, 0x67890abcdef01234);
        assert_eq!(row.trace_id, 0x6f);
        assert_eq!(row.sampling_mechanism, 4);
        assert_eq!(row.kind, "server");
        // idx ships a bool; the column keeps v0.4's width.
        assert_eq!(row.error, 1);
        assert_eq!(row.span_env, "prod");
        assert_eq!(row.span_version, "1.4.0");

        // Scalars are projected into the v0.4 maps…
        assert_eq!(row.meta["http.method"], "GET");
        assert_eq!(row.metrics["retries"], 3.0);
        assert_eq!(row.meta_struct["payload"], Blob(vec![0x00, 0x01]));
        // …and the typed form is kept whole: the only place the array survives.
        assert_eq!(tagged_text(&row.attributes_json, "tags", "array"), r#"[{"string":"a"}]"#);
        assert_eq!(tagged_text(&row.attributes_json, "payload", "bytes"), "\"AAE=\"");
        assert_eq!(
            row.tracer_attributes_json,
            r#"{"tags":{"kvlist":[{"key":"http.method","value":{"string":"GET"}}]}}"#
        );
        // A non-scalar attribute is not flattened into tracer_tags.
        assert!(!row.tracer_tags.contains_key("tags"));

        assert_eq!(row.link_trace_id, vec![0x6f]);
        assert_eq!(row.link_trace_id_high, vec![0x67890abcdef01234]);
        assert_eq!(row.link_tracestate, vec!["dd=s:1"]);
        assert_eq!(row.event_name, vec!["exception"]);
    }

    #[test]
    fn a_string_reference_that_is_zero_or_out_of_range_resolves_to_empty() {
        let table = strings(&["", "a"]);
        assert_eq!(string_at(&table, 0), "");
        assert_eq!(string_at(&table, 1), "a");
        assert_eq!(string_at(&table, 99), "");
    }

    #[test]
    fn a_trace_id_of_any_length_splits_into_high_and_low() {
        assert_eq!(split_trace_id(&[]), (0, 0));
        assert_eq!(split_trace_id(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2]), (1 << 56, 2));
        // A short id is left-padded, a long one keeps its last sixteen bytes.
        assert_eq!(split_trace_id(&[0xff]), (0, 0xff));
        assert_eq!(split_trace_id(&[[0xaa, 0xbb].as_slice(), &[0; 16]].concat()), (0, 0));
    }

    #[test]
    fn a_nested_attribute_is_cut_at_the_depth_limit_and_says_so() {
        use idx::any_value::Value as Any;
        let table = strings(&["", "deep", "leaf"]);
        // One array per level, far past the limit.
        let mut value = idx_string(2);
        for _ in 0..ATTRIBUTE_MAX_DEPTH * 4 {
            value = idx_any(Any::ArrayValue(idx::ArrayValue { values: vec![value] }));
        }
        let json = idx_attributes_json(&table, &HashMap::from([(1, value)]));
        assert!(json.contains("\"_depth_exceeded\""));
        // The marker replaces the tail, so the leaf never appears.
        assert!(!json.contains("\"leaf\""));

        // The two or three levels real tracers send still come out whole.
        let array = idx_any(Any::ArrayValue(idx::ArrayValue { values: vec![idx_string(2)] }));
        let list = idx::KeyValueList { key_values: vec![idx::KeyValue { key: 1, value: Some(array) }] };
        assert_eq!(
            idx_attributes_json(&table, &HashMap::from([(1, idx_any(Any::KeyValueList(list)))])),
            r#"{"deep":{"kvlist":[{"key":"deep","value":{"array":[{"string":"leaf"}]}}]}}"#
        );
    }

    #[test]
    fn a_float_attribute_is_written_in_the_layout_the_columns_hold() {
        for (value, expected) in [
            (1e21, "1e+21"),
            (1e-7, "1e-07"),
            (123456789.125, "1.23456789125e+08"),
            (100000.0, "100000"),
            (1000000.0, "1e+06"),
            (0.000123, "0.000123"),
            (-2.5e-5, "-2.5e-05"),
            (-0.0, "-0"),
            (0.5, "0.5"),
            (f64::NAN, "NaN"),
            (f64::NEG_INFINITY, "-Inf"),
        ] {
            assert_eq!(text::of_float(value), expected);
        }
    }

    // --- /api/v0.2/stats ---

    #[test]
    fn stat_rows_carry_payload_client_and_bucket_and_keep_the_sketches() {
        let ok_bytes = go_sketch();
        let first = ClientGroupedStats {
            service: "checkout".into(),
            name: "flask.request".into(),
            resource: "GET /pay".into(),
            http_status_code: 200,
            r#type: "web".into(),
            hits: 120,
            errors: 3,
            duration: 900_000_000,
            top_level_hits: 120,
            span_kind: "server".into(),
            is_trace_root: Trilean::True as i32,
            http_method: "GET".into(),
            http_endpoint: "/pay".into(),
            ok_summary: ok_bytes.clone(),
            error_summary: b"not a sketch".to_vec(),
            peer_tags: strings(&["db.hostname:pg-1"]),
            additional_metric_tags: strings(&["region:eu"]),
            ..Default::default()
        };
        let second = ClientGroupedStats {
            service: "checkout".into(),
            name: "postgres.query".into(),
            is_trace_root: Trilean::NotSet as i32,
            ..Default::default()
        };
        let bucket = ClientStatsBucket {
            start: 1_700_000_000_000_000_000,
            duration: 10_000_000_000,
            agent_time_shift: -500,
            stats: vec![first, second],
        };
        let client = ClientStatsPayload {
            hostname: "pod-1".into(),
            env: "prod".into(),
            version: "1.4.0".into(),
            lang: "python".into(),
            tracer_version: "2.9.0".into(),
            runtime_id: "runtime-1".into(),
            sequence: 7,
            service: "checkout".into(),
            container_id: "abc123".into(),
            git_commit_sha: "deadbeef".into(),
            image_tag: "v1.4.0".into(),
            process_tags: "entrypoint:gunicorn".into(),
            process_tags_hash: u64::MAX,
            // A pod behind two services sends kube_service twice.
            tags: strings(&["kube_service:a", "kube_service:b", "bare"]),
            stats: vec![bucket],
            ..Default::default()
        };
        let payload = StatsPayload {
            agent_hostname: "agent-1".into(),
            agent_env: "prod".into(),
            agent_version: "7.58.2".into(),
            client_computed: true,
            split_payload: true,
            stats: vec![client],
        };

        let rows = stat_rows("t1", received_at(), &payload);
        assert_eq!(rows.len(), 2);
        let row = &rows[0];
        assert_eq!(row.agent_version, "7.58.2");
        assert_eq!((row.client_computed, row.split_payload), (1, 1));
        assert_eq!(row.client_runtime_id, "runtime-1");
        assert_eq!(row.client_sequence, 7);
        assert_eq!(row.client_process_tags_hash, u64::MAX);
        assert_eq!(row.client_tags["kube_service"], vec!["a", "b"]);
        assert_eq!(row.client_tags["bare"], vec![""]);
        assert_eq!(row.bucket_start, 1_700_000_000_000_000_000);
        assert_eq!(row.agent_time_shift_ns, -500);
        // NOT_SET and FALSE are different answers.
        assert_eq!(row.is_trace_root, TraceRoot::True);
        assert_eq!(rows[1].is_trace_root, TraceRoot::NotSet);

        assert_eq!(row.ok_summary_state, SketchState::Ok);
        assert_eq!(row.ok_summary, Blob(ok_bytes));
        assert_eq!(row.ok_count, Some(3.0));
        assert_eq!((row.ok_bin_keys.len(), row.ok_bin_counts.len()), (3, 3));
        assert!(matches!(row.error_summary_state, SketchState::Undecodable(_)));
        assert_eq!(row.error_summary, Blob(b"not a sketch".to_vec()));
        assert_eq!(row.error_count, None);
        assert_eq!(rows[1].ok_summary_state, SketchState::Absent);
        assert!(rows[1].ok_summary.0.is_empty());
    }

    // --- /api/v0.1/pipeline_stats ---

    /// Just enough MessagePack to write what dd-trace-go sends.
    mod pack {
        pub fn map(count: u8) -> Vec<u8> {
            vec![0x80 | count]
        }
        pub fn array(count: u8) -> Vec<u8> {
            vec![0x90 | count]
        }
        pub fn nil() -> Vec<u8> {
            vec![0xc0]
        }
        pub fn uint(value: u64) -> Vec<u8> {
            [vec![0xcf], value.to_be_bytes().to_vec()].concat()
        }
        pub fn int(value: i64) -> Vec<u8> {
            [vec![0xd3], value.to_be_bytes().to_vec()].concat()
        }
        pub fn bin(bytes: &[u8]) -> Vec<u8> {
            [vec![0xc4, bytes.len() as u8], bytes.to_vec()].concat()
        }
        pub fn str(text: &str) -> Vec<u8> {
            let bytes = text.as_bytes();
            let head = if bytes.len() < 32 { vec![0xa0 | bytes.len() as u8] } else { vec![0xd9, bytes.len() as u8] };
            [head, bytes.to_vec()].concat()
        }
    }

    #[test]
    fn a_pipeline_stats_payload_is_decoded_by_its_go_field_names_unknown_ones_kept() {
        let pathway = go_sketch();
        let point = [
            pack::map(7),
            pack::str("EdgeTags"),
            pack::array(2),
            pack::str("type:kafka"),
            pack::str("topic:orders"),
            pack::str("Hash"),
            pack::uint(u64::MAX),
            pack::str("ParentHash"),
            pack::uint(42),
            pack::str("PathwayLatency"),
            pack::bin(&pathway),
            pack::str("EdgeLatency"),
            pack::nil(),
            pack::str("PayloadSize"),
            pack::bin(b"garbage"),
            pack::str("TimestampType"),
            pack::str("current"),
        ]
        .concat();
        let backlog = [
            pack::map(2),
            pack::str("Tags"),
            pack::array(2),
            pack::str("type:kafka_commit"),
            pack::str("partition:3"),
            pack::str("Value"),
            pack::int(-17),
        ]
        .concat();
        let bucket = [
            pack::map(5),
            pack::str("Start"),
            pack::uint(1_700_000_000_000_000_000),
            pack::str("Duration"),
            pack::uint(10_000_000_000),
            pack::str("Stats"),
            pack::array(1),
            point,
            pack::str("Backlogs"),
            pack::array(1),
            backlog,
            pack::str("Transactions"),
            pack::bin(&[1, 2, 3]),
        ]
        .concat();
        let body = [
            pack::map(9),
            pack::str("Env"),
            pack::str("prod"),
            pack::str("Service"),
            pack::str("orders"),
            pack::str("TracerVersion"),
            pack::str("2.9.0"),
            pack::str("Lang"),
            pack::str("go"),
            pack::str("Version"),
            pack::str("1.4.0"),
            pack::str("ProcessTags"),
            pack::array(2),
            pack::str("entrypoint:worker"),
            pack::str("cwd:/srv"),
            pack::str("ProductMask"),
            pack::uint(3),
            // A field this decoder has never heard of.
            pack::str("SomethingNew"),
            pack::str("hello"),
            pack::str("Stats"),
            pack::array(1),
            bucket,
        ]
        .concat();

        let payload = decode_dsm_payload(&body).unwrap();
        assert_eq!(payload.env, "prod");
        assert_eq!(payload.service, "orders");
        assert_eq!(payload.lang, "go");
        assert_eq!(payload.product_mask, 3);
        assert_eq!(payload.process_tags, vec!["entrypoint:worker", "cwd:/srv"]);
        assert_eq!(payload.unknown.get("SomethingNew"), Some(&MsgValue::Str("hello".into())));

        assert_eq!(payload.buckets.len(), 1);
        let decoded_bucket = &payload.buckets[0];
        assert_eq!((decoded_bucket.points.len(), decoded_bucket.backlogs.len()), (1, 1));
        let decoded_point = &decoded_bucket.points[0];
        assert_eq!(decoded_point.hash, u64::MAX);
        assert_eq!(decoded_point.edge_tags, vec!["type:kafka", "topic:orders"]);
        assert_eq!(decoded_point.pathway_latency, pathway);
        assert!(decoded_point.edge_latency.is_empty());
        assert_eq!(decoded_bucket.backlogs[0].value, -17);
        assert_eq!(decoded_bucket.transactions, vec![1, 2, 3]);

        // …and through to the rows.
        let headers = DsmHeaders { via: "trace-agent".into(), ..Default::default() };
        let found = dsm_rows("t1", received_at(), &payload, &headers);
        assert_eq!((found.points.len(), found.backlogs.len(), found.transactions.len()), (1, 1, 1));
        let point_row = &found.points[0];
        assert_eq!(point_row.service, "orders");
        assert_eq!(point_row.via, "trace-agent");
        assert_eq!(point_row.pathway_latency_state, SketchState::Ok);
        assert_eq!(point_row.edge_latency_state, SketchState::Absent);
        assert!(matches!(point_row.payload_size_state, SketchState::Undecodable(_)));
        assert_eq!(point_row.payload_size, Blob(b"garbage".to_vec()));
        assert_eq!(point_row.unknown_keys, vec!["payload.SomethingNew"]);
        assert_eq!(point_row.unknown_json, r#"{"payload.SomethingNew":"hello"}"#);
        assert_eq!(found.backlogs[0].value, -17);
        assert_eq!(found.backlogs[0].tags.len(), 2);
        assert_eq!(found.transactions[0].transactions, Blob(vec![1, 2, 3]));
    }

    #[test]
    fn a_body_that_is_not_a_pipeline_stats_payload_is_refused() {
        assert_eq!(decode_dsm_payload(&[]), Err("empty body".into()));
        assert!(decode_dsm_payload(br#"{"Env":"prod"}"#).is_err());
        // A map that announces one entry and ends.
        assert_eq!(decode_dsm_payload(&[0x81]), Err(msgpack::TRUNCATED.into()));
    }

    #[test]
    fn unknown_values_are_written_as_json_bytes_as_base64() {
        let value = MsgValue::Map(vec![
            ("f".into(), MsgValue::Float32(1.1)),
            ("n".into(), MsgValue::Float(f64::NAN)),
            ("b".into(), MsgValue::Bin(vec![0, 1])),
            ("e".into(), MsgValue::Bin(Vec::new())),
            ("x".into(), MsgValue::Ext(5, vec![9])),
        ]);
        assert_eq!(
            json::compact(&unknown_json(&value)),
            r#"{"b":"AAE=","e":null,"f":1.1,"n":"NaN","x":{"Data":"CQ==","Type":5}}"#
        );
    }

    // --- /api/v2/data_streams_messages ---

    #[test]
    fn message_rows_keep_the_text_the_place_in_the_batch_and_the_key_names() {
        let messages = json::try_parse_element_bytes(br#"[{"b":1,"a":{"nested":true}}, "just a string"]"#).unwrap();
        let rows = dsm_message_rows("t1", received_at(), messages, "browser", "5.0.0", "deflate");
        assert_eq!(rows.len(), 2);
        assert_eq!((rows[0].position, rows[1].position), (0, 1));
        assert_eq!(rows[0].message, Blob(br#"{"b":1,"a":{"nested":true}}"#.to_vec()));
        assert_eq!(rows[0].keys, vec!["a", "b"]);
        // A message that is not an object has no keys.
        assert!(rows[1].keys.is_empty());
        assert_eq!(rows[0].dd_evp_origin, "browser");
        assert_eq!(rows[0].dd_evp_origin_version, "5.0.0");
        assert_eq!(rows[0].content_encoding, "deflate");
    }

    #[test]
    fn a_message_that_is_not_utf8_is_kept_byte_for_byte() {
        let messages = json::try_parse_element_bytes(b"[{\"a\":\"x\xffy\"}]").unwrap();
        assert_eq!(messages, vec![b"{\"a\":\"x\xffy\"}".to_vec()]);
        // The keys are still read: only the value was not UTF-8.
        assert_eq!(message_keys(messages[0]), vec!["a"]);
    }
}
