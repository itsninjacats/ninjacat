//! data-obs-intake.<site>: OpenLineage events and query action results.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::evp;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_data_obs", Some(Scheme::Lineage), evp::mount_data_obs).await
}
