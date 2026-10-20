//! api.<site> and <version>-flare.agent.<site>: the core agent's metrics, checks, events, host
//! metadata and Private Action Runner traffic. It takes the place of that host of the F#
//! `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::api;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_api", Some(Scheme::Agent), api::mount).await
}
