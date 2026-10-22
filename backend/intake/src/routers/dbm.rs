//! dbm-metrics-intake.<site> — Database Monitoring.
//!
//!   config: none; an event platform track.
//!
//! Six tracks on one host, all JSON arrays of events the agent never parses: Python integrations
//! hand it bytes and the forwarder batches them. No schema is published for them, so the fields
//! the producers agree on become columns of `dbm_events` and the whole event is kept beside them.
//! A body or an element that cannot be read goes to raw_payloads.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use ninjapaws_storage::rows::dbm::DbmEvent;
use serde_json::Value;
use tako::types::Response;

use crate::fields::{Mismatches, fields};
use crate::json;
use crate::raw;
use crate::server::{Body, Ctx, accepted};
use crate::wire::{tags, time};

/// The part of an event that every track shares. The wire is not consistent across tracks, and
/// the envelope absorbs that:
///
///   - tags come as `ddtags` (one comma-joined string on samples, a list on activity) or as
///     `tags` (a list on metrics and metadata);
///   - the agent version is `ddagentversion`, except on dbmmetadata, which says `agent_version`;
///   - the interval is `collection_interval` or `min_collection_interval`.
#[derive(Clone, Debug, Default)]
pub struct Envelope {
    /// `timestamp`, Unix milliseconds, as the number was written: the oracle check sends an
    /// integer, Python integrations a fraction. "" when not sent.
    pub timestamp: String,
    pub host: String,
    pub database_instance: String,
    pub agent_hostname: String,
    pub agent_version: String,
    pub source: String,
    pub dbm_type: String,
    pub kind: String,
    pub dbms: String,
    pub dbms_version: String,
    /// None when neither key was sent: 0 seconds is a value.
    pub collection_interval: Option<f64>,
    pub tags: Vec<String>,
    /// Every key the envelope does not describe (the engine's row arrays, the `db` object of a
    /// sample), and every key whose value did not fit.
    pub extra: BTreeMap<String, Value>,
    /// Envelope keys whose value did not fit; they stay in extra.
    pub undecoded: Vec<String>,
}

/// One event of a batch: its text exactly as sent, and that text parsed.
#[derive(Clone, Debug)]
pub struct Event {
    pub text: String,
    pub json: Value,
}

/// Splits a body into events, each with its text: the elements of a list, or the one object of a
/// sender that skipped the batch. `null` is no events.
pub fn split_events(body: &[u8]) -> Result<Vec<Event>, String> {
    let events = json::try_parse_list(body)?;
    let text = json::raw::text(body);
    Ok(json::raw::list(&text).into_iter().zip(events).map(|(text, json)| Event { text: text.into(), json }).collect())
}

/// A tag list in either spelling: one comma-joined string, or a list.
fn tags_of(value: &Value) -> Option<Vec<String>> {
    match value {
        Value::String(text) => {
            Some(text.split(',').map(str::trim).filter(|tag| !tag.is_empty()).map(String::from).collect())
        }
        other => json::strings_of(other),
    }
}

/// The members of an event not yet taken by a field of the envelope.
struct Left {
    members: BTreeMap<String, Value>,
    undecoded: Vec<String>,
}

impl Left {
    /// The member read with `read`, and gone from the members; a value that does not fit is noted
    /// and stays.
    fn take<T>(&mut self, key: &str, read: impl Fn(&Value) -> Option<T>) -> Option<T> {
        let value = self.members.get(key)?;
        if value.is_null() {
            // null is the same as nothing sent.
            self.members.remove(key);
            return None;
        }
        match read(value) {
            Some(taken) => {
                self.members.remove(key);
                Some(taken)
            }
            None => {
                self.undecoded.push(key.to_string());
                None
            }
        }
    }

    fn text(&mut self, key: &str) -> String {
        self.take(key, json::string_of).unwrap_or_default()
    }
}

/// Reads the envelope field by field, so one malformed value cannot take the others down with it:
/// its key is noted and left among the extra keys.
pub fn decode_envelope(event: &Value) -> Result<Envelope, String> {
    // A null element is refused too, or it would become a row of empty columns.
    if !event.is_object() {
        return Err(format!("expected an object, got {}", json::kind(event)));
    }
    let mut left = Left { members: json::members(event), undecoded: Vec::new() };

    // The order is the order of `undecoded`.
    let timestamp = left.take("timestamp", json::number_text).unwrap_or_default();
    let host = left.text("host");
    let database_instance = left.text("database_instance");
    let agent_hostname = left.text("ddagenthostname");
    let source = left.text("ddsource");
    let dbm_type = left.text("dbm_type");
    let kind = left.text("kind");
    let dbms = left.text("dbms");
    let dbms_version = left.text("dbms_version");
    let agent_version = left.text("ddagentversion");
    let other_agent_version = left.text("agent_version");
    let collection_interval = left.take("collection_interval", json::float_of);
    let min_interval = left.take("min_collection_interval", json::float_of);
    let mut tags = left.take("ddtags", tags_of).unwrap_or_default();
    tags.extend(left.take("tags", tags_of).unwrap_or_default());

    Ok(Envelope {
        timestamp,
        host,
        database_instance,
        agent_hostname,
        agent_version: if agent_version.is_empty() { other_agent_version } else { agent_version },
        source,
        dbm_type,
        kind,
        dbms,
        dbms_version,
        collection_interval: collection_interval.or(min_interval),
        tags,
        extra: left.members,
        undecoded: left.undecoded,
    })
}

/// The envelope's timestamp as a time; None when it was not sent, which must never become 1970 or
/// "now".
///
/// An integer is read as one, so it never passes through a float. A fraction of a millisecond
/// (Python's time.time()*1000) is rounded: the column holds milliseconds anyway.
pub fn timestamp(wire: &str) -> Option<DateTime<Utc>> {
    if wire.is_empty() {
        return None;
    }
    match wire.parse::<i64>() {
        Ok(millis) => Some(time::from_unix_millis(millis)),
        Err(_) => match wire.parse::<f64>() {
            Ok(millis) if millis.is_finite() => Some(time::from_unix_millis(millis.round() as i64)),
            _ => None,
        },
    }
}

/// What `db.plan.definition` says about the plan's steps. The column can hold only two of these
/// facts (NULL or a count).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PlanSteps {
    /// Absent or null: the check sends null when it could not collect a plan.
    NoDefinition,
    /// The oracle check's shape: a list of steps.
    Steps(u32),
    /// The postgres and mysql checks' shape: the database's own EXPLAIN output, as JSON inside a
    /// string. There are no steps to count.
    Document,
    NotAPlan,
}

pub fn plan_steps(definition: Option<&Value>) -> PlanSteps {
    match definition {
        None | Some(Value::Null) => PlanSteps::NoDefinition,
        Some(Value::Array(steps)) => PlanSteps::Steps(steps.len() as u32),
        Some(Value::String(_)) => PlanSteps::Document,
        Some(_) => PlanSteps::NotAPlan,
    }
}

/// The `db` object of a databasequery event, as the oracle check's FQT and plan payloads agree on
/// it.
struct QuerySample {
    instance: String,
    query_signature: String,
    statement: String,
    /// The plan's signature and steps; None when no plan was sent.
    plan: Option<(String, PlanSteps)>,
}

/// None when a field of the object has the wrong type: then none of it is used.
fn query_sample(db: &Value) -> Option<QuerySample> {
    let bad = Mismatches::default();
    let fields = fields(&bad, "db", db);
    let sample = QuerySample {
        instance: fields.string("instance"),
        query_signature: fields.string("query_signature"),
        statement: fields.string("statement"),
        plan: fields
            .optional_object("plan")
            .map(|plan| (plan.string("signature"), plan_steps(plan.find("definition")))),
    };
    bad.borrow().is_empty().then_some(sample)
}

fn non_empty(text: &str) -> Option<String> {
    (!text.is_empty()).then(|| text.to_string())
}

pub fn to_row(tenant: &str, track: &str, received_at: DateTime<Utc>, event: &str, envelope: Envelope) -> DbmEvent {
    let sample = envelope.extra.get("db").and_then(query_sample);
    let plan = sample.as_ref().and_then(|s| s.plan.clone());

    // A definition that is neither shape is not "no plan". The column cannot say so, so the list
    // of undecoded keys does.
    let mut undecoded = envelope.undecoded;
    if matches!(plan, Some((_, PlanSteps::NotAPlan))) {
        undecoded.push("db.plan.definition".into());
    }

    DbmEvent {
        tenant_id: tenant.into(),
        received_at,
        track: track.into(),
        timestamp: timestamp(&envelope.timestamp),
        host: envelope.host,
        database_instance: envelope.database_instance,
        agent_hostname: envelope.agent_hostname,
        agent_version: envelope.agent_version,
        source: envelope.source,
        dbm_type: envelope.dbm_type,
        kind: envelope.kind,
        dbms: envelope.dbms,
        dbms_version: envelope.dbms_version,
        collection_interval: envelope.collection_interval,
        tags: tags::to_multi_map(&envelope.tags),
        extra_keys: envelope.extra.keys().cloned().collect(),
        undecoded_keys: undecoded,
        event: event.into(),
        db_instance: sample.as_ref().and_then(|s| non_empty(&s.instance)),
        query_signature: sample.as_ref().and_then(|s| non_empty(&s.query_signature)),
        statement: sample.as_ref().and_then(|s| non_empty(&s.statement)),
        plan_signature: plan.as_ref().and_then(|(signature, _)| non_empty(signature)),
        plan_definition_steps: match plan {
            Some((_, PlanSteps::Steps(count))) => Some(count),
            _ => None,
        },
    }
}

/// Stores one track's batch. `track` is the route's name, and what the rows say they arrived on.
pub async fn handle(track: &'static str, ctx: Ctx, body: Body) -> Response {
    // The agent's start-up probe: a row of it would be an event with nothing in it.
    if raw::is_probe(&body) {
        return accepted();
    }
    match split_events(&body) {
        Err(e) => ctx.raw("dbm", "decode_error", &format!("[{track}] not a JSON array or object: {e}"), &body),
        Ok(events) => {
            let received_at = Utc::now();
            let mut rows = Vec::new();
            for (i, event) in events.into_iter().enumerate() {
                match decode_envelope(&event.json) {
                    Err(e) => {
                        ctx.raw("dbm", "decode_error", &format!("[{track}] event {i}: {e}"), event.text.as_bytes())
                    }
                    Ok(envelope) => rows.push(to_row(ctx.tenant(), track, received_at, &event.text, envelope)),
                }
            }
            ctx.write(rows);
        }
    }
    accepted()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(event: &str) -> Envelope {
        decode_envelope(&json::try_parse(event.as_bytes()).unwrap()).expect("the event decodes")
    }

    fn received_at() -> DateTime<Utc> {
        "2026-09-23T12:00:00Z".parse().unwrap()
    }

    fn row(track: &str, event: &str) -> DbmEvent {
        to_row("test-tenant", track, received_at(), event, envelope(event))
    }

    fn keys(envelope: &Envelope) -> Vec<&str> {
        envelope.extra.keys().map(String::as_str).collect()
    }

    #[test]
    fn ddtags_as_one_comma_joined_string_becomes_a_tag_list_and_the_db_object_stays_an_extra_key() {
        // The oracle check's FQTPayload.
        let e = envelope(
            r#"{"timestamp":1758326400000,"host":"db1","database_instance":"db1/orcl",
                "ddagentversion":"7.60.0","ddsource":"oracle","ddtags":"env:prod, team:db,,",
                "dbm_type":"fqt","db":{"instance":"orcl","query_signature":"abc","statement":"select 1"}}"#,
        );
        assert_eq!(e.tags, ["env:prod", "team:db"]);
        assert_eq!(e.host, "db1");
        assert_eq!(e.database_instance, "db1/orcl");
        assert_eq!(e.dbm_type, "fqt");
        assert_eq!(e.source, "oracle");
        assert_eq!(e.agent_version, "7.60.0");
        assert_eq!(keys(&e), ["db"]);
        assert!(e.undecoded.is_empty());
    }

    #[test]
    fn tags_as_a_list_are_read_under_either_key_and_the_interval_under_either_name() {
        for event in [
            r#"{"ddtags":["env:prod","team:db"],"collection_interval":10,"oracle_activity":[{},{}]}"#,
            r#"{"tags":["env:prod","team:db"],"min_collection_interval":10,"oracle_rows":[{},{}]}"#,
        ] {
            let e = envelope(event);
            assert_eq!(e.tags, ["env:prod", "team:db"]);
            assert_eq!(e.collection_interval, Some(10.0));
        }
    }

    #[test]
    fn ddtags_and_tags_sent_together_are_both_kept_ddtags_first() {
        assert_eq!(envelope(r#"{"tags":["b:2"],"ddtags":"a:1"}"#).tags, ["a:1", "b:2"]);
    }

    #[test]
    fn agent_version_stands_in_for_ddagentversion() {
        // The oracle check's dbInstanceEvent says agent_version; everything else ddagentversion.
        let e = envelope(r#"{"agent_version":"7.60.0","kind":"database_instance","dbms":"oracle"}"#);
        assert_eq!(e.agent_version, "7.60.0");
        assert_eq!(e.kind, "database_instance");
        assert_eq!(e.dbms, "oracle");
    }

    #[test]
    fn a_malformed_envelope_field_is_noted_and_kept_and_does_not_take_the_others_down() {
        let e = envelope(r#"{"host":"db1","timestamp":"not-a-number","postgres_rows":[1,2,3],"custom":{"a":1}}"#);
        assert_eq!(e.host, "db1");
        assert_eq!(e.undecoded, ["timestamp"]);
        assert_eq!(keys(&e), ["custom", "postgres_rows", "timestamp"]);
        assert_eq!(json::compact(&e.extra["timestamp"]), "\"not-a-number\"");
    }

    #[test]
    fn null_leaves_a_field_empty_without_marking_it_undecoded() {
        let e =
            envelope(r#"{"host":null,"timestamp":null,"collection_interval":null,"ddtags":null,"tags":["a:1",null]}"#);
        assert_eq!(e.host, "");
        assert_eq!(e.timestamp, "");
        assert_eq!(e.collection_interval, None);
        assert_eq!(e.tags, ["a:1", ""]);
        assert!(e.undecoded.is_empty());
        assert!(e.extra.is_empty());
    }

    #[test]
    fn a_malformed_interval_is_absent_and_is_noted() {
        let e = envelope(r#"{"collection_interval":"x","min_collection_interval":3}"#);
        assert_eq!(e.collection_interval, Some(3.0));
        assert_eq!(e.undecoded, ["collection_interval"]);
        assert!(e.extra.contains_key("collection_interval"));

        assert_eq!(envelope(r#"{"min_collection_interval":true}"#).collection_interval, None);
        assert_eq!(
            envelope(r#"{"collection_interval":5,"min_collection_interval":"x"}"#).collection_interval,
            Some(5.0)
        );
        assert_eq!(
            envelope(r#"{"collection_interval":null,"min_collection_interval":7}"#).collection_interval,
            Some(7.0)
        );
    }

    #[test]
    fn envelope_keys_match_exactly_and_the_last_of_two_equal_keys_wins() {
        let e = envelope(r#"{"Host":"upper","host":"a","host":"b"}"#);
        assert_eq!(e.host, "b");
        assert_eq!(keys(&e), ["Host"]);
    }

    #[test]
    fn the_timestamp_is_kept_as_written_a_number_inside_a_string_included() {
        assert_eq!(envelope(r#"{"timestamp":1758326400123.456}"#).timestamp, "1758326400123.456");
        assert_eq!(envelope(r#"{"timestamp":"1758326400123"}"#).timestamp, "1758326400123");
        // Absent means not sent, not 1970.
        assert_eq!(envelope(r#"{"host":"db1"}"#).timestamp, "");
    }

    #[test]
    fn an_event_that_is_not_an_object_is_refused_null_too() {
        assert_eq!(decode_envelope(&Value::Null).unwrap_err(), "expected an object, got null");
        assert_eq!(decode_envelope(&Value::from("text")).unwrap_err(), "expected an object, got string");
        assert!(decode_envelope(&Value::from(42)).is_err());
    }

    #[test]
    fn a_body_is_an_array_of_events_one_bare_object_or_refused() {
        let count = |body: &str| split_events(body.as_bytes()).map(|events| events.len());
        assert_eq!(count(r#"[{"a":1},{"b":2}]"#), Ok(2));
        assert_eq!(count(r#"{"a":1}"#), Ok(1));
        assert_eq!(count("null"), Ok(0));
        assert!(count("not json").is_err());
        assert!(count("\"text\"").is_err());
    }

    #[test]
    fn a_bare_object_and_each_element_keep_their_text_as_sent() {
        let events = split_events(b"[ {\"host\" : \"db1\", \"n\": 1.50 ,\"e\":1E3} ]").unwrap();
        assert_eq!(events[0].text, r#"{"host" : "db1", "n": 1.50 ,"e":1E3}"#);

        let body = " {\"host\": \"db1\",\n  \"status\": \"ok\"}\n";
        assert_eq!(split_events(body.as_bytes()).unwrap()[0].text, body.trim());
        let texts: Vec<String> = split_events(br#"[ {"host" : "db1"} , {"host":"db2"} ]"#)
            .unwrap()
            .into_iter()
            .map(|event| event.text)
            .collect();
        assert_eq!(texts, [r#"{"host" : "db1"}"#, r#"{"host":"db2"}"#]);
    }

    #[test]
    fn a_string_python_could_not_encode_is_stored_with_the_replacement_character() {
        // Python writes half a surrogate pair for a byte it could not decode.
        let events = split_events(br#"[{"host":"a\ud800b","ddtags":"k:\udc80,ok:1","k\ud800":1}]"#).unwrap();
        let e = decode_envelope(&events[0].json).unwrap();
        assert_eq!(e.host, "a\u{fffd}b");
        assert_eq!(tags::to_multi_map(&e.tags), tags::to_multi_map(["k:\u{fffd}", "ok:1"]));
        assert_eq!(keys(&e), ["k\u{fffd}"]);
    }

    #[test]
    fn timestamp_an_integer_stays_exact_a_fraction_rounds_to_the_millisecond_absent_is_none() {
        assert_eq!(timestamp("1758326400000"), Some(time::from_unix_millis(1758326400000)));
        assert_eq!(timestamp("1758326400123.6"), Some(time::from_unix_millis(1758326400124)));
        assert_eq!(timestamp(""), None);
    }

    #[test]
    fn plan_steps_tells_no_definition_a_list_of_steps_an_explain_document_and_neither_apart() {
        let value = |text: &str| json::try_parse(text.as_bytes()).unwrap();
        assert_eq!(plan_steps(None), PlanSteps::NoDefinition);
        // What the postgres check sends when it could not collect a plan.
        assert_eq!(plan_steps(Some(&Value::Null)), PlanSteps::NoDefinition);
        assert_eq!(plan_steps(Some(&value("[]"))), PlanSteps::Steps(0));
        assert_eq!(plan_steps(Some(&value(r#"[{"id":1},{"id":2},{"id":3}]"#))), PlanSteps::Steps(3));
        assert_eq!(plan_steps(Some(&value(r#""{\"Plan\":{}}""#))), PlanSteps::Document);
        assert_eq!(plan_steps(Some(&value("5"))), PlanSteps::NotAPlan);
    }

    #[test]
    fn a_postgres_plan_explain_output_inside_a_string_is_a_plan_and_not_an_undecoded_key() {
        let r = row(
            "databasequery",
            r#"{"timestamp":1758326400000,"host":"pg1","ddsource":"postgres","dbm_type":"plan",
                "db":{"instance":"shop","statement":"SELECT ?","plan":{"signature":"sig-1","definition":"{\"Plan\":{\"Node Type\":\"Limit\"}}"}}}"#,
        );
        assert_eq!(r.plan_signature.as_deref(), Some("sig-1"));
        assert_eq!(r.plan_definition_steps, None);
        assert!(r.undecoded_keys.is_empty());
    }

    #[test]
    fn a_plan_whose_definition_is_neither_keeps_its_signature_and_is_noted_as_undecoded() {
        let r = row(
            "databasequery",
            r#"{"timestamp":1758326400000,"host":"db1","database_instance":"db1/orcl","dbm_type":"plan",
                "db":{"instance":"orcl","plan":{"signature":"plan-xyz","definition":5}}}"#,
        );
        assert_eq!(r.plan_definition_steps, None);
        assert_eq!(r.plan_signature.as_deref(), Some("plan-xyz"));
        assert!(r.undecoded_keys.contains(&"db.plan.definition".to_string()));
    }

    #[test]
    fn a_db_object_with_a_field_of_the_wrong_type_gives_none_of_the_sample_columns() {
        let r = row("databasequery", r#"{"db":{"instance":"orcl","query_signature":7,"statement":"select 1"}}"#);
        assert_eq!(r.db_instance, None);
        assert_eq!(r.query_signature, None);
        assert_eq!(r.statement, None);
        assert_eq!(r.extra_keys, ["db"]);
    }

    #[test]
    fn dbmmetrics_the_oracle_checks_metrics_payload() {
        let event = r#"{"host":"db1","database_instance":"db1/orcl","timestamp":1758326400000,
            "min_collection_interval":60,"tags":["env:prod","team:db"],
            "ddagentversion":"7.60.0","ddagenthostname":"agent-host-1",
            "oracle_rows":[{"query_signature":"sig1","sql_text":"select 1"}],"oracle_version":"19c"}"#;
        let r = row("dbmmetrics", event);
        assert_eq!(r.tenant_id, "test-tenant");
        assert_eq!(r.track, "dbmmetrics");
        assert_eq!(r.received_at, received_at());
        assert_eq!(r.timestamp, Some(time::from_unix_millis(1758326400000)));
        assert_eq!(r.event, event);
        assert_eq!(r.agent_hostname, "agent-host-1");
        assert_eq!(r.collection_interval, Some(60.0));
        assert_eq!(r.tags, tags::to_multi_map(["env:prod", "team:db"]));
        assert_eq!(r.extra_keys, ["oracle_rows", "oracle_version"]);
    }

    #[test]
    fn databasequery_a_full_query_text_sample_fills_the_sample_columns_and_has_no_plan() {
        let r = row(
            "databasequery",
            r#"{"timestamp":1758326400000,"host":"db1","database_instance":"db1/orcl",
                "ddagentversion":"7.60.0","ddsource":"oracle","ddtags":"env:prod,team:db","dbm_type":"fqt",
                "db":{"instance":"orcl","query_signature":"abc123","statement":"select 1 from dual",
                      "metadata":{"dd_tables":["t1"],"dd_commands":["SELECT"]}}}"#,
        );
        assert_eq!(r.db_instance.as_deref(), Some("orcl"));
        assert_eq!(r.query_signature.as_deref(), Some("abc123"));
        assert_eq!(r.statement.as_deref(), Some("select 1 from dual"));
        assert_eq!(r.plan_signature, None);
        assert_eq!(r.plan_definition_steps, None);
    }

    #[test]
    fn databasequery_a_plans_steps_are_counted() {
        let r = row(
            "databasequery",
            r#"{"timestamp":1758326400000,"host":"db1","dbm_type":"plan",
                "db":{"instance":"orcl","plan":{"signature":"plan-xyz",
                      "definition":[{"id":1},{"id":2},{"id":3}]}}}"#,
        );
        assert_eq!(r.plan_signature.as_deref(), Some("plan-xyz"));
        assert_eq!(r.plan_definition_steps, Some(3));
    }

    #[test]
    fn an_event_without_a_timestamp_gives_a_row_without_one() {
        assert_eq!(row("dbmhealth", r#"{"host":"db1"}"#).timestamp, None);
    }
}
