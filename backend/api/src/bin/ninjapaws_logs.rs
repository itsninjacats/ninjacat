//! http-intake.logs.<site> and agent-http-intake.logs.<site>, and beside them the agent's logs over
//! TCP (`NINJACAT_LOGS_TCP_ADDR`).

use ninjapaws_intake::auth::Scheme;
use ninjapaws_intake::routers::logs;
use ninjapaws_intake::tcp_logs;
use tako::types::BoxError;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    ninjapaws_intake::server::run_with("ninjapaws_logs", Some(Scheme::Agent), logs::mount, |router, process| {
        tcp_logs::listen(process)?;
        Ok(router)
    })
    .await
}
