//! browser-intake.<site>: the RUM and Logs SDKs, browser and mobile, with the CORS policy
//! (`NINJACAT_RUM_ALLOWED_ORIGINS`) and the trusted proxies (`NINJACAT_TRUSTED_PROXIES`) above its
//! routes.

use ninjapaws_intake::auth;
use ninjapaws_intake::keys;
use ninjapaws_intake::routers::{flare, logs, profiling, rum};
use ninjapaws_intake::server::{self, Body, Ctx, var_or};
use tako::extractors::state::State;
use tako::router::Router;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let allowed_origins = rum::parse_allowed_origins(&var_or("NINJACAT_RUM_ALLOWED_ORIGINS", ""));
    let trusted = rum::trusted_proxies(&var_or("NINJACAT_TRUSTED_PROXIES", ""))?;
    server::init_tracing();
    let clickhouse = ninjapaws_storage::client_from_env();
    let keys = keys::start(&server::var_or("DATABASE_URL", "")).await?;
    let settings = server::Settings::from_env();

    let mut router = Router::new();
    router.post("/api/v2/rum", rum::handle_rum).middleware(auth::browser);
    // Logs from an SDK are the wire format the agent sends; only the framing differs, and the logs
    // handler sniffs it.
    router.post("/api/v2/logs", logs::handle).middleware(auth::browser);
    router.post("/api/v2/replay", rum::handle_replay).middleware(auth::browser);
    router.post("/api/v2/spans", rum::handle_spans).middleware(auth::browser);
    // The browser profiler's multipart is the envelope the tracers send, so it reuses that handler
    // under its own label.
    router
        .post("/api/v2/profile", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            profiling::handle_profile("profile-browser", ctx, clickhouse, body)
        })
        .middleware(auth::browser);
    router.post("/api/v2/debugger", profiling::handle_debugger).middleware(auth::browser);
    router.get("/api/v2/profiling/quota", rum::handle_profiling_quota).middleware(auth::browser);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::browser);
    router.post("/support/flare", flare::handle).middleware(auth::browser);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::browser);

    // Without a key, on every host.
    router.get("/ping", server::ping);
    router.get("/_health", server::health);
    router.head("/support/flare", server::flare_probe);
    router.head("/support/flare/{case}", server::flare_probe);
    // Anything else: logged, and 404.
    router.middleware(server::log_405);
    router.fallback(server::not_found);

    // What the handlers and the middleware read.
    router.with_state(clickhouse);
    router.with_state(keys);
    router.with_state(settings);
    // CORS and the trusted proxies sit above every route.
    let router = rum::above(router, &allowed_origins, trusted);
    server::serve("ninjapaws_browser", router).await
}
