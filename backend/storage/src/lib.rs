//! Rows and where they go: one writer per ClickHouse table, the sink handlers hand rows to, and
//! the schema's migrations. Nothing here knows HTTP or Datadog, which is what lets a second source
//! (OTLP) write through it unchanged.

pub mod column;
pub mod fnv;
pub mod migrations;
pub mod rows;
pub mod sink;
pub mod table;
mod writer;

pub use sink::{Capture, Sink, Write};
pub use table::{Limits, Table};
pub use writer::WriterStats;

use std::env;

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
}
