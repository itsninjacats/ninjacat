//! The notifier, a stub: logs the transitions it would notify and records a `stub` row for each.
//! It runs on the one replica that holds the notifier's lock.

use std::sync::Arc;
use std::time::Duration;

use api::reads::ClickHouse;
use ninjapaws_engine::monitor::state::NEVER_NOTIFIED;
use ninjapaws_engine::{Sql, SqlValue};
use tokio::time::MissedTickBehavior;

use crate::rows::NotificationRow;
use crate::telemetry::Counters;

#[derive(Clone, Debug, PartialEq)]
pub struct Due {
    pub tenant: String,
    pub monitor_id: u64,
    pub group_key: String,
    pub at: i64,
    pub inserted_at: i64,
    pub from_state: String,
    pub to_state: String,
    pub reason: String,
    pub value: Option<f64>,
}

/// What one poll read.
#[derive(Clone, Debug, PartialEq)]
pub struct Polled {
    /// Oldest first, those already notified left out.
    pub found: Vec<Due>,
    /// Of any transition read, notified or not, so the cursor also moves through transitions that
    /// are never notified.
    pub newest: Option<i64>,
}

/// A full batch is followed by another poll at once.
pub const BATCH: usize = 1000;

pub trait Backend: Send + Sync + 'static {
    /// The insert time of the newest transition notified; None before the first.
    fn cursor(&self) -> impl Future<Output = Result<Option<i64>, String>> + Send;
    fn due(&self, since: i64) -> impl Future<Output = Result<Polled, String>> + Send;
    fn write(&self, rows: Vec<NotificationRow>) -> impl Future<Output = Result<(), String>> + Send;
    fn now(&self) -> i64;
}

pub async fn cursor(reader: &ClickHouse) -> Result<Option<i64>, String> {
    let sql = Sql {
        text: "SELECT toUnixTimestamp64Milli(maxOrNull(transition_inserted_at)) FROM monitor_notifications".into(),
        parameters: Vec::new(),
    };
    let rows: Vec<(Option<i64>,)> = reader.read(&sql).await?;
    Ok(rows.first().and_then(|r| r.0))
}

const DAY_MS: i64 = 24 * 3600 * 1000;

/// From 30 s before `since`, since rows of two inserts become visible in either order. A
/// transition is taken to be stored within a day of its `at`, so older partitions are skipped.
/// The newest is read first, so every row it covers is visible to the read of the due ones.
pub async fn due(reader: &ClickHouse, since: i64) -> Result<Polled, String> {
    let from = since - 30_000;
    let bounds =
        vec![("from".to_string(), SqlValue::Int64(from)), ("earliest".to_string(), SqlValue::Int64(from - DAY_MS))];
    let window =
        "inserted_at > fromUnixTimestamp64Milli({from:Int64}) AND at >= fromUnixTimestamp64Milli({earliest:Int64}) ";

    let newest = Sql {
        text: format!("SELECT toUnixTimestamp64Milli(maxOrNull(inserted_at)) FROM monitor_transitions WHERE {window}"),
        parameters: bounds.clone(),
    };
    let newest: Vec<(Option<i64>,)> = reader.read(&newest).await?;

    let never = NEVER_NOTIFIED.iter().map(|r| r.text().to_string()).collect();
    let mut parameters = vec![("never".to_string(), SqlValue::StringArray(never))];
    parameters.extend(bounds);
    let found = Sql {
        text: format!(
            "SELECT tenant_id, monitor_id, group_key, toUnixTimestamp64Milli(at), toUnixTimestamp64Milli(inserted_at), \
             from_state, to_state, reason, value \
             FROM monitor_transitions FINAL WHERE {window}\
             AND NOT has({{never:Array(String)}}, reason) \
             AND NOT (reason = 'new' AND to_state = 'OK') \
             AND (tenant_id, monitor_id, group_key, at) NOT IN (\
             SELECT tenant_id, monitor_id, group_key, transition_at FROM monitor_notifications \
             WHERE transition_at >= fromUnixTimestamp64Milli({{earliest:Int64}}) AND kind = 'transition') \
             ORDER BY inserted_at, tenant_id, monitor_id, group_key \
             LIMIT {BATCH}"
        ),
        parameters,
    };
    type Found = (String, u64, String, i64, i64, String, String, String, Option<f64>);
    let found: Vec<Found> = reader.read(&found).await?;
    Ok(Polled {
        found: found
            .into_iter()
            .map(|(tenant, monitor_id, group_key, at, inserted_at, from_state, to_state, reason, value)| Due {
                tenant,
                monitor_id,
                group_key,
                at,
                inserted_at,
                from_state,
                to_state,
                reason,
                value,
            })
            .collect(),
        newest: newest.first().and_then(|r| r.0),
    })
}

fn stub(now: i64, d: &Due) -> NotificationRow {
    NotificationRow {
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
        handled_at: now,
    }
}

pub struct Notifier<B> {
    pub backend: Arc<B>,
    pub interval: Duration,
    pub counts: Arc<Counters>,
}

impl<B: Backend> Notifier<B> {
    /// Polls every interval until the task is aborted; a poll still running when the next is due
    /// makes that one skipped.
    pub async fn run(self) {
        let mut cursor: Option<i64> = None;
        let mut failing = false;
        let mut every = tokio::time::interval(self.interval);
        every.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            every.tick().await;
            loop {
                let polled = match cursor {
                    None => self.start().await.map(|newest| {
                        cursor = Some(newest);
                        false
                    }),
                    Some(since) => self.notify(since).await.map(|polled| self.handled(&mut cursor, polled)),
                };
                match polled {
                    Ok(again) => {
                        failing = false;
                        if !again {
                            break;
                        }
                    }
                    Err(error) => {
                        self.counts.notifier_failed.add(1);
                        // A ClickHouse outage fails every poll; one warning is enough.
                        if !failing {
                            eprintln!(
                                "warning: notifier: transitions not read or not marked, retrying every {:?}: {error}",
                                self.interval
                            );
                        }
                        failing = true;
                        break;
                    }
                }
            }
        }
    }

    /// An older transition is history, not news.
    async fn start(&self) -> Result<i64, String> {
        let newest = self.backend.cursor().await?;
        Ok(newest.unwrap_or_else(|| self.backend.now() - DAY_MS))
    }

    async fn notify(&self, since: i64) -> Result<Polled, String> {
        let polled = self.backend.due(since).await?;
        if !polled.found.is_empty() {
            let now = self.backend.now();
            for d in &polled.found {
                self.counts.notifier_lag.observe((now - d.inserted_at) as f64 / 1000.0);
            }
            self.backend.write(polled.found.iter().map(|d| stub(now, d)).collect()).await?;
        }
        Ok(polled)
    }

    /// Moves the cursor past what the poll handled; true when it should poll again at once.
    fn handled(&self, cursor: &mut Option<i64>, polled: Polled) -> bool {
        for d in &polled.found {
            let value = d.value.map_or("null".to_string(), |v| v.to_string());
            println!(
                "Monitor {}/{} group {}: {} -> {} ({}, value {value}) at {}",
                d.tenant,
                d.monitor_id,
                d.group_key,
                d.from_state,
                d.to_state,
                d.reason,
                ninjapaws_engine::json::utc(d.at)
            );
        }
        let full = polled.found.len() >= BATCH;
        let mut passed: Vec<i64> = polled.found.iter().map(|d| d.inserted_at).collect();
        // A full batch may have left due transitions older than the newest read.
        if !full {
            passed.extend(polled.newest);
        }
        let current = cursor.unwrap_or(i64::MIN);
        *cursor = Some(passed.into_iter().fold(current, i64::max));
        full
    }
}
