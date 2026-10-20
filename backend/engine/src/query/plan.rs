//! A parsed request becomes a plan: defaults filled in, formula names resolved to queries, every
//! function's arguments checked. Anything the engine cannot do yet is refused here by name, so
//! later stages never have to check. (F#: `Query/Plan.fs`.)

use std::collections::{BTreeSet, HashMap};
use std::sync::LazyLock;

use chrono::{DateTime, LocalResult, Months, NaiveDateTime, TimeDelta, TimeZone};
use chrono_tz::{OffsetComponents, Tz};
use ninjapaws_anomaly::{Anomalies, Forecast, LinearModel, Outliers, Seasonality};
use regex::Regex;

use crate::api::v2::timeseries::{ParsedFormula, ParsedLimit, ParsedTimeseriesRequest, SortOrder};
use crate::duration;
use crate::metric_query::ast::{Arg, BinaryOp, Expr, Formula, Literal, MetricQuery, Modifier, TagFilter};
use crate::unix_time;

// --- types -----------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aggregation {
    Avg,
    Sum,
    Min,
    Max,
    /// Rollup only: `count:` as a prefix belongs to distributions.
    Count,
}

/// How one series' points in one bucket become one value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeAggregation {
    /// `.rollup(method)`, or avg when there is no modifier.
    ///
    /// WARNING(undocumented): whether Datadog applies `.as_count()` on its own to COUNT and RATE
    /// metrics. Its UI writes it into the query, so we only do what the query says.
    Plain(Aggregation),
    /// COUNT sums, RATE sums value × interval, GAUGE averages.
    AsCount,
    /// AsCount divided by the bucket width (for RATE, the width floored to the metric's own
    /// interval).
    AsRate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillMethod {
    Linear,
    Last,
    Zero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fill {
    NoFill,
    Within { method: FillMethod, limit_seconds: i32 },
}

#[derive(Clone, Debug, PartialEq)]
pub struct QueryPlan {
    pub metric: String,
    pub filter: TagFilter,
    pub group_by: Vec<String>,
    pub space_agg: Aggregation,
    pub time_agg: TimeAggregation,
    pub step: TimeDelta,
    /// Buckets read before `from`, for functions that look back.
    pub lookback: i32,
    /// Anything but NoFill makes ClickHouse return series one by one, for the engine to
    /// interpolate and combine.
    pub fill: Fill,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointwiseFn {
    Abs,
    Log2,
    Log10,
    Ceil,
    Floor,
    Round(i32),
    ClampMin(f64),
    ClampMax(f64),
    CutoffMin(f64),
    CutoffMax(f64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArithOp {
    Plus,
    Minus,
    Times,
    Divide,
    Minimum,
    Maximum,
    Power,
}

/// What `top` ranks series by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankBy {
    Max,
    Mean,
    Min,
    Sum,
    Last,
    L2norm,
    Area,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimewiseFn {
    Cumsum,
    /// FIXME(integral): implemented as the docs define it, which is probably a docs error; see
    /// `evaluate`.
    Integral,
    Diff,
    MonotonicDiff,
    Derivative,
    PerSecond,
    PerMinute,
    PerHour,
    Throughput,
    Ewma(i32),
    Median(i32),
    RollingAvg(i32),
    Autosmooth,
    TrendLine,
    RobustTrend,
    PiecewiseConstant,
}

/// A formula with its names resolved to queries.
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    /// The series one query returned; the index is into `Plan::queries`.
    Fetch(usize),
    Pointwise(PointwiseFn, Box<Node>),
    Top { inner: Box<Node>, count: usize, by: RankBy, order: SortOrder },
    CountNonzero(Box<Node>),
    CountNotNull(Box<Node>),
    ExcludeNull(Box<Node>),
    Timewise(TimewiseFn, Box<Node>),
    /// The inner node over data `offset_ms` away (negative: the past), drawn at the present
    /// window's times.
    Shift { offset_ms: i64, inner: Box<Node> },
    DefaultZero(Box<Node>),
    Outliers { inner: Box<Node>, algorithm: Outliers, tolerance: f64 },
    Constant(f64),
    /// `time()`: each point's own time, in seconds.
    TimeOfPoint,
    Arith(ArithOp, Box<Node>, Box<Node>),
}

/// How long the metrics tables keep their rows: `metrics` (NINJACAT_METRICS_RETENTION_DAYS), and
/// `metrics_1m` and `metric_tag_keys` (NINJACAT_METRICS_ROLLUP_RETENTION_DAYS). The migrating
/// process sets the same values as the tables' TTLs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retention {
    pub raw: TimeDelta,
    pub rollup: TimeDelta,
}

pub const DEFAULT_RETENTION: Retention = Retention { raw: TimeDelta::days(30), rollup: TimeDelta::days(400) };

/// The table a query reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Table {
    /// `metrics`: every point.
    Points,
    /// `metrics_1m`: one row per series per minute, holding counts, sums, minima, maxima and a
    /// rate's events.
    Rollup,
}

/// What a formula returns beside its series. Only a formula's outermost function can ask for one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Extra {
    None,
    Anomalies { algorithm: Anomalies, bounds: f64 },
    Forecast { deviations: f64, method: Forecast, history: Option<TimeDelta>, horizon: Option<TimeDelta> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Output {
    /// The formula's position, or the query's when there are no formulas.
    pub query_index: usize,
    pub node: Node,
    pub limit: Option<ParsedLimit>,
    pub extra: Extra,
}

/// Buckets before each point that the basic anomaly band is computed from.
///
/// WARNING(undocumented): the size of Datadog's "lagging rolling quantile" window. Ours is 60
/// buckets.
pub const ANOMALY_WINDOW: usize = ninjapaws_anomaly::LAG;

/// Datadog's cap on points per series.
pub const MAX_POINTS: i64 = 1500;

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// Unix milliseconds.
    pub from: i64,
    pub to: i64,
    pub step: TimeDelta,
    pub queries: Vec<QueryPlan>,
    pub outputs: Vec<Output>,
    /// What the plan was made against; seasonal history is capped by it.
    pub retention: Retention,
}

// --- spans -----------------------------------------------------------------------------------

fn nanos(d: TimeDelta) -> i128 {
    d.num_seconds() as i128 * 1_000_000_000 + d.subsec_nanos() as i128
}

/// .NET's 100 ns ticks, which the F# divided and compared spans in.
fn ticks(d: TimeDelta) -> i128 {
    nanos(d) / 100
}

fn total_seconds(d: TimeDelta) -> f64 {
    ticks(d) as f64 / 10_000_000.0
}

fn seconds_span(seconds: f64) -> TimeDelta {
    TimeDelta::try_seconds(seconds as i64).unwrap_or(TimeDelta::MAX)
}

fn whole_minutes(step: TimeDelta) -> TimeDelta {
    let minutes = (ticks(step) as f64 / 600_000_000.0).ceil();
    TimeDelta::try_minutes(minutes as i64).unwrap_or(TimeDelta::MAX)
}

// --- what other modules read -------------------------------------------------------------------

/// Buckets of history a time-wise function needs before its first point.
///
/// WARNING(undocumented): how much history Datadog reads for these. Ours: one point for diffs, a
/// full window for median and rollingavg, 2 × span for ewma (the start's weight is then under
/// 2 %), none where the docs say the function starts at the visible window.
pub fn lookback(f: TimewiseFn) -> i32 {
    match f {
        TimewiseFn::Cumsum
        | TimewiseFn::Integral
        | TimewiseFn::Throughput
        | TimewiseFn::TrendLine
        | TimewiseFn::RobustTrend
        | TimewiseFn::PiecewiseConstant => 0,
        TimewiseFn::Diff
        | TimewiseFn::MonotonicDiff
        | TimewiseFn::Derivative
        | TimewiseFn::PerSecond
        | TimewiseFn::PerMinute
        | TimewiseFn::PerHour => 1,
        TimewiseFn::Median(span) | TimewiseFn::RollingAvg(span) => span - 1,
        TimewiseFn::Ewma(span) => 2 * span,
        TimewiseFn::Autosmooth => 2 * 20,
    }
}

/// A seasonal model's season as a span: its period at its step.
pub fn season_length(s: Seasonality) -> TimeDelta {
    TimeDelta::milliseconds(s.step_ms() * s.period() as i64)
}

/// A step of whole minutes reads the rollup: its per-minute rows add up into such a bucket
/// exactly, so the answer is the raw table's, from about a quarter of the rows.
pub fn table_for(step: TimeDelta) -> Table {
    if nanos(step) % 60_000_000_000 == 0 { Table::Rollup } else { Table::Points }
}

/// History a seasonal model reads back from `to`: six seasons, as Datadog's "up to six weeks".
/// Its steps are whole minutes, so it reads the rollup, and no more seasons than it keeps whole.
pub fn seasonal_history(retention: Retention, s: Seasonality) -> TimeDelta {
    let length = season_length(s);
    let whole_seasons_kept = nanos(retention.rollup) / nanos(length);
    length * whole_seasons_kept.min(6) as i32
}

/// Datadog's default steps for a line graph: (window up to, step).
///
/// WARNING(undocumented): a window between two rows takes the coarser row.
const DEFAULT_STEPS: [(TimeDelta, TimeDelta); 10] = [
    (TimeDelta::minutes(1), TimeDelta::seconds(1)),
    (TimeDelta::minutes(5), TimeDelta::seconds(2)),
    (TimeDelta::minutes(15), TimeDelta::seconds(5)),
    (TimeDelta::minutes(30), TimeDelta::seconds(10)),
    (TimeDelta::hours(1), TimeDelta::seconds(20)),
    (TimeDelta::hours(4), TimeDelta::minutes(1)),
    (TimeDelta::days(1), TimeDelta::minutes(5)),
    (TimeDelta::days(7), TimeDelta::hours(1)),
    (TimeDelta::days(31), TimeDelta::hours(4)),
    (TimeDelta::days(366), TimeDelta::days(1)),
];

/// A requested step, widened when the window would hold more than `MAX_POINTS`.
///
/// WARNING(undocumented): how much Datadog widens by. We take the smallest whole second that fits;
/// buckets are whole seconds in ClickHouse.
fn fitted(window: TimeDelta, requested: TimeDelta) -> TimeDelta {
    let smallest_allowed = (ticks(window) / MAX_POINTS as i128) as f64 / 10_000_000.0;
    let seconds = total_seconds(requested).max(smallest_allowed);
    seconds_span(seconds.max(1.0).ceil())
}

pub fn step(window: TimeDelta, hint: Option<TimeDelta>) -> TimeDelta {
    match hint {
        Some(requested) => fitted(window, requested),
        None => match DEFAULT_STEPS.iter().find(|(up_to, _)| window <= *up_to) {
            Some((_, step)) => *step,
            None => TimeDelta::days(7),
        },
    }
}

/// The step of a request at `now` (unix milliseconds): Datadog's for its window, in whole minutes
/// when the window starts before the oldest raw point kept, as only the rollup still holds it.
pub fn request_step(retention: Retention, now: i64, from: i64, until: i64, hint: Option<TimeDelta>) -> TimeDelta {
    let s = step(TimeDelta::milliseconds(until.saturating_sub(from)), hint);
    if from < now.saturating_sub(retention.raw.num_milliseconds()) { whole_minutes(s) } else { s }
}

/// A word argument, quoted or not.
pub fn word(literal: &Literal) -> Option<&str> {
    match literal {
        Literal::Word(w) | Literal::Quoted(w) => Some(w),
        Literal::Num(_) => None,
    }
}

fn errors_in<T>(results: &[Result<T, Vec<String>>]) -> Vec<String> {
    results.iter().filter_map(|r| r.as_ref().err()).flatten().cloned().collect()
}

// --- queries ---------------------------------------------------------------------------------

fn space_aggregation(name: &str) -> Option<Aggregation> {
    match name {
        "avg" => Some(Aggregation::Avg),
        "sum" => Some(Aggregation::Sum),
        "min" => Some(Aggregation::Min),
        "max" => Some(Aggregation::Max),
        _ => None,
    }
}

fn rollup_method(name: &str) -> Option<Aggregation> {
    match name {
        "count" => Some(Aggregation::Count),
        _ => space_aggregation(name),
    }
}

fn aggregation_name(a: Aggregation) -> &'static str {
    match a {
        Aggregation::Avg => "avg",
        Aggregation::Sum => "sum",
        Aggregation::Min => "min",
        Aggregation::Max => "max",
        Aggregation::Count => "count",
    }
}

/// What a query's modifiers asked for, before they are checked together.
#[derive(Clone, Default)]
struct Modifiers {
    method: Option<Aggregation>,
    interval: Option<TimeDelta>,
    /// "as_count" or "as_rate".
    mode: Option<String>,
    fill: Option<Fill>,
}

fn read_rollup(sofar: Modifiers, args: &[Literal]) -> Result<Modifiers, String> {
    let Some((method, rest)) = args.split_first() else {
        return Err(".rollup() needs a method, e.g. .rollup(sum) or .rollup(sum, 60)".to_string());
    };
    let Some(method) = word(method).and_then(rollup_method) else {
        return Err(".rollup() method must be one of avg, sum, min, max, count".to_string());
    };
    match rest {
        [] => Ok(Modifiers { method: Some(method), ..sofar }),
        [Literal::Num(seconds)] if *seconds >= 1.0 => Ok(Modifiers {
            method: Some(method),
            interval: Some(unix_time::span_of_millis(seconds * 1000.0)),
            ..sofar
        }),
        [Literal::Num(_)] => Err(".rollup() interval must be at least 1 second".to_string()),
        _ => Err("calendar .rollup() (daily, weekly, monthly, alignment, timezone) is not supported yet".to_string()),
    }
}

fn read_fill(sofar: Modifiers, args: &[Literal]) -> Result<Modifiers, String> {
    let Some((method, rest)) = args.split_first() else {
        return Err(".fill() needs a method, e.g. .fill(zero) or .fill(linear, 300)".to_string());
    };
    let limit = match rest {
        [] => Ok(300),
        [Literal::Num(seconds)] if *seconds >= 1.0 && *seconds == seconds.floor() => Ok(*seconds as i32),
        _ => Err(".fill() limit must be a whole number of seconds, e.g. .fill(linear, 300)".to_string()),
    };
    let within = |method, limit_seconds| Ok(Modifiers { fill: Some(Fill::Within { method, limit_seconds }), ..sofar.clone() });
    match (word(method), limit) {
        (Some("null"), _) => Ok(Modifiers { fill: Some(Fill::NoFill), ..sofar.clone() }),
        (Some("linear"), Ok(limit)) => within(FillMethod::Linear, limit),
        (Some("last"), Ok(limit)) => within(FillMethod::Last, limit),
        (Some("zero"), Ok(limit)) => within(FillMethod::Zero, limit),
        (Some("linear" | "last" | "zero"), Err(e)) => Err(e),
        _ => Err(".fill() method must be one of null, zero, last, linear".to_string()),
    }
}

fn read_modifier(sofar: Modifiers, m: &Modifier) -> Result<Modifiers, String> {
    match m.name.as_str() {
        "rollup" => read_rollup(sofar, &m.args),
        "fill" => read_fill(sofar, &m.args),
        "as_count" | "as_rate" => match (m.args.as_slice(), &sofar.mode) {
            ([_, ..], _) => Err(format!(".{}() takes no arguments", m.name)),
            ([], Some(other)) if *other != m.name => Err(format!(".{}() cannot follow .{other}()", m.name)),
            ([], _) => Ok(Modifiers { mode: Some(m.name.clone()), ..sofar }),
        },
        name => Err(format!(".{name}() is not supported yet")),
    }
}

fn read_modifiers(label: &str, modifiers: &[Modifier]) -> (Modifiers, Vec<String>) {
    let mut read = Modifiers::default();
    let mut errors = Vec::new();
    for m in modifiers {
        match read_modifier(read.clone(), m) {
            Ok(updated) => read = updated,
            Err(e) => errors.push(format!("{label}: {e}")),
        }
    }
    (read, errors)
}

/// The interval a query's `.rollup()` asks for, read by the same rules as the plan's.
pub fn rollup_interval(q: &MetricQuery) -> Option<TimeDelta> {
    read_modifiers("", &q.modifiers).0.interval
}

fn time_aggregation(label: &str, m: &Modifiers) -> Result<TimeAggregation, String> {
    match (m.mode.as_deref(), m.method) {
        (None, None) => Ok(TimeAggregation::Plain(Aggregation::Avg)),
        (None, Some(method)) => Ok(TimeAggregation::Plain(method)),
        // as_count and as_rate sum within the bucket; a rollup other than sum contradicts them.
        (Some("as_count"), None | Some(Aggregation::Sum)) => Ok(TimeAggregation::AsCount),
        (Some("as_rate"), None | Some(Aggregation::Sum)) => Ok(TimeAggregation::AsRate),
        (Some(mode), Some(other)) => {
            Err(format!("{label}: .{mode}() with .rollup({}) is not supported yet", aggregation_name(other)))
        }
        (Some(mode), None) => Err(format!("{label}: unknown modifier .{mode}()")),
    }
}

/// `{$env}` left in a filter: a dashboard should have substituted it.
fn template_variables(filter: &TagFilter) -> Vec<String> {
    match filter {
        TagFilter::Bare(v) | TagFilter::Tag(_, v) if v.starts_with('$') => vec![v.clone()],
        TagFilter::Not(inner) => template_variables(inner),
        TagFilter::And(filters) | TagFilter::Or(filters) => filters.iter().flat_map(template_variables).collect(),
        _ => Vec::new(),
    }
}

fn plan_query(window: TimeDelta, request_step: TimeDelta, label: &str, q: &MetricQuery) -> Result<QueryPlan, Vec<String>> {
    let (modifiers, modifier_errors) = read_modifiers(label, &q.modifiers);
    let time_agg = time_aggregation(label, &modifiers);

    let mut errors = Vec::new();
    if let Some(a) = &q.space_agg
        && space_aggregation(a).is_none()
    {
        errors.push(format!("{label}: aggregator '{a}:' is not supported yet"));
    }
    errors.extend(modifier_errors);
    if let Err(e) = &time_agg {
        errors.push(e.clone());
    }
    errors.extend(
        template_variables(&q.filter)
            .into_iter()
            .map(|v| format!("{label}: template variable '{v}' was not substituted")),
    );

    match (errors.is_empty(), time_agg) {
        (true, Ok(time_agg)) => {
            // WARNING(undocumented): with no prefix, Datadog picks the space aggregator by metric
            // type. We use avg, its choice for a gauge.
            let space_agg = q.space_agg.as_deref().and_then(space_aggregation).unwrap_or(Aggregation::Avg);

            let step = match modifiers.interval {
                Some(interval) => fitted(window, interval),
                None => request_step,
            };

            // Linear up to 5 minutes by default; as_count/as_rate turn it off.
            // WARNING(undocumented): the docs keep it on for gauges there. The metric type is
            // known only per row, so we turn it off for all.
            let fill = match (modifiers.fill, time_agg) {
                (Some(fill), _) => fill,
                (None, TimeAggregation::AsCount | TimeAggregation::AsRate) => Fill::NoFill,
                (None, TimeAggregation::Plain(_)) => Fill::Within { method: FillMethod::Linear, limit_seconds: 300 },
            };

            Ok(QueryPlan {
                metric: q.metric.clone(),
                filter: q.filter.clone(),
                group_by: q.group_by.clone(),
                space_agg,
                time_agg,
                step,
                lookback: 0,
                fill,
            })
        }
        _ => Err(errors),
    }
}

// --- function arguments ----------------------------------------------------------------------

fn pointwise_function(name: &str, args: &[Literal]) -> Result<PointwiseFn, String> {
    let numbers: Vec<f64> = args
        .iter()
        .filter_map(|a| match a {
            Literal::Num(n) => Some(*n),
            _ => None,
        })
        .collect();

    if numbers.len() != args.len() {
        return Err(format!("{name}() takes only numbers after the query"));
    }
    match (name, numbers.as_slice()) {
        ("abs", []) => Ok(PointwiseFn::Abs),
        ("log2", []) => Ok(PointwiseFn::Log2),
        ("log10", []) => Ok(PointwiseFn::Log10),
        ("ceil", []) => Ok(PointwiseFn::Ceil),
        ("floor", []) => Ok(PointwiseFn::Floor),
        ("abs" | "log2" | "log10" | "ceil" | "floor", _) => Err(format!("{name}() takes one argument: {name}(query)")),
        ("round", []) => Ok(PointwiseFn::Round(0)),
        ("round", [d]) if *d >= 0.0 && *d <= 15.0 && *d == d.floor() => Ok(PointwiseFn::Round(*d as i32)),
        ("round", _) => {
            Err("round() takes a query and optionally a whole number of decimals from 0 to 15: round(query, 2)".to_string())
        }
        ("clamp_min", [t]) => Ok(PointwiseFn::ClampMin(*t)),
        ("clamp_max", [t]) => Ok(PointwiseFn::ClampMax(*t)),
        ("cutoff_min", [t]) => Ok(PointwiseFn::CutoffMin(*t)),
        ("cutoff_max", [t]) => Ok(PointwiseFn::CutoffMax(*t)),
        _ => Err(format!("{name}() takes a query and a number: {name}(query, 100)")),
    }
}

/// Only the numbered forms the docs list; `ewma` alone is ewma_20 and `median` alone is median_3,
/// as the docs say.
fn timewise_function(name: &str) -> Option<TimewiseFn> {
    match name {
        "cumsum" => Some(TimewiseFn::Cumsum),
        "integral" => Some(TimewiseFn::Integral),
        "diff" => Some(TimewiseFn::Diff),
        "monotonic_diff" => Some(TimewiseFn::MonotonicDiff),
        "derivative" => Some(TimewiseFn::Derivative),
        "per_second" => Some(TimewiseFn::PerSecond),
        "per_minute" => Some(TimewiseFn::PerMinute),
        "per_hour" => Some(TimewiseFn::PerHour),
        "throughput" => Some(TimewiseFn::Throughput),
        "ewma" => Some(TimewiseFn::Ewma(20)),
        "ewma_1" => Some(TimewiseFn::Ewma(1)),
        "ewma_3" => Some(TimewiseFn::Ewma(3)),
        "ewma_5" => Some(TimewiseFn::Ewma(5)),
        "ewma_7" => Some(TimewiseFn::Ewma(7)),
        "ewma_10" => Some(TimewiseFn::Ewma(10)),
        "ewma_20" => Some(TimewiseFn::Ewma(20)),
        "median" => Some(TimewiseFn::Median(3)),
        "median_3" => Some(TimewiseFn::Median(3)),
        "median_5" => Some(TimewiseFn::Median(5)),
        "median_7" => Some(TimewiseFn::Median(7)),
        "median_9" => Some(TimewiseFn::Median(9)),
        "rollingavg_5" => Some(TimewiseFn::RollingAvg(5)),
        "rollingavg_13" => Some(TimewiseFn::RollingAvg(13)),
        "rollingavg_21" => Some(TimewiseFn::RollingAvg(21)),
        "rollingavg_29" => Some(TimewiseFn::RollingAvg(29)),
        "autosmooth" => Some(TimewiseFn::Autosmooth),
        "trend_line" => Some(TimewiseFn::TrendLine),
        "robust_trend" => Some(TimewiseFn::RobustTrend),
        "piecewise_constant" => Some(TimewiseFn::PiecewiseConstant),
        _ => None,
    }
}

fn rank_by(name: &str) -> Option<RankBy> {
    match name {
        "max" => Some(RankBy::Max),
        "mean" => Some(RankBy::Mean),
        "min" => Some(RankBy::Min),
        "sum" => Some(RankBy::Sum),
        "last" => Some(RankBy::Last),
        "l2norm" => Some(RankBy::L2norm),
        "area" => Some(RankBy::Area),
        _ => None,
    }
}

/// `top(q, 10, 'mean', 'desc')`.
///
/// WARNING(undocumented): top()'s defaults. We require all four arguments, as Datadog's editor
/// always writes them.
struct Ranking {
    limit: usize,
    by: RankBy,
    order: SortOrder,
}

fn top_args(args: &[Literal]) -> Result<Ranking, String> {
    match args {
        [Literal::Num(n), by, direction] => {
            let limit = *n as i32;
            if limit as f64 != *n || ![5, 10, 25, 50, 100].contains(&limit) {
                return Err("top() limit must be one of 5, 10, 25, 50, 100".to_string());
            }
            let limit = limit as usize;
            match (word(by).and_then(rank_by), word(direction)) {
                (None, _) => Err("top() ranks by one of 'max', 'mean', 'min', 'sum', 'last', 'l2norm', 'area'".to_string()),
                (Some(by), Some("desc")) => Ok(Ranking { limit, by, order: SortOrder::Desc }),
                (Some(by), Some("asc")) => Ok(Ranking { limit, by, order: SortOrder::Asc }),
                (Some(_), _) => Err("top() direction must be 'asc' or 'desc'".to_string()),
            }
        }
        _ => Err("top() takes a query, a limit, a ranking and a direction: top(query, 10, 'mean', 'desc')".to_string()),
    }
}

static TOP_SHORTHAND: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(top|bottom)(5|10|15|20)_(mean|min|max|last|area|l2norm)$").unwrap());

/// `top5_mean`, `bottom10_min`…: the documented
/// `[top|bottom][5|10|15|20]_[mean|min|max|last|area|l2norm]`.
///
/// WARNING(undocumented): bare `top10()`. Not accepted.
fn top_shorthand(name: &str) -> Option<Ranking> {
    let m = TOP_SHORTHAND.captures(name)?;
    let order = if &m[1] == "top" { SortOrder::Desc } else { SortOrder::Asc };
    let limit = m[2].parse().ok()?;
    let by = rank_by(&m[3])?;
    Some(Ranking { limit, by, order })
}

/// `outliers(q, 'dbscan', 3)` or `outliers(q, 'mad', 3, 20)`. Algorithm names ignore case:
/// Datadog's own examples write both 'DBSCAN' and 'dbscan'.
pub fn outlier_args(args: &[Literal]) -> Result<(Outliers, f64), String> {
    let usage = "outliers() takes a query, an algorithm, a tolerance and for MAD a percentage: outliers(query, 'dbscan', 3) or outliers(query, 'mad', 3, 20)";
    let algorithm = |literal: &Literal| word(literal).map(str::to_lowercase);

    match args {
        [alg, Literal::Num(tolerance)] if *tolerance > 0.0 => match algorithm(alg).as_deref() {
            Some("dbscan") => Ok((Outliers::Dbscan, *tolerance)),
            Some("scaledbscan") => Ok((Outliers::ScaledDbscan, *tolerance)),
            Some("mad" | "scaledmad") => {
                Err("outliers() with MAD needs a percentage: outliers(query, 'mad', 3, 20)".to_string())
            }
            _ => Err(usage.to_string()),
        },
        [alg, Literal::Num(tolerance), Literal::Num(percent)]
            if *tolerance > 0.0 && *percent >= 0.0 && *percent <= 100.0 =>
        {
            match algorithm(alg).as_deref() {
                Some("mad") => Ok((Outliers::Mad { percent: *percent }, *tolerance)),
                Some("scaledmad") => Ok((Outliers::ScaledMad { percent: *percent }, *tolerance)),
                _ => Err(usage.to_string()),
            }
        }
        _ => Err(usage.to_string()),
    }
}

/// .NET's `$` also matches before a final newline, so the F# accepted one.
static CALENDAR_SHIFT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-([0-9]{1,4})(d|w|mo)\n?$").unwrap());

/// `TimeZoneInfo.GetUtcOffset` of a wall-clock time: one that is ambiguous or skipped takes the
/// zone's standard offset.
fn utc_offset(zone: Tz, local: NaiveDateTime) -> TimeDelta {
    match zone.offset_from_local_datetime(&local) {
        LocalResult::Single(o) => o.base_utc_offset() + o.dst_offset(),
        LocalResult::Ambiguous(o, _) => o.base_utc_offset(),
        LocalResult::None => zone.offset_from_utc_datetime(&local).base_utc_offset(),
    }
}

/// `calendar_shift`'s "-1d", "-2w", "-1mo", as milliseconds from `from`. The same wall-clock time
/// n units back in that zone, so "-1d" across a DST change is 23 or 25 hours.
fn calendar_shift(spec: &str, timezone: &str, from: i64) -> Result<i64, String> {
    let zone = timezone
        .parse::<Tz>()
        .map_err(|_| format!("calendar_shift() timezone '{timezone}' is not a known IANA zone"));
    let Some(m) = CALENDAR_SHIFT.captures(spec) else {
        return Err(format!("calendar_shift() shift must look like \"-1d\", \"-2w\" or \"-1mo\", got \"{spec}\""));
    };
    let zone = zone?;
    let n: u32 = m[1].parse().unwrap_or_default();
    let local = DateTime::from_timestamp_millis(from).unwrap_or_default().with_timezone(&zone).naive_local();

    let shifted = match &m[2] {
        "d" => local - TimeDelta::days(n as i64),
        "w" => local - TimeDelta::days(7 * n as i64),
        _ => local.checked_sub_months(Months::new(n)).unwrap_or(local),
    };

    let back = (shifted - utc_offset(zone, shifted)).and_utc().timestamp_millis();
    Ok(back - from)
}

/// A shift function's offset in milliseconds.
///
/// WARNING(undocumented): a calendar shift is computed once, at `from`. Over a window crossing a
/// month end or a DST change, later points drift from their exact calendar counterparts.
fn shift_offset(from: i64, name: &str, args: &[Literal]) -> Result<i64, String> {
    match (name, args) {
        ("timeshift", [Literal::Num(seconds)]) if *seconds != 0.0 && *seconds == seconds.floor() => {
            Ok(unix_time::span_of_millis(seconds * 1000.0).num_milliseconds())
        }
        ("timeshift", _) => {
            Err("timeshift() takes a query and a whole, non-zero number of seconds: timeshift(query, -3600)".to_string())
        }
        ("hour_before", []) => Ok(-3_600_000),
        ("day_before", []) => Ok(-86_400_000),
        ("week_before", []) => Ok(-604_800_000),
        // Datadog deprecated month_before in favour of calendar_shift "-1mo".
        ("month_before", []) => calendar_shift("-1mo", "UTC", from),
        ("calendar_shift", [Literal::Quoted(spec) | Literal::Word(spec)]) => calendar_shift(spec, "UTC", from),
        (
            "calendar_shift",
            [Literal::Quoted(spec) | Literal::Word(spec), Literal::Quoted(zone) | Literal::Word(zone)],
        ) => calendar_shift(spec, zone, from),
        ("calendar_shift", _) => Err(
            "calendar_shift() takes a query, a shift and optionally a timezone: calendar_shift(query, \"-1w\", \"Europe/Paris\")"
                .to_string(),
        ),
        _ => Err(format!("{name}() takes one argument: {name}(query)")),
    }
}

// --- formulas --------------------------------------------------------------------------------

/// An argument after the query, as a plain literal. The parser reads a bare word in a formula
/// (`mean`) as a query name; here it is a word again.
fn as_literal(arg: &Arg<String>) -> Option<Literal> {
    match arg {
        Arg::Value(Expr::Number(n)) => Some(Literal::Num(*n)),
        Arg::Value(Expr::Leaf(w)) => Some(Literal::Word(w.clone())),
        Arg::Lit(l) => Some(l.clone()),
        Arg::Value(_) | Arg::Named(..) => None,
    }
}

/// A function applied to its already resolved first argument.
fn apply_function(from: i64, name: &str, args: &[Literal], inner: Node) -> Result<Node, String> {
    let inner = Box::new(inner);
    let one_argument = || Err(format!("{name}() takes one argument: {name}(query)"));

    match name {
        "abs" | "log2" | "log10" | "ceil" | "floor" | "round" | "clamp_min" | "clamp_max" | "cutoff_min"
        | "cutoff_max" => pointwise_function(name, args).map(|f| Node::Pointwise(f, inner)),
        "top" => top_args(args).map(|r| Node::Top { inner, count: r.limit, by: r.by, order: r.order }),
        "outliers" => outlier_args(args).map(|(algorithm, tolerance)| Node::Outliers { inner, algorithm, tolerance }),
        "timeshift" | "hour_before" | "day_before" | "week_before" | "month_before" | "calendar_shift" => {
            shift_offset(from, name, args).map(|offset_ms| Node::Shift { offset_ms, inner })
        }
        "count_nonzero" | "count_nonzero_finite" if args.is_empty() => Ok(Node::CountNonzero(inner)),
        "count_not_null" if args.is_empty() => Ok(Node::CountNotNull(inner)),
        "exclude_null" if args.is_empty() => Ok(Node::ExcludeNull(inner)),
        "default_zero" if args.is_empty() => Ok(Node::DefaultZero(inner)),
        "count_nonzero" | "count_nonzero_finite" | "count_not_null" | "exclude_null" | "default_zero" => one_argument(),
        "anomalies" | "forecast" => Err(format!("{name}() must be the outermost function of a formula")),
        _ => match (timewise_function(name), top_shorthand(name)) {
            (Some(f), _) if args.is_empty() => Ok(Node::Timewise(f, inner)),
            (None, Some(r)) if args.is_empty() => Ok(Node::Top { inner, count: r.limit, by: r.by, order: r.order }),
            (Some(_), _) | (None, Some(_)) => one_argument(),
            (None, None) => Err(format!("function '{name}' is not supported yet")),
        },
    }
}

fn arith(op: ArithOp, left: Result<Node, Vec<String>>, right: Result<Node, Vec<String>>) -> Result<Node, Vec<String>> {
    match (left, right) {
        (Ok(l), Ok(r)) => Ok(Node::Arith(op, Box::new(l), Box::new(r))),
        (Err(mut l), Err(r)) => {
            l.extend(r);
            Err(l)
        }
        (Err(e), Ok(_)) | (Ok(_), Err(e)) => Err(e),
    }
}

struct Resolver<'a> {
    from: i64,
    label: &'a str,
    by_name: &'a HashMap<String, usize>,
}

impl Resolver<'_> {
    fn resolve(&self, expr: &Formula) -> Result<Node, Vec<String>> {
        let label = self.label;
        match expr {
            Expr::Leaf(name) => match self.by_name.get(name) {
                Some(&index) => Ok(Node::Fetch(index)),
                None => Err(vec![format!("{label}: no query is named '{name}'")]),
            },
            Expr::Number(n) => Ok(Node::Constant(*n)),
            Expr::Neg(inner) => arith(ArithOp::Times, Ok(Node::Constant(-1.0)), self.resolve(inner)),
            Expr::Binary(op, l, r) => {
                let op = match op {
                    BinaryOp::Add => ArithOp::Plus,
                    BinaryOp::Sub => ArithOp::Minus,
                    BinaryOp::Mul => ArithOp::Times,
                    BinaryOp::Div => ArithOp::Divide,
                };
                arith(op, self.resolve(l), self.resolve(r))
            }
            Expr::Call(name, args) => self.resolve_call(name, args),
        }
    }

    fn resolve_call(&self, name: &str, args: &[Arg<String>]) -> Result<Node, Vec<String>> {
        let label = self.label;
        match (name, args) {
            ("minimum", [Arg::Value(l), Arg::Value(r)]) => arith(ArithOp::Minimum, self.resolve(l), self.resolve(r)),
            ("maximum", [Arg::Value(l), Arg::Value(r)]) => arith(ArithOp::Maximum, self.resolve(l), self.resolve(r)),
            ("pow", [Arg::Value(l), Arg::Value(r)]) => arith(ArithOp::Power, self.resolve(l), self.resolve(r)),
            ("minimum" | "maximum" | "pow", _) => Err(vec![format!("{label}: {name}() takes two operands: {name}(a, b)")]),
            ("time", []) => Ok(Node::TimeOfPoint),
            ("time", _) => Err(vec![format!("{label}: time() takes no arguments")]),
            (_, []) => Err(vec![format!("{label}: {name}() needs a query to act on")]),
            (_, [Arg::Value(inner), rest @ ..]) => {
                let Some(literals) = rest.iter().map(as_literal).collect::<Option<Vec<_>>>() else {
                    return Err(vec![format!("{label}: {name}() takes only numbers and words after the query")]);
                };
                let inner = self.resolve(inner)?;
                apply_function(self.from, name, &literals, inner).map_err(|e| vec![format!("{label}: {e}")])
            }
            _ => Err(vec![format!("{label}: the first argument of {name}() must be a query")]),
        }
    }

    /// A formula's outermost function may be anomalies() or forecast(); below it everything is an
    /// ordinary Node.
    fn resolve_output(&self, expr: &Formula) -> Result<(Node, Extra), Vec<String>> {
        let label = self.label;
        match expr {
            Expr::Call(name, args) if name == "anomalies" || name == "forecast" => {
                let [Arg::Value(inner), rest @ ..] = args.as_slice() else {
                    return Err(vec![format!("{label}: the first argument must be a query")]);
                };
                let extra = if name == "anomalies" {
                    anomalies_args(rest).map(|(algorithm, bounds)| Extra::Anomalies { algorithm, bounds })
                } else {
                    forecast_args(rest).map(|(deviations, method, history, horizon)| Extra::Forecast {
                        deviations,
                        method,
                        history,
                        horizon,
                    })
                };
                match (self.resolve(inner), extra) {
                    (Ok(node), Ok(extra)) => Ok((node, extra)),
                    (Err(errors), Ok(_)) => Err(errors),
                    (Ok(_), Err(e)) => Err(vec![format!("{label}: {e}")]),
                    (Err(mut errors), Err(e)) => {
                        errors.push(format!("{label}: {e}"));
                        Err(errors)
                    }
                }
            }
            _ => self.resolve(expr).map(|node| (node, Extra::None)),
        }
    }
}

// --- anomalies and forecast ------------------------------------------------------------------

/// `4h`, `30m`, `1w`: a duration that is more than nothing.
fn positive_duration(text: &str) -> Option<TimeDelta> {
    duration::parse(text).ok().filter(|span| *span > TimeDelta::zero())
}

pub fn named_arg<'a>(name: &str, named: &'a [(String, Literal)]) -> Option<&'a Literal> {
    named.iter().find(|(n, _)| n == name).map(|(_, v)| v)
}

/// Positional arguments after the query, and the `name=value` ones.
pub fn split_args(rest: &[Arg<String>]) -> (Vec<Option<Literal>>, Vec<(String, Literal)>) {
    let positional = rest.iter().filter(|arg| !matches!(arg, Arg::Named(..))).map(as_literal).collect();
    let named = rest
        .iter()
        .filter_map(|arg| match arg {
            Arg::Named(n, v) => Some((n.clone(), v.clone())),
            _ => None,
        })
        .collect();
    (positional, named)
}

/// F#'s `string` of a float: .NET's shortest round-trip form, with an exponent from 1E+15 and
/// below 1E-04.
fn dotnet_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let scientific = format!("{n:e}");
    let (mantissa, exponent) = scientific.split_once('e').unwrap_or((&scientific, "0"));
    let exponent: i32 = exponent.parse().unwrap_or_default();
    if (-4..15).contains(&exponent) {
        format!("{n}")
    } else {
        let sign = if exponent < 0 { '-' } else { '+' };
        format!("{mantissa}E{sign}{:02}", exponent.abs())
    }
}

/// "By default, the robust and agile algorithms use weekly seasonality."
///
/// WARNING(undocumented): forecast()'s default seasonality. Weekly, as for anomalies().
/// WARNING(undocumented): `timezone` is accepted and ignored; our seasons are fixed-length, so
/// across DST they are off by an hour.
fn seasonality(named: &[(String, Literal)]) -> Result<Seasonality, String> {
    let invalid = |got: &str| Err(format!("seasonality must be 'hourly', 'daily' or 'weekly', got '{got}'"));
    match named_arg("seasonality", named) {
        None => Ok(Seasonality::Weekly),
        Some(Literal::Num(n)) => invalid(&dotnet_number(*n)),
        Some(Literal::Word(s) | Literal::Quoted(s)) => match s.as_str() {
            "hourly" => Ok(Seasonality::Hourly),
            "daily" => Ok(Seasonality::Daily),
            "weekly" => Ok(Seasonality::Weekly),
            other => invalid(other),
        },
    }
}

/// anomalies(q, 'basic' | 'agile' | 'robust', bounds, name=value…)
///
/// WARNING(undocumented): `direction`, `alert_window`, `interval` and `count_default_zero` are for
/// monitors. Accepted here and ignored: a graph draws the whole band.
pub fn anomalies_args(rest: &[Arg<String>]) -> Result<(Anomalies, f64), String> {
    let allowed = ["direction", "alert_window", "interval", "count_default_zero", "seasonality", "timezone"];
    let (positional, named) = split_args(rest);

    if let Some((n, _)) = named.iter().find(|(n, _)| !allowed.contains(&n.as_str())) {
        return Err(format!("anomalies() has no argument '{n}'"));
    }
    match positional.as_slice() {
        [Some(alg), Some(Literal::Num(bounds))] if *bounds > 0.0 => match word(alg) {
            Some("basic") => Ok((Anomalies::Basic, *bounds)),
            Some("robust") => seasonality(&named).map(|s| (Anomalies::Robust(s), *bounds)),
            Some("agile") => seasonality(&named).map(|s| (Anomalies::Agile(s), *bounds)),
            Some("adaptive") => {
                Err("anomalies() 'adaptive' is a legacy algorithm and is not supported; use 'agile' or 'robust'".to_string())
            }
            _ => Err("anomalies() algorithm must be 'basic', 'agile' or 'robust'".to_string()),
        },
        _ => Err("anomalies() takes a query, an algorithm and bounds above 0: anomalies(query, 'basic', 2)".to_string()),
    }
}

fn linear_model(named: &[(String, Literal)]) -> Result<LinearModel, String> {
    match named_arg("model", named).and_then(word) {
        None | Some("default") => Ok(LinearModel::Default),
        Some("simple") => Ok(LinearModel::Simple),
        Some("reactive") => Ok(LinearModel::Reactive),
        Some(m) => Err(format!("forecast() model must be 'default', 'simple' or 'reactive', got '{m}'")),
    }
}

fn duration_arg(name: &str, named: &[(String, Literal)]) -> Result<Option<TimeDelta>, String> {
    match named_arg(name, named) {
        None => Ok(None),
        Some(value) => match word(value).and_then(positive_duration) {
            Some(d) => Ok(Some(d)),
            None => Err(format!("forecast() {name} must be a duration like '4h', '3d' or '1w'")),
        },
    }
}

/// forecast(q, 'linear' | 'seasonal', deviations, model=, history=, horizon=, seasonality=,
/// timezone=): deviations, method, history and horizon.
///
/// `horizon` is ours: Datadog takes it from the monitor (`next_1w`) and does not document one for
/// graphs. `interval` is accepted and ignored.
pub fn forecast_args(rest: &[Arg<String>]) -> Result<(f64, Forecast, Option<TimeDelta>, Option<TimeDelta>), String> {
    let allowed = ["model", "history", "horizon", "seasonality", "timezone", "interval"];
    let (positional, named) = split_args(rest);

    if let Some((n, _)) = named.iter().find(|(n, _)| !allowed.contains(&n.as_str())) {
        return Err(format!("forecast() has no argument '{n}'"));
    }
    let model = linear_model(&named)?;
    let history = duration_arg("history", &named)?;
    let horizon = duration_arg("horizon", &named)?;

    match positional.as_slice() {
        [Some(alg), Some(Literal::Num(deviations))] if *deviations >= 0.0 => match word(alg) {
            Some("linear") => Ok((*deviations, Forecast::Linear(model), history, horizon)),
            Some("seasonal") => seasonality(&named).map(|s| (*deviations, Forecast::Seasonal(s), history, horizon)),
            _ => Err("forecast() algorithm must be 'linear' or 'seasonal'".to_string()),
        },
        _ => Err("forecast() takes a query, an algorithm and deviations: forecast(query, 'linear', 1)".to_string()),
    }
}

// --- grouping --------------------------------------------------------------------------------

/// The `by` keys of a node's series. Numbers and time() have no series at all.
#[derive(PartialEq)]
enum Grouping {
    NoSeries,
    GroupedBy(BTreeSet<String>),
}

/// Arithmetic needs both sides grouped by the same tags ("all queries must be grouped by the
/// same", docs: dashboards/querying). An ungrouped side, a single series or a number, applies to
/// every series of the other.
fn grouping(req: &ParsedTimeseriesRequest, node: &Node) -> Result<Grouping, String> {
    let needs_series = |inner: &Node| match grouping(req, inner) {
        Ok(Grouping::NoSeries) => Err("a function needs a query inside it, not only numbers".to_string()),
        other => other,
    };

    match node {
        Node::Fetch(i) => Ok(Grouping::GroupedBy(req.queries[*i].query.group_by.iter().cloned().collect())),
        Node::Constant(_) | Node::TimeOfPoint => Ok(Grouping::NoSeries),
        // One series out, whatever went in.
        Node::CountNonzero(inner) | Node::CountNotNull(inner) => needs_series(inner).map(|_| Grouping::GroupedBy(BTreeSet::new())),
        Node::Pointwise(_, inner)
        | Node::Top { inner, .. }
        | Node::ExcludeNull(inner)
        | Node::Timewise(_, inner)
        | Node::Shift { inner, .. }
        | Node::DefaultZero(inner)
        | Node::Outliers { inner, .. } => needs_series(inner),
        Node::Arith(_, l, r) => match (grouping(req, l)?, grouping(req, r)?) {
            (Grouping::NoSeries, other) | (other, Grouping::NoSeries) => Ok(other),
            (Grouping::GroupedBy(a), Grouping::GroupedBy(b)) if a.is_empty() => Ok(Grouping::GroupedBy(b)),
            (Grouping::GroupedBy(a), Grouping::GroupedBy(b)) if b.is_empty() || a == b => Ok(Grouping::GroupedBy(a)),
            (Grouping::GroupedBy(a), Grouping::GroupedBy(b)) => {
                let show = |keys: &BTreeSet<String>| keys.iter().map(|k| format!("'{k}'")).collect::<Vec<_>>().join(", ");
                Err(format!("arithmetic needs both sides grouped by the same tags, got {} and {}", show(&a), show(&b)))
            }
        },
    }
}

fn plan_output(
    req: &ParsedTimeseriesRequest,
    by_name: &HashMap<String, usize>,
    index: usize,
    formula: &ParsedFormula,
) -> Result<Output, Vec<String>> {
    let label = format!("formulas[{index}]");
    let resolver = Resolver { from: req.from, label: &label, by_name };
    let (node, extra) = resolver.resolve_output(&formula.formula)?;

    match grouping(req, &node) {
        Err(e) => Err(vec![format!("{label}: {e}")]),
        Ok(Grouping::NoSeries) => Err(vec![format!("{label}: a formula needs at least one query, not only numbers")]),
        Ok(Grouping::GroupedBy(_)) => Ok(Output { query_index: index, node, limit: formula.limit, extra }),
    }
}

fn plan_outputs(req: &ParsedTimeseriesRequest) -> Result<Vec<Output>, Vec<String>> {
    if req.formulas.is_empty() {
        // No formulas: each query is shown as it is.
        return Ok((0..req.queries.len())
            .map(|i| Output { query_index: i, node: Node::Fetch(i), limit: None, extra: Extra::None })
            .collect());
    }

    let by_name: HashMap<String, usize> =
        req.queries.iter().enumerate().filter_map(|(i, q)| q.name.clone().map(|name| (name, i))).collect();

    let outputs: Vec<_> = req.formulas.iter().enumerate().map(|(i, f)| plan_output(req, &by_name, i, f)).collect();

    match errors_in(&outputs).as_slice() {
        [] => Ok(outputs.into_iter().filter_map(Result::ok).collect()),
        errors => Err(errors.to_vec()),
    }
}

// --- lookback --------------------------------------------------------------------------------

/// What a node reads: which query, and shifted by how much. The same query may be read at several
/// offsets (`a` and `week_before(a)` side by side); each is one trip to ClickHouse.
pub fn fetches(node: &Node) -> Vec<(usize, i64)> {
    fn go(shift: i64, node: &Node, out: &mut Vec<(usize, i64)>) {
        match node {
            Node::Fetch(i) => out.push((*i, shift)),
            Node::Shift { offset_ms, inner } => go(shift.saturating_add(*offset_ms), inner, out),
            Node::Pointwise(_, inner)
            | Node::Top { inner, .. }
            | Node::CountNonzero(inner)
            | Node::CountNotNull(inner)
            | Node::ExcludeNull(inner)
            | Node::Timewise(_, inner)
            | Node::DefaultZero(inner)
            | Node::Outliers { inner, .. } => go(shift, inner, out),
            Node::Arith(_, l, r) => {
                go(shift, l, out);
                go(shift, r, out);
            }
            Node::Constant(_) | Node::TimeOfPoint => {}
        }
    }

    let mut out = Vec::new();
    go(0, node, &mut out);
    out
}

/// How far before `from` a query reads, in milliseconds: its lookback, and one bucket more, as
/// the bucket holding `from` minus the lookback may start before it and would be partial.
pub fn earliest_offset_ms(query: &QueryPlan) -> i64 {
    if query.lookback == 0 {
        0
    } else {
        (query.lookback as i64 + 1).saturating_mul(query.step.num_milliseconds())
    }
}

/// Buckets before the window that `node` reads from query `source`, or None when the node does
/// not read that query.
fn history_from(source: usize, node: &Node) -> Option<i32> {
    match node {
        Node::Fetch(i) => (*i == source).then_some(0),
        Node::Timewise(f, inner) => history_from(source, inner).map(|n| n.saturating_add(lookback(*f))),
        Node::Pointwise(_, inner)
        | Node::Top { inner, .. }
        | Node::CountNonzero(inner)
        | Node::CountNotNull(inner)
        | Node::ExcludeNull(inner)
        | Node::Shift { inner, .. }
        | Node::DefaultZero(inner)
        | Node::Outliers { inner, .. } => history_from(source, inner),
        Node::Arith(_, l, r) => match (history_from(source, l), history_from(source, r)) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (Some(a), None) | (None, Some(a)) => Some(a),
            (None, None) => None,
        },
        Node::Constant(_) | Node::TimeOfPoint => None,
    }
}

/// The first query a node reads, whose step an extra's history is counted in.
fn first_source(node: &Node) -> Option<usize> {
    match node {
        Node::Fetch(i) => Some(*i),
        Node::Pointwise(_, inner)
        | Node::Top { inner, .. }
        | Node::CountNonzero(inner)
        | Node::CountNotNull(inner)
        | Node::ExcludeNull(inner)
        | Node::Timewise(_, inner)
        | Node::Shift { inner, .. }
        | Node::DefaultZero(inner)
        | Node::Outliers { inner, .. } => first_source(inner),
        Node::Arith(_, l, r) => first_source(l).or_else(|| first_source(r)),
        Node::Constant(_) | Node::TimeOfPoint => None,
    }
}

/// Buckets before the window that an output's extra reads.
fn extra_history(window: TimeDelta, step: TimeDelta, extra: &Extra) -> i32 {
    match extra {
        Extra::None => 0,
        // Seasonal models fetch their own history at their own step; the band still needs the
        // lagging residuals before the window.
        Extra::Anomalies { .. } => ANOMALY_WINDOW as i32,
        Extra::Forecast { method: Forecast::Seasonal(_), .. } => 0,
        Extra::Forecast { method: Forecast::Linear(_), history, .. } => {
            let beyond_window = history.unwrap_or(window) - window;
            if beyond_window <= TimeDelta::zero() {
                0
            } else {
                (ticks(beyond_window) as f64 / ticks(step) as f64).ceil() as i32
            }
        }
    }
}

fn query_lookback(window: TimeDelta, queries: &[QueryPlan], outputs: &[Output], source: usize) -> i32 {
    outputs
        .iter()
        .filter_map(|output| {
            let buckets = history_from(source, &output.node)?;
            let step = queries[first_source(&output.node).unwrap_or(source)].step;
            Some(buckets.saturating_add(extra_history(window, step, &output.extra)))
        })
        .max()
        .unwrap_or(0)
}

// --- the plan --------------------------------------------------------------------------------

/// Whether any query reads before `cutoff`: from its lookback, or from a shift into the past.
fn reads_before(cutoff: i64, from: i64, queries: &[QueryPlan], outputs: &[Output]) -> bool {
    let reads: Vec<(usize, i64)> = outputs.iter().flat_map(|o| fetches(&o.node)).collect();

    queries.iter().enumerate().any(|(i, q)| {
        let furthest_back = reads.iter().filter(|(source, _)| *source == i).fold(0, |back, (_, offset)| back.min(*offset));
        from.saturating_add(furthest_back).saturating_sub(earliest_offset_ms(q)) < cutoff
    })
}

fn with_lookback(window: TimeDelta, queries: Vec<QueryPlan>, outputs: &[Output]) -> Vec<QueryPlan> {
    let lookbacks: Vec<i32> = (0..queries.len()).map(|i| query_lookback(window, &queries, outputs, i)).collect();
    queries.into_iter().zip(lookbacks).map(|(q, lookback)| QueryPlan { lookback, ..q }).collect()
}

/// Plans a request at `now` (unix milliseconds): the engine never reads the clock.
pub fn plan(retention: Retention, now: i64, req: &ParsedTimeseriesRequest) -> Result<Plan, Vec<String>> {
    let window = TimeDelta::milliseconds(req.to.saturating_sub(req.from));
    let request_step = request_step(retention, now, req.from, req.to, req.interval);

    let mut counts: Vec<(&str, usize)> = Vec::new();
    for name in req.queries.iter().filter_map(|q| q.name.as_deref()) {
        match counts.iter_mut().find(|(n, _)| *n == name) {
            Some((_, count)) => *count += 1,
            None => counts.push((name, 1)),
        }
    }
    let duplicate_names = counts
        .iter()
        .filter(|(_, count)| *count > 1)
        .map(|(name, _)| format!("query name '{name}' is used more than once"));

    let queries: Vec<_> = req
        .queries
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let label = match &q.name {
                Some(name) => format!("queries[{i}] ({name})"),
                None => format!("queries[{i}]"),
            };
            plan_query(window, request_step, &label, &q.query)
        })
        .collect();

    let outputs = plan_outputs(req);

    let mut errors: Vec<String> = duplicate_names.collect();
    errors.extend(errors_in(&queries));
    if let Err(e) = &outputs {
        errors.extend(e.iter().cloned());
    }

    match outputs {
        Ok(outputs) if errors.is_empty() => {
            let planned = with_lookback(window, queries.into_iter().filter_map(Result::ok).collect(), &outputs);

            // A read before the oldest raw point is answered by the rollup alone, so every step of
            // the request becomes whole minutes: steps stay alike across a formula, and its
            // buckets line up.
            let cutoff = now.saturating_sub(retention.raw.num_milliseconds());
            let (step, queries) = if reads_before(cutoff, req.from, &planned, &outputs) {
                let minutes = planned.into_iter().map(|q| QueryPlan { step: whole_minutes(q.step), ..q }).collect();
                (whole_minutes(request_step), with_lookback(window, minutes, &outputs))
            } else {
                (request_step, planned)
            };

            Ok(Plan { from: req.from, to: req.to, step, queries, outputs, retention })
        }
        _ => Err(errors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::v2::timeseries::read;

    fn request(queries: &[(Option<&str>, &str)], formulas: &[&str]) -> ParsedTimeseriesRequest {
        let qs: Vec<String> = queries
            .iter()
            .map(|(name, q)| {
                let n = name.map(|n| format!(r#","name":"{n}""#)).unwrap_or_default();
                format!(r#"{{"data_source":"metrics","query":"{q}"{n}}}"#)
            })
            .collect();
        let fs: Vec<String> = formulas.iter().map(|f| format!(r#"{{"formula":"{f}"}}"#)).collect();
        let body = format!(
            r#"{{"data":{{"type":"timeseries_request","attributes":{{"from":1711977600000,"to":1711981200000,"queries":[{}],"formulas":[{}]}}}}}}"#,
            qs.join(","),
            fs.join(",")
        );
        read(&body).unwrap_or_else(|es| panic!("{}", es.join("\n")))
    }

    /// The requests' `to`: their windows are within the raw retention.
    const NOW: i64 = 1711981200000;
    const DAY_MS: i64 = 86_400_000;

    fn plan_at(req: &ParsedTimeseriesRequest) -> Result<Plan, Vec<String>> {
        plan(DEFAULT_RETENTION, NOW, req)
    }

    fn plan_of(queries: &[(Option<&str>, &str)], formulas: &[&str]) -> Plan {
        plan_at(&request(queries, formulas)).unwrap_or_else(|es| panic!("{}", es.join("\n")))
    }

    fn errors_of(queries: &[(Option<&str>, &str)], formulas: &[&str]) -> Vec<String> {
        match plan_at(&request(queries, formulas)) {
            Ok(p) => panic!("expected errors, got {p:?}"),
            Err(es) => es,
        }
    }

    fn assert_contains(errors: &[String], expected: &str) {
        assert!(errors.iter().any(|e| e == expected), "{expected:?} not in {errors:?}");
    }

    fn single<T: std::fmt::Debug>(items: &[T]) -> &T {
        match items {
            [one] => one,
            _ => panic!("expected one, got {items:?}"),
        }
    }

    // --- the step ----------------------------------------------------------------------------

    /// Every row of Datadog's table for line graphs (docs: dashboards/functions/rollup).
    #[test]
    fn default_step_follows_datadogs_table() {
        for (window, step_seconds) in [
            (60, 1),             // past minute → 1s
            (300, 2),            // past 5 minutes → 2s
            (900, 5),            // past 15 minutes → 5s
            (1800, 10),          // past 30 minutes → 10s
            (3600, 20),          // past hour → 20s
            (14400, 60),         // past 4 hours → 1m
            (86400, 300),        // past day → 5m
            (604800, 3600),      // past week → 1h
            (2592000, 14400),    // past month (30 days) → 4h
            (31536000, 86400),   // past year → 1d
        ] {
            assert_eq!(step(TimeDelta::seconds(window), None), TimeDelta::seconds(step_seconds), "{window}");
        }
    }

    #[test]
    fn an_interval_hint_is_honoured() {
        // The recorded Datadog request: one hour, interval 5000 ms.
        assert_eq!(step(TimeDelta::hours(1), Some(TimeDelta::milliseconds(5000))), TimeDelta::seconds(5));
    }

    #[test]
    fn a_hint_that_would_exceed_1500_points_is_widened() {
        // A day at 1 s would be 86 400 points; 86 400 / 1500 = 57.6 → 58 s.
        assert_eq!(step(TimeDelta::days(1), Some(TimeDelta::seconds(1))), TimeDelta::seconds(58));
    }

    #[test]
    fn a_sub_second_hint_is_rounded_up_to_a_second() {
        assert_eq!(step(TimeDelta::minutes(1), Some(TimeDelta::milliseconds(200))), TimeDelta::seconds(1));
    }

    // --- the table ---------------------------------------------------------------------------

    #[test]
    fn a_step_of_whole_minutes_reads_the_rollup() {
        for (step_seconds, rollup) in [(60, true), (300, true), (86400, true), (20, false), (90, false)] {
            let expected = if rollup { Table::Rollup } else { Table::Points };
            assert_eq!(table_for(TimeDelta::seconds(step_seconds)), expected, "{step_seconds}");
        }
    }

    /// One hour, its `from` `days_back` days before `NOW`.
    fn hour_ago(days_back: i64, queries: &[(Option<&str>, &str)], formulas: &[&str]) -> Plan {
        let from = NOW - days_back * DAY_MS;
        let req = ParsedTimeseriesRequest { from, to: from + 3_600_000, ..request(queries, formulas) };
        plan_at(&req).unwrap_or_else(|es| panic!("{}", es.join("\n")))
    }

    #[test]
    fn a_window_before_the_raw_retention_gets_whole_minutes() {
        // An hour is 20 s steps; 40 days back, only the rollup has it.
        assert_eq!(hour_ago(1, &[(None, "x{*}")], &[]).step, TimeDelta::seconds(20));
        let old = hour_ago(40, &[(None, "x{*}")], &[]);
        assert_eq!(old.step, TimeDelta::minutes(1));
        assert_eq!(single(&old.queries).step, TimeDelta::minutes(1));
        // A rollup interval of the query's own is rounded up as well.
        assert_eq!(single(&hour_ago(40, &[(None, "x{*}.rollup(sum, 90)")], &[]).queries).step, TimeDelta::minutes(2));
        let from = NOW - 40 * DAY_MS;
        assert_eq!(request_step(DEFAULT_RETENTION, NOW, from, from + 3_600_000, None), TimeDelta::minutes(1));
    }

    #[test]
    fn a_shift_or_a_lookback_before_the_raw_retention_gets_whole_minutes_for_every_query() {
        let ab = [(Some("a"), "x{*}"), (Some("b"), "y{*}")];
        let steps_of = |p: Plan| p.queries.iter().map(|q| q.step.num_seconds()).collect::<Vec<_>>();
        assert_eq!(steps_of(hour_ago(1, &ab, &["a / b"])), [20, 20]);
        // A week back from a day ago is still in the raw points; from 25 days ago it is not.
        assert_eq!(steps_of(hour_ago(1, &ab, &["a / week_before(b)"])), [20, 20]);
        assert_eq!(steps_of(hour_ago(25, &ab, &["a / week_before(b)"])), [60, 60]);
        // Forty days of forecast history read far before the window.
        assert_eq!(steps_of(hour_ago(1, &[(Some("a"), "x{*}")], &["forecast(a, 'linear', 1, history='40d')"])), [60]);
    }

    // --- queries -----------------------------------------------------------------------------

    #[test]
    fn aggregator_defaults_to_avg() {
        let p = plan_of(&[(None, "system.cpu.user{*}")], &[]);
        assert_eq!(single(&p.queries).space_agg, Aggregation::Avg);
    }

    #[test]
    fn aggregator_filter_and_group_by_are_carried_over() {
        let p = plan_of(&[(None, "sum:system.cpu.user{env:prod} by {host,role}")], &[]);
        let q = single(&p.queries);
        assert_eq!(q.space_agg, Aggregation::Sum);
        assert_eq!(q.metric, "system.cpu.user");
        assert_eq!(q.group_by, ["host", "role"]);
    }

    #[test]
    fn what_is_not_supported_yet_is_named() {
        let es = errors_of(&[(Some("a"), "p95:x{*}.weighted()")], &[]);
        assert_contains(&es, "queries[0] (a): aggregator 'p95:' is not supported yet");
        assert_contains(&es, "queries[0] (a): .weighted() is not supported yet");
    }

    #[test]
    fn unsubstituted_template_variables_are_refused() {
        let es = errors_of(&[(None, "x{$env}")], &[]);
        assert_contains(&es, "queries[0]: template variable '$env' was not substituted");
    }

    // --- outputs -----------------------------------------------------------------------------

    #[test]
    fn no_formulas_every_query_is_an_output() {
        let p = plan_of(&[(Some("a"), "x{*}"), (Some("b"), "y{*}")], &[]);
        assert_eq!(p.outputs.iter().map(|o| o.query_index).collect::<Vec<_>>(), [0, 1]);
        assert_eq!(p.outputs.iter().map(|o| o.node.clone()).collect::<Vec<_>>(), [Node::Fetch(0), Node::Fetch(1)]);
    }

    #[test]
    fn formulas_name_their_query_and_set_query_index() {
        let p = plan_of(&[(Some("a"), "x{*}"), (Some("b"), "y{*}")], &["b"]);
        let o = single(&p.outputs);
        assert_eq!(o.query_index, 0);
        assert_eq!(o.node, Node::Fetch(1));
    }

    #[test]
    fn a_formula_naming_nothing_is_refused() {
        assert_contains(&errors_of(&[(Some("a"), "x{*}")], &["c"]), "formulas[0]: no query is named 'c'");
    }

    fn arith_node(op: ArithOp, l: Node, r: Node) -> Node {
        Node::Arith(op, Box::new(l), Box::new(r))
    }

    #[test]
    fn arithmetic_between_named_queries() {
        let p = plan_of(&[(Some("a"), "x{*}"), (Some("b"), "y{*}")], &["a / b"]);
        assert_eq!(single(&p.outputs).node, arith_node(ArithOp::Divide, Node::Fetch(0), Node::Fetch(1)));
    }

    #[test]
    fn duplicate_query_names_are_refused() {
        assert_contains(&errors_of(&[(Some("a"), "x{*}"), (Some("a"), "y{*}")], &[]), "query name 'a' is used more than once");
    }

    // --- rollup and type modifiers -----------------------------------------------------------

    fn only_query(q: &str) -> QueryPlan {
        single(&plan_of(&[(None, q)], &[]).queries).clone()
    }

    #[test]
    fn no_modifier_avg_over_the_requests_step() {
        let q = only_query("sum:requests{*}");
        assert_eq!(q.time_agg, TimeAggregation::Plain(Aggregation::Avg));
        assert_eq!(q.step, TimeDelta::seconds(20)); // one hour → 20 s
    }

    #[test]
    fn rollup_with_a_method_keeps_the_step() {
        let q = only_query("x{*}.rollup(max)");
        assert_eq!(q.time_agg, TimeAggregation::Plain(Aggregation::Max));
        assert_eq!(q.step, TimeDelta::seconds(20));
    }

    #[test]
    fn rollup_with_an_interval_sets_this_querys_step() {
        let q = only_query("x{*}.rollup(sum, 60)");
        assert_eq!(q.time_agg, TimeAggregation::Plain(Aggregation::Sum));
        assert_eq!(q.step, TimeDelta::seconds(60));
    }

    #[test]
    fn rollup_count_is_a_method() {
        assert_eq!(only_query("x{*}.rollup(count, 60)").time_agg, TimeAggregation::Plain(Aggregation::Count));
    }

    #[test]
    fn a_rollup_interval_over_1500_points_is_widened() {
        // One hour at 1 s is 3600 points; 3600 / 1500 = 2.4 → 3 s.
        assert_eq!(only_query("x{*}.rollup(avg, 1)").step, TimeDelta::seconds(3));
    }

    #[test]
    fn as_count_and_as_rate() {
        assert_eq!(only_query("sum:x{*}.as_count()").time_agg, TimeAggregation::AsCount);
        assert_eq!(only_query("sum:x{*}.as_rate()").time_agg, TimeAggregation::AsRate);
    }

    #[test]
    fn as_count_with_a_sum_rollup_in_either_order() {
        // Datadog docs: sum:requests.count{*}.as_count().rollup(sum, …)
        let q = only_query("sum:requests{*}.as_count().rollup(sum, 120)");
        assert_eq!(q.time_agg, TimeAggregation::AsCount);
        assert_eq!(q.step, TimeDelta::seconds(120));
        assert_eq!(only_query("sum:requests{*}.rollup(sum, 120).as_count()").time_agg, TimeAggregation::AsCount);
    }

    #[test]
    fn contradictions_and_unsupported_forms_are_named() {
        assert_contains(
            &errors_of(&[(None, "x{*}.as_count().rollup(max)")], &[]),
            "queries[0]: .as_count() with .rollup(max) is not supported yet",
        );
        assert_contains(
            &errors_of(&[(None, "x{*}.as_count().as_rate()")], &[]),
            "queries[0]: .as_rate() cannot follow .as_count()",
        );
        assert_contains(
            &errors_of(&[(None, "x{*}.rollup(median)")], &[]),
            "queries[0]: .rollup() method must be one of avg, sum, min, max, count",
        );
        assert_contains(
            &errors_of(&[(None, "x{*}.rollup(sum, monthly)")], &[]),
            "queries[0]: calendar .rollup() (daily, weekly, monthly, alignment, timezone) is not supported yet",
        );
    }

    // --- functions in formulas ---------------------------------------------------------------

    fn node_of(formula: &str) -> Node {
        single(&plan_of(&[(Some("a"), "x{*}")], &[formula]).outputs).node.clone()
    }

    fn err(formula: &str) -> Vec<String> {
        errors_of(&[(Some("a"), "x{*}")], &[formula])
    }

    fn fetch0() -> Box<Node> {
        Box::new(Node::Fetch(0))
    }

    #[test]
    fn pointwise_functions_resolve_to_typed_nodes() {
        assert_eq!(node_of("abs(a)"), Node::Pointwise(PointwiseFn::Abs, fetch0()));
        assert_eq!(node_of("clamp_min(a, 100)"), Node::Pointwise(PointwiseFn::ClampMin(100.0), fetch0()));
        assert_eq!(node_of("cutoff_max(a, -5)"), Node::Pointwise(PointwiseFn::CutoffMax(-5.0), fetch0()));
        assert_eq!(node_of("round(a)"), Node::Pointwise(PointwiseFn::Round(0), fetch0()));
        assert_eq!(node_of("round(a, 2)"), Node::Pointwise(PointwiseFn::Round(2), fetch0()));
    }

    #[test]
    fn functions_nest() {
        assert_eq!(
            node_of("abs(log10(a))"),
            Node::Pointwise(PointwiseFn::Abs, Box::new(Node::Pointwise(PointwiseFn::Log10, fetch0())))
        );
    }

    #[test]
    fn wrong_arguments_are_named() {
        assert_contains(&err("abs(a, 2)"), "formulas[0]: abs() takes one argument: abs(query)");
        assert_contains(&err("clamp_min(a)"), "formulas[0]: clamp_min() takes a query and a number: clamp_min(query, 100)");
        assert_contains(&err("clamp_min(a, 'x')"), "formulas[0]: clamp_min() takes only numbers after the query");
        assert_contains(&err("abs('a')"), "formulas[0]: the first argument of abs() must be a query");
        assert_contains(&err("abs()"), "formulas[0]: abs() needs a query to act on");
        assert_contains(
            &err("round(a, 1.5)"),
            "formulas[0]: round() takes a query and optionally a whole number of decimals from 0 to 15: round(query, 2)",
        );
    }

    #[test]
    fn unknown_functions_and_names_inside_functions_are_reported_together() {
        let es = errors_of(&[(Some("a"), "x{*}")], &["dt(a)", "abs(b)"]);
        assert_contains(&es, "formulas[0]: function 'dt' is not supported yet");
        assert_contains(&es, "formulas[1]: no query is named 'b'");
    }

    // --- across series -----------------------------------------------------------------------

    fn top(inner: Node, count: usize, by: RankBy, order: SortOrder) -> Node {
        Node::Top { inner: Box::new(inner), count, by, order }
    }

    #[test]
    fn top_with_quoted_or_bare_words() {
        assert_eq!(node_of("top(a, 10, 'mean', 'desc')"), top(Node::Fetch(0), 10, RankBy::Mean, SortOrder::Desc));
        // Bare words arrive as query names; top wants words there, so they are.
        assert_eq!(node_of("top(a, 5, l2norm, asc)"), top(Node::Fetch(0), 5, RankBy::L2norm, SortOrder::Asc));
    }

    #[test]
    fn top_shorthands() {
        assert_eq!(node_of("top10_mean(a)"), top(Node::Fetch(0), 10, RankBy::Mean, SortOrder::Desc));
        assert_eq!(node_of("bottom10_min(a)"), top(Node::Fetch(0), 10, RankBy::Min, SortOrder::Asc));
        assert_eq!(node_of("top20_area(a)"), top(Node::Fetch(0), 20, RankBy::Area, SortOrder::Desc));
    }

    #[test]
    fn count_and_exclude_functions() {
        assert_eq!(node_of("count_nonzero(a)"), Node::CountNonzero(fetch0()));
        assert_eq!(node_of("count_nonzero_finite(a)"), Node::CountNonzero(fetch0()));
        assert_eq!(node_of("count_not_null(a)"), Node::CountNotNull(fetch0()));
        assert_eq!(node_of("exclude_null(a)"), Node::ExcludeNull(fetch0()));
    }

    #[test]
    fn across_series_functions_combine_with_pointwise_ones() {
        assert_eq!(
            node_of("top(abs(a), 5, 'max', 'desc')"),
            top(Node::Pointwise(PointwiseFn::Abs, fetch0()), 5, RankBy::Max, SortOrder::Desc)
        );
    }

    #[test]
    fn top_arguments_are_checked() {
        assert_contains(&err("top(a, 7, 'mean', 'desc')"), "formulas[0]: top() limit must be one of 5, 10, 25, 50, 100");
        assert_contains(
            &err("top(a, 10, 'median', 'desc')"),
            "formulas[0]: top() ranks by one of 'max', 'mean', 'min', 'sum', 'last', 'l2norm', 'area'",
        );
        assert_contains(&err("top(a, 10, 'mean', 'up')"), "formulas[0]: top() direction must be 'asc' or 'desc'");
        assert_contains(
            &err("top(a)"),
            "formulas[0]: top() takes a query, a limit, a ranking and a direction: top(query, 10, 'mean', 'desc')",
        );
        assert_contains(&err("top10_mean(a, 3)"), "formulas[0]: top10_mean() takes one argument: top10_mean(query)");
        // Not in the documented pattern: 25 is a top() limit, not a shorthand size.
        assert_contains(&err("top25_mean(a)"), "formulas[0]: function 'top25_mean' is not supported yet");
    }

    // --- along time --------------------------------------------------------------------------

    #[test]
    fn time_wise_functions_resolve_with_the_documented_aliases() {
        assert_eq!(node_of("cumsum(a)"), Node::Timewise(TimewiseFn::Cumsum, fetch0()));
        assert_eq!(node_of("per_minute(a)"), Node::Timewise(TimewiseFn::PerMinute, fetch0()));
        assert_eq!(node_of("ewma_7(a)"), Node::Timewise(TimewiseFn::Ewma(7), fetch0()));
        assert_eq!(node_of("ewma(a)"), Node::Timewise(TimewiseFn::Ewma(20), fetch0()));
        assert_eq!(node_of("median(a)"), Node::Timewise(TimewiseFn::Median(3), fetch0()));
        assert_eq!(node_of("rollingavg_13(a)"), Node::Timewise(TimewiseFn::RollingAvg(13), fetch0()));
    }

    #[test]
    fn undocumented_spans_are_not_guessed() {
        assert_contains(&err("ewma_4(a)"), "formulas[0]: function 'ewma_4' is not supported yet");
    }

    #[test]
    fn lookback_is_the_deepest_a_query_is_read() {
        let p = plan_of(&[(Some("a"), "x{*}"), (Some("b"), "y{*}")], &["diff(ewma_5(a))", "median_7(a)", "cumsum(b)"]);
        // a: diff 1 + ewma 2×5 = 11, more than median_7's 6. b: cumsum needs none.
        assert_eq!(p.queries.iter().map(|q| q.lookback).collect::<Vec<_>>(), [11, 0]);
    }

    // --- shifts ------------------------------------------------------------------------------

    fn shift(offset_ms: i64) -> Node {
        Node::Shift { offset_ms, inner: fetch0() }
    }

    #[test]
    fn fixed_shifts() {
        assert_eq!(node_of("timeshift(a, -3600)"), shift(-3_600_000));
        assert_eq!(node_of("hour_before(a)"), shift(-3_600_000));
        assert_eq!(node_of("day_before(a)"), shift(-86_400_000));
        assert_eq!(node_of("week_before(a)"), shift(-604_800_000));
    }

    /// Plans one formula with a window starting at `from`.
    fn shift_at(from: &str, formula: &str) -> i64 {
        let from_ms = DateTime::parse_from_rfc3339(from).unwrap().timestamp_millis();
        let body = format!(
            r#"{{"data":{{"type":"timeseries_request","attributes":{{"from":{from_ms},"to":{},"queries":[{{"data_source":"metrics","name":"a","query":"x{{*}}"}}],"formulas":[{{"formula":"{formula}"}}]}}}}}}"#,
            from_ms + 3_600_000
        );
        let p = read(&body).and_then(|r| plan_at(&r)).unwrap_or_else(|es| panic!("{}", es.join("\n")));
        match &single(&p.outputs).node {
            Node::Shift { offset_ms, inner } if **inner == Node::Fetch(0) => *offset_ms,
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn calendar_shift_by_weeks_and_days() {
        assert_eq!(shift_at("2026-03-10T12:00:00Z", r#"calendar_shift(a, \"-2w\")"#), -2 * 604_800_000);
        assert_eq!(shift_at("2026-03-10T12:00:00Z", r#"calendar_shift(a, \"-1d\", \"UTC\")"#), -86_400_000);
    }

    #[test]
    fn calendar_shift_keeps_wall_clock_time_across_dst() {
        // Europe/Warsaw moved to summer time at 02:00 on 2026-03-29. Noon that day (10:00 UTC)
        // minus one calendar day is noon on the 28th, still winter time (11:00 UTC): 23 hours
        // earlier, not 24.
        assert_eq!(
            shift_at("2026-03-29T10:00:00Z", r#"calendar_shift(a, \"-1d\", \"Europe/Warsaw\")"#),
            -23 * 3_600_000
        );
    }

    #[test]
    fn a_month_back_is_a_calendar_month() {
        // 31 March → 28 February (2026 is not a leap year): 31 days back.
        assert_eq!(shift_at("2026-03-31T12:00:00Z", "month_before(a)"), -31 * DAY_MS);
        assert_eq!(shift_at("2026-03-31T12:00:00Z", r#"calendar_shift(a, \"-1mo\")"#), -31 * DAY_MS);
    }

    #[test]
    fn shift_arguments_are_checked() {
        assert_contains(
            &err("timeshift(a)"),
            "formulas[0]: timeshift() takes a query and a whole, non-zero number of seconds: timeshift(query, -3600)",
        );
        assert_contains(&err("day_before(a, 2)"), "formulas[0]: day_before() takes one argument: day_before(query)");
        assert_contains(
            &err("calendar_shift(a, '1d')"),
            "formulas[0]: calendar_shift() shift must look like \"-1d\", \"-2w\" or \"-1mo\", got \"1d\"",
        );
        assert_contains(
            &err("calendar_shift(a, '-1d', 'Mars/Olympus')"),
            "formulas[0]: calendar_shift() timezone 'Mars/Olympus' is not a known IANA zone",
        );
    }

    #[test]
    fn a_shift_keeps_the_lookback_of_what_it_wraps() {
        let p = plan_of(&[(Some("a"), "x{*}")], &["week_before(diff(a))"]);
        assert_eq!(single(&p.queries).lookback, 1);
    }

    // --- fill --------------------------------------------------------------------------------

    fn within(method: FillMethod, limit_seconds: i32) -> Fill {
        Fill::Within { method, limit_seconds }
    }

    #[test]
    fn interpolation_is_on_by_default_off_under_as_count_and_as_rate() {
        assert_eq!(only_query("avg:x{*}").fill, within(FillMethod::Linear, 300));
        assert_eq!(only_query("sum:x{*}.as_count()").fill, Fill::NoFill);
        assert_eq!(only_query("sum:x{*}.as_rate()").fill, Fill::NoFill);
    }

    #[test]
    fn the_fill_modifier() {
        assert_eq!(only_query("avg:x{*}.fill(null)").fill, Fill::NoFill);
        assert_eq!(only_query("avg:x{*}.fill(zero)").fill, within(FillMethod::Zero, 300));
        assert_eq!(only_query("avg:x{*}.fill(last, 120)").fill, within(FillMethod::Last, 120));
        assert_eq!(only_query("sum:x{*}.as_count().fill(linear, 60)").fill, within(FillMethod::Linear, 60));
    }

    #[test]
    fn fill_arguments_are_checked() {
        assert_contains(
            &errors_of(&[(None, "x{*}.fill(mean)")], &[]),
            "queries[0]: .fill() method must be one of null, zero, last, linear",
        );
        assert_contains(
            &errors_of(&[(None, "x{*}.fill(zero, 1.5)")], &[]),
            "queries[0]: .fill() limit must be a whole number of seconds, e.g. .fill(linear, 300)",
        );
    }

    #[test]
    fn default_zero_resolves() {
        assert_eq!(node_of("default_zero(a)"), Node::DefaultZero(fetch0()));
    }

    // --- algorithms --------------------------------------------------------------------------

    #[test]
    fn regressions_and_autosmooth_resolve() {
        assert_eq!(node_of("trend_line(a)"), Node::Timewise(TimewiseFn::TrendLine, fetch0()));
        assert_eq!(node_of("robust_trend(a)"), Node::Timewise(TimewiseFn::RobustTrend, fetch0()));
        assert_eq!(node_of("piecewise_constant(a)"), Node::Timewise(TimewiseFn::PiecewiseConstant, fetch0()));
        assert_eq!(node_of("autosmooth(a)"), Node::Timewise(TimewiseFn::Autosmooth, fetch0()));
    }

    fn outliers(algorithm: Outliers, tolerance: f64) -> Node {
        Node::Outliers { inner: fetch0(), algorithm, tolerance }
    }

    #[test]
    fn outliers_resolve_any_case() {
        assert_eq!(node_of("outliers(a, 'DBSCAN', 3)"), outliers(Outliers::Dbscan, 3.0));
        assert_eq!(node_of("outliers(a, 'scaledbscan', 2.5)"), outliers(Outliers::ScaledDbscan, 2.5));
        assert_eq!(node_of("outliers(a, 'mad', 3, 20)"), outliers(Outliers::Mad { percent: 20.0 }, 3.0));
        assert_eq!(node_of("outliers(a, 'scaledMAD', 2, 10)"), outliers(Outliers::ScaledMad { percent: 10.0 }, 2.0));
    }

    #[test]
    fn outliers_arguments_are_checked() {
        assert_contains(
            &err("outliers(a, 'mad', 3)"),
            "formulas[0]: outliers() with MAD needs a percentage: outliers(query, 'mad', 3, 20)",
        );
        assert_contains(
            &err("outliers(a, 'kmeans', 3)"),
            "formulas[0]: outliers() takes a query, an algorithm, a tolerance and for MAD a percentage: outliers(query, 'dbscan', 3) or outliers(query, 'mad', 3, 20)",
        );
    }

    // --- anomalies and forecast --------------------------------------------------------------

    fn output_of(formula: &str) -> Output {
        single(&plan_of(&[(Some("a"), "x{*}")], &[formula]).outputs).clone()
    }

    #[test]
    fn anomalies_and_forecast_are_extras_beside_the_node() {
        let o = output_of("anomalies(abs(a), 'basic', 2, direction='above', alert_window='last_15m')");
        assert_eq!(o.node, Node::Pointwise(PointwiseFn::Abs, fetch0()));
        assert_eq!(o.extra, Extra::Anomalies { algorithm: Anomalies::Basic, bounds: 2.0 });
        assert_eq!(
            output_of("forecast(a, 'linear', 1)").extra,
            Extra::Forecast { deviations: 1.0, method: Forecast::Linear(LinearModel::Default), history: None, horizon: None }
        );
        assert_eq!(
            output_of("forecast(a, 'linear', 2, model='reactive', history='1w', horizon='4h')").extra,
            Extra::Forecast {
                deviations: 2.0,
                method: Forecast::Linear(LinearModel::Reactive),
                history: Some(TimeDelta::days(7)),
                horizon: Some(TimeDelta::hours(4)),
            }
        );
    }

    #[test]
    fn anomalies_and_forecast_must_be_outermost() {
        assert_contains(
            &err("abs(anomalies(a, 'basic', 2))"),
            "formulas[0]: anomalies() must be the outermost function of a formula",
        );
    }

    #[test]
    fn unsupported_algorithms_and_bad_arguments_are_named() {
        assert_contains(&err("anomalies(a, 'magic', 2)"), "formulas[0]: anomalies() algorithm must be 'basic', 'agile' or 'robust'");
        assert_contains(
            &err("anomalies(a, 'robust', 2, seasonality='yearly')"),
            "formulas[0]: seasonality must be 'hourly', 'daily' or 'weekly', got 'yearly'",
        );
        assert_contains(
            &err("forecast(a, 'linear', 1, horizon='soon')"),
            "formulas[0]: forecast() horizon must be a duration like '4h', '3d' or '1w'",
        );
        assert_contains(&err("forecast(a, 'linear', 1, colour='red')"), "formulas[0]: forecast() has no argument 'colour'");
    }

    #[test]
    fn history_beyond_the_window_becomes_lookback() {
        // One hour window at 20 s steps; a day of history is 23 h more: 4140 buckets.
        let p = plan_of(&[(Some("a"), "x{*}")], &["forecast(a, 'linear', 1, history='1d')"]);
        assert_eq!(single(&p.queries).lookback, 4140);
        let p = plan_of(&[(Some("a"), "x{*}")], &["anomalies(a, 'basic', 2)"]);
        assert_eq!(single(&p.queries).lookback, ANOMALY_WINDOW as i32);
    }

    #[test]
    fn seasonal_algorithms_take_their_seasonality_weekly_by_default() {
        assert_eq!(
            output_of("anomalies(a, 'robust', 3)").extra,
            Extra::Anomalies { algorithm: Anomalies::Robust(Seasonality::Weekly), bounds: 3.0 }
        );
        assert_eq!(
            output_of("anomalies(a, 'agile', 2, seasonality='hourly', timezone='Europe/Paris')").extra,
            Extra::Anomalies { algorithm: Anomalies::Agile(Seasonality::Hourly), bounds: 2.0 }
        );
        assert_eq!(
            output_of("forecast(a, 'seasonal', 1, seasonality='daily')").extra,
            Extra::Forecast { deviations: 1.0, method: Forecast::Seasonal(Seasonality::Daily), history: None, horizon: None }
        );
    }

    #[test]
    fn seasonal_history_is_six_seasons_within_the_rollups_retention() {
        assert_eq!(seasonal_history(DEFAULT_RETENTION, Seasonality::Hourly), TimeDelta::hours(6));
        assert_eq!(seasonal_history(DEFAULT_RETENTION, Seasonality::Daily), TimeDelta::days(6));
        assert_eq!(seasonal_history(DEFAULT_RETENTION, Seasonality::Weekly), TimeDelta::days(42));
        // A rollup kept 30 days holds four whole weeks.
        let month = Retention { rollup: TimeDelta::days(30), ..DEFAULT_RETENTION };
        assert_eq!(seasonal_history(month, Seasonality::Weekly), TimeDelta::days(28));
        assert_eq!(
            [Seasonality::Hourly.period(), Seasonality::Daily.period(), Seasonality::Weekly.period()],
            [60, 288, 168]
        );
    }

    // --- arithmetic --------------------------------------------------------------------------

    fn node_of2(queries: &[(Option<&str>, &str)], formula: &str) -> Node {
        single(&plan_of(queries, &[formula]).outputs).node.clone()
    }

    const AB: [(Option<&str>, &str); 2] = [(Some("a"), "x{*} by {host}"), (Some("b"), "y{*} by {host}")];

    #[test]
    fn operators_precedence_numbers_and_minus() {
        // (query1 - query2) / query1 * 100, from the corpus plus a constant.
        assert_eq!(
            node_of2(&AB, "(a - b) / a * 100"),
            arith_node(
                ArithOp::Times,
                arith_node(ArithOp::Divide, arith_node(ArithOp::Minus, Node::Fetch(0), Node::Fetch(1)), Node::Fetch(0)),
                Node::Constant(100.0)
            )
        );
        assert_eq!(node_of2(&AB, "-a"), arith_node(ArithOp::Times, Node::Constant(-1.0), Node::Fetch(0)));
    }

    #[test]
    fn minimum_maximum_pow_and_time() {
        assert_eq!(node_of2(&AB, "minimum(a, b)"), arith_node(ArithOp::Minimum, Node::Fetch(0), Node::Fetch(1)));
        assert_eq!(node_of2(&AB, "pow(a, 2)"), arith_node(ArithOp::Power, Node::Fetch(0), Node::Constant(2.0)));
        // Datadog docs: time() - max:backup.last_completed_timestamp{*}
        assert_eq!(
            node_of2(&[(Some("a"), "max:backup.last_completed_timestamp{*}")], "time() - a"),
            arith_node(ArithOp::Minus, Node::TimeOfPoint, Node::Fetch(0))
        );
    }

    #[test]
    fn functions_over_arithmetic_and_arithmetic_over_functions() {
        assert_eq!(
            node_of2(&AB, "abs(a - b)"),
            Node::Pointwise(PointwiseFn::Abs, Box::new(arith_node(ArithOp::Minus, Node::Fetch(0), Node::Fetch(1))))
        );
        assert_eq!(
            node_of2(&AB, "cumsum(a) / 60"),
            arith_node(ArithOp::Divide, Node::Timewise(TimewiseFn::Cumsum, fetch0()), Node::Constant(60.0))
        );
    }

    #[test]
    fn both_sides_must_group_alike_or_one_not_at_all() {
        assert_contains(
            &errors_of(&[(Some("a"), "x{*} by {host}"), (Some("b"), "y{*} by {env}")], &["a / b"]),
            "formulas[0]: arithmetic needs both sides grouped by the same tags, got 'host' and 'env'",
        );
        // Ungrouped, or grouped the same in another order: fine.
        node_of2(&[(Some("a"), "x{*} by {host}"), (Some("b"), "y{*}")], "a / b");
        node_of2(&[(Some("a"), "x{*} by {host,env}"), (Some("b"), "y{*} by {env,host}")], "a / b");
    }

    #[test]
    fn a_formula_needs_a_query() {
        assert_contains(&err("2 * 3"), "formulas[0]: a formula needs at least one query, not only numbers");
        assert_contains(&err("a + abs(5)"), "formulas[0]: a function needs a query inside it, not only numbers");
    }

    #[test]
    fn floats_print_as_fsharp_strings() {
        assert_eq!(dotnet_number(3.0), "3");
        assert_eq!(dotnet_number(2.5), "2.5");
        assert_eq!(dotnet_number(0.0001), "0.0001");
        assert_eq!(dotnet_number(0.00001), "1E-05");
        assert_eq!(dotnet_number(1e15), "1E+15");
        assert_eq!(dotnet_number(123456789012345.0), "123456789012345");
        assert_eq!(dotnet_number(-1.5e20), "-1.5E+20");
    }
}
