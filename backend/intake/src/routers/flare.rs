//! `agent flare` uploads: POST /support/flare, with or without a case id in the path. The HEAD
//! that precedes one is answered with the probes.
//!
//! Served on every host that asks for a key, because the agent sends a flare to whichever host
//! its dd_url resolves to, not to a dedicated one.

use chrono::Utc;
use http::StatusCode;
use ninjapaws_storage as storage;
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::flares::AgentFlare;
use tako::extractors::state::State;
use tako::types::Response;

use crate::body;
use crate::raw;
use crate::server::{Body, Ctx, json, storage_failed};
use crate::wire::text;

/// The label of flare uploads in raw_payloads.
const INTAKE: &str = "flare";

/// Form fields with a column of their own; the rest go to `fields`.
const KNOWN_FIELDS: [&str; 5] = ["case_id", "email", "source", "agent_version", "hostname"];

/// What the agent's uploader parses the answer into. It needs all three keys and a 200, or it
/// reports "could not deserialize response body" and retries.
fn answer(case_id: i64, error: &str) -> Response {
    json(
        StatusCode::OK,
        &serde_json::json!({ "case_id": case_id, "error": error, "request_uuid": uuid::Uuid::new_v4().to_string() }),
    )
}

/// The number shown to the operator: the case id the agent sent, or for a new flare one minted
/// from a UUID (63 bits, so it fits an i64).
fn case_number(case_id: &str) -> i64 {
    case_id.parse().unwrap_or_else(|_| (uuid::Uuid::new_v4().as_u64_pair().0 >> 1) as i64)
}

pub async fn handle(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    // The case id from the path, "" for a new flare.
    let path_case_id = ctx.path().strip_prefix("/support/flare/").unwrap_or_default().to_string();

    let Some(boundary) = body::boundary(ctx.header("content-type")) else {
        // Refusing would only make the agent retry the same upload, so the answer keeps its
        // shape and carries the error.
        raw::store(&clickhouse, &ctx, INTAKE, "decode_error", "content-type is not multipart/form-data", &body).await;
        return answer(0, "content-type is not multipart/form-data");
    };
    let parts = body::parts(&boundary, body.clone()).await;

    // The agent writes agent_version and hostname AFTER the archive, so nothing here may depend
    // on the order of parts.
    let Some(archive) = parts.iter().rev().find(|p| p.name == "flare_file") else {
        raw::store(&clickhouse, &ctx, INTAKE, "decode_error", "no flare_file part in the multipart body", &body).await;
        return answer(0, "no flare archive in the request");
    };
    let field =
        |name: &str| parts.iter().rev().find(|p| p.name == name).map(|p| text::utf8(&p.data)).unwrap_or_default();
    let other_fields = parts
        .iter()
        .filter(|p| !p.name.is_empty() && p.name != "flare_file" && !KNOWN_FIELDS.contains(&p.name.as_str()))
        .map(|p| (p.name.clone(), text::utf8(&p.data)))
        .collect();

    // A flare for an existing case carries its id in the path and in the form; the form wins if
    // they ever disagree.
    let form_case_id = field("case_id");
    let case_id = text::first_non_empty(&[&form_case_id, &path_case_id]).to_string();

    let row = AgentFlare {
        tenant_id: ctx.tenant().into(),
        received_at: Utc::now(),
        hostname: field("hostname"),
        case_id: case_id.clone(),
        email: field("email"),
        source: field("source"),
        agent_version: field("agent_version"),
        filename: archive.file_name.clone(),
        size_bytes: archive.data.len() as u64,
        fields: other_fields,
        archive: Blob(archive.data.to_vec()),
    };
    if let Err(e) = storage::insert(&clickhouse, &[row]).await {
        return storage_failed(e);
    }

    answer(case_number(&case_id), "")
}
