//! CI Visibility / Test Optimization.
//!
//!   citestcycle-intake.<site>  POST /api/v2/citestcycle   msgpack (or JSON)
//!   citestcov-intake.<site>    POST /api/v2/citestcov     multipart + msgpack
//!   api.<site>                 what a tracer consults, git metadata, the datadog-ci CLI
//!
//! Most of the api.<site> endpoints are READS: the tracer asks what to do — is coverage on, which
//! tests may be skipped — and then behaves according to the answer. A wrong answer here changes
//! what a customer's test suite does, so every answer keeps the field names of the client struct
//! that parses it, and the safe answer is always "feature off, list empty".
//!
//! A tracer reaches these directly, or through the local agent's EVP proxy, which rewrites the
//! host and adds headers of its own (see [`agent_headers`]).
//!
//! The backend's own field catalogue is not public, so the decoders know every field a shipped
//! client writes (dd-trace-go, dd-trace-py, datadog-ci), and each table keeps a raw column for the
//! rest.

use std::collections::{BTreeMap, HashMap};

use base64::Engine;
use bytes::Bytes;
use chrono::{DateTime, Utc};
use http::{HeaderValue, StatusCode};
use ninjapaws_storage as storage;
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::Extra;
use ninjapaws_storage::rows::ci::{
    CiCoverage, CiPipelineEvent, CiSettingsRequest, CiTestEvent, GitCommit, GitPackfile,
};
use ninjapaws_storage::rows::raw::RawPayload;
use rmp::decode::ValueReadError;
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use tako::body::TakoBody;
use tako::extractors::state::State;
use tako::types::Response;

use crate::body;
use crate::fields::{self, Mismatches};
use crate::json;
use crate::msgpack::{self, MsgValue};
use crate::raw;
use crate::server::{Body, Ctx, accepted, json, status, storage_failed};
use crate::wire::text;

/// A decoded payload value. A test-cycle payload arrives as msgpack or as JSON, and both become
/// this, so the row builders are written once.
///
/// The rule that shapes it: a 64-bit id never passes through a float. A session id above 2^53
/// would come back rounded, and the row would then join to nothing. So a JSON number is kept as
/// its literal, and msgpack integers stay integers.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Float32(f32),
    Float(f64),
    /// A JSON number, as it was written.
    Number(String),
    String(String),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Object(BTreeMap<String, Value>),
    /// A msgpack extension.
    Ext(i8, Vec<u8>),
}

static NULL: Value = Value::Null;

const TWO_TO_53: f64 = 9007199254740992.0;

impl Value {
    pub fn of_msgpack(value: MsgValue) -> Value {
        match value {
            MsgValue::Nil => Value::Null,
            MsgValue::Bool(b) => Value::Bool(b),
            MsgValue::Int(n) => Value::Int(n),
            MsgValue::UInt(n) => Value::UInt(n),
            MsgValue::Float32(f) => Value::Float32(f),
            MsgValue::Float(f) => Value::Float(f),
            MsgValue::Str(s) => Value::String(s),
            MsgValue::Bin(b) => Value::Bytes(b),
            MsgValue::Array(items) => Value::List(items.into_iter().map(Value::of_msgpack).collect()),
            // A repeated key keeps its last value.
            MsgValue::Map(entries) => {
                Value::Object(entries.into_iter().map(|(k, v)| (k, Value::of_msgpack(v))).collect())
            }
            MsgValue::Ext(kind, data) => Value::Ext(kind, data),
        }
    }

    /// A parsed JSON value. Its numbers are still the literals they were written as: serde_json
    /// keeps them as text (`arbitrary_precision`).
    pub fn of_json(value: &serde_json::Value) -> Value {
        match value {
            serde_json::Value::Null => Value::Null,
            serde_json::Value::Bool(b) => Value::Bool(*b),
            serde_json::Value::Number(n) => Value::Number(n.to_string()),
            serde_json::Value::String(s) => Value::String(s.clone()),
            serde_json::Value::Array(items) => Value::List(items.iter().map(Value::of_json).collect()),
            // Of a repeated key the parser kept the last.
            serde_json::Value::Object(members) => {
                Value::Object(members.iter().map(|(k, v)| (k.clone(), Value::of_json(v))).collect())
            }
        }
    }

    /// One key of an object. Null when the key is absent, and when the value is not an object at
    /// all.
    pub fn field(&self, key: &str) -> &Value {
        match self {
            Value::Object(fields) => fields.get(key).unwrap_or(&NULL),
            _ => &NULL,
        }
    }

    /// Whether an object has the key, whatever its value.
    pub fn has(&self, key: &str) -> bool {
        matches!(self, Value::Object(fields) if fields.contains_key(key))
    }

    /// False when the value holds a float JSON has no form for.
    fn write(&self, out: &mut String) -> bool {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Int(n) => out.push_str(&n.to_string()),
            Value::UInt(n) => out.push_str(&n.to_string()),
            Value::Float32(f) if f.is_finite() => out.push_str(&serde_json::to_string(f).unwrap()),
            Value::Float(f) if f.is_finite() => out.push_str(&serde_json::to_string(f).unwrap()),
            // NaN and the infinities have no JSON form, so a value that holds one has none either.
            Value::Float32(_) | Value::Float(_) => return false,
            Value::Number(literal) => out.push_str(literal),
            Value::String(s) => out.push_str(&json::quoted(s)),
            Value::Bytes(data) => out.push_str(&json::quoted(&base64::engine::general_purpose::STANDARD.encode(data))),
            Value::List(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    if !item.write(out) {
                        return false;
                    }
                }
                out.push(']');
            }
            Value::Object(fields) => {
                out.push('{');
                for (i, (key, value)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push_str(&json::quoted(key));
                    out.push(':');
                    if !value.write(out) {
                        return false;
                    }
                }
                out.push('}');
            }
            // An extension has no JSON form: its bytes and its type number.
            Value::Ext(kind, data) => {
                let data = base64::engine::general_purpose::STANDARD.encode(data);
                out.push_str(&format!("{{\"Data\":{},\"Type\":{kind}}}", json::quoted(&data)));
            }
        }
        true
    }

    /// The value as JSON text, for a column that keeps it whole. Null is "" (no value at all).
    pub fn to_json(&self) -> String {
        if *self == Value::Null {
            return String::new();
        }
        let mut out = String::new();
        if self.write(&mut out) { out } else { String::new() }
    }

    /// The value as the text a column holds: exact, never rounded.
    pub fn text(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::String(s) => s.clone(),
            Value::Bytes(data) => text::utf8(data),
            Value::Number(literal) => literal.clone(),
            Value::Bool(b) => b.to_string(),
            Value::Int(n) => n.to_string(),
            Value::UInt(n) => n.to_string(),
            Value::Float(f) => text::of_float(*f),
            Value::Float32(f) => text::of_float32(*f),
            Value::List(_) | Value::Object(_) | Value::Ext(..) => self.to_json(),
        }
    }

    pub fn field_text(&self, key: &str) -> String {
        self.field(key).text()
    }

    /// The value as the bytes a column holds: binary data as it is, anything else as its text.
    pub fn bytes(&self) -> Vec<u8> {
        match self {
            Value::Bytes(data) => data.clone(),
            other => other.text().into_bytes(),
        }
    }

    /// The value as an id.
    ///
    /// A float is taken only when it is a whole number below 2^53: past that the float is already
    /// not the number that was sent, and 0 (which reads as "absent") beats a number that is
    /// almost right.
    pub fn to_u64(&self) -> u64 {
        match self {
            Value::UInt(n) => *n,
            Value::Int(n) => u64::try_from(*n).unwrap_or(0),
            Value::Number(digits) | Value::String(digits) => parse_u64(digits).unwrap_or(0),
            Value::Float(f) if *f >= 0.0 && *f == f.trunc() && *f < TWO_TO_53 => *f as u64,
            _ => 0,
        }
    }

    /// The value as a signed integer, with the same rule for floats.
    pub fn to_i64(&self) -> i64 {
        match self {
            Value::Int(n) => *n,
            Value::UInt(n) => i64::try_from(*n).unwrap_or(0),
            Value::Number(digits) | Value::String(digits) => parse_i64(digits).unwrap_or(0),
            Value::Float(f) if *f == f.trunc() && f.abs() < TWO_TO_53 => *f as i64,
            _ => 0,
        }
    }

    /// For a Nullable column: None when the value is absent, so "the producer never said" stays
    /// apart from zero.
    pub fn try_i64(&self) -> Option<i64> {
        match self {
            Value::Null => None,
            other => Some(other.to_i64()),
        }
    }

    /// The value as a float, for a metrics map.
    pub fn to_float(&self) -> f64 {
        match self {
            Value::Float(f) => *f,
            Value::Float32(f) => f64::from(*f),
            Value::Int(n) => *n as f64,
            Value::UInt(n) => *n as f64,
            Value::Number(digits) | Value::String(digits) => match digits.to_lowercase().as_str() {
                "inf" | "+inf" | "infinity" | "+infinity" => f64::INFINITY,
                "-inf" | "-infinity" => f64::NEG_INFINITY,
                "nan" => f64::NAN,
                // A sign, digits, a point and an exponent; no space, no other spelling.
                _ if digits.bytes().all(|b| b.is_ascii_digit() || b"+-.eE".contains(&b)) => {
                    digits.parse().unwrap_or(0.0)
                }
                _ => 0.0,
            },
            _ => 0.0,
        }
    }

    /// The elements of an array; none for anything else.
    pub fn items(&self) -> &[Value] {
        match self {
            Value::List(items) => items,
            _ => &[],
        }
    }

    /// The value as a list of strings. A bare string counts as a list of one: several producers
    /// send a single label unwrapped.
    pub fn strings(&self) -> Vec<String> {
        match self {
            Value::List(items) => items.iter().map(Value::text).collect(),
            Value::String(s) if !s.is_empty() => vec![s.clone()],
            _ => Vec::new(),
        }
    }

    /// An object as a string map, each value rendered exactly.
    pub fn string_map(&self) -> BTreeMap<String, String> {
        match self {
            Value::Object(fields) => fields.iter().map(|(k, v)| (k.clone(), v.text())).collect(),
            _ => BTreeMap::new(),
        }
    }

    /// An object as a numeric map, for a metrics column.
    pub fn float_map(&self) -> BTreeMap<String, f64> {
        match self {
            Value::Object(fields) => fields.iter().map(|(k, v)| (k.clone(), v.to_float())).collect(),
            _ => BTreeMap::new(),
        }
    }

    /// The keys of an object that `known` does not name, each value as JSON so a nested object
    /// survives.
    pub fn unknown(&self, known: &[&str]) -> Extra {
        match self {
            Value::Object(fields) => fields
                .iter()
                .filter(|(key, _)| !known.contains(&key.as_str()))
                .map(|(k, v)| (k.clone(), v.to_json()))
                .collect(),
            _ => Extra::new(),
        }
    }
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// Digits only: no sign, no space.
fn parse_u64(text: &str) -> Option<u64> {
    is_digits(text).then(|| text.parse().ok()).flatten()
}

/// Digits after at most one sign; no space.
fn parse_i64(text: &str) -> Option<i64> {
    is_digits(text.strip_prefix(['-', '+']).unwrap_or(text)).then(|| text.parse().ok()).flatten()
}

/// The root of a JSON text that has been repaired (`json::repair`), whole.
pub(crate) fn raw_root(repaired: &[u8]) -> Result<&RawValue, String> {
    serde_json::from_slice(repaired).map_err(|e| e.to_string())
}

/// The member at the end of a path of names, as the text it was written in, `null` included. None
/// when a member on the way is missing or what should hold it is not an object. Of a repeated
/// name the last counts. Each name reads the text below it once, so a path is a few passes over
/// the body however deep the body nests.
pub(crate) fn raw_at<'a>(root: &'a RawValue, path: &[&str]) -> Option<&'a RawValue> {
    path.iter().try_fold(root, |current, name| {
        let members: HashMap<String, &'a RawValue> = serde_json::from_str(current.get()).ok()?;
        members.get(*name).copied()
    })
}

fn or_unknown(content_type: &str) -> &str {
    if content_type.is_empty() { "(no content-type)" } else { content_type }
}

/// What a payload carries about the hop it took: direct, or through the agent's EVP proxy.
#[derive(Clone, Debug, Default)]
pub struct AgentHeaders {
    pub subdomain: String,
    pub container_id: String,
    pub hostname: String,
    pub agent_version: String,
}

/// The header names are the agent's real ones (pkg/trace/api/evp_proxy.go), not the obvious ones:
/// the container id has no X- prefix, and the version is only in "Via: trace-agent <version>".
/// X-Datadog-Container-Id and X-Datadog-AgentVersion are sent by no agent; they are read in case a
/// proxy invented them. The proxy consumes X-Datadog-EVP-Subdomain, so a value there means
/// something other than a datadog-agent sent the request.
pub fn agent_headers(ctx: &Ctx) -> AgentHeaders {
    let via_version = ctx.header("via").strip_prefix("trace-agent ").map(str::trim).unwrap_or_default();
    AgentHeaders {
        subdomain: ctx.header("x-datadog-evp-subdomain").into(),
        container_id: text::first_non_empty(&[
            ctx.header("datadog-container-id"),
            ctx.header("x-datadog-container-id"),
        ])
        .into(),
        hostname: ctx.header("x-datadog-hostname").into(),
        agent_version: text::first_non_empty(&[ctx.header("x-datadog-agentversion"), via_version]).into(),
    }
}

fn looks_json(body: &[u8]) -> bool {
    matches!(body.iter().find(|b| !b" \t\r\n".contains(b)), Some(b'{' | b'['))
}

/// Decodes a body that may be msgpack or JSON; says which it was taken for.
///
/// Content-Type decides when it names one of the two, otherwise the first byte does: JSON starts
/// with '{' or '[', and no msgpack map or array header shares those bytes. Sniffing matters
/// because the two shipped clients label the coverage part differently.
pub fn decode_any(content_type: &str, body: &[u8]) -> (&'static str, Result<Value, String>) {
    let msgpack = || ("msgpack", msgpack::decode(body).map(Value::of_msgpack));
    let json = || ("json", json::try_parse_first(body).map(|root| Value::of_json(&root)));
    let declared = content_type.to_lowercase();
    if declared.contains("msgpack") {
        msgpack()
    } else if declared.contains("json") || looks_json(body) {
        json()
    } else {
        msgpack()
    }
}

/// The envelope's metadata: event type → tags, where "*" holds the defaults for every type
/// (dd-trace-py puts language, runtime-id, library_version and env there). Entries that are not a
/// non-empty object are dropped.
fn cycle_metadata(value: &Value) -> BTreeMap<String, BTreeMap<String, String>> {
    match value {
        Value::Object(entries) => entries
            .iter()
            .map(|(name, entry)| (name.clone(), entry.string_map()))
            .filter(|(_, tags)| !tags.is_empty())
            .collect(),
        _ => BTreeMap::new(),
    }
}

/// The "*" entry with the event type's own entry on top: the more specific statement wins.
/// Flattened onto each row so a row can be read without the payload it came in.
fn metadata_for(metadata: &BTreeMap<String, BTreeMap<String, String>>, event_type: &str) -> BTreeMap<String, String> {
    let mut merged = metadata.get("*").cloned().unwrap_or_default();
    merged.extend(metadata.get(event_type).cloned().unwrap_or_default());
    merged
}

/// A boolean the tracer wrote into the string meta map. Absent is None, not false: a tracer with
/// early flake detection off sends no test.is_new at all, and "was not looking" must stay apart
/// from "is not new".
fn flag_tag(meta: &BTreeMap<String, String>, key: &str) -> Option<u8> {
    let value = meta.get(key)?.trim().to_lowercase();
    Some(text::flag(value == "true" || value == "1" || value == "yes"))
}

/// A number the tracer may have put in either map: a numeric tag goes to metrics, a string one to
/// meta, and test.source.start has been seen in both.
fn number_tag(meta: &BTreeMap<String, String>, metrics: &BTreeMap<String, f64>, key: &str) -> Option<i64> {
    match (metrics.get(key), meta.get(key)) {
        (Some(number), _) => Some(*number as i64),
        (None, Some(written)) => parse_i64(written.trim()),
        (None, None) => None,
    }
}

/// One event as a row. The tag columns are copies of meta entries, for query speed: a tracer that
/// renames a tag loses a column and keeps the data.
pub fn test_event_row(
    tenant: &str,
    received_at: DateTime<Utc>,
    headers: &AgentHeaders,
    payload_version: i32,
    metadata: &BTreeMap<String, BTreeMap<String, String>>,
    event: &Value,
) -> CiTestEvent {
    let event_type = event.field_text("type");
    let content = event.field("content");
    let meta = content.field("meta").string_map();
    let metrics = content.field("metrics").float_map();
    let merged = metadata_for(metadata, &event_type);
    let tag = |key: &str| meta.get(key).cloned().unwrap_or_default();
    // These belong to the envelope; the span's own tag wins when it set one.
    let tag_or_metadata = |key: &str| {
        let own = tag(key);
        if own.is_empty() { merged.get(key).cloned().unwrap_or_default() } else { own }
    };

    CiTestEvent {
        tenant_id: tenant.into(),
        received_at,
        event_type: event_type.clone(),
        event_version: event.field("version").to_i64() as i32,
        payload_version,
        session_id: content.field("test_session_id").to_u64(),
        module_id: content.field("test_module_id").to_u64(),
        suite_id: content.field("test_suite_id").to_u64(),
        trace_id: content.field("trace_id").to_u64(),
        span_id: content.field("span_id").to_u64(),
        parent_id: content.field("parent_id").to_u64(),
        itr_correlation_id: content.field_text("itr_correlation_id"),
        service: content.field_text("service"),
        env: tag_or_metadata("env"),
        name: content.field_text("name"),
        resource: content.field_text("resource"),
        span_type: content.field_text("type"),
        // Not defaulted to now when absent: a row dated 1970 says the producer sent no start, and
        // an invented time would look like a real one.
        start: content.field("start").to_i64(),
        duration_ns: content.field("duration").to_i64(),
        error: content.field("error").to_i64() as i32,
        test_name: tag("test.name"),
        test_suite: tag("test.suite"),
        test_module: tag("test.module"),
        test_framework: tag("test.framework"),
        test_framework_version: tag("test.framework_version"),
        test_status: tag("test.status"),
        test_type: tag("test.type"),
        test_source_file: tag("test.source.file"),
        test_source_start: number_tag(&meta, &metrics, "test.source.start"),
        test_source_end: number_tag(&meta, &metrics, "test.source.end"),
        test_parameters: tag("test.parameters"),
        test_codeowners: tag("test.codeowners"),
        test_command: tag("test.command"),
        test_session_name: tag("test_session.name"),
        git_repository_url: tag("git.repository_url"),
        git_branch: tag("git.branch"),
        git_tag: tag("git.tag"),
        git_commit_sha: tag("git.commit.sha"),
        git_commit_message: tag("git.commit.message"),
        git_commit_author_name: tag("git.commit.author.name"),
        git_commit_author_email: tag("git.commit.author.email"),
        git_commit_author_date: tag("git.commit.author.date"),
        git_commit_committer_name: tag("git.commit.committer.name"),
        git_commit_committer_email: tag("git.commit.committer.email"),
        git_commit_committer_date: tag("git.commit.committer.date"),
        ci_provider_name: tag("ci.provider.name"),
        ci_pipeline_id: tag("ci.pipeline.id"),
        ci_pipeline_name: tag("ci.pipeline.name"),
        ci_pipeline_number: tag("ci.pipeline.number"),
        ci_pipeline_url: tag("ci.pipeline.url"),
        ci_job_id: tag("ci.job.id"),
        ci_job_name: tag("ci.job.name"),
        ci_job_url: tag("ci.job.url"),
        ci_stage_name: tag("ci.stage.name"),
        ci_workspace_path: tag("ci.workspace_path"),
        ci_node_name: tag("ci.node.name"),
        ci_node_labels: tag("ci.node.labels"),
        os_platform: tag("os.platform"),
        os_version: tag("os.version"),
        os_architecture: tag("os.architecture"),
        runtime_name: tag("runtime.name"),
        runtime_version: tag("runtime.version"),
        language: tag_or_metadata("language"),
        runtime_id: tag_or_metadata("runtime-id"),
        library_version: tag_or_metadata("library_version"),
        test_is_new: flag_tag(&meta, "test.is_new"),
        test_is_retry: flag_tag(&meta, "test.is_retry"),
        test_is_modified: flag_tag(&meta, "test.is_modified"),
        test_skipped_by_itr: flag_tag(&meta, "test.skipped_by_itr"),
        itr_unskippable: flag_tag(&meta, "test.itr.unskippable"),
        itr_forced_run: flag_tag(&meta, "test.itr.forced_run"),
        code_coverage_enabled: flag_tag(&meta, "test.code_coverage.enabled"),
        test_retry_reason: tag("test.retry_reason"),
        early_flake_abort_reason: tag("test.early_flake.abort_reason"),
        evp_subdomain: headers.subdomain.clone(),
        container_id: headers.container_id.clone(),
        agent_hostname: headers.hostname.clone(),
        agent_version: headers.agent_version.clone(),
        // JSON whatever the request's format was: it is the spelling the tracer's own
        // documentation uses for an event.
        content: content.to_json(),
        meta,
        metrics,
        metadata: merged,
    }
}

/// The payload's rows. What does not decode is kept raw, into `kept`.
fn test_cycle_rows(ctx: &Ctx, body: &[u8], kept: &mut Vec<RawPayload>) -> Vec<CiTestEvent> {
    let content_type = ctx.header("content-type");
    let (format, decoded) = decode_any(content_type, body);
    let envelope = match decoded {
        Err(e) => {
            let note = format!("{format} envelope, content-type {}: {e}", or_unknown(content_type));
            kept.extend(raw::row(ctx, "citestcycle", "decode_error", &note, body));
            return Vec::new();
        }
        Ok(envelope @ Value::Object(_)) => envelope,
        Ok(_) => {
            kept.extend(raw::row(
                ctx,
                "citestcycle",
                "unexpected_shape",
                &format!("payload is not a {format} map"),
                body,
            ));
            return Vec::new();
        }
    };
    let events = envelope.field("events").items();
    if events.is_empty() {
        // Not an error — the encoder emits one when a flush finds nothing — but not a shape with
        // a published meaning either, so it is kept.
        kept.extend(raw::row(ctx, "citestcycle", "unexpected_shape", "envelope carries no events", body));
        return Vec::new();
    }
    let payload_version = envelope.field("version").to_i64() as i32;
    let metadata = cycle_metadata(envelope.field("metadata"));
    let now = Utc::now();
    let headers = agent_headers(ctx);
    let mut rows = Vec::new();
    let mut not_maps = Vec::new();
    for (i, event) in events.iter().enumerate() {
        match event {
            Value::Object(_) => {
                rows.push(test_event_row(ctx.tenant(), now, &headers, payload_version, &metadata, event))
            }
            _ => not_maps.push(i),
        }
    }
    // The body is kept once, however many of its events are bad.
    if !not_maps.is_empty() {
        let places: Vec<String> = not_maps.iter().take(8).map(usize::to_string).collect();
        let more = if not_maps.len() > 8 { ", …" } else { "" };
        let note = format!("{} of {} events are not maps: {}{more}", not_maps.len(), events.len(), places.join(", "));
        kept.extend(raw::row(ctx, "citestcycle", "decode_error", &note, body));
    }
    rows
}

/// A test-cycle payload: {version, metadata, events[]}, whose events are the tests, suites,
/// modules and sessions of a CI run.
///
/// The tracer's transport treats any status below 400 as success and never retries, so a payload
/// refused here is simply gone. A body that does not decode is therefore answered 202 and kept
/// raw.
pub async fn handle_test_cycle(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    if body.is_empty() {
        return accepted();
    }
    let mut kept = Vec::new();
    let rows = test_cycle_rows(&ctx, &body, &mut kept);
    if let Err(e) = raw::keep(&clickhouse, &kept).await {
        return storage_failed(e);
    }
    if let Err(e) = storage::insert(&clickhouse, &rows).await {
        return storage_failed(e);
    }
    accepted()
}

/// One coverages[] element: its decoded value and the bytes it occupied. The bytes are kept per
/// entry, not per request: a payload holds one entry per test, and storing the whole part on every
/// row would cost the square of its size.
#[derive(Clone, Debug)]
pub struct CoverageEntry {
    pub value: Value,
    pub raw: Vec<u8>,
}

fn coverage_entries_json(data: &[u8]) -> Result<(i32, Vec<CoverageEntry>), String> {
    let root = json::try_parse_first(data)?;
    let bad = Mismatches::default();
    let document = fields::fields(&bad, "", &root);
    document.items("coverages");
    if let Some(problem) = bad.borrow().first() {
        return Err(problem.clone());
    }
    // Read like the msgpack form: a version nobody can read is 0, and never a reason to lose the
    // coverage that came with it.
    let version = document.find("version").map_or(0, |found| Value::of_json(found).to_i64() as i32);
    // Each entry's bytes are its text in the body, the elements of the same list.
    let repaired = json::repair(data);
    let text = std::str::from_utf8(&repaired).map_err(|e| e.to_string())?;
    let written: Vec<&RawValue> = match serde_json::Deserializer::from_str(text).into_iter::<&RawValue>().next() {
        Some(Ok(raw)) => raw_at(raw, &["coverages"])
            .and_then(|coverages| serde_json::from_str(coverages.get()).ok())
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    let entries = document
        .items("coverages")
        .into_iter()
        .zip(written)
        .map(|(item, raw)| CoverageEntry { value: Value::of_json(item), raw: raw.get().as_bytes().to_vec() })
        .collect();
    Ok((version, entries))
}

/// The count a map or array header announced, `left` the bytes after it. A count larger than
/// those is refused before anything is read for it.
fn header<E: rmp::decode::RmpReadErr>(
    read: Result<u32, ValueReadError<E>>,
    left: usize,
    wanted: &str,
) -> Result<usize, String> {
    let count = read.map_err(|e| match e {
        ValueReadError::TypeMismatch(marker) => {
            format!("expected {wanted}, got the marker {marker:?}")
        }
        _ => msgpack::TRUNCATED.to_string(),
    })?;
    if count > i32::MAX as u32 {
        Err(msgpack::FORGED.into())
    } else if count as usize > left {
        Err(msgpack::TRUNCATED.into())
    } else {
        Ok(count as usize)
    }
}

/// Walks the msgpack document key by key, so each entry's exact bytes can be cut out of the part:
/// the bytes between the reader's position before and after one value.
pub fn coverage_entries_msgpack(data: &[u8]) -> Result<(i32, Vec<CoverageEntry>), String> {
    let mut entries = Vec::new();
    let mut version = 0;
    let mut reading = "envelope map header".to_string();
    let mut rest = data;
    let mut walk = || -> Result<(), String> {
        let keys = header(rmp::decode::read_map_len(&mut rest), rest.len(), "a map")?;
        for i in 0..keys {
            reading = format!("envelope key {i}");
            if !matches!(rest.first(), Some(0xa0..=0xbf | 0xd9..=0xdb)) {
                return Err("the key is not a string".into());
            }
            let MsgValue::Str(key) = msgpack::read(&mut rest)? else {
                return Err("the key is not a string".into());
            };
            match key.as_str() {
                "version" => {
                    // Read as any value: an encoder that changed this field's width or type must
                    // not make real coverage undecodable.
                    reading = "version".into();
                    version = Value::of_msgpack(msgpack::read(&mut rest)?).to_i64() as i32;
                }
                "coverages" => {
                    reading = "coverages array header".into();
                    let count = header(rmp::decode::read_array_len(&mut rest), rest.len(), "an array")?;
                    for j in 0..count {
                        reading = format!("coverages[{j}]");
                        let start = data.len() - rest.len();
                        let value = Value::of_msgpack(msgpack::read(&mut rest)?);
                        entries.push(CoverageEntry { value, raw: data[start..data.len() - rest.len()].to_vec() });
                    }
                }
                other => {
                    // A key this decoder does not know is skipped, not refused: a newer encoder
                    // adding a field must not cost the coverage.
                    reading = format!("skipping \"{other}\"");
                    msgpack::read(&mut rest)?;
                }
            }
        }
        Ok(())
    };
    match walk() {
        Ok(()) => Ok((version, entries)),
        Err(e) => Err(format!("{reading}: {e}")),
    }
}

/// Splits the coverage payload into its entries; says which format it was taken for.
fn coverage_entries(content_type: &str, data: &[u8]) -> (&'static str, Result<(i32, Vec<CoverageEntry>), String>) {
    let declared = content_type.to_lowercase();
    if declared.contains("json") || (!declared.contains("msgpack") && looks_json(data)) {
        ("json", coverage_entries_json(data))
    } else {
        ("msgpack", coverage_entries_msgpack(data))
    }
}

const COVERAGE_KNOWN_KEYS: [&str; 4] = ["test_session_id", "test_suite_id", "span_id", "files"];

/// One entry as a row. The bitmap is never decoded: by dd-trace-go's own contract the backend
/// stores it and hands it back as the bytes it got.
pub fn coverage_row(
    tenant: &str,
    received_at: DateTime<Utc>,
    headers: &AgentHeaders,
    event: &[u8],
    payload_version: i32,
    format: &str,
    entry: CoverageEntry,
) -> CiCoverage {
    let files = entry.value.field("files").items();
    CiCoverage {
        tenant_id: tenant.into(),
        received_at,
        payload_version,
        session_id: entry.value.field("test_session_id").to_u64(),
        suite_id: entry.value.field("test_suite_id").to_u64(),
        // The encoder leaves span_id out in suite-skipping mode.
        span_id: match entry.value.field("span_id") {
            Value::Null => None,
            id => Some(id.to_u64()),
        },
        files_filename: files.iter().map(|file| file.field_text("filename")).collect(),
        files_bitmap: files.iter().map(|file| Blob(file.field("bitmap").bytes())).collect(),
        extra: entry.value.unknown(&COVERAGE_KNOWN_KEYS),
        raw: Blob(entry.raw),
        raw_format: format.into(),
        event: Blob(event.to_vec()),
        evp_subdomain: headers.subdomain.clone(),
        container_id: headers.container_id.clone(),
        agent_hostname: headers.hostname.clone(),
        agent_version: headers.agent_version.clone(),
    }
}

fn part_names(parts: &[body::Part]) -> String {
    if parts.is_empty() { "none".into() } else { parts.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ") }
}

/// The upload's rows. What does not decode is kept raw, into `kept`.
async fn coverage_rows(ctx: &Ctx, body: Bytes, kept: &mut Vec<RawPayload>) -> Vec<CiCoverage> {
    let content_type = ctx.header("content-type");
    let parts = match body::try_parts(content_type, body.clone()).await {
        Ok(parts) => parts,
        Err(e) => {
            let note = format!("multipart, content-type {}: {e}", or_unknown(content_type));
            kept.extend(raw::row(ctx, "citestcov", "decode_error", &note, &body));
            return Vec::new();
        }
    };
    // The coverage part is "coveragex" to dd-trace-go and "coverage1" to dd-trace-py, so it cannot
    // be found by name: it is the first part that is not the event.
    let Some(coverage) = parts.iter().find(|p| p.name != "event") else {
        let note = format!("multipart carries no coverage part (only {})", part_names(&parts));
        kept.extend(raw::row(ctx, "citestcov", "unexpected_shape", &note, &body));
        return Vec::new();
    };
    let (format, entries) = coverage_entries(&coverage.content_type, &coverage.data);
    let (payload_version, entries) = match entries {
        Ok(found) => found,
        Err(e) => {
            let note = format!("coverage part ({}): {e}", or_unknown(&coverage.content_type));
            kept.extend(raw::row(ctx, "citestcov", "decode_error", &note, &body));
            return Vec::new();
        }
    };
    let event = parts.iter().rfind(|p| p.name == "event").map(|p| p.data.to_vec()).unwrap_or_default();
    let now = Utc::now();
    let headers = agent_headers(ctx);
    entries
        .into_iter()
        .map(|entry| coverage_row(ctx.tenant(), now, &headers, &event, payload_version, format, entry))
        .collect()
}

/// A code-coverage upload: multipart with a dummy JSON "event" part and the coverage payload,
/// msgpack or JSON:
///
///   {"version": 2, "coverages": [{"test_session_id", "test_suite_id",
///     "span_id", "files": [{"filename", "bitmap"}]}]}
pub async fn handle_test_cov(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    if body.is_empty() {
        return accepted();
    }
    let mut kept = Vec::new();
    let rows = coverage_rows(&ctx, body.0, &mut kept).await;
    if let Err(e) = raw::keep(&clickhouse, &kept).await {
        return storage_failed(e);
    }
    if let Err(e) = storage::insert(&clickhouse, &rows).await {
        return storage_failed(e);
    }
    accepted()
}

/// A tracer's question: the JSON:API envelope {"data": {"id", "type", "attributes"}} and the
/// attributes the five configuration endpoints send. They overlap and no endpoint sends all of
/// them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConfigRequest {
    pub id: String,
    pub r#type: String,
    pub service: String,
    pub env: String,
    pub repository_url: String,
    pub branch: String,
    pub sha: String,
    pub test_level: String,
    pub module: String,
    pub commit_message: String,
    /// The configurations object as it was written.
    pub configurations: String,
    pub page_state: String,
}

/// Reads a question; also says which part did not decode ("envelope" or "attributes") and why.
/// What did decode is returned either way, except that attributes are not read under a broken
/// envelope.
pub fn read_config_request(body: &[u8]) -> (ConfigRequest, Option<(&'static str, String)>) {
    let root = match json::try_parse(body) {
        Ok(root) => root,
        Err(e) => return (ConfigRequest::default(), Some(("envelope", e))),
    };
    let bad = Mismatches::default();
    let data = fields::fields(&bad, "", &root).object("data");
    let envelope = ConfigRequest { id: data.string("id"), r#type: data.string("type"), ..ConfigRequest::default() };
    if let Some(problem) = bad.borrow().first() {
        return (envelope, Some(("envelope", problem.clone())));
    }
    let attributes = data.object("attributes");
    let repaired = json::repair(body);
    let configurations = raw_root(&repaired)
        .ok()
        .and_then(|raw| raw_at(raw, &["data", "attributes", "configurations"]))
        .map(|found| found.get().to_string())
        .unwrap_or_default();
    let request = ConfigRequest {
        service: attributes.string("service"),
        env: attributes.string("env"),
        repository_url: attributes.string("repository_url"),
        branch: attributes.string("branch"),
        sha: attributes.string("sha"),
        test_level: attributes.string("test_level"),
        module: attributes.string("module"),
        commit_message: attributes.string("commit_message"),
        configurations,
        page_state: attributes.object("page_info").string("page_state"),
        ..envelope
    };
    let problem = bad.borrow().first().map(|problem| ("attributes", problem.clone()));
    (request, problem)
}

/// Answers a configuration question and keeps both the question and the answer. A body that does
/// not parse still gets a row and a well-formed answer: "a tracer sends something we cannot read"
/// is what the table is for, and a 4xx here is terminal for the client. The rows are best-effort
/// for the same reason: the answer does not depend on storage, so a ClickHouse outage does not
/// cost the tracer its configuration.
pub async fn answer_config(
    endpoint: &'static str,
    answer: fn(&ConfigRequest) -> String,
    ctx: Ctx,
    State(clickhouse): State<clickhouse::Client>,
    body: Body,
) -> Response {
    let (request, problem) = read_config_request(&body);
    if let Some((part, error)) = problem {
        // raw::keep has logged a failure.
        let note = format!("{endpoint} {part}: {error}");
        _ = raw::store(&clickhouse, &ctx, "cisettings", "decode_error", &note, &body).await;
    }
    let response = answer(&request);
    let row = CiSettingsRequest {
        tenant_id: ctx.tenant().into(),
        received_at: Utc::now(),
        endpoint: endpoint.into(),
        request_id: request.id,
        request_type: request.r#type,
        service: request.service,
        env: request.env,
        repository_url: request.repository_url,
        branch: request.branch,
        sha: request.sha,
        test_level: request.test_level,
        module: request.module,
        commit_message: request.commit_message,
        page_state: request.page_state,
        configurations: request.configurations,
        request_body: Blob(body.to_vec()),
        response_body: response.clone(),
    };
    if let Err(e) = storage::insert(&clickhouse, &[row]).await {
        tracing::warn!("{e}");
    }
    // The answer was built as JSON text, so that the row keeps exactly what was sent.
    let mut reply = http::Response::new(TakoBody::from(response));
    reply.headers_mut().insert(http::header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    reply
}

/// Which Test Optimization features are on: none, on purpose. Each flag gates one of the other
/// configuration endpoints in the tracer, so with all of them false a tracer still posts its
/// results but never asks what to skip. Turning one on before the feature exists makes a
/// customer's suite skip tests on data we do not have. `require_git` false also stops a tracer
/// unshallowing its clone to upload packfiles.
///
/// The keys are dd-trace-go's settingsResponse (settings_api.go).
pub fn settings_answer(request: &ConfigRequest) -> String {
    format!(
        r#"{{"data":{{"id":{},"type":"ci_app_test_service_libraries_settings","attributes":{{"code_coverage":false,"coverage_report_upload_enabled":false,"early_flake_detection":{{"enabled":false,"slow_test_retries":{{"5s":0,"10s":0,"30s":0,"5m":0}},"faulty_session_threshold":null}},"flaky_test_retries_enabled":false,"itr_enabled":false,"require_git":false,"tests_skipping":false,"known_tests_enabled":false,"impacted_tests_enabled":false,"test_management":{{"enabled":false,"attempt_to_fix_retries":0}}}}}}}}"#,
        json::quoted(&request.id)
    )
}

/// Which tests may be skipped: none. `meta` is sent although empty, because dd-trace-py treats a
/// missing meta as a malformed response. When this is implemented, meta.coverage holds base64 of
/// the bitmaps of a citestcov upload, unchanged: they are in ci_coverage.files_bitmap.
///
/// The keys are dd-trace-go's skippableResponse (skippable.go).
pub fn skippable_answer(_: &ConfigRequest) -> String {
    r#"{"meta":{"correlation_id":"","coverage":{}},"data":[]}"#.into()
}

/// Which tests has this service run before: none.
///
/// `has_next` MUST be false: the client keeps POSTing for as long as it is true, and dd-trace-go
/// has no iteration cap, so a true that never moves the cursor spins a customer's test process
/// forever.
///
/// The flaky-tests path gets the same answer. No current client names that path (older
/// dd-trace-py did); an empty list is harmless whatever its real shape is, a 404 in the middle of
/// a test run is not.
///
/// The keys are dd-trace-go's knownTestsResponse (known_tests_api.go).
pub fn test_list_answer(request: &ConfigRequest) -> String {
    format!(
        r#"{{"data":{{"id":{},"type":"ci_app_libraries_tests","attributes":{{"tests":{{}},"page_info":{{"cursor":"","size":0,"has_next":false}}}}}}}}"#,
        json::quoted(&request.id)
    )
}

/// Which tests are quarantined, disabled or being fixed: none. The keys are dd-trace-go's
/// testManagementTestsResponse.
pub fn test_management_answer(request: &ConfigRequest) -> String {
    format!(
        r#"{{"data":{{"id":{},"type":"ci_app_libraries_tests","attributes":{{"modules":{{}}}}}}}}"#,
        json::quoted(&request.id)
    )
}

/// The commit list of a search_commits request: the repository and the shas the client asked
/// about.
fn read_search_commits(body: &[u8]) -> Result<(String, Vec<String>), String> {
    let root = json::try_parse(body)?;
    let bad = Mismatches::default();
    let document = fields::fields(&bad, "", &root);
    let shas = document
        .objects("data")
        .iter()
        .map(|commit| {
            // Read only so that a type that is not text is noted.
            commit.string("type");
            commit.string("id")
        })
        .collect();
    let repository = document.object("meta").string("repository_url");
    match bad.into_inner().into_iter().next() {
        Some(problem) => Err(problem),
        None => Ok((repository, shas)),
    }
}

fn commits_answer(repository: &str) -> Response {
    json(StatusCode::OK, &serde_json::json!({ "data": [], "meta": { "repository_url": repository } }))
}

/// Records the shas a client asked about and answers that we hold none.
///
/// "Which of these do you have" is a read, and the intake has no read path. Answering none makes
/// the client upload its packs, which is what we want while git_packfiles is the only place those
/// objects can come from.
///
/// The answer is JSON even for a request that does not decode: the client retries a 200 of any
/// other content type, and a 4xx is terminal. The rows are best-effort: the answer does not
/// depend on them, so a ClickHouse outage does not stop the upload.
pub async fn handle_search_commits(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    match read_search_commits(&body) {
        Err(e) => {
            // raw::keep has logged a failure.
            _ = raw::store(&clickhouse, &ctx, "gitmeta", "decode_error", &format!("search_commits: {e}"), &body).await;
            commits_answer("")
        }
        Ok((repository, shas)) => {
            let now = Utc::now();
            let rows: Vec<_> = shas
                .into_iter()
                .filter(|sha| !sha.is_empty())
                .map(|sha| GitCommit {
                    tenant_id: ctx.tenant().into(),
                    repository_url: repository.clone(),
                    sha,
                    seen_at: now,
                    // The client is ASKING about this sha, which says nothing about whether we
                    // hold its objects.
                    packfile_id: None,
                    source: "search_commits".into(),
                })
                .collect();
            if let Err(e) = storage::insert(&clickhouse, &rows).await {
                tracing::warn!("{e}");
            }
            commits_answer(&repository)
        }
    }
}

/// The JSON part of a packfile upload: {"data": {"id", "type"}, "meta": {"repository_url"}}. The
/// sha and the repository are None when the part does not set them.
struct PushedSha {
    sha: Option<String>,
    repository: Option<String>,
    /// False when any of the part did not decode.
    decoded: bool,
}

fn read_pushed_sha(data: &[u8]) -> PushedSha {
    let Ok(root) = json::try_parse(data) else {
        return PushedSha { sha: None, repository: None, decoded: false };
    };
    let bad = Mismatches::default();
    let document = fields::fields(&bad, "", &root);
    let commit = document.object("data");
    commit.string("type");
    let sha = commit.optional_string("id");
    let repository = document.object("meta").optional_string("repository_url");
    PushedSha { sha, repository, decoded: bad.borrow().is_empty() }
}

/// One git packfile: multipart with a "pushedSha" JSON part and a "packfile" part, one request
/// per .pack file.
///
/// The answer is 204 with no body: dd-trace-go accepts any 2xx, dd-trace-py checks for exactly
/// 204, so 204 is the one status both call success.
pub async fn handle_packfile(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    let content_type = ctx.header("content-type");
    let parts = match body::try_parts(content_type, body.clone()).await {
        Ok(parts) => parts,
        Err(e) => {
            let note = format!("packfile multipart, content-type {}: {e}", or_unknown(content_type));
            if let Err(e) = raw::store(&clickhouse, &ctx, "gitmeta", "decode_error", &note, &body).await {
                return storage_failed(e);
            }
            return status(StatusCode::NO_CONTENT);
        }
    };
    let mut sha = String::new();
    let mut repository = String::new();
    let mut pack: Option<Bytes> = None;
    let mut filename = String::new();
    let mut other: BTreeMap<String, Blob> = BTreeMap::new();
    for part in parts {
        match part.name.as_str() {
            "pushedSha" => {
                // A repeated part changes only what it sets.
                let pushed = read_pushed_sha(&part.data);
                sha = pushed.sha.unwrap_or(sha);
                repository = pushed.repository.unwrap_or(repository);
                if !pushed.decoded {
                    tracing::warn!("[gitmeta] packfile pushedSha did not decode");
                    other.insert("pushedSha.undecodable".into(), Blob(part.data.to_vec()));
                }
            }
            "packfile" => {
                pack = Some(part.data);
                filename = part.file_name;
            }
            name => {
                other.insert(name.into(), Blob(part.data.to_vec()));
            }
        }
    }
    let Some(pack) = pack else {
        if let Err(e) = raw::store(
            &clickhouse,
            &ctx,
            "gitmeta",
            "unexpected_shape",
            "packfile upload without a packfile part",
            &body,
        )
        .await
        {
            return storage_failed(e);
        }
        return status(StatusCode::NO_CONTENT);
    };
    let packfile_id: String = Sha256::digest(&pack).iter().map(|b| format!("{b:02x}")).collect();
    let now = Utc::now();
    let row = GitPackfile {
        tenant_id: ctx.tenant().into(),
        received_at: now,
        repository_url: repository.clone(),
        pushed_sha: sha.clone(),
        packfile_id: packfile_id.clone(),
        filename,
        size_bytes: pack.len() as u64,
        packfile: Blob(pack.to_vec()),
        other_parts: other,
    };
    if let Err(e) = storage::insert(&clickhouse, &[row]).await {
        return storage_failed(e);
    }
    // The pushed sha also becomes a git_commits row, this time WITH a packfile id: the one path
    // where "we hold the objects" is true. ReplacingMergeTree lets it replace the NULL a
    // search_commits sighting left behind.
    if !sha.is_empty() {
        let row = GitCommit {
            tenant_id: ctx.tenant().into(),
            repository_url: repository,
            sha,
            seen_at: now,
            packfile_id: Some(packfile_id),
            source: "packfile".into(),
        };
        if let Err(e) = storage::insert(&clickhouse, &[row]).await {
            return storage_failed(e);
        }
    }
    status(StatusCode::NO_CONTENT)
}

const PIPELINE_KNOWN_KEYS: [&str; 9] =
    ["ci_env", "ci_level", "provider", "tags", "metrics", "measures", "name", "start_time", "end_time"];

/// The call's row. What does not decode is kept raw, into `kept`.
fn pipeline_event_row(kind: &str, ctx: &Ctx, body: &[u8], kept: &mut Vec<RawPayload>) -> Option<CiPipelineEvent> {
    let root = match json::try_parse(body) {
        Ok(root) => root,
        Err(e) => {
            kept.extend(raw::row(ctx, "cipipeline", "decode_error", &format!("{kind} envelope: {e}"), body));
            return None;
        }
    };
    let (data_type, attributes, problem) = {
        let problems = Mismatches::default();
        let data = fields::fields(&problems, "", &root).object("data");
        let data_type = data.string("type");
        let attributes = data.find("attributes").map_or(Value::Null, Value::of_json);
        let problem = problems.borrow().first().cloned();
        (data_type, attributes, problem)
    };
    if let Some(problem) = problem {
        kept.extend(raw::row(ctx, "cipipeline", "decode_error", &format!("{kind} envelope: {problem}"), body));
        return None;
    }
    let repaired = json::repair(body);
    let written = raw_root(&repaired).ok().and_then(|raw| raw_at(raw, &["data", "attributes"]));
    // `measure` calls the map "metrics"; a custom span may call the same idea "measures".
    let metrics = attributes.field("metrics").float_map();
    Some(CiPipelineEvent {
        tenant_id: ctx.tenant().into(),
        received_at: Utc::now(),
        kind: kind.into(),
        data_type,
        provider: attributes.field_text("provider"),
        ci_level: attributes.field("ci_level").try_i64(),
        ci_env: attributes.field("ci_env").string_map(),
        tags: attributes.field("tags").string_map(),
        metrics: if metrics.is_empty() { attributes.field("measures").float_map() } else { metrics },
        span_name: attributes.field_text("name"),
        span_start_raw: attributes.field_text("start_time"),
        span_end_raw: attributes.field_text("end_time"),
        attributes: written.map(|found| found.get().to_string()).unwrap_or_default(),
        body: Blob(body.to_vec()),
        extra: attributes.unknown(&PIPELINE_KNOWN_KEYS),
    })
}

/// One datadog-ci call: `tag`, `measure` or `trace`. All three share the envelope {"data":
/// {"type", "attributes"}}; `kind` says which route it arrived on. The custom-span attributes were
/// not read from the CLI's source: the span columns are filled from the obvious key names.
///
/// 202 whatever the body was: the CLI retries five times on an error, so a 5xx would stall
/// somebody's CI job for half a minute. Only a refused insert is answered 503.
pub async fn handle_pipeline_event(
    kind: &'static str,
    ctx: Ctx,
    State(clickhouse): State<clickhouse::Client>,
    body: Body,
) -> Response {
    if body.is_empty() {
        return accepted();
    }
    let mut kept = Vec::new();
    let row = pipeline_event_row(kind, &ctx, &body, &mut kept);
    if let Err(e) = raw::keep(&clickhouse, &kept).await {
        return storage_failed(e);
    }
    if let Err(e) = storage::insert(&clickhouse, &Vec::from_iter(row)).await {
        return storage_failed(e);
    }
    accepted()
}
