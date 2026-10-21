//! The notifier stub: its loop over fakes, and what it reads from and writes to ClickHouse, in a
//! scratch database.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Scratch, WAIT, eventually};
use crate::notifier::{self, BATCH, Backend, Due, Notifier, Polled};
use crate::rows::{NotificationRow, TransitionRow};
use crate::telemetry::Counters;
use crate::writer;

/// 2026-10-14T12:00:00Z.
const T0: i64 = 1_791_979_200_000;
const MINUTE: i64 = 60_000;

fn due(group: &str, inserted_at: i64) -> Due {
    Due {
        tenant: "acme".into(),
        monitor_id: 7,
        group_key: group.into(),
        at: inserted_at - 3000,
        inserted_at,
        from_state: "OK".into(),
        to_state: "Alert".into(),
        reason: "threshold".into(),
        value: Some(95.0),
    }
}

/// A poll that read only these transitions.
fn polled(found: Vec<Due>) -> Polled {
    let newest = found.iter().map(|d| d.inserted_at).max();
    Polled { found, newest }
}

/// What the services answer; a test sets it before the notifier starts.
struct Fakes {
    cursor: Option<i64>,
    /// Answered in turn, then nothing.
    batches: Mutex<VecDeque<Polled>>,
    failing_writes: Mutex<u32>,
    /// Each poll's `since`.
    polls: Mutex<Vec<i64>>,
    written: Mutex<Vec<Vec<NotificationRow>>>,
}

impl Fakes {
    fn new() -> Fakes {
        Fakes {
            cursor: Some(T0),
            batches: Mutex::new(VecDeque::new()),
            failing_writes: Mutex::new(0),
            polls: Mutex::new(Vec::new()),
            written: Mutex::new(Vec::new()),
        }
    }

    fn polls(&self) -> Vec<i64> {
        self.polls.lock().unwrap().clone()
    }

    fn written(&self) -> Vec<Vec<NotificationRow>> {
        self.written.lock().unwrap().clone()
    }
}

impl Backend for Fakes {
    async fn cursor(&self) -> Result<Option<i64>, String> {
        Ok(self.cursor)
    }

    async fn due(&self, since: i64) -> Result<Polled, String> {
        self.polls.lock().unwrap().push(since);
        Ok(self.batches.lock().unwrap().pop_front().unwrap_or_else(|| polled(Vec::new())))
    }

    async fn write(&self, rows: Vec<NotificationRow>) -> Result<(), String> {
        let mut failing = self.failing_writes.lock().unwrap();
        if *failing > 0 {
            *failing -= 1;
            return Err("the insert timed out".into());
        }
        self.written.lock().unwrap().push(rows);
        Ok(())
    }

    fn now(&self) -> i64 {
        T0 + 5 * MINUTE
    }
}

fn start(fakes: Fakes) -> (Arc<Fakes>, Arc<Counters>, tokio::task::JoinHandle<()>) {
    let fakes = Arc::new(fakes);
    let counts = Arc::new(Counters::default());
    let notifier = Notifier { backend: fakes.clone(), interval: Duration::from_millis(100), counts: counts.clone() };
    (fakes, counts, tokio::spawn(notifier.run()))
}

#[tokio::test]
async fn each_due_transition_gets_a_stub_row_and_the_cursor_moves_past_it() {
    let fakes = Fakes::new();
    fakes.batches.lock().unwrap().push_back(polled(vec![due("host:a", T0 + MINUTE), due("host:b", T0 + 2 * MINUTE)]));
    let (fakes, counts, task) = start(fakes);

    eventually(WAIT, || fakes.written().len() == 1).await;
    let rows = fakes.written()[0].clone();
    assert_eq!(rows.iter().map(|r| r.group_key.as_str()).collect::<Vec<_>>(), vec!["host:a", "host:b"]);

    let a = &rows[0];
    assert_eq!(
        (a.tenant_id.as_str(), a.monitor_id, a.kind.as_str(), a.seq, a.channel.as_str(), a.status.as_str(), a.attempt),
        ("acme", 7, "transition", 0, "stub", "stub", 1)
    );
    assert_eq!(a.transition_at, T0 + 57_000);
    assert_eq!(a.transition_inserted_at, T0 + MINUTE);
    assert_eq!(a.handled_at, T0 + 5 * MINUTE);

    // The next poll starts past the newest transition handled.
    eventually(WAIT, || fakes.polls().contains(&(T0 + 2 * MINUTE))).await;
    assert_eq!(fakes.polls()[0], T0);
    // The oldest waited longest: four minutes.
    assert_eq!(counts.notifier_lag.take(), 240.0);
    task.abort();
}

#[tokio::test]
async fn a_write_that_fails_leaves_the_cursor_where_it_was_and_is_counted() {
    let fakes = Fakes::new();
    *fakes.failing_writes.lock().unwrap() = 1;
    fakes.batches.lock().unwrap().push_back(polled(vec![due("host:a", T0 + MINUTE)]));
    // Lag 120 s, so the 240 s below can only come from the failed poll.
    fakes.batches.lock().unwrap().push_back(polled(vec![due("host:a", T0 + 3 * MINUTE)]));
    let (fakes, counts, task) = start(fakes);

    eventually(WAIT, || fakes.written().len() == 1).await;
    assert_eq!(fakes.polls()[..2], [T0, T0]);
    assert_eq!(counts.notifier_failed.value(), 1.0);
    // Reported even by a poll whose write failed.
    assert_eq!(counts.notifier_lag.take(), 240.0);
    task.abort();
}

#[tokio::test]
async fn transitions_never_notified_move_the_cursor_too() {
    let fakes = Fakes::new();
    fakes.batches.lock().unwrap().push_back(Polled { found: Vec::new(), newest: Some(T0 + 3 * MINUTE) });
    let (fakes, _, task) = start(fakes);

    eventually(WAIT, || fakes.polls().contains(&(T0 + 3 * MINUTE))).await;
    assert!(fakes.written().is_empty());
    task.abort();
}

#[tokio::test]
async fn a_full_batch_moves_the_cursor_only_past_what_it_notified_and_polls_again_at_once() {
    let fakes = Fakes::new();
    let found = (1..=BATCH).map(|i| due(&format!("host:{i}"), T0 + MINUTE)).collect();
    fakes.batches.lock().unwrap().push_back(Polled { found, newest: Some(T0 + 3 * MINUTE) });
    let (fakes, _, task) = start(fakes);

    eventually(WAIT, || fakes.polls().len() >= 2).await;
    assert_eq!(fakes.polls()[..2], [T0, T0 + MINUTE]);
    task.abort();
}

#[tokio::test]
async fn a_notifier_that_never_stored_one_starts_a_day_back() {
    let mut fakes = Fakes::new();
    fakes.cursor = None;
    let (fakes, _, task) = start(fakes);

    eventually(WAIT, || !fakes.polls().is_empty()).await;
    assert_eq!(fakes.polls()[0], T0 + 5 * MINUTE - 86_400_000);
    task.abort();
}

fn transition(monitor: u64, group: &str, at: i64, to: &str, reason: &str) -> TransitionRow {
    TransitionRow {
        tenant_id: "acme".into(),
        monitor_id: monitor,
        monitor_version: 1,
        query_hash: "h".into(),
        monitor_kind: "metric".into(),
        group_key: group.into(),
        group_tags: Vec::new(),
        at,
        from_state: "none".into(),
        to_state: to.into(),
        reason: reason.into(),
        actor: None,
        value: Some(1.0),
        crossed: "none".into(),
        comparator: ">".into(),
        critical: Some(90.0),
        warning: None,
        critical_recovery: None,
        warning_recovery: None,
        window_from: at - 5 * MINUTE,
        window_to: at,
        first_triggered_at: None,
        last_triggered_at: None,
    }
}

#[tokio::test]
async fn due_leaves_out_what_is_never_notified_and_what_was_already_notified() {
    let Some(scratch) = Scratch::new().await else { return };
    let (reader, writes) = (scratch.reader(), scratch.writes());
    let started = chrono::Utc::now().timestamp_millis();
    let at = started - 10_000;

    let rows = [
        transition(1, "host:a", at, "Alert", "new"),
        transition(1, "host:b", at, "OK", "new"),
        transition(1, "host:c", at, "Warn", "threshold"),
        transition(1, "host:d", at, "OK", "expired"),
        transition(1, "host:e", at, "OK", "definition_changed"),
        transition(1, "host:f", at, "OK", "deleted"),
        transition(1, "host:g", at, "No Data", "no_data_shown"),
        transition(1, "host:h", at, "OK", "no_data"),
        transition(2, "*", at, "OK", "recovery"),
    ];
    writer::insert(&writes, "monitor_transitions", &rows).await.unwrap();
    // Retried, as after a timeout: the same key collapses.
    writer::insert(&writes, "monitor_transitions", &rows[..1]).await.unwrap();

    // A list, not a set: a duplicate FINAL failed to collapse must show.
    let keys = |p: &Polled| {
        let mut keys: Vec<(u64, String)> = p.found.iter().map(|d| (d.monitor_id, d.group_key.clone())).collect();
        keys.sort();
        keys
    };
    let expected = |groups: &[(u64, &str)]| groups.iter().map(|(m, g)| (*m, g.to_string())).collect::<Vec<_>>();

    let first = notifier::due(&reader, started - MINUTE).await.unwrap();
    assert_eq!(keys(&first), expected(&[(1, "host:a"), (1, "host:c"), (1, "host:h"), (2, "*")]));
    // The newest covers the transitions never notified as well.
    let newest_found = first.found.iter().map(|d| d.inserted_at).max().unwrap();
    assert!(first.newest.is_some_and(|newest| newest >= newest_found));
    assert_eq!(notifier::cursor(&reader).await.unwrap(), None);

    let notified: Vec<NotificationRow> = first
        .found
        .iter()
        .filter(|d| d.group_key == "host:a")
        .map(|d| NotificationRow {
            tenant_id: d.tenant.clone(),
            monitor_id: d.monitor_id,
            group_key: d.group_key.clone(),
            transition_at: d.at,
            transition_inserted_at: d.inserted_at,
            kind: "transition".into(),
            seq: 0,
            channel: "stub".into(),
            status: "stub".into(),
            attempt: 1,
            handled_at: chrono::Utc::now().timestamp_millis(),
        })
        .collect();
    writer::insert(&writes, "monitor_notifications", &notified).await.unwrap();

    let rest = expected(&[(1, "host:c"), (1, "host:h"), (2, "*")]);
    assert_eq!(keys(&notifier::due(&reader, started - MINUTE).await.unwrap()), rest);
    assert_eq!(notifier::cursor(&reader).await.unwrap(), Some(notified[0].transition_inserted_at));

    // Inserted less than 30 s before `since`: read again, in case an older insert became visible
    // late.
    let now = chrono::Utc::now().timestamp_millis();
    assert_eq!(keys(&notifier::due(&reader, now + 10_000).await.unwrap()), rest);

    // More than 30 s before: not read again.
    let later = notifier::due(&reader, now + MINUTE).await.unwrap();
    assert!(later.found.is_empty());
    assert_eq!(later.newest, None);
}
