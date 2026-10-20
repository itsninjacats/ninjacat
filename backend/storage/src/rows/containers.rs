use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::Tags;
use crate::column;
use crate::table::{Limits, Table};

/// One container, pod or task lifecycle event: the restart and OOM history.
///
/// An Option wherever the wire has a proto3 optional: exit 0 and "no code reported" are different
/// facts, and a missing time must not become 1970.
///
/// A state transition is stored twice on purpose: old_state/new_state are the label a dashboard
/// shows, the structured fields beside them are what "every OOM kill with signal 9" needs.
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct ContainerEvent {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    pub cluster_id: String,
    pub object_kind: String,
    pub event_type: String,
    pub container_id: String,
    pub container_name: String,
    pub pod_uid: String,
    pub task_arn: String,
    pub source: String,
    pub exit_code: Option<i32>,
    #[serde(with = "column::millis::option")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(with = "column::millis::option")]
    pub exited_at: Option<DateTime<Utc>>,
    pub owner_type: String,
    pub owner_uid: String,
    pub old_state: String,
    pub new_state: String,
    #[serde(with = "column::millis::option")]
    pub transition_at: Option<DateTime<Utc>>,
    /// The schema version of the batch, not of an event in it.
    pub payload_version: String,
    /// "container", "pod", "task", or "unknown" for an event with no typed detail — which
    /// otherwise looks like a typed event with empty fields.
    pub event_variant: String,
    pub container_kind: String,
    pub precision: String,
    pub missed_intermediate: String,
    pub old_state_kind: String,
    pub old_reason: Option<String>,
    pub old_exit_code: Option<i32>,
    pub old_signal: Option<i32>,
    pub new_state_kind: String,
    pub new_reason: Option<String>,
    pub new_exit_code: Option<i32>,
    pub new_signal: Option<i32>,
    /// Which pod status field moved. Its value is a phase or a condition; *_state_variant says
    /// which, and each has its own columns.
    pub pod_status_field: String,
    pub old_state_variant: String,
    pub old_phase: Option<String>,
    pub old_condition_type: Option<String>,
    pub old_condition_status: Option<String>,
    pub old_condition_reason: Option<String>,
    pub old_condition_message: Option<String>,
    pub new_state_variant: String,
    pub new_phase: Option<String>,
    pub new_condition_type: Option<String>,
    pub new_condition_status: Option<String>,
    pub new_condition_reason: Option<String>,
    pub new_condition_message: Option<String>,
    /// 1 when the sender set container_name at all. The name's own column cannot become Nullable
    /// without rewriting every stored row.
    pub container_name_present: u8,
}

impl Table for ContainerEvent {
    const WRITER: &'static str = "storage_container_events";
    const TABLE: &'static str = "container_events";
    // A bad rollout turns restarts into a storm, and each row is a crash somebody will look for:
    // a deep buffer, two flushes.
    const LIMITS: Limits =
        Limits { max_rows: 1000, flush_interval: Duration::from_secs(5), buffer_limit: 50_000, max_in_flight: 2 };
}

/// One image as one host holds it. The table replaces on (tenant, image key, host), so host is
/// part of the row's identity.
///
/// The layer_* arrays are parallel and in wire order: an image is its ordered layer stack, so
/// nothing is sorted or deduplicated.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct ContainerImage {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub collected_at: DateTime<Utc>,
    pub host: String,
    /// The registry digest when there is one, the image id otherwise.
    pub image_key: String,
    /// "digest" or "image_id".
    pub identity_source: String,
    pub image_id: String,
    pub digest: String,
    pub name: String,
    pub short_name: String,
    pub registry: String,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub size_bytes: u64,
    pub os_name: String,
    pub os_version: String,
    pub architecture: String,
    pub layer_count: u32,
    pub layer_bytes: u64,
    #[serde(with = "column::millis::option")]
    pub built_at: Option<DateTime<Utc>>,
    #[serde(with = "column::millis::option")]
    pub published_at: Option<DateTime<Utc>>,
    pub dd_tags: Tags,
    pub payload_version: String,
    /// Which pipeline produced the inventory ("agent", "other"); None when the sender did not say.
    pub source: Option<String>,
    pub layer_media_types: Vec<String>,
    pub layer_digests: Vec<String>,
    /// As sent, negative values included.
    pub layer_sizes: Vec<i64>,
    pub layer_urls: Vec<Vec<String>>,
    #[serde(with = "column::millis::options")]
    pub layer_history_created: Vec<Option<DateTime<Utc>>>,
    pub layer_history_created_by: Vec<String>,
    pub layer_history_author: Vec<String>,
    pub layer_history_comment: Vec<String>,
    #[serde(with = "column::uint8_array")]
    pub layer_history_empty_layer: Vec<u8>,
    /// 1 when the image's size was negative and size_bytes was clamped to 0.
    pub size_negative: u8,
    /// 1 when any layer's size was negative: layer_bytes cannot be trusted.
    pub layer_size_negative: u8,
}

impl Table for ContainerImage {
    const WRITER: &'static str = "storage_container_images";
    const TABLE: &'static str = "container_images";
    // Every node re-announces every image it holds: bursty and repetitive. The table's engine
    // absorbs the repetition.
    const LIMITS: Limits =
        Limits { max_rows: 500, flush_interval: Duration::from_secs(10), buffer_limit: 20_000, max_in_flight: 2 };
}
