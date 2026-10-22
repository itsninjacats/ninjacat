use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::Tags;
use crate::column;
use crate::table::Table;

/// Host metadata from /intake/. The table is a ReplacingMergeTree on (tenant_id, host): repeated
/// writes collapse to the newest.
///
/// The sections whose shape moves between agent versions are kept as their JSON text. An empty
/// string there means the agent did not send the section, which is not the same statement as
/// "{}".
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct Host {
    pub tenant_id: String,
    pub host: String,
    #[serde(with = "column::secs")]
    pub seen_at: DateTime<Utc>,
    pub agent_version: String,
    pub os: String,
    pub platform: BTreeMap<String, String>,
    pub cpu: BTreeMap<String, String>,
    pub memory: BTreeMap<String, String>,
    /// The "system" tag source alone: the column charts filter on.
    pub tags: Tags,
    pub uuid: String,
    pub agent_flavor: String,
    pub python_version: String,
    pub meta: String,
    pub system_stats: BTreeMap<String, String>,
    pub network: String,
    pub filesystem: String,
    pub logs: String,
    pub install_method: BTreeMap<String, String>,
    pub proxy_info: BTreeMap<String, String>,
    pub otlp: String,
    pub container_meta: BTreeMap<String, String>,
    /// None when the agent sent no such key: "FIPS is off" and "this agent does not know about
    /// FIPS" are different statements.
    pub fips_mode: Option<u8>,
    pub fips_proxy_enabled: Option<u8>,
    /// Every tag source (system, google_tags, …) with its whole tag list.
    pub host_tags: Tags,
    /// gohai sections without a column, and envelope keys the decoder does not know. Values
    /// JSON-encoded.
    pub gohai_extra: BTreeMap<String, String>,
    pub intake_extra: BTreeMap<String, String>,
    /// The legacy V5 process snapshot riding on the host payload, verbatim.
    pub resources: String,
}

impl Table for Host {
    const TABLE: &'static str = "hosts";
}
