//! What the agent says about itself and its host on api.<site>, beside its telemetry: its
//! metadata, its checks, the tags of hosts it does not run on.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::Tags;
use crate::column;
use crate::table::{Limits, Table};

/// Who sent a sketch batch: CommonMetadata without its api_key, which is a credential and has no
/// column.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct AgentBatchMetadata {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub intake: String,
    pub agent_version: String,
    pub timezone: String,
    /// The agent's own clock at send time, in seconds. Against received_at it measures clock skew.
    pub current_epoch: f64,
    pub internal_ip: String,
    pub public_ip: String,
}

impl Table for AgentBatchMetadata {
    const WRITER: &'static str = "storage_agent_batch_metadata";
    const TABLE: &'static str = "agent_batch_metadata";
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}

/// One entry of the V5 collector's agent_checks list. The wire shape is a positional array with
/// no key names: the five documented positions are columns, anything after them is kept as a
/// JSON array.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct AgentCheck {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub hostname: String,
    pub agent_version: String,
    pub uuid: String,
    pub check_name: String,
    pub source_type: String,
    pub instance_id: String,
    /// None when the array stopped before position 3: status 0 means OK, and a truncated entry
    /// must not read as healthy.
    pub status: Option<i64>,
    pub message: String,
    pub positional_extra: String,
    pub meta: String,
}

impl Table for AgentCheck {
    const WRITER: &'static str = "storage_agent_checks";
    const TABLE: &'static str = "agent_checks";
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}

/// Tags an agent reported for a host it is NOT running on, one row per (host, source).
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct ExternalHostTags {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub host: String,
    pub source: String,
    pub tags: Tags,
}

impl Table for ExternalHostTags {
    const WRITER: &'static str = "storage_external_host_tags";
    const TABLE: &'static str = "external_host_tags";
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}

/// One variant of /api/v1/metadata: the shared envelope as columns, the variant's own body as
/// JSON text.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct AgentMetadata {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub variant: String,
    pub hostname: String,
    pub cluster_name: String,
    pub cluster_id: String,
    /// The envelope's own timestamp; optional, and 1970 is not an answer.
    #[serde(with = "column::millis::option")]
    pub timestamp: Option<DateTime<Utc>>,
    pub uuid: String,
    pub payload: String,
    pub envelope_extra: BTreeMap<String, String>,
}

impl Table for AgentMetadata {
    const WRITER: &'static str = "storage_agent_metadata";
    const TABLE: &'static str = "agent_metadata";
    // A row is a whole inventory document, so the counts stand for megabytes.
    const LIMITS: Limits =
        Limits { max_rows: 200, flush_interval: Duration::from_secs(10), buffer_limit: 5_000, max_in_flight: 1 };
}

/// One /api/v2/intake-key exchange. proof_fingerprint is the SHA-256 of the delegated-auth proof,
/// never the proof.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct DelegatedAuth {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub at: DateTime<Utc>,
    pub scheme: String,
    pub proof_fingerprint: String,
    pub api_key_id: String,
}

impl Table for DelegatedAuth {
    const WRITER: &'static str = "storage_delegated_auth_requests";
    const TABLE: &'static str = "delegated_auth_requests";
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}

/// The processes of one name on a host, summed: what the agent's resources check reports beside
/// the host metadata.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct ProcessGroup {
    pub tenant_id: String,
    /// The agent's clock when it took the snapshot.
    #[serde(with = "column::secs")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    pub name: String,
    pub usernames: Vec<String>,
    pub process_count: u32,
    pub cpu_pct: f64,
    pub mem_pct: f64,
    pub vms: u64,
    pub rss: u64,
}

impl Table for ProcessGroup {
    const WRITER: &'static str = "storage_process_groups";
    const TABLE: &'static str = "process_groups";
    const LIMITS: Limits = Limits { max_rows: 2_000, flush_interval: Duration::from_secs(10), ..Limits::DEFAULT };
}
