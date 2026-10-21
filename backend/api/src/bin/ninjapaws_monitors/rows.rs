//! The rows this process writes, field by field as the columns are named and typed
//! (`backend/schema/migrations`); the ClickHouse client checks each against the table before it
//! sends one. Instants are unix milliseconds, as a DateTime64(3) holds them. (F#:
//! `Storage/Rows/Monitors.fs`, `Metrics.fs`.)

use std::collections::BTreeMap;

use serde::Serialize;

/// One group of a monitor moving from one state to another, with the thresholds it was judged
/// by: Postgres keeps only the latest definition. `inserted_at` is left to its default, the
/// notifier's cursor.
#[derive(clickhouse::Row, Serialize, Clone, Debug, PartialEq)]
pub struct TransitionRow {
    pub tenant_id: String,
    pub monitor_id: u64,
    pub monitor_version: u32,
    pub query_hash: String,
    pub monitor_kind: String,
    pub group_key: String,
    pub group_tags: Vec<String>,
    pub at: i64,
    /// "none" for a group seen for the first time.
    pub from_state: String,
    pub to_state: String,
    pub reason: String,
    pub actor: Option<String>,
    pub value: Option<f64>,
    /// "none" when no threshold was crossed.
    pub crossed: String,
    pub comparator: String,
    pub critical: Option<f64>,
    pub warning: Option<f64>,
    pub critical_recovery: Option<f64>,
    pub warning_recovery: Option<f64>,
    pub window_from: i64,
    pub window_to: i64,
    pub first_triggered_at: Option<i64>,
    pub last_triggered_at: Option<i64>,
}

#[derive(clickhouse::Row, Serialize, Clone, Debug, PartialEq)]
pub struct EvaluationRow {
    pub tenant_id: String,
    pub monitor_id: u64,
    pub group_key: String,
    pub at: i64,
    pub value: Option<f64>,
    pub state: String,
    /// evaluated, skipped or error.
    pub outcome: String,
    pub detail: String,
}

/// What the notifier did with one transition, one row per attempt.
#[derive(clickhouse::Row, Serialize, Clone, Debug, PartialEq)]
pub struct NotificationRow {
    pub tenant_id: String,
    pub monitor_id: u64,
    pub group_key: String,
    pub transition_at: i64,
    /// The transition's `inserted_at`, the notifier's cursor.
    pub transition_inserted_at: i64,
    /// transition, renotify, downtime_end or escalation.
    pub kind: String,
    /// The nth renotify or escalation.
    pub seq: u32,
    pub channel: String,
    /// stub, sent, muted or failed.
    pub status: String,
    pub attempt: u32,
    pub handled_at: i64,
}

/// A point of `metrics`, as self-monitoring writes one. The lifted tag columns and `series_id`
/// are ClickHouse's own (MATERIALIZED).
#[derive(clickhouse::Row, Serialize, Clone, Debug, PartialEq)]
pub struct MetricPoint {
    pub tenant_id: String,
    pub timestamp: i64,
    pub metric: String,
    pub host: String,
    /// GAUGE, COUNT, RATE or UNSPECIFIED.
    pub metric_type: String,
    pub source_type: String,
    pub unit: String,
    pub interval: u32,
    pub value: f64,
    pub tags: BTreeMap<String, Vec<String>>,
    pub origin_product: u32,
    pub origin_category: u32,
    pub origin_service: u32,
    pub resources: BTreeMap<String, String>,
    pub extra: BTreeMap<String, String>,
}
