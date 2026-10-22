//! Datadog API keys as a route middleware: `.middleware(auth::agent)` on a route. A request whose
//! key is known goes on as that key, with it in its extensions; any other is answered 403, which
//! is what the agent understands as "bad key, stop sending".
//!
//! One function per place a host's clients put the key; the first place that holds one decides.

use std::sync::Arc;

use http::StatusCode;
use tako::middleware::Next;
use tako::types::{Request, Response};

use crate::keys::{Key, Store};
use crate::server::{json, query_value, state};

/// The key a request was admitted with, in its extensions.
#[derive(Clone, Debug)]
pub struct Admitted(pub Key);

/// Agents and tracers: the Dd-Api-Key header, then the api_key query parameter.
///
/// The query parameter is there for GET /api/v1/validate, which the agent calls with the key in
/// the URL; refusing it makes the agent mark itself unhealthy. A key in a URL leaks into access
/// logs, so no other query parameter is accepted unless an intake's clients leave no choice.
pub async fn agent(req: Request, next: Next) -> Response {
    let key = header(&req, "dd-api-key").or_else(|| query_value(req.uri(), "api_key"));
    admit(key, req, next).await
}

/// browser-intake: the header (iOS, Android) or the query string (browser). The browser has no
/// choice: its fetch sets no headers at all, to stay a "simple request" and interchangeable with
/// sendBeacon. The one request the browser SDK does put a header on, the profiler's quota check,
/// names it DD-CLIENT-TOKEN.
pub async fn browser(req: Request, next: Next) -> Response {
    let key = header(&req, "dd-api-key")
        .or_else(|| query_value(req.uri(), "dd-api-key"))
        .or_else(|| header(&req, "dd-client-token"));
    admit(key, req, next).await
}

/// webhook-intake: Jenkins sends the header, but a GitLab project webhook has no field for one,
/// so the key travels as `?dd-api-key=`.
pub async fn ci_webhook(req: Request, next: Next) -> Response {
    let key = header(&req, "dd-api-key")
        .or_else(|| query_value(req.uri(), "dd-api-key"))
        .or_else(|| query_value(req.uri(), "api_key"));
    admit(key, req, next).await
}

/// data-obs-intake: OpenLineage's transport sends `Authorization: Bearer`, and the trace-agent's
/// lineage proxy forwards it unchanged.
pub async fn lineage(req: Request, next: Next) -> Response {
    let key = header(&req, "dd-api-key").or_else(|| bearer(&req)).or_else(|| query_value(req.uri(), "api_key"));
    admit(key, req, next).await
}

async fn admit(key: Option<String>, mut req: Request, next: Next) -> Response {
    let known = match (key, state::<Arc<Store>>(req.extensions())) {
        (Some(key), Some(keys)) => keys.lookup(&key),
        _ => None,
    };
    match known {
        Some(known) => {
            req.extensions_mut().insert(Admitted(known));
            next.run(req).await
        }
        None => json(StatusCode::FORBIDDEN, &serde_json::json!({ "errors": ["invalid API key"] })),
    }
}

fn header(req: &Request, name: &str) -> Option<String> {
    let value = req.headers().get(name)?.to_str().ok()?;
    (!value.is_empty()).then(|| value.to_string())
}

/// `Authorization: Bearer <key>`. Anything that is not a Bearer credential is no key, so the next
/// place gets its turn.
fn bearer(req: &Request) -> Option<String> {
    let value = header(req, "authorization")?;
    let scheme = value.get(..7)?;
    let key = value[7..].trim();
    (scheme.eq_ignore_ascii_case("Bearer ") && !key.is_empty()).then(|| key.to_string())
}
