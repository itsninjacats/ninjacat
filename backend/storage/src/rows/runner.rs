//! The Private Action Runner: its enrolment, heartbeats and task traffic.

use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::{Extra, Tags};
use crate::column;
use crate::table::{Limits, Table};

/// One Private Action Runner identity. The table is a ReplacingMergeTree on (tenant_id,
/// runner_id).
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RunnerEnrollment {
    pub tenant_id: String,
    pub runner_id: String,
    #[serde(with = "column::millis")]
    pub enrolled_at: DateTime<Utc>,
    pub name: String,
    pub modes: Vec<String>,
    pub host: String,
    /// The whole PEM block: without it no signature of the runner's can ever be verified.
    pub public_key_pem: String,
    pub agent_hostname: String,
    pub orch_cluster_id: String,
    pub agent_flavor: String,
    /// What we answered with. The derivation from the tenant is ours, so only this row remembers
    /// it if the derivation changes.
    pub org_id: i64,
    /// The request's attributes as JSON, for a native JSON column.
    pub attributes: String,
}

impl Table for RunnerEnrollment {
    const WRITER: &'static str = "storage_runner_enrollments";
    const TABLE: &'static str = "runner_enrollments";
    const LIMITS: Limits = Limits { max_rows: 100, flush_interval: Duration::from_secs(10), ..Limits::DEFAULT };
}

/// The outcome of one action. outcome is "succeed_task" or "fail_task".
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RunnerTaskUpdate {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub at: DateTime<Utc>,
    pub outcome: String,
    pub task_id: String,
    pub action_fqn: String,
    pub job_id: String,
    pub client: String,
    pub branch: String,
    pub outputs: String,
    /// A numeric enum on the runner's side; a succeeded task sends none.
    pub error_code: Option<i64>,
    pub error_details: String,
    pub api_error: String,
    /// Undeclared attributes, values JSON-encoded; "payload."-prefixed when they came from the
    /// nested object.
    pub extra: Extra,
}

impl Table for RunnerTaskUpdate {
    const WRITER: &'static str = "storage_runner_task_updates";
    const TABLE: &'static str = "runner_task_updates";
    const LIMITS: Limits = Limits { max_rows: 200, ..Limits::DEFAULT };
}

#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RunnerHeartbeat {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub at: DateTime<Utc>,
    pub task_id: String,
    pub action_fqn: String,
    pub job_id: String,
    pub client: String,
}

impl Table for RunnerHeartbeat {
    const WRITER: &'static str = "storage_runner_heartbeats";
    const TABLE: &'static str = "runner_heartbeats";
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}

/// One idle poll for work: the only liveness signal of a runner with no tasks. Both timestamps
/// stay strings: their format is undocumented.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct RunnerDequeue {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub at: DateTime<Utc>,
    pub runner_started_at: String,
    pub last_task_received_at: String,
    pub version: String,
    pub modes: String,
}

impl Table for RunnerDequeue {
    const WRITER: &'static str = "storage_runner_dequeues";
    const TABLE: &'static str = "runner_dequeues";
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}

/// A connection an action runner may use to reach a third-party system. SENSITIVE: credentials
/// holds that integration's secrets verbatim.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct ActionConnection {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub at: DateTime<Utc>,
    pub name: String,
    pub runner_id: String,
    pub tags: Tags,
    pub integration_type: String,
    pub credentials: String,
    pub extra: Extra,
}

impl Table for ActionConnection {
    const WRITER: &'static str = "storage_action_connections";
    const TABLE: &'static str = "action_connections";
    const LIMITS: Limits =
        Limits { max_rows: 50, flush_interval: Duration::from_secs(15), buffer_limit: 1_000, max_in_flight: 1 };
}
