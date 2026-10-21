//! intake.profile.<site>: the profiles the trace-agent forwards.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::profiling;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_profile", Some(Scheme::Agent), profiling::mount_profile).await
}
