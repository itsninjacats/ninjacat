//! event-management-intake.<site>: notable events.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::evp;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_event_management", Some(Scheme::Agent), evp::mount_event_management).await
}
