//! Rows, the one insert that stores them, and the schema's migrations. Nothing here knows HTTP or
//! Datadog, which is what lets a second source (OTLP) write through it unchanged.

pub mod column;
pub mod fnv;
pub mod migrations;
pub mod rows;
pub mod table;

pub use table::Table;

use std::env;
use std::time::Duration;

/// An insert gives up after this long; ClickHouse may still finish it. Without it a hung insert
/// would hold its request forever.
const INSERT_TIMEOUT: Duration = Duration::from_secs(120);

/// The ClickHouse every process shares, as `CLICKHOUSE_*` names it. `CLICKHOUSE_HTTP_ADDR` is the
/// HTTP interface (:8123), which the driver speaks, not the native protocol's port.
pub fn client_from_env() -> clickhouse::Client {
    let var =
        |name: &str, default: &str| env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.into());
    clickhouse::Client::default()
        .with_url(format!("http://{}", var("CLICKHOUSE_HTTP_ADDR", "localhost:8123")))
        .with_database(var("CLICKHOUSE_DB", "ninjacat"))
        .with_user(var("CLICKHOUSE_USER", "ninjacat"))
        .with_password(var("CLICKHOUSE_PASSWORD", "ninjacat"))
        // A `JSON` column takes its object as the text it is in the row.
        .with_setting("input_format_binary_read_json_as_string", "1")
        // ClickHouse batches the many small inserts itself, and answers once the rows are stored.
        // Both are 26.8's defaults, but a server profile can change them.
        .with_setting("async_insert", "1")
        .with_setting("wait_for_async_insert", "1")
}

/// Inserts `rows` into their table now, and fails if ClickHouse refuses or does not answer in time.
pub async fn insert<T: Table>(client: &clickhouse::Client, rows: &[T]) -> Result<(), String> {
    if rows.is_empty() {
        return Ok(());
    }
    let sent = async {
        let mut insert = client.insert::<T>(T::TABLE).await?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await
    };
    match tokio::time::timeout(INSERT_TIMEOUT, sent).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(format!("the insert into {} failed: {e}", T::TABLE)),
        Err(_) => Err(format!("the insert into {} did not finish in {} seconds", T::TABLE, INSERT_TIMEOUT.as_secs())),
    }
}
