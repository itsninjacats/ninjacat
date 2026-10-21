//! config.<site> — remote configuration.
//!
//!   agent config: DD_REMOTE_CONFIGURATION_ENABLED (on/off only, no dd_url)
//!
//! The only intake where data flows DOWN: the agent polls it and applies what comes back —
//! integration configs, sampling rates, and in Kubernetes the actions the cluster agent then
//! performs. The agent verifies the answer against a TUF root, so nothing can be served before
//! signing exists (docs/protocol/zadania/remote-config-tuf.md). Until then 404 is the honest answer:
//! the agent keeps the configuration it has instead of acting on an empty one.
//!
//!   POST /api/v0.1/configurations  LatestConfigsRequest → LatestConfigsResponse
//!   GET  /api/v0.1/org             (no body)            → OrgDataResponse
//!   GET  /api/v0.1/status          (no body)            → OrgStatusResponse

use bytes::Bytes;
use http::StatusCode;
use ninjapaws_proto::datadog::config::LatestConfigsRequest;
use prost::Message;
use tako::types::Response;

use crate::server::{Ctx, Routes, json};

pub fn mount(routes: &mut Routes) {
    routes.post("/api/v0.1/configurations", handle_configurations);
    // The agent sends GET; POST is registered so anything else that tries gets the same explicit
    // answer.
    routes.post("/api/v0.1/org", not_served);
    routes.post("/api/v0.1/status", not_served);
    routes.get("/api/v0.1/org", not_served);
    routes.get("/api/v0.1/status", not_served);
}

/// The poll for new configuration. The request is the agent's full picture of itself: identity,
/// the TUF versions it holds, the products it wants and every client registered with it. Nothing
/// of it is stored yet.
pub async fn handle_configurations(_: Ctx, body: Bytes) -> Response {
    match LatestConfigsRequest::decode(body.as_ref()) {
        Ok(request)
            if request.hostname.is_empty() && request.products.is_empty() && request.active_clients.is_empty() =>
        {
            // A protobuf parser fails OPEN: a payload meant for another endpoint decodes into an
            // empty message.
            tracing::warn!("[remote-config] decoded to an empty request ({} bytes) — wrong payload type?", body.len());
        }
        // The agent could not apply what it got last time. Worth hearing even while nothing is
        // served.
        Ok(request) if request.has_error => tracing::warn!("[remote-config] agent reports error: {}", request.error),
        Ok(_) => {}
        Err(e) => tracing::warn!("[remote-config] protobuf: {e} ({} bytes)", body.len()),
    }
    json(StatusCode::NOT_FOUND, &serde_json::json!({ "error": "remote configuration not served" }))
}

/// The org identity lookup and the org/key status check: a GET with no body.
pub async fn not_served(_: Ctx, _: Bytes) -> Response {
    json(StatusCode::NOT_FOUND, &serde_json::json!({ "error": "not served" }))
}
