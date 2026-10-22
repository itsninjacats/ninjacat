//! app.<site> and <version>-app.agent.<site>: what api.<site> serves, and metrics v3.

use ninjapaws_intake::auth;
use ninjapaws_intake::keys;
use ninjapaws_intake::routers::{api, app, flare};
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
    router.post("/intake/", api::handle_intake).middleware(auth::agent);
    router.post("/api/v1/series", api::handle_series_v1).middleware(auth::agent);
    router.post("/api/v2/series", api::handle_series_v2).middleware(auth::agent);
    router.post("/api/v1/check_run", api::handle_check_run).middleware(auth::agent);
    router.post("/api/v1/metadata", api::handle_metadata).middleware(auth::agent);
    router.post("/api/beta/sketches", api::handle_sketches).middleware(auth::agent);

    // Not sent by any agent: API clients only.
    router.post("/api/v1/events", api::handle_events).middleware(auth::agent);
    router.post("/api/v1/distribution_points", api::handle_distribution_points).middleware(auth::agent);

    router.post("/api/v2/intake-key", api::handle_intake_key).middleware(auth::agent);
    router.post("/api/v2/profiles/symbols/query", api::handle_symbols_query).middleware(auth::agent);
    // The agent sends GET; POST gets the same answer.
    router.post("/api/v2/validate", api::handle_opm_validate).middleware(auth::agent);
    router.get("/api/v2/validate", api::handle_opm_validate).middleware(auth::agent);

    router.post("/api/unstable/on_prem_runners", api::handle_runner_enroll).middleware(auth::agent);
    router.post("/api/unstable/on_prem_runners/api_key_only", api::handle_runner_enroll).middleware(auth::agent);
    router
        .post("/api/v2/on-prem-management-service/workflow-tasks/dequeue", api::handle_runner_dequeue)
        .middleware(auth::agent);
    router
        .post("/api/v2/on-prem-management-service/workflow-tasks/publish-task-update", api::handle_runner_task_update)
        .middleware(auth::agent);
    router
        .post("/api/v2/on-prem-management-service/workflow-tasks/heartbeat", api::handle_runner_heartbeat)
        .middleware(auth::agent);
    router
        .get("/api/v2/on-prem-management-service/runner/health-check", api::handle_runner_health_check)
        .middleware(auth::agent);
    router.post("/api/v2/actions/connections", api::handle_action_connections).middleware(auth::agent);

    router.get("/api/v1/query", api::handle_query).middleware(auth::agent);

    // Columnar metrics, intake v3.
    router
        .post("/api/intake/metrics/v3/series", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            app::handle("v3series", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/intake/metrics/v3/sketches", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            app::handle("v3sketches", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    // A shadow copy of the v2 traffic on /api/beta/sketches.
    router
        .post("/api/intake/metrics/v3beta/sketches", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            app::handle("v3beta", ctx, clickhouse, body)
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
    server::serve("ninjapaws_app", router).await
}
