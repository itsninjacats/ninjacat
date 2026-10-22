//! What a monitor stored in ClickHouse: the groups it starts from, and the rows it writes.

use std::collections::BTreeMap;

use api::reads::ClickHouse;
use ninjapaws_engine::monitor::check;
use ninjapaws_engine::monitor::definition::Monitor;
use ninjapaws_engine::monitor::evaluate::{Evaluation, Reading};
use ninjapaws_engine::monitor::state::{CLOSES, Group, Reason, Status, Transition, overall};
use ninjapaws_engine::{Sql, SqlValue};

use crate::definitions::MonitorKey;
use crate::rows::{EvaluationRow, TransitionRow};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct History {
    pub groups: BTreeMap<String, Group>,
    pub query_hash: Option<String>,
}

fn state_text(s: Option<Status>) -> String {
    s.map_or("none", |s| s.text()).to_string()
}

/// The thresholds are kept on the row because Postgres has only the latest definition.
pub fn transition_row(
    key: &MonitorKey,
    version: i32,
    query_hash: &str,
    definition: Option<&Monitor>,
    actor: Option<&str>,
    t: &Transition,
) -> TransitionRow {
    let thresholds = definition.map(|d| d.options().thresholds).unwrap_or_default();
    TransitionRow {
        tenant_id: key.tenant.clone(),
        monitor_id: key.id as u64,
        monitor_version: version as u32,
        query_hash: query_hash.to_string(),
        monitor_kind: match definition {
            Some(Monitor::ServiceCheck(_)) => "service_check",
            _ => "metric",
        }
        .to_string(),
        group_key: t.key.clone(),
        group_tags: t.tags.clone(),
        at: t.at,
        from_state: state_text(t.from),
        to_state: t.to.text().to_string(),
        reason: t.reason.text().to_string(),
        actor: actor.map(str::to_string),
        value: t.value,
        crossed: t.crossed.map_or("none", |c| c.text()).to_string(),
        comparator: match definition {
            Some(Monitor::Metric(d)) => d.query.comparator.text(),
            _ => "",
        }
        .to_string(),
        critical: thresholds.critical,
        warning: thresholds.warning,
        critical_recovery: thresholds.critical_recovery,
        warning_recovery: thresholds.warning_recovery,
        // A transition outside a tick judged no window; it is stored as the epoch.
        window_from: t.period.map_or(0, |p| p.from),
        window_to: t.period.map_or(0, |p| p.to),
        first_triggered_at: t.first_triggered_at,
        last_triggered_at: t.last_triggered_at,
    }
}

pub fn evaluation_rows(
    key: &MonitorKey,
    at: i64,
    groups: &BTreeMap<String, Group>,
    e: &Evaluation,
) -> Vec<EvaluationRow> {
    e.groups
        .iter()
        .map(|r| {
            let (value, outcome, detail) = match &r.reading {
                Reading::Judged(v) => (Some(*v), "evaluated", ""),
                Reading::Skipped(_, reason) => (None, "skipped", reason.as_str()),
                Reading::Streaks(s) => (Some(check::worst(s)), "evaluated", ""),
            };
            EvaluationRow {
                tenant_id: key.tenant.clone(),
                monitor_id: key.id as u64,
                group_key: r.key.clone(),
                at,
                value,
                state: state_text(groups.get(&r.key).and_then(|g| g.state)),
                outcome: outcome.to_string(),
                detail: detail.to_string(),
            }
        })
        .collect()
}

/// An evaluation skipped or failed as a whole; its group key is empty, which no group has.
pub fn monitor_row(
    key: &MonitorKey,
    at: i64,
    groups: &BTreeMap<String, Group>,
    outcome: &str,
    detail: &str,
) -> EvaluationRow {
    EvaluationRow {
        tenant_id: key.tenant.clone(),
        monitor_id: key.id as u64,
        group_key: String::new(),
        at,
        value: None,
        state: state_text(overall(groups)),
        outcome: outcome.to_string(),
        detail: detail.to_string(),
    }
}

fn key_parameters(key: &MonitorKey) -> Vec<(String, SqlValue)> {
    vec![("tenant".into(), SqlValue::String(key.tenant.clone())), ("monitor".into(), SqlValue::Int64(key.id))]
}

const DAY_MS: i64 = 24 * 3600 * 1000;

/// Only the last two days: a group unseen longer is dropped at the next evaluation anyway.
async fn last_seen(reader: &ClickHouse, key: &MonitorKey, now: i64) -> Result<BTreeMap<String, i64>, String> {
    let mut parameters = key_parameters(key);
    parameters.push(("since".into(), SqlValue::Int64(now - 2 * DAY_MS)));
    let sql = Sql {
        text: "SELECT group_key, toUnixTimestamp64Milli(max(at)) FROM monitor_evaluations \
               WHERE tenant_id = {tenant:String} AND monitor_id = {monitor:Int64} AND group_key != '' \
               AND at >= fromUnixTimestamp64Milli({since:Int64}) GROUP BY group_key"
            .into(),
        parameters,
    };
    let rows: Vec<(String, i64)> = reader.read(&sql).await?;
    Ok(rows.into_iter().collect())
}

type Latest = (String, Vec<String>, String, String, i64, i64, Option<i64>, Option<i64>, String, String);

pub async fn history(reader: &ClickHouse, key: &MonitorKey, now: i64) -> Result<History, String> {
    let seen = last_seen(reader, key, now).await?;

    // A repeated row (a retried insert, two owners during a hand-off) has the same `at` and the
    // same values. argMax skips a NULL argument and would return an older trigger time; inside a
    // tuple the NULL is kept.
    let sql = Sql {
        text: "SELECT group_key, argMax(group_tags, at), argMax(to_state, at), argMax(reason, at), \
               toUnixTimestamp64Milli(max(at)), toUnixTimestamp64Milli(min(at)), \
               toUnixTimestamp64Milli(tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 1)), \
               toUnixTimestamp64Milli(tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 2)), \
               argMax(query_hash, at), argMax(from_state, at) \
               FROM monitor_transitions WHERE tenant_id = {tenant:String} AND monitor_id = {monitor:Int64} \
               GROUP BY group_key"
            .into(),
        parameters: key_parameters(key),
    };
    let latest: Vec<Latest> = reader.read(&sql).await?;

    let mut history = History::default();
    let mut newest: Option<i64> = None;
    for (group_key, tags, to_state, reason, at, first_seen, first_triggered, last_triggered, hash, from_state) in latest
    {
        let state = Status::of_text(&to_state);
        if state.is_none() || CLOSES.iter().any(|c| c.text() == reason) {
            continue;
        }
        let last_seen = seen.get(&group_key).copied().unwrap_or(at).max(at);
        let group = Group {
            key: group_key.clone(),
            tags,
            state,
            since: at,
            first_triggered_at: if state == Some(Status::Ok) { None } else { first_triggered },
            last_triggered_at: last_triggered,
            first_seen,
            last_seen,
            // Points are not stored with the evaluation; the last tick that saw the group is the
            // nearest bound. It errs late, so No Data comes at most a window and the evaluation
            // delay later.
            last_point: last_seen,
            interrupted: if state == Some(Status::NoData) && reason == Reason::NoDataShown.text() {
                Status::of_text(&from_state)
            } else {
                None
            },
        };
        if newest.is_none_or(|since| at > since) {
            newest = Some(at);
            history.query_hash = Some(hash);
        }
        history.groups.insert(group_key, group);
    }
    Ok(history)
}
