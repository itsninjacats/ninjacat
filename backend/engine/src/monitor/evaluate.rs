//! One evaluation of a metric monitor at the tick `at`; nothing here reads the clock. As in
//! Datadog, `.as_count()` reduces each query before the formula, otherwise the window is reduced
//! after it. (F#: `Monitor/Evaluate.fs`.)

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::TimeDelta;
use ninjapaws_anomaly::{Anomalies, Seasonality};
use sha2::{Digest, Sha256};

use crate::api::v2::scalar::ScalarAggregator;
use crate::api::v2::timeseries::{ParsedFormula, ParsedTimeseriesRequest};
use crate::metric_query::ast::{Arg, Expr, Formula, Literal, MetricQuery};
use crate::monitor::definition::{
    AnomalySpec, ChangeMeasure, Comparator, Definition, Detection, ForecastSpec, MonitorQuery, TimeAggregator, Window,
    WindowUnit,
};
use crate::query::engine::{self, Point, Tag, TimeSeries};
use crate::query::execute::{self, AnomalyModels, Executor, QueryError};
use crate::query::plan::{self, Plan, Retention};
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

// --- when ------------------------------------------------------------------------------------

/// How far back from `to` the evaluation reads.
fn reach(q: &MonitorQuery) -> TimeDelta {
    match &q.detection {
        // WARNING(undocumented): a forecast without history reads as far back as it looks ahead.
        Detection::Forecast(f) => f.history.unwrap_or(q.window.length()),
        // WARNING(undocumented): an anomaly monitor reads, and is timed by, its alert_window; its
        // outer window only sizes graphs.
        Detection::Anomaly(a) => a.alert_window.length(),
        Detection::Threshold | Detection::Change(_) | Detection::Outlier(_) => q.window.length(),
    }
}

/// Every minute for windows under a day, every 10 minutes up to two days, every 30 above.
pub fn frequency(q: &MonitorQuery) -> TimeDelta {
    let judged = reach(q);
    if judged < TimeDelta::hours(24) {
        TimeDelta::minutes(1)
    } else if judged <= TimeDelta::hours(48) {
        TimeDelta::minutes(10)
    } else {
        TimeDelta::minutes(30)
    }
}

/// Spreads monitors over the first 20 s after a tick; SHA-256, so every process spreads them
/// alike.
pub fn offset(monitor: &str) -> TimeDelta {
    let hash = Sha256::digest(monitor.as_bytes());
    let first = u32::from_le_bytes([hash[0], hash[1], hash[2], hash[3]]);
    TimeDelta::milliseconds(i64::from(first % 20_000))
}

/// The next tick on the wall clock, and when it fires. It comes after `previous` even when a timer
/// fired early or the clock stepped back, so a group gets one transition per tick. A missed tick
/// is not made up.
pub fn next_tick(monitor: &str, frequency: TimeDelta, previous: Option<i64>, now: i64) -> (i64, i64) {
    let delay = offset(monitor).num_milliseconds();
    let f = frequency.num_milliseconds();

    let after = match previous {
        Some(p) if p > now - delay => p,
        _ => now - delay,
    };

    let at = after.div_euclid(f) * f + f;
    (at, at + delay)
}

// --- the window and its step -----------------------------------------------------------------

/// The smallest step of whole seconds, at least `step`, that divides the shift, so a shifted
/// bucket lands on a current one.
fn dividing(shift: TimeDelta, step: TimeDelta) -> TimeDelta {
    let shift_seconds = shift.num_seconds();
    let least = (step.num_milliseconds() as f64 / 1000.0).ceil() as i64;

    match (least..=shift_seconds).find(|s| shift_seconds % s == 0) {
        Some(seconds) => TimeDelta::seconds(seconds),
        None => step,
    }
}

/// WARNING(undocumented): 15 s without an interval, so one agent point falls in one bucket.
/// WARNING(undocumented): a forecast's step also fits 1500 points in its horizon.
fn step_of(q: &MonitorQuery) -> TimeDelta {
    let rollup = engine::split(&q.measured).0.iter().filter_map(|p| plan::rollup_interval(&p.query)).max();

    let hint = match (rollup, &q.detection) {
        (Some(interval), _) => interval,
        (None, Detection::Anomaly(AnomalySpec { interval: Some(interval), .. })) => *interval,
        (None, Detection::Forecast(ForecastSpec { interval: Some(interval), .. })) => *interval,
        (None, _) => TimeDelta::seconds(15),
    };

    let span = match &q.detection {
        Detection::Forecast(_) => reach(q).max(q.window.length()),
        _ => reach(q),
    };

    let fitted = plan::step(span, Some(hint));

    match (rollup, &q.detection) {
        (None, Detection::Change(c)) => dividing(c.shift.length(), fitted),
        _ => fitted,
    }
}

fn floor_to(step: TimeDelta, t: i64) -> i64 {
    let step_ms = step.num_milliseconds();
    t.div_euclid(step_ms) * step_ms
}

fn formulas_of(q: &MonitorQuery, formula: Formula) -> Vec<Formula> {
    match &q.detection {
        Detection::Change(c) => {
            let back = Expr::Number(-(c.shift.length().num_seconds() as f64));
            let shifted = Expr::Call("timeshift".into(), vec![Arg::Value(formula.clone()), Arg::Value(back)]);
            vec![formula, shifted]
        }
        // WARNING(undocumented): count_default_zero as default_zero() inside anomalies(): an empty
        // bucket is 0 in the window judged, and in the history fitted from each group's first
        // point.
        Detection::Anomaly(a) if a.count_default_zero => match &formula {
            Expr::Call(name, args) => match args.split_first() {
                Some((Arg::Value(inner), rest)) => {
                    let zeroed = Arg::Value(Expr::Call("default_zero".into(), vec![Arg::Value(inner.clone())]));
                    vec![Expr::Call(name.clone(), [vec![zeroed], rest.to_vec()].concat())]
                }
                _ => vec![formula],
            },
            _ => vec![formula],
        },
        // The engine's horizon is the monitor's next_… window.
        Detection::Forecast(_) => match formula {
            Expr::Call(name, mut args) => {
                let horizon = format!("{}s", q.window.length().num_seconds());
                args.push(Arg::Named("horizon".into(), Literal::Quoted(horizon)));
                vec![Expr::Call(name, args)]
            }
            other => vec![other],
        },
        // Outliers and the query they are picked from, in one request: one read serves both.
        Detection::Outlier(_) => match &formula {
            Expr::Call(_, args) => match args.first() {
                Some(Arg::Value(inner)) => vec![formula.clone(), inner.clone()],
                _ => vec![formula],
            },
            _ => vec![formula],
        },
        Detection::Threshold | Detection::Anomaly(_) => vec![formula],
    }
}

fn request(q: &MonitorQuery, p: &Period, step: TimeDelta) -> ParsedTimeseriesRequest {
    let (queries, formula) = engine::split(&q.expression);

    ParsedTimeseriesRequest {
        from: p.from,
        to: p.to,
        interval: Some(step),
        queries,
        formulas: formulas_of(q, formula).into_iter().map(|formula| ParsedFormula { formula, limit: None }).collect(),
    }
}

/// The window a tick reads, and its step: `to = floor(at − max(evaluation_delay, 30 s), step)`,
/// `from = to − window`.
///
/// WARNING(undocumented): the 30 s floor. It covers the agent's 15 s flush and the intake's
/// batching.
pub fn period(retention: Retention, d: &Definition, at: i64) -> (Period, TimeDelta) {
    let q = &d.query;
    let delay = d.options.evaluation_delay.max(TimeDelta::seconds(30)).num_milliseconds();

    let within = |step: TimeDelta| {
        let until = floor_to(step, at - delay);
        Period { from: until - reach(q).num_milliseconds(), to: until }
    };

    let step = step_of(q);
    let p = within(step);

    // A read past the raw retention, lookback and shift included, makes the engine run whole
    // minutes; the window is aligned to the step it runs.
    match plan::plan(retention, at, &request(q, &p, step)) {
        Ok(plan) if plan.step != step => (within(plan.step), plan.step),
        Ok(_) | Err(_) => (p, step),
    }
}

// --- groups and values -----------------------------------------------------------------------

/// None for an N/A group, which Datadog drops.
/// WARNING(undocumented): a series with several values of a `by` tag is judged in each group;
/// Datadog's template docs suggest it may join them into one.
fn group_of(q: &MonitorQuery, tags: &[Tag]) -> Option<(String, Vec<String>)> {
    let mut pairs = Vec::with_capacity(tags.len());
    for t in tags {
        pairs.push(format!("{}:{}", t.key, t.value.as_deref()?));
    }
    pairs.sort();

    if pairs.is_empty() {
        // A series without tags in a multi alert is no group of it: default_zero over no series
        // gives one.
        q.group_by.is_empty().then(|| ("*".to_string(), pairs))
    } else {
        Some((pairs.join(","), pairs))
    }
}

fn finite(points: &[Point]) -> Vec<Point> {
    points.iter().filter(|p| p.value.is_finite()).copied().collect()
}

fn pairs(points: &[Point]) -> Vec<(i64, f64)> {
    points.iter().map(|p| (p.time, p.value)).collect()
}

/// The first of the least, as F#'s List.min picks it.
fn least(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    values.into_iter().reduce(|acc, x| if x < acc { x } else { acc })
}

fn greatest(values: impl IntoIterator<Item = f64>) -> Option<f64> {
    values.into_iter().reduce(|acc, x| if x > acc { x } else { acc })
}

fn reduce(agg: TimeAggregator, values: &[f64]) -> f64 {
    let sum = values.iter().fold(0.0, |acc, v| acc + v);
    match agg {
        TimeAggregator::Avg => sum / values.len() as f64,
        TimeAggregator::Sum => sum,
        TimeAggregator::Min => least(values.iter().copied()).unwrap_or(f64::NAN),
        TimeAggregator::Max => greatest(values.iter().copied()).unwrap_or(f64::NAN),
    }
}

/// Datadog's require_full_window buckets: a minute for windows counted in minutes, ten for hours,
/// an hour for days. Never finer than the step.
///
/// WARNING(undocumented): weeks and months take the hour of days.
fn full_window_bucket(w: &Window, step: TimeDelta) -> TimeDelta {
    let bucket = match w.unit {
        WindowUnit::Minutes => TimeDelta::minutes(1),
        WindowUnit::Hours => TimeDelta::minutes(10),
        WindowUnit::Days | WindowUnit::Weeks | WindowUnit::Months => TimeDelta::hours(1),
    };
    bucket.max(step)
}

/// Full: the first bucket has data and at most 3 are empty.
fn is_full(bucket: TimeDelta, p: &Period, times: &[i64]) -> bool {
    let bucket_ms = bucket.num_milliseconds() as f64;
    let count = ((p.to - p.from) as f64 / bucket_ms).ceil() as i64;
    let filled: HashSet<i64> = times.iter().map(|t| ((t - p.from) as f64 / bucket_ms) as i64).collect();
    filled.contains(&0) && count - filled.len() as i64 <= 3
}

fn incomplete(reason: &str) -> Reading {
    Reading::Skipped(SkipCause::Incomplete, reason.to_string())
}

const NOT_FULL: &str = "require_full_window: the window is not full";
const NO_VALUE: &str = "no bucket in the window has a value";

/// One group's points reduced to its value. A bucket whose arithmetic gave no number is left out;
/// a window with nothing left is Skipped.
///
/// WARNING(undocumented): Skipped, not No Data, when nothing is left. One Datadog page says No
/// Data.
fn reading_of(d: &Definition, p: &Period, step: TimeDelta, points: &[(i64, f64)]) -> Reading {
    let finite: Vec<(i64, f64)> = points.iter().filter(|(_, v)| v.is_finite()).copied().collect();
    let values: Vec<f64> = finite.iter().map(|(_, v)| *v).collect();
    let times: Vec<i64> = finite.iter().map(|(t, _)| *t).collect();
    let bucket = full_window_bucket(&d.query.window, step);

    // WARNING(undocumented): an empty `.as_count()` or `.as_rate()` bucket reads 0, zero-filled by
    // the engine, so the rule sees no gaps there; on the as_count path the window is one value and
    // is not checked at all.
    if d.options.require_full_window && !d.query.as_count && !is_full(bucket, p, &times) {
        incomplete(NOT_FULL)
    } else if values.is_empty() {
        incomplete(NO_VALUE)
    } else {
        Reading::Judged(reduce(d.query.aggregator, &values))
    }
}

/// Per bucket, the change from the shifted value: `a − b`, or `(a − b) / b × 100`. A bucket with
/// no shifted value has no change.
fn change_of(measure: ChangeMeasure, current: &TimeSeries, previous: Option<&TimeSeries>) -> Vec<(i64, f64)> {
    let before: HashMap<i64, f64> = previous.map(|line| pairs(&line.points).into_iter().collect()).unwrap_or_default();

    pairs(&current.points)
        .into_iter()
        .filter_map(|(t, a)| {
            let b = before.get(&t)?;
            Some(match measure {
                ChangeMeasure::Absolute => (t, a - b),
                ChangeMeasure::Percent => (t, (a - b) / b * 100.0),
            })
        })
        .collect()
}

/// The fraction of the buckets from `from` on outside their band; None when no bucket has one.
/// WARNING(undocumented): the denominator is the buckets present, after any count_default_zero
/// fill.
fn anomalous(a: &AnomalySpec, from: i64, points: &[Point]) -> Option<f64> {
    let judged: Vec<bool> = points
        .iter()
        .filter(|p| p.time >= from)
        .filter_map(|p| p.expected.map(|expected| expected.is_anomaly(p.value, a.direction)))
        .collect();

    if judged.is_empty() {
        None
    } else {
        Some(judged.iter().map(|outside| if *outside { 1.0 } else { 0.0 }).sum::<f64>() / judged.len() as f64)
    }
}

/// The fraction anomalous over alert_window, and the fraction over recovery_window it recovers by.
fn anomaly_reading(
    d: &Definition,
    a: &AnomalySpec,
    p: &Period,
    step: TimeDelta,
    line: &TimeSeries,
) -> (Reading, Option<f64>) {
    let points = finite(&line.points);
    let recovery_window = d.options.recovery_window.unwrap_or(a.alert_window);
    let times: Vec<i64> = points.iter().map(|p| p.time).collect();

    if d.options.require_full_window && !is_full(full_window_bucket(&a.alert_window, step), p, &times) {
        (incomplete(NOT_FULL), None)
    } else if points.is_empty() {
        (incomplete(NO_VALUE), None)
    } else {
        match anomalous(a, p.from, &points) {
            None => {
                (Reading::Skipped(SkipCause::Learning, "learning: the model has too little history yet".into()), None)
            }
            Some(fraction) => {
                let recovery = anomalous(a, p.to - recovery_window.length().num_milliseconds(), &points);
                (Reading::Judged(fraction), Some(recovery.unwrap_or(fraction)))
            }
        }
    }
}

/// A forecast's value: the highest upper bound over the horizon for `>=`, the lowest lower bound
/// for `<=`.
fn forecast_reading(q: &MonitorQuery, line: &TimeSeries) -> Reading {
    if finite(&line.points).is_empty() {
        return incomplete(NO_VALUE);
    }
    let value = match q.comparator {
        Comparator::Le | Comparator::Lt => least(line.forecast.iter().map(|f| f.band.lower)),
        Comparator::Ge | Comparator::Gt | Comparator::Eq | Comparator::Ne => {
            greatest(line.forecast.iter().map(|f| f.band.upper))
        }
    };
    match value {
        Some(value) => Reading::Judged(value),
        None => Reading::Skipped(SkipCause::Learning, "learning: too little history to forecast".into()),
    }
}

// --- running ---------------------------------------------------------------------------------

fn scalar_aggregator(agg: TimeAggregator) -> ScalarAggregator {
    match agg {
        TimeAggregator::Avg => ScalarAggregator::Avg,
        TimeAggregator::Sum => ScalarAggregator::Sum,
        TimeAggregator::Min => ScalarAggregator::Min,
        TimeAggregator::Max => ScalarAggregator::Max,
    }
}

/// The seasonal anomaly models a monitor evaluates with, the tick and the query they were fitted
/// for. Cloning one shares its models.
#[derive(Clone, Debug)]
pub struct Fit {
    pub at: i64,
    pub expression: Expr<MetricQuery>,
    pub models: Arc<HashMap<usize, AnomalyModels>>,
}

/// How long a fit is used; a robust level lags the data by up to this long.
fn refit_every(q: &MonitorQuery) -> Option<TimeDelta> {
    match &q.detection {
        Detection::Anomaly(a) => match a.algorithm {
            Anomalies::Robust(Seasonality::Hourly) | Anomalies::Agile(Seasonality::Hourly) => {
                Some(TimeDelta::minutes(10))
            }
            Anomalies::Robust(_) | Anomalies::Agile(_) => Some(TimeDelta::hours(1)),
            Anomalies::Basic => None,
        },
        Detection::Threshold | Detection::Change(_) | Detection::Forecast(_) | Detection::Outlier(_) => None,
    }
}

async fn timeseries<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    models: Option<&Fit>,
    formulas: usize,
    plan: &Plan,
) -> Result<Vec<Vec<TimeSeries>>, String> {
    let outputs = execute::evaluate(execute, tenant, plan, models.map(|f| f.models.as_ref())).await?;
    let names: Vec<String> = (0..formulas).map(|i| i.to_string()).collect();
    Ok(engine::results(&names, outputs).into_iter().map(|r| r.series).collect())
}

/// Each query reduced by the time aggregator first, the formula after.
async fn scalar<E: Executor>(
    execute: &E,
    tenant: &TenantId,
    agg: TimeAggregator,
    plan: &Plan,
) -> Result<Vec<Vec<TimeSeries>>, String> {
    let aggregators = vec![scalar_aggregator(agg); plan.queries.len()];
    match execute::evaluate_scalar(execute, tenant, plan, &aggregators).await {
        Err(QueryError::Invalid(problems)) => Err(problems.join("; ")),
        Err(QueryError::Failed(message)) => Err(message),
        Ok(outputs) => Ok(outputs
            .into_iter()
            .map(|series| {
                series
                    .into_iter()
                    .map(|s| TimeSeries {
                        tags: engine::tags(&s.group_tags),
                        points: s.points.iter().map(|(&time, &value)| Point { time, value, expected: None }).collect(),
                        forecast: vec![],
                    })
                    .collect()
            })
            .collect()),
    }
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
    let Some(every) = refit_every(&d.query) else {
        return Ok(None);
    };
    if let Some(f) = previous
        && f.expression == d.query.expression
        && at - f.at < every.num_milliseconds()
    {
        return Ok(Some(f.clone()));
    }

    let (p, step) = period(retention, d, at);
    let plan = plan::plan(retention, at, &request(&d.query, &p, step)).map_err(|problems| problems.join("; "))?;
    let models = execute::fit(execute, tenant, &plan).await?;
    Ok(Some(Fit { at, expression: d.query.expression.clone(), models: Arc::new(models) }))
}

/// One group of the result: its key, its sorted tags and its series.
type Keyed<'a> = (String, Vec<String>, &'a TimeSeries);

fn by_group<'a>(q: &MonitorQuery, lines: &'a [TimeSeries]) -> Vec<Keyed<'a>> {
    lines.iter().filter_map(|line| group_of(q, &line.tags).map(|(key, tags)| (key, tags, line))).collect()
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
    let q = &d.query;
    let (p, step) = period(retention, d, at);
    let req = request(q, &p, step);

    if let Detection::Change(c) = &q.detection
        && !q.as_count
        && c.shift.length().num_milliseconds() % step.num_milliseconds() != 0
    {
        return Err(format!(
            "change over {}: the step is {} s, which does not divide the shift, so no bucket has one to compare with",
            c.shift.text(),
            step.num_milliseconds() as f64 / 1000.0
        ));
    }
    let plan = plan::plan(retention, at, &req).map_err(|problems| problems.join("; "))?;

    let outputs = if q.as_count {
        scalar(execute, tenant, q.aggregator, &plan).await?
    } else {
        timeseries(execute, tenant, models, req.formulas.len(), &plan).await?
    };

    // An outlier monitor's groups are the query's; the first output holds only the outliers among
    // them.
    let (judged, outliers): (&[TimeSeries], HashSet<String>) = match (&q.detection, outputs.as_slice()) {
        (Detection::Outlier(_), [picked, all]) => {
            (all, by_group(q, picked).into_iter().map(|(key, _, _)| key).collect())
        }
        _ => (outputs.first().map(Vec::as_slice).unwrap_or_default(), HashSet::new()),
    };

    let mut seen = HashSet::new();
    let current: Vec<Keyed> = by_group(q, judged).into_iter().filter(|(key, _, _)| seen.insert(key.clone())).collect();

    if current.len() > max_groups {
        return Err(format!(
            "too_many_groups: the query yields {} groups, at most {max_groups} are evaluated",
            current.len()
        ));
    }

    let shifted: HashMap<String, &TimeSeries> = match (&q.detection, outputs.as_slice()) {
        (Detection::Change(_), [_, s]) => by_group(q, s).into_iter().map(|(key, _, line)| (key, line)).collect(),
        _ => HashMap::new(),
    };

    let compared = current.iter().filter(|(_, _, line)| !finite(&line.points).is_empty()).count();

    let reading = |key: &str, line: &TimeSeries| -> (Reading, Option<f64>) {
        match &q.detection {
            Detection::Threshold => (reading_of(d, &p, step, &pairs(&line.points)), None),
            Detection::Change(c) => {
                (reading_of(d, &p, step, &change_of(c.measure, line, shifted.get(key).copied())), None)
            }
            Detection::Anomaly(a) => anomaly_reading(d, a, &p, step, line),
            Detection::Forecast(_) => (forecast_reading(q, line), None),
            Detection::Outlier(_) if finite(&line.points).is_empty() => (incomplete(NO_VALUE), None),
            Detection::Outlier(_) if compared < 3 => (
                Reading::Skipped(
                    SkipCause::FewGroups,
                    "an outlier needs at least 3 groups with data to compare".into(),
                ),
                None,
            ),
            // WARNING(undocumented): an outlier is 1 and the rest 0, so `> 0` alerts on the
            // outliers.
            Detection::Outlier(_) => (Reading::Judged(if outliers.contains(key) { 1.0 } else { 0.0 }), None),
        }
    };

    // WARNING(undocumented): on the as_count path the window is one point, so the newest real
    // bucket is unknown and the last one is taken; a group that stops reporting goes to No Data up
    // to a window late.
    let last_point = |line: &TimeSeries| -> Option<i64> {
        if line.points.is_empty() {
            None
        } else if q.as_count {
            Some(p.to - step.num_milliseconds())
        } else {
            line.points.iter().map(|p| p.time).max()
        }
    };

    let groups = current
        .into_iter()
        .map(|(key, tags, line)| {
            let (value, recovery) = reading(&key, line);
            GroupResult { last_point: last_point(line), key, tags, reading: value, recovery }
        })
        .collect();

    Ok(Evaluation { period: p, groups })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::{DateTime, TimeZone, Timelike, Utc};
    use futures::executor::block_on;

    use super::*;
    use crate::monitor::{parse, validate};
    use crate::query::compile::Row;
    use crate::query::execute::Fake;
    use crate::query::plan::DEFAULT_RETENTION;
    use crate::sql::{Sql, SqlValue};

    const SECOND: i64 = 1_000;
    const MINUTE: i64 = 60 * SECOND;
    const HOUR: i64 = 60 * MINUTE;

    fn at() -> i64 {
        Utc.with_ymd_and_hms(2026, 10, 13, 12, 0, 0).unwrap().timestamp_millis()
    }

    /// With the default 30 s delay and a 15 s step, a 5-minute window.
    fn window_from() -> i64 {
        at() - 330 * SECOND
    }

    fn window_to() -> i64 {
        at() - 30 * SECOND
    }

    fn definition(query: &str, options: &str) -> Definition {
        validate::definition(DEFAULT_RETENTION, at(), query, options)
            .unwrap_or_else(|problems| panic!("refused:\n{}", problems.join("\n")))
    }

    fn query_of(text: &str) -> MonitorQuery {
        parse::query(text).unwrap_or_else(|problems| panic!("refused:\n{}", problems.join("\n")))
    }

    /// A point of a metric: its group values (in the query's `by` order), time and value.
    #[derive(Clone)]
    struct MetricPoint {
        metric: &'static str,
        groups: Vec<Option<&'static str>>,
        time: i64,
        value: f64,
    }

    fn param<'a>(sql: &'a Sql, name: &str) -> &'a SqlValue {
        sql.parameters
            .iter()
            .find(|(n, _)| {
                n.strip_prefix(name).is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
            })
            .map(|(_, v)| v)
            .unwrap_or_else(|| panic!("no {name}"))
    }

    fn number(sql: &Sql, name: &str) -> i64 {
        match param(sql, name) {
            SqlValue::Int64(n) => *n,
            other => panic!("{name} is {other:?}"),
        }
    }

    /// Answers compiled SQL from a list, as ClickHouse would for one series per group: the
    /// metric's points in [from, to), each in its bucket.
    fn rows_over(points: &[MetricPoint], sql: &Sql) -> Vec<Row> {
        let metric = match param(sql, "metric") {
            SqlValue::String(m) => m.clone(),
            other => panic!("metric is {other:?}"),
        };
        let (from_ms, to_ms, step_ms) = (number(sql, "from"), number(sql, "to"), number(sql, "step") * 1000);
        let grouped = sql.text.contains("AS g0");

        points
            .iter()
            .filter(|p| p.metric == metric && p.time >= from_ms && p.time < to_ms)
            .map(|p| Row {
                groups: if grouped { p.groups.iter().map(|g| g.map(str::to_string)).collect() } else { vec![] },
                series_id: Some(1),
                bucket_ms: p.time / step_ms * step_ms,
                value: p.value,
            })
            .collect()
    }

    fn execute_over(points: &[MetricPoint]) -> Fake<impl Fn(&Sql) -> Vec<Row> + Sync + '_> {
        Fake(move |sql: &Sql| rows_over(points, sql))
    }

    fn tenant() -> TenantId {
        TenantId("t".into())
    }

    /// One point every 15 s over the window ending at `until`, from `values`.
    fn series(metric: &'static str, groups: &[Option<&'static str>], until: i64, values: &[f64]) -> Vec<MetricPoint> {
        let start = until - 15 * SECOND * values.len() as i64;
        values
            .iter()
            .enumerate()
            .map(|(i, v)| MetricPoint {
                metric,
                groups: groups.to_vec(),
                time: start + 15 * SECOND * i as i64,
                value: *v,
            })
            .collect()
    }

    fn constant(metric: &'static str, groups: &[Option<&'static str>], value: f64) -> Vec<MetricPoint> {
        series(metric, groups, window_to(), &[value; 20])
    }

    fn evaluate_with(max_groups: usize, points: &[MetricPoint], d: &Definition) -> Result<Evaluation, String> {
        block_on(evaluate(&execute_over(points), &tenant(), DEFAULT_RETENTION, max_groups, None, d, at()))
    }

    fn evaluated(points: &[MetricPoint], d: &Definition) -> Evaluation {
        evaluate_with(1000, points, d).unwrap_or_else(|e| panic!("{e}"))
    }

    fn readings(e: &Evaluation) -> Vec<(String, Reading)> {
        e.groups.iter().map(|g| (g.key.clone(), g.reading.clone())).collect()
    }

    fn found<'a>(e: &'a Evaluation, key: &str) -> &'a GroupResult {
        e.groups.iter().find(|g| g.key == key).unwrap_or_else(|| panic!("no group {key}"))
    }

    fn value(e: &Evaluation, key: &str) -> f64 {
        match &found(e, key).reading {
            Reading::Judged(v) => *v,
            Reading::Skipped(_, reason) => panic!("{key} skipped: {reason}"),
            Reading::Streaks(_) => panic!("{key} is a check's"),
        }
    }

    fn skipped(e: &Evaluation, key: &str) -> bool {
        matches!(found(e, key).reading, Reading::Skipped(..))
    }

    /// Equal once both are rounded to `digits` decimals, as xunit's precision compares.
    fn assert_close(expected: f64, actual: f64, digits: i32) {
        let scale = 10f64.powi(digits);
        assert_eq!((expected * scale).round(), (actual * scale).round(), "{expected} vs {actual}");
    }

    fn judged(key: &str, v: f64) -> (String, Reading) {
        (key.to_string(), Reading::Judged(v))
    }

    // --- when --------------------------------------------------------------------------------

    #[test]
    fn how_often_a_monitor_runs() {
        for (query, minutes) in [
            ("avg(last_5m):avg:cpu{*} > 1", 1),
            ("avg(last_23h):avg:cpu{*} > 1", 1),
            ("avg(last_1d):avg:cpu{*} > 1", 10),
            ("avg(last_2d):avg:cpu{*} > 1", 10),
            ("avg(last_3d):avg:cpu{*} > 1", 30),
            ("change(avg(last_2d),1d_ago):avg:cpu{*} > 1", 10),
            ("avg(last_1w):anomalies(avg:cpu{*}, 'basic', 2, alert_window='last_1h') >= 1", 1),
        ] {
            assert_eq!(frequency(&query_of(query)), TimeDelta::minutes(minutes), "{query}");
        }
    }

    #[test]
    fn ticks_sit_on_the_wall_clock_each_monitor_offset_within_20_s() {
        let offsets: Vec<TimeDelta> = (1..=50).map(|i| offset(&format!("default/{i}"))).collect();
        assert!(offsets.iter().all(|o| *o >= TimeDelta::zero() && *o < TimeDelta::seconds(20)));
        let distinct: HashSet<TimeDelta> = offsets.iter().copied().collect();
        assert!(distinct.len() > 40, "offsets spread");
        assert_eq!(offset("default/7"), offset("default/7"));

        let minute = TimeDelta::minutes(1);
        for now in [at(), at() + 5 * SECOND, at() + 59_900, at() + 450 * SECOND] {
            let (tick, fires) = next_tick("default/7", minute, None, now);
            assert_eq!(tick % MINUTE, 0);
            assert_eq!(fires, tick + offset("default/7").num_milliseconds());
            assert!(fires > now && fires <= now + MINUTE, "{now}: fires at {fires}");
        }

        let (ten_minutes, _) = next_tick("default/1", TimeDelta::minutes(10), None, at() + 3 * MINUTE);
        assert_eq!(ten_minutes, at() + 10 * MINUTE);
    }

    /// The offsets the F# service gave these monitors (`Evaluate.offset`), so a takeover keeps
    /// each monitor's place in the minute.
    #[test]
    fn offsets_are_the_f_sharp_services() {
        for (monitor, ms) in
            [("default/7", 4551), ("default/1", 13234), ("acme/0f3c9a2e-8d4b-4e1a-9c7d-2b5e6f8a1c3d", 13858)]
        {
            assert_eq!(offset(monitor), TimeDelta::milliseconds(ms), "{monitor}");
        }
    }

    #[test]
    fn a_tick_re_armed_before_the_last_one_fired_is_still_the_next_one() {
        let minute = TimeDelta::minutes(1);
        let (tick, fires) = next_tick("default/7", minute, None, at());
        // The timer fired early, or the clock stepped back.
        for now in [fires - 1, fires - 3 * MINUTE] {
            let (next, _) = next_tick("default/7", minute, Some(tick), now);
            assert_eq!(next, tick + MINUTE);
        }
    }

    // --- the window --------------------------------------------------------------------------

    #[test]
    fn the_window_ends_30_s_before_the_tick_on_the_step() {
        let (p, step) = period(DEFAULT_RETENTION, &definition("avg(last_5m):avg:cpu{*} > 1", ""), at() + 7 * SECOND);
        assert_eq!(step, TimeDelta::seconds(15));
        assert_eq!(p, Period { from: window_from(), to: window_to() });
    }

    #[test]
    fn evaluation_delay_moves_the_window_back() {
        let (p, _) =
            period(DEFAULT_RETENTION, &definition("avg(last_5m):avg:cpu{*} > 1", r#"{"evaluation_delay":120}"#), at());
        assert_eq!(p.to, at() - 2 * MINUTE);
        assert_eq!(p.from, at() - 7 * MINUTE);
    }

    #[test]
    fn the_step() {
        for (query, seconds) in [
            ("avg(last_5m):avg:cpu{*} > 1", 15),
            ("avg(last_1d):avg:cpu{*} > 1", 58),
            ("avg(last_5m):avg:cpu{*}.rollup(max, 120) > 1", 120),
            ("avg(last_1mo):avg:cpu{*} > 1", 1800),
        ] {
            let (p, step) = period(DEFAULT_RETENTION, &definition(query, ""), at());
            assert_eq!(step, TimeDelta::seconds(seconds), "{query}");
            assert_eq!(p.to % step.num_milliseconds(), 0, "{query}");
        }
    }

    #[test]
    fn near_the_raw_retention_the_window_runs_on_the_step_the_engine_runs() {
        // diff reads a bucket before the window, which is past a one-day raw retention: the
        // engine reads the rollup at whole minutes.
        let retention = Retention { raw: TimeDelta::days(1), ..DEFAULT_RETENTION };
        let (p, step) = period(retention, &definition("avg(last_1437m):diff(avg:cpu{*}) > 1", ""), at());
        assert_eq!(step, TimeDelta::minutes(1));
        assert_eq!(p.to % MINUTE, 0);
    }

    #[test]
    fn past_the_raw_retention_the_step_is_whole_minutes() {
        // 31 days at 1500 points would be 1786 s; only the rollup holds that far back.
        let (p, step) = period(DEFAULT_RETENTION, &definition("avg(last_1mo):avg:cpu{*} > 1", ""), at());
        assert_eq!(step, TimeDelta::minutes(30));
        assert_eq!(p.to - p.from, TimeDelta::days(31).num_milliseconds());
    }

    // --- threshold, per bucket then the window -----------------------------------------------

    #[test]
    fn the_time_aggregator_reduces_each_groups_window() {
        let mut points = series("cpu", &[Some("a")], window_to(), &[10.0, 20.0, 30.0, 40.0]);
        points.extend(series("cpu", &[Some("b")], window_to(), &[1.0, 2.0, 3.0]));

        for (agg, a, b) in [("avg", 25.0, 2.0), ("max", 40.0, 3.0), ("min", 10.0, 1.0), ("sum", 100.0, 6.0)] {
            let e = evaluated(&points, &definition(&format!("{agg}(last_5m):avg:cpu{{*}} by {{host}} > 90"), ""));
            assert_eq!(readings(&e), vec![judged("host:a", a), judged("host:b", b)], "{agg}");
        }
    }

    #[test]
    fn a_groups_key_is_its_sorted_tags_n_a_groups_are_dropped() {
        let mut points = constant("cpu", &[Some("web"), Some("a")], 1.0);
        points.extend(constant("cpu", &[None, Some("b")], 2.0));

        let e = evaluated(&points, &definition("avg(last_5m):avg:cpu{*} by {role,host} > 90", ""));
        assert_eq!(e.groups.len(), 1);
        assert_eq!(e.groups[0].key, "host:a,role:web");
        assert_eq!(e.groups[0].tags, ["host:a", "role:web"]);
    }

    #[test]
    fn a_simple_alert_is_one_group_star() {
        let e = evaluated(&constant("cpu", &[], 5.0), &definition("avg(last_5m):avg:cpu{*} > 90", ""));
        assert_eq!(readings(&e), vec![judged("*", 5.0)]);
        assert!(e.groups[0].tags.is_empty());
    }

    #[test]
    fn no_series_no_groups_which_is_no_data() {
        let e = evaluated(&[], &definition("avg(last_5m):avg:cpu{*} by {host} > 90", ""));
        assert!(e.groups.is_empty());
        assert_eq!(e.period, Period { from: window_from(), to: window_to() });
    }

    #[test]
    fn the_groups_last_point_is_kept() {
        let points = series("cpu", &[Some("a")], window_to() - 2 * MINUTE, &[1.0, 2.0]);
        let e = evaluated(&points, &definition("avg(last_5m):avg:cpu{*} by {host} > 90", ""));
        assert_eq!(e.groups[0].last_point, Some(window_to() - 2 * MINUTE - 15 * SECOND));
    }

    #[test]
    fn more_groups_than_allowed_is_an_error_not_a_truncation() {
        let mut points = constant("cpu", &[Some("a")], 1.0);
        points.extend(constant("cpu", &[Some("b")], 1.0));

        match evaluate_with(1, &points, &definition("avg(last_5m):avg:cpu{*} by {host} > 90", "")) {
            Err(e) => assert!(e.starts_with("too_many_groups"), "{e}"),
            Ok(e) => panic!("expected an error, got {e:?}"),
        }
    }

    fn errors_and_hits() -> Vec<MetricPoint> {
        let mut points = series("errors", &[], window_to() - 15 * SECOND, &[5.0, 9.0]);
        points.extend(series("hits", &[], window_to() - 15 * SECOND, &[10.0, 90.0]));
        points
    }

    #[test]
    fn without_as_count_the_formula_runs_per_bucket_then_the_window() {
        // 5/10 and 9/90.
        let avg = evaluated(&errors_and_hits(), &definition("avg(last_5m):sum:errors{*} / sum:hits{*} > 1", ""));
        assert_close(0.3, value(&avg, "*"), 9);
        let sum = evaluated(&errors_and_hits(), &definition("sum(last_5m):sum:errors{*} / sum:hits{*} > 1", ""));
        assert_close(0.6, value(&sum, "*"), 9);
    }

    #[test]
    fn with_as_count_each_query_is_reduced_first_the_formula_after() {
        let e = evaluated(
            &errors_and_hits(),
            &definition("sum(last_5m):sum:errors{*}.as_count() / sum:hits{*}.as_count() > 1", ""),
        );
        assert_close(0.14, value(&e, "*"), 9);
        assert_eq!(e.groups[0].last_point, Some(window_to() - 15 * SECOND));
    }

    #[test]
    fn a_group_whose_every_bucket_has_no_number_is_skipped_with_no_last_point() {
        // Division by zero leaves a gap in every bucket.
        let mut points = constant("a", &[Some("x")], 1.0);
        points.extend(constant("b", &[Some("x")], 0.0));
        let e = evaluated(&points, &definition("avg(last_5m):avg:a{*} by {host} / avg:b{*} by {host} > 1", ""));
        assert!(skipped(&e, "host:x"));
        assert_eq!(e.groups[0].last_point, None);
    }

    #[test]
    fn a_series_kept_by_a_functions_lookback_with_no_point_in_the_window_has_no_last_point() {
        // ewma_10 reads ten buckets before the window; host:a stopped before it.
        let points = series("cpu", &[Some("a")], window_from() - 30 * SECOND, &[1.0, 2.0]);
        let e = evaluated(&points, &definition("avg(last_5m):ewma_10(avg:cpu{*} by {host}) > 1", ""));

        assert_eq!(e.groups.len(), 1);
        assert_eq!(e.groups[0].last_point, None);
        assert!(skipped(&e, &e.groups[0].key));
    }

    #[test]
    fn a_series_without_tags_is_no_group_of_a_multi_alert() {
        // default_zero over no series gives one series without tags.
        let e = evaluated(&[], &definition("avg(last_5m):default_zero(avg:cpu{*} by {host}) < 1", ""));
        assert!(e.groups.is_empty());
    }

    #[test]
    fn a_series_in_two_groups_is_judged_in_each() {
        // A host with two roles: the store gives one row per value.
        let mut points = constant("cpu", &[Some("web")], 5.0);
        points.extend(constant("cpu", &[Some("db")], 5.0));
        let e = evaluated(&points, &definition("avg(last_5m):avg:cpu{*} by {role} > 90", ""));
        let mut got = readings(&e);
        got.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(got, vec![judged("role:db", 5.0), judged("role:web", 5.0)]);
    }

    #[test]
    fn as_count_groups_sorted_keys_n_a_dropped() {
        let mut points = series("hits", &[Some("web"), Some("a")], window_to(), &[1.0, 2.0]);
        points.extend(series("hits", &[None, Some("b")], window_to(), &[5.0]));
        let e = evaluated(&points, &definition("sum(last_5m):sum:hits{*} by {role,host}.as_count() > 90", ""));
        assert_eq!(readings(&e), vec![judged("host:a,role:web", 3.0)]);
    }

    // --- require_full_window -----------------------------------------------------------------

    fn full_window() -> Definition {
        definition("avg(last_5m):avg:cpu{*} by {host} > 90", r#"{"require_full_window":true}"#)
    }

    /// Points in some of the window's five minutes.
    fn in_minutes(minutes: &[i64]) -> Vec<MetricPoint> {
        minutes
            .iter()
            .map(|m| MetricPoint {
                metric: "cpu",
                groups: vec![Some("a")],
                time: window_from() + m * MINUTE + 20 * SECOND,
                value: 1.0,
            })
            .collect()
    }

    #[test]
    fn a_full_window_data_in_the_first_bucket_at_most_3_empty() {
        for (minutes, judged) in [(&[0, 1, 2, 3, 4][..], true), (&[0, 4], true), (&[0], false), (&[1, 2, 3, 4], false)]
        {
            let e = evaluated(&in_minutes(minutes), &full_window());
            assert_eq!(!skipped(&e, "host:a"), judged, "{minutes:?}");
        }
    }

    #[test]
    fn without_require_full_window_a_gappy_window_is_judged() {
        let e = evaluated(&in_minutes(&[2]), &definition("avg(last_5m):avg:cpu{*} by {host} > 90", ""));
        assert!(!skipped(&e, "host:a"));
    }

    #[test]
    fn as_count_windows_have_no_gaps_to_see() {
        let points: Vec<MetricPoint> =
            in_minutes(&[2]).into_iter().map(|p| MetricPoint { metric: "hits", ..p }).collect();
        let e = evaluated(
            &points,
            &definition("sum(last_5m):sum:hits{*} by {host}.as_count() > 90", r#"{"require_full_window":true}"#),
        );
        assert_eq!(value(&e, "host:a"), 1.0);
    }

    // --- change ------------------------------------------------------------------------------

    fn shifted(values: &[f64], current: &[f64]) -> Vec<MetricPoint> {
        let mut points = series("cpu", &[Some("a")], window_to() - 5 * MINUTE, values);
        points.extend(series("cpu", &[Some("a")], window_to(), current));
        points
    }

    #[test]
    fn change_the_bucket_minus_the_one_a_timeshift_before_then_the_window() {
        let points = shifted(&[10.0; 20], &[12.0; 20]);
        let e = evaluated(&points, &definition("change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 1", ""));
        assert_close(2.0, value(&e, "host:a"), 9);
    }

    #[test]
    fn pct_change_buckets_with_no_number_are_left_out() {
        // The first shifted value is 0: that bucket's change has no number.
        let before: Vec<f64> = std::iter::once(0.0).chain(std::iter::repeat_n(10.0, 19)).collect();
        let now: Vec<f64> = std::iter::once(100.0).chain(std::iter::repeat_n(12.0, 19)).collect();
        let e = evaluated(
            &shifted(&before, &now),
            &definition("pct_change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 1", ""),
        );
        assert_close(20.0, value(&e, "host:a"), 9);
    }

    #[test]
    fn pct_change_from_zero_everywhere_is_skipped() {
        let e = evaluated(
            &shifted(&[0.0; 20], &[12.0; 20]),
            &definition("pct_change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 1", ""),
        );
        assert!(skipped(&e, "host:a"));
    }

    #[test]
    fn change_of_a_group_with_nothing_to_compare_with_is_skipped() {
        let points = series("cpu", &[Some("a")], window_to(), &[12.0; 20]);
        let e = evaluated(&points, &definition("change(avg(last_5m),last_5m):avg:cpu{*} by {host} > 1", ""));
        assert!(skipped(&e, "host:a"));
    }

    #[test]
    fn change_over_a_long_window_the_step_divides_the_shift_so_buckets_meet() {
        for (head, seconds) in [
            ("change(avg(last_12h),1h_ago)", 30),
            ("change(avg(last_1d),1d_ago)", 60),
            ("pct_change(avg(last_2d),1d_ago)", 120),
        ] {
            let d = definition(&format!("{head}:avg:cpu{{*}} by {{host}} > 1"), "");
            let (p, step) = period(DEFAULT_RETENTION, &d, at());
            assert_eq!(step, TimeDelta::seconds(seconds), "{head}");

            let shift = match &d.query.detection {
                Detection::Change(c) => c.shift.length().num_milliseconds(),
                other => panic!("{other:?}"),
            };

            // A value per minute that grows, so the change is the same in every bucket: by the
            // shift for a ramp, doubling over it for a percentage.
            let percent = head.starts_with("pct");
            let start = p.from - shift;
            let value_at = |t: i64| {
                let minutes = (t - start) as f64 / MINUTE as f64;
                if percent { 2f64.powf(minutes / (shift as f64 / MINUTE as f64)) } else { minutes }
            };

            let points: Vec<MetricPoint> = (0..(p.to - start) / MINUTE)
                .map(|i| {
                    let t = start + i * MINUTE;
                    MetricPoint { metric: "cpu", groups: vec![Some("a")], time: t, value: value_at(t) }
                })
                .collect();

            let e = evaluated(&points, &d);
            let expected = if percent { 100.0 } else { (shift / MINUTE) as f64 };
            assert_close(expected, value(&e, "host:a"), 6);
        }
    }

    #[test]
    fn a_shift_shorter_than_the_step_is_an_error_not_a_skip() {
        match evaluate_with(1000, &[], &definition("change(avg(last_2d),1m_ago):avg:cpu{*} > 1", "")) {
            Err(e) => assert!(e.contains("does not divide the shift"), "{e}"),
            Ok(e) => panic!("expected an error, got {e:?}"),
        }
    }

    #[test]
    fn change_with_as_count_the_two_windows_reduced_then_the_change() {
        let mut points = series("hits", &[], window_to() - 5 * MINUTE, &[2.0; 20]);
        points.extend(series("hits", &[], window_to(), &[3.0; 20]));

        let absolute = evaluated(&points, &definition("change(sum(last_5m),last_5m):sum:hits{*}.as_count() > 1", ""));
        assert_close(20.0, value(&absolute, "*"), 9);
        let percent =
            evaluated(&points, &definition("pct_change(sum(last_5m),last_5m):sum:hits{*}.as_count() > 1", ""));
        assert_close(50.0, value(&percent, "*"), 9);
    }

    // --- anomaly -----------------------------------------------------------------------------

    fn calm(i: usize) -> f64 {
        if i.is_multiple_of(2) { 10.0 } else { 12.0 }
    }

    /// An hour of 10s and 12s, then the 15 minutes of the alert window: one point a minute, None
    /// leaving that minute empty.
    fn anomaly_points(window: &[Option<f64>]) -> Vec<MetricPoint> {
        let values: Vec<Option<f64>> = (0..60).map(|i| Some(calm(i))).chain(window.iter().copied()).collect();
        let until = window_to() / MINUTE * MINUTE;
        let start = until - MINUTE * values.len() as i64;

        values
            .iter()
            .enumerate()
            .filter_map(|(i, v)| {
                v.map(|value| MetricPoint { metric: "cpu", groups: vec![], time: start + MINUTE * i as i64, value })
            })
            .collect()
    }

    fn calm_then(spikes: &[usize]) -> Vec<MetricPoint> {
        let window: Vec<Option<f64>> =
            (0..15).map(|i| Some(if spikes.contains(&i) { 100.0 } else { calm(i) })).collect();
        anomaly_points(&window)
    }

    fn basic(args: &str, options: &str) -> Definition {
        definition(
            &format!(
                "avg(last_4h):anomalies(avg:cpu{{*}}, 'basic', 2, alert_window='last_15m', interval=60{args}) >= 1"
            ),
            options,
        )
    }

    fn group(e: &Evaluation) -> &GroupResult {
        assert_eq!(e.groups.len(), 1, "{e:?}");
        &e.groups[0]
    }

    #[test]
    fn an_anomaly_is_the_fraction_of_the_alert_window_outside_the_band() {
        let spikes: Vec<usize> = (10..=14).collect();
        for (direction, outside) in
            [("", 5.0), (", direction='both'", 5.0), (", direction='above'", 5.0), (", direction='below'", 0.0)]
        {
            let e = evaluated(&calm_then(&spikes), &basic(direction, ""));
            assert_eq!(e.period.to - e.period.from, 15 * MINUTE);
            assert_close(outside / 15.0, value(&e, "*"), 9);
        }
    }

    #[test]
    fn an_anomaly_recovers_by_the_fraction_over_recovery_window() {
        let options = r#"{"threshold_windows":{"trigger_window":"last_15m","recovery_window":"last_5m"}}"#;
        let late = evaluated(&calm_then(&[10, 11, 12, 13, 14]), &basic("", options));
        assert_eq!(group(&late).recovery, Some(1.0));
        let early = evaluated(&calm_then(&[0, 1, 2, 3, 4]), &basic("", options));
        assert_eq!(group(&early).reading, Reading::Judged(5.0 / 15.0));
        assert_eq!(group(&early).recovery, Some(0.0));
    }

    #[test]
    fn count_default_zero_counts_an_empty_bucket_as_0() {
        let window: Vec<Option<f64>> = (0..15).map(|i| if i >= 10 { None } else { Some(calm(i)) }).collect();
        let points = anomaly_points(&window);
        assert_close(0.0, value(&evaluated(&points, &basic("", "")), "*"), 9);
        assert_close(5.0 / 15.0, value(&evaluated(&points, &basic(", count_default_zero='true'", "")), "*"), 9);
    }

    #[test]
    fn a_seasonal_model_without_enough_history_is_learning_not_ok() {
        let d = definition(
            "avg(last_4h):anomalies(avg:cpu{*}, 'agile', 2, alert_window='last_15m', interval=60, seasonality='hourly') >= 1",
            "",
        );
        let e = evaluated(&calm_then(&[]), &d);
        match &group(&e).reading {
            Reading::Skipped(_, reason) => assert!(reason.contains("learning"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn count_default_zero_fills_no_history_from_before_a_groups_first_point() {
        let d = definition(
            "avg(last_4h):anomalies(avg:cpu{*}, 'robust', 2, alert_window='last_15m', interval=60, seasonality='hourly', count_default_zero='true') >= 1",
            "",
        );
        let e = evaluated(&calm_then(&[]), &d);
        assert_eq!(
            group(&e).reading,
            Reading::Skipped(SkipCause::Learning, "learning: the model has too little history yet".into())
        );
    }

    /// Hourly-seasonal points every minute, from `hours` back to `until`, with a little noise so
    /// the models have a spread.
    fn seasonal_points(hours: i64, until: i64) -> Vec<MetricPoint> {
        (1..=hours * 60)
            .map(|m| {
                let t = until - m * MINUTE;
                let minute = DateTime::<Utc>::from_timestamp_millis(t).unwrap().minute();
                let value = 50.0
                    + 10.0 * (2.0 * std::f64::consts::PI * f64::from(minute) / 60.0).sin()
                    + 0.5 * (1.7 * m as f64).sin();
                MetricPoint { metric: "cpu", groups: vec![], time: t, value }
            })
            .collect()
    }

    #[test]
    fn a_seasonal_anomaly_judges_with_the_models_it_was_given() {
        for algorithm in ["robust", "agile"] {
            let d = definition(
                &format!(
                    "avg(last_4h):anomalies(avg:cpu{{*}}, '{algorithm}', 3, alert_window='last_15m', interval=60, seasonality='hourly') >= 1"
                ),
                "",
            );

            let calm = seasonal_points(8, at());
            let shifted: Vec<MetricPoint> = calm
                .iter()
                .map(|p| {
                    if p.time >= window_to() - 5 * MINUTE {
                        MetricPoint { value: p.value + 30.0, ..p.clone() }
                    } else {
                        p.clone()
                    }
                })
                .collect();

            let fitted = match block_on(fit(&execute_over(&calm), &tenant(), DEFAULT_RETENTION, &d, None, at())) {
                Ok(Some(f)) => f,
                other => panic!("{other:?}"),
            };
            assert!(!fitted.models[&0].is_empty(), "{algorithm}");

            let judged = |points: &[MetricPoint]| {
                let e = block_on(evaluate(
                    &execute_over(points),
                    &tenant(),
                    DEFAULT_RETENTION,
                    1000,
                    Some(&fitted),
                    &d,
                    at(),
                ))
                .unwrap_or_else(|e| panic!("{e}"));
                value(&e, "*")
            };

            let quiet = judged(&calm);
            assert!((0.0..=0.1).contains(&quiet), "{algorithm}: {quiet}");
            assert!(judged(&shifted) > quiet, "{algorithm}");
        }
    }

    #[test]
    fn models_are_fitted_again_once_they_are_stale_or_the_query_changed() {
        let reads = AtomicUsize::new(0);
        let execute = Fake(|sql: &Sql| {
            reads.fetch_add(1, Ordering::SeqCst);
            rows_over(&[], sql)
        });

        let hourly = "avg(last_4h):anomalies(avg:cpu{*}, 'robust', 3, alert_window='last_15m', interval=60, seasonality='hourly') >= 1";
        let d = definition(hourly, "");

        let fit_at = |previous: Option<&Fit>, tick: i64, d: &Definition| match block_on(fit(
            &execute,
            &tenant(),
            DEFAULT_RETENTION,
            d,
            previous,
            tick,
        )) {
            Ok(Some(f)) => f,
            other => panic!("{other:?}"),
        };

        let first = fit_at(None, at(), &d);
        let read = reads.load(Ordering::SeqCst);
        assert!(Arc::ptr_eq(&first.models, &fit_at(Some(&first), at() + 9 * MINUTE, &d).models));
        assert_eq!(reads.load(Ordering::SeqCst), read);
        assert_eq!(fit_at(Some(&first), at() + 10 * MINUTE, &d).at, at() + 10 * MINUTE);
        let other = definition(&hourly.replace("cpu", "mem"), "");
        assert_eq!(fit_at(Some(&first), at() + MINUTE, &other).at, at() + MINUTE);
        assert!(matches!(block_on(fit(&execute, &tenant(), DEFAULT_RETENTION, &basic("", ""), None, at())), Ok(None)));
    }

    // --- forecast ----------------------------------------------------------------------------

    /// A line through the last hour: `slope` a minute, `start` an hour ago.
    fn line(start: f64, slope: f64) -> Vec<MetricPoint> {
        (0..60)
            .map(|m| MetricPoint {
                metric: "cpu",
                groups: vec![],
                time: window_to() + (m - 60) * MINUTE,
                value: start + slope * m as f64,
            })
            .collect()
    }

    // A minute a minute: 60 by the window's end, a day of minutes more.
    #[test]
    fn a_forecast_is_the_highest_upper_bound_over_its_horizon_or_with_le_the_lowest_lower_bound() {
        for (query, slope, low, high) in [
            ("max(next_1d):forecast(avg:cpu{*}, 'linear', 1, history='1h') >= 2000", 1.0, 1450.0, 1550.0),
            ("min(next_1d):forecast(avg:cpu{*}, 'linear', 1, history='1h') <= -1000", -1.0, -1550.0, -1450.0),
        ] {
            let e = evaluated(&line(0.0, slope), &definition(query, ""));
            assert_eq!(e.period.to - e.period.from, HOUR);
            let v = value(&e, "*");
            assert!((low..=high).contains(&v), "{query}: {v}");
        }
    }

    #[test]
    fn a_forecast_without_the_history_to_fit_is_learning() {
        let d = definition("max(next_1d):forecast(avg:cpu{*}, 'linear', 1, history='1h') >= 90", "");
        let e = evaluated(&line(0.0, 1.0)[..1], &d);
        match &group(&e).reading {
            Reading::Skipped(_, reason) => assert!(reason.contains("learning"), "{reason}"),
            other => panic!("{other:?}"),
        }
    }

    // --- outlier -----------------------------------------------------------------------------

    fn hosts(levels: &[(&'static str, f64)]) -> Vec<MetricPoint> {
        levels.iter().flat_map(|(host, level)| constant("cpu", &[Some(host)], *level)).collect()
    }

    fn outlier_monitor() -> Definition {
        definition("avg(last_5m):outliers(avg:cpu{*} by {host}, 'dbscan', 3) > 0", "")
    }

    #[test]
    fn an_outlier_group_is_1_the_others_0() {
        let e = evaluated(&hosts(&[("a", 10.0), ("b", 10.5), ("c", 11.0), ("d", 50.0)]), &outlier_monitor());
        let mut got = readings(&e);
        got.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            got,
            vec![judged("host:a", 0.0), judged("host:b", 0.0), judged("host:c", 0.0), judged("host:d", 1.0)]
        );
    }

    #[test]
    fn fewer_than_3_groups_with_data_have_nothing_to_be_an_outlier_from() {
        let e = evaluated(&hosts(&[("a", 10.0), ("c", f64::NAN), ("d", 50.0)]), &outlier_monitor());
        assert_eq!(e.groups.len(), 3);
        let mut groups: Vec<&GroupResult> = e.groups.iter().collect();
        groups.sort_by(|a, b| a.key.cmp(&b.key));
        let few =
            Reading::Skipped(SkipCause::FewGroups, "an outlier needs at least 3 groups with data to compare".into());
        assert_eq!(
            groups.iter().map(|g| g.reading.clone()).collect::<Vec<_>>(),
            vec![
                few.clone(),
                Reading::Skipped(SkipCause::Incomplete, "no bucket in the window has a value".into()),
                few
            ]
        );
    }

    #[test]
    fn evaluate_and_fit_are_send() {
        fn send<T: Send>(_: T) {}
        let fake = Fake(|_: &Sql| vec![]);
        let d = basic("", "");
        send(evaluate(&fake, &tenant(), DEFAULT_RETENTION, 10, None, &d, at()));
        send(fit(&fake, &tenant(), DEFAULT_RETENTION, &d, None, at()));
    }
}
