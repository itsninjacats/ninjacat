//! What every intake program shares. A program's `main` reads the environment with [`start`],
//! registers its routes on a tako `Router` (each behind its API key, `auth`), puts the [`Intake`]
//! in the router's state and [`serve`]s it.
//!
//! Which host a request was sent to is not looked at: each host is a process of its own, and
//! whatever is in front of them (an ingress, a proxy) sends each host name to its process.

use std::env;
use std::future::Future;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use bytes::Bytes;
use http::request::Parts;
use http::{HeaderValue, StatusCode, Uri};
use http_body_util::{BodyExt, Limited};
use ninjapaws_storage::{Sink, Table};
use serde::Serialize;
use tako::Server;
use tako::body::TakoBody;
use tako::extractors::ipaddr::IpAddr;
use tako::extractors::{FromRequest, FromRequestParts};
use tako::middleware::Next;
use tako::router::Router;
use tako::router_state::RouterState;
use tako::types::{BoxError, Request, Response};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::auth::Admitted;
use crate::capture;
use crate::keys::{self, Store};
use crate::raw;

/// What a program has besides its routes: what its handlers write to and read from, and its
/// settings.
#[derive(Clone)]
pub struct Intake {
    pub sink: Sink,
    pub keys: Arc<Store>,
    /// Answer 202 instead of 404 on unknown paths (NINJACAT_ACK_UNKNOWN): keeps a chatty agent
    /// quiet while a new endpoint is looked at.
    pub ack_unknown: bool,
    /// The largest request body read (NINJACAT_MAX_BODY_BYTES); a larger one is refused with 413.
    pub max_body_bytes: usize,
    /// Cancelled once the HTTP listener has stopped. The tasks in `tasks` (the logs over TCP) end
    /// on it, and the writers flush only after every one of them has.
    pub stopping: CancellationToken,
    pub tasks: TaskTracker,
}

/// One request as a handler sees it: everything but the body, which it is given apart
/// ([`Body`]).
pub struct Ctx {
    pub parts: http::request::Parts,
    pub intake: Arc<Intake>,
}

impl Ctx {
    /// The tenant of the API key the request was admitted with, or "" on a route that asks for
    /// none.
    pub fn tenant(&self) -> &str {
        self.parts.extensions.get::<Admitted>().map_or("", |admitted| admitted.0.tenant_id.as_str())
    }

    /// The id of the key the request was admitted with.
    pub fn key_id(&self) -> Option<&str> {
        self.parts.extensions.get::<Admitted>().map(|admitted| admitted.0.id.as_str())
    }

    /// The header's first value, or "".
    pub fn header(&self, name: &str) -> &str {
        self.parts.headers.get(name).and_then(|v| v.to_str().ok()).unwrap_or_default()
    }

    /// The query parameter's first value, or "".
    pub fn query(&self, name: &str) -> String {
        query_value(&self.parts.uri, name).unwrap_or_default()
    }

    pub fn path(&self) -> &str {
        self.parts.uri.path()
    }

    /// Hands rows to the table's writer. A request with no tenant (a route that asks for no key)
    /// writes nothing: a row belongs to a tenant.
    pub fn write<T: Table>(&self, rows: Vec<T>) {
        if !self.tenant().is_empty() {
            self.intake.sink.write(rows);
        }
    }

    /// Keeps a request that could not be turned into rows (see [`raw::store`]).
    pub fn raw(&self, intake: &str, reason: &str, note: &str, body: &[u8]) {
        raw::store(self, intake, reason, note, body);
    }

    /// The agent's connectivity sweep at startup: an empty body or `{}`, nothing to decode.
    pub fn is_sweep(&self) -> bool {
        self.header("x-requested-with") == "datadog-agent-diagnose"
    }

    /// The sender's address: the peer, or, where the request carries the proxies its host trusts
    /// (an `IpAddrConfig` extension, as browser-intake puts there), the address such a proxy
    /// wrote in X-Forwarded-For. No other header is believed. "" when the transport has no IP
    /// peer.
    pub fn client_ip(&self) -> String {
        let mut forwarded = http::HeaderMap::new();
        for value in self.parts.headers.get_all("x-forwarded-for") {
            forwarded.append("x-forwarded-for", value.clone());
        }
        match IpAddr::resolve(&self.parts.extensions, &forwarded) {
            Ok(IpAddr(ip)) => ip.to_canonical().to_string(),
            Err(_) => String::new(),
        }
    }
}

/// Every value of a URL's query, decoded, in order.
pub fn query_pairs(uri: &Uri) -> Vec<(String, String)> {
    form_urlencoded::parse(uri.query().unwrap_or_default().as_bytes()).into_owned().collect()
}

/// A query parameter's first value; None when it is absent or empty.
pub fn query_value(uri: &Uri, name: &str) -> Option<String> {
    query_pairs(uri).into_iter().find(|(key, _)| key == name).map(|(_, value)| value).filter(|value| !value.is_empty())
}

/// JSON with the content type the agent has always been answered with.
pub fn json<T: Serialize + ?Sized>(status: StatusCode, value: &T) -> Response {
    json_as(status, "application/json; charset=utf-8", value)
}

pub fn json_as<T: Serialize + ?Sized>(status: StatusCode, content_type: &'static str, value: &T) -> Response {
    let mut response = http::Response::new(TakoBody::from(serde_json::to_vec(value).unwrap()));
    *response.status_mut() = status;
    response.headers_mut().insert(http::header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response
}

/// A status and no body.
pub fn status(status: StatusCode) -> Response {
    let mut response = http::Response::new(TakoBody::empty());
    *response.status_mut() = status;
    response
}

/// What Datadog's intakes answer once a body has arrived, whatever became of it: 202 and an
/// empty object.
pub fn accepted() -> Response {
    json(StatusCode::ACCEPTED, &serde_json::json!({}))
}

/// A request's body, read whole (up to NINJACAT_MAX_BODY_BYTES, 413 past it) with its
/// Content-Encoding undone. The last argument of every handler.
pub struct Body(pub Bytes);

impl Deref for Body {
    type Target = Bytes;

    fn deref(&self) -> &Bytes {
        &self.0
    }
}

impl<'a> FromRequest<'a> for Body {
    type Error = Response;

    async fn from_request(req: &'a mut Request) -> Result<Self, Response> {
        let limit = intake_of(req.extensions()).map_or(0, |intake| intake.max_body_bytes);
        let raw = match Limited::new(std::mem::take(req.body_mut()), limit).collect().await {
            Ok(collected) => collected.to_bytes(),
            Err(_) => return Err(status(StatusCode::PAYLOAD_TOO_LARGE)),
        };
        let encoding =
            req.headers().get(http::header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()).unwrap_or_default();
        Ok(Body(crate::body::decompress(encoding, raw)))
    }
}

impl<'a> FromRequestParts<'a> for Ctx {
    type Error = Response;

    fn from_request_parts(parts: &'a mut Parts) -> impl Future<Output = Result<Self, Response>> + Send + 'a {
        // The parts are copied, not taken: the body's extractor, which runs next, reads the limit
        // and the encoding from them.
        let ctx = intake_of(&parts.extensions).map(|intake| {
            let (mut copy, _) = http::Request::new(()).into_parts();
            copy.method = parts.method.clone();
            copy.uri = parts.uri.clone();
            copy.version = parts.version;
            copy.headers = parts.headers.clone();
            copy.extensions = parts.extensions.clone();
            Ctx { parts: copy, intake }
        });
        std::future::ready(ctx.ok_or_else(|| status(StatusCode::INTERNAL_SERVER_ERROR)))
    }
}

/// The program's [`Intake`], which its `main` puts in the router's state (`with_state`).
pub fn intake_of(extensions: &http::Extensions) -> Option<Arc<Intake>> {
    extensions.get::<Arc<RouterState>>()?.get::<Intake>()
}

/// GET /api/v1/validate, which the agent sweeps at startup on whatever host it talks to.
pub async fn validate() -> Response {
    json(StatusCode::OK, &serde_json::json!({ "valid": true }))
}

/// GET /ping, on every host, without a key.
pub async fn ping() -> Response {
    json(StatusCode::OK, &serde_json::json!({ "message": "pong" }))
}

/// GET /_health, on every host, without a key.
pub async fn health() -> Response {
    json(StatusCode::OK, &serde_json::json!({ "status": "ok" }))
}

/// HEAD /support/flare, on every host, without a key: an agent sends it to learn whether a flare
/// could be uploaded. Its connectivity check reports anything but 200 or a redirect as a failure
/// (pkg/diagnose/connectivity, agent 7.84.0).
pub async fn flare_probe() -> Response {
    status(StatusCode::OK)
}

/// The answer for a path nothing serves, logged: the log line is how unhandled endpoints are
/// found.
pub async fn not_found(req: Request) -> Response {
    log_unhandled(&unhandled(&req));
    if intake_of(req.extensions()).is_some_and(|intake| intake.ack_unknown) {
        accepted()
    } else {
        json(StatusCode::NOT_FOUND, &serde_json::json!({ "errors": ["unknown endpoint"] }))
    }
}

/// A router middleware: a known path with the wrong method is answered by the router itself, with
/// 405. It is as unhandled as an unknown path, so it is logged too.
pub async fn log_405(req: Request, next: Next) -> Response {
    let line = unhandled(&req);
    let response = next.run(req).await;
    if response.status() == StatusCode::METHOD_NOT_ALLOWED {
        log_unhandled(&line);
    }
    response
}

fn unhandled(req: &Request) -> String {
    let header = |name| req.headers().get(name).and_then(|v: &HeaderValue| v.to_str().ok()).unwrap_or_default();
    format!(
        "{} {} (Content-Type: {}, User-Agent: {})",
        req.method(),
        raw::path_without_key(req.uri().path()),
        header(http::header::CONTENT_TYPE),
        header(http::header::USER_AGENT)
    )
}

fn log_unhandled(line: &str) {
    tracing::warn!("UNHANDLED ENDPOINT: {line}");
}

/// An environment variable, or `default` when it is unset or empty.
pub fn var_or(name: &str, default: &str) -> String {
    env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

/// `:8080`, `*:8080` and `0.0.0.0:8080` are every interface, as the settings have always spelled it.
pub fn listen_on(addr: &str) -> String {
    match addr.rsplit_once(':') {
        Some(("" | "*", port)) => format!("0.0.0.0:{port}"),
        _ => addr.to_string(),
    }
}

/// Reads the settings, the API keys and ClickHouse from the environment.
pub async fn start() -> Result<Intake, BoxError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let database_url = var_or("DATABASE_URL", "");
    if database_url.is_empty() {
        return Err("DATABASE_URL is not set (e.g. postgres://root:mysecretpassword@localhost:5433/local)".into());
    }
    let keys = Arc::new(Store::default());
    keys::fetch_now(&keys, &database_url).await?;
    tokio::spawn(keys::keep(keys.clone(), database_url));

    Ok(Intake {
        keys,
        sink: Sink::clickhouse(ninjapaws_storage::client_from_env()),
        stopping: CancellationToken::new(),
        tasks: TaskTracker::new(),
        ack_unknown: var_or("NINJACAT_ACK_UNKNOWN", "") == "true",
        // Agents send a few megabytes; a flare or a profile is tens of them.
        max_body_bytes: var_or("NINJACAT_MAX_BODY_BYTES", "")
            .parse()
            .ok()
            .filter(|n| *n > 0)
            .unwrap_or(64 * 1024 * 1024),
    })
}

/// Serves `router` on `NINJACAT_ADDR` until the program is told to stop, then flushes what its
/// writers still hold.
pub async fn serve(name: &str, router: Router, intake: Intake) -> Result<(), BoxError> {
    if var_or("DEBUG", "") == "true" {
        let directory = PathBuf::from(var_or("NINJACAT_CAPTURE_DIR", "captures"));
        router.middleware(move |req, next| capture::dump(directory.clone(), req, next));
    }

    let addr = var_or("NINJACAT_ADDR", ":8080");
    let listener = TcpListener::bind(listen_on(&addr)).await?;
    tracing::info!("{name} listening on {addr}");
    Server::builder().build().try_spawn_http(listener, router)?.shutdown_on_signal().await?;
    intake.stopping.cancel();
    intake.tasks.close();
    intake.tasks.wait().await;
    // Last, once nothing feeds the writers any more.
    intake.sink.stop().await;
    Ok(())
}
