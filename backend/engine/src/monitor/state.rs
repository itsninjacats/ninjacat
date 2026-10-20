//! What each group of a monitor is in, and how an evaluation moves it: one pure step from the
//! groups and an evaluation to the new groups and the transitions to store. (F#:
//! `Monitor/State.fs`.)

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::io;

use chrono::TimeDelta;
use ninjapaws_anomaly::{Anomalies, Forecast, LinearModel, Outliers, Seasonality};
use serde_json::ser::{CharEscape, Formatter};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

use crate::metric_query::ast::{Arg, BinaryOp, Expr, Literal, MetricQuery, Modifier, TagFilter};
use crate::monitor::check::{self, LONGEST_NO_DATA};
use crate::monitor::definition::{
    ChangeMeasure, Comparator, Detection, Direction, MissingData, Monitor, MonitorQuery, Thresholds, TimeAggregator,
    Window, WindowUnit,
};
use crate::monitor::evaluate::{Evaluation, GroupResult, Period, Reading, Streak};

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
pub const NEVER_NOTIFIED: [Reason; 4] =
    [Reason::NoDataShown, Reason::Expired, Reason::DefinitionChanged, Reason::Deleted];

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

/// A group unseen this long is dropped.
/// WARNING(undocumented): 48 h for a check monitor that notifies No Data, as Datadog keeps one.
fn forget_after(m: &Monitor) -> TimeDelta {
    match m {
        Monitor::ServiceCheck(c) if c.options.notify_no_data => LONGEST_NO_DATA,
        _ => TimeDelta::hours(24),
    }
}

// --- thresholds ------------------------------------------------------------------------------

fn holds(c: Comparator, value: f64, threshold: f64) -> bool {
    match c {
        Comparator::Gt => value > threshold,
        Comparator::Ge => value >= threshold,
        Comparator::Lt => value < threshold,
        Comparator::Le => value <= threshold,
        Comparator::Eq => value == threshold,
        Comparator::Ne => value != threshold,
    }
}

/// Datadog's recovery table: past the recovery threshold, on the side the monitor recovers to.
/// Without one, when the condition stops holding.
fn recovered(c: Comparator, value: f64, threshold: f64, recovery: Option<f64>) -> bool {
    match (recovery, c) {
        (Some(r), Comparator::Gt) => value <= r,
        (Some(r), Comparator::Ge) => value < r,
        (Some(r), Comparator::Lt) => value >= r,
        (Some(r), Comparator::Le) => value > r,
        _ => !holds(c, value, threshold),
    }
}

fn triggered(s: Option<Status>) -> bool {
    matches!(s, Some(Status::Alert | Status::Warn))
}

/// The state a group is judged from: the one a silent No Data interrupted, or its own.
fn resumed(g: &Group) -> Option<Status> {
    g.interrupted.or(g.state)
}

/// The state a value puts a group in, from the state it was in, and the threshold that decided
/// it.
///
/// WARNING(undocumented): Datadog shows 0 as the usual recovery threshold of an anomaly monitor;
/// its default is not documented.
fn judge(
    q: &MonitorQuery,
    t: &Thresholds,
    current: Option<Status>,
    value: f64,
    recovery: Option<f64>,
) -> (Status, Option<Crossed>) {
    let c = q.comparator;
    let alerting = current == Some(Status::Alert);

    let is_recovered = |threshold: f64, recovery_threshold: Option<f64>| match recovery {
        Some(fraction) => fraction <= recovery_threshold.unwrap_or(0.0),
        None => recovered(c, value, threshold, recovery_threshold),
    };

    let leaving = match current {
        Some(Status::Alert) if t.critical_recovery.is_some() => Some(Crossed::CriticalRecovery),
        Some(Status::Warn) if t.warning_recovery.is_some() => Some(Crossed::WarningRecovery),
        _ => None,
    };

    if holds(c, value, q.threshold) || (alerting && !is_recovered(q.threshold, t.critical_recovery)) {
        return (Status::Alert, Some(Crossed::Critical));
    }
    match t.warning {
        Some(w) if holds(c, value, w) => (Status::Warn, Some(Crossed::Warning)),
        // WARNING(undocumented): a group leaving Alert passes through Warn, so it recovers from
        // there by warning_recovery.
        Some(w) if triggered(current) && !is_recovered(w, t.warning_recovery) => (Status::Warn, leaving),
        _ => (Status::Ok, leaving),
    }
}

fn has_known_run(s: &Streak) -> bool {
    s.critical + s.warning + s.ok > 0
}

/// Check::definition fills critical, warning and ok.
/// WARNING(undocumented): a series with `unknown` UNKNOWN runs in a row is No Data, whatever came
/// before them, and a No Data resolves on OK runs since the last UNKNOWN. With no threshold reached
/// the group keeps its state; without `unknown` a series with no known run is left out.
fn judge_runs(t: &Thresholds, current: Option<Status>, streaks: &[Streak]) -> Option<(Status, Option<Crossed>)> {
    let unknown_out = |s: &Streak| t.unknown.is_some_and(|u| f64::from(s.unknown) >= u);
    let (outs, rest): (Vec<&Streak>, Vec<&Streak>) = streaks.iter().partition(|s| unknown_out(s));
    let known: Vec<&Streak> = rest.into_iter().filter(|s| has_known_run(s)).collect();
    let ok_runs = |s: &Streak| if current == Some(Status::NoData) { s.ok_since_unknown } else { s.ok };
    let (critical, warning, ok) = (t.critical.unwrap_or(1.0), t.warning.unwrap_or(1.0), t.ok.unwrap_or(1.0));

    if known.iter().any(|s| f64::from(s.critical) >= critical) {
        Some((Status::Alert, Some(Crossed::Critical)))
    } else if known.iter().any(|s| f64::from(s.warning) >= warning) {
        Some((Status::Warn, Some(Crossed::Warning)))
    } else if !outs.is_empty() {
        Some((Status::NoData, None))
    } else if !known.is_empty() && known.iter().all(|s| f64::from(ok_runs(s)) >= ok) {
        Some((Status::Ok, triggered(current).then_some(Crossed::CriticalRecovery)))
    } else {
        None
    }
}

// --- moving a group --------------------------------------------------------------------------

/// `g` moved to `next` at `at`, and the transition that records it. The transition to OK carries
/// the trigger it ends.
fn move_to(
    g: &Group,
    next: Status,
    reason: Reason,
    value: Option<f64>,
    crossed: Option<Crossed>,
    period: Option<Period>,
    at: i64,
) -> (Group, Transition) {
    let (first, last) = match next {
        Status::Alert | Status::Warn => (if triggered(resumed(g)) { g.first_triggered_at } else { Some(at) }, Some(at)),
        Status::Ok | Status::NoData => (g.first_triggered_at, g.last_triggered_at),
    };

    let moved = Group {
        state: Some(next),
        since: at,
        first_triggered_at: if next == Status::Ok { None } else { first },
        last_triggered_at: last,
        interrupted: if next == Status::NoData && reason == Reason::NoDataShown { g.state } else { None },
        ..g.clone()
    };

    let transition = Transition {
        key: g.key.clone(),
        tags: g.tags.clone(),
        at,
        from: g.state,
        to: next,
        reason,
        value,
        crossed,
        period,
        first_triggered_at: first,
        last_triggered_at: last,
    };

    (moved, transition)
}

fn into(
    g: Group,
    next: Status,
    reason: Reason,
    value: Option<f64>,
    crossed: Option<Crossed>,
    p: &Period,
    at: i64,
) -> (Group, Option<Transition>) {
    if g.state == Some(next) {
        (g, None)
    } else {
        let (moved, transition) = move_to(&g, next, reason, value, crossed, Some(*p), at);
        (moved, Some(transition))
    }
}

fn judged(
    m: &Monitor,
    g: Group,
    reading: &Reading,
    recovery: Option<f64>,
    p: &Period,
    at: i64,
) -> (Group, Option<Transition>) {
    let outcome = match (m, reading) {
        (Monitor::Metric(d), Reading::Judged(value)) => {
            let (next, crossed) = judge(&d.query, &d.options.thresholds, resumed(&g), *value, recovery);
            Some((next, crossed, *value))
        }
        (Monitor::ServiceCheck(c), Reading::Streaks(streaks)) => {
            judge_runs(&c.options.thresholds, resumed(&g), streaks)
                .map(|(next, crossed)| (next, crossed, check::worst(streaks)))
        }
        _ => None,
    };

    let Some((next, crossed, value)) = outcome else {
        return (g, None);
    };

    let reason = match (g.state, next) {
        (_, Status::NoData) => Reason::UnknownRuns,
        (None, _) => Reason::New,
        // WARNING(undocumented): back from a No Data that was not notified, into the state it
        // interrupted, is not notified either; a trigger goes on unbroken. Into another state, it
        // is notified as usual.
        (Some(Status::NoData), _) if g.interrupted == Some(next) => Reason::NoDataShown,
        (Some(_), Status::Ok) => Reason::Recovery,
        (Some(_), _) => Reason::Threshold,
    };

    into(g, next, reason, Some(value), crossed, p, at)
}

/// on_missing_data `default`: a sum over no points is 0 (`.as_count()` takes only sum).
/// WARNING(undocumented): only for a threshold; a missing group of a change monitor keeps its
/// state.
fn zero_when_missing(m: &Monitor) -> bool {
    match m {
        Monitor::Metric(d) => {
            d.options.on_missing_data == Some(MissingData::Default)
                && d.query.detection == Detection::Threshold
                && d.query.aggregator == TimeAggregator::Sum
        }
        Monitor::ServiceCheck(_) => false,
    }
}

/// A judged group with no point in the window. Silence runs to the window's end, so an evaluation
/// delay does not count as silence.
/// WARNING(undocumented): from the last point, not from the last evaluation that had the group.
fn silent(m: &Monitor, g: Group, p: &Period, at: i64) -> (Group, Option<Transition>) {
    let silence = TimeDelta::milliseconds(p.to - g.last_point);
    let o = m.options();

    // Datadog's default for a check is 24 hours.
    let no_data_after = match m {
        Monitor::Metric(d) => o.no_data_timeframe.unwrap_or(d.query.window.length() * 2),
        Monitor::ServiceCheck(_) => o.no_data_timeframe.unwrap_or(TimeDelta::hours(24)),
    };

    let Some(state) = g.state else {
        return (g, None);
    };
    // Judged as 0, it is not without data, so timeout_h does not end it.
    if zero_when_missing(m) {
        return judged(m, g, &Reading::Judged(0.0), None, p, at);
    }
    if let Some(timeout) = o.timeout
        && triggered(resumed(&g))
        && silence >= timeout
    {
        return into(g, Status::Ok, Reason::Timeout, None, None, p, at);
    }

    // WARNING(undocumented): Datadog documents on_missing_data for event-based monitors; its
    // migration guide offers it for metrics. `resolve` is notified like a timeout when it ends
    // Alert or Warn, or a No Data that was notified; not when it ends a No Data that was not.
    match o.on_missing_data {
        Some(MissingData::Default) => (g, None),
        Some(MissingData::ShowNoData) => into(g, Status::NoData, Reason::NoDataShown, None, None, p, at),
        Some(MissingData::ShowAndNotifyNoData) => into(g, Status::NoData, Reason::NoData, None, None, p, at),
        Some(MissingData::Resolve) if g.interrupted == Some(Status::Ok) => {
            into(g, Status::Ok, Reason::NoDataShown, None, None, p, at)
        }
        Some(MissingData::Resolve) => into(g, Status::Ok, Reason::NoData, None, None, p, at),
        None if o.notify_no_data && state != Status::NoData && silence >= no_data_after => {
            into(g, Status::NoData, Reason::NoData, None, None, p, at)
        }
        None => (g, None),
    }
}

fn seen(m: &Monitor, g: Group, r: &GroupResult, p: &Period, at: i64) -> (Group, Option<Transition>) {
    let g = Group { last_seen: at, last_point: g.last_point.max(r.last_point.unwrap_or(g.last_point)), ..g };

    let group_by = match m {
        Monitor::Metric(d) => &d.query.group_by,
        Monitor::ServiceCheck(c) => &c.query.group_by,
    };
    let delay = if group_by.is_empty() { TimeDelta::zero() } else { m.options().new_group_delay };

    match (&r.reading, g.state) {
        (Reading::Skipped(..), _) if r.last_point.is_none() => silent(m, g, p, at),
        (Reading::Skipped(..), _) => (g, None),
        (_, None) if TimeDelta::milliseconds(at - g.first_seen) < delay => (g, None),
        (reading, _) => judged(m, g, reading, r.recovery, p, at),
    }
}

/// A known group missing from the result: silent, and forgotten once unseen for forget_after.
fn absent(m: &Monitor, g: Group, p: &Period, at: i64) -> (Option<Group>, Option<Transition>) {
    let forgotten = TimeDelta::milliseconds(at - g.last_seen) >= forget_after(m);
    match g.state {
        None if forgotten => (None, None),
        Some(_) if forgotten => {
            let (_, transition) = move_to(&g, Status::Ok, Reason::Expired, None, None, None, at);
            (None, Some(transition))
        }
        _ => {
            let (g, transition) = silent(m, g, p, at);
            (Some(g), transition)
        }
    }
}

/// One evaluation applied at the tick `at`: the groups after it and the transitions to store, at
/// most one per group.
pub fn step(
    m: &Monitor,
    groups: &BTreeMap<String, Group>,
    evaluation: &Evaluation,
    at: i64,
) -> (BTreeMap<String, Group>, Vec<Transition>) {
    let results: BTreeMap<&str, &GroupResult> = evaluation.groups.iter().map(|r| (r.key.as_str(), r)).collect();
    let keys: BTreeSet<&str> = groups.keys().map(String::as_str).chain(results.keys().copied()).collect();
    let p = &evaluation.period;

    let mut kept = BTreeMap::new();
    let mut transitions = vec![];
    for key in keys {
        let (group, transition) = match (groups.get(key), results.get(key)) {
            (Some(g), Some(r)) => {
                let (g, t) = seen(m, g.clone(), r, p, at);
                (Some(g), t)
            }
            (None, Some(r)) => {
                let fresh = Group {
                    key: key.to_string(),
                    tags: r.tags.clone(),
                    state: None,
                    since: at,
                    first_triggered_at: None,
                    last_triggered_at: None,
                    first_seen: at,
                    last_seen: at,
                    last_point: r.last_point.unwrap_or(p.from),
                    interrupted: None,
                };
                let (g, t) = seen(m, fresh, r, p, at);
                (Some(g), t)
            }
            (Some(g), None) => absent(m, g.clone(), p, at),
            (None, None) => (None, None),
        };
        if let Some(g) = group {
            kept.insert(key.to_string(), g);
        }
        transitions.extend(transition);
    }
    (kept, transitions)
}

// --- outside a tick --------------------------------------------------------------------------

/// Every judged group to OK, not notified. The caller keeps `at` with the pending rows, so a retry
/// writes the same key.
pub fn close(reason: Reason, at: i64, groups: &BTreeMap<String, Group>) -> (BTreeMap<String, Group>, Vec<Transition>) {
    let transitions = groups
        .values()
        .filter(|g| g.state.is_some())
        .map(|g| move_to(g, Status::Ok, reason, None, None, None, at).1)
        .collect();
    (BTreeMap::new(), transitions)
}

// --- the hash --------------------------------------------------------------------------------
//
// The hash is stored with every transition and was first written by the F# service, so the text
// hashed is what its System.Text.Json wrote (FSharp.SystemTextJson 1.4.36, snake_case names): a
// union as {"Case":…,"Fields":[…]}, a tuple as an array, a None field left out, a TimeSpan as
// [d.]hh:mm:ss[.fffffff], numbers and escapes as .NET writes them.

/// The order F# gives a union: by case, then field by field.
fn filter_rank(f: &TagFilter) -> u8 {
    match f {
        TagFilter::All => 0,
        TagFilter::Tag(..) => 1,
        TagFilter::Bare(_) => 2,
        TagFilter::In(..) => 3,
        TagFilter::Not(_) => 4,
        TagFilter::And(_) => 5,
        TagFilter::Or(_) => 6,
    }
}

fn compare_filters(a: &TagFilter, b: &TagFilter) -> Ordering {
    match (a, b) {
        (TagFilter::Tag(k1, v1), TagFilter::Tag(k2, v2)) => (k1, v1).cmp(&(k2, v2)),
        (TagFilter::Bare(v1), TagFilter::Bare(v2)) => v1.cmp(v2),
        (TagFilter::In(k1, vs1), TagFilter::In(k2, vs2)) => (k1, vs1).cmp(&(k2, vs2)),
        (TagFilter::Not(f1), TagFilter::Not(f2)) => compare_filters(f1, f2),
        (TagFilter::And(fs1), TagFilter::And(fs2)) | (TagFilter::Or(fs1), TagFilter::Or(fs2)) => fs1
            .iter()
            .zip(fs2)
            .map(|(f1, f2)| compare_filters(f1, f2))
            .find(|o| o.is_ne())
            .unwrap_or(fs1.len().cmp(&fs2.len())),
        _ => filter_rank(a).cmp(&filter_rank(b)),
    }
}

fn sorted_filter(f: &TagFilter) -> TagFilter {
    let sorted_all = |filters: &[TagFilter]| {
        let mut sorted: Vec<TagFilter> = filters.iter().map(sorted_filter).collect();
        sorted.sort_by(compare_filters);
        sorted
    };
    match f {
        TagFilter::In(key, values) => {
            let mut values = values.clone();
            values.sort();
            TagFilter::In(key.clone(), values)
        }
        TagFilter::Not(inner) => TagFilter::Not(Box::new(sorted_filter(inner))),
        TagFilter::And(filters) => TagFilter::And(sorted_all(filters)),
        TagFilter::Or(filters) => TagFilter::Or(sorted_all(filters)),
        TagFilter::All | TagFilter::Tag(..) | TagFilter::Bare(_) => f.clone(),
    }
}

/// The expression with what is written in any order sorted: filters and `by` keys.
fn canonical(e: &Expr<MetricQuery>) -> Expr<MetricQuery> {
    match e {
        Expr::Leaf(q) => {
            let mut group_by = q.group_by.clone();
            group_by.sort();
            Expr::Leaf(MetricQuery { filter: sorted_filter(&q.filter), group_by, ..q.clone() })
        }
        Expr::Number(_) => e.clone(),
        Expr::Neg(inner) => Expr::Neg(Box::new(canonical(inner))),
        Expr::Binary(op, l, r) => Expr::Binary(*op, Box::new(canonical(l)), Box::new(canonical(r))),
        Expr::Call(name, args) => Expr::Call(
            name.clone(),
            args.iter()
                .map(|arg| match arg {
                    Arg::Value(v) => Arg::Value(canonical(v)),
                    Arg::Lit(_) | Arg::Named(..) => arg.clone(),
                })
                .collect(),
        ),
    }
}

fn case(name: &str, fields: Vec<Value>) -> Value {
    if fields.is_empty() { json!({ "Case": name }) } else { json!({ "Case": name, "Fields": fields }) }
}

fn strings(values: &[String]) -> Value {
    Value::from(values.to_vec())
}

fn filter_json(f: &TagFilter) -> Value {
    let all = |filters: &[TagFilter]| Value::from(filters.iter().map(filter_json).collect::<Vec<_>>());
    match f {
        TagFilter::All => case("All", vec![]),
        TagFilter::Tag(key, value) => case("Tag", vec![json!(key), json!(value)]),
        TagFilter::Bare(value) => case("Bare", vec![json!(value)]),
        TagFilter::In(key, values) => case("In", vec![json!(key), strings(values)]),
        TagFilter::Not(inner) => case("Not", vec![filter_json(inner)]),
        TagFilter::And(filters) => case("And", vec![all(filters)]),
        TagFilter::Or(filters) => case("Or", vec![all(filters)]),
    }
}

fn literal_json(l: &Literal) -> Value {
    match l {
        Literal::Num(n) => case("Num", vec![json!(n)]),
        Literal::Quoted(s) => case("Quoted", vec![json!(s)]),
        Literal::Word(s) => case("Word", vec![json!(s)]),
    }
}

fn modifier_json(m: &Modifier) -> Value {
    json!({ "name": m.name, "args": m.args.iter().map(literal_json).collect::<Vec<_>>() })
}

fn metric_query_json(q: &MetricQuery) -> Value {
    let mut fields = Map::new();
    if let Some(space_agg) = &q.space_agg {
        fields.insert("space_agg".into(), json!(space_agg));
    }
    fields.insert("metric".into(), json!(q.metric));
    fields.insert("filter".into(), filter_json(&q.filter));
    fields.insert("group_by".into(), strings(&q.group_by));
    fields.insert("modifiers".into(), q.modifiers.iter().map(modifier_json).collect());
    Value::Object(fields)
}

fn expr_json(e: &Expr<MetricQuery>) -> Value {
    match e {
        Expr::Leaf(q) => case("Leaf", vec![metric_query_json(q)]),
        Expr::Number(n) => case("Number", vec![json!(n)]),
        Expr::Neg(inner) => case("Neg", vec![expr_json(inner)]),
        Expr::Binary(op, l, r) => {
            let op = match op {
                BinaryOp::Add => "Add",
                BinaryOp::Sub => "Sub",
                BinaryOp::Mul => "Mul",
                BinaryOp::Div => "Div",
            };
            case("Binary", vec![case(op, vec![]), expr_json(l), expr_json(r)])
        }
        Expr::Call(name, args) => {
            let args: Vec<Value> = args
                .iter()
                .map(|arg| match arg {
                    Arg::Value(v) => case("Value", vec![expr_json(v)]),
                    Arg::Lit(l) => case("Lit", vec![literal_json(l)]),
                    Arg::Named(name, l) => case("Named", vec![json!(name), literal_json(l)]),
                })
                .collect();
            case("Call", vec![json!(name), Value::from(args)])
        }
    }
}

/// System.Text.Json's TimeSpan: the constant ("c") format.
fn time_span(d: TimeDelta) -> String {
    let nanos = i128::from(d.num_seconds()) * 1_000_000_000 + i128::from(d.subsec_nanos());
    let ticks = nanos / 100;
    let sign = if ticks < 0 { "-" } else { "" };
    let ticks = ticks.abs();
    let (days, rest) = (ticks / 864_000_000_000, ticks % 864_000_000_000);
    let (hours, minutes, seconds, fraction) =
        (rest / 36_000_000_000, rest / 600_000_000 % 60, rest / 10_000_000 % 60, rest % 10_000_000);
    let days = if days > 0 { format!("{days}.") } else { String::new() };
    let fraction = if fraction > 0 { format!(".{fraction:07}") } else { String::new() };
    format!("{sign}{days}{hours:02}:{minutes:02}:{seconds:02}{fraction}")
}

fn window_json(w: &Window) -> Value {
    let unit = match w.unit {
        WindowUnit::Minutes => "Minutes",
        WindowUnit::Hours => "Hours",
        WindowUnit::Days => "Days",
        WindowUnit::Weeks => "Weeks",
        WindowUnit::Months => "Months",
    };
    json!({ "count": w.count, "unit": case(unit, vec![]) })
}

fn seasonality_json(s: Seasonality) -> Value {
    let name = match s {
        Seasonality::Hourly => "Hourly",
        Seasonality::Daily => "Daily",
        Seasonality::Weekly => "Weekly",
    };
    case(name, vec![])
}

fn detection_json(d: &Detection) -> Value {
    let span = |d: &Option<TimeDelta>| d.map(|d| Value::from(time_span(d)));
    let text = |t: &Option<String>| t.as_ref().map(|t| Value::from(t.as_str()));
    let object = |fields: Vec<(&str, Option<Value>)>| {
        Value::Object(fields.into_iter().filter_map(|(name, value)| Some((name.to_string(), value?))).collect())
    };

    match d {
        Detection::Threshold => case("Threshold", vec![]),
        Detection::Change(c) => {
            let measure = match c.measure {
                ChangeMeasure::Absolute => "Absolute",
                ChangeMeasure::Percent => "Percent",
            };
            case("Change", vec![json!({ "measure": case(measure, vec![]), "shift": window_json(&c.shift) })])
        }
        Detection::Anomaly(a) => {
            let algorithm = match a.algorithm {
                Anomalies::Basic => case("Basic", vec![]),
                Anomalies::Robust(s) => case("Robust", vec![seasonality_json(s)]),
                Anomalies::Agile(s) => case("Agile", vec![seasonality_json(s)]),
            };
            let direction = match a.direction {
                Direction::Above => "Above",
                Direction::Below => "Below",
                Direction::Both => "Both",
            };
            let spec = object(vec![
                ("algorithm", Some(algorithm)),
                ("bounds", Some(json!(a.bounds))),
                ("direction", Some(case(direction, vec![]))),
                ("alert_window", Some(window_json(&a.alert_window))),
                ("interval", span(&a.interval)),
                ("count_default_zero", Some(json!(a.count_default_zero))),
                ("timezone", text(&a.timezone)),
            ]);
            case("Anomaly", vec![spec])
        }
        Detection::Forecast(f) => {
            let method = match f.method {
                Forecast::Linear(model) => {
                    let model = match model {
                        LinearModel::Default => "DefaultModel",
                        LinearModel::Simple => "SimpleModel",
                        LinearModel::Reactive => "ReactiveModel",
                    };
                    case("LinearForecast", vec![case(model, vec![])])
                }
                Forecast::Seasonal(s) => case("SeasonalForecast", vec![seasonality_json(s)]),
            };
            let spec = object(vec![
                ("method", Some(method)),
                ("deviations", Some(json!(f.deviations))),
                ("interval", span(&f.interval)),
                ("history", span(&f.history)),
                ("timezone", text(&f.timezone)),
            ]);
            case("Forecast", vec![spec])
        }
        Detection::Outlier(o) => {
            // F#'s algorithm is Dbscan of scaled | Mad of scaled * percent.
            let algorithm = match o.algorithm {
                Outliers::Dbscan => case("Dbscan", vec![json!(false)]),
                Outliers::ScaledDbscan => case("Dbscan", vec![json!(true)]),
                Outliers::Mad { percent } => case("Mad", vec![json!(false), json!(percent)]),
                Outliers::ScaledMad { percent } => case("Mad", vec![json!(true), json!(percent)]),
            };
            case("Outlier", vec![json!({ "algorithm": algorithm, "tolerance": o.tolerance })])
        }
    }
}

/// A double as System.Text.Json writes it: the shortest digits that read back, in exponent form
/// past 17 integer digits or below 0.0001.
fn dotnet_number(v: f64) -> String {
    let sign = if v.is_sign_negative() { "-" } else { "" };
    if v == 0.0 {
        return format!("{sign}0");
    }
    let shortest = format!("{:e}", v.abs());
    let (mantissa, exponent) = shortest.split_once('e').unwrap_or((&shortest, "0"));
    let digits = mantissa.replace('.', "");
    let exponent: i32 = exponent.parse().unwrap_or(0);
    let scale = exponent + 1;

    let body = if !(-3..=17).contains(&scale) {
        let fraction = if digits.len() > 1 { format!(".{}", &digits[1..]) } else { String::new() };
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        format!("{}{fraction}E{exponent_sign}{:02}", &digits[..1], exponent.abs())
    } else if scale <= 0 {
        format!("0.{}{digits}", "0".repeat(scale.unsigned_abs() as usize))
    } else if scale as usize >= digits.len() {
        format!("{digits}{}", "0".repeat(scale as usize - digits.len()))
    } else {
        let (whole, fraction) = digits.split_at(scale as usize);
        format!("{whole}.{fraction}")
    };
    format!("{sign}{body}")
}

/// What System.Text.Json's default encoder writes as is: printable ASCII but for the characters
/// HTML gives a meaning to.
fn written_as_is(c: char) -> bool {
    matches!(c, ' '..='~') && !matches!(c, '"' | '\\' | '&' | '\'' | '+' | '<' | '>' | '`')
}

/// serde_json's writer with .NET's numbers and escapes.
struct DotNet;

impl Formatter for DotNet {
    fn write_f64<W: ?Sized + io::Write>(&mut self, writer: &mut W, value: f64) -> io::Result<()> {
        writer.write_all(dotnet_number(value).as_bytes())
    }

    fn write_string_fragment<W: ?Sized + io::Write>(&mut self, writer: &mut W, fragment: &str) -> io::Result<()> {
        for c in fragment.chars() {
            if written_as_is(c) {
                write!(writer, "{c}")?;
            } else {
                for unit in c.encode_utf16(&mut [0; 2]) {
                    write!(writer, "\\u{unit:04X}")?;
                }
            }
        }
        Ok(())
    }

    fn write_char_escape<W: ?Sized + io::Write>(&mut self, writer: &mut W, escape: CharEscape) -> io::Result<()> {
        let text = match escape {
            CharEscape::Quote => "\\u0022".to_string(),
            CharEscape::ReverseSolidus => "\\\\".to_string(),
            CharEscape::Solidus => "/".to_string(),
            CharEscape::Backspace => "\\b".to_string(),
            CharEscape::FormFeed => "\\f".to_string(),
            CharEscape::LineFeed => "\\n".to_string(),
            CharEscape::CarriageReturn => "\\r".to_string(),
            CharEscape::Tab => "\\t".to_string(),
            CharEscape::AsciiControl(byte) => format!("\\u{byte:04X}"),
        };
        writer.write_all(text.as_bytes())
    }
}

/// The JSON text the hash is taken of.
fn hashed_text(value: &Value) -> String {
    let mut out = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, DotNet);
    serde::Serialize::serialize(value, &mut serializer).expect("a JSON value serializes");
    String::from_utf8(out).expect("the writer writes UTF-8")
}

fn query_text(q: &MonitorQuery) -> String {
    hashed_text(&json!([expr_json(&canonical(&q.measured)), detection_json(&q.detection)]))
}

fn monitor_text(m: &Monitor) -> String {
    match m {
        Monitor::Metric(d) => query_text(&d.query),
        Monitor::ServiceCheck(c) => {
            let q = &c.query;
            hashed_text(&json!([
                "service check",
                q.check,
                filter_json(&sorted_filter(&q.filter)),
                strings(&q.group_by)
            ]))
        }
    }
}

fn sha256(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// What the monitor measures and how, not its thresholds, aggregator, window or message: a new
/// version with another hash closes the groups.
pub fn query_hash(q: &MonitorQuery) -> String {
    sha256(&query_text(q))
}

/// A check's hash leaves out last(N) and the thresholds, as a metric's leaves out the window.
pub fn monitor_hash(m: &Monitor) -> String {
    sha256(&monitor_text(m))
}

/// A new version applied at `at`: the groups go on under the same query, and are closed
/// (definition_changed) under another. `previous` is the hash the groups were judged under.
pub fn redefine(
    previous: &str,
    m: &Monitor,
    at: i64,
    groups: &BTreeMap<String, Group>,
) -> (BTreeMap<String, Group>, Vec<Transition>) {
    if monitor_hash(m) == previous { (groups.clone(), vec![]) } else { close(Reason::DefinitionChanged, at, groups) }
}

/// The monitor's state, its most severe judged group; None when no group is judged yet, which a
/// multi alert shows as "no groups found".
pub fn overall(groups: &BTreeMap<String, Group>) -> Option<Status> {
    groups.values().filter_map(|g| g.state).reduce(|a, b| if b.severity() > a.severity() { b } else { a })
}

#[cfg(test)]
mod tests {
    //! The state machine as a table: the groups before and an evaluation in, the groups after and
    //! the transitions out.

    use chrono::{TimeZone, Utc};

    use super::*;
    use crate::monitor::definition::Definition;
    use crate::monitor::evaluate::SkipCause;
    use crate::monitor::validate;
    use crate::query::plan::DEFAULT_RETENTION;

    const SECOND: i64 = 1_000;
    const MINUTE: i64 = 60 * SECOND;
    const HOUR: i64 = 60 * MINUTE;

    const OK: Status = Status::Ok;
    const WARN: Status = Status::Warn;
    const ALERT: Status = Status::Alert;
    const NO_DATA: Status = Status::NoData;

    fn at() -> i64 {
        Utc.with_ymd_and_hms(2026, 10, 13, 12, 0, 0).unwrap().timestamp_millis()
    }

    fn period() -> Period {
        Period { from: at() - 330 * SECOND, to: at() - 30 * SECOND }
    }

    fn definition(query: &str, options: &str) -> Definition {
        validate::definition(DEFAULT_RETENTION, at(), query, options)
            .unwrap_or_else(|problems| panic!("refused:\n{}", problems.join("\n")))
    }

    fn above90(options: &str) -> Definition {
        definition("avg(last_5m):avg:cpu{*} by {host} > 90", options)
    }

    fn simple(options: &str) -> Definition {
        definition("avg(last_5m):avg:cpu{*} > 90", options)
    }

    fn missing(rule: &str) -> String {
        format!(r#"{{"on_missing_data":"{rule}"}}"#)
    }

    fn few_requests(options: &str) -> Definition {
        definition("sum(last_5m):sum:requests{*} by {host} < 10", options)
    }

    fn counted_errors(options: &str) -> Definition {
        definition("sum(last_5m):sum:errors{*} by {host}.as_count() > 5", options)
    }

    fn traffic_drop(options: &str) -> Definition {
        definition("pct_change(sum(last_5m),last_1h):sum:requests{*} by {host} < -50", options)
    }

    /// A group judged before, seen at the previous tick, with a point near the end of its window.
    fn group(key: &str, state: Option<Status>) -> Group {
        Group {
            key: key.to_string(),
            tags: vec![key.to_string()],
            state,
            since: at() - HOUR,
            first_triggered_at: None,
            last_triggered_at: None,
            first_seen: at() - 2 * HOUR,
            last_seen: at() - MINUTE,
            last_point: period().to - 15 * SECOND,
            interrupted: None,
        }
    }

    /// A group in a No Data that show_no_data did not notify, entered from `from`.
    fn shown(key: &str, from: Status) -> Group {
        Group {
            interrupted: Some(from),
            first_triggered_at: if from == OK { None } else { Some(at() - HOUR) },
            ..group(key, Some(NO_DATA))
        }
    }

    fn silent_for(span: i64, g: Group) -> Group {
        Group { last_point: period().to - span, ..g }
    }

    fn judged_at(value: f64) -> Reading {
        Reading::Judged(value)
    }

    fn not_full() -> Reading {
        Reading::Skipped(SkipCause::Incomplete, "not full".into())
    }

    fn map_of(groups: Vec<Group>) -> BTreeMap<String, Group> {
        groups.into_iter().map(|g| (g.key.clone(), g)).collect()
    }

    const A: &str = "host:a";

    type Moved = (&'static str, Option<Status>, Status, Reason);

    struct Case {
        name: &'static str,
        definition: Definition,
        before: Vec<Group>,
        results: Vec<(&'static str, Reading)>,
        /// Each group's state after, None for one not judged yet; groups not listed are gone.
        after: Vec<(&'static str, Option<Status>)>,
        transitions: Vec<Moved>,
    }

    fn case(
        name: &'static str,
        definition: Definition,
        before: Vec<Group>,
        results: Vec<(&'static str, Reading)>,
        after: Vec<(&'static str, Option<Status>)>,
        transitions: Vec<Moved>,
    ) -> Case {
        Case { name, definition, before, results, after, transitions }
    }

    fn cases() -> Vec<Case> {
        let a = A;
        let new_now = r#"{"new_group_delay":0}"#;
        let warning80 = r#"{"thresholds":{"warning":80}}"#;
        vec![
            // --- new groups
            case(
                "a new group's first judgement is a transition from none",
                above90(new_now),
                vec![],
                vec![(a, judged_at(10.0))],
                vec![(a, Some(OK))],
                vec![(a, None, OK, Reason::New)],
            ),
            case(
                "a new group appearing in Alert: one transition, from none",
                above90(new_now),
                vec![],
                vec![(a, judged_at(95.0))],
                vec![(a, Some(ALERT))],
                vec![(a, None, ALERT, Reason::New)],
            ),
            case(
                "a new group waits new_group_delay",
                above90(""),
                vec![],
                vec![(a, judged_at(95.0))],
                vec![(a, None)],
                vec![],
            ),
            case(
                "a waiting group is judged once the delay has passed",
                above90(""),
                vec![Group { first_seen: at() - 60 * SECOND, ..group(a, None) }],
                vec![(a, judged_at(95.0))],
                vec![(a, Some(ALERT))],
                vec![(a, None, ALERT, Reason::New)],
            ),
            case(
                "a waiting group within the delay stays waiting",
                above90(""),
                vec![Group { first_seen: at() - 59 * SECOND, ..group(a, None) }],
                vec![(a, judged_at(95.0))],
                vec![(a, None)],
                vec![],
            ),
            case(
                "a simple alert has no new_group_delay",
                simple(""),
                vec![],
                vec![("*", judged_at(95.0))],
                vec![("*", Some(ALERT))],
                vec![("*", None, ALERT, Reason::New)],
            ),
            case(
                "a new group with nothing to judge waits",
                above90(new_now),
                vec![],
                vec![(a, not_full())],
                vec![(a, None)],
                vec![],
            ),
            // --- thresholds
            case(
                "OK to Alert",
                above90(""),
                vec![group(a, Some(OK))],
                vec![(a, judged_at(95.0))],
                vec![(a, Some(ALERT))],
                vec![(a, Some(OK), ALERT, Reason::Threshold)],
            ),
            case(
                "OK stays OK",
                above90(""),
                vec![group(a, Some(OK))],
                vec![(a, judged_at(50.0))],
                vec![(a, Some(OK))],
                vec![],
            ),
            case(
                "OK to Warn",
                above90(warning80),
                vec![group(a, Some(OK))],
                vec![(a, judged_at(85.0))],
                vec![(a, Some(WARN))],
                vec![(a, Some(OK), WARN, Reason::Threshold)],
            ),
            case(
                "Alert to Warn directly",
                above90(warning80),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(85.0))],
                vec![(a, Some(WARN))],
                vec![(a, Some(ALERT), WARN, Reason::Threshold)],
            ),
            case(
                "Warn to Alert directly",
                above90(warning80),
                vec![group(a, Some(WARN))],
                vec![(a, judged_at(95.0))],
                vec![(a, Some(ALERT))],
                vec![(a, Some(WARN), ALERT, Reason::Threshold)],
            ),
            case(
                "Warn to OK",
                above90(warning80),
                vec![group(a, Some(WARN))],
                vec![(a, judged_at(50.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(WARN), OK, Reason::Recovery)],
            ),
            case(
                "Alert to OK without a recovery threshold, once the condition stops holding",
                above90(""),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(90.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::Recovery)],
            ),
            // --- the recovery table
            case(
                "> stays Alert above critical_recovery",
                above90(r#"{"thresholds":{"critical_recovery":70}}"#),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(70.1))],
                vec![(a, Some(ALERT))],
                vec![],
            ),
            case(
                "> recovers at <= critical_recovery",
                above90(r#"{"thresholds":{"critical_recovery":70}}"#),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(70.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::Recovery)],
            ),
            case(
                ">= stays Alert at critical_recovery",
                definition("avg(last_5m):avg:cpu{*} by {host} >= 90", r#"{"thresholds":{"critical_recovery":70}}"#),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(70.0))],
                vec![(a, Some(ALERT))],
                vec![],
            ),
            case(
                ">= recovers below critical_recovery",
                definition("avg(last_5m):avg:cpu{*} by {host} >= 90", r#"{"thresholds":{"critical_recovery":70}}"#),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(69.9))],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::Recovery)],
            ),
            case(
                "< stays Alert below critical_recovery",
                definition("avg(last_5m):avg:free{*} by {host} < 10", r#"{"thresholds":{"critical_recovery":20}}"#),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(19.9))],
                vec![(a, Some(ALERT))],
                vec![],
            ),
            case(
                "< recovers at >= critical_recovery",
                definition("avg(last_5m):avg:free{*} by {host} < 10", r#"{"thresholds":{"critical_recovery":20}}"#),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(20.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::Recovery)],
            ),
            case(
                "<= stays Alert at critical_recovery",
                definition("avg(last_5m):avg:free{*} by {host} <= 10", r#"{"thresholds":{"critical_recovery":20}}"#),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(20.0))],
                vec![(a, Some(ALERT))],
                vec![],
            ),
            case(
                "<= recovers above critical_recovery",
                definition("avg(last_5m):avg:free{*} by {host} <= 10", r#"{"thresholds":{"critical_recovery":20}}"#),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(20.1))],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::Recovery)],
            ),
            case(
                "Warn stays Warn above warning_recovery",
                above90(r#"{"thresholds":{"warning":80,"warning_recovery":60}}"#),
                vec![group(a, Some(WARN))],
                vec![(a, judged_at(70.0))],
                vec![(a, Some(WARN))],
                vec![],
            ),
            case(
                "Warn recovers at warning_recovery",
                above90(r#"{"thresholds":{"warning":80,"warning_recovery":60}}"#),
                vec![group(a, Some(WARN))],
                vec![(a, judged_at(60.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(WARN), OK, Reason::Recovery)],
            ),
            case(
                "Alert past critical_recovery but short of warning_recovery is Warn",
                above90(r#"{"thresholds":{"critical_recovery":85,"warning":80,"warning_recovery":60}}"#),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(70.0))],
                vec![(a, Some(WARN))],
                vec![(a, Some(ALERT), WARN, Reason::Threshold)],
            ),
            case(
                "== recovers when the value differs",
                definition("avg(last_5m):avg:up{*} by {host} == 0", ""),
                vec![group(a, Some(ALERT))],
                vec![(a, judged_at(1.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::Recovery)],
            ),
            // --- nothing to judge
            case(
                "a skipped group keeps its state",
                above90(""),
                vec![group(a, Some(ALERT))],
                vec![(a, not_full())],
                vec![(a, Some(ALERT))],
                vec![],
            ),
            // --- no data
            case(
                "no data within no_data_timeframe keeps the state",
                above90(r#"{"notify_no_data":true,"no_data_timeframe":10}"#),
                vec![silent_for(594 * SECOND, group(a, Some(OK)))],
                vec![],
                vec![(a, Some(OK))],
                vec![],
            ),
            case(
                "no data for no_data_timeframe is No Data",
                above90(r#"{"notify_no_data":true,"no_data_timeframe":10}"#),
                vec![silent_for(10 * MINUTE, group(a, Some(ALERT)))],
                vec![],
                vec![(a, Some(NO_DATA))],
                vec![(a, Some(ALERT), NO_DATA, Reason::NoData)],
            ),
            case(
                "no_data_timeframe defaults to twice the window",
                above90(r#"{"notify_no_data":true}"#),
                vec![silent_for(594 * SECOND, group(a, Some(OK)))],
                vec![],
                vec![(a, Some(OK))],
                vec![],
            ),
            case(
                "after twice the window, No Data",
                above90(r#"{"notify_no_data":true}"#),
                vec![silent_for(10 * MINUTE, group(a, Some(OK)))],
                vec![],
                vec![(a, Some(NO_DATA))],
                vec![(a, Some(OK), NO_DATA, Reason::NoData)],
            ),
            case(
                "without notify_no_data a silent group keeps its state",
                above90(""),
                vec![silent_for(3 * HOUR, group(a, Some(ALERT)))],
                vec![],
                vec![(a, Some(ALERT))],
                vec![],
            ),
            case(
                "No Data stays No Data",
                above90(r#"{"notify_no_data":true}"#),
                vec![silent_for(HOUR, group(a, Some(NO_DATA)))],
                vec![],
                vec![(a, Some(NO_DATA))],
                vec![],
            ),
            case(
                "data back after No Data",
                above90(r#"{"notify_no_data":true}"#),
                vec![group(a, Some(NO_DATA))],
                vec![(a, judged_at(10.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(NO_DATA), OK, Reason::Recovery)],
            ),
            case(
                "a group not judged yet is never No Data",
                above90(r#"{"notify_no_data":true}"#),
                vec![silent_for(HOUR, group(a, None))],
                vec![],
                vec![(a, None)],
                vec![],
            ),
            // --- timeout_h
            case(
                "Alert silent for timeout_h is OK",
                above90(r#"{"timeout_h":1}"#),
                vec![silent_for(HOUR, group(a, Some(ALERT)))],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::Timeout)],
            ),
            case(
                "Warn silent for timeout_h is OK",
                above90(r#"{"timeout_h":1,"thresholds":{"warning":80}}"#),
                vec![silent_for(HOUR, group(a, Some(WARN)))],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(WARN), OK, Reason::Timeout)],
            ),
            case(
                "Alert silent for less than timeout_h stays",
                above90(r#"{"timeout_h":1}"#),
                vec![silent_for(59 * MINUTE, group(a, Some(ALERT)))],
                vec![],
                vec![(a, Some(ALERT))],
                vec![],
            ),
            case(
                "timeout_h before a longer no_data_timeframe",
                above90(r#"{"timeout_h":1,"notify_no_data":true,"no_data_timeframe":120}"#),
                vec![silent_for(HOUR, group(a, Some(ALERT)))],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::Timeout)],
            ),
            case(
                "No Data before a longer timeout_h",
                above90(r#"{"timeout_h":2,"notify_no_data":true,"no_data_timeframe":60}"#),
                vec![silent_for(HOUR, group(a, Some(ALERT)))],
                vec![],
                vec![(a, Some(NO_DATA))],
                vec![(a, Some(ALERT), NO_DATA, Reason::NoData)],
            ),
            // --- on_missing_data: missing at once, the window the monitor's own
            case(
                "default: a missing avg group keeps Alert",
                above90(&missing("default")),
                vec![group(a, Some(ALERT))],
                vec![],
                vec![(a, Some(ALERT))],
                vec![],
            ),
            case(
                "default: a missing avg group still times out",
                above90(r#"{"on_missing_data":"default","timeout_h":1}"#),
                vec![silent_for(HOUR, group(a, Some(ALERT)))],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::Timeout)],
            ),
            case(
                "default: a missing sum group is judged as 0 and alerts",
                few_requests(&missing("default")),
                vec![group(a, Some(OK))],
                vec![],
                vec![(a, Some(ALERT))],
                vec![(a, Some(OK), ALERT, Reason::Threshold)],
            ),
            case(
                "default: a missing as_count group is judged as 0 and recovers",
                counted_errors(&missing("default")),
                vec![group(a, Some(ALERT))],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::Recovery)],
            ),
            case(
                "default: a missing sum group judged as 0 stays in Alert",
                few_requests(&missing("default")),
                vec![group(a, Some(ALERT))],
                vec![],
                vec![(a, Some(ALERT))],
                vec![],
            ),
            case(
                "default: a missing change group is not judged as 0 and keeps OK",
                traffic_drop(&missing("default")),
                vec![group(a, Some(OK))],
                vec![],
                vec![(a, Some(OK))],
                vec![],
            ),
            case(
                "default: judged as 0, timeout_h does not end it",
                few_requests(r#"{"on_missing_data":"default","timeout_h":1}"#),
                vec![silent_for(2 * HOUR, group(a, Some(ALERT)))],
                vec![],
                vec![(a, Some(ALERT))],
                vec![],
            ),
            case(
                "show_no_data: a missing group is No Data at once, not notified",
                above90(&missing("show_no_data")),
                vec![group(a, Some(OK))],
                vec![],
                vec![(a, Some(NO_DATA))],
                vec![(a, Some(OK), NO_DATA, Reason::NoDataShown)],
            ),
            case(
                "show_no_data: an alerting group missing is No Data, not notified",
                above90(&missing("show_no_data")),
                vec![group(a, Some(ALERT))],
                vec![],
                vec![(a, Some(NO_DATA))],
                vec![(a, Some(ALERT), NO_DATA, Reason::NoDataShown)],
            ),
            case(
                "show_no_data: a sum group is No Data too",
                few_requests(&missing("show_no_data")),
                vec![group(a, Some(OK))],
                vec![],
                vec![(a, Some(NO_DATA))],
                vec![(a, Some(OK), NO_DATA, Reason::NoDataShown)],
            ),
            case(
                "show_no_data: No Data stays No Data",
                above90(&missing("show_no_data")),
                vec![shown(a, OK)],
                vec![],
                vec![(a, Some(NO_DATA))],
                vec![],
            ),
            case(
                "show_no_data: data back is not notified",
                above90(&missing("show_no_data")),
                vec![shown(a, OK)],
                vec![(a, judged_at(10.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(NO_DATA), OK, Reason::NoDataShown)],
            ),
            case(
                "show_no_data: data back ends the trigger the No Data interrupted, notified",
                above90(&missing("show_no_data")),
                vec![shown(a, ALERT)],
                vec![(a, judged_at(10.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(NO_DATA), OK, Reason::Recovery)],
            ),
            case(
                "show_no_data: data back breaching alerts",
                above90(&missing("show_no_data")),
                vec![shown(a, OK)],
                vec![(a, judged_at(95.0))],
                vec![(a, Some(ALERT))],
                vec![(a, Some(NO_DATA), ALERT, Reason::Threshold)],
            ),
            case(
                "show_no_data: an Alert back breaching after the gap is not notified again",
                above90(&missing("show_no_data")),
                vec![shown(a, ALERT)],
                vec![(a, judged_at(95.0))],
                vec![(a, Some(ALERT))],
                vec![(a, Some(NO_DATA), ALERT, Reason::NoDataShown)],
            ),
            case(
                "show_no_data: a Warn back as Warn is not notified again",
                above90(r#"{"on_missing_data":"show_no_data","thresholds":{"warning":80}}"#),
                vec![shown(a, WARN)],
                vec![(a, judged_at(85.0))],
                vec![(a, Some(WARN))],
                vec![(a, Some(NO_DATA), WARN, Reason::NoDataShown)],
            ),
            case(
                "show_no_data: a Warn back as Alert is notified",
                above90(r#"{"on_missing_data":"show_no_data","thresholds":{"warning":80}}"#),
                vec![shown(a, WARN)],
                vec![(a, judged_at(95.0))],
                vec![(a, Some(ALERT))],
                vec![(a, Some(NO_DATA), ALERT, Reason::Threshold)],
            ),
            case(
                "show_no_data: an Alert back above its recovery threshold stays Alert",
                above90(r#"{"on_missing_data":"show_no_data","thresholds":{"critical_recovery":80}}"#),
                vec![shown(a, ALERT)],
                vec![(a, judged_at(85.0))],
                vec![(a, Some(ALERT))],
                vec![(a, Some(NO_DATA), ALERT, Reason::NoDataShown)],
            ),
            case(
                "show_no_data: a No Data that was notified recovers notified",
                above90(&missing("show_no_data")),
                vec![group(a, Some(NO_DATA))],
                vec![(a, judged_at(10.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(NO_DATA), OK, Reason::Recovery)],
            ),
            case(
                "a No Data not notified stays silent on return after the option changed",
                above90(&missing("show_and_notify_no_data")),
                vec![shown(a, OK)],
                vec![(a, judged_at(10.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(NO_DATA), OK, Reason::NoDataShown)],
            ),
            case(
                "show_no_data: an interrupted Alert still times out, notified",
                above90(r#"{"on_missing_data":"show_no_data","timeout_h":1}"#),
                vec![silent_for(HOUR, shown(a, ALERT))],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(NO_DATA), OK, Reason::Timeout)],
            ),
            case(
                "show_no_data: a skipped group with points is not missing",
                above90(&missing("show_no_data")),
                vec![group(a, Some(OK))],
                vec![(a, not_full())],
                vec![(a, Some(OK))],
                vec![],
            ),
            case(
                "show_and_notify_no_data: a missing group is No Data at once, notified",
                above90(&missing("show_and_notify_no_data")),
                vec![group(a, Some(WARN))],
                vec![],
                vec![(a, Some(NO_DATA))],
                vec![(a, Some(WARN), NO_DATA, Reason::NoData)],
            ),
            case(
                "show_and_notify_no_data: data back recovers",
                above90(&missing("show_and_notify_no_data")),
                vec![group(a, Some(NO_DATA))],
                vec![(a, judged_at(10.0))],
                vec![(a, Some(OK))],
                vec![(a, Some(NO_DATA), OK, Reason::Recovery)],
            ),
            case(
                "show_and_notify_no_data: No Data stays No Data",
                above90(&missing("show_and_notify_no_data")),
                vec![group(a, Some(NO_DATA))],
                vec![],
                vec![(a, Some(NO_DATA))],
                vec![],
            ),
            case(
                "resolve: a missing Alert resolves to OK",
                above90(&missing("resolve")),
                vec![group(a, Some(ALERT))],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(ALERT), OK, Reason::NoData)],
            ),
            case(
                "resolve: a missing Warn resolves to OK",
                above90(r#"{"on_missing_data":"resolve","thresholds":{"warning":80}}"#),
                vec![group(a, Some(WARN))],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(WARN), OK, Reason::NoData)],
            ),
            case(
                "resolve: a No Data not notified resolves without a notice",
                above90(&missing("resolve")),
                vec![shown(a, OK)],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(NO_DATA), OK, Reason::NoDataShown)],
            ),
            case(
                "resolve: a No Data that interrupted an Alert resolves notified",
                above90(&missing("resolve")),
                vec![shown(a, ALERT)],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(NO_DATA), OK, Reason::NoData)],
            ),
            case(
                "resolve: a No Data that was notified resolves notified",
                above90(&missing("resolve")),
                vec![group(a, Some(NO_DATA))],
                vec![],
                vec![(a, Some(OK))],
                vec![(a, Some(NO_DATA), OK, Reason::NoData)],
            ),
            case(
                "resolve: a missing OK group stays OK",
                above90(&missing("resolve")),
                vec![group(a, Some(OK))],
                vec![],
                vec![(a, Some(OK))],
                vec![],
            ),
            // Datadog: "the monitor must be able to evaluate data before alerting on missing data".
            case(
                "default: a sum group never judged is not judged as 0",
                few_requests(&missing("default")),
                vec![group(a, None)],
                vec![],
                vec![(a, None)],
                vec![],
            ),
            // --- expiry
            case(
                "a group unseen for 24 h expires to OK",
                above90(""),
                vec![Group { last_seen: at() - 24 * HOUR, ..group(a, Some(ALERT)) }],
                vec![],
                vec![],
                vec![(a, Some(ALERT), OK, Reason::Expired)],
            ),
            case(
                "an OK group expires too, closing it",
                above90(""),
                vec![Group { last_seen: at() - 24 * HOUR, ..group(a, Some(OK)) }],
                vec![],
                vec![],
                vec![(a, Some(OK), OK, Reason::Expired)],
            ),
            case(
                "a group unseen for less than 24 h stays",
                above90(""),
                vec![Group { last_seen: at() - 24 * HOUR + SECOND, ..group(a, Some(OK)) }],
                vec![],
                vec![(a, Some(OK))],
                vec![],
            ),
            case(
                "a group never judged is forgotten without a transition",
                above90(""),
                vec![Group { last_seen: at() - 24 * HOUR, ..group(a, None) }],
                vec![],
                vec![],
                vec![],
            ),
            // --- several groups
            case(
                "each group moves on its own",
                above90(""),
                vec![group("host:a", Some(OK)), group("host:b", Some(ALERT)), group("host:c", Some(OK))],
                vec![("host:a", judged_at(95.0)), ("host:b", judged_at(95.0)), ("host:c", judged_at(10.0))],
                vec![("host:a", Some(ALERT)), ("host:b", Some(ALERT)), ("host:c", Some(OK))],
                vec![("host:a", Some(OK), ALERT, Reason::Threshold)],
            ),
        ]
    }

    fn result(key: &str, reading: Reading, last_point: Option<i64>) -> GroupResult {
        GroupResult { key: key.to_string(), tags: vec![key.to_string()], reading, recovery: None, last_point }
    }

    fn moved(transitions: &[Transition]) -> Vec<(String, Option<Status>, Status, Reason)> {
        transitions.iter().map(|t| (t.key.clone(), t.from, t.to, t.reason)).collect()
    }

    #[test]
    fn the_state_machine() {
        for c in cases() {
            let before = map_of(c.before);
            let evaluation = Evaluation {
                period: period(),
                groups: c.results.into_iter().map(|(key, r)| result(key, r, Some(period().to - 15 * SECOND))).collect(),
            };

            let (after, transitions) = step(&Monitor::Metric(c.definition), &before, &evaluation, at());

            let states: Vec<(String, Option<Status>)> = after.iter().map(|(k, g)| (k.clone(), g.state)).collect();
            let expected: Vec<(String, Option<Status>)> = c.after.iter().map(|(k, s)| (k.to_string(), *s)).collect();
            assert_eq!(states, expected, "{}", c.name);
            let expected: Vec<(String, Option<Status>, Status, Reason)> =
                c.transitions.iter().map(|(k, f, t, r)| (k.to_string(), *f, *t, *r)).collect();
            assert_eq!(moved(&transitions), expected, "{}", c.name);
            assert!(transitions.iter().all(|t| t.at == at()), "{}", c.name);
            let keys: BTreeSet<&str> = transitions.iter().map(|t| t.key.as_str()).collect();
            assert_eq!(keys.len(), transitions.len(), "{}", c.name);
        }
    }

    // --- what transitions carry --------------------------------------------------------------

    fn once(
        d: &Definition,
        groups: &BTreeMap<String, Group>,
        results: Vec<(&str, Reading)>,
        tick: i64,
    ) -> (BTreeMap<String, Group>, Vec<Transition>) {
        let evaluation = Evaluation {
            period: period(),
            groups: results.into_iter().map(|(key, r)| result(key, r, Some(period().to))).collect(),
        };
        step(&Monitor::Metric(d.clone()), groups, &evaluation, tick)
    }

    fn single(transitions: Vec<Transition>) -> Transition {
        assert_eq!(transitions.len(), 1, "{transitions:?}");
        transitions.into_iter().next().unwrap()
    }

    #[test]
    fn a_trigger_keeps_its_start_through_alert_and_warn_the_recovery_carries_it_the_group_drops_it() {
        let d = above90(r#"{"thresholds":{"warning":80,"critical_recovery":85}}"#);
        let (t1, t2, t3) = (at(), at() + MINUTE, at() + 2 * MINUTE);
        let groups = map_of(vec![group(A, Some(OK))]);

        let (groups, alerted) = once(&d, &groups, vec![(A, judged_at(95.0))], t1);
        let alert = single(alerted);
        assert_eq!((alert.first_triggered_at, alert.last_triggered_at), (Some(t1), Some(t1)));
        assert_eq!(alert.crossed, Some(Crossed::Critical));
        assert_eq!(alert.value, Some(95.0));
        assert_eq!(alert.period, Some(period()));
        assert_eq!(groups[A].since, t1);

        let (groups, warned) = once(&d, &groups, vec![(A, judged_at(82.0))], t2);
        let warn = single(warned);
        assert_eq!((warn.first_triggered_at, warn.last_triggered_at), (Some(t1), Some(t2)));
        assert_eq!(warn.crossed, Some(Crossed::Warning));

        let (groups, recovered) = once(&d, &groups, vec![(A, judged_at(50.0))], t3);
        let ok = single(recovered);
        assert_eq!((ok.first_triggered_at, ok.last_triggered_at), (Some(t1), Some(t2)));
        assert_eq!(groups[A].first_triggered_at, None);
        assert_eq!(groups[A].last_triggered_at, Some(t2));
        assert_eq!(groups[A].since, t3);
    }

    #[test]
    fn recovery_past_a_recovery_threshold_names_it() {
        let d = above90(r#"{"thresholds":{"critical_recovery":70}}"#);
        let (_, transitions) = once(&d, &map_of(vec![group(A, Some(ALERT))]), vec![(A, judged_at(60.0))], at());
        assert_eq!(single(transitions).crossed, Some(Crossed::CriticalRecovery));
    }

    #[test]
    fn seeing_a_group_moves_its_last_seen_and_last_point() {
        let (groups, _) = once(&above90(""), &map_of(vec![group(A, Some(OK))]), vec![(A, not_full())], at());
        assert_eq!(groups[A].last_seen, at());
        assert_eq!(groups[A].last_point, period().to);
    }

    /// A group in the result with no point in the window: the engine kept its series for a
    /// function's lookback, or every bucket had no number.
    fn pointless(d: &Definition, g: &Group) -> (BTreeMap<String, Group>, Vec<Transition>) {
        let evaluation = Evaluation {
            period: period(),
            groups: vec![GroupResult {
                key: g.key.clone(),
                tags: g.tags.clone(),
                reading: Reading::Skipped(SkipCause::Incomplete, "no value".into()),
                recovery: None,
                last_point: None,
            }],
        };
        step(&Monitor::Metric(d.clone()), &map_of(vec![g.clone()]), &evaluation, at())
    }

    #[test]
    fn a_group_with_no_point_in_the_window_goes_to_no_data_like_an_absent_one() {
        let d = above90(r#"{"notify_no_data":true}"#);
        let silent = silent_for(10 * MINUTE, group(A, Some(OK)));
        let (groups, transitions) = pointless(&d, &silent);
        assert_eq!(moved(&transitions), vec![(A.to_string(), Some(OK), NO_DATA, Reason::NoData)]);
        assert_eq!(groups[A].last_point, silent.last_point);
        assert_eq!(groups[A].last_seen, at());
    }

    #[test]
    fn a_group_with_no_point_in_the_window_keeps_its_last_point_and_its_state_until_then() {
        let d = above90(r#"{"notify_no_data":true}"#);
        let quiet = silent_for(9 * MINUTE, group(A, Some(ALERT)));
        let (groups, transitions) = pointless(&d, &quiet);
        assert!(transitions.is_empty());
        assert_eq!(groups[A].last_point, quiet.last_point);
        assert_eq!(groups[A].state, Some(ALERT));
    }

    #[test]
    fn no_data_and_timeout_transitions_carry_the_window_expiry_does_not() {
        let d = above90(r#"{"notify_no_data":true}"#);
        let (_, no_data) = once(&d, &map_of(vec![silent_for(HOUR, group(A, Some(OK)))]), vec![], at());
        assert_eq!(single(no_data).period, Some(period()));
        let (_, expired) =
            once(&d, &map_of(vec![Group { last_seen: at() - 24 * HOUR, ..group(A, Some(OK)) }]), vec![], at());
        assert_eq!(single(expired).period, None);
    }

    #[test]
    fn under_on_missing_data_a_group_with_no_point_in_the_window_is_missing_like_an_absent_one() {
        let (_, transitions) = pointless(&above90(&missing("show_and_notify_no_data")), &group(A, Some(OK)));
        assert_eq!(moved(&transitions), vec![(A.to_string(), Some(OK), NO_DATA, Reason::NoData)]);
    }

    #[test]
    fn a_missing_sum_group_judged_as_0_carries_0_and_the_window() {
        let (_, transitions) =
            once(&few_requests(&missing("default")), &map_of(vec![group(A, Some(OK))]), vec![], at());
        let t = single(transitions);
        assert_eq!((t.value, t.crossed, t.period), (Some(0.0), Some(Crossed::Critical), Some(period())));
    }

    #[test]
    fn a_resolve_carries_the_trigger_it_ends() {
        let triggered = Group {
            first_triggered_at: Some(at() - HOUR),
            last_triggered_at: Some(at() - MINUTE),
            ..group(A, Some(ALERT))
        };
        let (groups, transitions) = once(&above90(&missing("resolve")), &map_of(vec![triggered.clone()]), vec![], at());
        let t = single(transitions);
        assert_eq!(
            (t.first_triggered_at, t.last_triggered_at, t.value),
            (triggered.first_triggered_at, triggered.last_triggered_at, None)
        );
        assert_eq!(groups[A].first_triggered_at, None);
    }

    #[test]
    fn a_silent_no_data_inside_a_trigger_keeps_its_start_and_resumes_it() {
        let d = above90(&missing("show_no_data"));
        let first = at() - HOUR;
        let alerting =
            Group { first_triggered_at: Some(first), last_triggered_at: Some(first), ..group(A, Some(ALERT)) };

        let (groups, _) = once(&d, &map_of(vec![alerting]), vec![], at());
        assert_eq!(
            (groups[A].state, groups[A].interrupted, groups[A].first_triggered_at),
            (Some(NO_DATA), Some(ALERT), Some(first))
        );

        let later = at() + MINUTE;
        let (groups, back) = once(&d, &groups, vec![(A, judged_at(95.0))], later);
        let t = single(back);
        assert_eq!((t.first_triggered_at, t.last_triggered_at), (Some(first), Some(later)));
        assert_eq!(
            (groups[A].state, groups[A].interrupted, groups[A].first_triggered_at),
            (Some(ALERT), None, Some(first))
        );
    }

    // --- outside a tick ----------------------------------------------------------------------

    #[test]
    fn closing_sends_every_judged_group_to_ok_and_keeps_none() {
        let changed_at = at() + 12_500;
        let groups = map_of(vec![group("host:a", Some(ALERT)), group("host:b", Some(OK)), group("host:c", None)]);
        let (after, transitions) = close(Reason::Deleted, changed_at, &groups);
        assert!(after.is_empty());
        assert_eq!(
            moved(&transitions),
            vec![
                ("host:a".to_string(), Some(ALERT), OK, Reason::Deleted),
                ("host:b".to_string(), Some(OK), OK, Reason::Deleted)
            ]
        );
        assert!(transitions.iter().all(|t| t.at == changed_at && t.period.is_none()));
    }

    #[test]
    fn the_query_hash_follows_what_is_measured_and_how_not_thresholds_or_the_order_it_is_written_in() {
        let hash = |query: &str| query_hash(&definition(query, "").query);
        for (first, second, same) in [
            ("avg(last_5m):avg:cpu{env:prod} by {host} > 90", "avg(last_5m):avg:cpu{env:prod} by {host} > 95", true),
            ("avg(last_5m):avg:cpu{env:prod} by {host} > 90", "max(last_10m):avg:cpu{env:prod}   by {host} < 10", true),
            (
                "avg(last_5m):avg:cpu{env:prod AND role:web AND host IN (a,b)} by {host,role} > 90",
                "avg(last_5m):avg:cpu{host IN (b,a) AND role:web AND env:prod} by {role,host} > 90",
                true,
            ),
            ("avg(last_5m):avg:cpu{env:prod} by {host} > 90", "avg(last_5m):avg:cpu{env:dev} by {host} > 90", false),
            (
                "avg(last_5m):avg:cpu{env:prod} by {host} > 90",
                "avg(last_5m):avg:cpu{env:prod} by {host,role} > 90",
                false,
            ),
            ("avg(last_5m):avg:cpu{env:prod} by {host} > 90", "avg(last_5m):avg:mem{env:prod} by {host} > 90", false),
            ("avg(last_5m):avg:cpu{*} by {host} > 90", "change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 90", false),
            (
                "change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 90",
                "change(avg(last_5m),10m_ago):avg:cpu{*} by {host} > 90",
                false,
            ),
            (
                "change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 90",
                "pct_change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 90",
                false,
            ),
            (
                "avg(last_4h):anomalies(avg:cpu{*} by {host}, 'agile', 2, alert_window='last_15m') >= 1",
                "avg(last_4h):anomalies(avg:cpu{*} by {host}, 'agile', 2, alert_window='last_15m', timezone='Europe/Warsaw') >= 1",
                false,
            ),
        ] {
            assert_eq!(hash(first) == hash(second), same, "{first} / {second}");
        }
    }

    #[test]
    fn the_query_hash_is_stable() {
        // It is stored with every transition; if this changes, every monitor's groups are closed
        // when the service next starts.
        assert_eq!(
            query_hash(&definition("avg(last_5m):avg:cpu{env:prod} by {host} > 90", "").query),
            "5ddd55ce98d82c97d948f85a3f8dc8a18d6b5369ae3415a03c6adc557c56936e"
        );
    }

    #[test]
    fn the_hashed_text_is_what_the_f_sharp_service_hashed() {
        let d = definition("avg(last_5m):avg:cpu{env:prod} by {host} > 90", "");
        assert_eq!(
            query_text(&d.query),
            r#"[{"Case":"Leaf","Fields":[{"space_agg":"avg","metric":"cpu","filter":{"Case":"Tag","Fields":["env","prod"]},"group_by":["host"],"modifiers":[]}]},{"Case":"Threshold"}]"#
        );
        let forecast = definition(
            "max(next_1w):forecast(avg:system.disk.in_use{*} by {host}, 'linear', 1, interval='60m', history='1w', model='reactive') >= 0.9",
            "",
        );
        assert_eq!(
            query_text(&forecast.query),
            r#"[{"Case":"Leaf","Fields":[{"space_agg":"avg","metric":"system.disk.in_use","filter":{"Case":"All"},"group_by":["host"],"modifiers":[]}]},{"Case":"Forecast","Fields":[{"method":{"Case":"LinearForecast","Fields":[{"Case":"ReactiveModel"}]},"deviations":1,"interval":"01:00:00","history":"7.00:00:00"}]}]"#
        );
    }

    /// Digests the F# service computed (`State.monitorHash`, Engine built 2026-10-18) for these
    /// definitions, every detection and filter shape among them.
    #[test]
    fn the_hashes_are_the_f_sharp_services() {
        for (monitor_type, query, digest) in [
            (
                "query alert",
                r#"avg(last_5m):avg:system.cpu.user{*} > 90"#,
                "663ce9e7bfa856a652aae21d061afcf31970282bd67513789d81708c470b26ee",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:system.cpu.user{env:prod,host:a} by {host,role} > 90.5"#,
                "cd69c685cd0003d6fe3bf49ad184aae58d57d4f0d57754fc71c2575990582115",
            ),
            (
                "query alert",
                r#"sum(last_5m):sum:requests.errors{*}.as_count() / sum:requests.hits{*}.as_count() * 100 > 5"#,
                "e801a4c3d387211fc5b5614579018b41ebc33bc43b0aae776b95c088def2ad3a",
            ),
            (
                "query alert",
                r#"avg(last_5m):-avg:a{*} + 2 - 3 * avg:b{*} > 1e21"#,
                "2ff54509ab793045c63b9e7f9db3aadea86a80c56b9d9833a54a83800b376983",
            ),
            (
                "query alert",
                r#"pct_change(avg(last_5m),last_5m):avg:system.load.1{*} by {host} > 50"#,
                "1d24fe3ebe4fb4e8c46903b67ec4cfc108ad2b947c518eefabd9bed0ae4195bb",
            ),
            (
                "query alert",
                r#"change(avg(last_1h),1h_ago):avg:system.load.1{*} > 50"#,
                "e956ff782a31150d56bf1aff1c8b8768b7a6a3696bfed296376cd5fa698fb8f7",
            ),
            (
                "query alert",
                r#"avg(last_4h):anomalies(avg:system.cpu.user{role:db} by {host}, 'agile', 2, direction='above', alert_window='last_15m', interval=60, count_default_zero='true', seasonality='daily', timezone='Europe/Warsaw') >= 1"#,
                "7243bcc0a239e2c6d3f7781d379daff7f11681ae4e0ce98dbc60055c368d6b8d",
            ),
            (
                "query alert",
                r#"max(next_1w):forecast(avg:system.disk.in_use{*} by {host}, 'linear', 1, interval='60m', history='1w', model='reactive') >= 0.9"#,
                "9848c7604a45054761a573aa66f958f6b0a25f1e9c994369383e1dd731070bad",
            ),
            (
                "query alert",
                r#"max(next_1d):forecast(avg:system.disk.in_use{*}, 'seasonal', 2, seasonality='weekly') >= 0.9"#,
                "e6cd48a547c041a1b3009a82d21bbd852321b704ba3ea0e657c1290b6023e05d",
            ),
            (
                "query alert",
                r#"avg(last_15m):outliers(avg:system.cpu.user{*} by {host}, 'dbscan', 3) > 0"#,
                "31fa275c47b3786e31f419e19f198a528dcfc02a989043b8b27036ff29f2d5b9",
            ),
            (
                "query alert",
                r#"avg(last_15m):outliers(avg:system.cpu.user{*} by {host}, 'scaledmad', 3, 20) > 0"#,
                "0f8b25d1d2956c9e135e797927e14ce82a263e566d0935ee44fb9e1e172d5dcc",
            ),
            (
                "query alert",
                r#"avg(last_15m):outliers(avg:system.cpu.user{*} by {host}, 'mad', 3.5, 80) > 0"#,
                "ad9dc70f00e82c14fcc121f30a85faa7dc59173e1a4daf4dc3e5d821bebfaece",
            ),
            (
                "query alert",
                r#"avg(last_15m):outliers(avg:system.cpu.user{*} by {host}, 'scaledbscan', 2) > 0"#,
                "9f15eb449bda82b19051945bfe4924a51ca8a540dfbb07ce91f950ded962d584",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:a{(role:web OR role:db) AND NOT host:x} > 1"#,
                "2fca7ba169af0a7032f2e6a689848240b154f11806eafb1c901d66a021561c4a",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:a{b:c, a:d, prod, !e:f} > 1"#,
                "04cb5a9e3056d617c32fab0749402e1c5dafd7e688b27a2d62c4c538fc08b9a0",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:a{city:zürich} > 1"#,
                "42a8a49cfe9d0c475b23d61078da566cfb82991d657c97fca5fdd3887764c4ad",
            ),
            (
                "query alert",
                r#"avg(last_5m):abs(avg:a{*}) > 1"#,
                "997362056589cd00d5e3e780447e1bb722f796cb83f19fef69d157ed14334cd2",
            ),
            (
                "query alert",
                r#"avg(last_5m):top(avg:a{*} by {host}, 5, 'mean', 'desc') > 1"#,
                "75b7e3280df06369494db2eadf805bb04c775f0e2921676e30d0b84531e7e4f5",
            ),
            (
                "query alert",
                r#"avg(last_5m):timeshift(avg:a{*}, -3600) > 1"#,
                "904d5b8dd11ed9b501256e2160dce18ebabbc4bb5894412151ed69bfb4241d59",
            ),
            (
                "query alert",
                r#"avg(last_5m):ewma_3(avg:a{*}) / 0.1 > 1"#,
                "953563375616b0724006c16bfd5d0532eb02aed49b14aabd394347a0680798d8",
            ),
            (
                "service check",
                r#""consul.check".over("*").by("host").last(3).count_by_status()"#,
                "6f1f17f753c2fdc59ea11cfd4eae9513a662aa5aabb98ce042cfc0660ff6283c",
            ),
            (
                "service check",
                r#""pg.can_connect".over("env:prod","role IN (a, b)").exclude("host:x").by("host","db").last(3).count_by_status()"#,
                "8711d07b824ba2b3fc0964ef42195aeeb4715b4a42b18edd52204a3585321e54",
            ),
            (
                "service check",
                r#""x".over("*").by("*").last(2).count_by_status()"#,
                "f258514bb701374dd7621e8a18ca325401e4f17316317f05b173055b58f87738",
            ),
            (
                "service check",
                r#""x".last(2).count_by_status()"#,
                "fb279efc028660ade490f8561001d09650c98452554d506c302153cfb49340ea",
            ),
            (
                "query alert",
                r#"avg(last_4h):anomalies(avg:system.cpu.user{*}, 'basic', 2, alert_window='last_15m') >= 1"#,
                "e2e3cf2cbe09205eaee7e82d544b2931a18c9945a6aeba416ad37fd23799ab1e",
            ),
            (
                "query alert",
                r#"avg(last_1d):anomalies(avg:system.cpu.user{*}, 'robust', 3, seasonality='hourly', alert_window='last_1h') >= 0.5"#,
                "fb3ea9bd0ee88a5a3ad43fbd9495a0f6643e8ea31e69c6e90f0c4cd1a14af26a",
            ),
            (
                "query alert",
                r#"avg(last_4h):anomalies(avg:system.cpu.user{*}, 'agile', 2.5, alert_window='last_15m', interval=90) >= 1"#,
                "e30ada72c6cd6480b7048aa9694a9ef0829c6981d989bae8d99d5faa09df9a53",
            ),
            (
                "query alert",
                r#"avg(last_5m):a.b{*}.rollup(max, 60).fill(zero) > 1"#,
                "4ad20c64157db06cf62a8c480331886768bb6e7dd8383b8c9f5dd44f19bfd894",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:a{*}.rollup(avg, 1.5) > 1"#,
                "4c66c1fb981b02e84e669ed437455734b29db5ef090cd962d74f7c631d907343",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:a{*} * 1e21 + 1e15 + 123456789012345 + 0.00001 + 0.0001 + 1.5e-7 + 12345678901234567890 > 1"#,
                "a7fa095bfe20cc3e817e8df175790222008ae5f3b736bb1f38de743d62927584",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:a{role IN (web, db) AND NOT env:dev} by {z,a} > 1"#,
                "4bfba339550e73a46bbfbc93c5b842ebce380f4f6c125394ba55cc3092f232d7",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:a{z:1, a:2, b:1, a:1, NOT}  > 1"#,
                "accd9ae1a87ac7edbd4b90c1cad0c6e9d3d346c428513235fb320ff21c64bf67",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:a{b:c, a:d, prod, !e:f, !a:z, *x} > 1"#,
                "57df2d4a30aa7f7804a4e434db0a2b144dab18eabe6547bb470b01b6c1fe6b71",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:a{(c:1 OR b:2) AND (b:1 OR a:2) AND NOT (z:1 OR a:1)} > 1"#,
                "402615bdf32a4281c6248834eb369616fa5d6f2863058cb9bd637a66487334c6",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:a{role IN (z, a, b) AND role IN (b, a) AND role IN (b)} > 1"#,
                "b1b18b28d14420937443a6e96f5d47ccba1838c20f5f7018907ffb2fc0f357b1",
            ),
            (
                "query alert",
                r#"max(next_1w):forecast(avg:system.disk.in_use{*}, 'linear', 1, timezone='UTC', model='default') >= 0.9"#,
                "2b356d9f72cb6f1a6b6d584686529504cc9dabe751ced7eff58aafc6ef4814eb",
            ),
            (
                "query alert",
                r#"max(next_1w):forecast(avg:system.disk.in_use{*}, 'linear', 1, model='simple', history='3d', interval='90s') >= 0.9"#,
                "e22e07f8439a862d82c577d05bac28ac874489f8fb89610bbb5275e695507526",
            ),
            (
                "query alert",
                r#"max(next_1w):forecast(avg:system.disk.in_use{*}, 'seasonal', 1, seasonality='daily') >= 0.9"#,
                "79cf157f22d91b83fd2c76ab1bb4811a8d0fbe036db14099c8d747f6d6ce3516",
            ),
            (
                "query alert",
                r#"max(next_1w):forecast(avg:system.disk.in_use{*}, 'seasonal', 1, seasonality='hourly') >= 0.9"#,
                "b728e8b06474f83df65b508f2081d58336a80d399ebe3194efa2031e83091ae3",
            ),
            (
                "query alert",
                r#"avg(last_4h):anomalies(avg:cpu{*} by {host}, 'agile', 2, alert_window='last_15m', timezone='Etc/GMT+1') >= 1"#,
                "713c81c45969ba2d1647f73d8cb131b66082123aaaeb7106f4ff4deee6aded6d",
            ),
            (
                "query alert",
                r#"max(next_1w):forecast(avg:disk{*}, 'linear', 1, timezone='Etc/GMT-1') >= 0.9"#,
                "a861a2f439a7da8d8cec4def1a91b9201a9ef200afe4eeb003873d0bcc5a6d62",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:cpu{env:prod} by {host} > 90"#,
                "5ddd55ce98d82c97d948f85a3f8dc8a18d6b5369ae3415a03c6adc557c56936e",
            ),
            (
                "query alert",
                r#"avg(last_5m):avg:cpu{host IN (b,a) AND role:web AND env:prod} by {role,host} > 90"#,
                "b28098b52a9a6eb6af67d942d8c0f5e0df0b2d47602bd0e7ccee9394e246c929",
            ),
            (
                "query alert",
                r#"avg(last_5m):sum:a{*}.as_rate().rollup(sum, 120) > 1"#,
                "9d23bb28247b9c67516dcedd43a81b1029b339a4c7a46a36c0624584ca8023fa",
            ),
            (
                "service check",
                r#""pg.can_connect".over("env:prod","role:db").exclude("host:x","az:b").by("host","az").last(3).count_by_status()"#,
                "06cbca00e5046400aebb4b6809de72ddaaabcde5a1e40e1099770825c2911457",
            ),
        ] {
            let m = validate::by_type(DEFAULT_RETENTION, at(), monitor_type, query, "")
                .unwrap_or_else(|problems| panic!("{query}: {}", problems.join("\n")));
            assert_eq!(monitor_hash(&m), digest, "{query}\n{}", monitor_text(&m));
        }
    }

    /// What System.Text.Json wrote for these values.
    #[test]
    fn numbers_escapes_and_spans_as_dotnet_writes_them() {
        let numbers = [
            0.0,
            -0.0,
            1.0,
            -60.0,
            1e15,
            1e16,
            1e17,
            1.5e16,
            12345678901234567.0,
            123456789012345678.0,
            1e-4,
            1.5e-4,
            1e-5,
            1.2345e-5,
            1e300,
            5e-324,
            1.7976931348623157e308,
            0.1,
            1.0 / 3.0,
            2.0 / 3.0,
            100.5,
            1e21,
            -1e21,
            123.456e10,
        ];
        let written: Vec<String> = numbers.iter().map(|n| hashed_text(&json!(n))).collect();
        assert_eq!(
            written,
            [
                "0",
                "-0",
                "1",
                "-60",
                "1000000000000000",
                "10000000000000000",
                "1E+17",
                "15000000000000000",
                "12345678901234568",
                "1.2345678901234568E+17",
                "0.0001",
                "0.00015",
                "1E-05",
                "1.2345E-05",
                "1E+300",
                "5E-324",
                "1.7976931348623157E+308",
                "0.1",
                "0.3333333333333333",
                "0.6666666666666666",
                "100.5",
                "1E+21",
                "-1E+21",
                "1234560000000",
            ]
        );

        let ascii: Vec<String> = (0u8..128).map(|c| char::from(c).to_string()).collect();
        assert_eq!(hashed_text(&json!(ascii)), ASCII);
        let wider = ["é", "😀", "\u{2028}", "\u{FFFD}", "日本", "\u{7F}", "\u{80}", "\u{A0}"];
        assert_eq!(
            hashed_text(&json!(wider)),
            r#"["\u00E9","\uD83D\uDE00","\u2028","\uFFFD","\u65E5\u672C","\u007F","\u0080","\u00A0"]"#
        );

        let spans = [
            TimeDelta::milliseconds(500),
            TimeDelta::seconds(15),
            TimeDelta::days(31),
            TimeDelta::hours(25),
            TimeDelta::milliseconds(1),
            TimeDelta::nanoseconds(100),
            TimeDelta::seconds(3690),
        ];
        assert_eq!(
            spans.map(time_span),
            [
                "00:00:00.5000000",
                "00:00:15",
                "31.00:00:00",
                "1.01:00:00",
                "00:00:00.0010000",
                "00:00:00.0000001",
                "01:01:30"
            ]
        );
    }

    const ASCII: &str = r##"["\u0000","\u0001","\u0002","\u0003","\u0004","\u0005","\u0006","\u0007","\b","\t","\n","\u000B","\f","\r","\u000E","\u000F","\u0010","\u0011","\u0012","\u0013","\u0014","\u0015","\u0016","\u0017","\u0018","\u0019","\u001A","\u001B","\u001C","\u001D","\u001E","\u001F"," ","!","\u0022","#","$","%","\u0026","\u0027","(",")","*","\u002B",",","-",".","/","0","1","2","3","4","5","6","7","8","9",":",";","\u003C","=","\u003E","?","@","A","B","C","D","E","F","G","H","I","J","K","L","M","N","O","P","Q","R","S","T","U","V","W","X","Y","Z","[","\\","]","^","_","\u0060","a","b","c","d","e","f","g","h","i","j","k","l","m","n","o","p","q","r","s","t","u","v","w","x","y","z","{","|","}","~","\u007F"]"##;

    #[test]
    fn a_new_version_under_another_query_closes_the_groups_under_the_same_query_keeps_them() {
        let old = definition("avg(last_5m):avg:cpu{*} by {host} > 90", "");
        let groups = map_of(vec![group(A, Some(ALERT))]);
        let previous = query_hash(&old.query);

        let (kept, none) = redefine(
            &previous,
            &Monitor::Metric(definition("avg(last_5m):avg:cpu{*} by {host} > 80", "")),
            at(),
            &groups,
        );
        assert_eq!(kept, groups);
        assert!(none.is_empty());

        let (reset, closed) = redefine(
            &previous,
            &Monitor::Metric(definition("avg(last_5m):avg:cpu{*} by {role} > 90", "")),
            at(),
            &groups,
        );
        assert!(reset.is_empty());
        assert_eq!(single(closed).reason, Reason::DefinitionChanged);
    }

    // --- the monitor's state -----------------------------------------------------------------

    #[test]
    fn the_monitor_is_in_its_most_severe_judged_groups_state() {
        for (states, expected) in [
            ("", ""),
            ("none", ""),
            ("OK", "OK"),
            ("OK,No Data", "No Data"),
            ("No Data,Warn,OK", "Warn"),
            ("Warn,Alert,No Data,none", "Alert"),
        ] {
            let groups = map_of(
                states
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .enumerate()
                    .map(|(i, s)| group(&format!("host:{i}"), Status::of_text(s)))
                    .collect(),
            );
            assert_eq!(overall(&groups), Status::of_text(expected), "{states}");
        }
    }

    // --- anomaly -----------------------------------------------------------------------------

    fn anomaly(options: &str) -> Definition {
        definition("avg(last_4h):anomalies(avg:cpu{*} by {host}, 'basic', 2, alert_window='last_15m') >= 0.5", options)
    }

    /// An anomaly's trigger fraction and its fraction over recovery_window.
    fn fractions(d: &Definition, state: Status, trigger: f64, recovery: f64) -> Option<Status> {
        let result = GroupResult { recovery: Some(recovery), ..result(A, Reading::Judged(trigger), Some(period().to)) };
        let evaluation = Evaluation { period: period(), groups: vec![result] };
        let (after, _) = step(&Monitor::Metric(d.clone()), &map_of(vec![group(A, Some(state))]), &evaluation, at());
        after[A].state
    }

    #[test]
    fn an_anomaly_recovers_only_when_nothing_in_recovery_window_is_anomalous() {
        let d = anomaly("");
        assert_eq!(fractions(&d, OK, 0.6, 0.6), Some(ALERT));
        // Below the threshold, but anomalous points remain: still Alert.
        assert_eq!(fractions(&d, ALERT, 0.2, 0.1), Some(ALERT));
        assert_eq!(fractions(&d, ALERT, 0.2, 0.0), Some(OK));
        // Nothing judged yet stays OK however the recovery window looks.
        assert_eq!(fractions(&d, OK, 0.2, 0.1), Some(OK));
    }

    #[test]
    fn an_anomaly_recovers_at_or_below_its_recovery_threshold() {
        let d = anomaly(r#"{"thresholds":{"critical":0.5,"critical_recovery":0.1}}"#);
        assert_eq!(fractions(&d, ALERT, 0.3, 0.1), Some(OK));
        assert_eq!(fractions(&d, ALERT, 0.3, 0.15), Some(ALERT));
    }

    // --- service checks ----------------------------------------------------------------------

    fn service_check(query: &str, options: &str) -> Monitor {
        validate::by_type(DEFAULT_RETENTION, at(), "service check", query, options)
            .unwrap_or_else(|problems| panic!("refused:\n{}", problems.join("\n")))
    }

    fn by_host(options: &str) -> Monitor {
        service_check(r#""x".over("*").by("host").last(5).count_by_status()"#, options)
    }

    fn one_group(options: &str) -> Monitor {
        service_check(r#""x".over("*").last(5).count_by_status()"#, options)
    }

    const COUNTS: &str = r#"{"thresholds":{"critical":2,"warning":3,"ok":2}}"#;

    fn series(latest: i32, critical: i32, warning: i32, ok: i32) -> Streak {
        Streak { latest, critical, warning, ok, unknown: 0, ok_since_unknown: ok }
    }

    fn unknown_for(runs: i32) -> Streak {
        Streak { unknown: runs, ..series(3, 0, 0, 0) }
    }

    /// The check group `a` stepped on its series from `groups`: its state after, and its
    /// transitions.
    fn step_with(
        m: &Monitor,
        groups: &BTreeMap<String, Group>,
        streaks: Vec<Streak>,
    ) -> (Option<Status>, Vec<Transition>) {
        let evaluation =
            Evaluation { period: period(), groups: vec![result(A, Reading::Streaks(streaks), Some(period().to))] };
        let (after, transitions) = step(m, groups, &evaluation, at());
        (after[A].state, transitions)
    }

    type Judgement = (Option<Status>, Vec<(Option<Crossed>, Option<f64>)>);

    /// A check group in `state` judged on its series: its state after, and the crossing and value
    /// of the transition, if any.
    fn judge_runs_of(m: &Monitor, state: Option<Status>, streaks: Vec<Streak>) -> Judgement {
        let (after, transitions) = step_with(m, &map_of(vec![group(A, state)]), streaks);
        (after, transitions.iter().map(|t| (t.crossed, t.value)).collect())
    }

    #[test]
    fn a_check_group_is_judged_by_its_streaks_and_the_worst_series_decides() {
        let m = by_host(COUNTS);
        assert_eq!(
            judge_runs_of(&m, Some(OK), vec![series(2, 2, 0, 0)]),
            (Some(ALERT), vec![(Some(Crossed::Critical), Some(2.0))])
        );
        assert_eq!(
            judge_runs_of(&m, Some(OK), vec![series(1, 0, 3, 0)]),
            (Some(WARN), vec![(Some(Crossed::Warning), Some(1.0))])
        );
        assert_eq!(
            judge_runs_of(&m, Some(OK), vec![series(0, 0, 0, 4), series(2, 2, 0, 0)]),
            (Some(ALERT), vec![(Some(Crossed::Critical), Some(2.0))])
        );
        assert_eq!(
            judge_runs_of(&m, Some(ALERT), vec![series(0, 0, 0, 2), series(0, 0, 0, 5)]),
            (Some(OK), vec![(Some(Crossed::CriticalRecovery), Some(0.0))])
        );
        assert_eq!(
            judge_runs_of(&m, Some(ALERT), vec![series(1, 0, 3, 0)]),
            (Some(WARN), vec![(Some(Crossed::Warning), Some(1.0))])
        );
    }

    #[test]
    fn a_check_group_with_no_threshold_reached_keeps_its_state() {
        let m = by_host(COUNTS);
        assert_eq!(judge_runs_of(&m, Some(ALERT), vec![series(0, 0, 0, 3), series(1, 0, 1, 0)]), (Some(ALERT), vec![]));
        // A CRITICAL run is not a WARNING one.
        assert_eq!(judge_runs_of(&m, Some(OK), vec![series(2, 1, 0, 0)]), (Some(OK), vec![]));
        assert_eq!(judge_runs_of(&m, None, vec![series(1, 0, 1, 0)]), (None, vec![]));
    }

    #[test]
    fn a_series_whose_counted_runs_are_all_unknown_does_not_hold_its_group_back() {
        let m = by_host(COUNTS);
        assert_eq!(
            judge_runs_of(&m, Some(ALERT), vec![series(0, 0, 0, 2), unknown_for(5)]),
            (Some(OK), vec![(Some(Crossed::CriticalRecovery), Some(3.0))])
        );
        assert_eq!(judge_runs_of(&m, Some(ALERT), vec![unknown_for(5)]), (Some(ALERT), vec![]));
        assert_eq!(judge_runs_of(&m, Some(OK), vec![unknown_for(5)]), (Some(OK), vec![]));
    }

    const WITH_UNKNOWN: &str = r#"{"thresholds":{"critical":2,"warning":3,"ok":2,"unknown":3}}"#;

    /// A check group in `state` judged on its series: its state after, and the reasons of the
    /// transitions.
    fn reasons(m: &Monitor, state: Option<Status>, streaks: Vec<Streak>) -> (Option<Status>, Vec<Reason>) {
        let (after, transitions) = step_with(m, &map_of(vec![group(A, state)]), streaks);
        (after, transitions.iter().map(|t| t.reason).collect())
    }

    #[test]
    fn unknown_a_series_newest_runs_all_unknown_put_the_group_in_no_data_without_notify_no_data() {
        let m = by_host(WITH_UNKNOWN);
        assert_eq!(
            reasons(&m, Some(OK), vec![Streak { unknown: 3, ..series(3, 0, 0, 2) }]),
            (Some(NO_DATA), vec![Reason::UnknownRuns])
        );
        assert_eq!(
            reasons(&m, Some(OK), vec![series(0, 0, 0, 5), unknown_for(4)]),
            (Some(NO_DATA), vec![Reason::UnknownRuns])
        );
        assert_eq!(
            reasons(&one_group(WITH_UNKNOWN), None, vec![unknown_for(3)]),
            (Some(NO_DATA), vec![Reason::UnknownRuns])
        );
        assert_eq!(reasons(&m, Some(NO_DATA), vec![unknown_for(5)]), (Some(NO_DATA), vec![]));
        assert_eq!(reasons(&m, Some(OK), vec![Streak { unknown: 2, ..series(3, 0, 0, 2) }]), (Some(OK), vec![]));
    }

    #[test]
    fn unknown_another_series_alert_and_warn_come_before_no_data_the_series_own_older_runs_do_not() {
        let m = by_host(WITH_UNKNOWN);
        assert_eq!(
            reasons(&m, Some(ALERT), vec![Streak { unknown: 3, ..series(3, 2, 0, 0) }]),
            (Some(NO_DATA), vec![Reason::UnknownRuns])
        );
        assert_eq!(reasons(&m, Some(ALERT), vec![Streak { unknown: 2, ..series(3, 2, 0, 0) }]), (Some(ALERT), vec![]));
        assert_eq!(reasons(&m, Some(ALERT), vec![series(2, 2, 0, 0), unknown_for(3)]), (Some(ALERT), vec![]));
        assert_eq!(
            reasons(&m, Some(NO_DATA), vec![series(1, 0, 3, 0), unknown_for(4)]),
            (Some(WARN), vec![Reason::Threshold])
        );
    }

    #[test]
    fn unknown_the_known_runs_back_resolve_the_group_by_the_ok_threshold() {
        let m = by_host(WITH_UNKNOWN);
        assert_eq!(reasons(&m, Some(NO_DATA), vec![series(0, 0, 0, 1)]), (Some(NO_DATA), vec![]));
        assert_eq!(reasons(&m, Some(NO_DATA), vec![series(0, 0, 0, 2)]), (Some(OK), vec![Reason::Recovery]));
        // OK runs from before the UNKNOWN ones do not count.
        assert_eq!(
            reasons(&m, Some(NO_DATA), vec![Streak { ok_since_unknown: 1, ..series(0, 0, 0, 4) }]),
            (Some(NO_DATA), vec![])
        );
        assert_eq!(
            reasons(&m, Some(OK), vec![Streak { ok_since_unknown: 1, ..series(0, 0, 0, 4) }]),
            (Some(OK), vec![])
        );
    }

    fn absent_at(m: &Monitor, g: &Group) -> (BTreeMap<String, Group>, Vec<Transition>) {
        step(m, &map_of(vec![g.clone()]), &Evaluation { period: period(), groups: vec![] }, at())
    }

    fn state_after(m: &Monitor, g: &Group) -> Option<Status> {
        absent_at(m, g).0[A].state
    }

    #[test]
    fn a_silent_check_group_goes_to_no_data_after_24_hours_unless_told_otherwise() {
        let notify = by_host(r#"{"notify_no_data":true}"#);
        let quiet = |hours: i64| silent_for(hours * HOUR, group(A, Some(OK)));
        assert_eq!(state_after(&notify, &quiet(24)), Some(NO_DATA));
        assert_eq!(state_after(&notify, &quiet(23)), Some(OK));
        assert_eq!(
            state_after(&by_host(r#"{"notify_no_data":true,"no_data_timeframe":30}"#), &quiet(1)),
            Some(NO_DATA)
        );
    }

    #[test]
    fn with_notify_no_data_a_silent_check_group_is_kept_48_hours_otherwise_24() {
        let unseen =
            |hours: i64| silent_for(hours * HOUR, Group { last_seen: at() - hours * HOUR, ..group(A, Some(OK)) });

        let (kept, _) = absent_at(&by_host(r#"{"notify_no_data":true}"#), &unseen(30));
        assert!(kept.contains_key(A));

        let (gone, expired) = absent_at(&by_host(r#"{"notify_no_data":true}"#), &unseen(48));
        assert!(!gone.contains_key(A));
        assert_eq!(single(expired).reason, Reason::Expired);

        let (forgotten, _) = absent_at(&by_host(""), &unseen(24));
        assert!(!forgotten.contains_key(A));
    }

    #[test]
    fn new_group_delay_holds_a_new_check_group_only_when_it_has_by_keys() {
        let fresh = |m: &Monitor| step_with(m, &BTreeMap::new(), vec![series(2, 1, 0, 0)]).0;
        assert_eq!(fresh(&by_host("")), None);
        assert_eq!(fresh(&by_host(r#"{"new_group_delay":0}"#)), Some(ALERT));
        assert_eq!(fresh(&one_group("")), Some(ALERT));
    }

    #[test]
    fn timeout_h_resolves_a_silent_check_group() {
        let g = silent_for(2 * HOUR, group(A, Some(ALERT)));
        let (after, transitions) = absent_at(&by_host(r#"{"timeout_h":1}"#), &g);
        assert_eq!(after[A].state, Some(OK));
        assert_eq!(single(transitions).reason, Reason::Timeout);
    }

    #[test]
    fn a_checks_hash_follows_the_check_its_filter_and_its_groups_not_last_or_the_thresholds() {
        let hash = |query: &str, options: &str| monitor_hash(&service_check(query, options));
        let base = hash(r#""x".over("env:prod","role:db").by("host","az").last(3).count_by_status()"#, "");
        assert_eq!(base, hash(r#""x".over("role:db","env:prod").by("az","host").last(5).count_by_status()"#, COUNTS));
        assert_ne!(base, hash(r#""y".over("env:prod","role:db").by("host","az").last(3).count_by_status()"#, ""));
        assert_ne!(base, hash(r#""x".over("env:prod").by("host","az").last(3).count_by_status()"#, ""));
        assert_ne!(base, hash(r#""x".over("env:prod","role:db").by("host").last(3).count_by_status()"#, ""));
    }

    #[test]
    fn a_metric_monitors_hash_is_its_query_hash() {
        assert_eq!(
            monitor_hash(&Monitor::Metric(definition("avg(last_5m):avg:cpu{env:prod} by {host} > 90", ""))),
            "5ddd55ce98d82c97d948f85a3f8dc8a18d6b5369ae3415a03c6adc557c56936e"
        );
    }
}
