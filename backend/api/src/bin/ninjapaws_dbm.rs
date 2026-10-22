//! dbm-metrics-intake.<site>: Database Monitoring.

use ninjapaws_intake::auth;
use ninjapaws_intake::keys;
use ninjapaws_intake::routers::{dbm, flare};
use ninjapaws_intake::server::{self, Body, Ctx};
use tako::extractors::state::State;
use tako::router::Router;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    server::init_tracing();
    let clickhouse = ninjapaws_storage::client_from_env();
    let keys = keys::start(&server::var_or("DATABASE_URL", "")).await?;
    let settings = server::Settings::from_env();

    let mut router = Router::new();
    router
        .post("/api/v2/dbmmetrics", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("dbmmetrics", ctx, clickhouse, body)
        })
        .middleware(auth::agent); // query and lock metrics
    router
        .post("/api/v2/dbmactivity", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("dbmactivity", ctx, clickhouse, body)
        })
        .middleware(auth::agent); // active sessions
    router
        .post("/api/v2/databasequery", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("databasequery", ctx, clickhouse, body)
        })
        .middleware(auth::agent); // query samples and plans
    router
        .post("/api/v2/dbmmetadata", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("dbmmetadata", ctx, clickhouse, body)
        })
        .middleware(auth::agent); // instances and schemas
    // No producer of these two is in the agent's source: only the envelope can be claimed about
    // them.
    router
        .post("/api/v2/dbmhealth", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("dbmhealth", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/v2/dbmcolumnstatistics", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            dbm::handle("dbmcolumnstatistics", ctx, clickhouse, body)
        })
        .middleware(auth::agent);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::agent);
    router.post("/support/flare", flare::handle).middleware(auth::agent);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::agent);

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
    server::serve("ninjapaws_dbm", router).await
}
