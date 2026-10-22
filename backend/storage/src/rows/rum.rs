//! RUM: everything the browser, iOS and Android SDKs send to browser-intake.
//!
//! Every row carries the whole payload it came from (event, segment, span). The RUM schema has
//! over a hundred optional fields per variant and grows with every SDK release, so the typed
//! columns are a filtering index over the event, and the event is the record.
//!
//! Every table starts with the sender columns of [`RumRequest`]. A RUM event has no hostname,
//! agent or container id: the request it arrived in is the only thing that says who sent it. The
//! driver's rows cannot nest, so each row repeats them.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::Tags;
use crate::column::{self, Blob};
use crate::table::Table;

/// The sender columns every RUM table starts with, as one request gives them.
#[derive(Clone, Debug)]
pub struct RumRequest {
    pub tenant_id: String,
    pub received_at: DateTime<Utc>,
    pub ddsource: String,
    pub evp_origin: String,
    pub evp_origin_version: String,
    /// How the batch arrived compressed, as the sender spelled it. The body is stored decoded, so
    /// nothing else in the row remembers.
    pub evp_encoding: String,
    pub request_id: String,
    pub idempotency_key: String,
    /// Browser only: the transport, "fetch" or "beacon". A beacon is fire and forget at page
    /// unload, so a gap in beacon traffic means something different from a gap in fetch traffic.
    pub dd_api: String,
    pub batch_time: Option<DateTime<Utc>>,
    /// None on a first attempt: "never retried" and "retried zero times" are different facts.
    pub retry_count: Option<u32>,
    pub retry_after: Option<i64>,
    pub remote_addr: String,
    pub user_agent: String,
    /// Query parameters without a column of their own, as JSON, credentials removed.
    pub query_extra: String,
}

/// One view, upserted on (application, session, view) with document_version as the
/// ReplacingMergeTree version.
///
/// Counters and Web Vitals are options throughout: a view with no crash object has not measured
/// zero crashes, and a 0 would be averaged.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RumView {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub ddsource: String,
    pub evp_origin: String,
    pub evp_origin_version: String,
    pub evp_encoding: String,
    pub request_id: String,
    pub idempotency_key: String,
    pub dd_api: String,
    #[serde(with = "column::millis::option")]
    pub batch_time: Option<DateTime<Utc>>,
    pub retry_count: Option<u32>,
    pub retry_after: Option<i64>,
    pub remote_addr: String,
    pub user_agent: String,
    pub query_extra: String,
    #[serde(with = "column::millis")]
    pub date: DateTime<Utc>,
    pub application_id: String,
    pub session_id: String,
    pub view_id: String,
    pub document_version: u64,
    /// "view" or "view_update": which wire form produced this version.
    pub event_type: String,
    pub service: String,
    pub version: String,
    pub build_version: String,
    pub build_id: String,
    pub source: String,
    pub session_type: String,
    pub session_has_replay: Option<bool>,
    pub session_is_active: Option<bool>,
    pub session_sampled_for_replay: Option<bool>,
    pub usr_id: String,
    pub usr_name: String,
    pub usr_email: String,
    pub usr_anonymous_id: String,
    pub account_id: String,
    pub account_name: String,
    pub view_url: String,
    pub view_name: String,
    pub view_referrer: String,
    pub view_loading_type: String,
    pub view_loading_time: Option<i64>,
    pub view_time_spent: i64,
    pub view_is_active: Option<bool>,
    pub view_is_slow_rendered: Option<bool>,
    pub action_count: Option<i64>,
    pub error_count: Option<i64>,
    pub crash_count: Option<i64>,
    pub long_task_count: Option<i64>,
    pub frozen_frame_count: Option<i64>,
    pub resource_count: Option<i64>,
    pub frustration_count: Option<i64>,
    pub lcp: Option<i64>,
    pub cls: Option<f64>,
    pub inp: Option<i64>,
    pub fcp: Option<i64>,
    pub fid: Option<i64>,
    pub fbc: Option<i64>,
    pub ttfb: Option<i64>,
    pub device_type: String,
    pub device_brand: String,
    pub device_model: String,
    pub device_name: String,
    pub os_name: String,
    pub os_version: String,
    pub connectivity_status: String,
    /// JSON text: free-form maps whose values are arbitrary JSON.
    pub context: String,
    pub feature_flags: String,
    pub ddtags: Tags,
    /// The whole event, as it arrived.
    pub event: String,
}

impl Table for RumView {
    const TABLE: &'static str = "rum_views";
}

/// One action, error, resource, long_task, vital or transition. The columns of the other kinds
/// stay empty; event_type says which kind it is.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RumEvent {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub ddsource: String,
    pub evp_origin: String,
    pub evp_origin_version: String,
    pub evp_encoding: String,
    pub request_id: String,
    pub idempotency_key: String,
    pub dd_api: String,
    #[serde(with = "column::millis::option")]
    pub batch_time: Option<DateTime<Utc>>,
    pub retry_count: Option<u32>,
    pub retry_after: Option<i64>,
    pub remote_addr: String,
    pub user_agent: String,
    pub query_extra: String,
    #[serde(with = "column::millis")]
    pub date: DateTime<Utc>,
    pub application_id: String,
    pub session_id: String,
    pub view_id: String,
    pub event_type: String,
    pub service: String,
    pub version: String,
    pub build_version: String,
    pub build_id: String,
    pub source: String,
    pub session_type: String,
    pub session_has_replay: Option<bool>,
    pub usr_id: String,
    pub usr_name: String,
    pub usr_email: String,
    pub usr_anonymous_id: String,
    pub account_id: String,
    pub account_name: String,
    pub view_url: String,
    pub view_name: String,
    pub view_referrer: String,
    pub device_type: String,
    pub device_brand: String,
    pub device_model: String,
    pub device_name: String,
    pub os_name: String,
    pub os_version: String,
    pub connectivity_status: String,
    pub action_type: String,
    pub action_name: String,
    pub action_id: String,
    /// One click can be classified several ways at once (rage and dead).
    pub action_frustration_types: Vec<String>,
    pub error_id: String,
    pub error_message: String,
    pub error_type: String,
    pub error_source: String,
    pub error_stack: String,
    pub error_is_crash: Option<bool>,
    pub error_handling: String,
    pub error_fingerprint: String,
    pub resource_id: String,
    pub resource_type: String,
    pub resource_url: String,
    pub resource_method: String,
    pub resource_status_code: Option<i64>,
    pub resource_duration: Option<i64>,
    pub resource_size: Option<i64>,
    pub long_task_id: String,
    pub long_task_duration: Option<i64>,
    pub vital_id: String,
    pub vital_type: String,
    pub vital_name: String,
    pub vital_duration: Option<i64>,
    pub context: String,
    pub ddtags: Tags,
    pub event: String,
}

impl Table for RumEvent {
    const TABLE: &'static str = "rum_events";
}

/// One type:"telemetry" event: the SDK reporting on itself.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RumTelemetry {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub ddsource: String,
    pub evp_origin: String,
    pub evp_origin_version: String,
    pub evp_encoding: String,
    pub request_id: String,
    pub idempotency_key: String,
    pub dd_api: String,
    #[serde(with = "column::millis::option")]
    pub batch_time: Option<DateTime<Utc>>,
    pub retry_count: Option<u32>,
    pub retry_after: Option<i64>,
    pub remote_addr: String,
    pub user_agent: String,
    pub query_extra: String,
    #[serde(with = "column::millis")]
    pub date: DateTime<Utc>,
    pub application_id: String,
    pub session_id: String,
    pub view_id: String,
    pub action_id: String,
    pub r#type: String,
    pub status: String,
    pub telemetry_type: String,
    pub message: String,
    pub error_stack: String,
    pub error_kind: String,
    /// The whole telemetry.configuration object, as JSON.
    pub configuration: String,
    pub usage_feature: String,
    pub service: String,
    pub version: String,
    pub source: String,
    pub effective_sample_rate: Option<f64>,
    pub experimental_features: Vec<String>,
    pub device_brand: String,
    pub device_model: String,
    pub device_architecture: String,
    pub os_name: String,
    pub os_version: String,
    pub os_build: String,
    pub event: String,
}

impl Table for RumTelemetry {
    const TABLE: &'static str = "rum_telemetry";
}

/// One mobile cpu or memory sample run. Timestamps and values stay the parallel arrays they
/// arrived as: timestamps[i] belongs to values.*[i].
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RumTimeseries {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub ddsource: String,
    pub evp_origin: String,
    pub evp_origin_version: String,
    pub evp_encoding: String,
    pub request_id: String,
    pub idempotency_key: String,
    pub dd_api: String,
    #[serde(with = "column::millis::option")]
    pub batch_time: Option<DateTime<Utc>>,
    pub retry_count: Option<u32>,
    pub retry_after: Option<i64>,
    pub remote_addr: String,
    pub user_agent: String,
    pub query_extra: String,
    #[serde(with = "column::millis")]
    pub date: DateTime<Utc>,
    pub application_id: String,
    pub session_id: String,
    pub view_id: String,
    pub name: String,
    pub timeseries_id: String,
    pub timeseries_schema: String,
    #[serde(with = "column::nanos")]
    pub start: i64,
    #[serde(with = "column::nanos")]
    pub end: i64,
    pub timestamps: Vec<i64>,
    /// data.values, as JSON: a different object per variant.
    pub values: String,
    pub service: String,
    pub version: String,
    pub source: String,
    pub context: String,
    pub ddtags: Tags,
    pub event: String,
}

impl Table for RumTimeseries {
    const TABLE: &'static str = "rum_timeseries";
}

/// One blob part of a /api/v2/replay upload, with the metadata entry that described it. The bytes
/// are stored exactly as received: the SDK emits zlib streams built to be concatenated later.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RumReplaySegment {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub ddsource: String,
    pub evp_origin: String,
    pub evp_origin_version: String,
    pub evp_encoding: String,
    pub request_id: String,
    pub idempotency_key: String,
    pub dd_api: String,
    #[serde(with = "column::millis::option")]
    pub batch_time: Option<DateTime<Utc>>,
    pub retry_count: Option<u32>,
    pub retry_after: Option<i64>,
    pub remote_addr: String,
    pub user_agent: String,
    pub query_extra: String,
    pub application_id: String,
    pub session_id: String,
    pub view_id: String,
    pub source: String,
    /// "segment" or "resource".
    pub variant: String,
    pub part_name: String,
    pub part_filename: String,
    #[serde(with = "column::millis::option")]
    pub start: Option<DateTime<Utc>>,
    #[serde(with = "column::millis::option")]
    pub end: Option<DateTime<Utc>>,
    pub records_count: Option<i64>,
    pub has_full_snapshot: Option<bool>,
    pub index_in_view: Option<i64>,
    pub creation_reason: String,
    pub raw_segment_size: Option<i64>,
    pub compressed_segment_size: Option<i64>,
    pub segment: Blob,
    pub image: Blob,
    pub event: String,
}

impl Table for RumReplaySegment {
    const TABLE: &'static str = "rum_replay_segments";
}

/// One span out of a /api/v2/spans envelope.
///
/// The three ids are text. The mobile SDKs write them as decimal or hex text of 64- and 128-bit
/// numbers; parsing would need a guess about the base.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RumSpan {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub ddsource: String,
    pub evp_origin: String,
    pub evp_origin_version: String,
    pub evp_encoding: String,
    pub request_id: String,
    pub idempotency_key: String,
    pub dd_api: String,
    #[serde(with = "column::millis::option")]
    pub batch_time: Option<DateTime<Utc>>,
    pub retry_count: Option<u32>,
    pub retry_after: Option<i64>,
    pub remote_addr: String,
    pub user_agent: String,
    pub query_extra: String,
    pub trace_id: String,
    pub span_id: String,
    pub parent_id: String,
    pub name: String,
    pub service: String,
    pub resource: String,
    pub r#type: String,
    /// From the envelope around the span, not from the span.
    pub env: String,
    #[serde(with = "column::nanos")]
    pub start: i64,
    pub duration_ns: i64,
    pub error: i8,
    /// meta's nested objects (device, os) arrive flattened to dotted keys.
    pub meta: BTreeMap<String, String>,
    pub metrics: BTreeMap<String, f64>,
    pub span: String,
    /// Envelope keys other than "spans" and "env", as JSON.
    pub envelope_extra: String,
}

impl Table for RumSpan {
    const TABLE: &'static str = "rum_spans";
}
