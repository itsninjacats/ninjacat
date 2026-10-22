use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer};

use super::{Extra, Tags};
use crate::column;
use crate::table::Table;

/// One service check result.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct CheckRun {
    pub tenant_id: String,
    #[serde(with = "column::secs")]
    pub timestamp: DateTime<Utc>,
    pub check_name: String,
    pub host: String,
    pub status: CheckStatus,
    pub message: String,
    pub tags: Tags,
    /// Keys Datadog's ServiceCheck does not declare, values JSON-encoded.
    pub extra: Extra,
}

/// The column's Enum8: the numbers are the agent's own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckStatus {
    Ok = 0,
    Warning = 1,
    Critical = 2,
    Unknown = 3,
}

impl CheckStatus {
    pub fn of(status: i64) -> Self {
        match status {
            0 => CheckStatus::Ok,
            1 => CheckStatus::Warning,
            2 => CheckStatus::Critical,
            _ => CheckStatus::Unknown,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CheckStatus::Ok => "OK",
            CheckStatus::Warning => "WARNING",
            CheckStatus::Critical => "CRITICAL",
            CheckStatus::Unknown => "UNKNOWN",
        }
    }
}

impl Serialize for CheckStatus {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(self.name())
        } else {
            serializer.serialize_i8(*self as i8)
        }
    }
}

impl Table for CheckRun {
    const TABLE: &'static str = "check_runs";
}
