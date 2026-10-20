//! ndm-intake.<site>, snmp-traps-intake.<site>, ndmflow-intake.<site> and netpath-intake.<site>:
//! Network Device Monitoring. It takes the place of those hosts of the F#
//! `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::ndm;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_ndm", Some(Scheme::Agent), ndm::mount).await
}
