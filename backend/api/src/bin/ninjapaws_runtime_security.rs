//! runtime-security-http-intake.logs.<site>: CWS events and agent info.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::security;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_runtime_security", Some(Scheme::Agent), security::mount_runtime_security)
        .await
}
