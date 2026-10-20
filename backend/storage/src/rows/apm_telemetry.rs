use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::Extra;
use crate::column;
use crate::table::{Limits, Table};

/// One instrumentation telemetry request, or one entry of a message-batch request (see
/// batch_index).
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct ApmTelemetry {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub request_type: String,
    pub producer: String,
    pub api_version: String,
    pub runtime_id: String,
    pub seq_id: Option<i64>,
    /// The parsed Unix time; None when nothing was sent, never the epoch. The *_raw twin keeps the
    /// JSON text, so a value that did not parse is still there.
    #[serde(with = "column::millis::option")]
    pub tracer_time: Option<DateTime<Utc>>,
    pub tracer_time_raw: String,
    #[serde(with = "column::millis::option")]
    pub event_time: Option<DateTime<Utc>>,
    pub event_time_raw: String,
    pub service_name: String,
    pub service_version: String,
    pub env: String,
    pub language_name: String,
    pub language_version: String,
    pub tracer_version: String,
    pub hostname: String,
    pub host_os: String,
    pub host_architecture: String,
    /// The host block's other keys, as JSON text by name.
    pub host_extra: BTreeMap<String, String>,
    /// The producer's own part, as it arrived. On the parent row of a batch this is the whole
    /// batch.
    pub payload: String,
    pub debug: String,
    pub origin: String,
    // Headers the trace-agent's proxy adds, never the sender itself.
    pub via: String,
    pub dd_agent_hostname: String,
    pub dd_agent_env: String,
    pub datadog_container_id: String,
    pub x_datadog_container_tags: String,
    /// The envelope's keys with no column, as JSON text by name.
    pub extra: Extra,
    /// None on the request's own row; the entry's position on a row made from a message-batch
    /// entry. Entry 0 is not "no entry".
    pub batch_index: Option<u32>,
    /// "message-batch" on those rows, "" otherwise.
    pub parent_request_type: String,
}

impl Table for ApmTelemetry {
    const WRITER: &'static str = "storage_apm_telemetry";
    const TABLE: &'static str = "apm_telemetry";
    // Every instrumented process heartbeats on its own schedule, so a deploy makes many of them
    // report within the same second.
    const LIMITS: Limits =
        Limits { max_rows: 2_000, flush_interval: Duration::from_secs(5), buffer_limit: 50_000, max_in_flight: 2 };
}
