//! app.<site> and <version>-app.agent.<site>: what api.<site> serves, and metrics v3.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::app;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_app", Some(Scheme::Agent), app::mount).await
}
