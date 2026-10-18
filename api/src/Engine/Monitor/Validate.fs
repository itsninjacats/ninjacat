/// A monitor definition checked whole; the query process and the monitor service both run it.
module NinjaCat.Api.Engine.Monitor.Validate

open System
open System.Globalization
open System.Text.Json
open NinjaCat.Api.Engine.Api.V2.Timeseries
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor.Definition

/// Whether threshold `a` is reached after `b` on the way to alerting.
let private stricter (c: Comparator) (a: float) (b: float) =
    match c with
    | Gt
    | Ge -> a > b
    | Lt
    | Le -> a < b
    | Eq
    | Ne -> false

/// Where the less strict side of a threshold is.
let private laxSide (c: Comparator) =
    match c with
    | Gt
    | Ge -> "below"
    | Lt
    | Le
    | Eq
    | Ne -> "above"

let private warningProblems (q: MonitorQuery) (t: Thresholds) : string list =
    let critical = q.Threshold

    match q.Comparator, t.Warning with
    // WARNING(undocumented): "less strict" has no direction for == and !=;
    // the warning only has to differ from critical.
    | (Eq | Ne), Some w when w = critical -> [ $"options.thresholds.warning ({w}) must differ from critical ({critical})" ]
    | (Gt | Ge | Lt | Le) as c, Some w when not (stricter c critical w) ->
        [ $"options.thresholds.warning ({w}) must be less strict than critical ({critical}), so {laxSide c} it" ]
    | _ -> []

/// WARNING(undocumented): Datadog does not say whether a recovery threshold
/// may equal the one it recovers from. Ours must sit strictly on the recovery
/// side; equal would be the same as having none.
let private recoveryProblems (q: MonitorQuery) (t: Thresholds) : string list =
    let c = q.Comparator

    let recovery (name: string) (value: float option) (from: string) (threshold: float) =
        [ match value with
          | Some r when not (stricter c threshold r) ->
              $"options.thresholds.{name} ({r}) must be {laxSide c} {from} ({threshold}), on the side the monitor recovers to"
          | _ -> () ]

    match c, t.CriticalRecovery, t.WarningRecovery with
    | (Eq | Ne), None, None -> []
    | (Eq | Ne), _, _ -> [ $"recovery thresholds do not apply to {c.Text}: the monitor recovers when the condition stops holding" ]
    | _ ->
        recovery "critical_recovery" t.CriticalRecovery "critical" q.Threshold
        @ (match t.WarningRecovery, t.Warning with
           | Some _, None -> [ "options.thresholds.warning_recovery needs a warning threshold" ]
           | wr, Some w -> recovery "warning_recovery" wr "warning" w
           | None, None -> [])

let private thresholdProblems (q: MonitorQuery) (t: Thresholds) : string list =
    let critical =
        [ match t.Critical with
          | Some x when x <> q.Threshold -> $"options.thresholds.critical ({x}) must equal the query's threshold ({q.Threshold})"
          | _ -> () ]

    match q.Detection, t.Warning, t.CriticalRecovery, t.WarningRecovery with
    | Outlier _, None, None, None -> critical
    // WARNING(undocumented): an outlier's value is 1 or 0, so there is nothing
    // between alerting and not for a warning or a recovery to mark.
    | Outlier _, _, _, _ -> critical @ [ "an outlier monitor takes no warning or recovery thresholds" ]
    | _ -> critical @ warningProblems q t @ recoveryProblems q t

let private checkOnlyThresholds (t: Thresholds) : string list =
    [ if t.Ok.IsSome then
          "options.thresholds.ok is for service check monitors"
      if t.Unknown.IsSome then
          "options.thresholds.unknown is for service check monitors" ]

let private minutes (span: TimeSpan) = span.TotalMinutes.ToString("0.##", CultureInfo.InvariantCulture) + " minutes"

let private anomalyProblems (q: MonitorQuery) (a: AnomalySpec) (o: Options) : string list =
    let trigger = defaultArg o.TriggerWindow a.AlertWindow
    // WARNING(undocumented): without a recovery_window, the trigger window.
    let recovery = defaultArg o.RecoveryWindow trigger
    let c = q.Threshold
    let r = defaultArg o.Thresholds.CriticalRecovery 0.0
    // WARNING(undocumented): Datadog documents the bound for a recovery
    // threshold of 0, T·(1−c); generalised to T·(1−c)/(1−r). Only for r < c ≤ 1,
    // where it is finite.
    let lowest () = trigger.Length * ((1.0 - c) / (1.0 - r))

    [ if trigger.Length <> a.AlertWindow.Length then
          $"options.threshold_windows.trigger_window (last_{trigger.Text}) must equal anomalies() alert_window (last_{a.AlertWindow.Text})"
      if q.Window.Length < a.AlertWindow.Length then
          $"the monitor's window (last_{q.Window.Text}) must be at least alert_window (last_{a.AlertWindow.Text})"
      match a.Interval with
      | Some i when i > a.AlertWindow.Length / 5.0 ->
          $"anomalies() interval ({i.TotalSeconds} s) must be at most a fifth of alert_window (last_{a.AlertWindow.Text}), so the window holds at least 5 buckets"
      | _ -> ()
      if recovery.Length > trigger.Length then
          $"options.threshold_windows.recovery_window (last_{recovery.Text}) must not be longer than the trigger window (last_{trigger.Text})"
      elif r < c && recovery.Length <= lowest () then
          $"options.threshold_windows.recovery_window (last_{recovery.Text}) must be longer than {minutes (lowest ())}, or the monitor could recover while still anomalous"
      match o.Thresholds.Warning with
      | Some w when w <= 0.0 -> "options.thresholds.warning is a fraction of anomalous points, above 0"
      | _ -> ()
      for name, value in [ "critical_recovery", o.Thresholds.CriticalRecovery; "warning_recovery", o.Thresholds.WarningRecovery ] do
          match value with
          | Some v when v < 0.0 -> $"options.thresholds.{name} is a fraction of anomalous points, 0 or more"
          | _ -> () ]

let private days (span: TimeSpan) = span.TotalDays.ToString("0.##", CultureInfo.InvariantCulture) + " days"

/// What the metrics tables must still hold for the monitor to be evaluated.
let private retentionProblems (retention: Plan.Retention) (q: MonitorQuery) (o: Options) : string list =
    let kept = $"metrics are kept {days retention.Rollup}"

    let reach =
        match q.Detection with
        | Change c -> q.Window.Length + c.Shift.Length
        | Forecast f -> defaultArg f.History TimeSpan.Zero
        | Threshold
        | Anomaly _
        | Outlier _ -> q.Window.Length

    [ if reach > retention.Rollup then
          $"the monitor reads {days reach} back; {kept}"
      match q.Detection with
      | Forecast { Method = Plan.SeasonalForecast s } when s.Length * 2.0 > retention.Rollup ->
          $"a seasonal forecast needs two seasons of history ({days (s.Length * 2.0)}); {kept}"
      | _ -> ()
      match o.NoDataTimeframe with
      | Some timeframe when timeframe > retention.Rollup -> $"options.no_data_timeframe ({minutes timeframe}) is longer than {kept}"
      | _ -> () ]

let private windowProblems (q: MonitorQuery) (o: Options) : string list =
    match q.Detection, o.TriggerWindow, o.RecoveryWindow with
    | (Anomaly _ | Forecast _), _, _
    | _, None, None -> []
    | _ -> [ "options.threshold_windows is for anomaly monitors" ]

/// Datadog accepts only `default` beside default_zero(), which already
/// reads missing points as 0.
let private missingDataProblems (q: MonitorQuery) (o: Options) : string list =
    match o.OnMissingData with
    | None
    | Some MissingData.Default -> []
    | Some _ when List.contains "default_zero" (Parse.functionNames q.Expression) ->
        [ "options.on_missing_data must be default when the query has default_zero()" ]
    | Some _ -> []

/// Whether the engine can plan the query, as the evaluation will run it.
let private planProblems (retention: Plan.Retention) (now: DateTimeOffset) (q: MonitorQuery) : string list =
    let queries, formula = Engine.split q.Expression

    let lookback, interval =
        match q.Detection with
        | Forecast f -> defaultArg f.History q.Window.Length, f.Interval
        | Anomaly a -> q.Window.Length, a.Interval
        | Threshold
        | Change _
        | Outlier _ -> q.Window.Length, None

    // retentionProblems already refuses a lookback this long, and a far longer
    // one would not fit before `now`.
    if lookback > retention.Rollup then
        []
    else
        let request: ParsedTimeseriesRequest =
            { From = now - lookback
              To = now
              Interval = interval
              Queries = queries
              Formulas = [ { Formula = formula; Limit = None } ] }

        match Plan.plan retention now request with
        | Ok _ -> []
        | Error problems -> problems

/// `now` is the evaluation's time; the engine never reads the clock.
let definition (retention: Plan.Retention) (now: DateTimeOffset) (query: string) (options: string) : Result<Definition, string list> =
    match Parse.query query, Options.parse options with
    | Ok q, Ok o ->
        let problems =
            thresholdProblems q o.Thresholds
            @ checkOnlyThresholds o.Thresholds
            @ windowProblems q o
            @ missingDataProblems q o
            @ (match q.Detection with
               | Anomaly a -> anomalyProblems q a o
               | Threshold
               | Change _
               | Forecast _
               | Outlier _ -> [])
            @ retentionProblems retention q o
            @ planProblems retention now q

        let notActedOn =
            match q.Detection, o.TriggerWindow, o.RecoveryWindow with
            | Forecast _, Some _, _
            | Forecast _, _, Some _ -> o.NotActedOn @ [ "threshold_windows" ]
            | _ -> o.NotActedOn

        match problems with
        | [] ->
            Ok
                { Query = q
                  Options =
                    { o with
                        Thresholds = { o.Thresholds with Critical = Some q.Threshold }
                        NotActedOn = notActedOn } }
        | problems -> Error problems
    | q, o -> Error(Check.problemsOf q @ Check.problemsOf o)

/// A metric monitor or a service check monitor, by its type.
let byType (retention: Plan.Retention) (now: DateTimeOffset) (monitorType: string) (query: string) (options: string) : Result<Monitor, string list> =
    match monitorType with
    // Datadog still accepts the older name.
    | "query alert"
    | "metric alert" -> definition retention now query options |> Result.map Metric
    | "service check" -> Check.definition query options |> Result.map ServiceCheck
    | other -> Error [ $"monitor type '{other}' is not supported; a metric monitor is a 'query alert', a check monitor a 'service check'" ]

/// A monitor as Datadog's API takes it, as JSON; fields other than type, query and options are the panel's.
let monitor (retention: Plan.Retention) (now: DateTimeOffset) (json: string) : Result<Monitor, string list> =
    let text (root: JsonElement) (name: string) =
        match root.TryGetProperty name with
        | true, value when value.ValueKind = JsonValueKind.String -> Ok(value.GetString())
        | _ -> Error [ $"{name} must be a string" ]

    try
        use document = JsonDocument.Parse json
        let root = document.RootElement

        if root.ValueKind <> JsonValueKind.Object then
            Error [ "a monitor must be a JSON object" ]
        else
            let options =
                match root.TryGetProperty "options" with
                | true, value -> value.GetRawText()
                | false, _ -> ""

            match text root "type", text root "query" with
            | Ok monitorType, Ok query -> byType retention now monitorType query options
            | monitorType, query -> Error(Check.problemsOf monitorType @ Check.problemsOf query)
    with :? JsonException as e ->
        Error [ $"the monitor is not valid JSON: {e.Message}" ]
