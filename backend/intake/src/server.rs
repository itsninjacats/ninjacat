//! One intake host as a process: its routes, the ones every host has, the API key they ask for,
//! and the answer for anything else. A binary names its host's router and calls [`run`].
//!
//! Which host a request was sent to is not looked at: each host is a process of its own, and
//! whatever is in front of them (an ingress, a proxy) sends each host name to its process.

use std::env;
use std::future::Future;
use std::sync::Arc;

use bytes::Bytes;
use http::{HeaderValue, Method, StatusCode, Uri};
use http_body_util::{BodyExt, Limited};
use ninjapaws_storage::{Sink, Table};
use serde::Serialize;
use tako::Server;
use tako::body::TakoBody;
use tako::extractors::ipaddr::IpAddr;
use tako::middleware::Next;
use tako::router::Router;
use tako::types::{BoxError, Request, Response};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

use crate::auth::{self, Admitted, Scheme};
use crate::keys::{self, Store};
use crate::raw;
use crate::routers::flare;

/// What every handler of a process shares.
pub struct Intake {
    pub sink: Sink,
    pub keys: Arc<Store>,
    /// Answer 202 instead of 404 on unknown paths (NINJACAT_ACK_UNKNOWN): keeps a chatty agent
    /// quiet while a new endpoint is looked at.
    pub ack_unknown: bool,
    /// The largest request body read (NINJACAT_MAX_BODY_BYTES); a larger one is refused with 413.
    pub max_body_bytes: usize,
}

/// One request as a handler sees it: everything but the body, which it is given apart.
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
        query_value(&self.parts.uri, name)
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

/// A query parameter's first value, or "".
pub fn query_value(uri: &Uri, name: &str) -> String {
    query_pairs(uri).into_iter().find(|(key, _)| key == name).map(|(_, value)| value).unwrap_or_default()
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

/// The routes of one host, being registered.
pub struct Routes {
    router: Router,
    intake: Arc<Intake>,
    /// The key the routes ask for; None for a host whose clients are anonymous.
    scheme: Option<Scheme>,
}

impl Routes {
    pub fn post<F, Fut>(&mut self, path: &str, handler: F)
    where
        F: Fn(Ctx, Bytes) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        self.add(Method::POST, path, handler);
    }

    pub fn get<F, Fut>(&mut self, path: &str, handler: F)
    where
        F: Fn(Ctx, Bytes) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        self.add(Method::GET, path, handler);
    }

    pub fn head<F, Fut>(&mut self, path: &str, handler: F)
    where
        F: Fn(Ctx, Bytes) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        self.add(Method::HEAD, path, handler);
    }

    /// A route that reads the body whole, undoes its Content-Encoding, and hands the bytes to the
    /// handler; behind the host's key.
    fn add<F, Fut>(&mut self, method: Method, path: &str, handler: F)
    where
        F: Fn(Ctx, Bytes) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Response> + Send + 'static,
    {
        let intake = self.intake.clone();
        let route = self.router.route(method, path, move |req: Request| {
            let handler = handler.clone();
            let intake = intake.clone();
            async move {
                let (parts, body) = req.into_parts();
                let raw = match Limited::new(body, intake.max_body_bytes).collect().await {
                    Ok(collected) => collected.to_bytes(),
                    Err(_) => return status(StatusCode::PAYLOAD_TOO_LARGE),
                };
                let encoding =
                    parts.headers.get(http::header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()).unwrap_or_default();
                let body = crate::body::decompress(encoding, raw);
                handler(Ctx { parts, intake }, body).await
            }
        });
        if let Some(scheme) = self.scheme {
            let keys = self.intake.keys.clone();
            route.middleware(move |req, next| auth::guard(keys.clone(), scheme, req, next));
        }
    }
}

/// The host's router: its routes, the probes, what every host with a key serves, and the answer
/// for anything else.
pub fn router(intake: Intake, scheme: Option<Scheme>, mount: impl FnOnce(&mut Routes)) -> Router {
    let intake = Arc::new(intake);
    let mut routes = Routes { router: Router::new(), intake: intake.clone(), scheme };
    mount(&mut routes);

    // A host that asks for no key serves its own routes and nothing else: a flare upload there
    // would be a body anyone could send.
    if scheme.is_some() {
        every_host(&mut routes);
    }

    // What asks for no key on any host: /ping, /_health, and the HEAD an agent sends to learn
    // whether a flare could be uploaded. Its connectivity check sends that one without a key and
    // reports anything but 200 or a redirect as a failure (pkg/diagnose/connectivity, agent
    // 7.84.0).
    let mut open = Routes { router: routes.router, intake: intake.clone(), scheme: None };
    open.get("/ping", |_, _| async { json(StatusCode::OK, &serde_json::json!({ "message": "pong" })) });
    open.get("/_health", |_, _| async { json(StatusCode::OK, &serde_json::json!({ "status": "ok" })) });
    open.head("/support/flare", |_, _| async { status(StatusCode::OK) });
    open.head("/support/flare/{case}", |_, _| async { status(StatusCode::OK) });

    let mut router = open.router;
    // A known path with the wrong method is answered by the router itself, with 405. It is as
    // unhandled as an unknown path, so it is logged too.
    router.middleware(|req: Request, next: Next| async move {
        let unhandled = Unhandled::of(&req);
        let response = next.run(req).await;
        if response.status() == StatusCode::METHOD_NOT_ALLOWED {
            unhandled.log();
        }
        response
    });
    let ack_unknown = intake.ack_unknown;
    router.fallback(move |req: Request| async move {
        Unhandled::of(&req).log();
        if ack_unknown {
            accepted()
        } else {
            json(StatusCode::NOT_FOUND, &serde_json::json!({ "errors": ["unknown endpoint"] }))
        }
    });
    router
}

/// What the agent sweeps at startup on whatever host it talks to, before it knows which intake
/// that is; and a flare, which goes to whichever host dd_url resolves to. They ask for the key
/// the host asks for.
fn every_host(routes: &mut Routes) {
    routes.get("/api/v1/validate", |_, _| async { json(StatusCode::OK, &serde_json::json!({ "valid": true })) });
    routes.post("/support/flare", flare::handle);
    routes.post("/support/flare/{case}", flare::handle);
}

/// A request nothing serves, for the log line that is how unhandled endpoints are found.
struct Unhandled {
    method: Method,
    path: String,
    content_type: String,
    user_agent: String,
}

impl Unhandled {
    fn of(req: &Request) -> Self {
        let header =
            |name| req.headers().get(name).and_then(|v: &HeaderValue| v.to_str().ok()).unwrap_or_default().to_string();
        Unhandled {
            method: req.method().clone(),
            path: raw::path_without_key(req.uri().path()),
            content_type: header(http::header::CONTENT_TYPE),
            user_agent: header(http::header::USER_AGENT),
        }
    }

    fn log(&self) {
        tracing::warn!(
            "UNHANDLED ENDPOINT: {} {} (Content-Type: {}, User-Agent: {})",
            self.method,
            self.path,
            self.content_type,
            self.user_agent
        );
    }
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

/// What a host's process has besides its routes, for a binary that adds to them.
pub struct Process {
    pub keys: Arc<Store>,
    pub sink: Sink,
    /// Cancelled once the HTTP listener has stopped. The tasks in `tasks` end on it, and the
    /// writers flush only after every one of them has.
    pub stopping: CancellationToken,
    pub tasks: TaskTracker,
}

/// The process of one host: reads its settings, the API keys and ClickHouse from the
/// environment, serves `mount`'s routes on `NINJACAT_ADDR` until it is told to stop, then
/// flushes what its writers still hold.
pub async fn run(name: &str, scheme: Option<Scheme>, mount: impl FnOnce(&mut Routes)) -> Result<(), BoxError> {
    run_with(name, scheme, mount, |router, _| Ok(router)).await
}

/// [`run`], with `around` handed the host's router before it is served, and the process: for what
/// sits above a host's routes (browser-intake's CORS) or listens beside them (the logs over TCP).
pub async fn run_with(
    name: &str,
    scheme: Option<Scheme>,
    mount: impl FnOnce(&mut Routes),
    around: impl FnOnce(Router, &Process) -> Result<Router, BoxError>,
) -> Result<(), BoxError> {
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
    // Agents send a few megabytes; a flare or a profile is tens of them.
    let max_body_bytes =
        var_or("NINJACAT_MAX_BODY_BYTES", "").parse().ok().filter(|n| *n > 0).unwrap_or(64 * 1024 * 1024);

    let keys = Arc::new(Store::default());
    keys::fetch_now(&keys, &database_url).await?;
    tokio::spawn(keys::keep(keys.clone(), database_url));

    let sink = Sink::clickhouse(ninjapaws_storage::client_from_env());
    let process = Process { keys, sink, stopping: CancellationToken::new(), tasks: TaskTracker::new() };
    let intake = Intake {
        sink: process.sink.clone(),
        keys: process.keys.clone(),
        ack_unknown: var_or("NINJACAT_ACK_UNKNOWN", "") == "true",
        max_body_bytes,
    };
    let router = around(router(intake, scheme, mount), &process)?;

    let addr = var_or("NINJACAT_ADDR", ":8080");
    let listener = TcpListener::bind(listen_on(&addr)).await?;
    tracing::info!("{name} listening on {addr}");
    Server::builder().build().try_spawn_http(listener, router)?.shutdown_on_signal().await?;
    process.stopping.cancel();
    process.tasks.close();
    process.tasks.wait().await;
    // Last, once nothing feeds the writers any more.
    process.sink.stop().await;
    Ok(())
}
