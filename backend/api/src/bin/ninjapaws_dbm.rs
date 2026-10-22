//! dbm-metrics-intake.<site>: Database Monitoring.

use ninjapaws_intake::auth;
use ninjapaws_intake::routers::{dbm, flare};
use ninjapaws_intake::server::{self, Body, Ctx};
use tako::router::Router;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let intake = server::start().await?;

    let mut router = Router::new();
    router
        .post("/api/v2/dbmmetrics", |ctx: Ctx, body: Body| dbm::handle("dbmmetrics", ctx, body))
        .middleware(auth::agent); // query and lock metrics
    router
        .post("/api/v2/dbmactivity", |ctx: Ctx, body: Body| dbm::handle("dbmactivity", ctx, body))
        .middleware(auth::agent); // active sessions
    router
        .post("/api/v2/databasequery", |ctx: Ctx, body: Body| dbm::handle("databasequery", ctx, body))
        .middleware(auth::agent); // query samples and plans
    router
        .post("/api/v2/dbmmetadata", |ctx: Ctx, body: Body| dbm::handle("dbmmetadata", ctx, body))
        .middleware(auth::agent); // instances and schemas
    // No producer of these two is in the agent's source: only the envelope can be claimed about
    // them.
    router
        .post("/api/v2/dbmhealth", |ctx: Ctx, body: Body| dbm::handle("dbmhealth", ctx, body))
        .middleware(auth::agent);
    router
        .post("/api/v2/dbmcolumnstatistics", |ctx: Ctx, body: Body| dbm::handle("dbmcolumnstatistics", ctx, body))
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

    // What the handlers write to and read from.
    router.with_state(intake.clone());
    server::serve("ninjapaws_dbm", router, intake).await
}
