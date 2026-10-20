//! instrumentation-telemetry-intake.<site>: the instrumentation telemetry of tracers, the agent
//! and the installer. It takes the place of that host of the F# `ninjacat-api intake`, with the
//! same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::misc;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_telemetry", Some(Scheme::Agent), misc::telemetry).await
}
