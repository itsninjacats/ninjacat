//! sds-intake.<site>: Sensitive Data Scanner results.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::security;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_sds", Some(Scheme::Agent), security::mount_sds).await
}
