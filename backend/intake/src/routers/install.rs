//! Hosts that do not follow the prefix + site scheme.
//!
//!   install.datadoghq.com / install.datad0g.com   OCI registry, BTF files, HEAD probe
//!   eudm-intake.<site>                            /api/v2/aiusage
//!   llmobs-intake.<site>                          /api/v2/llmobs (diagnose probe only)
//!
//! The install host stores nothing: every answer is a fixed protocol error or an empty ack.
//! aiusage and llmobs have no published schema, so a real payload on either goes to raw_payloads
//! as "no_schema".

use http::{HeaderValue, StatusCode};
use tako::extractors::state::State;
use tako::types::Response;

use crate::raw;
use crate::server::{Body, Ctx, accepted, json, status};

/// Marks an answer as coming from a Distribution API v2 registry.
fn from_registry(mut response: Response) -> Response {
    response.headers_mut().insert("docker-distribution-api-version", HeaderValue::from_static("registry/2.0"));
    response
}

/// The fleet installer speaks OCI Distribution, and no packages are hosted here. The protocol's
/// own "not found" — 404 with an OCI error body — is not a network error, so the installer fails
/// at once with a readable message instead of retrying. go-containerregistry parses `code`.
fn oci_error(code: &str, message: &str) -> Response {
    from_registry(json(StatusCode::NOT_FOUND, &serde_json::json!({ "errors": [{ "code": code, "message": message }] })))
}

/// The two names after /v2/ in `/v2/<repo>/<kind>/<name>`.
fn repo_and_name(ctx: &Ctx) -> (String, String) {
    let segments: Vec<&str> = ctx.path().trim_start_matches("/v2/").split('/').collect();
    (segments.first().unwrap_or(&"").to_string(), segments.get(2).unwrap_or(&"").to_string())
}

/// The diagnose sweep's HEAD /: any 2xx is "Success".
pub async fn handle_probe(_: Ctx, _: Body) -> Response {
    status(StatusCode::OK)
}

/// The API version check every registry client starts with.
pub async fn handle_version_check(_: Ctx, _: Body) -> Response {
    from_registry(json(StatusCode::OK, &serde_json::json!({})))
}

pub async fn handle_manifest(ctx: Ctx, _: Body) -> Response {
    let (repo, tag) = repo_and_name(&ctx);
    oci_error("MANIFEST_UNKNOWN", &format!("ninjacat does not host packages: {repo}:{tag}"))
}

pub async fn handle_blob(ctx: Ctx, _: Body) -> Response {
    let (repo, digest) = repo_and_name(&ctx);
    oci_error("BLOB_UNKNOWN", &format!("ninjacat does not host packages: {repo}@{digest}"))
}

/// system-probe fetching a kernel BTF archive. Its SHA256 is checked against the BTF_DD remote
/// config catalog, so no substitute could be served; a non-200 makes the loader fall back to its
/// next BTF source.
pub async fn handle_btf(_: Ctx, _: Body) -> Response {
    json(StatusCode::NOT_FOUND, &serde_json::json!({ "errors": ["BTF archives are not hosted here"] }))
}

/// The Rust ai_prompt_logger posts through the local evp_proxy, which adds the API key; it only
/// checks for a 2xx. The producer is not in datadog-agent and publishes no schema, so the body is
/// kept as it arrived.
pub async fn handle_ai_usage(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    raw::store(&clickhouse, &ctx, "aiusage", "no_schema", &raw::origin_note(&ctx), &body).await;
    accepted()
}

/// The agent never sends LLM Observability data here; the only caller is the connectivity
/// diagnose, which posts no body and wants a 2xx.
pub async fn handle_llm_obs(ctx: Ctx, State(clickhouse): State<clickhouse::Client>, body: Body) -> Response {
    if !body.is_empty() && !ctx.is_sweep() {
        let note = format!("unexpected payload — the agent only probes this host; {}", raw::origin_note(&ctx));
        raw::store(&clickhouse, &ctx, "llmobs", "no_schema", &note, &body).await;
    }
    accepted()
}
