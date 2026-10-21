//! contlcycle-intake.<site> and contimage-intake.<site>: container lifecycle events and the image
//! inventory.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::containers;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_containers", Some(Scheme::Agent), containers::mount).await
}
