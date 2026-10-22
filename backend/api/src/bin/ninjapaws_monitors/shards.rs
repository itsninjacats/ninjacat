//! How replicas share the monitors: 100 shards, each held by the replica whose lock session holds
//! its Postgres advisory lock. A session that is lost frees its locks on the server, and the other
//! replicas take those shards over. A shard is released only after its entities have stopped, so
//! no shard has two owners while the lock session lives.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ninjapaws_storage::fnv;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio_postgres::NoTls;

use crate::definitions::MonitorKey;
use crate::telemetry::Counters;

/// A monitor's shard is its key's hash modulo this, so changing it moves every monitor and needs
/// every replica stopped first.
pub const SHARDS: i32 = 100;

/// The first key of NinjaCat's monitor locks; the second is the shard.
pub const LOCK_CLASS: i32 = 7100;

/// The second key of the notifier's lock, outside the shards.
pub const NOTIFIER_LOCK: i32 = SHARDS;

/// The lock sessions' `application_name`: the replicas are the sessions that carry it.
pub const APPLICATION_NAME: &str = "ninjacat-monitors";

/// FNV-1a, stable across processes and builds, as Rust's own hasher is not: every replica must
/// put a monitor in the same shard.
pub fn shard_of(key: &MonitorKey) -> i32 {
    (fnv::hash64(key.text().as_bytes()) % SHARDS as u64) as i32
}

/// What the balancer tells the manager.
pub enum Order {
    Take(i32),
    /// Answered once the shard's entities have stopped.
    Release(i32, oneshot::Sender<()>),
    /// The lock session is gone: every entity stops at once.
    Lost,
}

/// The session that holds this replica's locks.
pub struct Locks {
    client: tokio_postgres::Client,
    connection: JoinHandle<()>,
    ended: watch::Receiver<bool>,
    class: i32,
    application: String,
}

/// Postgres frees a session's locks only once it sees the session gone. A replica whose machine
/// died sends nothing more, so its socket is probed after 30 s of silence and given up after 30 s
/// more; until then, its shards have no owner.
const SERVER_KEEPALIVE: &str =
    "SET tcp_keepalives_idle = 30; SET tcp_keepalives_interval = 10; SET tcp_keepalives_count = 3";

impl Locks {
    pub async fn connect(config: &tokio_postgres::Config, class: i32, application: &str) -> Result<Locks, String> {
        let mut config = config.clone();
        config.application_name(application);
        let (client, connection) = config.connect(NoTls).await.map_err(|e| e.to_string())?;
        let (end, ended) = watch::channel(false);
        let connection = tokio::spawn(async move {
            let _ = connection.await;
            let _ = end.send(true);
        });
        client.batch_execute(SERVER_KEEPALIVE).await.map_err(|e| e.to_string())?;
        Ok(Locks { client, connection, ended, class, application: application.to_string() })
    }

    /// Once the connection has ended.
    pub async fn closed(&self) {
        let mut ended = self.ended.clone();
        let _ = ended.wait_for(|ended| *ended).await;
    }

    /// The session answers, within 5 s.
    pub async fn alive(&self) -> Result<(), String> {
        if self.connection.is_finished() {
            return Err("the connection closed".into());
        }
        match tokio::time::timeout(Duration::from_secs(5), self.client.simple_query("SELECT 1")).await {
            Ok(Ok(_)) => Ok(()),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err("it did not answer in 5 seconds".into()),
        }
    }

    pub async fn try_lock(&self, key: i32) -> Result<bool, String> {
        let row = self
            .client
            .query_one("SELECT pg_try_advisory_lock($1, $2)", &[&self.class, &key])
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.get(0))
    }

    pub async fn unlock(&self, key: i32) -> Result<(), String> {
        self.client
            .execute("SELECT pg_advisory_unlock($1, $2)", &[&self.class, &key])
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// The live replicas, this one included: the sessions of this database that carry the
    /// application name.
    pub async fn replicas(&self) -> Result<i64, String> {
        let row = self
            .client
            .query_one(
                "SELECT count(*) FROM pg_stat_activity WHERE datname = current_database() AND application_name = $1",
                &[&self.application],
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(row.get(0))
    }
}

impl Drop for Locks {
    fn drop(&mut self) {
        self.connection.abort();
    }
}

pub struct Balancer<N> {
    pub postgres: tokio_postgres::Config,
    pub class: i32,
    pub application: String,
    pub orders: mpsc::UnboundedSender<Order>,
    /// Starts the notifier; it runs until its task is aborted.
    pub notifier: N,
    pub interval: Duration,
    pub counts: Arc<Counters>,
    /// Whether the lock session is up: /health/ready.
    pub ready: Arc<AtomicBool>,
}

impl<N: Fn() -> JoinHandle<()>> Balancer<N> {
    /// Holds a lock session and a fair share of the shards until `shutdown` is set, then hands
    /// every shard off and ends the session.
    pub async fn run(self, mut shutdown: watch::Receiver<bool>) {
        let mut failing = false;
        while !*shutdown.borrow() {
            let locks = match Locks::connect(&self.postgres, self.class, &self.application).await {
                Ok(locks) => locks,
                Err(e) => {
                    if !failing {
                        eprintln!(
                            "warning: no lock session for the monitor shards, retrying every {:?}: {e}",
                            self.interval
                        );
                    }
                    failing = true;
                    let _ = tokio::time::timeout(self.interval, shutdown.changed()).await;
                    continue;
                }
            };
            failing = false;
            self.ready.store(true, Ordering::Relaxed);
            let mut held = BTreeSet::new();
            let mut notifier: Option<JoinHandle<()>> = None;
            let ended = self.hold(&locks, &mut held, &mut notifier, &mut shutdown).await;
            self.ready.store(false, Ordering::Relaxed);
            if let Some(task) = notifier {
                task.abort();
            }
            match ended {
                Ok(()) => {
                    let all = held.len();
                    self.release(&locks, &mut held, all).await;
                    return;
                }
                Err(e) => {
                    eprintln!("warning: the lock session of the monitor shards is lost, its monitors stop: {e}");
                    let _ = self.orders.send(Order::Lost);
                    self.counts.shards.set(0);
                }
            }
        }
    }

    /// Balances every interval until shutdown (Ok) or until the session fails (Err).
    async fn hold(
        &self,
        locks: &Locks,
        held: &mut BTreeSet<i32>,
        notifier: &mut Option<JoinHandle<()>>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), String> {
        let mut every = tokio::time::interval(self.interval);
        loop {
            tokio::select! {
                _ = every.tick() => {}
                _ = shutdown.changed() => return Ok(()),
                // Stopped at once rather than at the next tick: the shards may have another owner
                // already.
                _ = locks.closed() => return Err("the connection closed".into()),
            }
            if *shutdown.borrow() {
                return Ok(());
            }
            locks.alive().await?;
            self.balance(locks, held).await?;
            if notifier.is_none() && locks.try_lock(NOTIFIER_LOCK).await? {
                *notifier = Some((self.notifier)());
            }
        }
    }

    /// Aims at ceil(100 / replicas) shards: takes free ones, releases the extras.
    async fn balance(&self, locks: &Locks, held: &mut BTreeSet<i32>) -> Result<(), String> {
        let replicas = locks.replicas().await?.max(1);
        self.counts.replicas.set(replicas);
        let target = (SHARDS as u64).div_ceil(replicas as u64) as usize;
        if held.len() > target {
            self.release(locks, held, held.len() - target).await;
        }
        for shard in 0..SHARDS {
            if held.len() >= target {
                break;
            }
            if !held.contains(&shard) && locks.try_lock(shard).await? {
                held.insert(shard);
                let _ = self.orders.send(Order::Take(shard));
            }
        }
        self.counts.shards.set(held.len() as i64);
        Ok(())
    }

    /// The last `count` shards held: their entities stopped first, then their locks released.
    async fn release(&self, locks: &Locks, held: &mut BTreeSet<i32>, count: usize) {
        let extras: Vec<i32> = held.iter().rev().take(count).copied().collect();
        let mut stopped = Vec::new();
        for shard in &extras {
            let (done, waited) = oneshot::channel();
            if self.orders.send(Order::Release(*shard, done)).is_ok() {
                stopped.push(waited);
            }
        }
        for waited in stopped {
            let _ = waited.await;
        }
        for shard in extras {
            held.remove(&shard);
            if let Err(e) = locks.unlock(shard).await {
                eprintln!("warning: shard {shard} could not be released, it is freed with the session: {e}");
            }
        }
        self.counts.shards.set(held.len() as i64);
    }
}
