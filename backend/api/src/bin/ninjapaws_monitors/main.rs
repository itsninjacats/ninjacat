//! Evaluates the monitors. Replicas share the monitors through Postgres advisory
//! locks (`shards.rs`); each runs the entities of the shards it holds, and one of them the
//! notifier. Health is all it serves over HTTP.
mod definitions;
mod entity;
mod manager;
mod notifier;
mod reads;
mod rows;
mod selfmon;
mod shards;
mod store;
mod telemetry;
#[cfg(test)]
mod tests;
mod writer;

use std::collections::HashMap;
use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::TimeDelta;
use ninjapaws_engine::Sql;
use ninjapaws_engine::query::compile::Row;
use ninjapaws_engine::query::execute::Executor;
use ninjapaws_engine::query::plan::{DEFAULT_RETENTION, Retention};
use tako::Server;
use tako::StatusCode;
use tako::extractors::state::State;
use tako::router::Router;
use tako::types::BoxError;
use tokio::net::TcpListener;
use tokio::sync::{Semaphore, mpsc, watch};

use crate::definitions::{MonitorKey, MonitorRow};
use crate::entity::Services;
use crate::manager::{Entities, Manager, Postgres};
use crate::notifier::{Notifier, Polled};
use crate::reads::ClickHouse;
use crate::rows::{EvaluationRow, MetricPoint, NotificationRow, TransitionRow};
use crate::selfmon::SelfMonitor;
use crate::shards::{APPLICATION_NAME, Balancer, LOCK_CLASS};
use crate::store::History;
use crate::telemetry::Counters;
use crate::writer::{DEFAULT_LIMITS, METRICS_LIMITS, TableWriter};

const RESYNC_INTERVAL: Duration = Duration::from_secs(30);
const NOTIFY_INTERVAL: Duration = Duration::from_secs(5);
/// How often a replica counts the replicas and takes or releases shards.
const BALANCE_INTERVAL: Duration = Duration::from_secs(10);
const RETRY: Duration = Duration::from_secs(30);

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

fn var_or(name: &str, default: &str) -> String {
    env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
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

/// `:8559`, `*:8559` and `0.0.0.0:8559` are every interface, as the settings have always spelled it.
fn listen_on(addr: &str) -> String {
    match addr.rsplit_once(':') {
        Some(("" | "*", port)) => format!("0.0.0.0:{port}"),
        _ => addr.to_string(),
    }
}

/// A whole number of days, up to a century. A set value outside 1–36500 stops the start: falling
/// back to the default could set a shorter TTL than meant, and ClickHouse would delete the
/// difference.
fn days(name: &str, default: TimeDelta) -> Result<TimeDelta, String> {
    match var_or(name, "").as_str() {
        "" => Ok(default),
        raw => match raw.trim().parse::<i64>() {
            Ok(n) if (1..=36500).contains(&n) => Ok(TimeDelta::days(n)),
            _ => Err(format!("{name} must be a whole number of days from 1 to 36500, got \"{raw}\"")),
        },
    }
}

/// A step of whole minutes reads the rollup whatever its age, so a rollup kept shorter than the
/// raw points would answer nothing where the raw table still holds them.
fn metrics_retention() -> Result<Retention, String> {
    let raw = days("NINJACAT_METRICS_RETENTION_DAYS", DEFAULT_RETENTION.raw)?;
    let rollup = days("NINJACAT_METRICS_ROLLUP_RETENTION_DAYS", DEFAULT_RETENTION.rollup)?;
    if rollup < raw {
        return Err(format!(
            "NINJACAT_METRICS_ROLLUP_RETENTION_DAYS ({}) must not be shorter than NINJACAT_METRICS_RETENTION_DAYS ({})",
            rollup.num_days(),
            raw.num_days()
        ));
    }
    Ok(Retention { raw, rollup })
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// The entities' world: the `monitor` table, ClickHouse's reads and the monitor tables' writes.
struct Live {
    pool: deadpool_postgres::Pool,
    reader: ClickHouse,
    writes: clickhouse::Client,
    evaluations: Arc<TableWriter<EvaluationRow>>,
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
        writer::insert(&self.writes, "monitor_transitions", &rows).await
    }

    fn record(&self, rows: Vec<EvaluationRow>) {
        self.evaluations.add(rows);
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
    writes: clickhouse::Client,
}

impl notifier::Backend for LiveNotifier {
    async fn cursor(&self) -> Result<Option<i64>, String> {
        notifier::cursor(&self.reader).await
    }

    async fn due(&self, since: i64) -> Result<Polled, String> {
        notifier::due(&self.reader, since).await
    }

    async fn write(&self, rows: Vec<NotificationRow>) -> Result<(), String> {
        writer::insert(&self.writes, "monitor_notifications", &rows).await
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
    let s = settings()?;
    let mut postgres: tokio_postgres::Config = s.database_url.parse()?;
    // A connection gives up after 15 s, and an idle one is probed, so a dead
    // server or link is noticed.
    if postgres.get_connect_timeout().is_none() {
        postgres.connect_timeout(Duration::from_secs(15));
    }
    postgres.keepalives(true).keepalives_idle(Duration::from_secs(30));
    let pool = definitions::pool(postgres.clone());
    let reader = ClickHouse::new(&s.clickhouse_addr, &s.clickhouse_db, &s.clickhouse_user, &s.clickhouse_password);
    let writes = writer::client(&s.clickhouse_addr, &s.clickhouse_db, &s.clickhouse_user, &s.clickhouse_password);
    let evaluations = TableWriter::start(writes.clone(), "monitor_evaluations", DEFAULT_LIMITS);
    let metrics: Arc<TableWriter<MetricPoint>> = TableWriter::start(writes.clone(), "metrics", METRICS_LIMITS);
    let counts = Arc::new(Counters::default());

    let services = Arc::new(Services {
        backend: Arc::new(Live {
            pool: pool.clone(),
            reader: reader.clone(),
            writes: writes.clone(),
            evaluations: evaluations.clone(),
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

    let notify = Arc::new(LiveNotifier { reader, writes });
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
        writers: vec![evaluations.clone(), metrics.clone()],
        metrics: metrics.clone(),
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
    // The entities' last evaluations, and the last sample.
    evaluations.stop().await;
    metrics.stop().await;
    health.shutdown(Duration::from_secs(1)).await;
    Ok(())
}
