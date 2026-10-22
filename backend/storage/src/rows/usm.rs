//! Universal Service Monitoring: what system-probe saw on a connection, per HTTP endpoint, Kafka
//! topic and database operation. The agent ships these inside each Connection as serialized
//! messages; here every entry is a row. Reasoning per column: the connection_*_stats tables in
//! 0001_initial.sql.
//!
//! Each row starts with the nine columns it shares with the `connections` row it came from (see
//! [`ConnectionKey`]) and ends with the eight of its latency sketch.

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::traces::SketchState;
use crate::column::{self, Blob};
use crate::table::Table;

/// The columns a stat row shares with the `connections` row it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct ConnectionKey {
    pub tenant_id: String,
    pub timestamp: DateTime<Utc>,
    pub host: String,
    pub payload_id: Uuid,
    pub pid: i32,
    pub laddr_ip: String,
    pub laddr_port: i32,
    pub raddr_ip: String,
    pub raddr_port: i32,
}

/// One endpoint of one connection, under one response status.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct ConnectionHttpStat {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    #[serde(with = "clickhouse::serde::uuid")]
    pub payload_id: Uuid,
    pub pid: i32,
    pub laddr_ip: String,
    pub laddr_port: i32,
    pub raddr_ip: String,
    pub raddr_port: i32,

    /// "http" or "http2".
    pub protocol: String,
    pub method: String,
    pub path: String,
    pub full_path: u8,
    /// 0 when the agent reported only the class.
    pub status_code: i32,
    /// "1xx" to "5xx".
    pub status_class: String,
    pub count: u32,
    /// The one latency when count is 1: the agent sends no sketch then.
    pub first_latency_sample: f64,
    pub latency_sum: f64,

    pub latencies: Blob,
    pub latencies_state: SketchState,
    pub sketch_count: Option<f64>,
    pub sketch_sum: Option<f64>,
    pub sketch_min: Option<f64>,
    pub sketch_max: Option<f64>,
    pub sketch_bin_keys: Vec<i32>,
    pub sketch_bin_counts: Vec<f64>,
}

impl Table for ConnectionHttpStat {
    const TABLE: &'static str = "connection_http_stats";
}

/// One Kafka request kind and topic of one connection, under one error code.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct ConnectionKafkaStat {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    #[serde(with = "clickhouse::serde::uuid")]
    pub payload_id: Uuid,
    pub pid: i32,
    pub laddr_ip: String,
    pub laddr_port: i32,
    pub raddr_ip: String,
    pub raddr_port: i32,

    /// Kafka's API key: 0 is Produce, 1 is Fetch.
    pub api_key: u32,
    pub api_version: u32,
    pub topic: String,
    /// -1 to 119; 0 is success.
    pub error_code: i32,
    pub count: u32,
    pub first_latency_sample: f64,

    pub latencies: Blob,
    pub latencies_state: SketchState,
    pub sketch_count: Option<f64>,
    pub sketch_sum: Option<f64>,
    pub sketch_min: Option<f64>,
    pub sketch_max: Option<f64>,
    pub sketch_bin_keys: Vec<i32>,
    pub sketch_bin_counts: Vec<f64>,
}

impl Table for ConnectionKafkaStat {
    const TABLE: &'static str = "connection_kafka_stats";
}

/// One database operation of one connection: a Postgres operation on a table, or a Redis command
/// on a key under one error type.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct ConnectionDatabaseStat {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    #[serde(with = "clickhouse::serde::uuid")]
    pub payload_id: Uuid,
    pub pid: i32,
    pub laddr_ip: String,
    pub laddr_port: i32,
    pub raddr_ip: String,
    pub raddr_port: i32,

    /// "postgres" or "redis".
    pub dbms: String,
    /// The enum's name: PostgresSelectOp, RedisGetCommand.
    pub operation: String,
    /// Postgres only.
    pub table_name: String,
    /// Redis only.
    pub key_name: String,
    pub key_truncated: u8,
    /// Redis only, the enum's name; RedisNoError for a success.
    pub error_type: String,
    pub count: u32,
    pub first_latency_sample: f64,

    pub latencies: Blob,
    pub latencies_state: SketchState,
    pub sketch_count: Option<f64>,
    pub sketch_sum: Option<f64>,
    pub sketch_min: Option<f64>,
    pub sketch_max: Option<f64>,
    pub sketch_bin_keys: Vec<i32>,
    pub sketch_bin_counts: Vec<f64>,
}

impl Table for ConnectionDatabaseStat {
    const TABLE: &'static str = "connection_database_stats";
}
