//! orchestrator.<site> and kubeops-intake.<site>: what the cluster agent sends, Kubernetes and ECS
//! collections, manifests and the results of remote actions.

use ninjapaws_intake::auth;
use ninjapaws_intake::keys;
use ninjapaws_intake::routers::{flare, kubeops};
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
    // Kubernetes resource collections; /api/v1/orchestrator is the same under
    // orchestrator_explorer.use_legacy_endpoint.
    router
        .post("/api/v2/orch", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            kubeops::handle_frame("orch", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router
        .post("/api/v1/orchestrator", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            kubeops::handle_frame("orch", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    // Raw manifests (types 80-82): the object's own YAML or JSON, "exactly what kubectl would
    // show". Same frame, same decoder.
    router
        .post("/api/v2/orchmanif", |ctx: Ctx, clickhouse: State<clickhouse::Client>, body: Body| {
            kubeops::handle_frame("orchmanif", ctx, clickhouse, body)
        })
        .middleware(auth::agent);
    router.post("/api/v2/kubeactions", kubeops::handle_actions).middleware(auth::agent);

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
    server::serve("ninjapaws_kubeops", router).await
}
