//! Sensitive Data Scanner results: one payload is one scan of one resource, with a result per
//! location scanned and a match per rule that hit. Reasoning per column: the sds_* tables in
//! 0001_initial.sql.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use crate::column;
use crate::table::Table;

/// The payload itself: who scanned what, with which rules.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SdsScan {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub scan_id: Uuid,
    /// The scanner's clock; None when it sent 0.
    #[serde(with = "column::millis::option")]
    pub timestamp: Option<DateTime<Utc>>,
    pub resource_type: String,
    pub resource_name: String,
    /// "agent", "agentless", "datadog_crawler", or "" when not sent.
    pub scanning_source: String,
    pub source_version: String,
    pub source_region: String,
    pub source_hostname: String,
    pub source_service_name: String,
    pub scanner_version: String,
    pub scanner_region: String,
    pub rule_ids: Vec<String>,
    /// The deprecated rules map, as JSON; "" when empty.
    pub rules: String,
    /// None when the payload carried no statistics.
    pub scan_duration_ms: Option<i64>,
    pub total_files_found: Option<i64>,
    pub files_scanned: Option<i64>,
    pub files_skipped_unsupported_type: Option<i64>,
    pub files_partially_scanned_size_limit: Option<i64>,
    pub total_data_scanned_bytes: Option<i64>,
    pub skipped_files_by_type: BTreeMap<String, i64>,
    pub result_count: u32,
}

impl Table for SdsScan {
    const TABLE: &'static str = "sds_scans";
}

/// Where a result was found: copied onto the result and onto its matches, as the five location
/// columns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SdsLocation {
    /// "postgres_table", "rds_table", "snowflake_table", "s3_file", or "path" / "database" from the
    /// deprecated fields; "" when not sent.
    pub kind: String,
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub path: String,
}

/// One location that was scanned, with or without matches.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SdsResult {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub scan_id: Uuid,
    pub result_index: u32,
    pub resource_type: String,
    pub resource_name: String,
    pub location_kind: String,
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub path: String,
    pub table_row_count: Option<i64>,
    pub scanned_row_count: Option<i64>,
    pub scanned_column_names: Vec<String>,
    pub scanned_column_types: Vec<String>,
    /// The whole location as protobuf's JSON: each kind has fields of its own that have no
    /// column.
    pub location: String,
    pub duration: i64,
    pub task_id: String,
    pub sub_task_id: String,
    #[serde(with = "column::millis::option")]
    pub task_started_at: Option<DateTime<Utc>>,
    #[serde(with = "column::millis::option")]
    pub task_ended_at: Option<DateTime<Utc>>,
    /// The enum's name; "" when the result carried no task metadata.
    pub task_status: String,
    pub failure_reason: String,
    pub match_count: u32,
    pub table_match_count: u32,
}

impl Table for SdsResult {
    const TABLE: &'static str = "sds_results";
}

/// One rule that matched in one location: a match in text (a file), or a count of matching rows
/// in a column (a table).
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SdsMatch {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub scan_id: Uuid,
    pub result_index: u32,
    pub resource_type: String,
    pub resource_name: String,
    pub location_kind: String,
    pub database_name: String,
    pub schema_name: String,
    pub table_name: String,
    pub path: String,
    /// "match" or "table_match".
    pub kind: String,
    pub rule_id: String,
    // A match in text.
    pub sample: String,
    pub start_index: i64,
    pub end_index: i64,
    pub match_path: Option<String>,
    pub line: Option<i64>,
    pub column: Option<i64>,
    pub start_line: Option<i64>,
    pub end_line: Option<i64>,
    pub row: Option<i64>,
    pub start_index_in_line: Option<i64>,
    pub end_index_in_line: Option<i64>,
    pub match_status: Option<String>,
    // A match in a table.
    pub column_name: String,
    pub count_matched_rows: i64,
    pub count_total_rows: i64,
    pub count_matches: i64,
}

impl Table for SdsMatch {
    const TABLE: &'static str = "sds_matches";
}
