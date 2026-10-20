//! browser-intake.<site>: the RUM and Logs SDKs, browser and mobile, with the CORS policy
//! (`NINJACAT_RUM_ALLOWED_ORIGINS`) and the trusted proxies (`NINJACAT_TRUSTED_PROXIES`) above its
//! routes. It takes the place of that host of the F# `ninjacat-api intake`, with the same
//! environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::rum;
use ninjapaws_intake::server::var_or;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let allowed_origins = rum::parse_allowed_origins(&var_or("NINJACAT_RUM_ALLOWED_ORIGINS", ""));
    let trusted = rum::trusted_proxies(&var_or("NINJACAT_TRUSTED_PROXIES", ""))?;
    ninjapaws_intake::server::run_with("ninjapaws_browser", Some(Scheme::Browser), rum::mount, |router, _| {
        Ok(rum::above(router, &allowed_origins, trusted))
    })
    .await
}
