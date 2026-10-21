//! ndm-intake.<site>, snmp-traps-intake.<site>, ndmflow-intake.<site> and netpath-intake.<site>:
//! Network Device Monitoring.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::ndm;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_ndm", Some(Scheme::Agent), ndm::mount).await
}
