//! What cannot be decoded is kept, not dropped.

use std::collections::BTreeMap;

use chrono::Utc;
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::raw::RawPayload;

use crate::json;
use crate::server::{Ctx, query_pairs};

/// Request headers kept with a raw payload. An allowlist: a blocklist is one forgotten header
/// away from writing Dd-Api-Key into a table. These identify the sender; none is a secret.
const KEPT_HEADERS: [&str; 14] = [
    "Via",
    "User-Agent",
    "Dd-Evp-Origin",
    "Dd-Evp-Origin-Version",
    "Dd-Request-Id",
    "Dd-Agent-Hostname",
    "Dd-Agent-Env",
    "Datadog-Container-Id",
    "X-Datadog-Additional-Tags",
    "X-Datadog-Container-Tags",
    "X-Requested-With",
    "X-Dd-Hostname",
    "X-Dd-Processagentversion",
    "X-Dd-Request-Id",
];

/// The query parameters that carry a key.
pub const KEY_PARAMETERS: [&str; 2] = ["api_key", "dd-api-key"];

/// The path without a key in it: older log clients put theirs in /v1/input/<key>.
pub fn path_without_key(path: &str) -> String {
    let lower = path.to_ascii_lowercase();
    if lower.starts_with("/v1/input/") { "/v1/input/<key>".into() } else { path.into() }
}

/// The agent probes every intake at startup with nothing, `{}` or `[]`.
pub fn is_probe(body: &[u8]) -> bool {
    let text = String::from_utf8_lossy(body);
    matches!(text.trim(), "" | "{}" | "[]")
}

/// The note of a payload with no published schema: the event platform's origin headers and the
/// Content-Type are all the context there is beside the opaque bytes.
pub fn origin_note(ctx: &Ctx) -> String {
    let header = |name| json::quoted(ctx.header(name));
    format!(
        "DD-EVP-ORIGIN={} DD-EVP-ORIGIN-VERSION={} Content-Type={}",
        header("dd-evp-origin"),
        header("dd-evp-origin-version"),
        header("content-type")
    )
}

/// Keeps a request that could not be turned into rows.
///
/// The rule of this project: a handler that cannot decode a body, or gets a shape it has no
/// schema for, hands the bytes here. A payload that would show what an intake sends arrives once,
/// from someone's real agent; keeping it turns reverse-engineering into a SELECT. Bodies that did
/// become rows are never stored here as well.
///
///   intake  the endpoint family ("logs", "trace", "rum")
///   reason  "no_schema" | "decode_error" | "unexpected_shape"
///   note    the decoder's error, the field that was missing
///   body    what the handler tried to decode, already decompressed
pub async fn store(clickhouse: &clickhouse::Client, ctx: &Ctx, intake: &str, reason: &str, note: &str, body: &[u8]) {
    let path = path_without_key(ctx.path());
    if ctx.tenant().is_empty() {
        tracing::warn!("[raw] {intake} {path}: no tenant, payload dropped ({} B)", body.len());
        return;
    }
    if is_probe(body) {
        tracing::debug!("[raw] {intake} {path}: empty probe body, not stored");
        return;
    }

    let headers = KEPT_HEADERS
        .iter()
        .filter_map(|name| match ctx.header(name) {
            "" => None,
            value => Some((name.to_string(), value.to_string())),
        })
        .collect();

    // Without the key: a table is no place for a credential.
    let mut query: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (key, value) in query_pairs(&ctx.parts.uri) {
        if !KEY_PARAMETERS.contains(&key.to_lowercase().as_str()) {
            query.entry(key).or_default().push(value);
        }
    }

    let row = RawPayload {
        tenant_id: ctx.tenant().into(),
        received_at: Utc::now(),
        intake: intake.into(),
        reason: reason.into(),
        method: ctx.parts.method.to_string(),
        // The Host header is what a host's process was chosen by.
        host: match ctx.header("host") {
            "" => ctx.parts.uri.authority().map(|a| a.to_string()).unwrap_or_default(),
            host => host.into(),
        },
        path: path.clone(),
        query,
        content_type: ctx.header("content-type").into(),
        content_encoding: ctx.header("content-encoding").into(),
        headers,
        body: Blob(body.to_vec()),
        body_bytes: body.len() as u64,
        note: note.into(),
    };
    if let Err(e) = ninjapaws_storage::insert(clickhouse, &[row]).await {
        tracing::warn!("[raw] {intake} {path}: {reason}: {note} ({} B), not kept: {e}", body.len());
        return;
    }

    // The one line about a kept payload; handlers do not log it again. An intake with no schema
    // keeps everything it gets, which is its ordinary work and not worth a warning each time.
    if reason == "no_schema" {
        tracing::debug!("[raw] {intake} {path}: {reason}: {note} ({} B)", body.len());
    } else {
        tracing::warn!("[raw] {intake} {path}: {reason}: {note} ({} B)", body.len());
    }
}
