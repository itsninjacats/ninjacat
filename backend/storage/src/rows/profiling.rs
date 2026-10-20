use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::column;
use crate::table::{Limits, Table};

/// One "which of these build ids do you hold?" from the profiler's symbol uploader. build_ids
/// keeps its order: the reply is positional against it.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SymbolQuery {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub at: DateTime<Utc>,
    pub arch: String,
    pub build_ids: Vec<String>,
    pub resource: String,
}

impl Table for SymbolQuery {
    const WRITER: &'static str = "storage_symbol_queries";
    const TABLE: &'static str = "symbol_queries";
    const LIMITS: Limits =
        Limits { max_rows: 50, flush_interval: Duration::from_secs(15), buffer_limit: 1_000, max_in_flight: 1 };
}
