use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::Tags;
use crate::column;
use crate::table::Table;

/// One ECS task in one collection pass. It arrives on the orchestrator intake with the Kubernetes
/// collectors but has a table of its own: a task has no namespace, name or uid, the columns
/// k8s_resources sorts by.
///
/// Four tag sets, kept apart: the frame envelope's, and the three ECS itself keeps separate.
/// Merging them would answer "is this env:prod" while destroying "who said so".
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct EcsTask {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub collected_at: DateTime<Utc>,
    // The frame header, as on every orchestrator row.
    pub org_id: i32,
    pub subscription_id: u8,
    pub header_timestamp: Option<i64>,
    pub encoding: String,
    // The envelope.
    pub aws_account_id: i64,
    pub cluster_id: String,
    pub cluster_name: String,
    pub region: String,
    pub group_id: i32,
    pub group_size: i32,
    pub host_name: String,
    pub agent_version: String,
    pub envelope_tags: Tags,
    // The task.
    pub arn: String,
    pub resource_version: String,
    pub launch_type: String,
    pub desired_status: String,
    pub known_status: String,
    pub family: String,
    pub version: String,
    pub availability_zone: String,
    pub service_name: String,
    pub vpc_id: String,
    pub container_instance_arn: String,
    pub daemon_name: String,
    pub task_host_name: String,
    #[serde(with = "column::float_map")]
    pub limits: BTreeMap<String, f64>,
    pub ephemeral_storage_metrics: BTreeMap<String, i64>,
    #[serde(with = "column::secs::option")]
    pub pull_started_at: Option<DateTime<Utc>>,
    #[serde(with = "column::secs::option")]
    pub pull_stopped_at: Option<DateTime<Utc>>,
    #[serde(with = "column::secs::option")]
    pub execution_stopped_at: Option<DateTime<Utc>>,
    /// JSON: a container nests ports, networks, volumes, health and log options, too deep for
    /// parallel arrays.
    pub containers: String,
    pub container_count: u32,
    pub tags: Tags,
    pub ecs_tags: Tags,
    /// These belong to the machine the task runs on, not to the workload.
    pub container_instance_tags: Tags,
    /// The frame's message as JSON, without its task list.
    pub envelope: String,
    /// The task's Host message as JSON; task_host_name is only its name.
    pub task_host: String,
}

impl Table for EcsTask {
    const TABLE: &'static str = "ecs_tasks";
}
