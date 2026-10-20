//! process.<site>: what the process-agent sends, processes, containers, connections and
//! discoveries. It takes the place of that host of the F# `ninjacat-api intake`, with the same
//! environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::process;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_processes", Some(Scheme::Agent), process::mount).await
}
