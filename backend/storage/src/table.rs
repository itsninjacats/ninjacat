use std::time::Duration;

use serde::Serialize;

/// How a table's one writer batches.
#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Flush once this many rows are buffered…
    pub max_rows: usize,
    /// …or this much time has passed.
    pub flush_interval: Duration,
    /// Rows beyond this are dropped while ClickHouse is unreachable: losing data beats running
    /// out of memory.
    pub buffer_limit: usize,
    /// Concurrent flushes, so a slow database does not multiply connections.
    pub max_in_flight: usize,
}

impl Limits {
    pub const DEFAULT: Limits =
        Limits { max_rows: 10_000, flush_interval: Duration::from_secs(5), buffer_limit: 200_000, max_in_flight: 4 };
}

/// A row of one ClickHouse table. Its fields are the columns it inserts, named as the columns
/// are; the rest of the table's columns are left to their defaults.
pub trait Table: clickhouse::RowOwned + clickhouse::RowWrite + Serialize + Send + Sync {
    /// The name the sink keeps the table's one writer under ("storage_logs"); the golden
    /// fixtures name a write by it.
    const WRITER: &'static str;
    const TABLE: &'static str;
    const LIMITS: Limits = Limits::DEFAULT;
}
