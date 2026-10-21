//! webhook-intake.<site>: CI providers' pipeline webhooks (Jenkins, GitLab).

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::ci_webhook;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_ci_webhook", Some(Scheme::CiWebhook), ci_webhook::mount).await
}
