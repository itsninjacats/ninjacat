//! http-intake.logs.<site> — the logs intake.
//!
//!   agent config: logs_config.logs_dd_url / DD_LOGS_CONFIG_LOGS_DD_URL
//!
//! One `logs` row per item. An item that is JSON but not shaped like a log goes to raw_payloads as
//! "unexpected_shape", a body that is not JSON as "decode_error".

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use ninjapaws_storage as storage;
use ninjapaws_storage::column;
use ninjapaws_storage::rows::logs::Log;
use serde_json::Value;
use serde_json::value::RawValue;
use tako::extractors::state::State;
use tako::types::Response;

use crate::fields::{self, Mismatches};
use crate::json;
use crate::raw;
use crate::server::{Body, Ctx, accepted, storage_failed};
use crate::wire::{tags, text, time};

/// The fields Datadog's HTTPLogItem declares. Everything else in an item is an attribute.
const DECLARED_FIELDS: [&str; 5] = ["ddsource", "ddtags", "hostname", "message", "service"];

#[derive(Clone, Debug, Default)]
pub struct LogItem {
    pub source: String,
    pub tags: String,
    pub hostname: String,
    pub message: String,
    pub service: String,
    /// The item's other properties, by name.
    pub attributes: BTreeMap<String, Value>,
}

#[derive(Debug, PartialEq)]
pub enum ItemProblem {
    /// JSON, but not a log: a declared field is not a string. Says which.
    NotALog(String),
    Undecodable(String),
}

/// Splits a body into items, each as its JSON text. Three framings arrive here: a JSON array (the
/// agent), one bare object (small clients) and one object per line (the browser SDK).
pub fn parse_items(body: &[u8]) -> Result<Vec<Box<RawValue>>, String> {
    let first_significant = body.iter().find(|b| !matches!(b, b' ' | b'\t' | b'\r' | b'\n'));
    let repaired = json::repair(body);
    if first_significant == Some(&b'[') {
        serde_json::from_slice(&repaired).map_err(|e| format!("JSON logs array: {e}"))
    } else {
        let mut items = Vec::new();
        for item in serde_json::Deserializer::from_slice(&repaired).into_iter::<Box<RawValue>>() {
            match item {
                Ok(item) => items.push(item),
                Err(e) => return Err(format!("JSON logs item {}: {e}", items.len())),
            }
        }
        Ok(items)
    }
}

/// Reads one item: the five declared fields must be strings (or absent). Datadog's published model
/// calls `message` required, but its intake takes an item without one, and its own .NET tracer
/// sends such items.
pub fn decode_item(raw: &Value) -> Result<LogItem, ItemProblem> {
    if !raw.is_object() {
        return Err(ItemProblem::Undecodable("a log item must be a JSON object".into()));
    }
    let bad = Mismatches::default();
    let fields = fields::fields(&bad, "", raw);
    let item = LogItem {
        source: fields.string("ddsource"),
        tags: fields.string("ddtags"),
        hostname: fields.string("hostname"),
        message: fields.string("message"),
        service: fields.string("service"),
        attributes: fields
            .members()
            .into_iter()
            .filter(|(name, _)| !DECLARED_FIELDS.contains(&name.as_str()))
            .collect(),
    };
    match bad.into_inner().into_iter().next() {
        None => Ok(item),
        Some(problem) => Err(ItemProblem::NotALog(problem)),
    }
}

/// The row's timestamp, where it came from, and the attribute it used.
///
/// Five wire forms exist for the same idea. The order is Datadog's own: `timestamp` before `date`,
/// a millisecond number before an RFC 3339 string; `@t` is the compact form (see `to_row`). A value
/// that does not parse falls through to the next candidate, and is then left among the attributes
/// rather than lost.
#[derive(Debug, PartialEq)]
pub struct LogTime {
    pub time: DateTime<Utc>,
    /// "timestamp_ms", "timestamp_string", "date_ms", "date_string", "compact_string" or
    /// "arrival".
    pub source: &'static str,
    /// The attribute the time was read from; "" when it is the arrival time.
    pub key: &'static str,
}

pub fn timestamp(attributes: &BTreeMap<String, Value>, arrival: DateTime<Utc>) -> LogTime {
    let millis =
        |name: &str| attributes.get(name).and_then(json::int64_of).filter(|ms| *ms > 0).map(time::from_unix_millis);
    let text = |name: &str| attributes.get(name).and_then(Value::as_str).and_then(time::try_rfc3339);
    let candidates = [
        (millis("timestamp"), "timestamp_ms", "timestamp"),
        (text("timestamp"), "timestamp_string", "timestamp"),
        (millis("date"), "date_ms", "date"),
        (text("date"), "date_string", "date"),
        (text("@t"), "compact_string", "@t"),
    ];
    match candidates.into_iter().find_map(|(time, source, key)| time.map(|time| LogTime { time, source, key })) {
        Some(found) => found,
        // HTTPLogItem has no timestamp field at all, so official clients cannot always send one.
        // The row says so instead of passing arrival off as the sender's clock.
        None => LogTime { time: arrival, source: "arrival", key: "" },
    }
}

/// A .NET log level (Microsoft's names and Serilog's) under the status names the agent and the
/// browser SDK use.
fn compact_status(level: &str) -> String {
    let lower = level.to_lowercase();
    match lower.as_str() {
        "trace" | "verbose" | "debug" => "debug".into(),
        "information" => "info".into(),
        "warning" => "warn".into(),
        "error" => "error".into(),
        "critical" | "fatal" => "critical".into(),
        _ => lower,
    }
}

/// What the query string says about the whole batch.
#[derive(Clone, Debug, Default)]
pub struct BatchDefaults {
    pub source: String,
    pub service: String,
    pub host: String,
    pub tags: Vec<String>,
}

pub fn to_row(tenant: &str, item: LogItem, arrival: DateTime<Utc>, defaults: &BatchDefaults) -> Log {
    let attribute = |name: &str| item.attributes.get(name).and_then(Value::as_str).map(String::from);

    // `host` is a spelling of `hostname` Datadog also accepts; it is not a declared field, so it
    // arrives as an attribute.
    let host_attribute = if item.hostname.is_empty() { attribute("host").filter(|h| !h.is_empty()) } else { None };

    let time = timestamp(&item.attributes, arrival);

    // Datadog's .NET tracer sends logs in Serilog's compact form: @m for the message, @t for the
    // time, @l for the level (left out when it is Information), and dd_service where its log
    // injection already put one. Each stands in only where the usual field is missing.
    let status = attribute("status");
    let compact_message = if item.message.is_empty() { attribute("@m") } else { None };
    let compact_level = if status.is_none() { attribute("@l") } else { None };
    let compact_service = if item.service.is_empty() { attribute("dd_service") } else { None };

    // Only attributes that were actually USED leave the JSON column.
    let rest: BTreeMap<String, Value> = item
        .attributes
        .iter()
        .filter(|(name, _)| {
            let name = name.as_str();
            !(name == time.key
                || (name == "status" && status.is_some())
                || (name == "host" && host_attribute.is_some())
                || (name == "@m" && compact_message.is_some())
                || (name == "@l" && compact_level.is_some())
                || (name == "dd_service" && compact_service.is_some()))
        })
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();

    let status = match (status, &compact_level) {
        (Some(status), _) => status,
        (None, Some(level)) => compact_status(level),
        (None, None) if compact_message.is_some() => "info".into(),
        (None, None) => String::new(),
    };
    // Batch tags first: they are the weaker statement.
    let mut all_tags = defaults.tags.clone();
    all_tags.extend(tags::split_ddtags(&item.tags));

    Log {
        tenant_id: tenant.into(),
        timestamp: time.time,
        host: text::first_non_empty(&[&item.hostname, host_attribute.as_deref().unwrap_or(""), &defaults.host]).into(),
        service: text::first_non_empty(&[&item.service, compact_service.as_deref().unwrap_or(""), &defaults.service])
            .into(),
        source: text::first_non_empty(&[&item.source, &defaults.source]).into(),
        status,
        message: compact_message.unwrap_or(item.message),
        tags: tags::to_multi_map(all_tags),
        attributes: column::json_object(&if rest.is_empty() { String::new() } else { json::compact_object(&rest) }),
        timestamp_source: time.source.into(),
    }
}

pub async fn handle(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    if ctx.is_sweep() || body.is_empty() {
        return accepted();
    }
    let items = match parse_items(&body) {
        Ok(items) => items,
        Err(e) => {
            raw::store(&clickhouse, &ctx, "logs", "decode_error", &e, &body).await;
            return accepted();
        }
    };
    // Four fields may also arrive as query parameters, so a sender that cannot shape its body can
    // still say what it is. The body wins, except for tags, which merge.
    let defaults = BatchDefaults {
        source: ctx.query("ddsource"),
        service: ctx.query("service"),
        host: text::first_non_empty(&[&ctx.query("hostname"), &ctx.query("host")]).into(),
        tags: tags::split_ddtags(&ctx.query("ddtags")),
    };
    let arrival = Utc::now();
    let mut rows = Vec::new();
    for (i, raw) in items.iter().enumerate() {
        let raw_bytes = raw.get().as_bytes();
        let decoded = serde_json::from_str::<Value>(raw.get())
            .map_err(|e| ItemProblem::Undecodable(e.to_string()))
            .and_then(|item| decode_item(&item));
        match decoded {
            Err(ItemProblem::NotALog(problem)) => {
                raw::store(&clickhouse, &ctx, "logs", "unexpected_shape", &format!("item #{i}: {problem}"), raw_bytes)
                    .await
            }
            Err(ItemProblem::Undecodable(error)) => {
                raw::store(&clickhouse, &ctx, "logs", "decode_error", &format!("item #{i}: {error}"), raw_bytes).await
            }
            // `{}` says nothing: a probe, not a log.
            Ok(item)
                if item.message.is_empty()
                    && item.attributes.is_empty()
                    && item.source.is_empty()
                    && item.service.is_empty() => {}
            Ok(item) => rows.push(to_row(ctx.tenant(), item, arrival, &defaults)),
        }
    }
    if let Err(e) = storage::insert(&clickhouse, &rows).await {
        return storage_failed(e);
    }
    accepted()
}
