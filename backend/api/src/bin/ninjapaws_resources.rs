//! resources-intake.<site>: the opaque payloads integrations hand the agent. It takes the place of
//! that host of the F# `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::misc;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_resources", Some(Scheme::Agent), misc::resources).await
}
