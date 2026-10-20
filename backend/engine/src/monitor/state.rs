//! What each group of a monitor is in, and how an evaluation moves it: one pure step from the
//! groups and an evaluation to the new groups and the transitions to store. (F#:
//! `Monitor/State.fs`.)

use std::collections::BTreeMap;

use crate::monitor::definition::{Monitor, MonitorQuery};
use crate::monitor::evaluate::{Evaluation, Period};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Ok,
    Warn,
    Alert,
    NoData,
}

impl Status {
    pub const ALL: [Status; 4] = [Status::Ok, Status::Warn, Status::Alert, Status::NoData];

    pub fn text(&self) -> &'static str {
        match self {
            Status::Ok => "OK",
            Status::Warn => "Warn",
            Status::Alert => "Alert",
            Status::NoData => "No Data",
        }
    }

    pub fn of_text(text: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|s| s.text() == text)
    }

    /// The most severe group decides a monitor's state.
    pub fn severity(&self) -> i32 {
        match self {
            Status::Alert => 3,
            Status::Warn => 2,
            Status::NoData => 1,
            Status::Ok => 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    New,
    Threshold,
    Recovery,
    NoData,
    /// Into No Data from a check's UNKNOWN runs, not from silence.
    UnknownRuns,
    /// Into, or back out of, the No Data that show_no_data shows without notifying.
    NoDataShown,
    Timeout,
    Expired,
    DefinitionChanged,
    Deleted,
}

impl Reason {
    pub fn text(&self) -> &'static str {
        match self {
            Reason::New => "new",
            Reason::Threshold => "threshold",
            Reason::Recovery => "recovery",
            Reason::NoData => "no_data",
            Reason::UnknownRuns => "unknown_runs",
            Reason::NoDataShown => "no_data_shown",
            Reason::Timeout => "timeout",
            Reason::Expired => "expired",
            Reason::DefinitionChanged => "definition_changed",
            Reason::Deleted => "deleted",
        }
    }
}

/// Reasons the notifier never notifies. A new group that is OK is not notified either.
pub const NEVER_NOTIFIED: [Reason; 4] = [Reason::NoDataShown, Reason::Expired, Reason::DefinitionChanged, Reason::Deleted];

/// A group whose latest transition has one of these reasons is closed: the service does not load
/// it and the panel does not show it.
pub const CLOSES: [Reason; 3] = [Reason::Expired, Reason::DefinitionChanged, Reason::Deleted];

/// The threshold a transition's value crossed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crossed {
    Critical,
    Warning,
    CriticalRecovery,
    WarningRecovery,
}

impl Crossed {
    pub fn text(&self) -> &'static str {
        match self {
            Crossed::Critical => "critical",
            Crossed::Warning => "warning",
            Crossed::CriticalRecovery => "critical_recovery",
            Crossed::WarningRecovery => "warning_recovery",
        }
    }
}

/// Instants are unix milliseconds.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub key: String,
    pub tags: Vec<String>,
    /// None until the group is first judged.
    pub state: Option<Status>,
    pub since: i64,
    pub first_triggered_at: Option<i64>,
    pub last_triggered_at: Option<i64>,
    pub first_seen: i64,
    pub last_seen: i64,
    pub last_point: i64,
    /// In a No Data that show_no_data did not notify: the state it interrupted, to resume from.
    pub interrupted: Option<Status>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Transition {
    pub key: String,
    pub tags: Vec<String>,
    pub at: i64,
    pub from: Option<Status>,
    pub to: Status,
    pub reason: Reason,
    pub value: Option<f64>,
    pub crossed: Option<Crossed>,
    pub period: Option<Period>,
    pub first_triggered_at: Option<i64>,
    pub last_triggered_at: Option<i64>,
}

/// One evaluation applied at the tick `at`: the groups after it and the transitions to store, at
/// most one per group.
pub fn step(
    m: &Monitor,
    groups: &BTreeMap<String, Group>,
    evaluation: &Evaluation,
    at: i64,
) -> (BTreeMap<String, Group>, Vec<Transition>) {
    let _ = (m, groups, evaluation, at);
    todo!()
}

/// Every judged group to OK, not notified. The caller keeps `at` with the pending rows, so a retry
/// writes the same key.
pub fn close(reason: Reason, at: i64, groups: &BTreeMap<String, Group>) -> (BTreeMap<String, Group>, Vec<Transition>) {
    let _ = (reason, at, groups);
    todo!()
}

/// What the monitor measures and how, not its thresholds, aggregator, window or message: a new
/// version with another hash closes the groups.
pub fn query_hash(q: &MonitorQuery) -> String {
    let _ = q;
    todo!()
}

/// A check's hash leaves out last(N) and the thresholds, as a metric's leaves out the window.
pub fn monitor_hash(m: &Monitor) -> String {
    let _ = m;
    todo!()
}

/// A new version applied at `at`: the groups go on under the same query, and are closed
/// (definition_changed) under another. `previous` is the hash the groups were judged under.
pub fn redefine(
    previous: &str,
    m: &Monitor,
    at: i64,
    groups: &BTreeMap<String, Group>,
) -> (BTreeMap<String, Group>, Vec<Transition>) {
    let _ = (previous, m, at, groups);
    todo!()
}

/// The monitor's state, its most severe judged group; None when no group is judged yet, which a
/// multi alert shows as "no groups found".
pub fn overall(groups: &BTreeMap<String, Group>) -> Option<Status> {
    let _ = groups;
    todo!()
}
