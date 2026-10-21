//! agenthealth-intake.<site>: the agent's reports on its own setup.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::evp;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_agent_health", Some(Scheme::Agent), evp::mount_agent_health).await
}
