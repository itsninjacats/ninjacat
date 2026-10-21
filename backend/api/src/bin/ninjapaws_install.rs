//! install.datadoghq.com and install.datad0g.com: the installer's OCI registry, BTF archives and
//! the HEAD probe; anonymous, as its clients send no key. It takes the place of that host of the F#
//! `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::routers::install;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_install", None, install::mount).await
}
