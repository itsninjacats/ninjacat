//! The service's parts driven by hand over fakes, and against a scratch database of the dev
//! ClickHouse and a scratch schema of the dev Postgres. Neither is the dev stack's own: the
//! monitor service running there must not see what these write. The tests that need a database
//! pass without asserting when it is not running.

mod check;
mod definitions;
mod entity;
mod manager;
mod notifier;
mod selfmon;
mod shards;
mod store;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use api::reads::ClickHouse;
use ninjapaws_storage::Sink;
use tokio::sync::mpsc::UnboundedReceiver;

pub const WAIT: Duration = Duration::from_secs(5);
pub const QUIET: Duration = Duration::from_millis(300);

/// The next message within `WAIT`.
pub async fn expect<T>(received: &mut UnboundedReceiver<T>) -> T {
    match tokio::time::timeout(WAIT, received.recv()).await {
        Ok(Some(message)) => message,
        Ok(None) => panic!("the sender is gone"),
        Err(_) => panic!("nothing arrived in {WAIT:?}"),
    }
}

/// Nothing within `QUIET`.
pub async fn expect_none<T: std::fmt::Debug>(received: &mut UnboundedReceiver<T>) {
    if let Ok(Some(message)) = tokio::time::timeout(QUIET, received.recv()).await {
        panic!("expected nothing, got {message:?}");
    }
}

/// The first message within `WAIT` that `wanted` accepts; the others are dropped.
pub async fn fish<T>(received: &mut UnboundedReceiver<T>, wanted: impl Fn(&T) -> bool) -> T {
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        match tokio::time::timeout_at(deadline, received.recv()).await {
            Ok(Some(message)) if wanted(&message) => return message,
            Ok(Some(_)) => {}
            Ok(None) => panic!("the sender is gone"),
            Err(_) => panic!("nothing wanted arrived in {WAIT:?}"),
        }
    }
}

/// Polls until `check` holds, within `within`.
pub async fn eventually(within: Duration, check: impl Fn() -> bool) {
    let deadline = tokio::time::Instant::now() + within;
    while !check() {
        if tokio::time::Instant::now() > deadline {
            panic!("the condition did not hold within {within:?}");
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// A name no other test run has.
pub fn unique(prefix: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    format!("{prefix}_{}_{nanos}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
}

/// Blocks on a runtime of its own: the test's runtime is not there in `Drop`.
fn on_own_runtime<F: Future + Send + 'static>(work: F) {
    std::thread::spawn(move || {
        tokio::runtime::Runtime::new().unwrap().block_on(work);
    })
    .join()
    .ok();
}

// --- the scratch ClickHouse database ---------------------------------------------------------

pub const DEV_CLICKHOUSE: &str = "localhost:8123";

/// What the service reads and writes, made like the dev stack's `ninjacat` tables.
const TABLES: [&str; 5] =
    ["monitor_transitions", "monitor_evaluations", "monitor_notifications", "check_runs", "metrics"];

fn admin() -> clickhouse::Client {
    clickhouse::Client::default()
        .with_url(format!("http://{DEV_CLICKHOUSE}"))
        .with_user("ninjacat")
        .with_password("ninjacat")
}

/// A database of its own on the dev ClickHouse, dropped afterwards. None when there is no
/// ClickHouse with the dev stack's schema.
pub struct Scratch {
    admin: clickhouse::Client,
    pub database: String,
}

impl Scratch {
    pub async fn new() -> Option<Scratch> {
        let admin = admin();
        let database = unique("ninjacat_monitors");
        if let Err(e) = admin.query(&format!("CREATE DATABASE {database}")).execute().await {
            eprintln!("skipped: ClickHouse is not reachable at {DEV_CLICKHOUSE}: {e}");
            return None;
        }
        let scratch = Scratch { admin, database };
        for table in TABLES {
            if let Err(e) = scratch.run(&format!("CREATE TABLE {{db}}.{table} AS ninjacat.{table}")).await {
                eprintln!("skipped: the dev ClickHouse has no ninjacat.{table}: {e}");
                return None;
            }
        }
        Some(scratch)
    }

    /// `{db}` is the scratch database.
    pub async fn run(&self, sql: &str) -> Result<(), clickhouse::error::Error> {
        self.admin.query(&sql.replace("{db}", &self.database)).execute().await
    }

    pub fn reader(&self) -> ClickHouse {
        ClickHouse::new(DEV_CLICKHOUSE, &self.database, "ninjacat", "ninjacat", crate::QUERY_LIMIT_SECONDS)
    }

    pub fn sink(&self) -> Sink {
        Sink::clickhouse(crate::write_client(DEV_CLICKHOUSE, &self.database, "ninjacat", "ninjacat"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let sql = format!("DROP DATABASE IF EXISTS {}", self.database);
        on_own_runtime(async move { admin().query(&sql).execute().await });
    }
}

// --- the scratch Postgres schema -------------------------------------------------------------

pub fn dev_postgres_url() -> String {
    std::env::var("DATABASE_URL")
        .ok()
        .filter(|url| !url.is_empty())
        .unwrap_or_else(|| "postgres://root:mysecretpassword@localhost:5433/local".into())
}

/// A schema of its own on the dev Postgres, with a `monitor` table made like the panel's
/// (`LIKE public.monitor`, so a change to the table that breaks the reader fails here), dropped
/// afterwards. Its sessions find that table first.
pub struct ScratchPostgres {
    pub schema: String,
    pub config: tokio_postgres::Config,
    pub client: tokio_postgres::Client,
}

pub async fn connect(config: &tokio_postgres::Config) -> Result<tokio_postgres::Client, tokio_postgres::Error> {
    let (client, connection) = config.connect(tokio_postgres::NoTls).await?;
    tokio::spawn(connection);
    Ok(client)
}

impl ScratchPostgres {
    pub async fn new() -> Option<ScratchPostgres> {
        let url = dev_postgres_url();
        let mut config: tokio_postgres::Config = url.parse().unwrap();
        config.connect_timeout(Duration::from_secs(3));
        let client = match connect(&config).await {
            Ok(client) => client,
            Err(e) => {
                eprintln!("skipped: Postgres is not reachable at {url}: {e}");
                return None;
            }
        };
        let schema = unique("monitors_test");
        let made = client
            .batch_execute(&format!(
                "CREATE SCHEMA {schema}; CREATE TABLE {schema}.monitor (LIKE public.monitor INCLUDING ALL)"
            ))
            .await;
        if let Err(e) = made {
            eprintln!("skipped: the dev Postgres has no migrated monitor table: {e}");
            let _ = client.batch_execute(&format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).await;
            return None;
        }
        config.options(format!("-c search_path={schema}"));
        let client = connect(&config).await.unwrap();
        Some(ScratchPostgres { schema, config, client })
    }

    /// `deleted_at` is SQL: NULL, now(), …
    pub async fn insert(&self, tenant: &str, query: &str, deleted_at: &str) -> i64 {
        let sql = format!(
            "INSERT INTO monitor (tenant_id, name, type, query, options, version, deleted_at) \
             VALUES ($1, 'cpu', 'query alert', $2, '{{\"thresholds\": {{\"critical\": 90}}}}'::jsonb, 3, {deleted_at}) RETURNING id"
        );
        self.client.query_one(&sql, &[&tenant, &query]).await.unwrap().get(0)
    }
}

impl Drop for ScratchPostgres {
    fn drop(&mut self) {
        let sql = format!("DROP SCHEMA IF EXISTS {} CASCADE", self.schema);
        let url = dev_postgres_url();
        on_own_runtime(async move {
            let config: tokio_postgres::Config = url.parse().unwrap();
            if let Ok(client) = connect(&config).await {
                let _ = client.batch_execute(&sql).await;
            }
        });
    }
}
