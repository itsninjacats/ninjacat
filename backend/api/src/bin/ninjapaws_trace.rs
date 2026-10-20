//! trace.agent.<site>: what the trace-agent sends, spans, APM stats and Data Streams. It takes the
//! place of that host of the F# `ninjacat-api intake`, with the same environment.

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::trace;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run("ninjapaws_trace", Some(Scheme::Agent), trace::mount).await
}
