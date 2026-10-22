use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::column::{self, Blob};
use crate::table::Table;

/// One `agent flare` upload: the archive and the form fields sent with it.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct AgentFlare {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub hostname: String,
    pub case_id: String,
    pub email: String,
    pub source: String,
    pub agent_version: String,
    pub filename: String,
    pub size_bytes: u64,
    /// Form fields without a column of their own.
    pub fields: BTreeMap<String, String>,
    pub archive: Blob,
}

impl Table for AgentFlare {
    const TABLE: &'static str = "agent_flares";
}
