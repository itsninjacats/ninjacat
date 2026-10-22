//! Three hosts fed by the trace-agent's proxies and the host profiler:
//!
//!   intake.profile.<site>    /api/v2/profile, /v1/input  → profiles
//!   debugger-intake.<site>   /api/v2/debugger            → debugger_logs, debugger_diagnostics, symdb_uploads
//!   sourcemap-intake.<site>  /api/v2/srcmap              → symbol_uploads
//!
//!   agent config: apm_config.profiling_dd_url, internal_profiling.profile_dd_url,
//!                 apm_config.debugger_diagnostics_dd_url, apm_config.symdb_dd_url,
//!                 profiling.symbol_uploader.symbol_endpoints[].site
//!
//! The trace-agent only forwards these bodies, so the agent has no type for any of them: the
//! attachments are pprof, the event parts JSON. A body that does not decode goes to raw_payloads.

use std::collections::BTreeMap;
use std::io::Read;

use bytes::Bytes;
use chrono::{DateTime, TimeDelta, Utc};
use ninjapaws_storage as storage;
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::Tags;
use ninjapaws_storage::rows::profiling::{DebuggerDiagnostic, DebuggerLog, Profile, SymbolUpload, SymdbUpload};
use serde_json::Value;
use tako::extractors::state::State;
use tako::types::Response;

use super::pprof::{self, Summary};
use crate::body;
use crate::json;
use crate::raw;
use crate::server::{Body, Ctx, accepted, storage_failed};
use crate::wire::{tags, text, time};

/// An epoch number as a time. Producers send seconds, milliseconds, microseconds or nanoseconds;
/// the magnitude says which, as it would to a person reading the number.
fn epoch_time(n: i64) -> DateTime<Utc> {
    if n >= 1_000_000_000_000_000_000 {
        time::from_unix_seconds(n / 1_000_000_000) + TimeDelta::nanoseconds(n % 1_000_000_000 / 100 * 100)
    } else if n >= 1_000_000_000_000_000 {
        time::from_unix_seconds(n / 1_000_000) + TimeDelta::microseconds(n % 1_000_000)
    } else if n >= 1_000_000_000_000 {
        time::from_unix_millis(n)
    } else {
        time::from_unix_seconds(n)
    }
}

/// A profiler event's start or end, or a diagnostic's timestamp: RFC 3339 text from Datadog's
/// profiler, an epoch number from other producers. None means "could not tell", never a guessed
/// time: the row keeps the original value next to this one.
pub fn parse_timestamp(value: Option<&Value>) -> Option<DateTime<Utc>> {
    match value? {
        Value::String(s) => time::try_rfc3339(s),
        number @ Value::Number(_) => json::int64_of(number).filter(|n| *n > 0).map(epoch_time),
        _ => None,
    }
}

/// The data inflated; None when it is not a whole gzip stream, or inflates past `limit` bytes: a
/// result is never cut off at the limit.
fn gunzip(limit: u64, data: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::new();
    flate2::read::MultiGzDecoder::new(data).take(limit.saturating_add(1)).read_to_end(&mut output).ok()?;
    // A stream that has bytes after its end must not pass for a whole one. A whole stream ends with
    // the size of what it inflates to.
    let declared_size = data
        .len()
        .checked_sub(4)
        .filter(|_| data.len() >= 18)
        .map(|at| u32::from_le_bytes(data[at..].try_into().unwrap()));
    (output.len() as u64 <= limit && declared_size == Some(output.len() as u32)).then_some(output)
}

/// The envelope of a pprof attachment, or None when the bytes are not pprof. Profilers send them
/// gzip-compressed.
pub fn read_pprof(data: &[u8]) -> Option<Summary> {
    if data.starts_with(&[0x1F, 0x8B]) {
        gunzip(u64::MAX, data).as_deref().and_then(pprof::read)
    } else {
        pprof::read(data)
    }
}

/// One profiles row from a multipart submission. `event` is the decoded "event" part (None when
/// absent or not a JSON object), `profiles` the attachments that are pprof, by part name. Every
/// part other than "event" becomes an attachment whether it parsed or not.
pub fn profile_row(
    tenant: &str,
    label: &str,
    origin: &str,
    origin_version: &str,
    parts: &BTreeMap<String, Bytes>,
    event: Option<&Value>,
    profiles: &BTreeMap<String, Summary>,
) -> Profile {
    // A BTreeMap lists its keys sorted, which is the order the columns promise.
    let attachments: Vec<(&String, &Bytes)> = parts.iter().filter(|(name, _)| *name != "event").collect();
    let parsed: Vec<Option<&Summary>> = attachments.iter().map(|(name, _)| profiles.get(*name)).collect();
    let not_pprof = Summary::default();
    let shapes: Vec<&Summary> = parsed.iter().map(|p| p.unwrap_or(&not_pprof)).collect();
    let field = |name| json::child(name, event);
    // start and end are also kept as written: the event part's text.
    let event_text = parts.get("event").map(|e| json::raw::text(e)).unwrap_or_default();
    let as_written = |name| json::raw::text_of(json::raw::member(name, &event_text));

    Profile {
        tenant_id: tenant.into(),
        received_at: Utc::now(),
        variant: label.into(),
        dd_evp_origin: origin.into(),
        dd_evp_origin_version: origin_version.into(),
        event: Blob(parts.get("event").map(|e| e.to_vec()).unwrap_or_default()),
        start_raw: as_written("start"),
        start_parsed: parse_timestamp(field("start")),
        end_raw: as_written("end"),
        end_parsed: parse_timestamp(field("end")),
        family: json::lenient_string(field("family")),
        version: json::lenient_string(field("version")),
        runtime: json::lenient_string(field("runtime")),
        language: json::lenient_string(field("language")),
        // A third tag convention: one comma-joined string.
        tags_profiler: tags::to_multi_map(tags::split_ddtags(&json::lenient_string(field("tags_profiler")))),
        attach_name: attachments.iter().map(|(name, _)| (*name).clone()).collect(),
        attach_bytes: attachments.iter().map(|(_, data)| Blob(data.to_vec())).collect(),
        attach_size: attachments.iter().map(|(_, data)| data.len() as u64).collect(),
        attach_parsed: parsed.iter().map(|p| text::flag(p.is_some())).collect(),
        attach_sample_types: shapes.iter().map(|s| s.sample_types.clone()).collect(),
        attach_sample_units: shapes.iter().map(|s| s.sample_units.clone()).collect(),
        attach_sample_count: shapes.iter().map(|s| s.sample_count).collect(),
        attach_time_nanos: shapes.iter().map(|s| s.time_nanos).collect(),
        attach_duration_nanos: shapes.iter().map(|s| s.duration_nanos).collect(),
        attach_period_type: shapes.iter().map(|s| s.period_type.clone()).collect(),
        attach_period: shapes.iter().map(|s| s.period).collect(),
        attach_mapping_count: shapes.iter().map(|s| s.mapping_count).collect(),
        attach_location_count: shapes.iter().map(|s| s.location_count).collect(),
        attach_function_count: shapes.iter().map(|s| s.function_count).collect(),
    }
}

fn or_unknown(content_type: &str) -> &str {
    if content_type.is_empty() { "(no content-type)" } else { content_type }
}

/// The parts of a multipart request by form name, or why it could not be read. A repeated name
/// keeps its last part; no producer here repeats one, so the warning is the alarm.
async fn read_parts(ctx: &Ctx, body: &Bytes, label: &str) -> Result<BTreeMap<String, Bytes>, String> {
    let mut by_name = BTreeMap::new();
    for part in body::try_parts(ctx.header("content-type"), body.clone()).await? {
        if by_name.contains_key(&part.name) {
            tracing::warn!("[{label}] part {} repeats, the later one replaces it", part.name);
        }
        by_name.insert(part.name, part.data);
    }
    Ok(by_name)
}

fn is_multipart_form(content_type: &str) -> bool {
    content_type.split(';').next().unwrap_or_default().trim().eq_ignore_ascii_case("multipart/form-data")
}

/// A part that should be a JSON object, decoded; a warning when it is not.
fn decode_object(what: &str, data: &[u8]) -> Option<Value> {
    let decoded = json::try_parse_object(data).map(Value::Object);
    if decoded.is_none() {
        tracing::warn!("[{what}] not a JSON object ({} B)", data.len());
    }
    decoded
}

/// POST of a profile; `label` names the path it came by and is stored as the row's variant.
pub async fn handle_profile(
    label: &'static str,
    ctx: Ctx,
    State(clickhouse): State<clickhouse::Client>,
    body: Body,
) -> Response {
    match read_parts(&ctx, &body, label).await {
        Err(problem) => {
            let note = format!("not multipart, content-type {}: {problem}", or_unknown(ctx.header("content-type")));
            if let Err(e) = raw::store(&clickhouse, &ctx, "profiling", "decode_error", &note, &body).await {
                return storage_failed(e);
            }
        }
        Ok(parts) => {
            // The event part carries the submission: which attachments follow, the interval they
            // cover, the tags of the process.
            let event = parts.get("event").and_then(|data| decode_object(&format!("{label} event"), data));
            let mut profiles = BTreeMap::new();
            for (name, data) in parts.iter().filter(|(name, _)| *name != "event") {
                match read_pprof(data) {
                    Some(summary) => {
                        profiles.insert(name.clone(), summary);
                    }
                    None => tracing::debug!("[{label}] {name} is not pprof ({} B)", data.len()),
                }
            }
            if let Err(e) = storage::insert(
                &clickhouse,
                &[profile_row(
                    ctx.tenant(),
                    label,
                    ctx.header("dd-evp-origin"),
                    ctx.header("dd-evp-origin-version"),
                    &parts,
                    event.as_ref(),
                    &profiles,
                )],
            )
            .await
            {
                return storage_failed(e);
            }
        }
    }
    accepted()
}

fn is_json_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n')
}

/// The bytes without the white space at either end: Unicode's white space, not ASCII's. Bytes
/// that are not UTF-8 stop the trimming where they stand.
fn trim_space(data: &[u8]) -> &[u8] {
    let lead = data.utf8_chunks().next().map_or(0, |chunk| chunk.valid().len() - chunk.valid().trim_start().len());
    let rest = &data[lead..];
    let trail = match rest.utf8_chunks().last() {
        Some(chunk) if chunk.invalid().is_empty() => chunk.valid().len() - chunk.valid().trim_end().len(),
        _ => 0,
    };
    &rest[..rest.len() - trail]
}

/// The entries of a Dynamic Instrumentation logs batch, in order; None when the body is neither a
/// JSON array nor NDJSON.
///
/// The agent's dyninst uploader and the tracers send an array, the browser SDK's debugger track
/// one object per line. Both reach this endpoint, so the first significant byte decides.
pub fn decode_logs(body: &[u8]) -> Option<Vec<&[u8]>> {
    let start = body.iter().position(|b| !is_json_space(*b))?;
    match body[start] {
        b'[' => json::try_parse_element_bytes(body).ok(),
        b'{' => Some(body[start..].split(|b| *b == b'\n').map(trim_space).filter(|line| !line.is_empty()).collect()),
        _ => None,
    }
}

/// The entry's top-level keys other than the two with a column of their own: there is no schema
/// for these entries, so everything else is "extra".
fn extra_keys(decoded: Option<&Value>) -> Vec<String> {
    let mut names: Vec<String> =
        json::lenient_members(decoded).into_keys().filter(|name| name != "service" && name != "ddsource").collect();
    names.sort();
    names.dedup();
    names
}

/// One probe status message as a row. `decoded` is None when that one entry is not a JSON object:
/// its columns stay empty, `message` still has its bytes.
///
/// The shape is the agent's uploader.DiagnosticMessage: service, ddsource, timestamp, and the
/// probe under debugger.diagnostics.
pub fn diagnostic_row(
    tenant: &str,
    arrival: DateTime<Utc>,
    tags: &Tags,
    message: &[u8],
    decoded: Option<&Value>,
) -> DebuggerDiagnostic {
    let diagnostics = json::child("diagnostics", json::child("debugger", decoded));
    let failure = json::child("exception", diagnostics).filter(|e| e.is_object());
    let in_failure = |name| failure.map(|_| json::lenient_string(json::child(name, failure)));

    DebuggerDiagnostic {
        tenant_id: tenant.into(),
        received_at: arrival,
        timestamp: parse_timestamp(json::child("timestamp", decoded)),
        service: json::lenient_string(json::child("service", decoded)),
        ddsource: json::lenient_string(json::child("ddsource", decoded)),
        ddtags: tags.clone(),
        runtime_id: json::lenient_string(json::child("runtimeId", diagnostics)),
        probe_id: json::lenient_string(json::child("probeId", diagnostics)),
        status: json::lenient_string(json::child("status", diagnostics)),
        probe_version: json::lenient_string(json::child("probeVersion", diagnostics)),
        exception_type: in_failure("type"),
        exception_message: in_failure("message"),
        message: Blob(message.to_vec()),
    }
}

/// What the file part of a symdb upload holds.
#[derive(Clone, Debug, Default)]
pub struct SymdbFile {
    /// The JSON envelope {service, version, language, upload_id, batch_num, scopes, final}; None
    /// when the part is not gzip or not a JSON object.
    pub envelope: Option<Value>,
    /// How many top-level scopes; None when "scopes" is missing or malformed, which the envelope
    /// survives.
    pub scope_count: Option<usize>,
    pub inflated_size: usize,
}

/// The most a symdb file may inflate to. The encoder flushes a batch at about 2 MiB compressed;
/// symbol JSON compresses well, but not this well.
const SYMDB_MAX_INFLATED: u64 = 256 * 1024 * 1024;

pub fn decode_symdb_file(file: &[u8]) -> SymdbFile {
    let Some(inflated) = gunzip(SYMDB_MAX_INFLATED, file) else {
        return SymdbFile::default();
    };
    let envelope = json::try_parse_object(&inflated).map(Value::Object);
    let scope_count = match json::child("scopes", envelope.as_ref()) {
        Some(Value::Array(scopes)) if scopes.iter().all(|s| s.is_object() || s.is_null()) => Some(scopes.len()),
        _ => None,
    };
    SymdbFile { envelope, scope_count, inflated_size: inflated.len() }
}

/// One symdb_uploads row. `event` is the event part as it arrived, `decoded` what it decoded to
/// (None when it is not a JSON object).
pub fn symdb_upload_row(
    tenant: &str,
    event: &[u8],
    file: &[u8],
    ddtags: &str,
    decoded: Option<&Value>,
    symdb: &SymdbFile,
) -> SymdbUpload {
    let field = |name| json::child(name, decoded);
    let in_envelope = |name| json::text(json::child(name, symdb.envelope.as_ref()));
    SymdbUpload {
        tenant_id: tenant.into(),
        received_at: Utc::now(),
        event: Blob(event.to_vec()),
        ddtags: tags::to_multi_map(tags::split_ddtags(ddtags)),
        service: json::lenient_string(field("service")),
        version: json::lenient_string(field("version")),
        language: json::lenient_string(field("language")),
        runtime_id: json::lenient_string(field("runtimeId")),
        // Identifiers, read as the text on the wire: a producer that sends uploadId as a bare
        // number must not lose it.
        upload_id: json::text(field("uploadId")),
        batch_num: json::text(field("batchNum")),
        r#final: json::flag(field("final")),
        attachment_size: field("attachmentSize").and_then(json::uint64_of),
        file: Blob(file.to_vec()),
        inflated_size: symdb.inflated_size as u64,
        scope_count: symdb.scope_count.unwrap_or(0) as u32,
        scopes_ok: text::flag(symdb.scope_count.is_some()),
        env_service: in_envelope("service"),
        env_version: in_envelope("version"),
        env_language: in_envelope("language"),
        env_upload_id: in_envelope("upload_id"),
        env_batch_num: in_envelope("batch_num"),
        env_final: in_envelope("final"),
    }
}

fn log_rows(ctx: &Ctx, ddtags: &str, entries: Vec<&[u8]>) -> Vec<DebuggerLog> {
    let tags = tags::to_multi_map(tags::split_ddtags(ddtags));
    let arrival = Utc::now();
    entries
        .into_iter()
        .map(|entry| {
            // Best effort: an entry that is not an object keeps its bytes and has no columns.
            let decoded = json::try_parse_object(entry).map(Value::Object);
            DebuggerLog {
                tenant_id: ctx.tenant().into(),
                received_at: arrival,
                service: json::lenient_string(json::child("service", decoded.as_ref())),
                ddsource: json::lenient_string(json::child("ddsource", decoded.as_ref())),
                ddtags: tags.clone(),
                entry: Blob(entry.to_vec()),
                extra_keys: extra_keys(decoded.as_ref()),
            }
        })
        .collect()
}

fn diagnostic_rows(ctx: &Ctx, ddtags: &str, messages: Vec<&[u8]>) -> Vec<DebuggerDiagnostic> {
    let tags = tags::to_multi_map(tags::split_ddtags(ddtags));
    let arrival = Utc::now();
    messages
        .into_iter()
        .enumerate()
        .map(|(i, message)| {
            let decoded = json::try_parse_object(message).map(Value::Object);
            if decoded.is_none() {
                tracing::warn!("[debugger] diagnostics: entry {i} is not a JSON object");
            }
            diagnostic_row(ctx.tenant(), arrival, &tags, message, decoded.as_ref())
        })
        .collect()
}

/// POST /api/v2/debugger: one path, three producers, told apart by Content-Type and part names.
///
///   not multipart          Dynamic Instrumentation snapshots and logs
///   multipart: event only  probe diagnostics, a JSON array
///   multipart: file+event  symbol database: gzip JSON, then its metadata
///
/// Each variant has a table of its own.
pub async fn handle_debugger(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    let content_type = ctx.header("content-type");
    let ddtags = ctx.query("ddtags");

    if !is_multipart_form(content_type) {
        match decode_logs(&body) {
            None => {
                let note =
                    format!("logs variant: not a JSON array or NDJSON, content-type {}", or_unknown(content_type));
                if let Err(e) = raw::store(&clickhouse, &ctx, "debugger", "decode_error", &note, &body).await {
                    return storage_failed(e);
                }
            }
            Some(entries) => {
                if let Err(e) = storage::insert(&clickhouse, &log_rows(&ctx, &ddtags, entries)).await {
                    return storage_failed(e);
                }
            }
        }
        return accepted();
    }
    let parts = match read_parts(&ctx, &body, "debugger").await {
        Ok(parts) => parts,
        Err(problem) => {
            let note = format!("multipart, content-type {}: {problem}", or_unknown(content_type));
            if let Err(e) = raw::store(&clickhouse, &ctx, "debugger", "decode_error", &note, &body).await {
                return storage_failed(e);
            }
            return accepted();
        }
    };
    match (parts.get("file"), parts.get("event")) {
        (Some(file), Some(event)) => {
            let decoded = decode_object("debugger symdb event", event);
            let symdb = decode_symdb_file(file);
            if symdb.envelope.is_none() {
                tracing::warn!("[debugger] symdb file is not a gzip-compressed JSON object ({} B)", file.len());
            }
            if let Err(e) = storage::insert(
                &clickhouse,
                &[symdb_upload_row(ctx.tenant(), event, file, &ddtags, decoded.as_ref(), &symdb)],
            )
            .await
            {
                return storage_failed(e);
            }
        }
        (None, Some(event)) => match json::try_parse_element_bytes(event) {
            // Not an array at all, unlike an empty batch: `[]` stores nothing and is not an error.
            Err(_) => {
                if let Err(e) = raw::store(
                    &clickhouse,
                    &ctx,
                    "debugger",
                    "decode_error",
                    "diagnostics variant: event part is not a JSON array",
                    &body,
                )
                .await
                {
                    return storage_failed(e);
                }
            }
            Ok(messages) => {
                if let Err(e) = storage::insert(&clickhouse, &diagnostic_rows(&ctx, &ddtags, messages)).await {
                    return storage_failed(e);
                }
            }
        },
        _ => {
            let names = parts.keys().map(String::as_str).collect::<Vec<_>>().join(",");
            let note = format!("multipart with neither file nor event parts: {names}");
            if let Err(e) = raw::store(&clickhouse, &ctx, "debugger", "unexpected_shape", &note, &body).await {
                return storage_failed(e);
            }
        }
    }
    accepted()
}

/// The class and byte order from the ELF ident bytes, without parsing the file. Both "" when the
/// bytes are not ELF.
pub fn elf_info(data: &[u8]) -> (&'static str, &'static str) {
    if data.len() < 6 || !data.starts_with(b"\x7FELF") {
        return ("", "");
    }
    let class = match data[4] {
        1 => "ELF32",
        2 => "ELF64",
        _ => "ELF?",
    };
    let endianness = match data[5] {
        1 => "LE",
        2 => "BE",
        _ => "?",
    };
    (class, endianness)
}

/// One symbol_uploads row. `meta` is the decoded event part (None when absent or not a JSON
/// object): the host profiler's symbolUploadRequestMetadata.
pub fn symbol_upload_row(tenant: &str, parts: &BTreeMap<String, Bytes>, meta: Option<&Value>) -> SymbolUpload {
    let elf = parts.get("elf_symbol_file");
    let (elf_class, elf_endianness) = elf.map_or(("", ""), |data| elf_info(data));
    let field = |name| json::lenient_string(json::child(name, meta));
    SymbolUpload {
        tenant_id: tenant.into(),
        received_at: Utc::now(),
        r#type: field("type"),
        arch: field("arch"),
        gnu_build_id: field("gnu_build_id"),
        go_build_id: field("go_build_id"),
        file_hash: field("file_hash"),
        symbol_source: field("symbol_source"),
        origin: field("origin"),
        origin_version: field("origin_version"),
        filename: field("filename"),
        meta: Blob(parts.get("event").map(|e| e.to_vec()).unwrap_or_default()),
        has_elf: text::flag(elf.is_some()),
        elf_class: elf_class.into(),
        elf_endianness: elf_endianness.into(),
        elf: Blob(elf.map(|e| e.to_vec()).unwrap_or_default()),
        elf_size: elf.map_or(0, |e| e.len() as u64),
        // A producer that sends a part this handler does not name still shows.
        other_parts: parts
            .iter()
            .filter(|(name, _)| *name != "event" && *name != "elf_symbol_file")
            .map(|(name, data)| (name.clone(), Blob(data.to_vec())))
            .collect(),
    }
}

/// POST /api/v2/srcmap: an ELF symbol file (elf_symbol_file) and its metadata (event), usually
/// zstd-compressed as a whole. The agent sends it only after /api/v2/profiles/symbols/query said
/// the symbols are missing.
pub async fn handle_sourcemap(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    match read_parts(&ctx, &body, "srcmap").await {
        Err(problem) => {
            let note = format!("multipart, content-type {}: {problem}", or_unknown(ctx.header("content-type")));
            if let Err(e) = raw::store(&clickhouse, &ctx, "srcmap", "decode_error", &note, &body).await {
                return storage_failed(e);
            }
        }
        Ok(parts) => {
            let meta = parts.get("event").and_then(|data| decode_object("srcmap event", data));
            if let Err(e) =
                storage::insert(&clickhouse, &[symbol_upload_row(ctx.tenant(), &parts, meta.as_ref())]).await
            {
                return storage_failed(e);
            }
        }
    }
    accepted()
}
