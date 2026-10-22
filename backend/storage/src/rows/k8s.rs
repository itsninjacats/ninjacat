//! Every orchestrator row carries the frame header (org_id, subscription_id, header_timestamp,
//! encoding). Today's agent leaves the first three at zero, which is the reason to keep them: the
//! day a sender fills them, they are already stored. header_timestamp is the raw integer because
//! the frame format documents no unit for it; None when the wire said 0.

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::Tags;
use crate::column::{self, Blob};
use crate::table::Table;

/// One Kubernetes object in one collection pass, whatever its kind.
///
/// Three layers keep 27 kinds in one table: what every kind has is a column; numbers that differ
/// per kind travel in counts; and object holds the whole message as JSON, so a kind with no typed
/// column of its own loses nothing.
///
/// The parallel arrays (owner_*, condition_*, container_*) are index-aligned and in wire order.
///
/// k8s_resources_current is fed from this table by a materialized view that lists every column by
/// name: a field added here needs the column on both tables and in the view.
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct K8sResource {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub collected_at: DateTime<Utc>,
    pub cluster_id: String,
    pub cluster_name: String,
    pub kind: String,
    pub namespace: String,
    pub name: String,
    pub uid: String,
    pub resource_version: String,
    /// The first owner reference; all of them are in owner_* below.
    pub owner_kind: String,
    pub owner_name: String,
    pub node_name: String,
    pub phase: String,
    pub status: String,
    pub ready: i32,
    pub desired: i32,
    pub available: i32,
    pub counts: BTreeMap<String, i64>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub tags: Tags,
    pub agent_version: String,
    pub group_id: i32,
    pub group_size: i32,
    pub org_id: i32,
    pub subscription_id: u8,
    pub header_timestamp: Option<i64>,
    pub encoding: String,
    #[serde(with = "column::secs::option")]
    pub creation_timestamp: Option<DateTime<Utc>>,
    #[serde(with = "column::secs::option")]
    pub deletion_timestamp: Option<DateTime<Utc>>,
    pub deletion_grace_period_seconds: Option<i64>,
    pub finalizers: Vec<String>,
    pub owner_kinds: Vec<String>,
    pub owner_names: Vec<String>,
    pub owner_uids: Vec<String>,
    pub condition_types: Vec<String>,
    pub condition_statuses: Vec<String>,
    pub condition_reasons: Vec<String>,
    pub condition_messages: Vec<String>,
    #[serde(with = "column::secs::options")]
    pub condition_last_transition: Vec<Option<DateTime<Utc>>>,
    #[serde(with = "column::secs::options")]
    pub condition_last_update: Vec<Option<DateTime<Utc>>>,
    #[serde(with = "column::secs::options")]
    pub condition_last_probe: Vec<Option<DateTime<Utc>>>,
    /// The agent's own one-line summary, a separate wire field from the condition list.
    pub condition_message: String,
    /// JSON: one object carries several (one per container), each with two maps, so a flat map
    /// would lose which container the numbers belong to.
    pub resource_requirements: String,
    #[serde(with = "column::float_map")]
    pub metrics: BTreeMap<String, f64>,
    /// The whole object as JSON.
    pub object: String,
    // Pod.
    pub pod_ip: String,
    pub nominated_node_name: String,
    pub qos_class: String,
    pub priority_class: String,
    #[serde(with = "column::secs::option")]
    pub start_time: Option<DateTime<Utc>>,
    #[serde(with = "column::secs::option")]
    pub scheduled_time: Option<DateTime<Utc>>,
    /// The reporting agent's host, which is not node_name for cluster-scoped kinds.
    pub host_name: String,
    // Container statuses: regular containers first, then init containers, told apart by
    // container_is_init.
    pub container_names: Vec<String>,
    pub container_ids: Vec<String>,
    #[serde(with = "column::uint8_array")]
    pub container_ready: Vec<u8>,
    pub container_restarts: Vec<i32>,
    pub container_states: Vec<String>,
    pub container_messages: Vec<String>,
    pub container_images: Vec<String>,
    pub container_image_ids: Vec<String>,
    #[serde(with = "column::uint8_array")]
    pub container_is_init: Vec<u8>,
    // Node.
    pub pod_cidr: String,
    pub pod_cidrs: Vec<String>,
    pub unschedulable: u8,
    /// JSON.
    pub taints: String,
    pub provider_id: String,
    pub node_roles: Vec<String>,
    pub capacity: BTreeMap<String, i64>,
    pub allocatable: BTreeMap<String, i64>,
    /// JSON: address type → address.
    pub node_addresses: String,
    pub kubelet_version: String,
    pub kube_proxy_version: String,
    pub operating_system: String,
    pub architecture: String,
    pub kernel_version: String,
    pub os_image: String,
    pub container_runtime_version: String,
    // Service.
    pub service_type: String,
    pub cluster_ip: String,
    /// JSON.
    pub service_ports: String,
    // RBAC, all three JSON. The rules are the permissions actually granted.
    pub rbac_rules: String,
    pub rbac_subjects: String,
    pub rbac_role_ref: String,
    /// The frame's message as JSON without its object list: the per-kind extras no column holds.
    /// The same on every row of a pass.
    pub envelope: String,
}

impl Table for K8sResource {
    const TABLE: &'static str = "k8s_resources";
}

/// One object's manifest, its YAML or JSON exactly as the cluster agent collected it.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct K8sManifest {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub collected_at: DateTime<Utc>,
    pub cluster_id: String,
    pub cluster_name: String,
    pub uid: String,
    pub kind: String,
    pub api_version: String,
    pub resource_version: String,
    pub content: Blob,
    pub content_type: String,
    pub is_terminated: u8,
    pub org_id: i32,
    pub subscription_id: u8,
    pub header_timestamp: Option<i64>,
    pub encoding: String,
    pub group_id: i32,
    pub group_size: i32,
    pub host_name: String,
    pub agent_version: String,
    pub origin_collector: String,
    /// Every tag that applies, from all three places the wire attaches them, merged. tag_sources
    /// keeps who attached what.
    pub tags: Tags,
    pub tag_sources: BTreeMap<String, Vec<String>>,
    pub node_name: String,
    pub r#type: i32,
    pub version: String,
    pub extra_attributes: BTreeMap<String, String>,
    /// 1 when content is valid UTF-8, so a reader knows before parsing.
    pub content_is_utf8: u8,
    /// The frame's message as JSON, without its manifest list.
    pub envelope: String,
    /// The machine this one object was seen on, as JSON.
    pub manifest_host: String,
}

impl Table for K8sManifest {
    const TABLE: &'static str = "k8s_manifests";
}

/// The cluster as the agent sums it up: one row per cluster per pass.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct K8sCluster {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub collected_at: DateTime<Utc>,
    pub cluster_id: String,
    pub cluster_name: String,
    pub node_count: u32,
    pub pod_capacity: u32,
    pub pod_allocatable: u32,
    pub cpu_capacity: u64,
    pub cpu_allocatable: u64,
    pub memory_capacity: u64,
    pub memory_allocatable: u64,
    /// Version → nodes running it.
    pub kubelet_versions: BTreeMap<String, u32>,
    pub apiserver_versions: BTreeMap<String, u32>,
    pub org_id: i32,
    pub subscription_id: u8,
    pub header_timestamp: Option<i64>,
    pub encoding: String,
    pub group_id: i32,
    pub group_size: i32,
    pub agent_version: String,
    /// The frame's tags; cluster_tags are the cluster object's own.
    pub tags: Tags,
    pub cluster_tags: Tags,
    pub resource_version: String,
    #[serde(with = "column::secs::option")]
    pub creation_timestamp: Option<DateTime<Utc>>,
    #[serde(with = "column::float_map")]
    pub metrics: BTreeMap<String, f64>,
    /// GPUs and whatever else a node advertises beyond cpu, memory, pods.
    pub extended_resources_capacity: BTreeMap<String, i64>,
    pub extended_resources_allocatable: BTreeMap<String, i64>,
    // The per-node breakdown behind node_count, parallel and in wire order.
    pub nodes_name: Vec<String>,
    pub nodes_region: Vec<String>,
    pub nodes_instance_type: Vec<String>,
    pub nodes_os: Vec<String>,
    pub nodes_os_image: Vec<String>,
    pub nodes_architecture: Vec<String>,
    pub nodes_kernel_version: Vec<String>,
    pub nodes_container_runtime_version: Vec<String>,
    pub nodes_kubelet_version: Vec<String>,
    /// Kubernetes quantity strings ("16Gi", "4"), kept as sent.
    pub nodes_allocatable: Vec<BTreeMap<String, String>>,
    pub nodes_capacity: Vec<BTreeMap<String, String>>,
}

impl Table for K8sCluster {
    const TABLE: &'static str = "k8s_cluster";
}

/// What the cluster agent did with a remote action: the audit record.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct K8sAction {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub action_id: String,
    pub org_id: i64,
    pub event_type: String,
    pub status: String,
    pub action_type: String,
    pub cluster_id: String,
    pub cluster_name: String,
    pub resource_id: String,
    pub resource_kind: String,
    pub resource_name: String,
    pub resource_namespace: String,
    pub requested_by: String,
    pub message: String,
    /// The names of the keys the event carried beyond the known ones, sorted; extra has them with
    /// their values, as a JSON object.
    pub extra_keys: Vec<String>,
    /// What the executor attached, name → bytes.
    pub payloads: BTreeMap<String, Blob>,
    pub extra: String,
    /// The timestamp exactly as it arrived. timestamp falls back to the arrival time when this is
    /// empty or does not parse.
    pub timestamp_raw: String,
}

impl Table for K8sAction {
    const TABLE: &'static str = "k8s_actions";
}
