//! eudm-intake.<site>: AI usage reports through the agent's EVP proxy.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::install;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_ai_usage", Some(Scheme::Agent), install::mount_ai_usage).await
}
