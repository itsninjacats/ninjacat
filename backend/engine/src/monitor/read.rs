//! What the panel reads of monitors: each monitor's state, its groups, its transitions and its
//! evaluations, from what the monitor service stored. Every statement binds the tenant it is
//! given; the server takes that from the panel's token and runs the SQL. (F#: `Monitor/Read.fs`.)
//!
//! The records are the JSON the panel reads: times as `json::offset`, a None left out.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::monitor::state::{CLOSES, Status};
use crate::sql::{Sql, SqlParams, SqlValue, TenantId};

fn tenant_of(tenant: &TenantId) -> SqlValue {
    SqlValue::String(tenant.0.clone())
}

fn scope(bound: &mut SqlParams, tenant: &TenantId, monitor: i64) -> String {
    let tenant = bound.add(tenant_of(tenant));
    let monitor = bound.add(SqlValue::Int64(monitor));
    format!("tenant_id = {tenant} AND monitor_id = {monitor}")
}

fn closed() -> SqlValue {
    SqlValue::StringArray(CLOSES.iter().map(|r| r.text().to_string()).collect())
}

/// The most monitors one status request may name: 200 ids of 19 digits and their encoded commas
/// keep the request line under Kestrel's 8 KB.
pub const MAX_IDS: usize = 200;

/// The most rows a transitions or evaluations request returns.
pub const ROW_LIMIT: usize = 10_000;

/// `ids=1,2&ids=3`: monitor ids, comma-separated or repeated, in the order first given.
pub fn parse_ids(raw: &[String]) -> Result<Vec<i64>, String> {
    let parts: Vec<&str> = raw.iter().flat_map(|value| value.split(',')).map(str::trim).filter(|part| !part.is_empty()).collect();
    let mut ids: Vec<i64> = Vec::new();
    for part in parts {
        match part.parse::<i64>() {
            Ok(id) if id > 0 => {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
            _ => return Err(format!("bad monitor id \"{part}\"")),
        }
    }
    if ids.is_empty() {
        Err("ids is required".to_string())
    } else if ids.len() > MAX_IDS {
        Err(format!("at most {MAX_IDS} ids at once"))
    } else {
        Ok(ids)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct StatusRow {
    pub monitor: i64,
    pub state: String,
    pub open: bool,
    pub groups: i64,
    pub last_triggered_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MonitorStatus {
    pub id: i64,
    /// None when no group is open, which a multi alert shows as "no groups found".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    pub groups: BTreeMap<String, i64>,
    /// Closed groups included.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "crate::json::as_offset_option")]
    pub last_triggered_at: Option<i64>,
}

pub fn status(tenant: &TenantId, ids: &[i64]) -> Sql {
    let mut bound = SqlParams::new();
    let closed = bound.add(closed());
    let tenant = bound.add(tenant_of(tenant));
    let ids = bound.add(SqlValue::Int64Array(ids.to_vec()));

    // The aliases differ from the column names: ClickHouse would put an alias in place of a column
    // of the same name inside another aggregate.
    let latest = format!(
        "SELECT monitor_id, argMax(to_state, at) AS group_state, \
         argMax(reason, at) NOT IN {closed} AS is_open, max(last_triggered_at) AS group_triggered \
         FROM monitor_transitions WHERE tenant_id = {tenant} \
         AND monitor_id IN {ids} GROUP BY monitor_id, group_key"
    );

    Sql {
        text: format!(
            "SELECT monitor_id, group_state, is_open, count(), toUnixTimestamp64Milli(max(group_triggered)) \
             FROM ({latest}) GROUP BY monitor_id, group_state, is_open"
        ),
        parameters: bound.into_parameters(),
    }
}

/// One status per id asked for, in that order; a monitor with nothing stored has no state and no
/// groups.
pub fn statuses(ids: &[i64], rows: &[StatusRow]) -> Vec<MonitorStatus> {
    let severity = |r: &StatusRow| Status::of_text(&r.state).map_or(-1, |s| s.severity());
    ids.iter()
        .map(|&id| {
            let mine: Vec<&StatusRow> = rows.iter().filter(|r| r.monitor == id).collect();
            let opened: Vec<&StatusRow> = mine.iter().copied().filter(|r| r.open).collect();
            // The first of the most severe, as List.maxBy keeps it.
            let worst = opened.iter().copied().fold(None::<&StatusRow>, |worst, r| match worst {
                Some(w) if severity(w) >= severity(r) => Some(w),
                _ => Some(r),
            });
            let groups = Status::ALL
                .iter()
                .map(|s| {
                    let count = opened.iter().filter(|r| r.state == s.text()).map(|r| r.groups).sum();
                    (s.text().to_string(), count)
                })
                .collect();
            MonitorStatus {
                id,
                state: worst.map(|r| r.state.clone()),
                groups,
                last_triggered_at: mine.iter().filter_map(|r| r.last_triggered_at).max(),
            }
        })
        .collect()
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GroupStatus {
    pub group: String,
    pub tags: Vec<String>,
    pub state: String,
    #[serde(serialize_with = "crate::json::as_offset")]
    pub since: i64,
    pub reason: String,
    /// None when OK: the trigger it carries is the one the recovery ended.
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "crate::json::as_offset_option")]
    pub first_triggered_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "crate::json::as_offset_option")]
    pub last_triggered_at: Option<i64>,
    /// None when no evaluation judged it in the last day.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "crate::json::as_offset_option")]
    pub evaluated_at: Option<i64>,
}

/// The open groups of a monitor, with their latest evaluated value.
pub fn groups(tenant: &TenantId, monitor: i64) -> Sql {
    let mut bound = SqlParams::new();

    // argMax skips a NULL argument and would return an older trigger time; inside a tuple the NULL
    // is kept.
    let latest = format!(
        "SELECT group_key, argMax(group_tags, at) AS latest_tags, argMax(to_state, at) AS group_state, \
         argMax(reason, at) AS group_reason, max(at) AS since, \
         tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 1) AS first_triggered, \
         tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 2) AS last_triggered \
         FROM monitor_transitions WHERE {} GROUP BY group_key",
        scope(&mut bound, tenant, monitor)
    );

    // A group unseen for a day is closed as expired, so older rows never belong to an open group.
    let evaluated = format!(
        "SELECT group_key, argMax(value, at) AS last_value, max(at) AS evaluated_at \
         FROM monitor_evaluations WHERE {} AND outcome = 'evaluated' \
         AND at >= now64(3) - INTERVAL 1 DAY GROUP BY group_key",
        scope(&mut bound, tenant, monitor)
    );

    let closed = bound.add(closed());
    Sql {
        text: format!(
            "SELECT t.group_key, t.latest_tags, t.group_state, toUnixTimestamp64Milli(t.since), t.group_reason, \
             toUnixTimestamp64Milli(t.first_triggered), toUnixTimestamp64Milli(t.last_triggered), \
             e.last_value, toUnixTimestamp64Milli(e.evaluated_at) \
             FROM ({latest}) AS t LEFT JOIN ({evaluated}) AS e ON e.group_key = t.group_key \
             WHERE t.group_reason NOT IN {closed} ORDER BY t.group_key \
             SETTINGS join_use_nulls = 1"
        ),
        parameters: bound.into_parameters(),
    }
}

/// The SQL asks for one row more than the limit; the oldest is dropped, and true says rows were
/// left out.
pub fn newest<T>(mut rows: Vec<T>) -> (Vec<T>, bool) {
    if rows.len() > ROW_LIMIT {
        rows.remove(0);
        (rows, true)
    } else {
        (rows, false)
    }
}

/// The newest `ROW_LIMIT` + 1 rows in `[from, until)`, oldest first.
fn range(table: &str, columns: &str, tenant: &TenantId, monitor: i64, from: i64, until: i64, groups: Vec<String>) -> Sql {
    let mut bound = SqlParams::new();
    let mut condition = scope(&mut bound, tenant, monitor);
    let from = bound.add(SqlValue::Int64(from));
    let until = bound.add(SqlValue::Int64(until));
    condition.push_str(&format!(
        " AND at >= fromUnixTimestamp64Milli({from}) AND at < fromUnixTimestamp64Milli({until})"
    ));
    if !groups.is_empty() {
        condition.push_str(&format!(" AND group_key IN {}", bound.add(SqlValue::StringArray(groups))));
    }

    Sql {
        text: format!(
            "SELECT * FROM (SELECT {columns} FROM {table} WHERE {condition} ORDER BY at DESC, group_key LIMIT {}) \
             ORDER BY at_ms, group_key",
            ROW_LIMIT + 1
        ),
        parameters: bound.into_parameters(),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TransitionRecord {
    pub group: String,
    pub tags: Vec<String>,
    #[serde(serialize_with = "crate::json::as_offset")]
    pub at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_state: Option<String>,
    pub to_state: String,
    pub reason: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crossed: Option<String>,
    pub comparator: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critical: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub critical_recovery: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning_recovery: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "crate::json::as_offset_option")]
    pub window_from: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "crate::json::as_offset_option")]
    pub window_to: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "crate::json::as_offset_option")]
    pub first_triggered_at: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", serialize_with = "crate::json::as_offset_option")]
    pub last_triggered_at: Option<i64>,
    pub monitor_version: i64,
}

const TRANSITION_COLUMNS: &str = "group_key, group_tags, toUnixTimestamp64Milli(at) AS at_ms, from_state, to_state, reason, actor, value, crossed, \
     comparator, critical, warning, critical_recovery, warning_recovery, \
     toUnixTimestamp64Milli(window_from), toUnixTimestamp64Milli(window_to), \
     toUnixTimestamp64Milli(first_triggered_at), toUnixTimestamp64Milli(last_triggered_at), monitor_version";

fn one_or_all(group: &str) -> Vec<String> {
    if group.is_empty() { vec![] } else { vec![group.to_string()] }
}

/// A monitor's transitions in `[from, until)` (unix milliseconds), one group or all (`group`
/// empty). FINAL: a retried insert or two owners during a hand-off write the same row twice.
pub fn transitions(tenant: &TenantId, monitor: i64, from: i64, until: i64, group: &str) -> Sql {
    range("monitor_transitions FINAL", TRANSITION_COLUMNS, tenant, monitor, from, until, one_or_all(group))
}

/// Each open group's latest transition before `from`, one group or all: the state the groups
/// enter the range in, which no transition inside it may say.
pub fn initial(tenant: &TenantId, monitor: i64, from: i64, group: &str) -> Sql {
    let mut bound = SqlParams::new();
    let mut condition = scope(&mut bound, tenant, monitor);
    condition.push_str(&format!(" AND at < fromUnixTimestamp64Milli({})", bound.add(SqlValue::Int64(from))));
    if !group.is_empty() {
        condition.push_str(&format!(" AND group_key = {}", bound.add(SqlValue::String(group.to_string()))));
    }
    let closed = bound.add(closed());

    Sql {
        text: format!(
            "SELECT * FROM (SELECT {TRANSITION_COLUMNS} FROM monitor_transitions WHERE {condition} \
             ORDER BY group_key, at DESC LIMIT 1 BY group_key) WHERE reason NOT IN {closed} ORDER BY group_key"
        ),
        parameters: bound.into_parameters(),
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EvaluationRecord {
    /// Empty for an evaluation skipped or failed as a whole.
    pub group: String,
    #[serde(serialize_with = "crate::json::as_offset")]
    pub at: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    /// "none" before the group's first transition.
    pub state: String,
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A monitor's evaluations in `[from, until)`, one group or all: the values for its graph. One
/// group's come with the monitor's own rows, the evaluations skipped or failed as a whole, so a gap
/// in its graph has its reason.
pub fn evaluations(tenant: &TenantId, monitor: i64, from: i64, until: i64, group: &str) -> Sql {
    let groups = if group.is_empty() { vec![] } else { vec![group.to_string(), String::new()] };
    range(
        "monitor_evaluations",
        "group_key, toUnixTimestamp64Milli(at) AS at_ms, value, state, outcome, detail",
        tenant,
        monitor,
        from,
        until,
        groups,
    )
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    fn acme() -> TenantId {
        TenantId("acme".to_string())
    }

    fn noon() -> i64 {
        Utc.with_ymd_and_hms(2026, 10, 14, 12, 0, 0).unwrap().timestamp_millis()
    }

    const HOUR: i64 = 3_600_000;

    fn values(sql: &Sql) -> Vec<&SqlValue> {
        sql.parameters.iter().map(|(_, value)| value).collect()
    }

    #[test]
    fn every_statement_binds_the_tenant_in_every_table_it_reads() {
        let statements = [
            ("status", status(&acme(), &[7, 8])),
            ("groups", groups(&acme(), 7)),
            ("transitions", transitions(&acme(), 7, noon() - HOUR, noon(), "host:a")),
            ("initial", initial(&acme(), 7, noon() - HOUR, "host:a")),
            ("evaluations", evaluations(&acme(), 7, noon() - HOUR, noon(), "")),
        ];
        for (name, sql) in statements {
            let tenants = values(&sql).into_iter().filter(|v| **v == SqlValue::String("acme".into())).count();
            let tables = sql.text.matches("FROM monitor_").count();
            assert!(tables > 0, "{name}");
            assert_eq!(tenants, tables, "{name}: {tables} tables read, the tenant bound {tenants} times");
            assert!(!sql.text.contains("acme"), "{name}");
        }
    }

    #[test]
    fn a_callers_group_and_ids_are_bound_never_written_into_the_sql() {
        let transitions = transitions(&acme(), 7, noon() - HOUR, noon(), "host:a' OR 1=1");
        assert!(!transitions.text.contains("OR 1=1"));
        assert!(values(&transitions).contains(&&SqlValue::StringArray(vec!["host:a' OR 1=1".into()])));
        let initial = initial(&acme(), 7, noon(), "host:a' OR 1=1");
        assert!(!initial.text.contains("OR 1=1"));
        assert!(values(&initial).contains(&&SqlValue::String("host:a' OR 1=1".into())));
        assert!(values(&status(&acme(), &[7, 8])).contains(&&SqlValue::Int64Array(vec![7, 8])));
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn ids_are_comma_separated_or_repeated_positive_at_most_max_ids() {
        assert_eq!(parse_ids(&strings(&["1, 2", "3,1"])), Ok(vec![1, 2, 3]));
        assert_eq!(parse_ids(&[]), Err("ids is required".to_string()));
        assert_eq!(parse_ids(&strings(&[","])), Err("ids is required".to_string()));
        assert_eq!(parse_ids(&strings(&["1,x"])), Err("bad monitor id \"x\"".to_string()));
        assert_eq!(parse_ids(&strings(&["-4"])), Err("bad monitor id \"-4\"".to_string()));
        let ids = |n: usize| vec![(1..=n).map(|i| i.to_string()).collect::<Vec<_>>().join(",")];
        assert!(parse_ids(&ids(MAX_IDS)).is_ok());
        assert!(parse_ids(&ids(MAX_IDS + 1)).is_err());
    }

    fn row(monitor: i64, state: &str, open: bool, groups: i64, triggered: Option<i64>) -> StatusRow {
        StatusRow {
            monitor,
            state: state.to_string(),
            open,
            groups,
            last_triggered_at: triggered.map(|minutes| noon() - minutes * 60_000),
        }
    }

    #[test]
    fn a_monitors_state_is_its_most_severe_open_groups_one_status_per_id_in_order() {
        let rows = [
            row(7, "OK", true, 3, None),
            row(7, "No Data", true, 1, Some(30)),
            row(7, "Warn", true, 2, Some(20)),
            // Closed groups count only for when the monitor last triggered.
            row(7, "OK", false, 4, Some(5)),
            row(8, "OK", false, 1, None),
            row(9, "Alert", true, 1, Some(1)),
        ];
        match statuses(&[8, 7, 10], &rows).as_slice() {
            [eight, seven, ten] => {
                assert_eq!(seven.state.as_deref(), Some("Warn"));
                let expected: BTreeMap<String, i64> =
                    [("OK", 3), ("Warn", 2), ("Alert", 0), ("No Data", 1)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
                assert_eq!(seven.groups, expected);
                assert_eq!(seven.last_triggered_at, Some(noon() - 5 * 60_000));
                assert_eq!(eight.state, None);
                assert_eq!(eight.groups["OK"], 0);
                assert_eq!(ten.id, 10);
                assert_eq!(ten.state, None);
                assert_eq!(ten.last_triggered_at, None);
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn past_the_row_limit_the_oldest_rows_are_dropped_and_that_is_said() {
        let rows: Vec<usize> = (0..=ROW_LIMIT).collect();
        let (kept, truncated) = newest(rows);
        assert!(truncated);
        assert_eq!(kept.len(), ROW_LIMIT);
        assert_eq!(kept[0], 1);
        assert_eq!(newest(vec![1, 2]), (vec![1, 2], false));
    }

    #[test]
    fn the_sql_is_the_fsharps() {
        let s = status(&acme(), &[7, 8]);
        assert_eq!(
            s.text,
            "SELECT monitor_id, group_state, is_open, count(), toUnixTimestamp64Milli(max(group_triggered)) FROM (SELECT monitor_id, argMax(to_state, at) AS group_state, argMax(reason, at) NOT IN {p0:Array(String)} AS is_open, max(last_triggered_at) AS group_triggered FROM monitor_transitions WHERE tenant_id = {p1:String} AND monitor_id IN {p2:Array(Int64)} GROUP BY monitor_id, group_key) GROUP BY monitor_id, group_state, is_open"
        );
        assert_eq!(s.parameters[0].1, SqlValue::StringArray(strings(&["expired", "definition_changed", "deleted"])));

        assert_eq!(
            evaluations(&acme(), 7, 1, 2, "host:a").text,
            "SELECT * FROM (SELECT group_key, toUnixTimestamp64Milli(at) AS at_ms, value, state, outcome, detail FROM monitor_evaluations WHERE tenant_id = {p0:String} AND monitor_id = {p1:Int64} AND at >= fromUnixTimestamp64Milli({p2:Int64}) AND at < fromUnixTimestamp64Milli({p3:Int64}) AND group_key IN {p4:Array(String)} ORDER BY at DESC, group_key LIMIT 10001) ORDER BY at_ms, group_key"
        );

        assert_eq!(
            initial(&acme(), 7, 1, "").text,
            "SELECT * FROM (SELECT group_key, group_tags, toUnixTimestamp64Milli(at) AS at_ms, from_state, to_state, reason, actor, value, crossed, comparator, critical, warning, critical_recovery, warning_recovery, toUnixTimestamp64Milli(window_from), toUnixTimestamp64Milli(window_to), toUnixTimestamp64Milli(first_triggered_at), toUnixTimestamp64Milli(last_triggered_at), monitor_version FROM monitor_transitions WHERE tenant_id = {p0:String} AND monitor_id = {p1:Int64} AND at < fromUnixTimestamp64Milli({p2:Int64}) ORDER BY group_key, at DESC LIMIT 1 BY group_key) WHERE reason NOT IN {p3:Array(String)} ORDER BY group_key"
        );

        assert_eq!(
            groups(&acme(), 7).text,
            "SELECT t.group_key, t.latest_tags, t.group_state, toUnixTimestamp64Milli(t.since), t.group_reason, toUnixTimestamp64Milli(t.first_triggered), toUnixTimestamp64Milli(t.last_triggered), e.last_value, toUnixTimestamp64Milli(e.evaluated_at) FROM (SELECT group_key, argMax(group_tags, at) AS latest_tags, argMax(to_state, at) AS group_state, argMax(reason, at) AS group_reason, max(at) AS since, tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 1) AS first_triggered, tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 2) AS last_triggered FROM monitor_transitions WHERE tenant_id = {p0:String} AND monitor_id = {p1:Int64} GROUP BY group_key) AS t LEFT JOIN (SELECT group_key, argMax(value, at) AS last_value, max(at) AS evaluated_at FROM monitor_evaluations WHERE tenant_id = {p2:String} AND monitor_id = {p3:Int64} AND outcome = 'evaluated' AND at >= now64(3) - INTERVAL 1 DAY GROUP BY group_key) AS e ON e.group_key = t.group_key WHERE t.group_reason NOT IN {p4:Array(String)} ORDER BY t.group_key SETTINGS join_use_nulls = 1"
        );
    }
}
