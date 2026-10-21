//! http-synthetics.<site>: the results of the tests the agent ran.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::evp;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_synthetics_results", Some(Scheme::Agent), evp::mount_synthetics_results)
        .await
}
