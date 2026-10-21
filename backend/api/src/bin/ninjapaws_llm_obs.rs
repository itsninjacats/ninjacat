//! llmobs-intake.<site>: LLM Observability, which the agent only probes. It takes the place of that
//! host of the F# `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::install;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_llm_obs", Some(Scheme::Agent), install::mount_llm_obs).await
}
