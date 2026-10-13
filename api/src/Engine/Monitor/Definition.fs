/// A metric monitor as Datadog defines one: its query, read into what each
/// detection method needs, and the options the evaluation acts on.
module NinjaCat.Api.Engine.Monitor.Definition

open System
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Query

type WindowUnit =
    | Minutes
    | Hours
    | Days
    | Weeks
    | Months

/// `last_5m`, `next_1w`, `1h_ago`: a count of one unit. The unit is kept
/// because require_full_window sizes its buckets by it.
type Window =
    { Count: int
      Unit: WindowUnit }

    member w.Length =
        match w.Unit with
        | Minutes -> TimeSpan.FromMinutes(float w.Count)
        | Hours -> TimeSpan.FromHours(float w.Count)
        | Days -> TimeSpan.FromDays(float w.Count)
        | Weeks -> TimeSpan.FromDays(7.0 * float w.Count)
        // WARNING(undocumented): how long Datadog's month is. 31 days, so
        // last_1mo covers every calendar month.
        | Months -> TimeSpan.FromDays(31.0 * float w.Count)

    /// `15m`, `1mo`: the count and unit as Datadog writes them.
    member w.Text =
        let unit =
            match w.Unit with
            | Minutes -> "m"
            | Hours -> "h"
            | Days -> "d"
            | Weeks -> "w"
            | Months -> "mo"

        $"{w.Count}{unit}"

type Comparator =
    | Lt
    | Le
    | Gt
    | Ge
    | Eq
    | Ne

    member c.Text =
        match c with
        | Lt -> "<"
        | Le -> "<="
        | Gt -> ">"
        | Ge -> ">="
        | Eq -> "=="
        | Ne -> "!="

/// What reduces the window to one value per group.
type TimeAggregator =
    | Avg
    | Sum
    | Min
    | Max

/// `change(…)` or `pct_change(…)`.
type ChangeMeasure =
    | Absolute
    | Percent

type ChangeSpec =
    { Measure: ChangeMeasure
      /// How far back the compared value is: `5m_ago`, or `last_5m`.
      Shift: Window }

type Direction =
    | Above
    | Below
    | Both

type AnomalySpec =
    { Algorithm: Plan.AnomalyAlgorithm
      Bounds: float
      Direction: Direction
      /// The window whose buckets are judged; the outer window only sizes graphs.
      AlertWindow: Window
      Interval: TimeSpan option
      /// Empty buckets of a count read as 0 rather than missing.
      CountDefaultZero: bool
      /// Accepted and not acted on: the engine's seasons are fixed-length.
      Timezone: string option }

type ForecastSpec =
    { Method: Plan.ForecastMethod
      Deviations: float
      Interval: TimeSpan option
      History: TimeSpan option
      /// Accepted and not acted on, as for anomalies.
      Timezone: string option }

type OutlierSpec =
    { Algorithm: Plan.OutlierAlgorithm
      Tolerance: float }

type Detection =
    | Threshold
    | Change of ChangeSpec
    | Anomaly of AnomalySpec
    | Forecast of ForecastSpec
    | Outlier of OutlierSpec

/// `avg(last_5m):avg:system.cpu.user{*} by {host} > 90`, read.
type MonitorQuery =
    { Aggregator: TimeAggregator
      /// `last_…`; for a forecast, the horizon `next_…`.
      Window: Window
      Detection: Detection
      /// Everything between the colon and the comparator, as written.
      Expression: Expr<MetricQuery>
      /// What is measured: the expression itself, or the query inside
      /// anomalies(), forecast() or outliers().
      Measured: Expr<MetricQuery>
      /// The `by` keys, sorted. Empty: a simple alert, one group.
      GroupBy: string list
      AsCount: bool
      Comparator: Comparator
      Threshold: float }

type Thresholds =
    { Critical: float option
      Warning: float option
      CriticalRecovery: float option
      WarningRecovery: float option }

/// on_missing_data: what a judged group with no point in the window becomes.
[<RequireQualifiedAccess>]
type MissingData =
    /// A threshold over a sum or `.as_count()` judges it as 0; otherwise it
    /// keeps its state.
    | Default
    | ShowNoData
    | ShowAndNotifyNoData
    | Resolve

/// The options evaluation acts on. Options Datadog has that are stored but
/// not acted on yet are only named, in NotActedOn.
type Options =
    { Thresholds: Thresholds
      TriggerWindow: Window option
      RecoveryWindow: Window option
      EvaluationDelay: TimeSpan
      NewGroupDelay: TimeSpan
      RequireFullWindow: bool
      NotifyNoData: bool
      /// None: twice the window.
      NoDataTimeframe: TimeSpan option
      /// None: notify_no_data and no_data_timeframe decide instead.
      OnMissingData: MissingData option
      /// timeout_h; None when absent or 0.
      Timeout: TimeSpan option
      NotActedOn: string list }

type Definition =
    { Query: MonitorQuery
      Options: Options }
