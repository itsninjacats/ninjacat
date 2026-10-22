//! CI Visibility, CI provider webhooks, git metadata and the synthetics test catalogue. One
//! product, but separate tables: each is a different wire format.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::{Extra, Tags};
use crate::column::{self, Blob};
use crate::table::Table;

/// One event of a citestcycle payload: a test, or the end of a suite, module or session.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct CiTestEvent {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub event_type: String,
    pub event_version: i32,
    pub payload_version: i32,
    /// Zero when the event type carries no such id: that is the tracer's own spelling of "none".
    pub session_id: u64,
    pub module_id: u64,
    pub suite_id: u64,
    pub trace_id: u64,
    pub span_id: u64,
    pub parent_id: u64,
    pub itr_correlation_id: String,
    pub service: String,
    pub env: String,
    pub name: String,
    pub resource: String,
    pub span_type: String,
    #[serde(with = "column::nanos")]
    pub start: i64,
    pub duration_ns: i64,
    pub error: i32,
    pub test_name: String,
    pub test_suite: String,
    pub test_module: String,
    pub test_framework: String,
    pub test_framework_version: String,
    pub test_status: String,
    pub test_type: String,
    pub test_source_file: String,
    pub test_source_start: Option<i64>,
    pub test_source_end: Option<i64>,
    pub test_parameters: String,
    pub test_codeowners: String,
    pub test_command: String,
    pub test_session_name: String,
    pub git_repository_url: String,
    pub git_branch: String,
    pub git_tag: String,
    pub git_commit_sha: String,
    pub git_commit_message: String,
    pub git_commit_author_name: String,
    pub git_commit_author_email: String,
    pub git_commit_author_date: String,
    pub git_commit_committer_name: String,
    pub git_commit_committer_email: String,
    pub git_commit_committer_date: String,
    pub ci_provider_name: String,
    pub ci_pipeline_id: String,
    pub ci_pipeline_name: String,
    pub ci_pipeline_number: String,
    pub ci_pipeline_url: String,
    pub ci_job_id: String,
    pub ci_job_name: String,
    pub ci_job_url: String,
    pub ci_stage_name: String,
    pub ci_workspace_path: String,
    pub ci_node_name: String,
    pub ci_node_labels: String,
    pub os_platform: String,
    pub os_version: String,
    pub os_architecture: String,
    pub runtime_name: String,
    pub runtime_version: String,
    pub language: String,
    pub runtime_id: String,
    pub library_version: String,
    /// The flags are optional because absent and false differ: absent means the tracer has the
    /// feature off, false that it is on and said no.
    pub test_is_new: Option<u8>,
    pub test_is_retry: Option<u8>,
    pub test_is_modified: Option<u8>,
    pub test_skipped_by_itr: Option<u8>,
    pub itr_unskippable: Option<u8>,
    pub itr_forced_run: Option<u8>,
    pub code_coverage_enabled: Option<u8>,
    pub test_retry_reason: String,
    pub early_flake_abort_reason: String,
    pub evp_subdomain: String,
    pub container_id: String,
    pub agent_hostname: String,
    pub agent_version: String,
    pub meta: BTreeMap<String, String>,
    #[serde(with = "column::float_map")]
    pub metrics: BTreeMap<String, f64>,
    pub metadata: BTreeMap<String, String>,
    /// The event's content as JSON, whatever format the request was in.
    pub content: String,
}

impl Table for CiTestEvent {
    const TABLE: &'static str = "ci_test_events";
}

/// One entry of a citestcov payload's coverages[].
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct CiCoverage {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub payload_version: i32,
    pub session_id: u64,
    pub suite_id: u64,
    /// Optional because the encoder leaves it out in suite-skipping mode, and 0 is a legal span
    /// id.
    pub span_id: Option<u64>,
    pub files_filename: Vec<String>,
    /// Parallel to files_filename: a file without line data keeps an empty bitmap at its index.
    /// The bitmaps are opaque bytes.
    pub files_bitmap: Vec<Blob>,
    /// This entry's bytes as they arrived; raw_format is "msgpack" or "json".
    pub raw: Blob,
    pub raw_format: String,
    pub event: Blob,
    pub extra: Extra,
    pub evp_subdomain: String,
    pub container_id: String,
    pub agent_hostname: String,
    pub agent_version: String,
}

impl Table for CiCoverage {
    const TABLE: &'static str = "ci_coverage";
}

/// A sha that was mentioned for a repository. Not a claim that its objects are held: most rows
/// come from a tracer asking what we have. packfile_id is set only when the sha arrived with a
/// packfile.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct GitCommit {
    pub tenant_id: String,
    pub repository_url: String,
    pub sha: String,
    #[serde(with = "column::millis")]
    pub seen_at: DateTime<Utc>,
    pub packfile_id: Option<String>,
    pub source: String,
}

impl Table for GitCommit {
    const TABLE: &'static str = "git_commits";
}

/// One .pack file upload.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct GitPackfile {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub repository_url: String,
    pub pushed_sha: String,
    /// SHA-256 of the pack, hex: the upload has no id of its own, and git_commits.packfile_id has
    /// to point at something.
    pub packfile_id: String,
    pub filename: String,
    pub packfile: Blob,
    pub size_bytes: u64,
    pub other_parts: BTreeMap<String, Blob>,
}

impl Table for GitPackfile {
    const TABLE: &'static str = "git_packfiles";
}

/// One question a tracer asked about its configuration, and the answer it got. The answer is kept
/// because it is a contract of ours: the day it changes, what a tracer was told then is what
/// explains its behaviour.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct CiSettingsRequest {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub endpoint: String,
    pub request_id: String,
    pub request_type: String,
    pub service: String,
    pub env: String,
    pub repository_url: String,
    pub branch: String,
    pub sha: String,
    pub test_level: String,
    pub module: String,
    pub commit_message: String,
    pub page_state: String,
    pub configurations: String,
    pub request_body: Blob,
    pub response_body: String,
}

impl Table for CiSettingsRequest {
    const TABLE: &'static str = "ci_settings_requests";
}

/// One element of a CI webhook batch: a pipeline, a stage or a job, told apart by level.
///
/// The ids are strings although Jenkins sends numbers: other providers put their own identifiers
/// in the same fields, and some are UUIDs.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct CiWebhookEvent {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub provider: String,
    pub level: String,
    pub service: String,
    pub payload_version: Option<i64>,
    pub partial_retry: Option<u8>,
    pub is_manual: Option<u8>,
    pub trace_id: String,
    pub span_id: String,
    pub parent_span_id: String,
    pub id: String,
    pub unique_id: String,
    pub pipeline_id: String,
    pub pipeline_unique_id: String,
    pub pipeline_name: String,
    pub stage_id: String,
    pub stage_name: String,
    pub parent_stage_id: String,
    pub name: String,
    pub url: String,
    pub status: String,
    pub start_raw: String,
    #[serde(with = "column::millis::option")]
    pub start_parsed: Option<DateTime<Utc>>,
    pub end_raw: String,
    #[serde(with = "column::millis::option")]
    pub end_parsed: Option<DateTime<Utc>>,
    pub queue_time_ms: Option<i64>,
    pub node_name: String,
    pub node_hostname: String,
    pub node_workspace: String,
    pub node_labels: Vec<String>,
    pub git_repository_url: String,
    pub git_default_branch: String,
    pub git_branch: String,
    pub git_sha: String,
    pub git_tag: String,
    pub git_message: String,
    pub git_author_name: String,
    pub git_author_email: String,
    pub git_author_time: String,
    pub git_committer_name: String,
    pub git_committer_email: String,
    pub git_commit_time: String,
    pub user_name: String,
    pub user_email: String,
    pub error_message: String,
    pub error_type: String,
    pub error_domain: String,
    pub error_stack: String,
    pub parent_pipeline_trace_id: String,
    pub parent_pipeline_url: String,
    pub parameters: BTreeMap<String, String>,
    /// A tag multiset: the Jenkins plugin repeats keys.
    pub tags: Tags,
    pub delivery_id: String,
    pub headers: BTreeMap<String, String>,
    pub extra: Extra,
    pub body: String,
}

impl Table for CiWebhookEvent {
    const TABLE: &'static str = "ci_webhook_events";
}

/// One datadog-ci tag / measure / custom-span call.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct CiPipelineEvent {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub kind: String,
    pub data_type: String,
    pub provider: String,
    pub ci_level: Option<i64>,
    pub ci_env: BTreeMap<String, String>,
    /// A plain map: on this wire tags are a JSON object, so keys are unique.
    pub tags: BTreeMap<String, String>,
    #[serde(with = "column::float_map")]
    pub metrics: BTreeMap<String, f64>,
    pub span_name: String,
    pub span_start_raw: String,
    pub span_end_raw: String,
    pub attributes: String,
    pub body: Blob,
    pub extra: Extra,
}

impl Table for CiPipelineEvent {
    const TABLE: &'static str = "ci_pipeline_events";
}

/// One synthetic test the agent's poller may be handed. Nothing writes these yet: the table is
/// the catalogue a future panel fills.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SyntheticsTestConfig {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub updated_at: DateTime<Utc>,
    pub public_id: String,
    pub version: i32,
    pub r#type: String,
    /// Only "UDP", "TCP" and "ICMP" are valid: the agent fails its whole poll on a subtype it
    /// does not know.
    pub subtype: String,
    pub tick_every: i32,
    pub org_id: i64,
    pub main_dc: String,
    pub result_id: String,
    pub run_type: String,
    pub assertions: String,
    pub request: String,
    pub enabled: u8,
}

impl Table for SyntheticsTestConfig {
    const TABLE: &'static str = "synthetics_test_configs";
}
