//! dbm-metrics-intake.<site>: Database Monitoring.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::dbm;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_dbm", Some(Scheme::Agent), dbm::mount).await
}
