//! Evaluates the monitors. Replicas share the monitors through Postgres advisory
//! locks (`shards.rs`); each runs the entities of the shards it holds, and one of them the
//! notifier. Health is all it serves over HTTP.
mod definitions;
mod entity;
mod manager;
mod notifier;
mod rows;
mod selfmon;
mod shards;
mod store;
mod telemetry;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use api::reads::ClickHouse;
use api::{listen_on, metrics_retention, var_or};
use chrono::TimeDelta;
use ninjapaws_engine::Sql;
use ninjapaws_engine::query::compile::Row;
use ninjapaws_engine::query::execute::Executor;
use ninjapaws_engine::query::plan::Retention;
use tako::Server;
use tako::StatusCode;
use tako::extractors::state::State;
use tako::router::Router;
use tako::types::BoxError;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc, watch};
use tokio_util::task::TaskTracker;

use crate::definitions::{MonitorKey, MonitorRow};
use crate::entity::Services;
use crate::manager::{Entities, Manager, Postgres};
use crate::notifier::{Notifier, Polled};
use crate::rows::{EvaluationRow, NotificationRow, TransitionRow};
use crate::selfmon::SelfMonitor;
use crate::shards::{APPLICATION_NAME, Balancer, LOCK_CLASS};
use crate::store::History;
use crate::telemetry::Counters;

const RESYNC_INTERVAL: Duration = Duration::from_secs(30);
const NOTIFY_INTERVAL: Duration = Duration::from_secs(5);
/// How often a replica counts the replicas and takes or releases shards.
const BALANCE_INTERVAL: Duration = Duration::from_secs(10);
const RETRY: Duration = Duration::from_secs(30);
/// ClickHouse's limit for one query: a model's history read can pass the query process's 7 s.
const QUERY_LIMIT_SECONDS: u64 = 30;

struct Settings {
    clickhouse_addr: String,
    clickhouse_db: String,
    clickhouse_user: String,
    clickhouse_password: String,
    database_url: String,
    retention: Retention,
    concurrency: usize,
    /// More is an error, never a truncation.
    max_groups: usize,
    health_addr: String,
    selfmon_interval: Duration,
    selfmon_tenant: String,
    selfmon_host: String,
}

fn settings() -> Result<Settings, String> {
    let database_url = var_or("DATABASE_URL", "");
    if database_url.is_empty() {
        return Err("DATABASE_URL is not set (e.g. postgres://root:mysecretpassword@localhost:5433/local)".into());
    }
    Ok(Settings {
        clickhouse_addr: var_or("CLICKHOUSE_HTTP_ADDR", "localhost:8123"),
        clickhouse_db: var_or("CLICKHOUSE_DB", "ninjacat"),
        clickhouse_user: var_or("CLICKHOUSE_USER", "ninjacat"),
        clickhouse_password: var_or("CLICKHOUSE_PASSWORD", "ninjacat"),
        database_url,
        retention: metrics_retention()?,
        concurrency: positive("NINJACAT_MONITOR_CONCURRENCY", 8)?,
        max_groups: positive("NINJACAT_MONITOR_MAX_GROUPS", 1000)?,
        health_addr: var_or("NINJACAT_MONITOR_HEALTH_ADDR", ":8559"),
        selfmon_interval: duration("NINJACAT_SELFMON_INTERVAL", Duration::from_secs(15)),
        selfmon_tenant: var_or("NINJACAT_SELFMON_TENANT", "default"),
        selfmon_host: match var_or("NINJACAT_SELFMON_HOST", "") {
            host if !host.is_empty() => host,
            _ => machine_name(),
        },
    })
}

/// A set value that is not a positive whole number stops the start rather than falling back to
/// the default.
fn positive(name: &str, default: usize) -> Result<usize, String> {
    match var_or(name, "").as_str() {
        "" => Ok(default),
        raw => match raw.parse::<usize>() {
            Ok(n) if n > 0 && n <= i32::MAX as usize => Ok(n),
            _ => Err(format!("{name} must be a positive whole number, got \"{raw}\"")),
        },
    }
}

/// A duration ("15s", "1m30s"), or a bare number of seconds; anything else is the default.
fn duration(name: &str, default: Duration) -> Duration {
    let raw = var_or(name, "");
    match (ninjapaws_engine::duration::parse(&raw), raw.parse::<u64>()) {
        (Ok(d), _) if d > TimeDelta::zero() => d.to_std().unwrap_or(default),
        (_, Ok(seconds)) if seconds > 0 => Duration::from_secs(seconds),
        _ => default,
    }
}

fn machine_name() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .map(|name| name.trim().to_string())
        .ok()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| var_or("HOSTNAME", "localhost"))
}

/// The client the write path uses. Kept apart from the read client so no query can ever write,
/// and so a slow query and a slow insert do not share a timeout. ClickHouse batches the inserts
/// on its side and answers once the rows are stored, as for every other program.
fn write_client(addr: &str, database: &str, user: &str, password: &str) -> clickhouse::Client {
    clickhouse::Client::default()
        .with_url(format!("http://{addr}"))
        .with_database(database)
        .with_user(user)
        .with_password(password)
        .with_setting("async_insert", "1")
        .with_setting("wait_for_async_insert", "1")
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The entities' world: the `monitor` table, ClickHouse's reads and the monitor tables' writes.
struct Live {
    pool: deadpool_postgres::Pool,
    reader: ClickHouse,
    writer: clickhouse::Client,
    /// The evaluation rows' inserts, which an entity does not wait for; the program waits for
    /// them before it exits.
    records: TaskTracker,
}

impl Executor for Live {
    async fn rows(&self, sql: Sql) -> Result<Vec<Row>, String> {
        self.reader.rows(sql).await
    }
}

impl entity::Backend for Live {
    async fn definition(&self, key: &MonitorKey) -> Result<Option<MonitorRow>, String> {
        definitions::by_key(&self.pool, key).await
    }

    async fn history(&self, key: &MonitorKey, now: i64) -> Result<History, String> {
        store::history(&self.reader, key, now).await
    }

    async fn insert(&self, rows: Vec<TransitionRow>) -> Result<(), String> {
        ninjapaws_storage::insert(&self.writer, &rows).await
    }

    fn record(&self, rows: Vec<EvaluationRow>) {
        let writer = self.writer.clone();
        self.records.spawn(async move {
            if let Err(e) = ninjapaws_storage::insert(&writer, &rows).await {
                eprintln!("warning: {} evaluation rows not stored: {e}", rows.len());
            }
        });
    }

    fn now(&self) -> i64 {
        now_ms()
    }

    fn gate_wait(&self, frequency: TimeDelta) -> Duration {
        frequency.to_std().unwrap_or(Duration::ZERO)
    }
}

struct LiveNotifier {
    reader: ClickHouse,
    writer: clickhouse::Client,
}

impl notifier::Backend for LiveNotifier {
    async fn cursor(&self) -> Result<Option<i64>, String> {
        notifier::cursor(&self.reader).await
    }

    async fn due(&self, since: i64) -> Result<Polled, String> {
        notifier::due(&self.reader, since).await
    }

    async fn write(&self, rows: Vec<NotificationRow>) -> Result<(), String> {
        ninjapaws_storage::insert(&self.writer, &rows).await
    }

    fn now(&self) -> i64 {
        now_ms()
    }
}

#[derive(Clone)]
struct Ready(Arc<AtomicBool>);

/// Kubernetes' probes: live while the process runs, ready while the lock session is up.
async fn live() -> &'static str {
    "ok"
}

async fn ready(State(ready): State<Ready>) -> (StatusCode, &'static str) {
    match ready.0.load(Ordering::Relaxed) {
        true => (StatusCode::OK, "ok"),
        false => (StatusCode::SERVICE_UNAVAILABLE, "no lock session"),
    }
}

async fn terminated() {
    let mut term =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("SIGTERM can be watched");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    // The libraries (tako, the drivers) report through tracing.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let s = settings()?;
    let mut postgres: tokio_postgres::Config = s.database_url.parse()?;
    // A connection gives up after 15 s, and an idle one is probed, so a dead
    // server or link is noticed.
    if postgres.get_connect_timeout().is_none() {
        postgres.connect_timeout(Duration::from_secs(15));
    }
    postgres.keepalives(true).keepalives_idle(Duration::from_secs(30));
    let pool = definitions::pool(postgres.clone());
    let reader = ClickHouse::new(
        &s.clickhouse_addr,
        &s.clickhouse_db,
        &s.clickhouse_user,
        &s.clickhouse_password,
        QUERY_LIMIT_SECONDS,
    );
    let writer = write_client(&s.clickhouse_addr, &s.clickhouse_db, &s.clickhouse_user, &s.clickhouse_password);
    let records = TaskTracker::new();
    let counts = Arc::new(Counters::default());

    let services = Arc::new(Services {
        backend: Arc::new(Live {
            pool: pool.clone(),
            reader: reader.clone(),
            writer: writer.clone(),
            records: records.clone(),
        }),
        gate: Arc::new(Semaphore::new(s.concurrency)),
        retention: s.retention,
        max_groups: s.max_groups,
        retry: RETRY,
        resync: RESYNC_INTERVAL,
        counts: counts.clone(),
    });

    let (orders, received) = mpsc::unbounded_channel();
    let manager = Manager {
        definitions: Arc::new(Postgres { pool, config: postgres.clone(), channel: definitions::CHANNEL.to_string() }),
        region: Entities { services, running: HashMap::new() },
        interval: RESYNC_INTERVAL,
    };
    let manager = tokio::spawn(manager.run(received));

    let notify = Arc::new(LiveNotifier { reader, writer: writer.clone() });
    let notifier_counts = counts.clone();
    let is_ready = Arc::new(AtomicBool::new(false));
    let balancer = Balancer {
        postgres,
        class: LOCK_CLASS,
        application: APPLICATION_NAME.to_string(),
        orders,
        notifier: move || {
            println!("ninjapaws_monitors: this replica runs the notifier");
            let notifier =
                Notifier { backend: notify.clone(), interval: NOTIFY_INTERVAL, counts: notifier_counts.clone() };
            tokio::spawn(notifier.run())
        },
        interval: BALANCE_INTERVAL,
        counts: counts.clone(),
        ready: is_ready.clone(),
    };
    let (stop, stopping) = watch::channel(false);
    let balancer = tokio::spawn(balancer.run(stopping));

    let selfmon = SelfMonitor {
        tenant: s.selfmon_tenant.clone(),
        host: s.selfmon_host.clone(),
        interval: s.selfmon_interval,
        counts,
        clickhouse: writer,
    };
    let selfmon = tokio::spawn(selfmon.run());

    let mut router = Router::new();
    router.with_state(Ready(is_ready));
    router.get("/health/live", live);
    router.get("/health/ready", ready);
    let listener = TcpListener::bind(listen_on(&s.health_addr)).await?;
    let health = Server::builder().build().try_spawn_http(listener, router)?;
    println!("ninjapaws_monitors: health on {}", s.health_addr);

    terminated().await;
    println!("ninjapaws_monitors: stopping, handing the shards off");
    let _ = stop.send(true);
    let _ = balancer.await;
    let _ = manager.await;
    selfmon.abort();
    // The entities' last evaluations.
    records.close();
    records.wait().await;
    health.shutdown(Duration::from_secs(1)).await;
    Ok(())
}
