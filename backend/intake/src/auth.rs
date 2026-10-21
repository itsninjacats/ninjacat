//! Datadog API keys as a route middleware. A request whose key is known goes on as that key,
//! with it in its extensions; any other is answered 403, which is what the agent understands as
//! "bad key, stop sending".

use std::sync::Arc;

use http::StatusCode;
use tako::middleware::Next;
use tako::types::{Request, Response};

use crate::keys::{Key, Store};
use crate::server::{json, query_value};

/// Where a request may carry its key.
enum Source {
    Header(&'static str),
    /// `Authorization: Bearer <key>`.
    Bearer,
    Query(&'static str),
}

/// Where a host's clients put the key. The first source that offers one decides.
#[derive(Clone, Copy, Debug)]
pub enum Scheme {
    /// The Dd-Api-Key header, then the api_key query parameter.
    ///
    /// The query parameter is there for GET /api/v1/validate, which the agent calls with the key
    /// in the URL; refusing it makes the agent mark itself unhealthy. A key in a URL leaks into
    /// access logs, so no other query parameter is accepted unless an intake's clients leave no
    /// choice.
    Agent,
    /// browser-intake: the header (iOS, Android) or the query string (browser). The browser has
    /// no choice: its fetch sets no headers at all, to stay a "simple request" and
    /// interchangeable with sendBeacon. The one request the browser SDK does put a header on, the
    /// profiler's quota check, names it DD-CLIENT-TOKEN.
    Browser,
    /// webhook-intake: Jenkins sends the header, but a GitLab project webhook has no field for
    /// one, so the key travels as `?dd-api-key=`.
    CiWebhook,
    /// data-obs-intake: OpenLineage's transport sends `Authorization: Bearer`, and the
    /// trace-agent's lineage proxy forwards it unchanged.
    Lineage,
}

impl Scheme {
    fn sources(self) -> &'static [Source] {
        match self {
            Scheme::Agent => &[Source::Header("dd-api-key"), Source::Query("api_key")],
            Scheme::Browser => {
                &[Source::Header("dd-api-key"), Source::Query("dd-api-key"), Source::Header("dd-client-token")]
            }
            Scheme::CiWebhook => &[Source::Header("dd-api-key"), Source::Query("dd-api-key"), Source::Query("api_key")],
            Scheme::Lineage => &[Source::Header("dd-api-key"), Source::Bearer, Source::Query("api_key")],
        }
    }
}

/// The key a request was admitted with, in its extensions.
#[derive(Clone, Debug)]
pub struct Admitted(pub Key);

fn candidate(source: &Source, req: &Request) -> String {
    let header = |name: &str| req.headers().get(name).and_then(|v| v.to_str().ok()).unwrap_or_default().to_string();
    match source {
        Source::Header(name) => header(name),
        Source::Query(name) => query_value(req.uri(), name),
        // Anything that is not a Bearer credential yields nothing, so the next source gets its
        // turn.
        Source::Bearer => {
            let value = header("authorization");
            match value.get(..7) {
                Some(scheme) if scheme.eq_ignore_ascii_case("Bearer ") => value[7..].trim().to_string(),
                _ => String::new(),
            }
        }
    }
}

pub async fn guard(keys: Arc<Store>, scheme: Scheme, mut req: Request, next: Next) -> Response {
    let key = scheme.sources().iter().map(|source| candidate(source, &req)).find(|found| !found.is_empty());
    match key.and_then(|key| keys.lookup(&key)) {
        Some(known) => {
            req.extensions_mut().insert(Admitted(known));
            next.run(req).await
        }
        None => json(StatusCode::FORBIDDEN, &serde_json::json!({ "errors": ["invalid API key"] })),
    }
}
