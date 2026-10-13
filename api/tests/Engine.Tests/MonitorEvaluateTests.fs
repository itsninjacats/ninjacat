module NinjaCat.Api.Engine.Tests.MonitorEvaluateTests

open System
open System.Text.RegularExpressions
open System.Threading.Tasks
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Query.Compile
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Engine.Monitor.Definition
open NinjaCat.Api.Engine.Monitor.Evaluate

let private at = DateTimeOffset(2026, 10, 13, 12, 0, 0, TimeSpan.Zero)

/// With the default 30 s delay and a 15 s step, a 5-minute window.
let private windowFrom = at.AddMinutes -5.5
let private windowTo = at.AddSeconds -30.0

let private definition (query: string) (options: string) : Definition =
    match Validate.definition Plan.defaultRetention at "query alert" query options with
    | Ok d -> d
    | Error problems -> failwith $"refused:\n{String.Join('\n', problems)}"

let private queryOf (text: string) : MonitorQuery =
    match Parse.query text with
    | Ok q -> q
    | Error problems -> failwith $"refused:\n{String.Join('\n', problems)}"

/// A point of a metric: its group values (in the query's `by` order), time
/// and value.
type private Point =
    { Metric: string
      Groups: string option list
      Time: DateTimeOffset
      Value: float }

/// Answers compiled SQL from a list, as ClickHouse would for one series per
/// group: the metric's points in [from, to), each in its bucket.
let private executeOver (points: Point list) (sql: Sql) : Task<Row list> =
    let param (name: string) =
        sql.Parameters |> List.pick (fun (n, v) -> if Regex.IsMatch(n, $"^{name}\\d+$") then Some v else None)

    let number name =
        match param name with
        | Int64 n -> n
        | other -> failwith $"{name} is {other}"

    let metric =
        match param "metric" with
        | String m -> m
        | other -> failwith $"metric is {other}"

    let fromMs, toMs, stepMs = number "from", number "to", number "step" * 1000L
    let grouped = sql.Text.Contains "AS g0"

    Task.FromResult
        [ for p in points do
              let ms = p.Time.ToUnixTimeMilliseconds()

              if p.Metric = metric && ms >= fromMs && ms < toMs then
                  { Groups = (if grouped then p.Groups else [])
                    SeriesId = Some 1UL
                    BucketMs = ms / stepMs * stepMs
                    Value = p.Value } ]

/// One point every 15 s over the window ending at `until`, from `values`.
let private series (metric: string) (groups: string option list) (until: DateTimeOffset) (values: float list) : Point list =
    let start = until.AddSeconds(-15.0 * float values.Length)
    values |> List.mapi (fun i v -> { Metric = metric; Groups = groups; Time = start.AddSeconds(15.0 * float i); Value = v })

let private constant (metric: string) (groups: string option list) (value: float) =
    series metric groups windowTo (List.replicate 20 value)

let private evaluateWith (maxGroups: int) (points: Point list) (d: Definition) =
    (evaluate (executeOver points) (TenantId "t") Plan.defaultRetention maxGroups d at).Result

let private evaluated (points: Point list) (d: Definition) : Evaluation =
    match evaluateWith 1000 points d with
    | Ok e -> e
    | Error e -> failwith e

let private readings (e: Evaluation) = e.Groups |> List.map (fun g -> g.Key, g.Reading)

let private value (e: Evaluation) (key: string) : float =
    match (e.Groups |> List.find (fun g -> g.Key = key)).Reading with
    | Judged v -> v
    | Skipped reason -> failwith $"{key} skipped: {reason}"

let private skipped (e: Evaluation) (key: string) : bool =
    match (e.Groups |> List.find (fun g -> g.Key = key)).Reading with
    | Judged _ -> false
    | Skipped _ -> true

// --- when ------------------------------------------------------------------------------

[<Theory>]
[<InlineData("avg(last_5m):avg:cpu{*} > 1", 1.0)>]
[<InlineData("avg(last_23h):avg:cpu{*} > 1", 1.0)>]
[<InlineData("avg(last_1d):avg:cpu{*} > 1", 10.0)>]
[<InlineData("avg(last_2d):avg:cpu{*} > 1", 10.0)>]
[<InlineData("avg(last_3d):avg:cpu{*} > 1", 30.0)>]
[<InlineData("change(avg(last_2d),1d_ago):avg:cpu{*} > 1", 10.0)>]
[<InlineData("avg(last_1w):anomalies(avg:cpu{*}, 'basic', 2, alert_window='last_1h') >= 1", 1.0)>]
let ``how often a monitor runs`` (query: string, minutes: float) =
    Assert.Equal(TimeSpan.FromMinutes minutes, frequency (queryOf query))

[<Fact>]
let ``ticks sit on the wall clock, each monitor offset within 20 s`` () =
    let offsets = [ for i in 1..50 -> offset $"default/{i}" ]
    Assert.True(offsets |> List.forall (fun o -> o >= TimeSpan.Zero && o < TimeSpan.FromSeconds 20.0))
    Assert.True(offsets |> List.distinct |> List.length > 40, "offsets spread")
    Assert.Equal(offset "default/7", offset "default/7")

    let minute = TimeSpan.FromMinutes 1.0

    for now in [ at; at.AddSeconds 5.0; at.AddSeconds 59.9; at.AddMinutes 7.5 ] do
        let tick, fires = nextTick "default/7" minute None now
        Assert.Equal(0L, tick.UtcTicks % minute.Ticks)
        Assert.Equal(tick + offset "default/7", fires)
        Assert.True(fires > now && fires <= now + minute, $"{now}: fires at {fires}")

[<Fact>]
let ``ten-minute ticks sit on ten minutes`` () =
    let tick, _ = nextTick "default/1" (TimeSpan.FromMinutes 10.0) None (at.AddMinutes 3.0)
    Assert.Equal(at.AddMinutes 10.0, tick)

[<Fact>]
let ``a tick re-armed before the last one fired is still the next one`` () =
    let minute = TimeSpan.FromMinutes 1.0
    let tick, fires = nextTick "default/7" minute None at
    // The timer fired early, or the clock stepped back.
    for now in [ fires.AddMilliseconds -1.0; fires.AddMinutes -3.0 ] do
        let next, _ = nextTick "default/7" minute (Some tick) now
        Assert.Equal(tick + minute, next)

// --- the window -------------------------------------------------------------------------

[<Fact>]
let ``the window ends 30 s before the tick, on the step`` () =
    let p, step = period Plan.defaultRetention (definition "avg(last_5m):avg:cpu{*} > 1" "") (at.AddSeconds 7.0)
    Assert.Equal(TimeSpan.FromSeconds 15.0, step)
    Assert.Equal({ From = windowFrom; To = windowTo }, p)

[<Fact>]
let ``evaluation_delay moves the window back`` () =
    let p, _ = period Plan.defaultRetention (definition "avg(last_5m):avg:cpu{*} > 1" """{"evaluation_delay":120}""") at
    Assert.Equal(at.AddMinutes -2.0, p.To)
    Assert.Equal(at.AddMinutes -7.0, p.From)

[<Theory>]
[<InlineData("avg(last_5m):avg:cpu{*} > 1", 15.0)>]
[<InlineData("avg(last_1d):avg:cpu{*} > 1", 58.0)>]
[<InlineData("avg(last_5m):avg:cpu{*}.rollup(max, 120) > 1", 120.0)>]
[<InlineData("avg(last_1mo):avg:cpu{*} > 1", 1800.0)>]
let ``the step`` (query: string, seconds: float) =
    let p, step = period Plan.defaultRetention (definition query "") at
    Assert.Equal(TimeSpan.FromSeconds seconds, step)
    Assert.Equal(0L, p.To.ToUnixTimeMilliseconds() % int64 step.TotalMilliseconds)

[<Fact>]
let ``near the raw retention the window runs on the step the engine runs`` () =
    // diff reads a bucket before the window, which is past a one-day raw
    // retention: the engine reads the rollup at whole minutes.
    let retention = { Plan.defaultRetention with Raw = TimeSpan.FromDays 1.0 }
    let p, step = period retention (definition "avg(last_1437m):diff(avg:cpu{*}) > 1" "") at
    Assert.Equal(TimeSpan.FromMinutes 1.0, step)
    Assert.Equal(0L, p.To.ToUnixTimeMilliseconds() % 60_000L)

[<Fact>]
let ``past the raw retention the step is whole minutes`` () =
    // 31 days at 1500 points would be 1786 s; only the rollup holds that far back.
    let p, step = period Plan.defaultRetention (definition "avg(last_1mo):avg:cpu{*} > 1" "") at
    Assert.Equal(TimeSpan.FromMinutes 30.0, step)
    Assert.Equal(TimeSpan.FromDays 31.0, p.To - p.From)

// --- threshold, per bucket then the window ---------------------------------------------

[<Fact>]
let ``the time aggregator reduces each group's window`` () =
    let points =
        series "cpu" [ Some "a" ] windowTo [ 10.0; 20.0; 30.0; 40.0 ]
        @ series "cpu" [ Some "b" ] windowTo [ 1.0; 2.0; 3.0 ]

    for agg, a, b in [ "avg", 25.0, 2.0; "max", 40.0, 3.0; "min", 10.0, 1.0; "sum", 100.0, 6.0 ] do
        let e = evaluated points (definition $"{agg}(last_5m):avg:cpu{{*}} by {{host}} > 90" "")
        Assert.Equal<(string * Reading) list>([ "host:a", Judged a; "host:b", Judged b ], readings e)

[<Fact>]
let ``a group's key is its sorted tags; N/A groups are dropped`` () =
    let points =
        constant "cpu" [ Some "web"; Some "a" ] 1.0
        @ constant "cpu" [ None; Some "b" ] 2.0

    let e = evaluated points (definition "avg(last_5m):avg:cpu{*} by {role,host} > 90" "")
    let g = Assert.Single e.Groups
    Assert.Equal("host:a,role:web", g.Key)
    Assert.Equal<string list>([ "host:a"; "role:web" ], g.Tags)

[<Fact>]
let ``a simple alert is one group, *`` () =
    let e = evaluated (constant "cpu" [] 5.0) (definition "avg(last_5m):avg:cpu{*} > 90" "")
    Assert.Equal<(string * Reading) list>([ "*", Judged 5.0 ], readings e)
    Assert.Equal<string list>([], e.Groups.Head.Tags)

[<Fact>]
let ``no series: no groups, which is no data`` () =
    let e = evaluated [] (definition "avg(last_5m):avg:cpu{*} by {host} > 90" "")
    Assert.Empty e.Groups
    Assert.Equal({ From = windowFrom; To = windowTo }, e.Period)

[<Fact>]
let ``the group's last point is kept`` () =
    let points = series "cpu" [ Some "a" ] (windowTo.AddMinutes -2.0) [ 1.0; 2.0 ]
    let e = evaluated points (definition "avg(last_5m):avg:cpu{*} by {host} > 90" "")
    Assert.Equal(Some(windowTo.AddMinutes(-2.0).AddSeconds -15.0), e.Groups.Head.LastPoint)

[<Fact>]
let ``more groups than allowed is an error, not a truncation`` () =
    let points = constant "cpu" [ Some "a" ] 1.0 @ constant "cpu" [ Some "b" ] 1.0

    match evaluateWith 1 points (definition "avg(last_5m):avg:cpu{*} by {host} > 90" "") with
    | Error e -> Assert.StartsWith("too_many_groups", e)
    | Ok e -> Assert.Fail $"expected an error, got {e}"

[<Fact>]
let ``without as_count the formula runs per bucket, then the window`` () =
    let points =
        series "errors" [] (windowTo.AddSeconds -15.0) [ 5.0; 9.0 ]
        @ series "hits" [] (windowTo.AddSeconds -15.0) [ 10.0; 90.0 ]

    // 5/10 and 9/90.
    let avg = evaluated points (definition "avg(last_5m):sum:errors{*} / sum:hits{*} > 1" "")
    Assert.Equal(0.3, value avg "*", 9)
    let sum = evaluated points (definition "sum(last_5m):sum:errors{*} / sum:hits{*} > 1" "")
    Assert.Equal(0.6, value sum "*", 9)

[<Fact>]
let ``with as_count each query is reduced first, the formula after`` () =
    let points =
        series "errors" [] (windowTo.AddSeconds -15.0) [ 5.0; 9.0 ]
        @ series "hits" [] (windowTo.AddSeconds -15.0) [ 10.0; 90.0 ]

    let e = evaluated points (definition "sum(last_5m):sum:errors{*}.as_count() / sum:hits{*}.as_count() > 1" "")
    Assert.Equal(0.14, value e "*", 9)
    Assert.Equal(Some(windowTo.AddSeconds -15.0), e.Groups.Head.LastPoint)

[<Fact>]
let ``a group whose every bucket has no number is skipped, with no last point`` () =
    // Division by zero leaves a gap in every bucket.
    let points = constant "a" [ Some "x" ] 1.0 @ constant "b" [ Some "x" ] 0.0
    let e = evaluated points (definition "avg(last_5m):avg:a{*} by {host} / avg:b{*} by {host} > 1" "")
    Assert.True(skipped e "host:x")
    Assert.Equal(None, e.Groups.Head.LastPoint)

[<Fact>]
let ``a series kept by a function's lookback, with no point in the window, has no last point`` () =
    // ewma_10 reads ten buckets before the window; host:a stopped before it.
    let points = series "cpu" [ Some "a" ] (windowFrom.AddSeconds -30.0) [ 1.0; 2.0 ]
    let e = evaluated points (definition "avg(last_5m):ewma_10(avg:cpu{*} by {host}) > 1" "")

    let g = Assert.Single e.Groups
    Assert.Equal(None, g.LastPoint)
    Assert.True(skipped e g.Key)

[<Fact>]
let ``a series without tags is no group of a multi alert`` () =
    // default_zero over no series gives one series without tags.
    let e = evaluated [] (definition "avg(last_5m):default_zero(avg:cpu{*} by {host}) < 1" "")
    Assert.Empty e.Groups

[<Fact>]
let ``a series in two groups is judged in each`` () =
    // A host with two roles: the store gives one row per value.
    let points = constant "cpu" [ Some "web" ] 5.0 @ constant "cpu" [ Some "db" ] 5.0
    let e = evaluated points (definition "avg(last_5m):avg:cpu{*} by {role} > 90" "")
    Assert.Equal<(string * Reading) list>([ "role:db", Judged 5.0; "role:web", Judged 5.0 ], readings e |> List.sort)

[<Fact>]
let ``as_count groups: sorted keys, N/A dropped`` () =
    let points =
        series "hits" [ Some "web"; Some "a" ] windowTo [ 1.0; 2.0 ]
        @ series "hits" [ None; Some "b" ] windowTo [ 5.0 ]

    let e = evaluated points (definition "sum(last_5m):sum:hits{*} by {role,host}.as_count() > 90" "")
    Assert.Equal<(string * Reading) list>([ "host:a,role:web", Judged 3.0 ], readings e)

// --- require_full_window -----------------------------------------------------------------

let private fullWindow = definition "avg(last_5m):avg:cpu{*} by {host} > 90" """{"require_full_window":true}"""

/// Points in some of the window's five minutes.
let private inMinutes (minutes: int list) =
    [ for m in minutes -> { Metric = "cpu"; Groups = [ Some "a" ]; Time = windowFrom.AddMinutes(float m).AddSeconds 20.0; Value = 1.0 } ]

[<Theory>]
[<InlineData("0,1,2,3,4", true)>]
[<InlineData("0,4", true)>]
[<InlineData("0", false)>]
[<InlineData("1,2,3,4", false)>]
let ``a full window: data in the first bucket, at most 3 empty`` (minutes: string, judged: bool) =
    let e = evaluated (inMinutes (minutes.Split ',' |> Array.map int |> List.ofArray)) fullWindow
    Assert.Equal(judged, not (skipped e "host:a"))

[<Fact>]
let ``without require_full_window a gappy window is judged`` () =
    let e = evaluated (inMinutes [ 2 ]) (definition "avg(last_5m):avg:cpu{*} by {host} > 90" "")
    Assert.False(skipped e "host:a")

[<Fact>]
let ``as_count windows have no gaps to see`` () =
    let points = inMinutes [ 2 ] |> List.map (fun p -> { p with Metric = "hits" })
    let e = evaluated points (definition "sum(last_5m):sum:hits{*} by {host}.as_count() > 90" """{"require_full_window":true}""")
    Assert.Equal(1.0, value e "host:a")

// --- change ------------------------------------------------------------------------------

let private shifted (values: float list) (current: float list) =
    series "cpu" [ Some "a" ] (windowTo.AddMinutes -5.0) values @ series "cpu" [ Some "a" ] windowTo current

[<Fact>]
let ``change: the bucket minus the one a timeshift before, then the window`` () =
    let points = shifted (List.replicate 20 10.0) (List.replicate 20 12.0)
    let e = evaluated points (definition "change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 1" "")
    Assert.Equal(2.0, value e "host:a", 9)

[<Fact>]
let ``pct_change: buckets with no number are left out`` () =
    // The first shifted value is 0: that bucket's change has no number.
    let points = shifted (0.0 :: List.replicate 19 10.0) (100.0 :: List.replicate 19 12.0)
    let e = evaluated points (definition "pct_change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 1" "")
    Assert.Equal(20.0, value e "host:a", 9)

[<Fact>]
let ``pct_change from zero everywhere is skipped`` () =
    let points = shifted (List.replicate 20 0.0) (List.replicate 20 12.0)
    let e = evaluated points (definition "pct_change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 1" "")
    Assert.True(skipped e "host:a")

[<Fact>]
let ``change of a group with nothing to compare with is skipped`` () =
    let points = series "cpu" [ Some "a" ] windowTo (List.replicate 20 12.0)
    let e = evaluated points (definition "change(avg(last_5m),last_5m):avg:cpu{*} by {host} > 1" "")
    Assert.True(skipped e "host:a")

[<Theory>]
[<InlineData("change(avg(last_12h),1h_ago)", 30.0)>]
[<InlineData("change(avg(last_1d),1d_ago)", 60.0)>]
[<InlineData("pct_change(avg(last_2d),1d_ago)", 120.0)>]
let ``change over a long window: the step divides the shift, so buckets meet`` (head: string, seconds: float) =
    let d = definition $"{head}:avg:cpu{{*}} by {{host}} > 1" ""
    let p, step = period Plan.defaultRetention d at
    Assert.Equal(TimeSpan.FromSeconds seconds, step)

    let shift =
        match d.Query.Detection with
        | Change c -> c.Shift.Length
        | other -> failwith $"{other}"

    // A value per minute that grows, so the change is the same in every
    // bucket: by the shift for a ramp, doubling over it for a percentage.
    let percent = head.StartsWith "pct"
    let start = p.From - shift

    let valueAt (t: DateTimeOffset) =
        let minutes = (t - start).TotalMinutes
        if percent then Math.Pow(2.0, minutes / shift.TotalMinutes) else minutes

    let points =
        [ for i in 0 .. int ((p.To - start).TotalMinutes) - 1 do
              let t = start.AddMinutes(float i)
              { Metric = "cpu"; Groups = [ Some "a" ]; Time = t; Value = valueAt t } ]

    let e = evaluated points d
    Assert.Equal((if percent then 100.0 else shift.TotalMinutes), value e "host:a", 6)

[<Fact>]
let ``a shift shorter than the step is an error, not a skip`` () =
    match evaluateWith 1000 [] (definition "change(avg(last_2d),1m_ago):avg:cpu{*} > 1" "") with
    | Error e -> Assert.Contains("does not divide the shift", e)
    | Ok e -> Assert.Fail $"expected an error, got {e}"

[<Fact>]
let ``change with as_count: the two windows reduced, then the change`` () =
    let points =
        (series "hits" [] (windowTo.AddMinutes -5.0) (List.replicate 20 2.0))
        @ (series "hits" [] windowTo (List.replicate 20 3.0))

    let absolute = evaluated points (definition "change(sum(last_5m),last_5m):sum:hits{*}.as_count() > 1" "")
    Assert.Equal(20.0, value absolute "*", 9)
    let percent = evaluated points (definition "pct_change(sum(last_5m),last_5m):sum:hits{*}.as_count() > 1" "")
    Assert.Equal(50.0, value percent "*", 9)

// --- not yet ------------------------------------------------------------------------------

[<Fact>]
let ``anomaly monitors are refused until they are evaluated`` () =
    let d = definition "avg(last_4h):anomalies(avg:cpu{*}, 'basic', 2, alert_window='last_15m') >= 1" ""

    match evaluateWith 1000 [] d with
    | Error e -> Assert.Contains("not evaluated yet", e)
    | Ok e -> Assert.Fail $"expected an error, got {e}"
