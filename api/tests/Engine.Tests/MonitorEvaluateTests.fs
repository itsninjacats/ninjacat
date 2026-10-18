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
    match Validate.definition Plan.defaultRetention at query options with
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
    (evaluate (executeOver points) (TenantId "t") Plan.defaultRetention maxGroups None d at).Result

let private evaluated (points: Point list) (d: Definition) : Evaluation =
    match evaluateWith 1000 points d with
    | Ok e -> e
    | Error e -> failwith e

let private readings (e: Evaluation) = e.Groups |> List.map (fun g -> g.Key, g.Reading)

let private value (e: Evaluation) (key: string) : float =
    match (e.Groups |> List.find (fun g -> g.Key = key)).Reading with
    | Judged v -> v
    | Skipped(_, reason) -> failwith $"{key} skipped: {reason}"
    | Streaks _ -> failwith $"{key} is a check's"

let private skipped (e: Evaluation) (key: string) : bool =
    match (e.Groups |> List.find (fun g -> g.Key = key)).Reading with
    | Judged _
    | Streaks _ -> false
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

    let tenMinutes, _ = nextTick "default/1" (TimeSpan.FromMinutes 10.0) None (at.AddMinutes 3.0)
    Assert.Equal(at.AddMinutes 10.0, tenMinutes)

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

// --- anomaly ----------------------------------------------------------------------------

let private calm i = if i % 2 = 0 then 10.0 else 12.0

/// An hour of 10s and 12s, then the 15 minutes of the alert window: one
/// point a minute, None leaving that minute empty.
let private anomalyPoints (window: float option list) : Point list =
    let values = List.init 60 (calm >> Some) @ window
    let until = DateTimeOffset.FromUnixTimeMilliseconds(windowTo.ToUnixTimeMilliseconds() / 60000L * 60000L)
    let start = until.AddMinutes(-float values.Length)

    values
    |> List.indexed
    |> List.choose (fun (i, v) -> v |> Option.map (fun v -> { Metric = "cpu"; Groups = []; Time = start.AddMinutes(float i); Value = v }))

let private calmThen (spikes: int list) =
    anomalyPoints [ for i in 0..14 -> Some(if List.contains i spikes then 100.0 else calm i) ]

let private basic (args: string) (options: string) =
    definition $"avg(last_4h):anomalies(avg:cpu{{*}}, 'basic', 2, alert_window='last_15m', interval=60{args}) >= 1" options

let private group (e: Evaluation) = Assert.Single e.Groups

[<Theory>]
[<InlineData("", 5.0)>]
[<InlineData(", direction='both'", 5.0)>]
[<InlineData(", direction='above'", 5.0)>]
[<InlineData(", direction='below'", 0.0)>]
let ``an anomaly is the fraction of the alert window outside the band`` (direction: string, outside: float) =
    let e = evaluated (calmThen [ 10..14 ]) (basic direction "")
    Assert.Equal(TimeSpan.FromMinutes 15.0, e.Period.To - e.Period.From)
    Assert.Equal(outside / 15.0, value e "*", 9)

[<Fact>]
let ``an anomaly recovers by the fraction over recovery_window`` () =
    let options = """{"threshold_windows":{"trigger_window":"last_15m","recovery_window":"last_5m"}}"""
    let late = group (evaluated (calmThen [ 10..14 ]) (basic "" options))
    Assert.Equal(Some 1.0, late.Recovery)
    let early = group (evaluated (calmThen [ 0..4 ]) (basic "" options))
    Assert.Equal(Judged(5.0 / 15.0), early.Reading)
    Assert.Equal(Some 0.0, early.Recovery)

[<Fact>]
let ``count_default_zero counts an empty bucket as 0`` () =
    let points = anomalyPoints [ for i in 0..14 -> if i >= 10 then None else Some(calm i) ]
    Assert.Equal(0.0, value (evaluated points (basic "" "")) "*", 9)
    Assert.Equal(5.0 / 15.0, value (evaluated points (basic ", count_default_zero='true'" "")) "*", 9)

[<Fact>]
let ``a seasonal model without enough history is learning, not OK`` () =
    let d = definition "avg(last_4h):anomalies(avg:cpu{*}, 'agile', 2, alert_window='last_15m', interval=60, seasonality='hourly') >= 1" ""
    let g = group (evaluated (calmThen []) d)
    Assert.True(g.Reading.IsSkipped)
    Assert.Contains("learning", string g.Reading)

[<Fact>]
let ``count_default_zero fills no history from before a group's first point`` () =
    let d =
        definition
            "avg(last_4h):anomalies(avg:cpu{*}, 'robust', 2, alert_window='last_15m', interval=60, seasonality='hourly', count_default_zero='true') >= 1"
            ""

    let g = group (evaluated (calmThen []) d)
    Assert.Equal(Skipped(Learning, "learning: the model has too little history yet"), g.Reading)

/// Hourly-seasonal points every minute, from `hours` back to `until`, with a
/// little noise so the models have a spread.
let private seasonalPoints (hours: int) (until: DateTimeOffset) =
    [ for m in 1 .. hours * 60 ->
          let t = until.AddMinutes(-float m)
          { Metric = "cpu"
            Groups = []
            Time = t
            Value = 50.0 + 10.0 * sin (2.0 * Math.PI * float t.Minute / 60.0) + 0.5 * sin (1.7 * float m) } ]

[<Theory>]
[<InlineData("robust")>]
[<InlineData("agile")>]
let ``a seasonal anomaly judges with the models it was given`` (algorithm: string) =
    let d =
        definition
            $"avg(last_4h):anomalies(avg:cpu{{*}}, '{algorithm}', 3, alert_window='last_15m', interval=60, seasonality='hourly') >= 1"
            ""

    let calm = seasonalPoints 8 at
    let shifted = calm |> List.map (fun p -> if p.Time >= windowTo.AddMinutes -5.0 then { p with Value = p.Value + 30.0 } else p)
    let tenant = TenantId "t"

    let fit =
        match (fit (executeOver calm) tenant Plan.defaultRetention d None at).Result with
        | Ok(Some f) -> f
        | other -> failwith $"%A{other}"

    Assert.False(fit.Models[0].IsEmpty)

    let judged points =
        match (evaluate (executeOver points) tenant Plan.defaultRetention 1000 (Some fit) d at).Result with
        | Ok e -> value e "*"
        | Error e -> failwith e

    let quiet = judged calm
    Assert.InRange(quiet, 0.0, 0.1)
    Assert.True(judged shifted > quiet)

[<Fact>]
let ``models are fitted again once they are stale or the query changed`` () =
    let reads = ref 0

    let execute sql =
        lock reads (fun () -> reads.Value <- reads.Value + 1)
        executeOver [] sql

    let tenant = TenantId "t"
    let hourly = "avg(last_4h):anomalies(avg:cpu{*}, 'robust', 3, alert_window='last_15m', interval=60, seasonality='hourly') >= 1"
    let d = definition hourly ""

    let fitAt (previous: Fit option) (tick: DateTimeOffset) (d: Definition) =
        match (fit execute tenant Plan.defaultRetention d previous tick).Result with
        | Ok(Some f) -> f
        | other -> failwith $"%A{other}"

    let first = fitAt None at d
    let read = reads.Value
    Assert.Same(first, fitAt (Some first) (at.AddMinutes 9.0) d)
    Assert.Equal(read, reads.Value)
    Assert.Equal(at.AddMinutes 10.0, (fitAt (Some first) (at.AddMinutes 10.0) d).At)
    let other = definition (hourly.Replace("cpu", "mem")) ""
    Assert.Equal(at.AddMinutes 1.0, (fitAt (Some first) (at.AddMinutes 1.0) other).At)
    Assert.Equal(Ok None, (fit execute tenant Plan.defaultRetention (basic "" "") None at).Result)

// --- forecast ---------------------------------------------------------------------------

/// A line through the last hour: `slope` a minute, `start` an hour ago.
let private line (start: float) (slope: float) =
    [ for m in 0..59 ->
          { Metric = "cpu"
            Groups = []
            Time = windowTo.AddMinutes(float (m - 60))
            Value = start + slope * float m } ]

// A minute a minute: 60 by the window's end, a day of minutes more.
[<Theory>]
[<InlineData("max(next_1d):forecast(avg:cpu{*}, 'linear', 1, history='1h') >= 2000", 1.0, 1450.0, 1550.0)>]
[<InlineData("min(next_1d):forecast(avg:cpu{*}, 'linear', 1, history='1h') <= -1000", -1.0, -1550.0, -1450.0)>]
let ``a forecast is the highest upper bound over its horizon, or with <= the lowest lower bound`` (query: string, slope: float, low: float, high: float) =
    let e = evaluated (line 0.0 slope) (definition query "")
    Assert.Equal(TimeSpan.FromHours 1.0, e.Period.To - e.Period.From)
    Assert.InRange(value e "*", low, high)

[<Fact>]
let ``a forecast without the history to fit is learning`` () =
    let d = definition "max(next_1d):forecast(avg:cpu{*}, 'linear', 1, history='1h') >= 90" ""
    let g = group (evaluated (line 0.0 1.0 |> List.truncate 1) d)
    Assert.Contains("learning", string g.Reading)

// --- outlier ----------------------------------------------------------------------------

let private hosts (levels: (string * float) list) =
    levels |> List.collect (fun (host, level) -> constant "cpu" [ Some host ] level)

let private outlierMonitor = definition "avg(last_5m):outliers(avg:cpu{*} by {host}, 'dbscan', 3) > 0" ""

[<Fact>]
let ``an outlier group is 1, the others 0`` () =
    let e = evaluated (hosts [ "a", 10.0; "b", 10.5; "c", 11.0; "d", 50.0 ]) outlierMonitor
    Assert.Equal<(string * Reading) list>(
        [ "host:a", Judged 0.0; "host:b", Judged 0.0; "host:c", Judged 0.0; "host:d", Judged 1.0 ],
        readings e |> List.sortBy fst
    )

[<Fact>]
let ``fewer than 3 groups with data have nothing to be an outlier from`` () =
    let e = evaluated (hosts [ "a", 10.0; "c", nan; "d", 50.0 ]) outlierMonitor
    Assert.Equal(3, e.Groups.Length)
    Assert.Equal<Reading list>(
        [ Skipped(FewGroups, "an outlier needs at least 3 groups with data to compare")
          Skipped(Incomplete, "no bucket in the window has a value")
          Skipped(FewGroups, "an outlier needs at least 3 groups with data to compare") ],
        e.Groups |> List.sortBy _.Key |> List.map _.Reading
    )
