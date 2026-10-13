module NinjaCat.Api.Engine.Tests.MonitorParseTests

open System
open Xunit
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Engine.Monitor.Definition

let private parsed (text: string) : MonitorQuery =
    match Parse.query text with
    | Ok q -> q
    | Error problems -> failwith $"'{text}' was refused:\n{String.Join('\n', problems)}"

let private refused (text: string) (fragment: string) =
    match Parse.query text with
    | Ok q -> Assert.Fail $"'{text}' should be refused, got {q}"
    | Error problems -> Assert.True(problems |> List.exists (fun p -> p.Contains fragment), $"no problem mentions '{fragment}':\n{String.Join('\n', problems)}")

let private leaf agg metric filter groups =
    Leaf { SpaceAgg = Some agg; Metric = metric; Filter = filter; GroupBy = groups; Modifiers = [] }

let private minutes n = { Count = n; Unit = Minutes }

// --- threshold ------------------------------------------------------------------------

[<Fact>]
let ``threshold, Datadog's create-a-monitor example`` () =
    let q = parsed "avg(last_5m):sum:system.net.bytes_rcvd{host:host0} > 100"

    Assert.Equal(Avg, q.Aggregator)
    Assert.Equal(minutes 5, q.Window)
    Assert.Equal(Threshold, q.Detection)
    Assert.Equal(leaf "sum" "system.net.bytes_rcvd" (Tag("host", "host0")) [], q.Expression)
    Assert.Equal(q.Expression, q.Measured)
    Assert.Empty q.GroupBy
    Assert.Equal(Gt, q.Comparator)
    Assert.Equal(100.0, q.Threshold)

[<Fact>]
let ``by makes a multi alert, its keys sorted`` () =
    let q = parsed "avg(last_1h):avg:system.cpu.user{env:prod} by {host,env} >= 90.5"

    Assert.Equal<string list>([ "env"; "host" ], q.GroupBy)
    Assert.Equal({ Count = 1; Unit = Hours }, q.Window)
    Assert.Equal(Ge, q.Comparator)
    Assert.Equal(90.5, q.Threshold)

[<Fact>]
let ``a formula of as_count queries with sum`` () =
    // Datadog's APM error-rate monitor.
    let q =
        parsed "sum(last_5m):sum:trace.http.request.errors{service:web}.as_count() / sum:trace.http.request.hits{service:web}.as_count() > 0.05"

    Assert.True q.AsCount
    Assert.Equal(Sum, q.Aggregator)

[<Fact>]
let ``no spaces, a negative threshold`` () =
    let q = parsed "min(last_1d):avg:x{*}<-5"
    Assert.Equal(Lt, q.Comparator)
    Assert.Equal(-5.0, q.Threshold)

[<Fact>]
let ``functions and filters the engine takes`` () =
    let q = parsed "max(last_15m):abs(diff(avg:x{env:prod AND NOT host:a} by {host})) != 0"
    Assert.Equal<string list>([ "host" ], q.GroupBy)
    Assert.Equal(Ne, q.Comparator)

[<Fact>]
let ``count functions leave one group`` () =
    let q = parsed "avg(last_5m):count_nonzero(avg:x{*} by {host}) < 3"
    Assert.Empty q.GroupBy

[<Theory>]
[<InlineData("last_1mo")>]
[<InlineData("last_31d")>]
[<InlineData("last_4w")>]
[<InlineData("last_44640m")>]
let ``windows up to a month`` (window: string) =
    parsed $"avg({window}):avg:x{{*}} > 1" |> ignore

[<Theory>]
[<InlineData("last_5w", "longer than last_1mo")>]
[<InlineData("last_32d", "longer than last_1mo")>]
[<InlineData("last_2mo", "only month window")>]
[<InlineData("current_1d", "cumulative windows")>]
[<InlineData("next_1w", "forecast horizon")>]
[<InlineData("5m", "is not a window")>]
[<InlineData("last_0m", "is not a window")>]
let ``windows refused`` (window: string, fragment: string) =
    refused $"avg({window}):avg:x{{*}} > 1" fragment

[<Fact>]
let ``percentile is refused`` () =
    refused "percentile(last_5m):p99:request.latency{*} > 1" "percentile"

[<Fact>]
let ``an unknown time aggregation is refused`` () =
    refused "last(last_5m):avg:x{*} > 1" "unknown time aggregation 'last'"

[<Theory>]
[<InlineData("avg")>]
[<InlineData("min")>]
[<InlineData("max")>]
let ``as_count takes only sum`` (agg: string) =
    refused $"{agg}(last_5m):sum:requests{{*}}.as_count() > 1" "must be sum"

[<Fact>]
let ``a missing comparator is a syntax error`` () =
    refused "avg(last_5m):avg:x{*}" "comparator"

[<Fact>]
let ``outliers inside arithmetic is refused`` () =
    refused "avg(last_5m):outliers(avg:x{*} by {host}, 'dbscan', 3) * 2 > 0" "outermost"

// --- change ---------------------------------------------------------------------------

[<Fact>]
let ``change over the window before`` () =
    let q = parsed "change(avg(last_5m),last_5m):avg:system.load.1{*} by {host} > 10"

    Assert.Equal(Change { Measure = Absolute; Shift = minutes 5 }, q.Detection)
    Assert.Equal(minutes 5, q.Window)
    Assert.Equal(Avg, q.Aggregator)
    Assert.Equal<string list>([ "host" ], q.GroupBy)

[<Fact>]
let ``pct_change with an _ago shift`` () =
    let q = parsed "pct_change(avg(last_5m),1h_ago):avg:system.load.1{*} < -50"
    Assert.Equal(Change { Measure = Percent; Shift = { Count = 1; Unit = Hours } }, q.Detection)
    Assert.Equal(-50.0, q.Threshold)

[<Theory>]
[<InlineData("5m_ago")>]
[<InlineData("30m_ago")>]
[<InlineData("4h_ago")>]
[<InlineData("1d_ago")>]
[<InlineData("last_1h")>]
let ``documented shifts`` (shift: string) =
    parsed $"change(sum(last_1h),{shift}):sum:x{{*}}.as_count() > 1" |> ignore

[<Theory>]
[<InlineData("change(avg(last_3d),5m_ago)", "two days")>]
[<InlineData("change(avg(last_2881m),5m_ago)", "two days")>]
[<InlineData("change(avg(last_1w),5m_ago)", "two days")>]
[<InlineData("change(avg(last_5m),3d_ago)", "two days")>]
[<InlineData("change(avg(last_5m),yesterday)", "is not a window")>]
[<InlineData("change(avg(last_5m))", "takes a time aggregation and a timeshift")>]
[<InlineData("pct_change(last_5m,5m_ago)", "takes a time aggregation and a timeshift")>]
let ``change refusals`` (head: string, fragment: string) =
    refused $"{head}:avg:x{{*}} > 1" fragment

[<Fact>]
let ``change of an anomaly is refused`` () =
    refused "change(avg(last_5m),5m_ago):anomalies(avg:x{*}, 'basic', 2, alert_window='last_5m') >= 1" "cannot be combined"

// --- anomaly --------------------------------------------------------------------------

[<Fact>]
let ``basic anomaly, Datadog's API example`` () =
    let q =
        parsed
            "avg(last_1h):anomalies(avg:system.cpu.system{name:cassandra}, 'basic', 3, direction='above', alert_window='last_5m', interval=20, count_default_zero='true') >= 1"

    Assert.Equal(leaf "avg" "system.cpu.system" (Tag("name", "cassandra")) [], q.Measured)

    Assert.Equal(
        Anomaly
            { Algorithm = Plan.Basic
              Bounds = 3.0
              Direction = Above
              AlertWindow = minutes 5
              Interval = Some(TimeSpan.FromSeconds 20.0)
              CountDefaultZero = true
              Timezone = None },
        q.Detection
    )

    Assert.Equal({ Count = 1; Unit = Hours }, q.Window)

[<Fact>]
let ``agile anomaly with every argument, the plan's example`` () =
    let q =
        parsed
            "avg(last_4h):anomalies(avg:system.cpu.user{*} by {host}, 'agile', 2, direction='above', alert_window='last_15m', interval=60, count_default_zero='true', seasonality='weekly', timezone='Europe/Warsaw') >= 1"

    match q.Detection with
    | Anomaly a ->
        Assert.Equal(Plan.Agile Plan.Weekly, a.Algorithm)
        Assert.Equal(Some "Europe/Warsaw", a.Timezone)
        Assert.Equal(minutes 15, a.AlertWindow)
    | other -> Assert.Fail $"expected an anomaly, got {other}"

    Assert.Equal<string list>([ "host" ], q.GroupBy)

[<Fact>]
let ``anomaly defaults`` () =
    match (parsed "avg(last_1h):anomalies(avg:x{*}, 'robust', 2, alert_window='last_10m') >= 0.5").Detection with
    | Anomaly a ->
        Assert.Equal(Plan.Robust Plan.Weekly, a.Algorithm)
        Assert.Equal(Both, a.Direction)
        Assert.Equal(None, a.Interval)
        Assert.False a.CountDefaultZero
    | other -> Assert.Fail $"expected an anomaly, got {other}"

[<Theory>]
[<InlineData("anomalies(avg:x{*}, 'adaptive', 2, alert_window='last_15m') >= 1", "legacy")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2) >= 1", "needs alert_window")>]
[<InlineData("anomalies(avg:x{*}, 'basic', 2, alert_window='last_15m', seasonality='daily') >= 1", "'basic' has no seasonality")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', seasonality='monthly') >= 1", "seasonality must be")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 0, alert_window='last_15m') >= 1", "bounds above 0")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', direction='up') >= 1", "direction must be")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', foo=1) >= 1", "has no argument 'foo'")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', direction='above', direction='below') >= 1", "more than once")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', interval=1.5) >= 1", "whole number of seconds")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', count_default_zero='yes') >= 1", "'true' or 'false'")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m', timezone='Mars/Olympus') >= 1", "not a known IANA zone")>]
[<InlineData("anomalies(sum:x{*}.as_count(), 'agile', 2, alert_window='last_15m') >= 1", "do not take .as_count()")>]
[<InlineData("anomalies(cumsum(avg:x{*}), 'agile', 2, alert_window='last_15m') >= 1", "cumsum() is not allowed inside anomalies()")>]
[<InlineData("anomalies(outliers(avg:x{*} by {host}, 'dbscan', 3), 'agile', 2, alert_window='last_15m') >= 1", "outliers() is not allowed inside anomalies()")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m') > 1", "compares with >=")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2, alert_window='last_15m') >= 2", "at most 1")>]
[<InlineData("anomalies(avg:x{*}, 'agile', 2, alert_window='15m') >= 1", "alert_window '15m' is not a window")>]
let ``anomaly refusals`` (expression: string, fragment: string) =
    refused $"avg(last_4h):{expression}" fragment

// --- forecast -------------------------------------------------------------------------

[<Fact>]
let ``linear forecast, Datadog's API example`` () =
    let q =
        parsed "max(next_1w):forecast(avg:system.load.1{*}, 'linear', 1, interval='60m', history='1w', model='default') >= 3"

    Assert.Equal(Max, q.Aggregator)
    Assert.Equal({ Count = 1; Unit = Weeks }, q.Window)
    Assert.Equal(leaf "avg" "system.load.1" All [], q.Measured)

    Assert.Equal(
        Forecast
            { Method = Plan.LinearForecast Plan.DefaultModel
              Deviations = 1.0
              Interval = Some(TimeSpan.FromHours 1.0)
              History = Some(TimeSpan.FromDays 7.0)
              Timezone = None },
        q.Detection
    )

[<Fact>]
let ``interval in seconds, the plan's example`` () =
    match (parsed "max(next_1w):forecast(avg:x{*}, 'linear', 1, interval=3600, history='1w', model='default') >= 90").Detection with
    | Forecast f -> Assert.Equal(Some(TimeSpan.FromHours 1.0), f.Interval)
    | other -> Assert.Fail $"expected a forecast, got {other}"

[<Fact>]
let ``seasonal forecast, falling below`` () =
    let q =
        parsed "min(next_3mo):forecast(avg:system.disk.free{*} by {host}, 'seasonal', 2, interval=3600, seasonality='daily') <= 0"

    Assert.Equal({ Count = 3; Unit = Months }, q.Window)
    Assert.Equal<string list>([ "host" ], q.GroupBy)

    match q.Detection with
    | Forecast f -> Assert.Equal(Plan.SeasonalForecast Plan.Daily, f.Method)
    | other -> Assert.Fail $"expected a forecast, got {other}"

[<Theory>]
[<InlineData("max(last_1w):forecast(avg:x{*}, 'linear', 1) >= 3", "is a horizon")>]
[<InlineData("max(next_6h):forecast(avg:x{*}, 'linear', 1) >= 3", "12 hours to 3 months")>]
[<InlineData("max(next_720m):forecast(avg:x{*}, 'linear', 1) >= 3", "12 hours to 3 months")>]
[<InlineData("max(next_4mo):forecast(avg:x{*}, 'linear', 1) >= 3", "12 hours to 3 months")>]
[<InlineData("min(next_1w):forecast(avg:x{*}, 'linear', 1) >= 3", "max(next_…)")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'linear', 1) > 3", "max(next_…)")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'linear', 0.5) >= 3", "at least 1")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'quadratic', 1) >= 3", "'linear' or 'seasonal'")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'linear', 1, seasonality='weekly') >= 3", "'linear' takes no seasonality")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'seasonal', 1, history='2w') >= 3", "'seasonal' takes no model or history")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'linear', 1, model='fast') >= 3", "model must be")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'linear', 1, history='a week') >= 3", "history must be a duration")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'linear', 1, horizon='1w') >= 3", "takes no horizon")>]
[<InlineData("max(next_1w):forecast(sum:x{*}.as_count(), 'linear', 1) >= 3", "do not take .as_count()")>]
[<InlineData("max(next_1w):forecast(trend_line(avg:x{*}), 'linear', 1) >= 3", "trend_line() is not allowed inside forecast()")>]
let ``forecast refusals`` (text: string, fragment: string) = refused text fragment

// --- outlier --------------------------------------------------------------------------

[<Fact>]
let ``dbscan outliers, Datadog's API example`` () =
    let q = parsed "avg(last_30m):outliers(avg:system.cpu.user{role:es-events-data} by {host}, 'dbscan', 7) > 0"

    Assert.Equal(Outlier { Algorithm = Plan.Dbscan false; Tolerance = 7.0 }, q.Detection)
    Assert.Equal(leaf "avg" "system.cpu.user" (Tag("role", "es-events-data")) [ "host" ], q.Measured)
    Assert.Equal<string list>([ "host" ], q.GroupBy)

[<Fact>]
let ``MAD outliers take a percentage, the name in any case`` () =
    match (parsed "avg(last_1h):outliers(avg:system.load.1{*} by {host}, 'MAD', 3, 20) > 0").Detection with
    | Outlier o -> Assert.Equal(Plan.Mad(false, 20.0), o.Algorithm)
    | other -> Assert.Fail $"expected an outlier, got {other}"

[<Theory>]
[<InlineData("outliers(avg:x{*}, 'dbscan', 3) > 0", "needs a by")>]
[<InlineData("outliers(avg:x{*} by {host}, 'dbscan', 3) > 1", "ends in > 0")>]
[<InlineData("outliers(avg:x{*} by {host}, 'dbscan', 3) >= 0", "ends in > 0")>]
[<InlineData("outliers(avg:x{*} by {host}, 'mad', 3) > 0", "needs a percentage")>]
[<InlineData("outliers(avg:x{*} by {host}, 'kmeans', 3) > 0", "outliers() takes")>]
[<InlineData("outliers(sum:x{*}.as_count() by {host}, 'dbscan', 3) > 0", "do not take .as_count()")>]
let ``outlier refusals`` (expression: string, fragment: string) =
    refused $"avg(last_30m):{expression}" fragment

// --- numbers too large ----------------------------------------------------------------

[<Theory>]
[<InlineData("avg(last_99999999d):avg:x{*} > 90", "is not a window")>]
[<InlineData("avg(last_5m):avg:x{*} > 1e999", "too large")>]
[<InlineData("avg(last_5m):avg:x{*} * 1e999 > 1", "too large")>]
[<InlineData("change(avg(last_5m),99999999d_ago):avg:x{*} > 1", "two days")>]
[<InlineData("max(next_99999999w):forecast(avg:x{*}, 'linear', 1) >= 3", "is a horizon")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'linear', 1, history='999999999999w') >= 3", "history must be a duration")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'linear', 1, interval=1e300) >= 3", "up to 93 days")>]
[<InlineData("max(next_1w):forecast(avg:x{*}, 'linear', 1, interval='99999999999w') >= 3", "up to 93 days")>]
[<InlineData("avg(last_1h):anomalies(avg:a{*}, 'basic', 2, alert_window='last_5m', interval=1e12) >= 1", "up to 93 days")>]
[<InlineData("avg(last_1h):anomalies(avg:a{*}, 'basic', 1e999, alert_window='last_5m') >= 1", "too large")>]
[<InlineData("avg(last_30m):outliers(avg:x{*} by {host}, 'dbscan', 1e999) > 0", "too large")>]
let ``numbers too large are refused, not thrown`` (text: string, fragment: string) = refused text fragment
