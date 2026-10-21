//! softinv-intake.<site>: installed software per host.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::evp;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_software_inventory", Some(Scheme::Agent), evp::mount_software_inventory)
        .await
}
