use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{Extra, Tags};
use crate::column;
use crate::table::Table;

/// One event: a deploy, a restart, a config change.
///
/// Two sources feed the table: the public /api/v1/events request and the agent's own events on
/// /intake/. They fill different subsets of it, and one table spares every chart a UNION.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct Event {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub event_id: Uuid,
    /// The numeric id the API handed back to the caller, who keeps it to build links.
    pub event_id_num: u64,
    pub title: String,
    pub text: String,
    pub host: String,
    pub alert_type: String,
    pub priority: String,
    pub aggregation_key: String,
    pub source_type_name: String,
    pub device_name: String,
    pub tags: Tags,
    /// Only the /intake/ source sends it.
    pub event_type: String,
    /// Datadog's ids start at 1, so "no parent" must not be stored as 0.
    pub related_event_id: Option<i64>,
    /// What the sender wrote, before the coercion that fills alert_type and priority; "" when
    /// nothing was sent.
    pub alert_type_raw: String,
    pub priority_raw: String,
    pub extra: Extra,
}

impl Table for Event {
    const TABLE: &'static str = "events";
}
