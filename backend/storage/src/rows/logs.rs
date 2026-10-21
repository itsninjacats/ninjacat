use chrono::{DateTime, Utc};
use serde::Serialize;

use super::Tags;
use crate::column;
use crate::table::{Limits, Table};

#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct Log {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    pub service: String,
    pub source: String,
    pub status: String,
    pub message: String,
    pub tags: Tags,
    /// Everything that did not become a column, as the text of a JSON object.
    pub attributes: String,
    /// Which wire field the timestamp came from, or "arrival".
    pub timestamp_source: String,
}

impl Table for Log {
    const WRITER: &'static str = "storage_logs";
    const TABLE: &'static str = "logs";
    const LIMITS: Limits = Limits { max_rows: 20_000, buffer_limit: 500_000, ..Limits::DEFAULT };
}
