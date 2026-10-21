use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{Extra, Tags};
use crate::column::{self, Blob};
use crate::table::{Limits, Table};

/// One AgentDiscoveryPayload. The batch's host id repeats onto every row.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct AgentDiscovery {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub host_id: String,
    pub integration: String,
    pub runtime: String,
    pub runtime_id: String,
    #[serde(with = "column::millis::option")]
    pub ingestion_timestamp: Option<DateTime<Utc>>,
    /// The four config arrays are index-aligned.
    pub config_paths: Vec<String>,
    /// `bytes` on the wire: a config file is not always UTF-8.
    pub config_contents: Vec<Blob>,
    pub config_truncated: Vec<u8>,
    pub config_formats: Vec<String>,
    /// Names and VALUES, index-aligned. Arrays, not a map: env vars are a repeated field, so a
    /// name can occur twice and the order is the agent's.
    pub env_var_names: Vec<String>,
    pub env_var_values: Vec<String>,
}

impl Table for AgentDiscovery {
    const WRITER: &'static str = "storage_agent_discovery";
    const TABLE: &'static str = "agent_discovery";
    // A row can carry whole config files.
    const LIMITS: Limits =
        Limits { max_rows: 200, flush_interval: Duration::from_secs(10), buffer_limit: 5_000, max_in_flight: 1 };
}

/// One HealthReport. The wire has no report id; report_id is minted at ingest and joins the
/// report to its issues.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct AgentHealthReport {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub report_id: Uuid,
    pub schema_version: String,
    pub event_type: String,
    pub emitted_at: String,
    #[serde(with = "column::millis::option")]
    pub emitted_at_parsed: Option<DateTime<Utc>>,
    pub service: String,
    pub host: String,
    pub agent_version: Option<String>,
    pub par_ids: Vec<String>,
    pub issue_count: u32,
}

impl Table for AgentHealthReport {
    const WRITER: &'static str = "storage_agent_health_reports";
    const TABLE: &'static str = "agent_health_reports";
    const LIMITS: Limits = Limits { max_rows: 200, ..Limits::DEFAULT };
}

/// One entry of a HealthReport's issues map.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct AgentHealthIssue {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub report_id: Uuid,
    pub issue_key: String,
    pub id: String,
    pub issue_name: String,
    pub title: String,
    pub description: String,
    pub category: String,
    pub location: String,
    /// The enum's name.
    pub severity: String,
    pub detected_at: String,
    #[serde(with = "column::millis::option")]
    pub detected_at_parsed: Option<DateTime<Utc>>,
    pub source: String,
    /// Schemaless per issue, as JSON text.
    pub extra: String,
    pub remediation_summary: String,
    pub remediation_step_order: Vec<i32>,
    pub remediation_step_text: Vec<String>,
    pub script_language: String,
    pub script_language_version: String,
    pub script_filename: String,
    /// None when there is no script at all, which is not the same as a script that needs no root.
    pub script_requires_root: Option<u8>,
    pub script_content: String,
    pub tags: Tags,
    /// The enum's name.
    pub persisted_state: String,
    pub first_seen: String,
    pub last_seen: String,
    pub resolved_at: Option<String>,
    pub issue_type: String,
}

impl Table for AgentHealthIssue {
    const WRITER: &'static str = "storage_agent_health_issues";
    const TABLE: &'static str = "agent_health_issues";
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}

/// One envelope from /api/v2/events.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct EventManagementEvent {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub data_type: String,
    pub host: String,
    pub title: String,
    pub category: String,
    pub integration_id: String,
    pub message: String,
    pub timestamp: String,
    #[serde(with = "column::millis::option")]
    pub timestamp_parsed: Option<DateTime<Utc>>,
    pub tags: Tags,
    pub aggregation_key: String,
    pub notable_event_type: String,
    /// The producer's own data.attributes.attributes object, as JSON text.
    pub attributes: String,
    /// Keys with no column, JSON per key.
    pub outer_extra: Extra,
}

impl Table for EventManagementEvent {
    const WRITER: &'static str = "storage_event_management_events";
    const TABLE: &'static str = "event_management_events";
    const LIMITS: Limits = Limits { max_rows: 200, ..Limits::DEFAULT };
}

/// One software entry of a softinv payload: a row per package, so "every host running curl < 8.0"
/// is a plain query.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct HostSoftware {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub hostname: String,
    pub software_type: String,
    pub name: String,
    pub version: String,
    pub publisher: String,
    pub deployment_status: String,
    pub deployment_time: String,
    #[serde(with = "column::millis::option")]
    pub deployment_time_parsed: Option<DateTime<Utc>>,
    pub product_code: String,
    /// None when the agent never determined it; false would claim 32-bit.
    pub is_64_bit: Option<u8>,
    pub install_paths: Vec<String>,
    /// Keys of the entry with no column, JSON per key.
    pub extra: Extra,
    /// Keys of the payload with no column, repeated onto each of its rows.
    pub payload_extra: Extra,
}

impl Table for HostSoftware {
    const WRITER: &'static str = "storage_host_software";
    const TABLE: &'static str = "host_software";
    // A full inventory is a few hundred rows per host.
    const LIMITS: Limits =
        Limits { max_rows: 10_000, flush_interval: Duration::from_secs(5), buffer_limit: 100_000, max_in_flight: 2 };
}

/// One synthetic test result.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SyntheticsResult {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub test_id: String,
    pub test_name: String,
    pub test_type: String,
    pub test_subtype: String,
    pub test_version: String,
    pub location_id: String,
    pub location_name: String,
    pub location_display_name: String,
    pub result_id: String,
    pub result_initial_id: String,
    pub status: String,
    pub run_type: String,
    /// The wire type is undocumented; kept as sent.
    pub duration: String,
    pub test_started_at: String,
    #[serde(with = "column::millis::option")]
    pub test_started_at_parsed: Option<DateTime<Utc>>,
    pub test_finished_at: String,
    #[serde(with = "column::millis::option")]
    pub test_finished_at_parsed: Option<DateTime<Utc>>,
    pub test_triggered_at: String,
    #[serde(with = "column::millis::option")]
    pub test_triggered_at_parsed: Option<DateTime<Utc>>,
    /// The five assertion arrays are index-aligned.
    pub assertion_type: Vec<String>,
    pub assertion_operator: Vec<String>,
    pub assertion_expected: Vec<String>,
    pub assertion_actual: Vec<String>,
    pub assertion_valid: Vec<u8>,
    /// Both None when the result has no failure object.
    pub failure_code: Option<String>,
    pub failure_message: Option<String>,
    pub config: String,
    pub netstats_packets_sent: Option<i64>,
    pub netstats_packets_received: Option<i64>,
    pub netstats_packet_loss_percentage: Option<f64>,
    pub netstats_jitter: Option<f64>,
    pub netstats_latency: Option<f64>,
    pub netstats_hops: Option<i64>,
    pub netpath: String,
    pub dd: String,
    pub enrichment: String,
    pub v: String,
    pub extra: Extra,
}

impl Table for SyntheticsResult {
    const WRITER: &'static str = "storage_synthetics_results";
    const TABLE: &'static str = "synthetics_results";
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}

/// One OpenLineage RunEvent. Facets are the spec's own extension point, so they stay JSON text.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct OpenLineageEvent {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub event_type: String,
    pub event_time: String,
    #[serde(with = "column::millis::option")]
    pub event_time_parsed: Option<DateTime<Utc>>,
    pub producer: String,
    pub schema_url: String,
    pub run_id: String,
    pub run_facets: String,
    pub job_namespace: String,
    pub job_name: String,
    pub job_facets: String,
    /// Index-aligned per list; inputs and outputs are independent.
    pub input_namespace: Vec<String>,
    pub input_name: Vec<String>,
    pub input_facets: Vec<String>,
    pub output_namespace: Vec<String>,
    pub output_name: Vec<String>,
    pub output_facets: Vec<String>,
    pub api_version: String,
    pub via: String,
    pub extra: Extra,
}

impl Table for OpenLineageEvent {
    const WRITER: &'static str = "storage_openlineage_events";
    const TABLE: &'static str = "openlineage_events";
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}

/// One entry of a query-actions batch. Nothing upstream names its fields, so the entry is kept
/// whole and `keys` lists its top-level names.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct QueryActionResult {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub result: String,
    pub keys: Vec<String>,
    pub dd_evp_origin: String,
    pub dd_evp_origin_version: String,
}

impl Table for QueryActionResult {
    const WRITER: &'static str = "storage_query_action_results";
    const TABLE: &'static str = "query_action_results";
    const LIMITS: Limits = Limits { max_rows: 200, ..Limits::DEFAULT };
}
