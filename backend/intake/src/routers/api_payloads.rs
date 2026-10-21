//! The wire formats of api.<site>.
//!
//!   /api/v1/series               the older public API's series, JSON
//!   /api/v2/series               agent-payload's MetricPayload (protobuf, or JSON of the same shape)
//!   /api/beta/sketches           agent-payload's SketchPayload (protobuf, or JSON of the same shape)
//!   /api/v1/check_run            a list of service checks, JSON
//!   /api/v1/events               an event or a list of them, JSON
//!   /api/v1/distribution_points  series of raw values, JSON
//!
//! The JSON ones are read item by item. An item that is not an object, has a member of the wrong
//! type or lacks one it must have is an Err saying which; the handler keeps that item raw beside
//! the rows of the others.

use std::collections::BTreeMap;

use ninjapaws_proto::datadog::agentpayload::{MetricPayload, SketchPayload};
use prost::Message;
use serde_json::Value;

use crate::fields::{Fields, Mismatches, fields};
use crate::json;

fn parse_protobuf<T: Message + Default>(name: &str, body: &[u8]) -> Result<T, String> {
    T::decode(body).map_err(|e| format!("protobuf {name}: {e}"))
}

pub fn parse_series_v2_protobuf(body: &[u8]) -> Result<MetricPayload, String> {
    parse_protobuf("MetricPayload", body)
}

/// What the public API's clients send: the same message as JSON.
pub fn parse_series_v2_json(body: &[u8]) -> Result<MetricPayload, String> {
    json::proto::try_parse(body).map_err(|e| format!("JSON MetricPayload: {e}"))
}

pub fn parse_sketches_protobuf(body: &[u8]) -> Result<SketchPayload, String> {
    parse_protobuf("SketchPayload", body)
}

pub fn parse_sketches_json(body: &[u8]) -> Result<SketchPayload, String> {
    json::proto::try_parse(body).map_err(|e| format!("JSON SketchPayload: {e}"))
}

// ---------------------------------------------------------------------------
// The JSON bodies of the v1 API
// ---------------------------------------------------------------------------

/// Reads the item at `path` with `read`; Err is everything that did not fit, by member.
fn read_item<'a, T>(path: &str, item: &'a Value, read: impl FnOnce(&Fields<'_, 'a>) -> T) -> Result<T, String> {
    let bad = Mismatches::default();
    let fields = fields(&bad, path, item);
    if let Some(first) = bad.borrow().first() {
        return Err(first.clone());
    }
    let result = read(&fields);
    let bad = bad.borrow();
    if bad.is_empty() { Ok(result) } else { Err(bad.join("; ")) }
}

/// The members of an item that have no field of their own.
fn additional(declared: &[&str], item: &Value) -> BTreeMap<String, Value> {
    json::members(item).into_iter().filter(|(name, _)| !declared.contains(&name.as_str())).collect()
}

/// The items of a list with their places, the null ones left out: a null carries nothing.
fn placed<'a>(list: impl IntoIterator<Item = &'a Value>) -> Vec<(usize, &'a Value)> {
    list.into_iter().enumerate().filter(|(_, item)| !item.is_null()).collect()
}

/// A v1 point as sent: a [timestamp, value] pair where either may be null.
#[derive(Clone, Debug)]
pub struct SeriesPoint {
    pub pair: Vec<Option<f64>>,
    /// The point's text as sent.
    pub raw: String,
}

#[derive(Clone, Debug)]
pub struct SeriesV1 {
    pub host: String,
    pub interval: Option<i64>,
    pub metric: String,
    pub points: Vec<SeriesPoint>,
    pub tags: Vec<String>,
    pub kind: String,
    /// The members without a field here. The agent's v1 encoder sends three: device,
    /// source_type_name and unit.
    pub additional: BTreeMap<String, Value>,
}

/// The points of a series; `texts` are their texts as sent, in the same order.
fn series_points_of(list: &Value, texts: &[&str]) -> Option<Vec<SeriesPoint>> {
    let pair = |(item, text): (&Value, &str)| -> Option<SeriesPoint> {
        let raw = text.to_string();
        match item {
            Value::Null => Some(SeriesPoint { pair: Vec::new(), raw }),
            Value::Array(numbers) => {
                let pair = numbers
                    .iter()
                    .map(|number| if number.is_null() { Some(None) } else { json::float_of(number).map(Some) })
                    .collect::<Option<Vec<_>>>()?;
                Some(SeriesPoint { pair, raw })
            }
            _ => None,
        }
    };
    list.as_array()?.iter().zip(texts.iter().copied().chain(std::iter::repeat(""))).map(pair).collect()
}

/// The texts of a member's elements, as sent.
fn element_texts<'t>(name: &str, text: &'t str) -> Vec<&'t str> {
    json::raw::member(name, text).map(json::raw::elements).unwrap_or_default()
}

/// `text` is the item as it was sent.
pub fn decode_series_v1(path: &str, item: &Value, text: &str) -> Result<SeriesV1, String> {
    let point_texts = element_texts("points", text);
    read_item(path, item, |fields| {
        fields.require("metric");
        fields.require("points");
        SeriesV1 {
            host: fields.string("host"),
            interval: fields.optional_int64("interval"),
            metric: fields.string("metric"),
            points: fields.read("points", "a list of [timestamp, value] pairs", Vec::new(), |list| {
                series_points_of(list, &point_texts)
            }),
            tags: fields.strings("tags"),
            kind: fields.string("type"),
            additional: additional(&["host", "interval", "metric", "points", "tags", "type"], item),
        }
    })
}

/// One element of a distribution point.
#[derive(Clone, Debug, PartialEq)]
pub enum DistributionItem {
    Timestamp(f64),
    Values(Vec<f64>),
    /// null: neither.
    Absent,
    /// Anything else.
    Unfit,
}

#[derive(Clone, Debug)]
pub struct DistributionPoint {
    pub items: Vec<DistributionItem>,
    /// The point's text as sent.
    pub raw: String,
}

#[derive(Clone, Debug)]
pub struct DistributionSeries {
    pub host: String,
    pub metric: String,
    pub points: Vec<DistributionPoint>,
    pub tags: Vec<String>,
    /// "" when not sent. The only word the API has is "distribution".
    pub kind: String,
    pub additional: BTreeMap<String, Value>,
    /// The series as it arrived.
    pub raw: String,
}

fn distribution_item_of(item: &Value) -> DistributionItem {
    match item {
        Value::Null => DistributionItem::Absent,
        Value::Number(_) => json::float_of(item).map_or(DistributionItem::Unfit, DistributionItem::Timestamp),
        _ => json::floats_of(item).map_or(DistributionItem::Unfit, DistributionItem::Values),
    }
}

/// The points of a series; `texts` are their texts as sent, in the same order.
fn distribution_points_of(list: &Value, texts: &[&str]) -> Option<Vec<DistributionPoint>> {
    let point = |(item, text): (&Value, &str)| match item {
        Value::Null => Some(DistributionPoint { items: Vec::new(), raw: text.into() }),
        Value::Array(elements) => {
            Some(DistributionPoint { items: elements.iter().map(distribution_item_of).collect(), raw: text.into() })
        }
        _ => None,
    };
    list.as_array()?.iter().zip(texts.iter().copied().chain(std::iter::repeat(""))).map(point).collect()
}

/// `text` is the item as it was sent.
pub fn decode_distribution_series(path: &str, item: &Value, text: &str) -> Result<DistributionSeries, String> {
    let point_texts = element_texts("points", text);
    read_item(path, item, |fields| {
        fields.require("metric");
        fields.require("points");
        DistributionSeries {
            host: fields.string("host"),
            metric: fields.string("metric"),
            points: fields.read("points", "a list of [timestamp, [values]] pairs", Vec::new(), |list| {
                distribution_points_of(list, &point_texts)
            }),
            tags: fields.strings("tags"),
            kind: fields.string("type"),
            additional: additional(&["host", "metric", "points", "tags", "type"], item),
            raw: text.into(),
        }
    })
}

/// A body of the shape {"series": [...]}, and whatever else the sender put beside the list.
#[derive(Clone, Debug)]
pub struct SeriesPayload<S> {
    pub series: Vec<S>,
    /// Where each of series stood in the list as sent.
    pub positions: Vec<usize>,
    /// The elements of the list that did not fit, each with its place, its text as sent and what
    /// was wrong with it.
    pub rejected: Vec<(usize, String, String)>,
    pub additional: BTreeMap<String, Value>,
}

/// Item by item, so one element that does not fit does not cost the series beside it.
fn parse_payload<S>(
    label: &str,
    decode: impl Fn(&str, &Value, &str) -> Result<S, String>,
    body: &[u8],
) -> Result<SeriesPayload<S>, String> {
    let root = json::try_parse(body).map_err(|e| format!("{label}: {e}"))?;
    let list = read_item("", &root, |fields| {
        fields.require("series");
        fields.items("series")
    })
    .map_err(|e| format!("{label}: {e}"))?;

    let mut payload = SeriesPayload {
        series: Vec::new(),
        positions: Vec::new(),
        rejected: Vec::new(),
        additional: additional(&["series"], &root),
    };
    let text = json::raw::text(body);
    let texts = element_texts("series", &text);
    for (i, item) in placed(list) {
        let item_text = texts.get(i).copied().unwrap_or_default();
        match decode(&format!("series[{i}]"), item, item_text) {
            Ok(series) => {
                payload.series.push(series);
                payload.positions.push(i);
            }
            Err(why) => payload.rejected.push((i, item_text.into(), why)),
        }
    }
    Ok(payload)
}

pub fn parse_series_v1(body: &[u8]) -> Result<SeriesPayload<SeriesV1>, String> {
    parse_payload("JSON v1 series", decode_series_v1, body)
}

pub fn parse_distribution_points(body: &[u8]) -> Result<SeriesPayload<DistributionSeries>, String> {
    parse_payload("JSON distribution_points", decode_distribution_series, body)
}

#[derive(Clone, Debug)]
pub struct ServiceCheck {
    pub check: String,
    pub host_name: String,
    pub message: String,
    pub status: i32,
    pub tags: Vec<String>,
    pub timestamp: i64,
    pub additional: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, Default)]
pub struct CheckRuns {
    pub runs: Vec<ServiceCheck>,
    /// The items that did not fit, each with its place, its text as sent and what was wrong.
    pub rejected: Vec<(usize, String, String)>,
}

/// Decodes one check. Datadog's OpenAPI spec calls `tags` required; the agent disagrees and sends
/// `"tags": null` on checks without any (containerd.health, the kubelet checks). With
/// `supply_tags` a missing or null list is an empty one.
///
/// A status outside 0..3 does not fit: it has no name, and an unreadable check must never be
/// stored as OK.
pub fn decode_service_check(supply_tags: bool, path: &str, item: &Value) -> Result<ServiceCheck, String> {
    read_item(path, item, |fields| {
        fields.require("check");
        fields.require("host_name");
        fields.require("status");
        if !supply_tags {
            fields.require("tags");
        }
        let status = fields.int32("status");
        if !(0..=3).contains(&status) {
            fields.invalid("status", &format!("expected 0 to 3, got {status}"));
        }
        ServiceCheck {
            check: fields.string("check"),
            host_name: fields.string("host_name"),
            message: fields.string("message"),
            status,
            tags: fields.strings("tags"),
            timestamp: fields.int64("timestamp"),
            additional: additional(&["check", "host_name", "message", "status", "tags", "timestamp"], item),
        }
    })
}

/// Decodes a check_run batch item by item, so one odd check does not discard the checks beside
/// it. A body that is not a list is read as one check (datadogpy posts a bare object), and there
/// no tags are supplied.
pub fn parse_check_runs(body: &[u8]) -> Result<CheckRuns, String> {
    let root = json::try_parse(body).map_err(|e| format!("JSON check_run: {e}"))?;
    match &root {
        Value::Null => Ok(CheckRuns::default()),
        Value::Array(items) => {
            let mut checks = CheckRuns::default();
            let text = json::raw::text(body);
            let texts = json::raw::elements(&text);
            for (i, item) in placed(items) {
                match decode_service_check(true, &format!("[{i}]"), item) {
                    Ok(run) => checks.runs.push(run),
                    Err(why) => checks.rejected.push((i, texts.get(i).copied().unwrap_or_default().into(), why)),
                }
            }
            Ok(checks)
        }
        _ => match decode_service_check(false, "", &root) {
            Ok(run) => Ok(CheckRuns { runs: vec![run], rejected: Vec::new() }),
            Err(e) => Err(format!("JSON check_run: {e}")),
        },
    }
}

/// The alert types and priorities Datadog has.
pub const EVENT_ALERT_TYPES: [&str; 7] =
    ["error", "warning", "info", "success", "user_update", "recommendation", "snapshot"];
pub const EVENT_PRIORITIES: [&str; 2] = ["normal", "low"];

#[derive(Clone, Debug)]
pub struct EventRequest {
    pub aggregation_key: String,
    /// "" when not sent.
    pub alert_type: String,
    pub date_happened: i64,
    pub device_name: String,
    pub host: String,
    pub priority: Option<String>,
    pub related_event_id: Option<i64>,
    pub source_type_name: String,
    /// None when not sent: the reply then says null, not [].
    pub tags: Option<Vec<String>>,
    pub text: String,
    pub title: String,
    pub additional: BTreeMap<String, Value>,
}

pub fn decode_event(path: &str, item: &Value) -> Result<EventRequest, String> {
    read_item(path, item, |fields| {
        fields.require("text");
        fields.require("title");
        EventRequest {
            aggregation_key: fields.string("aggregation_key"),
            alert_type: fields.string("alert_type"),
            date_happened: fields.int64("date_happened"),
            device_name: fields.string("device_name"),
            host: fields.string("host"),
            priority: fields.optional_string("priority"),
            related_event_id: fields.optional_int64("related_event_id"),
            source_type_name: fields.string("source_type_name"),
            tags: fields.optional_strings("tags"),
            text: fields.string("text"),
            title: fields.string("title"),
            additional: additional(
                &[
                    "aggregation_key",
                    "alert_type",
                    "date_happened",
                    "device_name",
                    "host",
                    "priority",
                    "related_event_id",
                    "source_type_name",
                    "tags",
                    "text",
                    "title",
                ],
                item,
            ),
        }
    })
}

/// The events of a body: a list, or one bare object. One event that does not fit refuses the
/// whole body: the sender gets a 400 and has to know that nothing of it was taken.
pub fn parse_events(body: &[u8]) -> Result<Vec<EventRequest>, String> {
    let root = json::try_parse(body).map_err(|e| format!("JSON events: {e}"))?;
    match &root {
        Value::Null => Ok(Vec::new()),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| decode_event(&format!("[{i}]"), item))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("JSON events: {e}")),
        _ => decode_event("", &root).map(|event| vec![event]).map_err(|e| format!("JSON events: {e}")),
    }
}
