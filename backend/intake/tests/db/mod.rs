//! The scratch ClickHouse database a test binary inserts into, and the rows read back from it.
//! Every test returns early, after one "skipped:" line, when ClickHouse is not running.

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine;
use serde_json::Value;
use tokio::sync::OnceCell;

/// What this file's databases are named with; every test database's name starts "ninjacat_test_".
const PREFIX: &str = "ninjacat_test_intake";

/// Tables a materialized view fills, and the migrations' ledger: no handler inserts into them.
pub const DERIVED: &[&str] = &[
    "metrics_1m",
    "metrics_1m_mv",
    "metric_tag_keys",
    "metric_tag_keys_mv",
    "k8s_resources_current",
    "k8s_resources_current_mv",
    "schema_migrations",
];

/// What a test inserts with: production's client, with each async insert flushed at once.
pub fn client(database: &str) -> clickhouse::Client {
    ninjapaws_storage::client_from_env()
        .with_database(database)
        .with_setting("async_insert_busy_timeout_min_ms", "1")
        .with_setting("async_insert_busy_timeout_max_ms", "1")
}

/// A client of no server, for a request that must store nothing: an insert fails at once instead
/// of reaching a real database.
pub fn unconnected() -> clickhouse::Client {
    clickhouse::Client::default().with_url("http://127.0.0.1:1")
}

/// This binary's database, built from the migrations by the first test that asks. None when
/// ClickHouse is not running. Only the name is kept: each test makes its own client, so no
/// connection crosses the tests' runtimes.
pub async fn scratch() -> Option<String> {
    static DATABASE: OnceCell<Option<String>> = OnceCell::const_new();
    DATABASE.get_or_init(build).await.clone()
}

/// A client of this binary's database; None when ClickHouse is not running.
pub async fn scratch_client() -> Option<clickhouse::Client> {
    Some(client(&scratch().await?))
}

async fn build() -> Option<String> {
    let server = ninjapaws_storage::client_from_env().with_database("default");
    if let Err(e) = server.query("SELECT 1").execute().await {
        eprintln!("skipped: ClickHouse is not running: {e}");
        return None;
    }
    sweep(&server).await;

    // The pid keeps two runs apart, in two worktrees or at once.
    let exe = std::env::current_exe().unwrap();
    let binary = exe.file_stem().unwrap().to_string_lossy().replace('-', "_");
    let database = format!("{PREFIX}_{binary}_{}", std::process::id());
    server.query(&format!("DROP DATABASE IF EXISTS {database} SYNC")).execute().await.unwrap();
    server.query(&format!("CREATE DATABASE {database}")).execute().await.unwrap();
    let client = client(&database);
    ninjapaws_storage::migrations::apply(&client).await.unwrap();

    // ClickHouse drops a row its TTL has expired already on insert, and the fixtures' rows are old.
    let with_ttl: Vec<String> = client
        .query(
            "SELECT name FROM system.tables WHERE database = currentDatabase() \
             AND engine LIKE '%MergeTree' AND create_table_query LIKE '% TTL %'",
        )
        .fetch_all()
        .await
        .unwrap();
    for table in with_ttl {
        client.query(&format!("ALTER TABLE `{table}` REMOVE TTL")).execute().await.unwrap();
    }
    Some(database)
}

/// A database outlives its run, since a static is never dropped. This file's go once their process
/// has ended, and any test database once its tables are over an hour old. The pid is taken as one
/// of this machine's: the ClickHouse tests use is the developer's or the CI job's own.
async fn sweep(server: &clickhouse::Client) {
    let found: Vec<(String, bool)> = server
        .query(
            "SELECT d.name, toBool(t.oldest != toDateTime(0) AND t.oldest < now() - INTERVAL 1 HOUR) \
             FROM system.databases AS d LEFT JOIN \
             (SELECT database, min(metadata_modification_time) AS oldest FROM system.tables GROUP BY database) AS t \
             ON t.database = d.name WHERE startsWith(d.name, 'ninjacat_test_')",
        )
        .fetch_all()
        .await
        .unwrap_or_default();
    for (database, old) in found {
        let ended = database.starts_with(PREFIX) && database.rsplit('_').next().is_some_and(|pid| !alive(pid));
        if old || ended {
            let _ = server.query(&format!("DROP DATABASE IF EXISTS `{database}` SYNC")).execute().await;
        }
    }
}

/// Whether a process with this pid runs here: `kill -0` checks without sending a signal.
fn alive(pid: &str) -> bool {
    let check = std::process::Command::new("kill").args(["-0", pid]).stderr(std::process::Stdio::null()).status();
    check.map_or(true, |status| status.success())
}

/// A tenant no other test of the binary has: they share its database, and every row carries its
/// tenant. A test publishes a key for it.
pub fn tenant() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("t{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// How many rows the tenant has in each table, the derived ones left out.
pub async fn tables(client: &clickhouse::Client, tenant: &str) -> BTreeMap<String, u64> {
    let mut found: BTreeMap<String, u64> = client
        .query("SELECT _table, count() FROM merge(currentDatabase(), '.') WHERE tenant_id = ? GROUP BY _table")
        .bind(tenant)
        .fetch_all::<(String, u64)>()
        .await
        .unwrap()
        .into_iter()
        .collect();
    found.retain(|table, _| !DERIVED.contains(&table.as_str()));
    found
}

/// The tenant's rows of `table`, each a JSON object of every column, in the table's sort order
/// (not the order they were inserted in).
pub async fn rows(client: &clickhouse::Client, table: &str, tenant: &str) -> Vec<Value> {
    let sql = format!("SELECT * FROM `{table}` WHERE tenant_id = ?");
    json_lines(client.query(&sql).bind(tenant), "JSONEachRow").await
}

/// The query's rows as ClickHouse writes them in `format` (JSONEachRow, JSONCompactEachRow),
/// spelled as a row's serde JSON is: 64-bit integers as numbers, times in ISO 8601, and a string
/// that is not UTF-8 as `{"$b64": …}`.
pub async fn json_lines(query: clickhouse::query::Query, format: &str) -> Vec<Value> {
    let query = query
        .with_setting("output_format_json_quote_64bit_integers", "0")
        .with_setting("output_format_json_quote_denormals", "1")
        .with_setting("date_time_output_format", "iso");
    let mut cursor = query.fetch_bytes(format).unwrap();
    let mut bytes = Vec::new();
    while let Some(chunk) = cursor.next().await.unwrap() {
        bytes.extend_from_slice(&chunk);
    }
    bytes
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_slice(&bytes_as_b64(line)).unwrap())
        .collect()
}

/// ClickHouse writes a String's bytes into its JSON as they are. A JSON string whose bytes are not
/// UTF-8 becomes `{"$b64": …}`, as a row's serde JSON spells a Blob.
fn bytes_as_b64(line: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(line.len());
    let mut i = 0;
    while i < line.len() {
        if line[i] != b'"' {
            out.push(line[i]);
            i += 1;
            continue;
        }
        let start = i;
        i += 1;
        let mut bytes = Vec::new();
        while line[i] != b'"' {
            if line[i] != b'\\' {
                bytes.push(line[i]);
                i += 1;
                continue;
            }
            let escaped = line[i + 1];
            i += 2;
            match escaped {
                b'n' => bytes.push(b'\n'),
                b't' => bytes.push(b'\t'),
                b'r' => bytes.push(b'\r'),
                b'b' => bytes.push(8),
                b'f' => bytes.push(12),
                b'u' => {
                    let code = u32::from_str_radix(std::str::from_utf8(&line[i..i + 4]).unwrap(), 16).unwrap();
                    i += 4;
                    let mut buf = [0; 4];
                    let c = char::from_u32(code).unwrap_or('\u{fffd}');
                    bytes.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                other => bytes.push(other),
            }
        }
        i += 1;
        if std::str::from_utf8(&bytes).is_ok() {
            out.extend_from_slice(&line[start..i]);
        } else {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            out.extend_from_slice(format!(r#"{{"$b64":"{encoded}"}}"#).as_bytes());
        }
    }
    out
}
