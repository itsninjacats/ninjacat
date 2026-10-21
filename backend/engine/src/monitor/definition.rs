//! A monitor as Datadog defines one, read into what the evaluation needs.

use chrono::TimeDelta;
use ninjapaws_anomaly::{Anomalies, Forecast, Outliers};

use crate::metric_query::ast::{Expr, MetricQuery, TagFilter};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowUnit {
    Minutes,
    Hours,
    Days,
    Weeks,
    Months,
}

/// `last_5m`, `next_1w`, `1h_ago`: a count of one unit. The unit is kept because
/// require_full_window sizes its buckets by it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub count: i32,
    pub unit: WindowUnit,
}

impl Window {
    pub fn length(&self) -> TimeDelta {
        let n = i64::from(self.count);
        match self.unit {
            WindowUnit::Minutes => TimeDelta::minutes(n),
            WindowUnit::Hours => TimeDelta::hours(n),
            WindowUnit::Days => TimeDelta::days(n),
            WindowUnit::Weeks => TimeDelta::days(7 * n),
            // WARNING(undocumented): how long Datadog's month is. 31 days, so last_1mo covers
            // every calendar month.
            WindowUnit::Months => TimeDelta::days(31 * n),
        }
    }

    /// `15m`, `1mo`: the count and unit as Datadog writes them.
    pub fn text(&self) -> String {
        let unit = match self.unit {
            WindowUnit::Minutes => "m",
            WindowUnit::Hours => "h",
            WindowUnit::Days => "d",
            WindowUnit::Weeks => "w",
            WindowUnit::Months => "mo",
        };
        format!("{}{unit}", self.count)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comparator {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl Comparator {
    pub fn text(&self) -> &'static str {
        match self {
            Comparator::Lt => "<",
            Comparator::Le => "<=",
            Comparator::Gt => ">",
            Comparator::Ge => ">=",
            Comparator::Eq => "==",
            Comparator::Ne => "!=",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeAggregator {
    Avg,
    Sum,
    Min,
    Max,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeMeasure {
    Absolute,
    Percent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChangeSpec {
    pub measure: ChangeMeasure,
    /// How far back the compared value is: `5m_ago`, or `last_5m`.
    pub shift: Window,
}

pub use ninjapaws_anomaly::Direction;

#[derive(Clone, Debug, PartialEq)]
pub struct AnomalySpec {
    pub algorithm: Anomalies,
    pub bounds: f64,
    pub direction: Direction,
    /// The window whose buckets are judged; the outer window only sizes graphs.
    pub alert_window: Window,
    pub interval: Option<TimeDelta>,
    /// Empty buckets of a count read as 0 rather than missing.
    pub count_default_zero: bool,
    /// Accepted and not acted on: the engine's seasons are fixed-length.
    pub timezone: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ForecastSpec {
    pub method: Forecast,
    pub deviations: f64,
    pub interval: Option<TimeDelta>,
    pub history: Option<TimeDelta>,
    pub timezone: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutlierSpec {
    pub algorithm: Outliers,
    pub tolerance: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Detection {
    Threshold,
    Change(ChangeSpec),
    Anomaly(AnomalySpec),
    Forecast(ForecastSpec),
    Outlier(OutlierSpec),
}

/// `avg(last_5m):avg:system.cpu.user{*} by {host} > 90`, read.
#[derive(Clone, Debug, PartialEq)]
pub struct MonitorQuery {
    pub aggregator: TimeAggregator,
    /// `last_…`; for a forecast, the horizon `next_…`.
    pub window: Window,
    pub detection: Detection,
    /// Everything between the colon and the comparator, as written.
    pub expression: Expr<MetricQuery>,
    /// What is measured: the expression itself, or the query inside anomalies(), forecast() or
    /// outliers().
    pub measured: Expr<MetricQuery>,
    /// The `by` keys, sorted. Empty: a simple alert, one group.
    pub group_by: Vec<String>,
    pub as_count: bool,
    pub comparator: Comparator,
    pub threshold: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Thresholds {
    pub critical: Option<f64>,
    pub warning: Option<f64>,
    pub critical_recovery: Option<f64>,
    pub warning_recovery: Option<f64>,
    /// A service check's: consecutive OK runs to resolve.
    pub ok: Option<f64>,
    /// A service check's: consecutive UNKNOWN runs that put a group in No Data.
    pub unknown: Option<f64>,
}

/// on_missing_data: what a judged group with no point in the window becomes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissingData {
    /// A threshold over a sum or `.as_count()` judges it as 0; otherwise it keeps its state.
    Default,
    ShowNoData,
    ShowAndNotifyNoData,
    Resolve,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub thresholds: Thresholds,
    pub trigger_window: Option<Window>,
    pub recovery_window: Option<Window>,
    pub evaluation_delay: TimeDelta,
    pub new_group_delay: TimeDelta,
    pub require_full_window: bool,
    pub notify_no_data: bool,
    /// None: twice the window.
    pub no_data_timeframe: Option<TimeDelta>,
    /// None: notify_no_data and no_data_timeframe decide instead.
    pub on_missing_data: Option<MissingData>,
    /// timeout_h; None when absent or 0.
    pub timeout: Option<TimeDelta>,
    pub not_acted_on: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Definition {
    pub query: MonitorQuery,
    pub options: Options,
}

/// `"postgres.can_connect".over("env:prod").by("host").last(3).count_by_status()`, read.
#[derive(Clone, Debug, PartialEq)]
pub struct CheckQuery {
    pub check: String,
    /// over() and exclude() as one filter; All for "*".
    pub filter: TagFilter,
    /// The `by` keys, sorted. Empty: one group, `*`; `["*"]`: one group per series.
    pub group_by: Vec<String>,
    pub last: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CheckDefinition {
    pub query: CheckQuery,
    pub options: Options,
}

// One per definition read, so the size of the larger variant does not matter.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Monitor {
    Metric(Definition),
    ServiceCheck(CheckDefinition),
}

impl Monitor {
    pub fn options(&self) -> &Options {
        match self {
            Monitor::Metric(d) => &d.options,
            Monitor::ServiceCheck(c) => &c.options,
        }
    }
}
