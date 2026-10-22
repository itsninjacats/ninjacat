//! The APM family: everything that arrives on trace.agent.<site>.
//!
//!   spans                    POST /api/v0.2/traces
//!   apm_stats                POST /api/v0.2/stats
//!   dsm_pipeline_stats       POST /api/v0.1/pipeline_stats
//!   dsm_backlogs             the same request, the bucket's backlog list
//!   dsm_bucket_transactions  the same request, the bucket's two packed blobs
//!   dsm_messages             POST /api/v2/data_streams_messages

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Serialize, Serializer};

use super::Tags;
use crate::column::{self, Blob};
use crate::table::Table;

/// One latency distribution: the bytes as the sender shipped them, plus what they decoded to.
///
/// The raw bytes are the record: a DDSketch merges with other sketches and answers any quantile
/// afterwards. The decoded fields only save the common query from unmarshalling a blob.
///
/// A row holds a summary as eight columns of its own (the bytes, the state, the four numbers and
/// the two bin arrays), named by the row.
#[derive(Clone, Debug, PartialEq)]
pub struct SketchSummary {
    pub raw: Vec<u8>,
    pub state: SketchState,
    /// The four numbers exist only in the Ok state: a zero would read as a measurement of zero.
    pub count: Option<f64>,
    pub sum: Option<f64>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    /// The positive-value store, index and count in parallel.
    pub bin_keys: Vec<i32>,
    pub bin_counts: Vec<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SketchState {
    /// No bytes were sent.
    Absent,
    /// Bytes arrived and are not a DDSketch. The reason is for the log; the table records only
    /// that it happened, and keeps the bytes.
    Undecodable(String),
    /// A valid sketch with no values.
    Empty,
    Ok,
}

impl SketchState {
    /// The state as the tables' Enum8 column spells it.
    pub fn name(&self) -> &'static str {
        match self {
            SketchState::Absent => "absent",
            SketchState::Undecodable(_) => "undecodable",
            SketchState::Empty => "empty",
            SketchState::Ok => "ok",
        }
    }

    fn number(&self) -> i8 {
        match self {
            SketchState::Absent => 0,
            SketchState::Undecodable(_) => 1,
            SketchState::Empty => 2,
            SketchState::Ok => 3,
        }
    }
}

impl Serialize for SketchState {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(self.name())
        } else {
            serializer.serialize_i8(self.number())
        }
    }
}

/// One span, with the agent, tracer and chunk it arrived under copied onto it.
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct Span {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    /// "v04" or "idx". Several columns exist in only one of the two wire formats, so a zero means
    /// "no such field" for one and "left unset" for the other; nothing else tells those apart.
    pub wire_format: String,

    pub agent_hostname: String,
    pub agent_env: String,
    pub agent_version: String,
    pub target_tps: f64,
    pub error_tps: f64,
    pub rare_sampler_enabled: u8,
    pub agent_tags: BTreeMap<String, String>,

    pub container_id: String,
    pub language: String,
    pub language_version: String,
    pub tracer_version: String,
    pub runtime_id: String,
    pub tracer_env: String,
    pub tracer_hostname: String,
    pub app_version: String,
    pub tracer_tags: BTreeMap<String, String>,
    /// The idx typed form of the tracer's attributes; empty for v0.4.
    pub tracer_attributes_json: String,

    /// Present only when the agent had trouble resolving the container.
    pub container_debug_error: Option<String>,
    pub container_debug_latency_ms: Option<i64>,
    pub container_debug_was_buffered: Option<u8>,
    pub container_debug_buffer_ms: Option<i64>,
    pub container_debug_buffer_eviction_reason: Option<String>,

    pub priority: i32,
    pub origin: String,
    pub dropped_trace: u8,
    pub sampling_mechanism: u32,
    pub chunk_tags: BTreeMap<String, String>,
    pub chunk_attributes_json: String,

    pub service: String,
    pub name: String,
    pub resource: String,
    pub span_type: String,

    /// The 128-bit trace id as two integers, whichever wire shape it came in.
    pub trace_id: u64,
    pub trace_id_high: u64,
    pub span_id: u64,
    /// 0 means root.
    pub parent_id: u64,

    #[serde(with = "column::nanos")]
    pub start: i64,
    pub duration_ns: i64,
    pub error: i32,

    pub kind: String,
    pub span_env: String,
    pub span_version: String,
    pub component: String,

    pub meta: BTreeMap<String, String>,
    pub metrics: BTreeMap<String, f64>,
    /// msgpack blobs with no published type per key: bytes, not text.
    pub meta_struct: BTreeMap<String, Blob>,
    /// The idx typed attribute map in full; meta and metrics carry its scalar projection.
    pub attributes_json: String,

    /// Links and events as parallel arrays: the index is the wire order.
    pub link_trace_id: Vec<u64>,
    pub link_trace_id_high: Vec<u64>,
    pub link_span_id: Vec<u64>,
    pub link_attributes: Vec<String>,
    pub link_tracestate: Vec<String>,
    pub link_flags: Vec<u32>,

    #[serde(with = "column::nanos::list")]
    pub event_time: Vec<i64>,
    pub event_name: Vec<String>,
    pub event_attributes: Vec<String>,
}

impl Table for Span {
    const TABLE: &'static str = "spans";
}

/// The column's Enum8: the tracer not saying is an answer too.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TraceRoot {
    NotSet = 0,
    True = 1,
    False = 2,
}

impl TraceRoot {
    /// The Trilean as sent: 1 is true, 2 false, anything else not set.
    pub fn of(trilean: i32) -> Self {
        match trilean {
            1 => TraceRoot::True,
            2 => TraceRoot::False,
            _ => TraceRoot::NotSet,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            TraceRoot::NotSet => "not_set",
            TraceRoot::True => "true",
            TraceRoot::False => "false",
        }
    }
}

impl Serialize for TraceRoot {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            serializer.serialize_str(self.name())
        } else {
            serializer.serialize_i8(*self as i8)
        }
    }
}

/// One grouped stat with its payload, client and bucket copied onto it.
///
/// These counts are complete: the agent's concentrator sees every trace before the sampler runs,
/// which is why this table is not derived from spans.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct ApmStat {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,

    pub agent_hostname: String,
    pub agent_env: String,
    pub agent_version: String,
    pub client_computed: u8,
    pub split_payload: u8,

    pub client_hostname: String,
    pub client_env: String,
    pub client_version: String,
    pub client_lang: String,
    pub client_tracer_version: String,
    pub client_runtime_id: String,
    pub client_sequence: u64,
    pub client_agent_aggregation: String,
    pub client_service: String,
    pub client_container_id: String,
    /// The one flat "key:value" tag list in the trace protocol.
    pub client_tags: Tags,
    pub client_git_commit_sha: String,
    pub client_image_tag: String,
    pub client_process_tags: String,
    pub client_process_tags_hash: u64,

    #[serde(with = "column::nanos")]
    pub bucket_start: i64,
    pub bucket_duration_ns: u64,
    pub agent_time_shift_ns: i64,

    pub service: String,
    pub name: String,
    pub resource: String,
    pub http_status_code: u32,
    pub span_type: String,
    pub db_type: String,
    pub hits: u64,
    pub errors: u64,
    pub duration_ns: u64,
    pub synthetics: u8,
    pub top_level_hits: u64,
    pub span_kind: String,
    pub peer_tags: Vec<String>,
    pub is_trace_root: TraceRoot,
    pub grpc_status_code: String,
    pub http_method: String,
    pub http_endpoint: String,
    pub service_source: String,
    pub span_derived_primary_tags: Vec<String>,
    pub additional_metric_tags: Vec<String>,

    pub ok_summary: Blob,
    pub ok_summary_state: SketchState,
    pub ok_count: Option<f64>,
    pub ok_sum: Option<f64>,
    pub ok_min: Option<f64>,
    pub ok_max: Option<f64>,
    pub ok_bin_keys: Vec<i32>,
    pub ok_bin_counts: Vec<f64>,

    pub error_summary: Blob,
    pub error_summary_state: SketchState,
    pub error_count: Option<f64>,
    pub error_sum: Option<f64>,
    pub error_min: Option<f64>,
    pub error_max: Option<f64>,
    pub error_bin_keys: Vec<i32>,
    pub error_bin_counts: Vec<f64>,
}

impl Table for ApmStat {
    const TABLE: &'static str = "apm_stats";
}

/// One checkpoint in a queue pathway. Hash and parent_hash rebuild the pathway graph by
/// self-join.
///
/// The first eleven columns are the payload and bucket every Data Streams row carries: the three
/// tables are children of the same bucket and join on them.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct DsmPipelineStat {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub env: String,
    pub service: String,
    pub tracer_version: String,
    pub lang: String,
    pub version: String,
    pub process_tags: Vec<String>,
    pub product_mask: u64,
    #[serde(with = "column::nanos")]
    pub bucket_start: i64,
    pub bucket_duration_ns: u64,

    pub edge_tags: Vec<String>,
    pub hash: u64,
    pub parent_hash: u64,
    pub timestamp_type: String,

    /// Latencies are in seconds; payload_size is a distribution of message sizes in bytes.
    pub pathway_latency: Blob,
    pub pathway_latency_state: SketchState,
    pub pathway_count: Option<f64>,
    pub pathway_sum: Option<f64>,
    pub pathway_min: Option<f64>,
    pub pathway_max: Option<f64>,
    pub pathway_bin_keys: Vec<i32>,
    pub pathway_bin_counts: Vec<f64>,

    pub edge_latency: Blob,
    pub edge_latency_state: SketchState,
    pub edge_count: Option<f64>,
    pub edge_sum: Option<f64>,
    pub edge_min: Option<f64>,
    pub edge_max: Option<f64>,
    pub edge_bin_keys: Vec<i32>,
    pub edge_bin_counts: Vec<f64>,

    pub payload_size: Blob,
    pub payload_size_state: SketchState,
    pub payload_size_count: Option<f64>,
    pub payload_size_sum: Option<f64>,
    pub payload_size_min: Option<f64>,
    pub payload_size_max: Option<f64>,
    pub payload_size_bin_keys: Vec<i32>,
    pub payload_size_bin_counts: Vec<f64>,

    /// The headers the trace-agent adds as it proxies the request: the only link from a pathway
    /// to a workload, the body has none.
    pub via: String,
    pub additional_tags: String,
    pub container_tags: String,
    pub content_encoding: String,
    /// Keys the decoder did not recognise, and their values as JSON.
    pub unknown_keys: Vec<String>,
    pub unknown_json: String,
}

impl Table for DsmPipelineStat {
    const TABLE: &'static str = "dsm_pipeline_stats";
}

/// How far behind a consumer is. Tags stay an ordered array: the tracer hashes on the list, order
/// included.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct DsmBacklog {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub env: String,
    pub service: String,
    pub tracer_version: String,
    pub lang: String,
    pub version: String,
    pub process_tags: Vec<String>,
    pub product_mask: u64,
    #[serde(with = "column::nanos")]
    pub bucket_start: i64,
    pub bucket_duration_ns: u64,

    pub tags: Vec<String>,
    pub value: i64,
}

impl Table for DsmBacklog {
    const TABLE: &'static str = "dsm_backlogs";
}

/// A bucket's two packed binary blobs, verbatim. The layout is hand-rolled and must match the Java
/// tracer byte for byte, so it is not decoded here.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct DsmBucketTransactions {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub env: String,
    pub service: String,
    pub tracer_version: String,
    pub lang: String,
    pub version: String,
    pub process_tags: Vec<String>,
    pub product_mask: u64,
    #[serde(with = "column::nanos")]
    pub bucket_start: i64,
    pub bucket_duration_ns: u64,

    pub transactions: Blob,
    pub transaction_checkpoint_ids: Blob,
}

impl Table for DsmBucketTransactions {
    const TABLE: &'static str = "dsm_bucket_transactions";
}

/// One element of the /api/v2/data_streams_messages array, as the JSON text it arrived as: its
/// producer is not public, so no shape is invented.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct DsmMessage {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    /// The place in the batch, the only ordering these messages have.
    pub position: u32,
    /// JSON as it arrived, as bytes.
    pub message: Blob,
    /// The element's top-level key names, sorted.
    pub keys: Vec<String>,
    pub dd_evp_origin: String,
    pub dd_evp_origin_version: String,
    pub content_encoding: String,
}

impl Table for DsmMessage {
    const TABLE: &'static str = "dsm_messages";
}
