use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::{Extra, Tags};
use crate::column;
use crate::table::{Limits, Table};

#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct MetricPoint {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub metric: String,
    pub host: String,
    /// GAUGE, COUNT, RATE or UNSPECIFIED.
    pub metric_type: String,
    pub source_type: String,
    pub unit: String,
    pub interval: u32,
    pub value: f64,
    pub tags: Tags,
    /// Datadog's provenance of the metric: which product and integration.
    pub origin_product: u32,
    pub origin_category: u32,
    pub origin_service: u32,
    /// Resources other than the host, by type ("device" → "eth0").
    pub resources: BTreeMap<String, String>,
    /// Keys of the payload with no column, JSON-encoded.
    pub extra: Extra,
}

impl MetricPoint {
    /// A point with only what every point has; the rest is empty.
    pub fn new(tenant: &str, timestamp: DateTime<Utc>, metric: &str, value: f64) -> Self {
        MetricPoint { tenant_id: tenant.into(), timestamp, metric: metric.into(), value, ..Default::default() }
    }
}

impl Table for MetricPoint {
    const WRITER: &'static str = "storage_metrics";
    const TABLE: &'static str = "metrics";
    // One insert is a part here and one in each table the views fill (metrics_1m,
    // metric_tag_keys). A buffered point with its tags takes a few KB, and while ClickHouse
    // stalls four batches are in flight beside the buffer, so the limit is what keeps the
    // process inside its memory.
    const LIMITS: Limits =
        Limits { max_rows: 50_000, flush_interval: Duration::from_secs(10), buffer_limit: 150_000, ..Limits::DEFAULT };
}

#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct SketchRow {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub metric: String,
    pub host: String,
    pub tags: Tags,
    pub count: u64,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub sum: f64,
    pub bucket_keys: Vec<i32>,
    pub bucket_counts: Vec<u32>,
    pub origin_product: u32,
    pub origin_category: u32,
    pub origin_service: u32,
    pub resources: BTreeMap<String, String>,
    pub extra: Extra,
}

impl Table for SketchRow {
    const WRITER: &'static str = "storage_sketches";
    const TABLE: &'static str = "sketches";
    const LIMITS: Limits = Limits { max_rows: 1000, ..Limits::DEFAULT };
}
