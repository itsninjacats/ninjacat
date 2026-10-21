//! sbom-intake.<site>: software bills of materials.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::security;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_sbom", Some(Scheme::Agent), security::mount_sbom).await
}
