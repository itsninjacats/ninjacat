//! The groups an entity starts from, read back from transitions and evaluations stored through the
//! real writes, in a scratch database (F#: `StoreTests.fs`).

use ninjapaws_engine::monitor::evaluate::Period;
use ninjapaws_engine::monitor::state::{Reason, Status, Transition};

use super::Scratch;
use crate::definitions::MonitorKey;
use crate::rows::EvaluationRow;
use crate::store::{self, History};
use crate::writer;

fn key() -> MonitorKey {
    MonitorKey { tenant: "acme".into(), id: 7 }
}

/// Today at 10:00, within the tables' TTLs.
fn now() -> i64 {
    let today = chrono::Utc::now().timestamp_millis() / 86_400_000 * 86_400_000;
    today + 10 * 3_600_000
}

fn minutes_ago(n: i64) -> i64 {
    now() - n * 60_000
}

fn transition(group: &str, at: i64, from: Option<Status>, to: Status, reason: Reason) -> Transition {
    let triggered = matches!(to, Status::Alert | Status::Warn).then_some(at);
    Transition {
        key: group.into(),
        tags: vec![group.into()],
        at,
        from,
        to,
        reason,
        value: Some(1.0),
        crossed: None,
        period: Some(Period { from: at - 5 * 60_000, to: at }),
        first_triggered_at: triggered,
        last_triggered_at: triggered,
    }
}

async fn stored(writes: &clickhouse::Client, hash: &str, t: Transition) {
    let row = store::transition_row(&key(), 1, hash, None, None, &t);
    writer::insert(writes, "monitor_transitions", &[row]).await.unwrap();
}

fn evaluated(group: &str, at: i64) -> EvaluationRow {
    EvaluationRow {
        tenant_id: "acme".into(),
        monitor_id: 7,
        group_key: group.into(),
        at,
        value: Some(1.0),
        state: "Alert".into(),
        outcome: "evaluated".into(),
        detail: String::new(),
    }
}

#[tokio::test]
async fn open_groups_load_with_their_latest_transition_closed_ones_do_not() {
    let Some(scratch) = Scratch::new().await else { return };
    let writes = scratch.writes();
    use Status::*;

    stored(&writes, "old", transition("host:a", minutes_ago(30), None, Alert, Reason::New)).await;
    stored(&writes, "new", transition("host:a", minutes_ago(20), Some(Alert), Warn, Reason::Threshold)).await;
    // Stored twice, as a retried insert would.
    stored(&writes, "old", transition("host:b", minutes_ago(25), None, Ok, Reason::New)).await;
    stored(&writes, "old", transition("host:b", minutes_ago(25), None, Ok, Reason::New)).await;
    stored(&writes, "old", transition("host:c", minutes_ago(30), None, Alert, Reason::New)).await;
    stored(&writes, "old", transition("host:c", minutes_ago(10), Some(Alert), Ok, Reason::Expired)).await;
    // The latest row has no trigger times; an older one has.
    stored(&writes, "new", transition("host:d", minutes_ago(30), None, Alert, Reason::New)).await;
    stored(&writes, "new", transition("host:d", minutes_ago(15), Some(Alert), NoData, Reason::NoData)).await;
    // A No Data not notified remembers what it interrupted.
    stored(&writes, "new", transition("host:e", minutes_ago(30), None, Warn, Reason::New)).await;
    stored(&writes, "new", transition("host:e", minutes_ago(15), Some(Warn), NoData, Reason::NoDataShown)).await;

    // Another monitor's group is not this one's.
    let mut other =
        store::transition_row(&key(), 1, "x", None, None, &transition("host:z", now(), None, Alert, Reason::New));
    other.monitor_id = 8;
    writer::insert(&writes, "monitor_transitions", &[other]).await.unwrap();

    let mut monitor_level = evaluated("", now());
    monitor_level.outcome = "error".into();
    // A monitor-level row is no group's sighting.
    let evaluations = [evaluated("host:a", minutes_ago(2)), evaluated("host:a", minutes_ago(1)), monitor_level];
    writer::insert(&writes, "monitor_evaluations", &evaluations).await.unwrap();

    let history = store::history(&scratch.reader(), &key(), now()).await.unwrap();

    assert_eq!(history.groups.keys().collect::<Vec<_>>(), vec!["host:a", "host:b", "host:d", "host:e"]);
    assert_eq!(history.query_hash.as_deref(), Some("new"));

    let a = &history.groups["host:a"];
    assert_eq!((a.state, a.since, a.tags.clone()), (Some(Warn), minutes_ago(20), vec!["host:a".to_string()]));
    assert_eq!((a.first_triggered_at, a.last_triggered_at), (Some(minutes_ago(20)), Some(minutes_ago(20))));
    assert_eq!((a.first_seen, a.last_seen), (minutes_ago(30), minutes_ago(1)));

    let b = &history.groups["host:b"];
    assert_eq!((b.state, b.since, b.last_seen), (Some(Ok), minutes_ago(25), minutes_ago(25)));
    assert_eq!(b.first_triggered_at, None);

    let d = &history.groups["host:d"];
    assert_eq!((d.state, d.first_triggered_at, d.last_triggered_at, d.interrupted), (Some(NoData), None, None, None));

    let e = &history.groups["host:e"];
    assert_eq!((e.state, e.interrupted), (Some(NoData), Some(Warn)));
    assert_eq!(a.interrupted, None);
}

#[tokio::test]
async fn a_monitor_with_nothing_stored_starts_empty() {
    let Some(scratch) = Scratch::new().await else { return };
    let history = store::history(&scratch.reader(), &key(), now()).await.unwrap();
    assert_eq!(history, History::default());
}
