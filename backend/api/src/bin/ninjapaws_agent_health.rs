//! agenthealth-intake.<site>: the agent's reports on its own setup.

use ninjapaws_intake::auth;
use ninjapaws_intake::routers::{evp, flare};
use ninjapaws_intake::server;
use tako::router::Router;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let intake = server::start().await?;

    let mut router = Router::new();
    router.post("/api/v2/agenthealth", evp::handle_agent_health).middleware(auth::agent);

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
    server::serve("ninjapaws_agent_health", router, intake).await
}
