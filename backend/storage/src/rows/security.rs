use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::{Extra, Tags};
use crate::column::{self, Blob};
use crate::table::{Limits, Table};

/// One CWS activity dump request: the "event" header part, the envelope and metadata of the
/// "dump" part, and the dump itself.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct CwsActivityDump {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub dump_id: Uuid,
    pub header_host: String,
    pub header_service: String,
    pub header_source: String,
    pub header_tags: Tags,
    /// The header's dns_names as JSON text.
    pub dns_names: String,
    /// Header keys without a column, each as JSON text.
    pub header_extra: Extra,
    /// The "event" part as it arrived, whether or not it was JSON.
    pub header_raw: Blob,
    pub dump_host: String,
    pub dump_service: String,
    pub dump_source: String,
    pub dump_tags: Tags,
    pub agent_version: String,
    pub agent_commit: String,
    pub kernel_version: String,
    pub linux_distribution: String,
    pub arch: String,
    pub metadata_name: String,
    pub protobuf_version: String,
    pub differentiate_args: u8,
    pub comm: String,
    pub container_id: String,
    /// None without metadata: a real dump can report 0 here (the counters are relative to kernel
    /// boot), so 0 cannot stand for "absent".
    pub start_raw: Option<u64>,
    pub end_raw: Option<u64>,
    pub size_raw: Option<u64>,
    pub serialization: String,
    pub cgroup_id: String,
    pub cgroup_manager: String,
    /// Every node of the tree, at every depth.
    pub tree_node_count: u32,
    /// The "dump" part as it arrived: protobuf.
    pub dump: Blob,
}

impl Table for CwsActivityDump {
    const WRITER: &'static str = "storage_cws_activity_dumps";
    const TABLE: &'static str = "cws_activity_dumps";
    const LIMITS: Limits = Limits { max_rows: 100, flush_interval: Duration::from_secs(10), ..Limits::DEFAULT };
}

/// One process node of a dump's tree, flattened depth-first.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct CwsDumpNode {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub dump_id: Uuid,
    /// The index path from the root; enough to rebuild the tree.
    pub node_path: Vec<u32>,
    pub depth: u16,
    pub parent_path: Vec<u32>,
    /// The node as protobuf JSON, without its children.
    pub node: String,
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
    pub container_id: String,
    pub file_path: String,
    pub args: Vec<String>,
    pub image_tags: Vec<String>,
    pub matched_rule_ids: Vec<String>,
    pub generation_type: String,
    pub files_count: u32,
    pub dns_count: u32,
    pub sockets_count: u32,
    pub syscalls_count: u32,
}

impl Table for CwsDumpNode {
    const WRITER: &'static str = "storage_cws_dump_nodes";
    const TABLE: &'static str = "cws_dump_nodes";
    // One dump unpacks into thousands of nodes in a single request.
    const LIMITS: Limits =
        Limits { max_rows: 2000, flush_interval: Duration::from_secs(5), buffer_limit: 50_000, max_in_flight: 2 };
}

/// One logs-pipeline envelope from secruntime, secinfo or compliance, with the event inside its
/// "message" when that decoded.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SecurityEvent {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub track: String,
    pub seq_in_batch: u32,
    #[serde(with = "column::millis::option")]
    pub timestamp: Option<DateTime<Utc>>,
    pub hostname: String,
    pub service: String,
    pub ddsource: String,
    pub status: String,
    pub ddtags: Tags,
    /// Envelope keys without a column, each as JSON text.
    pub extra: Extra,
    /// The envelope's "message" as it arrived.
    pub message_raw: String,
    /// The event inside "message" as JSON; "" when it did not decode.
    pub message: String,
    /// Tells "decoded to an empty object" from "did not decode".
    pub message_decoded: u8,
    pub rule_id: String,
    pub policy_name: String,
    pub evt_name: String,
    pub evt_category: String,
    pub title: String,
    pub event_kind: String,
}

impl Table for SecurityEvent {
    const WRITER: &'static str = "storage_security_events";
    const TABLE: &'static str = "security_events";
    // Logs-shaped traffic, so the logs table's numbers.
    const LIMITS: Limits = Limits { max_rows: 20_000, buffer_limit: 500_000, ..Limits::DEFAULT };
}

/// SBOM rows are bursty: one payload is a whole image.
const SBOM: Limits =
    Limits { max_rows: 2000, flush_interval: Duration::from_secs(10), buffer_limit: 50_000, max_in_flight: 2 };

/// One entity of an SBOM payload. entity_id is minted on arrival, never taken from the wire's own
/// id, and is what components and vulnerabilities join on.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SbomEntity {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub entity_id: Uuid,
    pub payload_version: i32,
    pub host: String,
    pub source: Option<String>,
    pub dd_env: Option<String>,
    #[serde(rename = "type")]
    pub kind: String,
    pub id: String,
    #[serde(with = "column::millis::option")]
    pub generated_at: Option<DateTime<Utc>>,
    pub repo_tags: Vec<String>,
    pub repo_digests: Vec<String>,
    pub in_use: u8,
    pub generation_duration_ms: Option<i64>,
    pub dd_tags: Tags,
    pub heartbeat: u8,
    pub hash: String,
    pub status: String,
    pub kernel_version: String,
    pub cpu_architecture: String,
    /// error and bom are the two arms of the wire's oneof.
    pub error: String,
    pub bom: String,
    /// The F# server's backstop for a BOM it could not write as JSON, as base64 protobuf;
    /// json::proto writes every message, so it stays "".
    pub bom_raw: String,
    pub component_count: u32,
    pub vulnerability_count: u32,
}

impl Table for SbomEntity {
    const WRITER: &'static str = "storage_sbom_entities";
    const TABLE: &'static str = "sbom_entities";
    const LIMITS: Limits = Limits { max_rows: 500, buffer_limit: 20_000, ..SBOM };
}

/// One CycloneDX component; sub-components get rows of their own, linked by parent_bom_ref.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SbomComponent {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub entity_id: Uuid,
    pub bom_ref: String,
    pub parent_bom_ref: String,
    pub depth: u16,
    #[serde(rename = "type")]
    pub kind: String,
    pub name: String,
    pub version: String,
    /// None is "never set", which CycloneDX keeps apart from "".
    pub purl: Option<String>,
    pub cpe: Option<String>,
    pub group: Option<String>,
    pub publisher: Option<String>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub scope: String,
    pub licenses: Vec<String>,
    pub hashes: BTreeMap<String, String>,
    /// A multiset: CycloneDX allows properties to repeat a name.
    pub properties: BTreeMap<String, Vec<String>>,
    /// Protobuf JSON arrays of the repeated fields.
    pub external_references: String,
    pub evidence: String,
}

impl Table for SbomComponent {
    const WRITER: &'static str = "storage_sbom_components";
    const TABLE: &'static str = "sbom_components";
    const LIMITS: Limits = SBOM;
}

/// One CycloneDX vulnerability of an entity's BOM.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SbomVulnerability {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub entity_id: Uuid,
    pub bom_ref: String,
    pub id: String,
    pub source_name: Option<String>,
    pub source_url: Option<String>,
    pub ratings: String,
    pub cwes: Vec<i32>,
    pub description: Option<String>,
    pub detail: Option<String>,
    pub recommendation: Option<String>,
    pub advisories: String,
    #[serde(with = "column::millis::option")]
    pub created: Option<DateTime<Utc>>,
    #[serde(with = "column::millis::option")]
    pub published: Option<DateTime<Utc>>,
    #[serde(with = "column::millis::option")]
    pub updated: Option<DateTime<Utc>>,
    pub analysis_state: String,
    pub analysis_justification: String,
    pub analysis_response: Vec<String>,
    pub analysis_detail: String,
    pub affects_refs: Vec<String>,
    pub affects: String,
    pub properties: BTreeMap<String, Vec<String>>,
}

impl Table for SbomVulnerability {
    const WRITER: &'static str = "storage_sbom_vulnerabilities";
    const TABLE: &'static str = "sbom_vulnerabilities";
    const LIMITS: Limits = SBOM;
}
