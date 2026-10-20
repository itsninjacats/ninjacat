//! One evaluation of a metric monitor at the tick `at`; nothing here reads the clock. As in
//! Datadog, `.as_count()` reduces each query before the formula, otherwise the window is reduced
//! after it. (F#: `Monitor/Evaluate.fs`.)

use std::collections::HashMap;
use std::sync::Arc;

use chrono::TimeDelta;

use crate::metric_query::ast::{Expr, MetricQuery};
use crate::monitor::definition::{Definition, MonitorQuery};
use crate::query::execute::{AnomalyModels, Executor};
use crate::query::plan::Retention;
use crate::sql::TenantId;

/// The read window, `[from, to)`, in unix milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Period {
    pub from: i64,
    pub to: i64,
}

/// Why a group in the result is not judged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipCause {
    /// The window is not full, or has no value.
    Incomplete,
    /// A seasonal model, or a forecast, has too little history yet.
    Learning,
    /// An outlier monitor with fewer than 3 groups to compare.
    FewGroups,
}

/// One series of a service check, a host and tag set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Streak {
    /// The newest run's status: 0 OK, 1 WARNING, 2 CRITICAL, 3 UNKNOWN.
    pub latest: i32,
    /// Runs in a row from the newest, among the newest N that are not UNKNOWN.
    pub critical: i32,
    pub warning: i32,
    pub ok: i32,
    /// UNKNOWN runs in a row from the newest.
    pub unknown: i32,
    /// OK runs in a row from the newest, among the newest N of any status: what resolves a No Data.
    pub ok_since_unknown: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Reading {
    /// The value the thresholds are compared with.
    Judged(f64),
    /// The group is in the result, but nothing in this window is judged.
    Skipped(SkipCause, String),
    /// A check group's live series.
    Streaks(Vec<Streak>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupResult {
    /// `host:a,role:web`: the sorted `k:v` pairs; `*` for a simple alert.
    pub key: String,
    pub tags: Vec<String>,
    pub reading: Reading,
    /// An anomaly's fraction over recovery_window, which it recovers by.
    pub recovery: Option<f64>,
    /// None: the engine kept the series for a function's lookback, but the window has no point.
    pub last_point: Option<i64>,
}

/// An evaluation that judged. A group absent from it had no data.
#[derive(Clone, Debug, PartialEq)]
pub struct Evaluation {
    pub period: Period,
    pub groups: Vec<GroupResult>,
}

/// Every minute for windows under a day, every 10 minutes up to two days, every 30 above.
pub fn frequency(q: &MonitorQuery) -> TimeDelta {
    let _ = q;
    todo!()
}

/// Spreads monitors over the first 20 s after a tick; SHA-256, so every process spreads them
/// alike.
pub fn offset(monitor: &str) -> TimeDelta {
    let _ = monitor;
    todo!()
}

/// The next tick on the wall clock, and when it fires. It comes after `previous` even when a timer
/// fired early or the clock stepped back, so a group gets one transition per tick. A missed tick
/// is not made up.
pub fn next_tick(monitor: &str, frequency: TimeDelta, previous: Option<i64>, now: i64) -> (i64, i64) {
    let _ = (monitor, frequency, previous, now);
    todo!()
}

/// The window a tick reads, and its step: `to = floor(at − max(evaluation_delay, 30 s), step)`,
/// `from = to − window`.
///
/// WARNING(undocumented): the 30 s floor. It covers the agent's 15 s flush and the intake's
/// batching.
pub fn period(retention: Retention, d: &Definition, at: i64) -> (Period, TimeDelta) {
    let _ = (retention, d, at);
    todo!()
}

/// The seasonal anomaly models a monitor evaluates with, the tick and the query they were fitted
/// for. Cloning one shares its models.
#[derive(Clone, Debug)]
pub struct Fit {
    pub at: i64,
    pub expression: Expr<MetricQuery>,
    pub models: Arc<HashMap<usize, AnomalyModels>>,
}

/// `previous` while it is fresh and fitted for this query, otherwise new models; None for anything
/// but a robust or agile anomaly monitor.
pub async fn fit<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    retention: Retention,
    d: &Definition,
    previous: Option<&Fit>,
    at: i64,
) -> Result<Option<Fit>, String> {
    let _ = (execute, tenant, retention, d, previous, at);
    todo!()
}

/// Evaluates `d` at the tick `at`; without `models` a seasonal model is fitted for this evaluation
/// alone.
pub async fn evaluate<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    retention: Retention,
    max_groups: usize,
    models: Option<&Fit>,
    d: &Definition,
    at: i64,
) -> Result<Evaluation, String> {
    let _ = (execute, tenant, retention, max_groups, models, d, at);
    todo!()
}
