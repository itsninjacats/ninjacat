//! Two small intakes with one endpoint each.
//!
//!   resources-intake.<site>                   /api/v2/genresources   opaque bytes
//!   instrumentation-telemetry-intake.<site>   /api/v2/apmtelemetry   JSON envelope
//!
//!   config: none for resources (an event platform track); telemetry is switched off wholesale
//!           with DD_TELEMETRY_ENABLED.

use std::collections::BTreeMap;

use chrono::{DateTime, TimeDelta, Utc};
use ninjapaws_storage as storage;
use ninjapaws_storage::rows::Extra;
use ninjapaws_storage::rows::apm_telemetry::ApmTelemetry;
use serde_json::Value;
use tako::extractors::state::State;
use tako::types::Response;

use crate::json;
use crate::raw;
use crate::server::{Body, Ctx, accepted, storage_failed};
use crate::wire::time;

/// One opaque integration payload per request. The agent never looks inside: an integration hands
/// it bytes, and no schema for them is published anywhere. So the bytes are kept as they are, with
/// the sender's identity, which the event platform carries in headers, in the note.
pub async fn handle_gen_resources(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    raw::store(&clickhouse, &ctx, "genresources", "no_schema", &raw::origin_note(&ctx), &body).await;
    accepted()
}

// instrumentation-telemetry-intake: one path, five producers, one envelope with `request_type` as
// the discriminator.
//
//   tracers, through the trace-agent's proxy: api_version, tracer_time, runtime_id, seq_id,
//     application, host, payload, debug; the proxy adds the Via, DD-Agent-* and container
//     headers.
//   fleet installer: the same plus origin; request_type "logs" or "traces".
//   agent telemetry: event_time instead of tracer_time; request_type agent-metrics, agent-logs,
//     message-batch, or a profile event.
//   trace-agent onboarding, cluster-agent remote config: request_type apm-onboarding-event /
//     apm-remote-config-event and a payload.
//
// None of their types is published, so the envelope is read generically: `application` and
// `host` give their known fields as columns, and `payload` stays JSON text whoever sent it.

/// `seq_id`, read from its digits: a float would round a large sequence number past 2^53.
pub fn seq_id(value: Option<&Value>) -> Option<i64> {
    value.and_then(json::number_text)?.parse().ok()
}

/// A telemetry timestamp: Unix seconds, with an optional fraction, sent as a number or (by some
/// producers) as a string. None when absent or not a number, never the epoch.
pub fn unix_time(value: Option<&Value>) -> Option<DateTime<Utc>> {
    let text = json::compact(value?);
    let digits = text.trim().trim_matches('"');
    // A sign and a bare point are taken, as producers write them; infinity and NaN are not times.
    let seconds: f64 = digits.parse().ok().filter(|s: &f64| s.is_finite())?;
    let whole = seconds.trunc();
    if whole <= -62135596800.0 {
        Some(time::from_unix_seconds(i64::MIN))
    } else if whole >= 253402300799.0 {
        Some(time::from_unix_seconds(i64::MAX))
    } else {
        let nanos = ((seconds - whole) * 1e9) as i64;
        Some(time::from_unix_seconds(whole as i64) + TimeDelta::nanoseconds(nanos))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Application {
    pub service_name: String,
    pub service_version: String,
    pub env: String,
    pub language_name: String,
    pub language_version: String,
    pub tracer_version: String,
}

/// A block's members; None when it is there but is not an object (null is no block).
fn block(value: Option<&Value>) -> Option<BTreeMap<String, Value>> {
    match value {
        Some(v) if !v.is_object() && !v.is_null() => None,
        _ => Some(json::lenient_members(value)),
    }
}

/// The `application` block's documented fields; all "" when it is absent. None when it is there
/// but is not an object: then its text must be kept somewhere else, not lost.
pub fn application(value: Option<&Value>) -> Option<Application> {
    let fields = block(value)?;
    let text = |name: &str| json::text(fields.get(name));
    Some(Application {
        service_name: text("service_name"),
        service_version: text("service_version"),
        env: text("env"),
        language_name: text("language_name"),
        language_version: text("language_version"),
        tracer_version: text("tracer_version"),
    })
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Host {
    pub hostname: String,
    pub os: String,
    pub architecture: String,
    /// The block's other keys, as JSON text by name: gohai and the tracer both put more there than
    /// these three.
    pub extra: BTreeMap<String, String>,
}

/// The `host` block; None under the same rule as `application`. `text` is the block as it was
/// sent, which its other keys are kept from.
pub fn host(value: Option<&Value>, text: Option<&str>) -> Option<Host> {
    let fields = block(value)?;
    Some(Host {
        hostname: json::text(fields.get("hostname")),
        os: json::text(fields.get("os")),
        architecture: json::text(fields.get("architecture")),
        extra: text.map(|t| json::raw::other_members(&["hostname", "os", "architecture"], t)).unwrap_or_default(),
    })
}

/// The envelope keys read by name. Any other key goes to `extra`.
const ENVELOPE_KEYS: [&str; 11] = [
    "request_type",
    "api_version",
    "runtime_id",
    "seq_id",
    "tracer_time",
    "event_time",
    "application",
    "host",
    "payload",
    "debug",
    "origin",
];

/// The envelope's keys without a column of their own, each as the text it was sent as.
pub fn extra_keys(text: &str) -> Extra {
    json::raw::other_members(&ENVELOPE_KEYS, text)
}

/// Names the producer from request_type, falling back to the envelope keys that only one producer
/// sets.
pub fn producer(envelope: &BTreeMap<String, Value>, request_type: &str) -> &'static str {
    match request_type {
        "apm-onboarding-event" => "trace-agent",
        "apm-remote-config-event" => "cluster-agent",
        "agent-metrics" | "agent-logs" => "agent-telemetry",
        "app-started"
        | "app-heartbeat"
        | "app-closing"
        | "app-dependencies-loaded"
        | "app-integrations-change"
        | "app-client-configuration-change"
        | "app-product-change"
        | "app-extended-heartbeat"
        | "generate-metrics"
        | "distributions" => "tracer",
        // "logs", "traces" and "message-batch" are used by more than one producer; the envelope
        // decides.
        _ if envelope.contains_key("event_time") => "agent-telemetry", // profile events, e.g. agent-bsod
        _ if envelope.contains_key("origin") => "fleet-installer",
        _ if envelope.contains_key("runtime_id") => "tracer",
        _ => "unknown",
    }
}

/// The row of the request itself, whoever sent it. `text` is the envelope as it was sent: what
/// is stored as JSON is stored as that text.
fn request_row(ctx: &Ctx, envelope: &BTreeMap<String, Value>, text: &str, request_type: &str) -> ApmTelemetry {
    let member = |name: &str| envelope.get(name);
    let sent = json::raw::members(text);
    let as_sent = |name: &str| sent.get(name).map(|t| t.to_string()).unwrap_or_default();
    let app = application(member("application"));
    let sender = host(member("host"), sent.get("host").copied());

    // `application` and `host` are known keys, so they are not in `extra`. A block that did not
    // read as an object has no column to go to either, so its text is put back under its own
    // name.
    let mut extra = extra_keys(text);
    if app.is_none() {
        extra.insert("application".into(), as_sent("application"));
    }
    if sender.is_none() {
        extra.insert("host".into(), as_sent("host"));
    }
    let app = app.unwrap_or_default();
    let sender = sender.unwrap_or_default();

    ApmTelemetry {
        tenant_id: ctx.tenant().into(),
        received_at: Utc::now(),
        request_type: request_type.into(),
        producer: producer(envelope, request_type).into(),
        api_version: json::text(member("api_version")),
        runtime_id: json::text(member("runtime_id")),
        seq_id: seq_id(member("seq_id")),
        tracer_time: unix_time(member("tracer_time")),
        tracer_time_raw: as_sent("tracer_time"),
        event_time: unix_time(member("event_time")),
        event_time_raw: as_sent("event_time"),
        service_name: app.service_name,
        service_version: app.service_version,
        env: app.env,
        language_name: app.language_name,
        language_version: app.language_version,
        tracer_version: app.tracer_version,
        hostname: sender.hostname,
        host_os: sender.os,
        host_architecture: sender.architecture,
        host_extra: sender.extra,
        payload: as_sent("payload"),
        debug: as_sent("debug"),
        origin: json::text(member("origin")),
        via: ctx.header("Via").into(),
        dd_agent_hostname: ctx.header("DD-Agent-Hostname").into(),
        dd_agent_env: ctx.header("DD-Agent-Env").into(),
        datadog_container_id: ctx.header("Datadog-Container-Id").into(),
        x_datadog_container_tags: ctx.header("X-Datadog-Container-Tags").into(),
        extra,
        batch_index: None,
        parent_request_type: String::new(),
    }
}

/// The entries of a message-batch payload, each a {request_type, payload} object; None when the
/// payload is not a list of objects.
fn batch_entries(payload: Option<&Value>) -> Option<Vec<BTreeMap<String, Value>>> {
    match payload {
        Some(Value::Null) => Some(Vec::new()),
        Some(Value::Array(entries)) if entries.iter().all(|e| e.is_object() || e.is_null()) => {
            Some(entries.iter().map(json::members).collect())
        }
        _ => None,
    }
}

/// One row for the request, whoever sent it. A message-batch, whose payload is
/// [{request_type, payload}, …], adds one row per entry on top of that row, which keeps the whole
/// batch.
pub async fn handle_telemetry(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    match json::try_parse(&body) {
        Err(e) => raw::store(&clickhouse, &ctx, "apmtelemetry", "decode_error", &e, &body).await,
        Ok(root) if !root.is_object() && !root.is_null() => {
            let note = format!("expected an object, got {}", json::kind(&root));
            raw::store(&clickhouse, &ctx, "apmtelemetry", "decode_error", &note, &body).await
        }
        Ok(root) => {
            let envelope = json::members(&root);
            let text = json::raw::text(&body);
            let request_type = json::text(envelope.get("request_type"));
            let parent = request_row(&ctx, &envelope, &text, &request_type);
            let mut found = Vec::new();
            if request_type == "message-batch" {
                match batch_entries(envelope.get("payload")) {
                    None => tracing::warn!(
                        "[apmtelemetry] batch payload is not a list of objects; kept only on the request's row"
                    ),
                    // Entries carry no envelope of their own: they inherit the request's identity.
                    Some(entries) => {
                        let sent = json::raw::member("payload", &text).map(json::raw::elements).unwrap_or_default();
                        for (i, entry) in entries.iter().enumerate() {
                            let entry_type = json::text(entry.get("request_type"));
                            let payload = sent.get(i).and_then(|e| json::raw::member("payload", e));
                            found.push(ApmTelemetry {
                                producer: producer(&envelope, &entry_type).into(),
                                request_type: entry_type,
                                payload: payload.unwrap_or_default().into(),
                                batch_index: Some(i as u32),
                                parent_request_type: request_type.clone(),
                                ..parent.clone()
                            });
                        }
                    }
                }
            }
            found.insert(0, parent);
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
    use crate::wire::time::from_unix_seconds;

    fn value(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn seq_id_keeps_every_digit_past_2_to_the_53() {
        assert_eq!(seq_id(Some(&value("9007199254740993"))), Some(9007199254740993));
        // Absent, not zero.
        assert_eq!(seq_id(None), None);
        assert_eq!(seq_id(Some(&value("\"not-a-number\""))), None);
    }

    #[test]
    fn seq_id_is_an_integer_or_nothing() {
        assert_eq!(seq_id(Some(&value("1.0"))), None);
        assert_eq!(seq_id(Some(&value("true"))), None);
        assert_eq!(seq_id(Some(&value("null"))), None);
        assert_eq!(seq_id(Some(&value("9223372036854775808"))), None);
        // A number in a string is taken.
        assert_eq!(seq_id(Some(&value("\"123\""))), Some(123));
    }

    #[test]
    fn a_telemetry_time_is_unix_seconds_with_a_fraction_and_nothing_when_it_does_not_parse() {
        let half = TimeDelta::milliseconds(500);
        assert_eq!(unix_time(Some(&value("1732000000.5"))), Some(from_unix_seconds(1732000000) + half));
        assert_eq!(unix_time(Some(&value("\"1732000000\""))), Some(from_unix_seconds(1732000000)));
        // Absent or garbage, not the epoch.
        assert_eq!(unix_time(None), None);
        assert_eq!(unix_time(Some(&value("\"not a time\""))), None);
        assert_eq!(unix_time(Some(&value("null"))), None);
    }

    #[test]
    fn a_telemetry_time_is_a_float_in_any_spelling_with_no_surrounding_space() {
        assert_eq!(unix_time(Some(&value("\"+5\""))), Some(from_unix_seconds(5)));
        assert_eq!(unix_time(Some(&value("\"5.\""))), Some(from_unix_seconds(5)));
        assert_eq!(unix_time(Some(&value("1e3"))), Some(from_unix_seconds(1000)));
        assert_eq!(unix_time(Some(&value("-0.5"))), Some(from_unix_seconds(0) - TimeDelta::milliseconds(500)));
        assert_eq!(unix_time(Some(&value("\" 5\""))), None);
        assert_eq!(unix_time(Some(&value("[5]"))), None);
    }

    #[test]
    fn a_time_no_column_can_hold_becomes_the_nearest_one() {
        let latest = chrono::TimeZone::with_ymd_and_hms(&Utc, 9999, 12, 31, 23, 59, 59).unwrap();
        assert_eq!(unix_time(Some(&value("1e300"))), Some(latest));
    }

    #[test]
    fn an_absent_application_or_host_block_is_empty_not_malformed() {
        assert_eq!(application(None), Some(Application::default()));
        assert_eq!(host(None, None), Some(Host::default()));
    }

    #[test]
    fn an_application_or_host_block_that_is_not_an_object_is_malformed() {
        assert_eq!(application(Some(&value("\"not-an-object\""))), None);
        assert_eq!(host(Some(&value("42")), Some("42")), None);
    }

    #[test]
    fn the_host_blocks_undeclared_keys_are_kept_by_name() {
        let text = r#"{"hostname": "h1", "os": "linux", "architecture": "amd64", "kernel_version": "5.15.0", "load": [ 1.50, 2 ]}"#;
        let found = host(Some(&value(text)), Some(text)).unwrap();
        assert_eq!((found.hostname.as_str(), found.os.as_str(), found.architecture.as_str()), ("h1", "linux", "amd64"));
        // As sent, spaces and digits included.
        assert_eq!(
            found.extra,
            BTreeMap::from([("kernel_version".into(), "\"5.15.0\"".into()), ("load".into(), "[ 1.50, 2 ]".into())])
        );
    }

    #[test]
    fn the_envelopes_undeclared_keys_are_kept_by_name() {
        let text = r#"{"request_type": "app-started", "api_version": "v2", "future_field": "surprise", "n": 1.50}"#;
        assert_eq!(
            extra_keys(text),
            BTreeMap::from([("future_field".into(), "\"surprise\"".into()), ("n".into(), "1.50".into())])
        );
    }
}
