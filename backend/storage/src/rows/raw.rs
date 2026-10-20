use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::column::{self, Blob};
use crate::table::{Limits, Table};

/// A request that could not be turned into rows, kept as it arrived.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RawPayload {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub intake: String,
    pub reason: String,
    pub method: String,
    pub host: String,
    pub path: String,
    pub query: BTreeMap<String, Vec<String>>,
    pub content_type: String,
    pub content_encoding: String,
    pub headers: BTreeMap<String, String>,
    pub body: Blob,
    pub body_bytes: u64,
    pub note: String,
}

impl Table for RawPayload {
    const WRITER: &'static str = "storage_raw_payloads";
    const TABLE: &'static str = "raw_payloads";
    const LIMITS: Limits = Limits {
        max_rows: 200,
        flush_interval: std::time::Duration::from_secs(10),
        buffer_limit: 5_000,
        max_in_flight: 1,
    };
}
