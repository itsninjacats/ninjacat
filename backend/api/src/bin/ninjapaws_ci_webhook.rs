//! webhook-intake.<site>: CI providers' pipeline webhooks (Jenkins, GitLab).

use ninjapaws_intake::auth;
use ninjapaws_intake::keys;
use ninjapaws_intake::routers::{ci_webhook, flare};
use ninjapaws_intake::server;
use tako::router::Router;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    server::init_tracing();
    let clickhouse = ninjapaws_storage::client_from_env();
    let keys = keys::start(&server::var_or("DATABASE_URL", "")).await?;
    let settings = server::Settings::from_env();

    let mut router = Router::new();
    // With or without the slash the Jenkins plugin's URL ends in.
    router.post("/api/v2/webhook", ci_webhook::handle).middleware(auth::ci_webhook);
    router.post("/api/v2/webhook/", ci_webhook::handle).middleware(auth::ci_webhook);

    // What the agent sweeps at startup on whatever host it talks to, and a flare, which goes to
    // whichever host dd_url resolves to.
    router.get("/api/v1/validate", server::validate).middleware(auth::ci_webhook);
    router.post("/support/flare", flare::handle).middleware(auth::ci_webhook);
    router.post("/support/flare/{case}", flare::handle).middleware(auth::ci_webhook);

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
    server::serve("ninjapaws_ci_webhook", router).await
}
