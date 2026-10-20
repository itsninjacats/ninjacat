//! config.<site>: remote configuration, answered 404 until signing exists. It takes the place of
//! that host of the F# `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::config;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_remote_config", Some(Scheme::Agent), config::mount).await
}
