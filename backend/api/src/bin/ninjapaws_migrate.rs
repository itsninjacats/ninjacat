//! Applies the ClickHouse migrations and the metrics retention, then exits. An applied migration's
//! checksum is in the ledger; one whose file changed stops the run. The intake hosts only write;
//! this runs before them, as a pre-upgrade Job where replicas race.

use std::process::ExitCode;

use ninjapaws_storage::migrations;

async fn migrate() -> Result<(), String> {
    let retention = api::metrics_retention()?;
    let client = ninjapaws_storage::client_from_env();
    let applied = migrations::apply(&client).await?;
    println!("storage: applied {applied} schema migration(s)");
    migrations::set_metrics_retention(&client, retention.raw.num_days(), retention.rollup.num_days()).await
}

#[tokio::main]
async fn main() -> ExitCode {
    match migrate().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
