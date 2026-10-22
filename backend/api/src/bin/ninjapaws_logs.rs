//! http-intake.logs.<site> and agent-http-intake.logs.<site>, and beside them the agent's logs over
//! TCP (`NINJACAT_LOGS_TCP_ADDR`).

use ninjapaws_intake::auth;
use ninjapaws_intake::keys;
use ninjapaws_intake::routers::{flare, logs};
use ninjapaws_intake::server;
use ninjapaws_intake::tcp_logs;
use tako::router::Router;
use tako::types::BoxError;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    server::init_tracing();
    let clickhouse = ninjapaws_storage::client_from_env();
    let keys = keys::start(&server::var_or("DATABASE_URL", "")).await?;
    let settings = server::Settings::from_env();

    let mut router = Router::new();
    router.post("/api/v2/logs", logs::handle).middleware(auth::agent);
    router.post("/v1/input", logs::handle).middleware(auth::agent);
    // Older clients put the key in the path.
    router.post("/v1/input/{key}", logs::handle).middleware(auth::agent);

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

    // The agent's logs over TCP, beside the HTTP routes. Its connections end once the HTTP
    // listener has stopped, and the program waits for them.
    let stopping = CancellationToken::new();
    let tcp = TaskTracker::new();
    tcp_logs::listen(clickhouse.clone(), keys.clone(), stopping.clone(), tcp.clone())?;

    // What the handlers and the middleware read.
    router.with_state(clickhouse);
    router.with_state(keys);
    router.with_state(settings);
    server::serve("ninjapaws_logs", router).await?;

    stopping.cancel();
    tcp.close();
    tcp.wait().await;
    Ok(())
}
