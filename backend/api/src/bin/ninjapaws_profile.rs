//! intake.profile.<site>: the profiles the trace-agent forwards. It takes the place of that host
//! of the F# `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::profiling;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_profile", Some(Scheme::Agent), profiling::mount_profile).await
}
