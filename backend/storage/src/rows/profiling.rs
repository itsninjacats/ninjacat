use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use super::Tags;
use crate::column::{self, Blob};
use crate::table::{Limits, Table};

/// One multipart submission to intake.profile.<site>.
///
/// Samples are not exploded into rows: a CPU profile carries tens of thousands of them, and pprof
/// reads the bytes kept in attach_bytes. What is kept per attachment is the pprof envelope, as
/// parallel arrays, so a query can filter profiles by shape without parsing them again.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct Profile {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    /// Which path produced the row: "profile" or "profile-v1".
    pub variant: String,
    pub dd_evp_origin: String,
    pub dd_evp_origin_version: String,
    /// The "event" part as it arrived. The fields after it are read from it.
    pub event: Blob,
    pub start_raw: String,
    #[serde(with = "column::millis::option")]
    pub start_parsed: Option<DateTime<Utc>>,
    pub end_raw: String,
    #[serde(with = "column::millis::option")]
    pub end_parsed: Option<DateTime<Utc>>,
    pub family: String,
    pub version: String,
    pub runtime: String,
    pub language: String,
    pub tags_profiler: Tags,
    /// One entry per part other than "event", ordered by part name: the order on the wire is not
    /// kept.
    pub attach_name: Vec<String>,
    /// The part as it arrived, still gzip-compressed if it was.
    pub attach_bytes: Vec<Blob>,
    pub attach_size: Vec<u64>,
    /// 1 when the part is pprof; the fields after it are zero when it is not.
    #[serde(with = "column::uint8_array")]
    pub attach_parsed: Vec<u8>,
    pub attach_sample_types: Vec<Vec<String>>,
    pub attach_sample_units: Vec<Vec<String>>,
    pub attach_sample_count: Vec<u64>,
    pub attach_time_nanos: Vec<i64>,
    pub attach_duration_nanos: Vec<i64>,
    pub attach_period_type: Vec<String>,
    pub attach_period: Vec<i64>,
    pub attach_mapping_count: Vec<u32>,
    pub attach_location_count: Vec<u32>,
    pub attach_function_count: Vec<u32>,
}

impl Table for Profile {
    const WRITER: &'static str = "storage_profiles";
    const TABLE: &'static str = "profiles";
    // A row is a whole submission with megabytes of pprof: few rows per batch, one flush at a
    // time.
    const LIMITS: Limits =
        Limits { max_rows: 200, flush_interval: Duration::from_secs(10), buffer_limit: 5_000, max_in_flight: 1 };
}

/// One entry of a Dynamic Instrumentation logs batch.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct DebuggerLog {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub service: String,
    pub ddsource: String,
    /// The `ddtags` query parameter: it describes the whole batch.
    pub ddtags: Tags,
    /// The array element or NDJSON line as it arrived.
    pub entry: Blob,
    /// The entry's top-level keys other than service and ddsource.
    pub extra_keys: Vec<String>,
}

impl Table for DebuggerLog {
    const WRITER: &'static str = "storage_debugger_logs";
    const TABLE: &'static str = "debugger_logs";
    const LIMITS: Limits = Limits { max_rows: 500, ..Limits::DEFAULT };
}

/// One probe status message.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct DebuggerDiagnostic {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    /// The message's own time; None when missing or unreadable, never the receive time.
    #[serde(with = "column::millis::option")]
    pub timestamp: Option<DateTime<Utc>>,
    pub service: String,
    pub ddsource: String,
    pub ddtags: Tags,
    pub runtime_id: String,
    pub probe_id: String,
    pub status: String,
    pub probe_version: String,
    /// Both None without an exception, so "no exception" and "an exception with an empty
    /// message" stay apart.
    pub exception_type: Option<String>,
    pub exception_message: Option<String>,
    /// The array element as it arrived.
    pub message: Blob,
}

impl Table for DebuggerDiagnostic {
    const WRITER: &'static str = "storage_debugger_diagnostics";
    const TABLE: &'static str = "debugger_diagnostics";
    const LIMITS: Limits = Limits { max_rows: 200, ..Limits::DEFAULT };
}

/// One symbol-database upload.
///
/// The event part and the envelope inside the gzip file describe the same upload but come from two
/// serializers: the event says `uploadId`, the envelope `upload_id`. Both are kept (the envelope's
/// under env_…) so a drift between them can be seen.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SymdbUpload {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    /// The "event" part as it arrived.
    pub event: Blob,
    pub ddtags: Tags,
    pub service: String,
    pub version: String,
    pub language: String,
    pub runtime_id: String,
    /// Identifiers, kept as the text on the wire: nothing says they stay numeric.
    pub upload_id: String,
    pub batch_num: String,
    /// None when the event does not carry them, which is not "false" or "0".
    pub r#final: Option<u8>,
    pub attachment_size: Option<u64>,
    /// The "file" part as it arrived, gzip-compressed.
    pub file: Blob,
    pub inflated_size: u64,
    /// Top-level scopes only.
    pub scope_count: u32,
    /// 1 when the envelope's "scopes" was present and decoded.
    pub scopes_ok: u8,
    pub env_service: String,
    pub env_version: String,
    pub env_language: String,
    pub env_upload_id: String,
    pub env_batch_num: String,
    pub env_final: String,
}

impl Table for SymdbUpload {
    const WRITER: &'static str = "storage_symdb_uploads";
    const TABLE: &'static str = "symdb_uploads";
    const LIMITS: Limits =
        Limits { max_rows: 200, flush_interval: Duration::from_secs(10), buffer_limit: 5_000, max_in_flight: 1 };
}

/// One native symbol upload from the host profiler.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SymbolUpload {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    pub r#type: String,
    pub arch: String,
    pub gnu_build_id: String,
    pub go_build_id: String,
    pub file_hash: String,
    pub symbol_source: String,
    pub origin: String,
    pub origin_version: String,
    pub filename: String,
    /// The "event" part as it arrived.
    pub meta: Blob,
    pub has_elf: u8,
    /// "ELF32", "ELF64", "ELF?"; "" without an ELF file.
    pub elf_class: String,
    /// "LE", "BE", "?"; "" without an ELF file.
    pub elf_endianness: String,
    pub elf: Blob,
    pub elf_size: u64,
    /// Every part other than "event" and "elf_symbol_file".
    pub other_parts: BTreeMap<String, Blob>,
}

impl Table for SymbolUpload {
    const WRITER: &'static str = "storage_symbol_uploads";
    const TABLE: &'static str = "symbol_uploads";
    const LIMITS: Limits =
        Limits { max_rows: 200, flush_interval: Duration::from_secs(10), buffer_limit: 5_000, max_in_flight: 1 };
}

/// One "which of these build ids do you hold?" from the profiler's symbol uploader. build_ids
/// keeps its order: the reply is positional against it.
#[derive(clickhouse::Row, Serialize, Clone, Debug)]
pub struct SymbolQuery {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub at: DateTime<Utc>,
    pub arch: String,
    pub build_ids: Vec<String>,
    pub resource: String,
}

impl Table for SymbolQuery {
    const WRITER: &'static str = "storage_symbol_queries";
    const TABLE: &'static str = "symbol_queries";
    const LIMITS: Limits =
        Limits { max_rows: 50, flush_interval: Duration::from_secs(15), buffer_limit: 1_000, max_in_flight: 1 };
}
