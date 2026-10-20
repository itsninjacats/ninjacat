//! The manager with the entities replaced by a probe and Postgres by fakes over values the test
//! sets, and once by a scratch schema of the dev Postgres itself (F#: `ManagerTests.fs`); and the
//! entities' region handing a shard off.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::sync::{Semaphore, oneshot, watch};

use super::entity::{Fakes, T0, key, services};
use super::{QUIET, ScratchPostgres, WAIT, eventually, expect, expect_none, fish, unique};
use crate::definitions::{self, MonitorKey, Seen, Snapshot};
use crate::entity::Command;
use crate::manager::{Definitions, Entities, Manager, Postgres, Region};
use crate::shards::{Order, SHARDS, shard_of};

fn seen(tenant: &str, id: i64, version: i32) -> Seen {
    Seen { key: MonitorKey { tenant: tenant.into(), id }, version }
}

fn empty() -> Snapshot {
    Snapshot { at: T0, live: Vec::new(), deleted: Vec::new() }
}

/// What the fake Postgres answers, and what the manager asked of it.
struct FakePostgres {
    /// Handed out in turn; the last one is repeated.
    snapshots: Mutex<VecDeque<Result<Snapshot, String>>>,
    /// Every read waits while it is false.
    snapshot_gate: watch::Sender<bool>,
    since: Mutex<Vec<Option<i64>>>,
    notify: Mutex<Option<UnboundedSender<String>>>,
    /// The LISTEN connections opened; the next one fails when set.
    listens: AtomicUsize,
    listen_fails: AtomicBool,
}

impl FakePostgres {
    fn new(snapshots: Vec<Result<Snapshot, String>>) -> FakePostgres {
        FakePostgres {
            snapshots: Mutex::new(snapshots.into()),
            snapshot_gate: watch::Sender::new(true),
            since: Mutex::new(Vec::new()),
            notify: Mutex::new(None),
            listens: AtomicUsize::new(0),
            listen_fails: AtomicBool::new(false),
        }
    }

    fn since(&self) -> Vec<Option<i64>> {
        self.since.lock().unwrap().clone()
    }

    fn notify(&self, payload: &str) {
        self.notify.lock().unwrap().as_ref().expect("listening").send(payload.into()).unwrap();
    }
}

impl Definitions for FakePostgres {
    async fn snapshot(&self, since: Option<i64>) -> Result<Snapshot, String> {
        let next = {
            self.since.lock().unwrap().push(since);
            let mut snapshots = self.snapshots.lock().unwrap();
            match snapshots.len() {
                0 => Ok(empty()),
                1 => snapshots[0].clone(),
                _ => snapshots.pop_front().unwrap(),
            }
        };
        let mut open = self.snapshot_gate.subscribe();
        let _ = open.wait_for(|open| *open).await;
        next
    }

    async fn listen(&self, notified: UnboundedSender<String>) -> String {
        self.listens.fetch_add(1, Ordering::SeqCst);
        if self.listen_fails.swap(false, Ordering::SeqCst) {
            return "the connection broke".into();
        }
        *self.notify.lock().unwrap() = Some(notified);
        std::future::pending::<()>().await;
        unreachable!()
    }
}

/// The region, as what it was asked.
struct Probe {
    told: UnboundedSender<(String, Command)>,
    handed_off: UnboundedSender<i32>,
    killed: UnboundedSender<()>,
}

impl Region for Probe {
    fn tell(&mut self, key: &MonitorKey, command: Command) {
        let _ = self.told.send((key.text(), command));
    }

    fn hand_off(&mut self, shard: i32) -> impl Future<Output = ()> + Send + 'static {
        let _ = self.handed_off.send(shard);
        async {}
    }

    fn kill_all(&mut self) {
        let _ = self.killed.send(());
    }

    fn sweep(&mut self) {}
}

struct Started {
    told: UnboundedReceiver<(String, Command)>,
    handed_off: UnboundedReceiver<i32>,
    killed: UnboundedReceiver<()>,
    orders: UnboundedSender<Order>,
}

/// Starts the manager over `definitions`, holding the shards given.
fn start<D: Definitions>(definitions: Arc<D>, interval: Duration, held: impl IntoIterator<Item = i32>) -> Started {
    let (told, told_rx) = mpsc::unbounded_channel();
    let (handed_off, handed_off_rx) = mpsc::unbounded_channel();
    let (killed, killed_rx) = mpsc::unbounded_channel();
    let (orders, received) = mpsc::unbounded_channel();
    for shard in held {
        orders.send(Order::Take(shard)).unwrap();
    }
    let manager = Manager { definitions, region: Probe { told, handed_off, killed }, interval };
    tokio::spawn(manager.run(received));
    Started { told: told_rx, handed_off: handed_off_rx, killed: killed_rx, orders }
}

fn ensure(key: &str, version: i32) -> (String, Command) {
    (key.into(), Command::Ensure(version))
}

const HOUR: Duration = Duration::from_secs(3600);

#[tokio::test]
async fn a_resync_ensures_every_live_monitor_and_deletes_every_recent_deletion() {
    let snapshot =
        Snapshot { at: T0, live: vec![seen("acme", 1, 1), seen("a/b", 2, 5)], deleted: vec![seen("acme", 3, 4)] };
    let postgres = Arc::new(FakePostgres::new(vec![Ok(snapshot)]));
    let mut m = start(postgres.clone(), HOUR, 0..SHARDS);

    assert_eq!(expect(&mut m.told).await, ensure("acme/1", 1));
    assert_eq!(expect(&mut m.told).await, ensure("a/b/2", 5));
    assert_eq!(expect(&mut m.told).await, ("acme/3".into(), Command::Delete(4)));
    expect_none(&mut m.told).await;
    // The first resync after a start reaches a day back.
    assert_eq!(postgres.since(), vec![None]);
}

#[tokio::test]
async fn nothing_is_sent_for_a_shard_not_held_and_a_shard_taken_starts_from_the_latest_snapshot() {
    let acme1 = MonitorKey { tenant: "acme".into(), id: 1 };
    let postgres = Arc::new(FakePostgres::new(vec![Ok(Snapshot { live: vec![seen("acme", 1, 1)], ..empty() })]));
    let others = (0..SHARDS).filter(|s| *s != shard_of(&acme1));
    let mut m = start(postgres.clone(), HOUR, others);
    eventually(WAIT, || postgres.since().len() == 1).await;
    expect_none(&mut m.told).await;

    m.orders.send(Order::Take(shard_of(&acme1))).unwrap();
    assert_eq!(expect(&mut m.told).await, ensure("acme/1", 1));
    // Taken over, the shard may have deletions its previous owner missed: the next read reaches a
    // day back.
    eventually(WAIT, || postgres.since().len() == 2).await;
    assert_eq!(postgres.since(), vec![None, None]);
}

#[tokio::test]
async fn each_resync_reads_deletions_from_one_interval_before_the_last_snapshot() {
    let interval = Duration::from_millis(200);
    let t1 = T0 + 30_000;
    let postgres = Arc::new(FakePostgres::new(vec![
        Ok(empty()),
        Err("Postgres is down".into()),
        Ok(Snapshot { at: t1, ..empty() }),
    ]));
    let _m = start(postgres.clone(), interval, 0..SHARDS);
    eventually(WAIT, || postgres.since().len() >= 4).await;

    // A failed read moves nothing: the next one starts from the same place.
    assert_eq!(postgres.since()[..4], [None, Some(T0 - 200), Some(T0 - 200), Some(t1 - 200)]);
}

#[tokio::test]
async fn every_resync_ensures_the_live_monitors_again() {
    let postgres = Arc::new(FakePostgres::new(vec![Ok(Snapshot { live: vec![seen("acme", 1, 1)], ..empty() })]));
    let mut m = start(postgres, Duration::from_millis(200), 0..SHARDS);
    for _ in 0..3 {
        assert_eq!(expect(&mut m.told).await, ensure("acme/1", 1));
    }
}

#[tokio::test]
async fn a_resync_does_not_start_while_the_last_read_is_still_running() {
    let postgres = Arc::new(FakePostgres::new(vec![Ok(Snapshot { live: vec![seen("acme", 1, 1)], ..empty() })]));
    postgres.snapshot_gate.send_replace(false);
    let mut m = start(postgres.clone(), Duration::from_millis(100), 0..SHARDS);

    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(postgres.since().len(), 1);

    postgres.snapshot_gate.send_replace(true);
    expect(&mut m.told).await;
    eventually(WAIT, || postgres.since().len() >= 2).await;
}

#[tokio::test]
async fn a_notification_ensures_its_version_at_once() {
    let postgres = Arc::new(FakePostgres::new(Vec::new()));
    let mut m = start(postgres.clone(), HOUR, 0..SHARDS);
    eventually(WAIT, || postgres.notify.lock().unwrap().is_some()).await;

    postgres.notify("not a monitor");
    postgres.notify("acme/7/4");
    assert_eq!(expect(&mut m.told).await, ensure("acme/7", 4));
    expect_none(&mut m.told).await;
}

#[tokio::test]
async fn a_broken_listen_connection_is_opened_again_after_one_interval() {
    let postgres = Arc::new(FakePostgres::new(Vec::new()));
    postgres.listen_fails.store(true, Ordering::SeqCst);
    let mut m = start(postgres.clone(), Duration::from_secs(1), 0..SHARDS);
    let listens = || postgres.listens.load(Ordering::SeqCst);
    eventually(WAIT, || listens() == 1).await;

    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(listens(), 1);
    eventually(WAIT, || listens() == 2).await;

    postgres.notify("acme/7/4");
    fish(&mut m.told, |(key, _)| key == "acme/7").await;
}

#[tokio::test]
async fn a_released_shard_is_handed_off_and_its_monitors_are_sent_nothing_more() {
    let acme1 = MonitorKey { tenant: "acme".into(), id: 1 };
    let postgres = Arc::new(FakePostgres::new(vec![Ok(Snapshot { live: vec![seen("acme", 1, 1)], ..empty() })]));
    let mut m = start(postgres.clone(), Duration::from_millis(100), 0..SHARDS);
    assert_eq!(expect(&mut m.told).await, ensure("acme/1", 1));

    let (done, stopped) = oneshot::channel();
    m.orders.send(Order::Release(shard_of(&acme1), done)).unwrap();
    assert_eq!(expect(&mut m.handed_off).await, shard_of(&acme1));
    tokio::time::timeout(WAIT, stopped).await.unwrap().unwrap();

    let reads = postgres.since().len();
    eventually(WAIT, || postgres.since().len() >= reads + 2).await;
    while let Ok(told) = m.told.try_recv() {
        assert_ne!(told.0, "acme/1", "sent after its shard was released");
    }
    expect_none(&mut m.told).await;
}

#[tokio::test]
async fn a_lost_lock_session_stops_every_entity_and_sends_nothing_more() {
    let postgres = Arc::new(FakePostgres::new(vec![Ok(Snapshot { live: vec![seen("acme", 1, 1)], ..empty() })]));
    let mut m = start(postgres.clone(), Duration::from_millis(100), 0..SHARDS);
    expect(&mut m.told).await;

    m.orders.send(Order::Lost).unwrap();
    expect(&mut m.killed).await;
    let reads = postgres.since().len();
    eventually(WAIT, || postgres.since().len() >= reads + 2).await;
    while m.told.try_recv().is_ok() {}
    expect_none(&mut m.told).await;
}

#[tokio::test]
async fn the_dev_postgres_reaches_the_region_through_a_resync_and_through_listen() {
    let Some(pg) = ScratchPostgres::new().await else { return };
    let tenant = "acme";
    let id = pg.insert(tenant, "avg(last_5m):avg:system.cpu.user{*} > 90", "now()").await;
    let key = format!("{tenant}/{id}");
    // A channel of its own, so the dev stack's monitor service hears nothing.
    let channel = unique("monitors_test");
    let postgres =
        Postgres { pool: definitions::pool(pg.config.clone()), config: pg.config.clone(), channel: channel.clone() };
    let mut m = start(Arc::new(postgres), HOUR, 0..SHARDS);
    fish(&mut m.told, |(k, c)| *k == key && *c == Command::Delete(3)).await;

    // The LISTEN connection opens asynchronously and a notification sent before it is lost, so
    // one is sent until one is heard.
    let deadline = tokio::time::Instant::now() + WAIT;
    loop {
        pg.client.execute("SELECT pg_notify($1, $2)", &[&channel, &format!("{key}/4")]).await.unwrap();
        match tokio::time::timeout(Duration::from_millis(500), m.told.recv()).await {
            Ok(Some((k, Command::Ensure(4)))) if k == key => break,
            _ if tokio::time::Instant::now() > deadline => panic!("no Ensure of version 4 arrived"),
            _ => {}
        }
    }
}

#[tokio::test]
async fn a_hand_off_waits_for_the_transitions_in_flight() {
    let (fakes, mut inserts, _records) = Fakes::new();
    let fakes = Arc::new(fakes);
    fakes.insert_gate.send_replace(false);
    let mut region =
        Entities { services: services(&fakes, Arc::new(Semaphore::new(8)), HOUR), running: HashMap::new() };
    region.tell(&key(), Command::Ensure(1));
    region.tell(&key(), Command::Tick(T0));
    eventually(WAIT, || fakes.executions.load(Ordering::SeqCst) == 1).await;
    tokio::time::sleep(QUIET).await;

    let handing_off = tokio::spawn(region.hand_off(shard_of(&key())));
    tokio::time::sleep(Duration::from_millis(500)).await;
    // The hand-off waits for the insert; stopping at once would have dropped it.
    assert!(!handing_off.is_finished());
    assert!(region.running.is_empty());

    fakes.insert_gate.send_replace(true);
    assert!(expect(&mut inserts).await.stored);
    tokio::time::timeout(WAIT, handing_off).await.unwrap().unwrap();
}

#[tokio::test]
async fn an_entity_that_stopped_is_started_again_by_the_next_message() {
    let (fakes, _inserts, _records) = Fakes::new();
    let fakes = Arc::new(fakes);
    *fakes.row.lock().unwrap() = None;
    let services = services(&fakes, Arc::new(Semaphore::new(8)), HOUR);
    let counts = services.counts.clone();
    let mut region = Entities { services, running: HashMap::new() };

    // Missing from Postgres, it stops after its load.
    region.tell(&key(), Command::Ensure(1));
    eventually(WAIT, || counts.entities.value() == 0.0).await;
    eventually(WAIT, || region.running.values().all(|h| h.task.is_finished())).await;
    region.sweep();
    assert!(region.running.is_empty());

    region.tell(&key(), Command::Ensure(1));
    eventually(WAIT, || fakes.reads.load(Ordering::SeqCst) == 2).await;
}
