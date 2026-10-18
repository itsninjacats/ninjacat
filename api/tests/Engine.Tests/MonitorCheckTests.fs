/// Service check monitors: the query, the definition's rules, the SQL and the evaluation of runs.
module NinjaCat.Api.Engine.Tests.MonitorCheckTests

open System
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Engine.Monitor.Definition
open NinjaCat.Api.Engine.Monitor.Evaluate

let private orFail (result: Result<'T, string list>) : 'T =
    match result with
    | Ok v -> v
    | Error problems -> failwith (String.concat "\n" problems)

let private parsed (text: string) : CheckQuery = Check.query text |> orFail

let private mentions (fragment: string) (problems: string list) =
    Assert.True(problems |> List.exists (fun p -> p.Contains fragment), $"no problem mentions '{fragment}':\n{String.Join('\n', problems)}")

let private problemsOf (result: Result<'T, string list>) : string list =
    match result with
    | Ok v -> failwith $"expected problems, got {v}"
    | Error problems -> problems

let private check name filter groupBy last =
    { Check = name
      Filter = filter
      GroupBy = groupBy
      Last = last }

// --- the query ---------------------------------------------------------------------------

[<Fact>]
let ``queries as integrations-core and the Datadog Operator write them`` () =
    Assert.Equal(check "consul.check" All [ "host" ] 3, parsed """"consul.check".over("*").by("host").last(3).count_by_status()""")

    Assert.Equal(
        check "helm.release_state" All [ "helm_namespace"; "helm_release"; "helm_storage"; "kube_cluster_name" ] 6,
        parsed """"helm.release_state".over("*").by("helm_namespace","helm_release","helm_storage","kube_cluster_name").last(6).count_by_status()"""
    )

    Assert.Equal(check "mysql.replication.replica_running" All [ "*" ] 2, parsed """"mysql.replication.replica_running".over("*").by("*").last(2).count_by_status()""")
    Assert.Equal(check "kubernetes.kubelet.check" All [ "host" ] 2, parsed """"kubernetes.kubelet.check".over("*").by("host").last(2).count_by_status()""")

[<Fact>]
let ``calls come in any order, over and by may be left out`` () =
    Assert.Equal(
        check "c" (And [ Tag("env", "prod"); Tag("role", "db") ]) [ "host" ] 3,
        parsed """"c".over("env:prod", "role:db").last(3).by("host").count_by_status()"""
    )

    Assert.Equal(check "custom.check" All [ "host" ] 2, parsed """"custom.check".by("host").last(2).count_by_status()""")
    Assert.Equal(check "ntp.in_sync" All [] 2, parsed """"ntp.in_sync".over("*").last(2).count_by_status()""")

[<Fact>]
let ``exclude is ANDed on as NOT`` () =
    let q = parsed """"datadog.agent.up".over("*").exclude("group:pgsynclog","environment:enikshay").by("host").last(2).count_by_status()"""
    Assert.Equal(And [ Not(Tag("group", "pgsynclog")); Not(Tag("environment", "enikshay")) ], q.Filter)

    let scoped = parsed """"x".over("env:prod").exclude("role:db").last(1).count_by_status()"""
    Assert.Equal(And [ Tag("env", "prod"); Not(Tag("role", "db")) ], scoped.Filter)

[<Fact>]
let ``over takes what a tag filter takes`` () =
    Assert.Equal(Bare "database", (parsed """"x".over("database").last(1).count_by_status()""").Filter)

    let q = parsed """  'Mixed.Case'  .over( 'role:web-*' , '!env:dev' ) .by( 'host' ) .last( 2 ) .count_by_status( )  """
    Assert.Equal(check "Mixed.Case" (And [ Tag("role", "web-*"); Not(Tag("env", "dev")) ]) [ "host" ] 2, q)

    Assert.Equal(Tag("env", "prod"), (parsed """"x".over("*", "env:prod").last(1).count_by_status()""").Filter)

[<Theory>]
[<InlineData("\"x\".over(\"*\").by(\"environment:foo\").last(2).count_by_status()", "by() takes tag keys; put environment:foo in over()")>]
[<InlineData("\"x\".over(\"*\").by(\"host\").last(2).pct_by_status()", "cluster alerts (pct_by_status) are not supported yet")>]
[<InlineData("\"x\".over(\"*\").by(\"host\").count_by_status()", "last(N) is required")>]
[<InlineData("\"x\".last(0).count_by_status()", "whole number of runs from 1 to 100")>]
[<InlineData("\"x\".last(101).count_by_status()", "whole number of runs from 1 to 100")>]
[<InlineData("\"x\".last(2.5).count_by_status()", "whole number of runs from 1 to 100")>]
[<InlineData("\"x\".by(\"host\").by(\"role\").last(2).count_by_status()", ".by() is given more than once")>]
[<InlineData("\"x\".foo().last(2).count_by_status()", ".foo() is not part of a check query")>]
[<InlineData("\"x\".last(2)", "ends in .count_by_status()")>]
[<InlineData("\"x\".last(2).count_by_status() > 1", "Expecting")>]
[<InlineData("\"\".last(2).count_by_status()", "the check name is empty")>]
[<InlineData("\"x\".over().last(2).count_by_status()", "over() cannot be blank")>]
[<InlineData("\"x\".over(\" \").last(2).count_by_status()", "over() cannot be blank")>]
[<InlineData("\"x\".exclude(\"*\").last(2).count_by_status()", "exclude() cannot take \"*\"")>]
[<InlineData("\"x\".by(\"*\",\"host\").last(2).count_by_status()", "by() takes \"*\" alone")>]
let ``queries that are refused say why`` (text: string, fragment: string) =
    Check.query text |> problemsOf |> mentions fragment

// --- the definition ----------------------------------------------------------------------

let private consul = """"consul.check".over("*").by("host").last(3).count_by_status()"""

let private definition (query: string) (options: string) : CheckDefinition = Check.definition query options |> orFail

[<Fact>]
let ``a missing threshold is one run`` () =
    let t = (definition consul "").Options.Thresholds
    Assert.Equal((Some 1.0, Some 1.0, Some 1.0), (t.Critical, t.Warning, t.Ok))

[<Fact>]
let ``thresholds as integrations-core sets them`` () =
    let t = (definition consul """{"thresholds":{"critical":2,"warning":2,"ok":1}}""").Options.Thresholds
    Assert.Equal((Some 2.0, Some 2.0, Some 1.0), (t.Critical, t.Warning, t.Ok))

    // foundationdb: a warning above its critical.
    definition """"foundationdb.check".over("*").by("host").last(6).count_by_status()""" """{"thresholds":{"critical":2,"warning":5,"ok":1}}"""
    |> ignore

    let helm = """"helm.release_state".over("*").by("helm_release").last(6).count_by_status()"""
    definition helm """{"thresholds":{"critical":5,"warning":1,"ok":1}}""" |> ignore

[<Theory>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"thresholds\":{\"critical\":3}}", "last(2) must be at least the largest threshold (3)")>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"thresholds\":{\"critical\":\"0.5\"}}", "options.thresholds.critical must be a number")>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"thresholds\":{\"warning\":1.5}}", "options.thresholds.warning is a count of consecutive runs")>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"thresholds\":{\"ok\":0}}", "options.thresholds.ok is a count of consecutive runs")>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"thresholds\":{\"unknown\":0}}", "options.thresholds.unknown is a count of consecutive runs")>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"thresholds\":{\"unknown\":1.5}}", "options.thresholds.unknown is a count of consecutive runs")>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"thresholds\":{\"unknown\":3}}", "last(2) must be at least the largest threshold (3)")>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"thresholds\":{\"critical\":1,\"critical_recovery\":1}}", "recovery thresholds are not available")>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"threshold_windows\":{\"trigger_window\":\"last_15m\"}}", "options.threshold_windows")>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"on_missing_data\":\"resolve\"}", "options.on_missing_data is not available")>]
[<InlineData("\"x\".last(2).count_by_status()", "{\"notify_no_data\":true,\"no_data_timeframe\":2881}", "must be at most 2880")>]
let ``definitions that are refused say why`` (query: string, options: string, fragment: string) =
    Check.definition query options |> problemsOf |> mentions fragment

[<Fact>]
let ``unknown is kept, require_full_window is accepted and not acted on`` () =
    let c = definition consul """{"thresholds":{"unknown":2},"require_full_window":true,"renotify_interval":60}"""
    Assert.Equal<string list>([ "renotify_interval"; "require_full_window" ], c.Options.NotActedOn)
    Assert.Equal(Some 2.0, c.Options.Thresholds.Unknown)
    Assert.Equal(None, (definition consul "").Options.Thresholds.Unknown)
    Assert.Equal(Some(TimeSpan.FromMinutes 2880.0), (definition consul """{"notify_no_data":true,"no_data_timeframe":2880}""").Options.NoDataTimeframe)

[<Fact>]
let ``the type picks the kind of monitor`` () =
    let now = DateTimeOffset(2026, 10, 13, 12, 0, 0, TimeSpan.Zero)
    let byType = Validate.byType Plan.defaultRetention now
    let metric = "avg(last_5m):avg:cpu{*} > 90"

    Assert.True(byType "query alert" metric "" |> Result.exists _.IsMetric)
    Assert.True(byType "metric alert" metric "" |> Result.exists _.IsMetric)
    Assert.True(byType "service check" consul "" |> Result.exists _.IsServiceCheck)
    byType "log alert" metric "" |> problemsOf |> mentions "'log alert' is not supported"
    byType "query alert" metric """{"thresholds":{"ok":1}}""" |> problemsOf |> mentions "options.thresholds.ok is for service check monitors"
    byType "service check" metric "" |> problemsOf |> ignore

// --- the SQL -----------------------------------------------------------------------------

let private at = DateTimeOffset(2026, 10, 17, 12, 0, 0, TimeSpan.Zero)

[<Fact>]
let ``the SQL binds every value and reads the newest runs of each series`` () =
    let c = definition """"pg.can_connect".over("env:prod").by("kube_namespace","host").last(3).count_by_status()""" """{"evaluation_delay":60}"""
    let p = Check.period c at
    Assert.Equal(({ From = at.AddMinutes -121.0; To = at.AddMinutes -1.0 }: Period), p)

    let sql = Check.sql (TenantId "acme") c.Query p
    let values = sql.Parameters |> List.map snd

    for v in
        [ SqlValue.String "acme"
          SqlValue.String "pg.can_connect"
          SqlValue.Int64(p.From.ToUnixTimeMilliseconds())
          SqlValue.Int64(p.To.ToUnixTimeMilliseconds())
          SqlValue.String "env"
          SqlValue.String "prod" ] do
        Assert.Contains(v, values)

    Assert.DoesNotContain("acme", sql.Text)
    Assert.Contains("SELECT DISTINCT timestamp", sql.Text)
    Assert.Contains("LIMIT 3 BY series_id, status = 'UNKNOWN'", sql.Text)
    Assert.Matches(@"nullIf\(toString\(host\), ''\) AS g0, arrayJoin.* AS g1, toInt64", sql.Text)
    Assert.Contains("LIMIT 2 BY series_id, status = 'UNKNOWN'", (Check.sql (TenantId "acme") { c.Query with Last = 1 } p).Text)

[<Fact>]
let ``a longer no-data timeframe reads further back`` () =
    let c = definition consul """{"notify_no_data":true,"no_data_timeframe":1440}"""
    Assert.Equal(at.AddDays -1.0, (Check.period c at).From)
    Assert.Equal(at.AddHours -2.0, (Check.period (definition consul """{"no_data_timeframe":30}""") at).From)

[<Fact>]
let ``by star groups by series, its host and tags`` () =
    let sql = Check.sql (TenantId "acme") (parsed """"x".by("*").last(2).count_by_status()""") { From = at.AddHours -2.0; To = at }
    Assert.Contains("concat('host:', toString(host))", sql.Text)
    Assert.Contains(" AS g0, toInt64", sql.Text)

// --- the evaluation ----------------------------------------------------------------------

let private ok = 0
let private warning = 1
let private critical = 2
let private unknown = 3

/// Runs of one series, newest first, `every` seconds apart, the newest `age` seconds before `to`.
let private runs (series: uint64) (groups: string option list) (age: float) (every: float) (statuses: int list) : Compile.Row list =
    let p = Check.period (definition consul "") at

    statuses
    |> List.mapi (fun i status ->
        { Groups = groups
          SeriesId = Some series
          BucketMs = p.To.AddSeconds(-age - every * float i).ToUnixTimeMilliseconds()
          Value = float status }
        : Compile.Row)

let private evaluate (query: string) (rows: Compile.Row list) =
    let c = definition query ""

    match Check.evaluation c.Query (Check.period c at) 100 rows with
    | Ok e -> e
    | Error e -> failwith e

let private byHost = """"x".over("*").by("host").last(5).count_by_status()"""
let private oneGroup = """"x".over("*").last(5).count_by_status()"""

let private streaks (e: Evaluation) =
    [ for g in e.Groups ->
          match g.Reading with
          | Streaks s -> g.Key, s
          | other -> failwith $"{other}" ]

[<Fact>]
let ``a streak counts the newest runs in a row, within the last N`` () =
    let e = evaluate byHost (runs 1UL [ Some "a" ] 10.0 15.0 [ critical; critical; warning; ok; ok; ok ])
    Assert.Equal<(string * Streak list) list>([ "host:a", [ { Latest = critical; Critical = 2; Warning = 0; Ok = 0; Unknown = 0; OkSinceUnknown = 0 } ] ], streaks e)

    let healthy = evaluate byHost (runs 1UL [ Some "a" ] 10.0 15.0 [ ok; ok; ok; ok; ok; ok; ok ])
    Assert.Equal({ Latest = ok; Critical = 0; Warning = 0; Ok = 5; Unknown = 0; OkSinceUnknown = 5 }, (streaks healthy).Head |> snd |> List.exactlyOne)

[<Fact>]
let ``UNKNOWN runs take no place among the last N, and the newest in a row are counted`` () =
    let e = evaluate byHost (runs 1UL [ Some "a" ] 10.0 15.0 [ unknown; critical; unknown; critical; ok; critical; critical ])
    Assert.Equal({ Latest = unknown; Critical = 2; Warning = 0; Ok = 0; Unknown = 1; OkSinceUnknown = 0 }, (streaks e).Head |> snd |> List.exactlyOne)

    // ntp: four UNKNOWN in a row do not push the CRITICAL runs out.
    let ntp = evaluate byHost (runs 1UL [ Some "a" ] 10.0 15.0 [ unknown; unknown; unknown; unknown; critical; critical ])
    Assert.Equal({ Latest = unknown; Critical = 2; Warning = 0; Ok = 0; Unknown = 4; OkSinceUnknown = 0 }, (streaks ntp).Head |> snd |> List.exactlyOne)

    // OK runs before an UNKNOWN one count for Ok, not for OkSinceUnknown.
    let back = evaluate byHost (runs 1UL [ Some "a" ] 10.0 15.0 [ ok; unknown; unknown; ok; ok ])
    Assert.Equal({ Latest = ok; Critical = 0; Warning = 0; Ok = 3; Unknown = 0; OkSinceUnknown = 1 }, (streaks back).Head |> snd |> List.exactlyOne)

    // All UNKNOWN: no streak of a known status.
    let silent = evaluate byHost (runs 1UL [ Some "a" ] 10.0 15.0 [ unknown; unknown ])
    Assert.Equal({ Latest = unknown; Critical = 0; Warning = 0; Ok = 0; Unknown = 2; OkSinceUnknown = 0 }, (streaks silent).Head |> snd |> List.exactlyOne)

[<Fact>]
let ``CRITICAL runs between UNKNOWN ones alert`` () =
    let c = definition """"x".over("*").last(3).count_by_status()""" """{"thresholds":{"critical":2}}"""
    let p = Check.period c at
    let rows = runs 1UL [] 10.0 15.0 [ unknown; critical; unknown; critical; unknown; critical ]

    match Check.evaluation c.Query p 100 rows with
    | Ok e ->
        let groups, transitions = State.step (ServiceCheck c) Map.empty e at
        Assert.Equal(Some State.Alert, groups["*"].State)
        Assert.Equal(State.Reason.New, (Assert.Single transitions).Reason)
    | Error e -> failwith e

[<Theory>]
[<InlineData(90.0, 15.0, 2, true)>]
[<InlineData(91.0, 15.0, 2, false)>]
[<InlineData(1800.0, 900.0, 2, true)>]
[<InlineData(1861.0, 900.0, 2, false)>]
[<InlineData(3600.0, 15.0, 1, true)>]
let ``a series counts while its newest run is at most twice its gap and a minute old, or is its only one`` (age: float, every: float, count: int, live: bool) =
    let e = evaluate byHost (runs 1UL [ Some "a" ] age every (List.replicate count critical))
    Assert.Equal(live, not e.Groups.IsEmpty)

[<Fact>]
let ``a node whose tags changed: the old series drops out`` () =
    let old = runs 1UL [ Some "a" ] 600.0 15.0 [ critical; critical; critical ]
    let current = runs 2UL [ Some "a" ] 5.0 15.0 [ ok; ok ]
    let e = evaluate byHost (old @ current)
    Assert.Equal<(string * Streak list) list>([ "host:a", [ { Latest = ok; Critical = 0; Warning = 0; Ok = 2; Unknown = 0; OkSinceUnknown = 2 } ] ], streaks e)

[<Fact>]
let ``runs without a by key are N/A and dropped`` () =
    let e = evaluate byHost (runs 1UL [ None ] 5.0 15.0 [ critical ] @ runs 2UL [ Some "b" ] 5.0 15.0 [ ok ])
    Assert.Equal<string list>([ "host:b" ], e.Groups |> List.map _.Key)
    Assert.Equal<string list>([ "host:b" ], e.Groups.Head.Tags)

[<Fact>]
let ``no by is one group judging each series`` () =
    let e = evaluate oneGroup (runs 1UL [] 5.0 15.0 [ critical ] @ runs 2UL [] 5.0 15.0 [ ok ])
    let group = Assert.Single e.Groups
    Assert.Equal(("*", ([]: string list)), (group.Key, group.Tags))
    Assert.Equal(2, (streaks e).Head |> snd |> List.length)

[<Fact>]
let ``by star is a group per series`` () =
    let perSeries = """"x".over("*").by("*").last(5).count_by_status()"""
    let e = evaluate perSeries (runs 1UL [ Some "host:a,role:db" ] 5.0 15.0 [ critical ] @ runs 2UL [ Some "" ] 5.0 15.0 [ ok ])
    Assert.Equal<(string * string list) list>([ "*", []; "host:a,role:db", [ "host:a"; "role:db" ] ], e.Groups |> List.map (fun g -> g.Key, g.Tags) |> List.sort)

[<Fact>]
let ``a group with only dead series is left out`` () =
    let e = evaluate byHost (runs 1UL [ Some "a" ] 3600.0 15.0 [ critical; critical ] @ runs 2UL [ Some "b" ] 5.0 15.0 [ ok ])
    Assert.Equal<string list>([ "host:b" ], e.Groups |> List.map _.Key)

[<Fact>]
let ``too many groups is an error`` () =
    let c = definition byHost ""
    let rows = [ for i in 1..3 -> runs (uint64 i) [ Some $"h{i}" ] 5.0 15.0 [ ok ] ] |> List.concat

    match Check.evaluation c.Query (Check.period c at) 2 rows with
    | Error e -> Assert.StartsWith("too_many_groups: the query yields 3 groups, at most 2", e)
    | Ok e -> failwith $"{e}"

[<Fact>]
let ``a group's last point is its newest run; one run per second counts once`` () =
    let rows = runs 1UL [ Some "a" ] 20.0 15.0 [ critical; critical ] @ runs 2UL [ Some "a" ] 5.0 15.0 [ ok; ok ]
    let e = evaluate byHost (rows @ runs 1UL [ Some "a" ] 20.0 15.0 [ critical ])
    let p = Check.period (definition byHost "") at
    Assert.Equal(Some(p.To.AddSeconds -5.0), e.Groups.Head.LastPoint)
    Assert.Equal(2, ((streaks e).Head |> snd |> List.find (fun s -> s.Latest = critical)).Critical)

[<Fact>]
let ``the worst newest status is the group's value`` () =
    let streak latest = { Latest = latest; Critical = 0; Warning = 0; Ok = 0; Unknown = 0; OkSinceUnknown = 0 }
    Assert.Equal(2.0, Check.worst [ streak ok; streak critical; streak warning ])
    Assert.Equal(1.0, Check.worst [ streak unknown; streak warning ])
    Assert.Equal(3.0, Check.worst [ streak ok; streak unknown ])
    Assert.Equal(0.0, Check.worst [ streak ok ])
