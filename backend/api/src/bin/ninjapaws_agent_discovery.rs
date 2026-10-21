//! agentdiscovery-intake.<site>: the integrations each agent runtime sees.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::evp;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_agent_discovery", Some(Scheme::Agent), evp::mount_agent_discovery).await
}
