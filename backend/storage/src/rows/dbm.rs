use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::Tags;
use crate::column;
use crate::table::{Limits, Table};

/// One Database Monitoring event. All six tracks and every database engine share the table: the
/// engine-specific row arrays (oracle_rows, postgres_activity, …) have no published schema, so the
/// fields every track agrees on are columns and the complete event is kept beside them.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct DbmEvent {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    /// The route the event arrived on: dbmmetrics, dbmactivity, databasequery, dbmmetadata,
    /// dbmhealth or dbmcolumnstatistics.
    pub track: String,
    /// The event's own timestamp; None when it sent none, never 1970.
    #[serde(with = "column::millis::option")]
    pub timestamp: Option<DateTime<Utc>>,
    /// The database host, not the agent's.
    pub host: String,
    pub database_instance: String,
    pub agent_hostname: String,
    pub agent_version: String,
    pub source: String,
    pub dbm_type: String,
    pub kind: String,
    pub dbms: String,
    pub dbms_version: String,
    /// None when not reported: 0 is a value an integration could send.
    pub collection_interval: Option<f64>,
    pub tags: Tags,
    /// The keys with no column, sorted: the engine's row arrays live here.
    pub extra_keys: Vec<String>,
    /// Envelope keys whose value had an unexpected shape.
    pub undecoded_keys: Vec<String>,
    /// The complete event exactly as received.
    pub event: String,
    /// From the "db" object of a databasequery event; None elsewhere.
    pub db_instance: Option<String>,
    pub query_signature: Option<String>,
    pub statement: Option<String>,
    pub plan_signature: Option<String>,
    pub plan_definition_steps: Option<u32>,
}

impl Table for DbmEvent {
    const WRITER: &'static str = "storage_dbm_events";
    const TABLE: &'static str = "dbm_events";
    // A row carries a whole event, which can hold hundreds of sessions.
    const LIMITS: Limits =
        Limits { max_rows: 500, flush_interval: Duration::from_secs(5), buffer_limit: 20_000, max_in_flight: 2 };
}
