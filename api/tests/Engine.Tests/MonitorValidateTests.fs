module NinjaCat.Api.Engine.Tests.MonitorValidateTests

open System
open Xunit
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Engine.Monitor.Definition

let private now = DateTimeOffset(2026, 10, 13, 12, 0, 0, TimeSpan.Zero)

let private validateWith (retention: Plan.Retention) (query: string) (options: string) =
    Validate.definition retention now "query alert" query options

let private valid (query: string) (options: string) : Definition =
    match validateWith Plan.defaultRetention query options with
    | Ok d -> d
    | Error problems -> failwith $"refused:\n{String.Join('\n', problems)}"

let private problemsOf (result: Result<Definition, string list>) : string list =
    match result with
    | Ok d -> failwith $"expected problems, got {d}"
    | Error problems -> problems

let private mentions (fragment: string) (problems: string list) =
    Assert.True(problems |> List.exists (fun p -> p.Contains fragment), $"no problem mentions '{fragment}':\n{String.Join('\n', problems)}")

let private refused (query: string) (options: string) (fragment: string) =
    validateWith Plan.defaultRetention query options |> problemsOf |> mentions fragment

let private above90 = "avg(last_5m):avg:system.cpu.user{*} by {host} > 90"

// --- options --------------------------------------------------------------------------

[<Fact>]
let ``options as Datadog exports them`` () =
    let d =
        valid
            above90
            """{"thresholds":{"critical":90,"warning":80,"critical_recovery":70,"warning_recovery":60},
                "notify_no_data":true,"no_data_timeframe":10,"evaluation_delay":60,"new_host_delay":300,
                "require_full_window":false,"timeout_h":2,"include_tags":true,"notify_audit":false,
                "renotify_interval":0,"silenced":{},"locked":false,"escalation_message":null}"""

    let o = d.Options
    Assert.Equal(Some 80.0, o.Thresholds.Warning)
    Assert.Equal(Some 70.0, o.Thresholds.CriticalRecovery)
    Assert.Equal(Some 60.0, o.Thresholds.WarningRecovery)
    Assert.True o.NotifyNoData
    Assert.Equal(Some(TimeSpan.FromMinutes 10.0), o.NoDataTimeframe)
    Assert.Equal(None, o.OnMissingData)
    Assert.Equal(TimeSpan.FromSeconds 60.0, o.EvaluationDelay)
    Assert.Equal(TimeSpan.FromSeconds 300.0, o.NewGroupDelay)
    Assert.Equal(Some(TimeSpan.FromHours 2.0), o.Timeout)
    Assert.Equal<string list>([ "include_tags"; "notify_audit"; "renotify_interval"; "silenced"; "locked" ], o.NotActedOn)

[<Fact>]
let ``no options: the defaults, critical from the query`` () =
    for options in [ ""; "null"; "{}" ] do
        let o = (valid above90 options).Options
        Assert.Equal(Some 90.0, o.Thresholds.Critical)
        Assert.Equal(TimeSpan.FromSeconds 60.0, o.NewGroupDelay)
        Assert.Equal(TimeSpan.Zero, o.EvaluationDelay)
        Assert.False o.RequireFullWindow
        Assert.False o.NotifyNoData
        Assert.Equal(None, o.NoDataTimeframe)
        Assert.Equal(None, o.Timeout)
        Assert.Empty o.NotActedOn

[<Fact>]
let ``options this version does not act on are accepted and named`` () =
    // Other products' options, and a misspelling, which the panel then shows as ignored.
    let o =
        (valid above90 """{"groupby_simple_monitor":false,"enable_logs_sample":true,"notify_nodata":true,"min_failure_duration":0}""").Options

    Assert.Equal<string list>([ "groupby_simple_monitor"; "enable_logs_sample"; "notify_nodata"; "min_failure_duration" ], o.NotActedOn)

[<Fact>]
let ``require_full_window is read`` () =
    Assert.True (valid above90 """{"require_full_window":true}""").Options.RequireFullWindow

[<Fact>]
let ``new_group_delay wins over the deprecated new_host_delay`` () =
    let o = (valid above90 """{"new_group_delay":120,"new_host_delay":300}""").Options
    Assert.Equal(TimeSpan.FromSeconds 120.0, o.NewGroupDelay)

[<Fact>]
let ``timeout_h 0 is no timeout`` () =
    Assert.Equal(None, (valid above90 """{"timeout_h":0}""").Options.Timeout)

[<Theory>]
[<InlineData("""{"variables":[{"data_source":"metrics","name":"query1","query":"avg:x{*}"}]}""", "options.variables")>]
[<InlineData("""{"new_group_delay":86401}""", "new_group_delay must be a whole number from 0 to 86400")>]
[<InlineData("""{"new_group_delay":-1}""", "new_group_delay must be a whole number from 0 to 86400")>]
[<InlineData("""{"thresholds":{"critical":1e999}}""", "thresholds.critical is too large a number")>]
[<InlineData("""{"threshold_windows":{"trigger_window":"last_99999999d"}}""", "trigger_window: 'last_99999999d' is not a window")>]
[<InlineData("""{"scheduling_options":{"evaluation_window":{"day_starts":"04:00"}}}""", "custom schedules")>]
[<InlineData("""{"evaluation_delay":90000}""", "evaluation_delay must be a whole number from 0 to 86400")>]
[<InlineData("""{"evaluation_delay":1.5}""", "evaluation_delay must be a whole number")>]
[<InlineData("""{"timeout_h":25}""", "timeout_h must be a whole number from 0 to 24")>]
[<InlineData("""{"no_data_timeframe":0}""", "no_data_timeframe must be a whole number of minutes, at least 1")>]
[<InlineData("""{"notify_no_data":"yes"}""", "notify_no_data must be true or false")>]
[<InlineData("""{"thresholds":{"critical":90,"ok":1}}""", "options.thresholds has no 'ok'")>]
[<InlineData("""{"thresholds":{"critical":"90"}}""", "thresholds.critical must be a number")>]
[<InlineData("""{"thresholds":[]}""", "thresholds must be an object")>]
[<InlineData("""[1]""", "options must be an object")>]
[<InlineData("""{"thresholds":""", "not valid JSON")>]
let ``options refused`` (options: string, fragment: string) = refused above90 options fragment

[<Theory>]
[<InlineData("default")>]
[<InlineData("show_no_data")>]
[<InlineData("show_and_notify_no_data")>]
[<InlineData("resolve")>]
let ``on_missing_data is read`` (rule: string) =
    let o = (valid above90 $"""{{"on_missing_data":"{rule}","notify_no_data":false}}""").Options

    let expected =
        match rule with
        | "default" -> MissingData.Default
        | "show_no_data" -> MissingData.ShowNoData
        | "show_and_notify_no_data" -> MissingData.ShowAndNotifyNoData
        | _ -> MissingData.Resolve

    Assert.Equal(Some expected, o.OnMissingData)
    Assert.Empty o.NotActedOn

[<Theory>]
[<InlineData("""{"on_missing_data":"notify"}""", "on_missing_data must be one of default, show_no_data")>]
[<InlineData("""{"on_missing_data":true}""", "on_missing_data must be one of")>]
[<InlineData("""{"on_missing_data":"resolve","notify_no_data":true}""", "on_missing_data replaces notify_no_data and no_data_timeframe")>]
[<InlineData("""{"on_missing_data":"default","no_data_timeframe":10}""", "on_missing_data replaces notify_no_data and no_data_timeframe")>]
let ``on_missing_data refused`` (options: string, fragment: string) = refused above90 options fragment

[<Theory>]
[<InlineData("show_no_data")>]
[<InlineData("show_and_notify_no_data")>]
[<InlineData("resolve")>]
let ``with default_zero only the default on_missing_data`` (rule: string) =
    let query = "sum(last_5m):default_zero(sum:requests{*} by {host}) < 10"
    refused query $"""{{"on_missing_data":"{rule}"}}""" "on_missing_data must be default when the query has default_zero()"
    Assert.Equal(Some MissingData.Default, (valid query """{"on_missing_data":"default"}""").Options.OnMissingData)

[<Fact>]
let ``several problems are reported together`` () =
    let problems = validateWith Plan.defaultRetention above90 """{"timeout_h":25,"evaluation_delay":-1}""" |> problemsOf
    mentions "timeout_h" problems
    mentions "evaluation_delay" problems

// --- the type -------------------------------------------------------------------------

[<Fact>]
let ``metric alert is the older name`` () =
    Assert.True((Validate.definition Plan.defaultRetention now "metric alert" above90 "").IsOk)

[<Fact>]
let ``other monitor types are refused`` () =
    Validate.definition Plan.defaultRetention now "service check" above90 ""
    |> problemsOf
    |> mentions "'service check' is not supported"

// --- thresholds -----------------------------------------------------------------------

[<Fact>]
let ``critical must equal the query's`` () =
    refused above90 """{"thresholds":{"critical":95}}""" "must equal the query's threshold (90)"

[<Theory>]
[<InlineData(">", 90.0, 80.0, true, "less strict than critical")>]
[<InlineData(">", 90.0, 90.0, false, "less strict than critical")>]
[<InlineData(">", 90.0, 95.0, false, "less strict than critical")>]
[<InlineData("<", 10.0, 20.0, true, "so above it")>]
[<InlineData("<", 10.0, 10.0, false, "so above it")>]
[<InlineData("<", 10.0, 5.0, false, "so above it")>]
let ``warning less strict than critical`` (comparator: string, critical: float, warning: float, ok: bool, fragment: string) =
    let query = $"avg(last_5m):avg:x{{*}} by {{host}} {comparator} {critical}"
    let result = validateWith Plan.defaultRetention query $$$"""{"thresholds":{"warning":{{{warning}}}}}"""
    if ok then Assert.True result.IsOk else problemsOf result |> mentions fragment

[<Theory>]
[<InlineData(">", 89.0, true)>]
[<InlineData(">", 90.0, false)>]
[<InlineData(">=", 90.0, false)>]
[<InlineData("<", 91.0, true)>]
[<InlineData("<", 90.0, false)>]
[<InlineData("<=", 85.0, false)>]
let ``critical recovery strictly on the recovery side`` (comparator: string, recovery: float, ok: bool) =
    let query = $"avg(last_5m):avg:x{{*}} {comparator} 90"
    let result = validateWith Plan.defaultRetention query $$$"""{"thresholds":{"critical_recovery":{{{recovery}}}}}"""
    if ok then Assert.True result.IsOk else problemsOf result |> mentions "on the side the monitor recovers to"

[<Fact>]
let ``warning recovery is checked against warning`` () =
    refused above90 """{"thresholds":{"warning":80,"warning_recovery":85}}""" "warning_recovery (85) must be below warning (80)"

[<Fact>]
let ``warning recovery needs a warning`` () =
    refused above90 """{"thresholds":{"warning_recovery":70}}""" "needs a warning threshold"

[<Theory>]
[<InlineData("==")>]
[<InlineData("!=")>]
let ``no recovery for equality`` (comparator: string) =
    refused $"avg(last_5m):avg:x{{*}} {comparator} 1" """{"thresholds":{"critical_recovery":0}}""" "do not apply"

[<Fact>]
let ``an equality warning must differ`` () =
    refused "avg(last_5m):avg:x{*} == 1" """{"thresholds":{"warning":1}}""" "must differ from critical"
    Assert.True((validateWith Plan.defaultRetention "avg(last_5m):avg:x{*} == 1" """{"thresholds":{"warning":2}}""").IsOk)

[<Fact>]
let ``threshold_windows are for anomalies`` () =
    refused above90 """{"threshold_windows":{"trigger_window":"last_5m"}}""" "for anomaly monitors"

[<Fact>]
let ``outliers take no warning`` () =
    refused "avg(last_30m):outliers(avg:x{*} by {host}, 'dbscan', 3) > 0" """{"thresholds":{"warning":0}}""" "no warning or recovery"

// --- anomaly --------------------------------------------------------------------------

let private anomaly (outer: string) (alert: string) (interval: int) (threshold: string) =
    $"avg({outer}):anomalies(avg:system.cpu.user{{*}} by {{host}}, 'agile', 2, direction='above', alert_window='{alert}', interval={interval}) >= {threshold}"

let private windows (trigger: string) (recovery: string) (thresholds: string) =
    $$$"""{"thresholds":{{{{thresholds}}}},"threshold_windows":{"trigger_window":"{{{trigger}}}","recovery_window":"{{{recovery}}}"}}"""

[<Fact>]
let ``anomaly as Datadog writes it`` () =
    let d = valid (anomaly "last_4h" "last_15m" 60 "1") (windows "last_15m" "last_15m" "\"critical\":1,\"critical_recovery\":0")
    Assert.Equal(Some { Count = 15; Unit = Minutes }, d.Options.TriggerWindow)

[<Fact>]
let ``anomaly without threshold_windows takes alert_window`` () =
    valid (anomaly "last_4h" "last_15m" 60 "1") "" |> ignore

[<Fact>]
let ``the trigger window is alert_window`` () =
    refused (anomaly "last_4h" "last_15m" 60 "1") (windows "last_30m" "last_15m" "\"critical\":1") "must equal anomalies() alert_window"

[<Fact>]
let ``the window holds alert_window`` () =
    refused (anomaly "last_10m" "last_15m" 60 "1") "" "must be at least alert_window"

[<Fact>]
let ``the interval is at most a fifth of alert_window`` () =
    valid (anomaly "last_4h" "last_15m" 180 "1") "" |> ignore
    refused (anomaly "last_4h" "last_15m" 181 "1") "" "at most a fifth of alert_window"

[<Fact>]
let ``the recovery window is not longer than the trigger window`` () =
    refused (anomaly "last_4h" "last_15m" 60 "1") (windows "last_15m" "last_30m" "\"critical\":1") "must not be longer than the trigger window"

/// Datadog's documented case: T = 15 m, c = 0.8, no recovery threshold, so
/// the recovery window must exceed 15 · (1 − 0.8) = 3 minutes. With a
/// recovery threshold r = 0.5: 15 · (1 − 0.8) / (1 − 0.5) = 6 minutes.
[<Theory>]
[<InlineData("last_3m", "", false, "3 minutes")>]
[<InlineData("last_4m", "", true, "3 minutes")>]
[<InlineData("last_6m", ",\"critical_recovery\":0.5", false, "6 minutes")>]
[<InlineData("last_7m", ",\"critical_recovery\":0.5", true, "6 minutes")>]
let ``the recovery window's lower bound`` (recovery: string, recoveryThreshold: string, ok: bool, bound: string) =
    let result =
        validateWith
            Plan.defaultRetention
            (anomaly "last_4h" "last_15m" 60 "0.8")
            (windows "last_15m" recovery ("\"critical\":0.8" + recoveryThreshold))

    if ok then Assert.True result.IsOk else problemsOf result |> mentions $"must be longer than {bound}"

/// A recovery threshold of 1 is a problem, not a crash.
[<Theory>]
[<InlineData("""{"thresholds":{"critical":0.5,"critical_recovery":1},"threshold_windows":{"trigger_window":"last_15m","recovery_window":"last_15m"}}""")>]
[<InlineData("""{"thresholds":{"critical_recovery":0.5}}""")>]
let ``anomaly recovery below critical`` (options: string) =
    refused (anomaly "last_4h" "last_15m" 60 "0.5") options "on the side the monitor recovers to"

// --- forecast -------------------------------------------------------------------------

[<Fact>]
let ``a forecast's threshold_windows are stored, not acted on`` () =
    let d =
        valid
            "max(next_1w):forecast(avg:system.load.1{*}, 'linear', 1, interval='60m', history='1w', model='default') >= 3"
            """{"thresholds":{"critical":3,"critical_recovery":2},"threshold_windows":{"trigger_window":"last_1h","recovery_window":"last_1h"}}"""

    Assert.Contains("threshold_windows", d.Options.NotActedOn)

// --- retention ------------------------------------------------------------------------

[<Theory>]
[<InlineData(7.0, "avg(last_2w):avg:x{*} > 1", "", "metrics are kept 7 days")>]
[<InlineData(3.0, "change(avg(last_2d),2d_ago):avg:x{*} > 1", "", "reads 4 days back")>]
[<InlineData(7.0, "max(next_1w):forecast(avg:x{*}, 'linear', 1, history='2w') >= 3", "", "reads 14 days back")>]
[<InlineData(400.0, "max(next_1w):forecast(avg:x{*}, 'linear', 1, history='999999w') >= 3", "", "metrics are kept 400 days")>]
[<InlineData(7.0, "max(next_1w):forecast(avg:x{*}, 'seasonal', 1) >= 3", "", "two seasons")>]
[<InlineData(7.0, "avg(last_5m):avg:x{*} > 1", """{"no_data_timeframe":20160}""", "no_data_timeframe")>]
let ``beyond the rollup's retention`` (rollupDays: float, query: string, options: string, fragment: string) =
    let retention: Plan.Retention =
        { Raw = TimeSpan.FromDays 2.0
          Rollup = TimeSpan.FromDays rollupDays }

    validateWith retention query options |> problemsOf |> mentions fragment

// --- the engine plans the query -------------------------------------------------------

[<Theory>]
[<InlineData("avg(last_5m):nosuchfn(avg:x{*}) > 1", "'nosuchfn' is not supported yet")>]
[<InlineData("avg(last_5m):avg:x{*}.rollup() > 1", ".rollup() needs a method")>]
[<InlineData("avg(last_5m):avg:a{*} by {host} / avg:b{*} by {env} > 1", "grouped by the same tags")>]
let ``what the engine cannot plan`` (query: string, fragment: string) = refused query "" fragment

[<Fact>]
let ``every type plans`` () =
    for query in
        [ above90
          "change(avg(last_5m),5m_ago):avg:x{*} by {host} > 10"
          "pct_change(sum(last_1h),1d_ago):sum:x{*}.as_count() < -50"
          anomaly "last_4h" "last_15m" 60 "1"
          "max(next_1w):forecast(avg:x{*}, 'seasonal', 1, seasonality='daily') >= 3"
          "avg(last_30m):outliers(avg:x{*} by {host}, 'scaledmad', 3, 20) > 0" ] do
        valid query "" |> ignore

// --- a monitor as JSON ------------------------------------------------------------------

let private monitorJson (json: string) = Validate.monitor Plan.defaultRetention now json

[<Fact>]
let ``a monitor's JSON is checked by its type, query and options`` () =
    let json =
        """{"name":"cpu","type":"query alert","query":"avg(last_5m):avg:system.cpu.user{*} by {host} > 90",
            "message":"@ops","tags":["team:a"],"options":{"thresholds":{"warning":80},"renotify_interval":60}}"""

    match monitorJson json with
    | Ok d ->
        Assert.Equal(Some 90.0, d.Options.Thresholds.Critical)
        Assert.Contains("renotify_interval", d.Options.NotActedOn)
    | Error problems -> failwith (String.Join('\n', problems))

    Assert.True((monitorJson """{"type":"metric alert","query":"avg(last_5m):avg:x{*} > 1","options":null}""").IsOk)

[<Fact>]
let ``JSON that is no monitor is a problem`` () =
    monitorJson "{" |> problemsOf |> mentions "not valid JSON"
    monitorJson "[]" |> problemsOf |> mentions "must be a JSON object"

    let both = monitorJson """{"type":1}""" |> problemsOf
    mentions "type must be a string" both
    mentions "query must be a string" both

    monitorJson """{"type":"query alert","query":"avg(last_5m):avg:x{*} > 1","options":{"thresholds":{"critical":2}}}"""
    |> problemsOf
    |> mentions "must equal the query's threshold"
