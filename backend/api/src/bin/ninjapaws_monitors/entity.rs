//! One monitor: evaluates on its ticks and stores transitions before adopting them; slow work runs
//! as spawned futures whose outcome comes back as a message.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;
use ninjapaws_engine::TenantId;
use ninjapaws_engine::monitor::check;
use ninjapaws_engine::monitor::definition::Monitor;
use ninjapaws_engine::monitor::evaluate::{self, Evaluation, Fit, Reading, SkipCause};
use ninjapaws_engine::monitor::state::{self, Group, Reason, Transition};
use ninjapaws_engine::monitor::validate;
use ninjapaws_engine::query::execute::Executor;
use ninjapaws_engine::query::plan::Retention;
use ninjapaws_storage::fnv;
use tokio::sync::Semaphore;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task::{JoinError, JoinHandle, JoinSet};
use tokio::time::Instant;

use crate::definitions::{MonitorKey, MonitorRow};
use crate::rows::{EvaluationRow, TransitionRow};
use crate::store::{self, History};
use crate::telemetry::{Counter, Counters};

/// What an entity is sent. A tick is the aligned tick `at`, the evaluation's identity.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Tick(i64),
    Ensure(i32),
    Delete(i32),
    /// The hand-off stop.
    Stop,
}

/// What an entity asks of the world; the service's is Postgres and ClickHouse, a test's are fakes.
pub trait Backend: Executor + Send + Sync + 'static {
    fn definition(&self, key: &MonitorKey) -> impl Future<Output = Result<Option<MonitorRow>, String>> + Send;
    fn history(&self, key: &MonitorKey, now: i64) -> impl Future<Output = Result<History, String>> + Send;
    fn insert(&self, rows: Vec<TransitionRow>) -> impl Future<Output = Result<(), String>> + Send;
    fn record(&self, rows: Vec<EvaluationRow>);
    fn now(&self) -> i64;
    /// How long a tick waits for a gate slot; the service waits the whole interval.
    fn gate_wait(&self, frequency: TimeDelta) -> Duration;
}

pub struct Services<B> {
    pub backend: Arc<B>,
    /// The evaluations, loads and inserts that run at once on this replica.
    pub gate: Arc<Semaphore>,
    pub retention: Retention,
    pub max_groups: usize,
    pub retry: Duration,
    /// After 3 to 4 silent resyncs the entity rereads, so a lost Delete still closes it.
    pub resync: Duration,
    pub counts: Arc<Counters>,
}

/// Fits the 10 s a hand-off gives an entity to stop.
const INSERT_TIMEOUT: Duration = Duration::from_secs(7);

enum Outcome {
    Loaded(Option<MonitorRow>, History),
    LoadFailed(String),
    Reloaded(Option<MonitorRow>),
    ReloadFailed(String),
    Evaluated(i64, Result<Evaluation, String>, Option<Fit>),
    GateRefused(i64),
    Committed,
    CommitFailed(String),
}

enum Timer {
    Tick(i64),
    Unheard,
    RetryLoad,
    RetryReload,
}

/// One of each timer: setting one replaces it, so a re-armed tick drops the tick it replaces.
#[derive(Default)]
struct Timers {
    tick: Option<(Instant, i64)>,
    unheard: Option<Instant>,
    load: Option<Instant>,
    reload: Option<Instant>,
}

impl Timers {
    /// When the first is due.
    fn next(&self) -> Option<Instant> {
        [self.tick.map(|(at, _)| at), self.unheard, self.load, self.reload].into_iter().flatten().min()
    }

    fn take_due(&mut self, now: Instant) -> Option<Timer> {
        if let Some((due, at)) = self.tick
            && due <= now
        {
            self.tick = None;
            return Some(Timer::Tick(at));
        }
        if self.unheard.is_some_and(|due| due <= now) {
            self.unheard = None;
            return Some(Timer::Unheard);
        }
        if self.load.is_some_and(|due| due <= now) {
            self.load = None;
            return Some(Timer::RetryLoad);
        }
        if self.reload.is_some_and(|due| due <= now) {
            self.reload = None;
            return Some(Timer::RetryReload);
        }
        None
    }
}

/// Transitions waiting to be stored, and the groups adopted once they are. A failed insert is
/// retried with exactly these rows. The evaluations go to the writer only then, so they never
/// show a state that was not stored.
struct Pending {
    rows: Vec<TransitionRow>,
    groups: BTreeMap<String, Group>,
    query_hash: Option<String>,
    evaluations: Vec<EvaluationRow>,
}

pub struct Handle {
    pub sender: UnboundedSender<Command>,
    pub task: JoinHandle<()>,
}

/// Starts the entity of `key`; it loads at once.
pub fn spawn<B: Backend>(key: MonitorKey, services: Arc<Services<B>>) -> Handle {
    let (sender, receiver) = mpsc::unbounded_channel();
    let entity = Entity::new(key, services);
    Handle { sender, task: tokio::spawn(entity.run(receiver)) }
}

struct Entity<B: Backend> {
    key: MonitorKey,
    services: Arc<Services<B>>,
    work: JoinSet<Outcome>,
    timers: Timers,
    stash: VecDeque<Command>,
    /// Unstashed, to be taken before the mailbox.
    unstashed: VecDeque<Command>,
    stopped: bool,
    unheard_after: Duration,

    loaded: bool,
    row: Option<MonitorRow>,
    definition: Result<Monitor, Vec<String>>,
    groups: BTreeMap<String, Group>,
    query_hash: Option<String>,
    last_at: Option<i64>,
    models: Option<Fit>,
    busy: bool,
    pending: Option<Pending>,
    reload_due: bool,
    closing: bool,
    stopping: bool,
    last_attempt_made: bool,
    reload_failing: bool,
}

// One at a time, so the size of the larger variant does not matter.
#[allow(clippy::large_enum_variant)]
enum Event {
    Command(Command),
    Done(Outcome),
    Timer(Timer),
    /// Nobody can send to the entity any more.
    Orphaned,
}

/// A panic in the entity's work is a bug: it ends the entity, and the next message for the
/// monitor starts it again.
fn outcome(done: Result<Outcome, JoinError>) -> Outcome {
    match done {
        Ok(outcome) => outcome,
        Err(e) => std::panic::resume_unwind(e.into_panic()),
    }
}

/// A check is evaluated every minute: its streaks are counted again from the stored runs.
fn frequency_of(m: &Monitor) -> TimeDelta {
    match m {
        Monitor::Metric(d) => evaluate::frequency(&d.query),
        Monitor::ServiceCheck(_) => TimeDelta::minutes(1),
    }
}

impl<B: Backend> Entity<B> {
    fn new(key: MonitorKey, services: Arc<Services<B>>) -> Self {
        // One resync's Ensure reaches every entity within milliseconds, so without a spread they
        // would all read Postgres in the same instant once the manager falls silent.
        let spread = (fnv::hash64(key.text().as_bytes()) % 1000) as f64 / 1000.0;
        let unheard_after = services.resync.mul_f64(3.0 + spread);
        services.counts.entities.add(1);
        Entity {
            key,
            services,
            work: JoinSet::new(),
            timers: Timers::default(),
            stash: VecDeque::new(),
            unstashed: VecDeque::new(),
            stopped: false,
            unheard_after,
            loaded: false,
            row: None,
            definition: Err(vec!["not loaded".to_string()]),
            groups: BTreeMap::new(),
            query_hash: None,
            last_at: None,
            models: None,
            busy: false,
            pending: None,
            reload_due: false,
            closing: false,
            stopping: false,
            last_attempt_made: false,
            reload_failing: false,
        }
    }

    async fn run(mut self, mut mailbox: UnboundedReceiver<Command>) {
        self.load();
        while !self.stopped {
            let event = match self.unstashed.pop_front() {
                Some(command) => Event::Command(command),
                None => self.next_event(&mut mailbox).await,
            };
            match event {
                Event::Command(command) => self.receive(command),
                Event::Done(outcome) => self.on_outcome(outcome),
                Event::Timer(timer) => self.on_timer(timer),
                Event::Orphaned => self.stopped = true,
            }
        }
    }

    async fn next_event(&mut self, mailbox: &mut UnboundedReceiver<Command>) -> Event {
        let due = self.timers.next();
        tokio::select! {
            command = mailbox.recv() => match command {
                Some(command) => Event::Command(command),
                None => Event::Orphaned,
            },
            Some(done) = self.work.join_next() => Event::Done(outcome(done)),
            _ = tokio::time::sleep_until(due.unwrap_or_else(Instant::now)), if due.is_some() => {
                let due = due.expect("slept only for a timer");
                Event::Timer(self.timers.take_due(due).expect("the timer slept for is due"))
            }
        }
    }

    fn backend(&self) -> Arc<B> {
        self.services.backend.clone()
    }

    fn record(&self, rows: Vec<EvaluationRow>) {
        if !rows.is_empty() {
            self.services.backend.record(rows);
        }
    }

    fn record_monitor(&self, at: i64, outcome: &str, detail: &str) {
        self.record(vec![store::monitor_row(&self.key, at, &self.groups, outcome, detail)]);
    }

    fn skipped(&self, counter: &Counter, at: i64, detail: &str) {
        counter.add(1);
        self.record_monitor(at, "skipped", detail);
    }

    fn failed(&self, at: i64, detail: &str) {
        self.services.counts.errored.add(1);
        self.record_monitor(at, "error", detail);
    }

    /// When a change outside a tick is stored: now, but after the last tick, so it is the group's
    /// latest row and never shares a tick's key.
    fn change_at(&self) -> i64 {
        let now = self.services.backend.now();
        match self.last_at {
            Some(last) if last >= now => last + 1,
            _ => now,
        }
    }

    fn rows_of(&self, actor: Option<&str>, hash: Option<&str>, transitions: &[Transition]) -> Vec<TransitionRow> {
        let version = self.row.as_ref().map_or(0, |r| r.version);
        let definition = self.definition.as_ref().ok();
        transitions
            .iter()
            .map(|t| store::transition_row(&self.key, version, hash.unwrap_or(""), definition, actor, t))
            .collect()
    }

    fn validated(&self, r: &MonitorRow) -> Result<Monitor, Vec<String>> {
        let now = self.services.backend.now();
        validate::by_type(self.services.retention, now, &r.monitor_type, &r.query, &r.options)
    }

    fn arm(&mut self) {
        let frequency = match &self.definition {
            Ok(d) => frequency_of(d),
            Err(_) => TimeDelta::minutes(1),
        };
        let now = self.services.backend.now();
        let (at, fire_at) = evaluate::next_tick(&self.key.text(), frequency, self.last_at, now);
        let wait = Duration::from_millis((fire_at - now).max(0) as u64);
        self.timers.tick = Some((Instant::now() + wait, at));
    }

    fn heard(&mut self) {
        self.timers.unheard = Some(Instant::now() + self.unheard_after);
    }

    fn load(&mut self) {
        let now = self.services.backend.now();
        let (backend, gate, key) = (self.backend(), self.services.gate.clone(), self.key.clone());
        self.work.spawn(async move {
            let _slot = gate.acquire_owned().await.expect("the gate is never closed");
            let found = match backend.definition(&key).await {
                Ok(found) => found,
                Err(e) => return Outcome::LoadFailed(e),
            };
            match backend.history(&key, now).await {
                Ok(history) => Outcome::Loaded(found, history),
                Err(e) => Outcome::LoadFailed(e),
            }
        });
    }

    fn reload(&mut self) {
        self.busy = true;
        self.reload_due = false;
        let (backend, gate, key) = (self.backend(), self.services.gate.clone(), self.key.clone());
        self.work.spawn(async move {
            let _slot = gate.acquire_owned().await.expect("the gate is never closed");
            match backend.definition(&key).await {
                Ok(found) => Outcome::Reloaded(found),
                Err(e) => Outcome::ReloadFailed(e),
            }
        });
    }

    fn insert_pending(&mut self) {
        let Some(p) = &self.pending else { return };
        self.busy = true;
        let (backend, gate, rows) = (self.backend(), self.services.gate.clone(), p.rows.clone());
        self.work.spawn(async move {
            let _slot = gate.acquire_owned().await.expect("the gate is never closed");
            match tokio::time::timeout(INSERT_TIMEOUT, backend.insert(rows)).await {
                Ok(Ok(())) => Outcome::Committed,
                Ok(Err(e)) => Outcome::CommitFailed(e),
                Err(_) => {
                    Outcome::CommitFailed(format!("the insert did not finish in {} seconds", INSERT_TIMEOUT.as_secs()))
                }
            }
        });
    }

    fn commit(
        &mut self,
        rows: Vec<TransitionRow>,
        next: BTreeMap<String, Group>,
        hash: Option<String>,
        evaluations: Vec<EvaluationRow>,
    ) {
        if rows.is_empty() {
            self.groups = next;
            self.query_hash = hash;
            self.record(evaluations);
        } else {
            self.services.counts.transitions_pending.add(rows.len() as i64);
            self.pending = Some(Pending { rows, groups: next, query_hash: hash, evaluations });
            self.insert_pending();
        }
    }

    fn close(&mut self, actor: Option<&str>) {
        let (next, transitions) = state::close(Reason::Deleted, self.change_at(), &self.groups);
        self.closing = true;
        let rows = self.rows_of(actor, self.query_hash.as_deref(), &transitions);
        self.commit(rows, next, self.query_hash.clone(), Vec::new());
    }

    fn apply(&mut self, r: MonitorRow) {
        self.definition = self.validated(&r);
        let version = r.version;
        let updated_by = r.updated_by.clone();
        self.row = Some(r);

        match &self.definition {
            Err(problems) => {
                eprintln!(
                    "warning: Monitor {} version {version} is not valid: {}",
                    self.key.text(),
                    problems.join("; ")
                );
            }
            Ok(d) => {
                let hash = state::monitor_hash(d);
                match &self.query_hash {
                    Some(previous) => {
                        let at = self.change_at();
                        let (next, transitions) = state::redefine(previous, d, at, &self.groups);
                        // The next tick must come after the change, or a tick already armed would
                        // store an older `at` under the new query.
                        if !transitions.is_empty() {
                            self.last_at = Some(at);
                        }
                        let rows = self.rows_of(updated_by.as_deref(), Some(&hash), &transitions);
                        self.commit(rows, next, Some(hash), Vec::new());
                    }
                    None => self.query_hash = Some(hash),
                }
            }
        }

        // Re-arming also drops a tick of the old timer, and a new window may tick at another
        // frequency.
        self.arm();
    }

    fn read(&mut self, found: Option<MonitorRow>) {
        match found {
            None => self.close(None),
            Some(r) if r.deleted_at.is_some() => {
                // Deleted before it was ever read: its open groups were judged by this definition
                // as far as anything says.
                if self.row.is_none() {
                    self.definition = self.validated(&r);
                }
                let actor = r.deleted_by.clone();
                self.row = Some(r);
                self.close(actor.as_deref());
            }
            Some(r) if self.row.as_ref().is_none_or(|held| r.version > held.version) => self.apply(r),
            Some(_) => {}
        }
    }

    fn settle(&mut self) {
        if self.busy {
        } else if self.stopping {
            if self.pending.is_some() && !self.last_attempt_made {
                self.last_attempt_made = true;
                self.insert_pending();
            } else {
                self.stopped = true;
            }
        } else if self.closing {
            if self.pending.is_none() {
                self.stopped = true;
            }
        } else if self.reload_due && self.pending.is_none() {
            self.reload();
        }
    }

    fn evaluate(&mut self, d: Monitor, at: i64) {
        self.busy = true;
        let wait = self.services.backend.gate_wait(frequency_of(&d));
        let previous = self.models.clone();
        let services = self.services.clone();
        let tenant = TenantId(self.key.tenant.clone());
        self.work.spawn(async move {
            let Ok(slot) = tokio::time::timeout(wait, services.gate.clone().acquire_owned()).await else {
                return Outcome::GateRefused(at);
            };
            let started = std::time::Instant::now();
            let execute = &*services.backend;
            let outcome = match &d {
                Monitor::ServiceCheck(c) => {
                    Outcome::Evaluated(at, check::evaluate(execute, &tenant, services.max_groups, c, at).await, None)
                }
                Monitor::Metric(md) => {
                    match evaluate::fit(execute, &tenant, services.retention, md, previous.as_ref(), at).await {
                        Err(e) => Outcome::Evaluated(at, Err(e), previous),
                        Ok(current) => {
                            let result = evaluate::evaluate(
                                execute,
                                &tenant,
                                services.retention,
                                services.max_groups,
                                current.as_ref(),
                                md,
                                at,
                            )
                            .await;
                            Outcome::Evaluated(at, result, current)
                        }
                    }
                }
            };
            services.counts.evaluation_nanos.add(started.elapsed().as_nanos() as i64);
            drop(slot);
            outcome
        });
    }

    fn on_tick(&mut self, at: i64) {
        self.last_at = Some(at);
        self.arm();

        if self.closing {
            if !self.busy {
                self.insert_pending();
            }
            return;
        }
        let counts = self.services.counts.clone();
        match &self.definition {
            Err(problems) => self.failed(at, &format!("the definition is not valid: {}", problems.join("; "))),
            Ok(_) if self.busy => self.skipped(
                &counts.skipped_busy,
                at,
                "busy: an evaluation, its insert or a definition read is still running",
            ),
            Ok(_) if self.pending.is_some() => {
                self.skipped(&counts.skipped_pending, at, "transitions of a failed insert are stored first");
                self.insert_pending();
            }
            Ok(d) => {
                let d = d.clone();
                self.evaluate(d, at);
            }
        }
    }

    fn receive(&mut self, command: Command) {
        match command {
            // Before the stash, so a hand-off never waits for a load. Stopping drops what is
            // running; the next owner evaluates the next tick.
            Command::Stop if self.pending.is_none() => self.stopped = true,
            command if !self.loaded => self.stash.push_back(command),
            Command::Tick(at) => self.on_tick(at),
            Command::Ensure(version) => {
                self.heard();
                match &self.row {
                    Some(r) if version <= r.version => {}
                    _ => {
                        self.reload_due = true;
                        self.settle();
                    }
                }
            }
            Command::Delete(_) => {
                self.reload_due = true;
                self.settle();
            }
            Command::Stop => {
                self.stopping = true;
                self.settle();
            }
        }
    }

    fn on_timer(&mut self, timer: Timer) {
        match timer {
            Timer::Tick(at) => self.receive(Command::Tick(at)),
            Timer::Unheard => {
                self.reload_due = true;
                self.settle();
            }
            Timer::RetryLoad => self.load(),
            Timer::RetryReload => self.settle(),
        }
    }

    fn on_outcome(&mut self, outcome: Outcome) {
        let retry = self.services.retry;
        match outcome {
            Outcome::Loaded(found, history) => {
                self.loaded = true;
                self.groups = history.groups;
                self.query_hash = history.query_hash;
                self.read(found);
                self.arm();
                self.heard();
                self.unstashed.extend(self.stash.drain(..));
                self.settle();
            }
            Outcome::LoadFailed(error) => {
                eprintln!("warning: Monitor {} could not be loaded, retrying in {retry:?}: {error}", self.key.text());
                self.timers.load = Some(Instant::now() + retry);
            }
            Outcome::Reloaded(found) => {
                self.busy = false;
                self.reload_failing = false;
                self.heard();
                self.read(found);
                self.settle();
            }
            Outcome::ReloadFailed(error) => {
                self.busy = false;
                self.reload_due = true;
                // With Postgres down every entity fails here; one warning each is enough.
                if !self.reload_failing {
                    eprintln!(
                        "warning: Monitor {}: its definition could not be read, retrying every {retry:?}: {error}",
                        self.key.text()
                    );
                }
                self.reload_failing = true;
                self.timers.reload = Some(Instant::now() + retry);
            }
            Outcome::Evaluated(at, Ok(evaluation), fitted) => {
                self.busy = false;
                self.models = fitted;
                self.count(&evaluation);

                // A definition is replaced only while nothing runs, so the one evaluated is still
                // the one held.
                if let Ok(d) = &self.definition {
                    let (next, transitions) = state::step(d, &self.groups, &evaluation, at);
                    let rows = self.rows_of(None, self.query_hash.as_deref(), &transitions);
                    let evaluations = store::evaluation_rows(&self.key, at, &next, &evaluation);
                    self.commit(rows, next, self.query_hash.clone(), evaluations);
                }
                self.settle();
            }
            Outcome::Evaluated(at, Err(error), fitted) => {
                self.busy = false;
                self.models = fitted;
                self.failed(at, &error);
                self.settle();
            }
            Outcome::GateRefused(at) => {
                self.busy = false;
                let counts = self.services.counts.clone();
                self.skipped(&counts.skipped_gate, at, "gate: no evaluation slot on this replica within one interval");
                self.settle();
            }
            Outcome::Committed => {
                self.busy = false;
                if let Some(p) = self.pending.take() {
                    let counts = &self.services.counts;
                    counts.transitions_written.add(p.rows.len() as i64);
                    counts.transitions_pending.add(-(p.rows.len() as i64));
                    self.groups = p.groups;
                    self.query_hash = p.query_hash;
                    self.record(p.evaluations);
                }
                self.settle();
            }
            Outcome::CommitFailed(error) => {
                self.busy = false;
                if let Some(p) = &self.pending {
                    self.services.counts.transitions_failed.add(p.rows.len() as i64);
                }
                eprintln!(
                    "warning: Monitor {}: transitions not stored, retried at the next tick: {error}",
                    self.key.text()
                );
                self.settle();
            }
        }
    }

    /// Groups found but none judged is a skip, as the evaluation's rows say; no group found at all
    /// is an evaluation (No Data).
    fn count(&self, evaluation: &Evaluation) {
        let counts = &self.services.counts;
        let causes: Vec<SkipCause> = evaluation
            .groups
            .iter()
            .filter_map(|r| match &r.reading {
                Reading::Skipped(cause, _) => Some(*cause),
                Reading::Judged(_) | Reading::Streaks(_) => None,
            })
            .collect();
        if !evaluation.groups.is_empty() && causes.len() == evaluation.groups.len() {
            if causes.contains(&SkipCause::Learning) {
                counts.skipped_learning.add(1);
            } else if causes.contains(&SkipCause::FewGroups) {
                counts.skipped_few_groups.add(1);
            } else {
                counts.skipped_window.add(1);
            }
        } else {
            counts.evaluated.add(1);
        }
        let lag = self.services.backend.now() - evaluation.period.to;
        counts.evaluation_lag.observe(lag as f64 / 1000.0);
    }
}

/// However the entity ends, stopped, orphaned or aborted: what it was running is dropped with it.
impl<B: Backend> Drop for Entity<B> {
    fn drop(&mut self) {
        let counts = &self.services.counts;
        counts.entities.add(-1);
        if let Some(p) = &self.pending {
            counts.transitions_pending.add(-(p.rows.len() as i64));
        }
    }
}
