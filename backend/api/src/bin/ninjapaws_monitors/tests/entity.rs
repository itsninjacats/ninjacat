//! The entity driven by hand: ticks sent to it, Postgres, ClickHouse and the writers replaced by
//! fakes over values the test sets.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::TimeDelta;
use ninjapaws_engine::monitor::check;
use ninjapaws_engine::monitor::definition::Monitor;
use ninjapaws_engine::monitor::evaluate::next_tick;
use ninjapaws_engine::monitor::parse;
use ninjapaws_engine::monitor::state::{self, Group, Status};
use ninjapaws_engine::query::compile::Row;
use ninjapaws_engine::query::execute::Executor;
use ninjapaws_engine::query::plan::DEFAULT_RETENTION;
use ninjapaws_engine::{Sql, SqlValue};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{Semaphore, watch};

use super::{WAIT, eventually, expect, expect_none, fish};
use crate::definitions::{MonitorKey, MonitorRow};
use crate::entity::{self, Backend, Command, Handle, Services};
use crate::rows::{EvaluationRow, TransitionRow};
use crate::store::History;
use crate::telemetry::Counters;

/// 2026-10-13T12:00:00Z.
pub const T0: i64 = 1_791_892_800_000;
const MINUTE: i64 = 60_000;

const CPU: &str = "avg(last_5m):avg:system.cpu.user{*} > 90";
const CRITICAL_90: &str = r#"{"thresholds":{"critical":90}}"#;

pub fn key() -> MonitorKey {
    MonitorKey { tenant: "acme".into(), id: 7 }
}

fn monitor_row(version: i32, query: &str, options: &str) -> MonitorRow {
    MonitorRow {
        key: key(),
        version,
        monitor_type: "query alert".into(),
        query: query.into(),
        options: options.into(),
        updated_by: Some("user-1".into()),
        deleted_by: None,
        deleted_at: None,
    }
}

fn deleted_row(version: i32, at: i64) -> Option<MonitorRow> {
    Some(MonitorRow {
        deleted_by: Some("user-3".into()),
        deleted_at: Some(at),
        ..monitor_row(version, CPU, CRITICAL_90)
    })
}

/// One group in Alert since `since`, as a restart would load it.
fn alert_since(since: i64, last_seen: i64, hash: &str) -> History {
    let group = Group {
        key: "*".into(),
        tags: Vec::new(),
        state: Some(Status::Alert),
        since,
        first_triggered_at: Some(since),
        last_triggered_at: Some(since),
        first_seen: since,
        last_seen,
        last_point: last_seen,
        interrupted: None,
    };
    History { groups: BTreeMap::from([("*".to_string(), group)]), query_hash: Some(hash.into()) }
}

fn hash_of(query: &str) -> String {
    state::query_hash(&parse::query(query).unwrap())
}

/// One insert attempt, and whether the fake stored it.
#[derive(Debug)]
pub struct Attempt {
    pub rows: Vec<TransitionRow>,
    pub stored: bool,
}

type Answer = Box<dyn Fn(&Sql, f64) -> Vec<Row> + Send + Sync>;

/// What the services answer; a test changes it between ticks.
pub struct Fakes {
    pub row: Mutex<Option<MonitorRow>>,
    pub history: Mutex<History>,
    pub value: Mutex<f64>,
    /// What a query answers, from its SQL and `value`.
    pub answer: Mutex<Answer>,
    /// The frequency the entity last waited for the gate by.
    pub frequency: Mutex<TimeDelta>,
    pub clock: Mutex<i64>,
    /// Inserts that fail before one is stored.
    pub failing_inserts: Mutex<u32>,
    pub execute_fails: AtomicBool,
    /// Queries and the definition read wait while these are false.
    pub execute_gate: watch::Sender<bool>,
    pub definition_gate: watch::Sender<bool>,
    pub insert_gate: watch::Sender<bool>,
    pub executions: AtomicUsize,
    pub reads: AtomicUsize,
    inserts: UnboundedSender<Attempt>,
    records: UnboundedSender<Vec<EvaluationRow>>,
}

impl Fakes {
    pub fn new() -> (Fakes, UnboundedReceiver<Attempt>, UnboundedReceiver<Vec<EvaluationRow>>) {
        let (inserts, inserted) = mpsc::unbounded_channel();
        let (records, recorded) = mpsc::unbounded_channel();
        let fakes = Fakes {
            row: Mutex::new(Some(monitor_row(1, CPU, CRITICAL_90))),
            history: Mutex::new(History::default()),
            value: Mutex::new(95.0),
            answer: Mutex::new(Box::new(rows_for)),
            frequency: Mutex::new(TimeDelta::zero()),
            clock: Mutex::new(T0),
            failing_inserts: Mutex::new(0),
            execute_fails: AtomicBool::new(false),
            execute_gate: watch::Sender::new(true),
            definition_gate: watch::Sender::new(true),
            insert_gate: watch::Sender::new(true),
            executions: AtomicUsize::new(0),
            reads: AtomicUsize::new(0),
            inserts,
            records,
        };
        (fakes, inserted, recorded)
    }

    fn executions(&self) -> usize {
        self.executions.load(Ordering::SeqCst)
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

async fn opened(gate: &watch::Sender<bool>) {
    let mut open = gate.subscribe();
    let _ = open.wait_for(|open| *open).await;
}

/// Answers the engine's SQL with one point per bucket of the window read, all of `value`, for a
/// query without groups.
pub fn rows_for(sql: &Sql, value: f64) -> Vec<Row> {
    let number = |name: &str| {
        sql.parameters
            .iter()
            .find_map(|(n, v)| match v {
                SqlValue::Int64(x)
                    if n.strip_prefix(name).is_some_and(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_digit())) =>
                {
                    Some(*x)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("no {name} parameter in {sql:?}"))
    };
    let (from, to, step) = (number("from"), number("to"), number("step") * 1000);
    (from / step * step..to)
        .step_by(step as usize)
        .filter(|bucket| *bucket >= from)
        .map(|bucket| Row { groups: Vec::new(), series_id: Some(1), bucket_ms: bucket, value })
        .collect()
}

impl Executor for Fakes {
    async fn rows(&self, sql: Sql) -> Result<Vec<Row>, String> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        opened(&self.execute_gate).await;
        if self.execute_fails.load(Ordering::SeqCst) {
            return Err("ClickHouse is down".into());
        }
        let value = *self.value.lock().unwrap();
        Ok((self.answer.lock().unwrap())(&sql, value))
    }
}

impl Backend for Fakes {
    async fn definition(&self, _: &MonitorKey) -> Result<Option<MonitorRow>, String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        opened(&self.definition_gate).await;
        Ok(self.row.lock().unwrap().clone())
    }

    async fn history(&self, _: &MonitorKey, _: i64) -> Result<History, String> {
        Ok(self.history.lock().unwrap().clone())
    }

    async fn insert(&self, rows: Vec<TransitionRow>) -> Result<(), String> {
        opened(&self.insert_gate).await;
        let fails = {
            let mut failing = self.failing_inserts.lock().unwrap();
            let fails = *failing > 0;
            if fails {
                *failing -= 1;
            }
            fails
        };
        let _ = self.inserts.send(Attempt { rows, stored: !fails });
        if fails { Err("the insert timed out".into()) } else { Ok(()) }
    }

    fn record(&self, rows: Vec<EvaluationRow>) {
        let _ = self.records.send(rows);
    }

    fn now(&self) -> i64 {
        *self.clock.lock().unwrap()
    }

    fn gate_wait(&self, frequency: TimeDelta) -> Duration {
        *self.frequency.lock().unwrap() = frequency;
        Duration::from_millis(200)
    }
}

pub fn services(fakes: &Arc<Fakes>, gate: Arc<Semaphore>, resync: Duration) -> Arc<Services<Fakes>> {
    Arc::new(Services {
        backend: fakes.clone(),
        gate,
        retention: DEFAULT_RETENTION,
        max_groups: 1000,
        retry: Duration::from_millis(200),
        resync,
        counts: Arc::new(Counters::default()),
    })
}

struct Test {
    fakes: Arc<Fakes>,
    inserts: UnboundedReceiver<Attempt>,
    records: UnboundedReceiver<Vec<EvaluationRow>>,
    gate: Arc<Semaphore>,
    resync: Duration,
    counts: Option<Arc<Counters>>,
}

impl Test {
    fn new() -> Test {
        let (fakes, inserts, records) = Fakes::new();
        Test {
            fakes: Arc::new(fakes),
            inserts,
            records,
            gate: Arc::new(Semaphore::new(8)),
            resync: Duration::from_secs(3600),
            counts: None,
        }
    }

    fn start(&mut self) -> Handle {
        let services = services(&self.fakes, self.gate.clone(), self.resync);
        self.counts = Some(services.counts.clone());
        entity::spawn(key(), services)
    }

    fn counts(&self) -> &Counters {
        self.counts.as_ref().expect("started")
    }

    async fn inserted(&mut self) -> Attempt {
        expect(&mut self.inserts).await
    }

    async fn single(&mut self) -> TransitionRow {
        let attempt = self.inserted().await;
        assert_eq!(attempt.rows.len(), 1, "{attempt:?}");
        attempt.rows.into_iter().next().unwrap()
    }

    async fn recorded(&mut self) -> Vec<EvaluationRow> {
        expect(&mut self.records).await
    }

    async fn recorded_one(&mut self) -> EvaluationRow {
        let rows = self.recorded().await;
        assert_eq!(rows.len(), 1, "{rows:?}");
        rows.into_iter().next().unwrap()
    }

    fn set_row(&self, row: Option<MonitorRow>) {
        *self.fakes.row.lock().unwrap() = row;
    }
}

fn send(entity: &Handle, command: Command) {
    entity.sender.send(command).unwrap();
}

async fn terminated(entity: Handle) {
    tokio::time::timeout(WAIT, entity.task).await.expect("the entity stops").unwrap();
}

#[tokio::test]
async fn a_tick_evaluates_and_stores_the_new_groups_transition_and_self_monitoring_counts_it() {
    let mut t = Test::new();
    *t.fakes.clock.lock().unwrap() = T0 + 2000;
    let entity = t.start();
    assert_eq!(t.counts().entities.value(), 1.0);
    send(&entity, Command::Tick(T0));

    let row = t.single().await;
    assert_eq!((row.tenant_id.as_str(), row.monitor_id, row.monitor_version), ("acme", 7, 1));
    assert_eq!(row.query_hash, hash_of(CPU));
    assert_eq!((row.group_key.as_str(), row.at), ("*", T0));
    assert_eq!(
        (row.from_state.as_str(), row.to_state.as_str(), row.reason.as_str(), row.crossed.as_str()),
        ("none", "Alert", "new", "critical")
    );
    assert_eq!(row.value, Some(95.0));
    assert_eq!((row.comparator.as_str(), row.critical, row.warning), (">", Some(90.0), None));
    assert_eq!(row.window_to, T0 - 30_000);
    assert_eq!(row.actor, None);

    let only = t.recorded_one().await;
    assert_eq!(
        (only.group_key.as_str(), only.value, only.state.as_str(), only.outcome.as_str()),
        ("*", Some(95.0), "Alert", "evaluated")
    );

    let counts = t.counts();
    assert_eq!(
        (
            counts.evaluated.value(),
            counts.errored.value(),
            counts.transitions_written.value(),
            counts.transitions_pending.value()
        ),
        (1.0, 0.0, 1.0, 0.0)
    );
    // The window ends 30 s before the tick.
    assert_eq!(counts.evaluation_lag.take(), 32.0);
    assert!(counts.evaluation_lag.take().is_nan());

    // Nobody can send to it any more, so it stops.
    let counts = t.counts.clone().unwrap();
    drop(entity.sender);
    tokio::time::timeout(WAIT, entity.task).await.unwrap().unwrap();
    assert_eq!(counts.entities.value(), 0.0);
}

#[tokio::test]
async fn a_service_check_monitor_counts_the_runs_every_minute_and_stores_the_counts_it_crossed() {
    let mut t = Test::new();
    let query = r#""kube.check".over("*").last(2).count_by_status()"#;
    let options = r#"{"thresholds":{"critical":2,"warning":1,"ok":1}}"#;
    t.set_row(Some(MonitorRow { monitor_type: "service check".into(), ..monitor_row(1, query, options) }));
    *t.fakes.answer.lock().unwrap() = Box::new(|_, _| {
        [10_000, 25_000]
            .iter()
            .map(|ago| Row { groups: Vec::new(), series_id: Some(1), bucket_ms: T0 - ago, value: 2.0 })
            .collect()
    });

    let entity = t.start();
    send(&entity, Command::Tick(T0));

    let row = t.single().await;
    assert_eq!(
        (row.monitor_kind.as_str(), row.group_key.as_str(), row.to_state.as_str(), row.crossed.as_str()),
        ("service_check", "*", "Alert", "critical")
    );
    assert_eq!(
        (row.value, row.comparator.as_str(), row.critical, row.warning, row.critical_recovery),
        (Some(2.0), "", Some(2.0), Some(1.0), None)
    );
    assert_eq!((row.window_from, row.window_to), (T0 - 2 * 3_600_000, T0));
    assert_eq!(*t.fakes.frequency.lock().unwrap(), TimeDelta::minutes(1));
    let c = check::definition(query, options).unwrap();
    assert_eq!(row.query_hash, state::monitor_hash(&Monitor::ServiceCheck(c)));
}

#[tokio::test]
async fn a_group_moves_only_when_its_state_changes() {
    let mut t = Test::new();
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    t.inserted().await;

    send(&entity, Command::Tick(T0 + MINUTE));
    expect_none(&mut t.inserts).await;

    *t.fakes.value.lock().unwrap() = 10.0;
    send(&entity, Command::Tick(T0 + 2 * MINUTE));
    let row = t.single().await;
    assert_eq!((row.from_state.as_str(), row.to_state.as_str(), row.reason.as_str()), ("Alert", "OK", "recovery"));
    assert_eq!(row.first_triggered_at, Some(T0));
}

#[tokio::test]
async fn a_failed_insert_is_retried_as_the_same_rows_before_anything_new_is_evaluated() {
    let mut t = Test::new();
    *t.fakes.failing_inserts.lock().unwrap() = 1;
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    let failed = t.inserted().await;
    assert!(!failed.stored);
    // Not stored, so its evaluation is not recorded either.
    expect_none(&mut t.records).await;
    let counts = t.counts.clone().unwrap();
    let written =
        || (counts.transitions_failed.value(), counts.transitions_pending.value(), counts.transitions_written.value());
    eventually(WAIT, || written() == (1.0, 1.0, 0.0)).await;

    send(&entity, Command::Tick(T0 + MINUTE));
    let retried = t.inserted().await;
    assert!(retried.stored);
    assert_eq!(failed.rows, retried.rows);
    assert_eq!(t.fakes.executions(), 1);
    eventually(WAIT, || written() == (1.0, 0.0, 1.0)).await;
    assert_eq!(counts.skipped_pending.value(), 1.0);

    fish(&mut t.records, |rows| {
        rows.iter().any(|r| r.group_key.is_empty() && r.outcome == "skipped" && r.at == T0 + MINUTE)
    })
    .await;

    // Stored, so adopted: the same value moves nothing.
    send(&entity, Command::Tick(T0 + 2 * MINUTE));
    expect_none(&mut t.inserts).await;
    assert_eq!(t.fakes.executions(), 2);
}

#[tokio::test]
async fn an_evaluation_that_judged_no_group_counts_as_skipped() {
    let mut t = Test::new();
    *t.fakes.value.lock().unwrap() = f64::NAN;
    let entity = t.start();
    send(&entity, Command::Tick(T0));

    let row = t.recorded_one().await;
    assert_eq!((row.group_key.as_str(), row.outcome.as_str()), ("*", "skipped"));
    expect_none(&mut t.inserts).await;
    assert_eq!((t.counts().evaluated.value(), t.counts().skipped_window.value()), (0.0, 1.0));
}

#[tokio::test]
async fn an_anomaly_monitor_keeps_its_models_until_they_are_due_for_a_refit() {
    let mut t = Test::new();
    let robust = "avg(last_4h):anomalies(avg:system.cpu.user{*}, 'robust', 2, alert_window='last_15m', interval=60, seasonality='hourly') >= 1";
    t.set_row(Some(monitor_row(1, robust, "")));
    let entity = t.start();

    // The history read to fit and the window read; then the window alone until the hourly
    // season's ten minutes are up.
    for (tick, executions) in [(T0, 2), (T0 + MINUTE, 3), (T0 + 10 * MINUTE, 5)] {
        send(&entity, Command::Tick(tick));
        fish(&mut t.records, |rows| rows.iter().any(|r| r.at == tick)).await;
        assert_eq!(t.fakes.executions(), executions, "after the tick at {tick}");
    }
}

#[tokio::test]
async fn a_tick_while_the_evaluation_runs_is_skipped() {
    let mut t = Test::new();
    t.fakes.execute_gate.send_replace(false);
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    eventually(WAIT, || t.fakes.executions() == 1).await;

    send(&entity, Command::Tick(T0 + MINUTE));
    let skipped = t.recorded_one().await;
    assert_eq!((skipped.group_key.as_str(), skipped.outcome.as_str(), skipped.at), ("", "skipped", T0 + MINUTE));
    assert!(skipped.detail.starts_with("busy"), "{}", skipped.detail);

    t.fakes.execute_gate.send_replace(true);
    let row = t.single().await;
    assert_eq!(row.at, T0);
    assert_eq!(t.fakes.executions(), 1);
    assert_eq!(t.counts().skipped_busy.value(), 1.0);
}

#[tokio::test]
async fn messages_before_the_load_are_stashed() {
    let mut t = Test::new();
    t.fakes.definition_gate.send_replace(false);
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    expect_none(&mut t.inserts).await;

    t.fakes.definition_gate.send_replace(true);
    assert_eq!(t.single().await.at, T0);
}

#[tokio::test]
async fn loads_wait_for_a_slot_of_the_replicas_gate() {
    let mut t = Test::new();
    t.gate = Arc::new(Semaphore::new(0));
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    expect_none(&mut t.inserts).await;

    t.gate.add_permits(1);
    t.inserted().await;
}

#[tokio::test]
async fn an_evaluation_that_fails_is_recorded_and_changes_nothing() {
    let mut t = Test::new();
    t.fakes.execute_fails.store(true, Ordering::SeqCst);
    let entity = t.start();
    send(&entity, Command::Tick(T0));

    let failed = t.recorded_one().await;
    assert_eq!((failed.group_key.as_str(), failed.outcome.as_str(), failed.state.as_str()), ("", "error", "none"));
    assert!(failed.detail.contains("ClickHouse is down"), "{}", failed.detail);
    expect_none(&mut t.inserts).await;
}

#[tokio::test]
async fn a_newer_version_with_another_query_closes_the_groups_after_the_running_commit() {
    let mut t = Test::new();
    t.fakes.execute_gate.send_replace(false);
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    eventually(WAIT, || t.fakes.executions() == 1).await;

    let changed = "avg(last_5m):avg:system.cpu.system{*} > 90";
    t.set_row(Some(MonitorRow { updated_by: Some("user-2".into()), ..monitor_row(2, changed, CRITICAL_90) }));
    send(&entity, Command::Ensure(2));
    t.fakes.execute_gate.send_replace(true);

    let created = t.single().await;
    assert_eq!((created.from_state.as_str(), created.to_state.as_str(), created.monitor_version), ("none", "Alert", 1));

    let closed = t.single().await;
    assert_eq!(
        (closed.from_state.as_str(), closed.to_state.as_str(), closed.reason.as_str()),
        ("Alert", "OK", "definition_changed")
    );
    assert_eq!(
        (closed.actor.as_deref(), closed.monitor_version, closed.query_hash.clone()),
        (Some("user-2"), 2, hash_of(changed))
    );
    assert!(closed.at > created.at);
    // Outside a tick no window was judged.
    assert_eq!(closed.window_to, 0);

    // The closed group starts again under the new query.
    send(&entity, Command::Tick(T0 + MINUTE));
    let reopened = t.single().await;
    assert_eq!(
        (reopened.from_state.as_str(), reopened.reason.as_str(), reopened.query_hash.clone()),
        ("none", "new", hash_of(changed))
    );
}

#[tokio::test]
async fn a_newer_version_with_the_same_query_keeps_the_groups() {
    let mut t = Test::new();
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    t.inserted().await;

    t.set_row(Some(monitor_row(2, "avg(last_5m):avg:system.cpu.user{*} > 80", r#"{"thresholds":{"critical":80}}"#)));
    send(&entity, Command::Ensure(2));
    expect_none(&mut t.inserts).await;

    // Still Alert, now judged by the new threshold.
    *t.fakes.value.lock().unwrap() = 85.0;
    send(&entity, Command::Tick(T0 + MINUTE));
    expect_none(&mut t.inserts).await;
}

async fn a_deleted_monitor_closes_its_groups_and_stops(via_delete: bool) {
    let mut t = Test::new();
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    t.inserted().await;

    t.set_row(deleted_row(2, T0 + MINUTE));
    // An Ensure is what the manager sends for the panel's notification of a delete.
    send(&entity, if via_delete { Command::Delete(2) } else { Command::Ensure(2) });

    let closed = t.single().await;
    assert_eq!(
        (closed.from_state.as_str(), closed.to_state.as_str(), closed.reason.as_str(), closed.actor.as_deref()),
        ("Alert", "OK", "deleted", Some("user-3"))
    );
    assert_eq!(closed.monitor_version, 2);
    terminated(entity).await;
}

#[tokio::test]
async fn a_deleted_monitor_closes_its_groups_and_stops_on_a_delete() {
    a_deleted_monitor_closes_its_groups_and_stops(true).await;
}

#[tokio::test]
async fn a_deleted_monitor_closes_its_groups_and_stops_on_an_ensure_of_the_deleting_version() {
    a_deleted_monitor_closes_its_groups_and_stops(false).await;
}

#[tokio::test]
async fn a_monitor_missing_from_postgres_stops() {
    let mut t = Test::new();
    t.set_row(None);
    let entity = t.start();
    terminated(entity).await;
    expect_none(&mut t.inserts).await;
}

#[tokio::test]
async fn a_change_at_start_comes_after_the_tick_already_begun_and_that_tick_is_not_evaluated() {
    let mut t = Test::new();
    let (at, fire_at) = next_tick(&key().text(), TimeDelta::minutes(1), None, T0);
    let applied = fire_at - 200;
    assert!(applied > at);
    *t.fakes.clock.lock().unwrap() = applied;
    // Loaded under another query hash, so the groups are closed at once.
    *t.fakes.history.lock().unwrap() = alert_since(T0 - 3_600_000, T0, "an older query");

    let _entity = t.start();
    let closed = t.single().await;
    assert_eq!(
        (closed.from_state.as_str(), closed.to_state.as_str(), closed.reason.as_str(), closed.actor.as_deref()),
        ("Alert", "OK", "definition_changed", Some("user-1"))
    );
    assert_eq!(closed.at, applied);
    // The next tick is a minute on; `at` would sort before the change.
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(t.inserts.try_recv().is_err());
    assert_eq!(t.fakes.executions(), 0);
}

#[tokio::test]
async fn a_restart_under_the_same_query_hash_with_the_same_reading_stores_nothing() {
    let mut t = Test::new();
    *t.fakes.history.lock().unwrap() = alert_since(T0 - 3_600_000, T0 - MINUTE, &hash_of(CPU));

    let entity = t.start();
    send(&entity, Command::Tick(T0));

    let evaluated = t.recorded_one().await;
    assert_eq!(
        (evaluated.group_key.as_str(), evaluated.state.as_str(), evaluated.outcome.as_str()),
        ("*", "Alert", "evaluated")
    );
    expect_none(&mut t.inserts).await;
}

#[tokio::test]
async fn a_deletion_during_an_evaluation_closes_the_groups_after_its_commit() {
    let mut t = Test::new();
    t.fakes.execute_gate.send_replace(false);
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    eventually(WAIT, || t.fakes.executions() == 1).await;

    t.set_row(deleted_row(2, T0));
    send(&entity, Command::Delete(2));
    expect_none(&mut t.inserts).await;
    t.fakes.execute_gate.send_replace(true);

    let created = t.single().await;
    assert_eq!(
        (created.from_state.as_str(), created.to_state.as_str(), created.reason.as_str()),
        ("none", "Alert", "new")
    );
    let closed = t.single().await;
    assert_eq!(
        (closed.from_state.as_str(), closed.to_state.as_str(), closed.reason.as_str()),
        ("Alert", "OK", "deleted")
    );
    assert!(closed.at > created.at);
    terminated(entity).await;
}

#[tokio::test]
async fn a_newer_version_waits_for_the_pending_rows_to_be_stored() {
    let mut t = Test::new();
    *t.fakes.failing_inserts.lock().unwrap() = 1;
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    let failed = t.inserted().await;

    let changed = "avg(last_5m):avg:system.cpu.system{*} > 90";
    t.set_row(Some(monitor_row(2, changed, CRITICAL_90)));
    send(&entity, Command::Ensure(2));
    expect_none(&mut t.inserts).await;

    send(&entity, Command::Tick(T0 + MINUTE));
    let retried = t.inserted().await;
    assert_eq!(failed.rows, retried.rows);

    let closed = t.single().await;
    assert_eq!(
        (closed.from_state.as_str(), closed.to_state.as_str(), closed.reason.as_str(), closed.query_hash.clone()),
        ("Alert", "OK", "definition_changed", hash_of(changed))
    );
    assert_eq!(t.fakes.executions(), 1);
}

#[tokio::test]
async fn a_deleted_monitor_read_first_keeps_the_thresholds_on_its_closing_rows() {
    let mut t = Test::new();
    t.set_row(deleted_row(3, T0));
    *t.fakes.history.lock().unwrap() = alert_since(T0 - 3_600_000, T0, &hash_of(CPU));

    let entity = t.start();
    let closed = t.single().await;
    assert_eq!((closed.reason.as_str(), closed.comparator.as_str(), closed.critical), ("deleted", ">", Some(90.0)));
    terminated(entity).await;
}

#[tokio::test]
async fn a_tick_without_a_gate_slot_within_the_wait_is_skipped() {
    let mut t = Test::new();
    t.gate = Arc::new(Semaphore::new(1));
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    t.inserted().await;
    t.recorded().await;

    let taken = t.gate.clone().try_acquire_owned().expect("the slot is free");
    send(&entity, Command::Tick(T0 + MINUTE));
    let skipped = t.recorded_one().await;
    assert_eq!((skipped.group_key.as_str(), skipped.outcome.as_str()), ("", "skipped"));
    assert!(skipped.detail.starts_with("gate"), "{}", skipped.detail);
    assert_eq!(t.fakes.executions(), 1);
    drop(taken);
}

#[tokio::test]
async fn a_hand_off_before_the_load_stops_at_once() {
    let mut t = Test::new();
    t.fakes.definition_gate.send_replace(false);
    let entity = t.start();
    send(&entity, Command::Stop);
    terminated(entity).await;
}

#[tokio::test]
async fn a_hand_off_during_an_evaluation_with_nothing_to_store_stops_at_once() {
    let mut t = Test::new();
    t.fakes.execute_gate.send_replace(false);
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    eventually(WAIT, || t.fakes.executions() == 1).await;

    send(&entity, Command::Stop);
    terminated(entity).await;
    expect_none(&mut t.inserts).await;
}

#[tokio::test]
async fn a_hand_off_tries_the_pending_rows_once_more_then_stops() {
    let mut t = Test::new();
    *t.fakes.failing_inserts.lock().unwrap() = 5;
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    let failed = t.inserted().await;

    send(&entity, Command::Stop);
    let last = t.inserted().await;
    assert_eq!(failed.rows, last.rows);
    terminated(entity).await;
    expect_none(&mut t.inserts).await;
    assert_eq!(t.counts().transitions_pending.value(), 0.0);
}

#[tokio::test]
async fn the_timer_fires_at_the_aligned_tick() {
    let mut t = Test::new();
    let (at, fire_at) = next_tick(&key().text(), TimeDelta::minutes(1), None, T0);
    *t.fakes.clock.lock().unwrap() = fire_at - 200;
    let _entity = t.start();
    assert_eq!(t.single().await.at, at);
}

#[tokio::test]
async fn three_resyncs_without_ensure_read_the_definition_again_and_a_lost_deletion_closes_it() {
    let mut t = Test::new();
    t.resync = Duration::from_millis(100);
    let entity = t.start();
    send(&entity, Command::Tick(T0));
    t.inserted().await;

    t.set_row(deleted_row(2, T0 + MINUTE));

    let closed = t.single().await;
    assert_eq!(
        (closed.from_state.as_str(), closed.to_state.as_str(), closed.reason.as_str()),
        ("Alert", "OK", "deleted")
    );
    terminated(entity).await;
}

#[tokio::test]
async fn an_ensure_even_of_a_version_already_held_keeps_the_entity_from_reading_again() {
    let mut t = Test::new();
    t.resync = Duration::from_millis(300);
    let entity = t.start();
    eventually(WAIT, || t.fakes.reads() == 1).await;

    for _ in 0..8 {
        send(&entity, Command::Ensure(1));
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(t.fakes.reads(), 1);

    // Unheard for three resyncs, it reads again.
    eventually(WAIT, || t.fakes.reads() == 2).await;
}
