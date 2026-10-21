//! citestcycle-intake.<site>: CI Visibility test events from tracers in CI. It takes the place of
//! that host of the F# `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::ci_visibility;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_ci_test_cycle", Some(Scheme::Agent), ci_visibility::mount_test_cycle).await
}
