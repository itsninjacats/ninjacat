//! Applies the ClickHouse migrations and the metrics retention, then exits. An applied migration's
//! checksum is in the ledger; one whose file changed stops the run. The intake hosts only write;
//! this runs before them, as a pre-upgrade Job where replicas race.

use std::env;
use std::process::ExitCode;

use ninjapaws_storage::migrations;

/// A whole number of days, up to a century. A set value outside 1–36500 stops the run: falling
/// back to the default could set a shorter TTL than meant, and ClickHouse would delete the
/// difference.
fn days(name: &str, default: i64) -> Result<i64, String> {
    match env::var(name).unwrap_or_default().as_str() {
        "" => Ok(default),
        raw => match raw.trim().parse::<i64>() {
            Ok(n) if (1..=36500).contains(&n) => Ok(n),
            _ => Err(format!("{name} must be a whole number of days from 1 to 36500, got \"{raw}\"")),
        },
    }
}

async fn migrate() -> Result<(), String> {
    let raw = days("NINJACAT_METRICS_RETENTION_DAYS", 30)?;
    let rollup = days("NINJACAT_METRICS_ROLLUP_RETENTION_DAYS", 400)?;
    // A step of whole minutes reads the rollup whatever its age, so a rollup kept shorter than
    // the raw points would answer nothing where the raw table still holds them.
    if rollup < raw {
        return Err(format!(
            "NINJACAT_METRICS_ROLLUP_RETENTION_DAYS ({rollup}) must not be shorter than NINJACAT_METRICS_RETENTION_DAYS ({raw})"
        ));
    }
    let client = ninjapaws_storage::client_from_env();
    let applied = migrations::apply(&client).await?;
    println!("storage: applied {applied} schema migration(s)");
    migrations::set_metrics_retention(&client, raw, rollup).await
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
