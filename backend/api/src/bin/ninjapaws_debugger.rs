//! debugger-intake.<site>: Dynamic Instrumentation logs, probe diagnostics and the symbol
//! database. It takes the place of that host of the F# `ninjacat-api intake`, with the same
//! environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::profiling;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_debugger", Some(Scheme::Agent), profiling::mount_debugger).await
}
