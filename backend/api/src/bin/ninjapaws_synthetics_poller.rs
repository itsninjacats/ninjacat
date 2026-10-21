//! intake.synthetics.<site>: the poller the agent reads its synthetics test list from. It takes the
//! place of that host of the F# `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::ci_webhook;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run(
        "ninjapaws_synthetics_poller",
        Some(Scheme::Agent),
        ci_webhook::mount_synthetics_poller,
    )
    .await
}
