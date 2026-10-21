//! Keeps the entities of the shards this replica holds in step with Postgres; it holds nothing the
//! table does not, so a new owner starts from the table alone. Every replica runs one, for its own
//! shards.

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc::{self, UnboundedReceiver};
use tokio::task::JoinSet;
use tokio::time::{Instant, MissedTickBehavior};

use crate::definitions::{self, MonitorKey, Snapshot, parse_notification};
use crate::entity::{self, Backend, Command, Services};
use crate::shards::{Order, shard_of};

/// What the manager reads of Postgres; the service's is the `monitor` table, a test's are fakes.
pub trait Definitions: Send + Sync + 'static {
    fn snapshot(&self, since: Option<i64>) -> impl Future<Output = Result<Snapshot, String>> + Send;
    /// Sends each payload heard; returns, with why, when the connection ends.
    fn listen(&self, notified: mpsc::UnboundedSender<String>) -> impl Future<Output = String> + Send;
}

/// The `monitor` table and its notifications.
pub struct Postgres {
    pub pool: deadpool_postgres::Pool,
    pub config: tokio_postgres::Config,
    pub channel: String,
}

impl Definitions for Postgres {
    async fn snapshot(&self, since: Option<i64>) -> Result<Snapshot, String> {
        definitions::snapshot(&self.pool, since).await
    }

    async fn listen(&self, notified: mpsc::UnboundedSender<String>) -> String {
        definitions::listen(&self.config, &self.channel, notified).await
    }
}

/// The entities the manager sends to; a test's records what is sent.
pub trait Region: Send + 'static {
    /// Starts the monitor's entity when none runs.
    fn tell(&mut self, key: &MonitorKey, command: Command);
    /// Stops the shard's entities as a hand-off does; the future ends once they have stopped.
    fn hand_off(&mut self, shard: i32) -> impl Future<Output = ()> + Send + 'static;
    /// Stops every entity at once, what it runs dropped.
    fn kill_all(&mut self);
    /// Forgets the entities that stopped by themselves.
    fn sweep(&mut self);
}

/// An entity that has not stopped this long after its Stop is stopped with what it runs.
const HAND_OFF: Duration = Duration::from_secs(10);

pub struct Entities<B: Backend> {
    pub services: Arc<Services<B>>,
    pub running: HashMap<MonitorKey, entity::Handle>,
}

impl<B: Backend> Region for Entities<B> {
    fn tell(&mut self, key: &MonitorKey, command: Command) {
        let command = match self.running.get(key) {
            Some(handle) => match handle.sender.send(command) {
                Ok(()) => return,
                Err(refused) => refused.0,
            },
            None => command,
        };
        let handle = entity::spawn(key.clone(), self.services.clone());
        let _ = handle.sender.send(command);
        self.running.insert(key.clone(), handle);
    }

    fn hand_off(&mut self, shard: i32) -> impl Future<Output = ()> + Send + 'static {
        let keys: Vec<MonitorKey> = self.running.keys().filter(|k| shard_of(k) == shard).cloned().collect();
        let stopping: Vec<entity::Handle> = keys.iter().filter_map(|k| self.running.remove(k)).collect();
        for handle in &stopping {
            let _ = handle.sender.send(Command::Stop);
        }
        async move {
            let deadline = Instant::now() + HAND_OFF;
            for mut handle in stopping {
                if tokio::time::timeout_at(deadline, &mut handle.task).await.is_err() {
                    handle.task.abort();
                }
            }
        }
    }

    fn kill_all(&mut self) {
        for (_, handle) in self.running.drain() {
            handle.task.abort();
        }
    }

    fn sweep(&mut self) {
        self.running.retain(|_, handle| !handle.task.is_finished());
    }
}

pub struct Manager<D, R> {
    pub definitions: Arc<D>,
    pub region: R,
    /// The resync interval, also the LISTEN retry delay.
    pub interval: Duration,
}

/// What the manager's loop keeps between messages.
struct Running {
    held: BTreeSet<i32>,
    latest: Option<Snapshot>,
    reading: bool,
    last_read: Option<i64>,
    /// The next read reaches a day back, for the deletions a shard's previous owner missed.
    reach_back: bool,
}

enum Event {
    Order(Order),
    Notified(String),
    Read(Option<i64>, Result<Snapshot, String>),
    Resync,
    ListenEnded(String),
    ListenAgain,
    Done,
}

impl<D: Definitions, R: Region> Manager<D, R> {
    /// Until the balancer's orders end.
    pub async fn run(mut self, mut orders: UnboundedReceiver<Order>) {
        let (notified, mut heard) = mpsc::unbounded_channel();
        let mut reads: JoinSet<(Option<i64>, Result<Snapshot, String>)> = JoinSet::new();
        let mut listening: JoinSet<String> = JoinSet::new();
        let mut listen_again: Option<Instant> = None;
        let mut resyncs = tokio::time::interval_at(Instant::now() + self.interval, self.interval);
        resyncs.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut state =
            Running { held: BTreeSet::new(), latest: None, reading: false, last_read: None, reach_back: false };

        self.listen(&mut listening, &notified);
        self.resync(&mut state, &mut reads);
        loop {
            // Orders first: a shard is held before the reads that follow are sent to it.
            let event = tokio::select! {
                biased;
                order = orders.recv() => order.map_or(Event::Done, Event::Order),
                Some(payload) = heard.recv() => Event::Notified(payload),
                Some(read) = reads.join_next() => match read {
                    Ok((since, result)) => Event::Read(since, result),
                    Err(e) => Event::Read(None, Err(e.to_string())),
                },
                Some(ended) = listening.join_next() => Event::ListenEnded(ended.unwrap_or_else(|e| e.to_string())),
                _ = resyncs.tick() => Event::Resync,
                _ = tokio::time::sleep_until(listen_again.unwrap_or_else(Instant::now)), if listen_again.is_some() => {
                    listen_again = None;
                    Event::ListenAgain
                }
            };
            match event {
                Event::Done => return,
                Event::Order(order) => self.order(&mut state, &mut reads, order),
                Event::Notified(payload) => match parse_notification(&payload) {
                    // The payload does not say whether it was a deletion; the entity reads the
                    // row either way, and a deleted one closes.
                    Some(s) => self.send(&state, &s.key, Command::Ensure(s.version)),
                    None => eprintln!("warning: not a monitor notification: {payload}"),
                },
                Event::Read(_, Ok(snapshot)) => {
                    state.reading = false;
                    state.last_read = Some(snapshot.at);
                    for s in &snapshot.live {
                        self.send(&state, &s.key, Command::Ensure(s.version));
                    }
                    for s in &snapshot.deleted {
                        self.send(&state, &s.key, Command::Delete(s.version));
                    }
                    state.latest = Some(snapshot);
                    self.region.sweep();
                    if state.reach_back {
                        self.resync(&mut state, &mut reads);
                    }
                }
                Event::Read(since, Err(error)) => {
                    state.reading = false;
                    // A failed read moves nothing: the next starts from the same place.
                    if since.is_none() {
                        state.reach_back = true;
                    }
                    eprintln!("warning: monitors could not be read, retrying in {:?}: {error}", self.interval);
                }
                Event::Resync => self.resync(&mut state, &mut reads),
                Event::ListenEnded(error) => {
                    eprintln!("warning: not listening for monitor changes, retrying in {:?}: {error}", self.interval);
                    listen_again = Some(Instant::now() + self.interval);
                }
                Event::ListenAgain => self.listen(&mut listening, &notified),
            }
        }
    }

    fn listen(&self, listening: &mut JoinSet<String>, notified: &mpsc::UnboundedSender<String>) {
        let (definitions, notified) = (self.definitions.clone(), notified.clone());
        listening.spawn(async move { definitions.listen(notified).await });
    }

    /// From one interval before the last snapshot, so a deletion committed during that read is
    /// not missed; the first after a start, or after a shard is taken, reaches a day back.
    fn resync(&self, state: &mut Running, reads: &mut JoinSet<(Option<i64>, Result<Snapshot, String>)>) {
        if state.reading {
            return;
        }
        state.reading = true;
        let since = match (state.reach_back, state.last_read) {
            (false, Some(at)) => Some(at - self.interval.as_millis() as i64),
            _ => None,
        };
        state.reach_back = false;
        let definitions = self.definitions.clone();
        reads.spawn(async move { (since, definitions.snapshot(since).await) });
    }

    fn send(&mut self, state: &Running, key: &MonitorKey, command: Command) {
        if state.held.contains(&shard_of(key)) {
            self.region.tell(key, command);
        }
    }

    fn order(
        &mut self,
        state: &mut Running,
        reads: &mut JoinSet<(Option<i64>, Result<Snapshot, String>)>,
        order: Order,
    ) {
        match order {
            Order::Take(shard) => {
                state.held.insert(shard);
                if let Some(snapshot) = &state.latest {
                    for s in snapshot.live.iter().filter(|s| shard_of(&s.key) == shard) {
                        self.region.tell(&s.key, Command::Ensure(s.version));
                    }
                    for s in snapshot.deleted.iter().filter(|s| shard_of(&s.key) == shard) {
                        self.region.tell(&s.key, Command::Delete(s.version));
                    }
                }
                // The first read reaches a day back anyway.
                if state.last_read.is_some() {
                    state.reach_back = true;
                    self.resync(state, reads);
                }
            }
            Order::Release(shard, done) => {
                state.held.remove(&shard);
                let stopping = self.region.hand_off(shard);
                tokio::spawn(async move {
                    stopping.await;
                    let _ = done.send(());
                });
            }
            Order::Lost => {
                state.held.clear();
                self.region.kill_all();
            }
        }
    }
}
