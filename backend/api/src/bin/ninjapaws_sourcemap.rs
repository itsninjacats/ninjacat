//! sourcemap-intake.<site>: the host profiler's native symbol uploads.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::profiling;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_sourcemap", Some(Scheme::Agent), profiling::mount_sourcemap).await
}
