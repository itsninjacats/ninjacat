//! Shard ownership through advisory locks on the dev Postgres, each test under a lock class and an
//! application name of its own. The manager is a
//! fake that notes what it was ordered; Postgres's `pg_locks` says who holds what.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ninjapaws_storage::fnv;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::{connect, dev_postgres_url, eventually, unique};
use crate::definitions::MonitorKey;
use crate::shards::{Balancer, NOTIFIER_LOCK, Order, SHARDS, shard_of};
use crate::telemetry::Counters;

const PATIENCE: Duration = Duration::from_secs(20);
const INTERVAL: Duration = Duration::from_millis(200);

#[test]
fn a_shard_is_a_stable_hash_of_the_key_and_the_keys_spread_over_every_shard() {
    // FNV-1a's published test vectors.
    assert_eq!(fnv::hash64(b""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv::hash64(b"a"), 0xaf63_dc4c_8601_ec8c);

    let mut per_shard = BTreeMap::new();
    for id in 1..=10_000 {
        *per_shard.entry(shard_of(&MonitorKey { tenant: "acme".into(), id })).or_insert(0) += 1;
    }
    assert_eq!(per_shard.len(), SHARDS as usize);
    assert!(per_shard.values().all(|n| (50..=150).contains(n)), "{per_shard:?}");
}

/// The dev Postgres, as the tests look at it; None when it is not running.
struct Lab {
    config: tokio_postgres::Config,
    admin: tokio_postgres::Client,
    class: i32,
    application: String,
}

impl Lab {
    async fn new() -> Option<Lab> {
        let url = dev_postgres_url();
        let mut config: tokio_postgres::Config = url.parse().unwrap();
        config.connect_timeout(Duration::from_secs(3));
        let admin = match connect(&config).await {
            Ok(admin) => admin,
            Err(e) => {
                // CI sets it: there a missing database is a broken job, not a test to skip.
                if std::env::var_os("NINJACAT_TEST_REQUIRE_DATABASES").is_some() {
                    panic!("Postgres is not reachable at {url}: {e}");
                }
                eprintln!("skipped: Postgres is not reachable at {url}: {e}");
                return None;
            }
        };
        let class = 1_000_000 + (fnv::hash64(unique("class").as_bytes()) % 1_000_000_000) as i32;
        Some(Lab { config, admin, class, application: unique("monitors_test") })
    }

    /// Each session's shards, by its pid.
    async fn locks(&self) -> BTreeMap<i32, BTreeSet<i64>> {
        let rows = self
            .admin
            .query(
                "SELECT pid, objid::int8 FROM pg_locks WHERE locktype = 'advisory' AND granted \
                 AND classid::int8 = $1 AND objsubid = 2",
                &[&(self.class as i64)],
            )
            .await
            .unwrap();
        let mut held: BTreeMap<i32, BTreeSet<i64>> = BTreeMap::new();
        for r in rows {
            let shard: i64 = r.get(1);
            if shard < SHARDS as i64 {
                held.entry(r.get(0)).or_default().insert(shard);
            }
        }
        held
    }

    /// The shards held, session by session, sorted by count: every shard by one session each.
    async fn shared(&self) -> Vec<usize> {
        let held = self.locks().await;
        let mut all: BTreeSet<i64> = BTreeSet::new();
        let mut counts = Vec::new();
        for shards in held.values() {
            counts.push(shards.len());
            all.extend(shards);
        }
        counts.sort();
        if all.len() != counts.iter().sum::<usize>() {
            panic!("a shard is held twice: {held:?}");
        }
        counts
    }

    fn replica(&self) -> Replica {
        let (orders, mut received) = mpsc::unbounded_channel();
        let held = Arc::new(Mutex::new(BTreeSet::new()));
        let lost = Arc::new(AtomicUsize::new(0));
        let releasing = Arc::new(AtomicUsize::new(0));
        let release_gate = watch::Sender::new(true);
        let manager = {
            let (held, lost, releasing, release_gate) =
                (held.clone(), lost.clone(), releasing.clone(), release_gate.clone());
            tokio::spawn(async move {
                while let Some(order) = received.recv().await {
                    match order {
                        Order::Take(shard) => {
                            held.lock().unwrap().insert(shard);
                        }
                        Order::Release(shard, done) => {
                            releasing.fetch_add(1, Ordering::SeqCst);
                            let (held, mut open) = (held.clone(), release_gate.subscribe());
                            tokio::spawn(async move {
                                let _ = open.wait_for(|open| *open).await;
                                held.lock().unwrap().remove(&shard);
                                let _ = done.send(());
                            });
                        }
                        Order::Lost => {
                            held.lock().unwrap().clear();
                            lost.fetch_add(1, Ordering::SeqCst);
                        }
                    }
                }
            })
        };
        let notifiers = Arc::new(AtomicUsize::new(0));
        let started = notifiers.clone();
        let counts = Arc::new(Counters::default());
        let ready = Arc::new(AtomicBool::new(false));
        let balancer = Balancer {
            postgres: self.config.clone(),
            class: self.class,
            application: self.application.clone(),
            orders,
            notifier: move || {
                started.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(std::future::pending::<()>())
            },
            interval: INTERVAL,
            counts: counts.clone(),
            ready: ready.clone(),
        };
        let (stop, stopping) = watch::channel(false);
        let task = tokio::spawn(balancer.run(stopping));
        Replica { held, lost, releasing, release_gate, notifiers, counts, ready, stop, task, _manager: manager }
    }
}

struct Replica {
    /// What the fake manager was told it holds.
    held: Arc<Mutex<BTreeSet<i32>>>,
    lost: Arc<AtomicUsize>,
    releasing: Arc<AtomicUsize>,
    /// A Release is answered, as when the shard's entities have stopped, while it is open.
    release_gate: watch::Sender<bool>,
    notifiers: Arc<AtomicUsize>,
    counts: Arc<Counters>,
    ready: Arc<AtomicBool>,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
    _manager: JoinHandle<()>,
}

impl Replica {
    fn held(&self) -> usize {
        self.held.lock().unwrap().len()
    }

    fn notifiers(&self) -> usize {
        self.notifiers.load(Ordering::SeqCst)
    }
}

/// Polls until `check` holds; its last answer is the test's.
async fn eventually_async<F: Future<Output = bool>>(check: impl Fn() -> F) {
    let deadline = tokio::time::Instant::now() + PATIENCE;
    while !check().await {
        if tokio::time::Instant::now() > deadline {
            panic!("the condition did not hold within {PATIENCE:?}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

#[tokio::test]
async fn a_replica_alone_takes_every_shard_and_runs_the_notifier() {
    let Some(lab) = Lab::new().await else { return };
    let a = lab.replica();

    eventually(PATIENCE, || a.held() == SHARDS as usize).await;
    assert_eq!(lab.shared().await, vec![SHARDS as usize]);
    eventually(PATIENCE, || a.notifiers() == 1).await;
    assert!(a.ready.load(Ordering::SeqCst));
    assert_eq!((a.counts.replicas.value(), a.counts.shards.value()), (1.0, 100.0));
    a.task.abort();
}

#[tokio::test]
async fn two_replicas_share_the_shards_each_held_once_and_one_runs_the_notifier() {
    let Some(lab) = Lab::new().await else { return };
    let a = lab.replica();
    eventually(PATIENCE, || a.held() == SHARDS as usize).await;
    let b = lab.replica();

    eventually_async(|| async { lab.shared().await == vec![50, 50] }).await;
    eventually(PATIENCE, || a.held() == 50 && b.held() == 50).await;
    let (ours, theirs) = (a.held.lock().unwrap().clone(), b.held.lock().unwrap().clone());
    assert!(ours.is_disjoint(&theirs));
    assert_eq!(a.notifiers() + b.notifiers(), 1);
    assert_eq!((b.counts.replicas.value(), b.counts.shards.value()), (2.0, 50.0));
    a.task.abort();
    b.task.abort();
}

#[tokio::test]
async fn three_replicas_share_the_shards() {
    let Some(lab) = Lab::new().await else { return };
    let replicas = [lab.replica(), lab.replica(), lab.replica()];

    eventually_async(|| async { lab.shared().await == vec![32, 34, 34] }).await;
    eventually(PATIENCE, || replicas.iter().map(Replica::held).sum::<usize>() == SHARDS as usize).await;
    assert_eq!(replicas.iter().map(Replica::notifiers).sum::<usize>(), 1);
    for r in &replicas {
        r.task.abort();
    }
}

#[tokio::test]
async fn a_shard_is_released_only_after_its_entities_have_stopped() {
    let Some(lab) = Lab::new().await else { return };
    let a = lab.replica();
    eventually(PATIENCE, || a.held() == SHARDS as usize).await;
    a.release_gate.send_replace(false);
    let b = lab.replica();

    eventually(PATIENCE, || a.releasing.load(Ordering::SeqCst) == 50).await;
    tokio::time::sleep(INTERVAL * 3).await;
    // Its entities have not stopped, so the shards are still locked to it.
    assert_eq!(lab.shared().await, vec![SHARDS as usize]);
    assert_eq!(b.held(), 0);

    a.release_gate.send_replace(true);
    eventually_async(|| async { lab.shared().await == vec![50, 50] }).await;
    eventually(PATIENCE, || a.held() == 50 && b.held() == 50).await;
    a.task.abort();
    b.task.abort();
}

#[tokio::test]
async fn the_shards_of_a_replica_that_dies_are_taken_over() {
    let Some(lab) = Lab::new().await else { return };
    let a = lab.replica();
    eventually(PATIENCE, || a.notifiers() == 1).await;
    let b = lab.replica();
    eventually_async(|| async { lab.shared().await == vec![50, 50] }).await;

    // As a process that is killed: its lock session closes, and Postgres frees its locks.
    a.task.abort();
    eventually_async(|| async { lab.shared().await == vec![SHARDS as usize] }).await;
    eventually(PATIENCE, || b.held() == SHARDS as usize).await;
    // The notifier's lock went with the session, so the other replica runs the notifier now.
    eventually(PATIENCE, || b.notifiers() == 1).await;
    assert_eq!(b.counts.replicas.value(), 1.0);
    b.task.abort();
}

#[tokio::test]
async fn a_replica_whose_lock_session_is_lost_stops_its_monitors_and_joins_again() {
    let Some(lab) = Lab::new().await else { return };
    let a = lab.replica();
    eventually(PATIENCE, || a.held() == SHARDS as usize).await;

    let killed = lab
        .admin
        .execute(
            "SELECT pg_terminate_backend(pid) FROM pg_stat_activity WHERE application_name = $1",
            &[&lab.application],
        )
        .await
        .unwrap();
    assert_eq!(killed, 1);

    eventually(PATIENCE, || a.lost.load(Ordering::SeqCst) == 1).await;
    eventually(PATIENCE, || a.held() == SHARDS as usize).await;
    eventually_async(|| async { lab.shared().await == vec![SHARDS as usize] }).await;
    assert!(a.ready.load(Ordering::SeqCst));
    eventually(PATIENCE, || a.notifiers() == 2).await;
    a.task.abort();
}

#[tokio::test]
async fn a_replica_that_stops_hands_every_shard_off_and_frees_its_locks() {
    let Some(lab) = Lab::new().await else { return };
    let a = lab.replica();
    eventually(PATIENCE, || a.held() == SHARDS as usize).await;
    eventually(PATIENCE, || a.notifiers() == 1).await;

    a.stop.send_replace(true);
    tokio::time::timeout(PATIENCE, a.task).await.unwrap().unwrap();
    assert_eq!(a.held.lock().unwrap().len(), 0);
    assert!(lab.locks().await.is_empty());
    let notifier = lab
        .admin
        .query(
            "SELECT 1 FROM pg_locks WHERE locktype = 'advisory' AND classid::int8 = $1 AND objid::int8 = $2",
            &[&(lab.class as i64), &(NOTIFIER_LOCK as i64)],
        )
        .await
        .unwrap();
    assert!(notifier.is_empty());
}
