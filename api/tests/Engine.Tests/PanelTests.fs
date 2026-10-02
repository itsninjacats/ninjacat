module NinjaCat.Api.Engine.Tests.PanelTests

open System
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Panel

let private tenant = TenantId "default"
let private noon = DateTimeOffset(2026, 9, 21, 12, 0, 0, TimeSpan.Zero)

let private ok (result: Result<'a, string>) : 'a =
    match result with
    | Ok value -> value
    | Error e -> failwith e

let private values (sql: Sql) : SqlValue list = sql.Parameters |> List.map snd

// --- tag filters --------------------------------------------------------------------

[<Fact>]
let ``tags of one key are one condition, keys in order of first appearance`` () =
    Assert.Equal(All, ok (parseTagFilters []))
    Assert.Equal(Tag("env", "prod"), ok (parseTagFilters [ "env:prod" ]))

    Assert.Equal(
        And [ Tag("env", "prod"); In("kube_service", [ "a"; "b" ]) ],
        ok (parseTagFilters [ "env:prod"; "kube_service:a"; "kube_service:b" ])
    )

[<Fact>]
let ``a tag value keeps its colons`` () =
    Assert.Equal(Tag("image", "nginx:1.25"), ok (parseTagFilters [ "image:nginx:1.25" ]))

[<Theory>]
[<InlineData("envprod")>]
[<InlineData(":prod")>]
[<InlineData("env:")>]
let ``a tag without key or value is refused`` (tag: string) =
    Assert.True(Result.isError (parseTagFilters [ tag ]))

// --- times and durations ------------------------------------------------------------------

[<Fact>]
let ``times are relative, absolute, or the fallback`` () =
    let fallback = noon.AddHours -1.0
    let parse raw = ok (parseTime raw fallback noon)
    Assert.Equal(fallback, parse "")
    Assert.Equal(noon, parse "now")
    Assert.Equal(noon.AddHours -6.0, parse "-6h")
    Assert.Equal(noon.AddMinutes -15.0, parse "-15m")
    Assert.Equal(noon.AddDays -7.0, parse "-7d")
    Assert.Equal(noon.AddDays -14.0, parse "-2w")
    Assert.Equal(noon.AddHours -36.0, parse "-1.5d")
    Assert.Equal(DateTimeOffset(2026, 9, 21, 9, 30, 0, TimeSpan.Zero), parse "2026-09-21T09:30:00Z")
    Assert.Equal(noon.AddMinutes -90.0, parse "-1h30m")

[<Theory>]
[<InlineData("yesterday")>]
[<InlineData("-3x")>]
[<InlineData("-")>]
let ``a time that is none of those is refused`` (raw: string) =
    Assert.True(Result.isError (parseTime raw noon noon))

[<Fact>]
let ``a duration is a sign and number-and-unit pairs, from nanoseconds to weeks`` () =
    Assert.Equal(Ok(TimeSpan.FromSeconds 15.0), Duration.parse "15s")
    Assert.Equal(Ok(TimeSpan.FromMinutes -90.0), Duration.parse "-1h30m")
    Assert.Equal(Ok(TimeSpan.FromMilliseconds 300.0), Duration.parse "300ms")
    Assert.Equal(Ok(TimeSpan.FromDays 7.0), Duration.parse "7d")
    Assert.Equal(Ok(TimeSpan.FromDays 14.0), Duration.parse "+2w")
    Assert.Equal(Ok(TimeSpan.FromHours 36.0), Duration.parse "1.5d")

    for bad in [ ""; "15"; "s"; "3x"; "1h 30m"; "-" ] do
        Assert.True(Result.isError (Duration.parse bad), bad)

// --- series ------------------------------------------------------------------------------

let private cpu: SeriesQuery =
    { Metric = "system.cpu.user"
      Hosts = []
      From = noon.AddMinutes -1.0
      To = noon
      Step = Some(TimeSpan.FromSeconds 20.0)
      Aggregation = ""
      Tags = All
      GroupBy = [] }

[<Fact>]
let ``a series query is the engine's own request: by host unless grouped, avg unless told`` () =
    let plain = (seriesRequest cpu).Queries.Head.Query
    Assert.Equal(Some "avg", plain.SpaceAgg)
    Assert.Equal<string list>([ "host" ], plain.GroupBy)
    Assert.Equal(All, plain.Filter)
    Assert.Empty (seriesRequest cpu).Formulas
    Assert.Equal(cpu.Step, (seriesRequest cpu).Interval)

    let narrowed =
        (seriesRequest { cpu with Hosts = [ "a"; "b" ]; Tags = Tag("env", "prod"); Aggregation = "MAX"; GroupBy = [ "service"; "env"; "env" ] })
            .Queries.Head.Query

    Assert.Equal(Some "max", narrowed.SpaceAgg)
    Assert.Equal<string list>([ "env"; "service" ], narrowed.GroupBy)
    Assert.Equal(And [ In("host", [ "a"; "b" ]); Tag("env", "prod") ], narrowed.Filter)
    Assert.Equal(In("host", [ "a" ]), (seriesRequest { cpu with Hosts = [ "a" ] }).Queries.Head.Query.Filter)

/// Two series, each with one point per bucket; with two group keys the
/// second series lacks the second tag.
let private execute (sql: Sql) =
    let groups = sql.Text.Split("AS g").Length - 1
    let row (group: string option list) (t: int64) (v: float) : Query.Compile.Row = { Groups = group; BucketMs = t; Value = v; SeriesId = None }
    let start = noon.AddMinutes(-1.0).ToUnixTimeMilliseconds()

    Threading.Tasks.Task.FromResult
        [ for i in 0L .. 2L do
              row (if groups = 1 then [ Some "a" ] else [ Some "prod"; Some "web" ]) (start + i * 20000L) (float i)
              row (if groups = 1 then [ Some "b" ] else [ Some "prod"; None ]) (start + i * 20000L) (float i * 10.0) ]

let private seriesFor (q: SeriesQuery) : Series list =
    match (Query.Engine.parsed execute tenant [ "q" ] (seriesRequest q)).Result with
    | Ok results -> seriesOf q results
    | Error problems -> failwith (String.concat "; " problems)

[<Fact>]
let ``ungrouped series are hosts; grouped ones are named by their tags, N/A where one is missing`` () =
    let hosts = seriesFor cpu
    Assert.Equal<string list>([ "a"; "b" ], hosts |> List.map _.Name)
    Assert.Equal<string option list>([ Some "a"; Some "b" ], hosts |> List.map _.Host)
    Assert.Equal<Map<string, string>>(Map [ "host", "a" ], hosts.Head.Tags)
    Assert.Equal<(DateTimeOffset * float) list>([ noon.AddSeconds -60.0, 0.0; noon.AddSeconds -40.0, 1.0; noon.AddSeconds -20.0, 2.0 ], hosts.Head.Points)

    let grouped = seriesFor { cpu with GroupBy = [ "service"; "env" ] }
    Assert.Equal<string list>([ "env:prod, service:web"; "env:prod, service:N/A" ], grouped |> List.map _.Name |> List.sortDescending)
    Assert.True(grouped |> List.forall (fun series -> series.Host.IsNone))

[<Fact>]
let ``an aggregation the engine does not have is refused, not replaced`` () =
    match (Query.Engine.parsed execute tenant [ "q" ] (seriesRequest { cpu with Aggregation = "median" })).Result with
    | Error problems -> Assert.Contains("median", String.concat "; " problems)
    | Ok _ -> Assert.Fail "median was accepted"

// --- SQL ---------------------------------------------------------------------------------

let private logs: LogSearch =
    { Tenant = tenant; Text = ""; Service = ""; Host = ""; Status = ""; Tags = All; From = noon.AddHours -1.0; To = noon; Limit = 0 }

[<Fact>]
let ``a tag filter on logs is a membership test on the map, several values hasAny`` () =
    let rows, _ = searchLogs { logs with Tags = And [ Tag("team", "core"); In("zone", [ "a"; "b" ]) ] }
    Assert.Contains("AND ((has(tags[{k3:String}], {p4:String})) AND (hasAny(tags[{k5:String}], {p6:Array(String)})))", rows.Text)
    Assert.Contains(SqlValue.String "team", values rows)
    Assert.Contains(SqlValue.StringArray [ "a"; "b" ], values rows)

    let unfiltered, _ = searchLogs logs
    Assert.DoesNotContain("tags[", unfiltered.Text.Substring(unfiltered.Text.IndexOf " WHERE"))

[<Fact>]
let ``the caller's text is bound, never part of the SQL`` () =
    let evil = "env'] , 1); DROP TABLE logs; --"
    let rows, count = searchLogs { logs with Tags = Tag(evil, "x'y"); Service = evil; Text = evil }

    for sql in [ rows; count ] do
        Assert.DoesNotContain("DROP", sql.Text)
        Assert.DoesNotContain("'", sql.Text.Replace("toUnixTimestamp64Milli", ""))
        Assert.Contains(SqlValue.String evil, values sql)
        Assert.Contains(SqlValue.String "x'y", values sql)

[<Fact>]
let ``message tokens are runs of letters and digits`` () =
    Assert.Equal<string list>([ "connection"; "refused" ], messageTokens "connection refused")
    Assert.Equal<string list>([ "GET"; "api"; "v2"; "logs"; "x"; "1" ], messageTokens "GET /api/v2/logs?x=1")
    Assert.Equal<string list>([ "err"; "code"; "500" ], messageTokens "err_code=500")
    Assert.Empty(messageTokens "...")

[<Fact>]
let ``log search scopes by tenant first, then tokens, then the phrase`` () =
    let search: LogSearch =
        { Tenant = tenant
          Text = "connection refused"
          Service = "nginx"
          Host = ""
          Status = ""
          Tags = Tag("k\"ey'", "v")
          From = noon.AddHours -1.0
          To = noon
          Limit = 5000 }

    let rows, count = searchLogs search
    let where = rows.Text.Substring(rows.Text.IndexOf " WHERE")
    Assert.StartsWith(" WHERE tenant_id = {p0:String}", where)
    Assert.Equal(2, where.Split("hasToken(message, ").Length - 1)
    Assert.True(where.IndexOf "hasToken" < where.IndexOf "position(message")
    Assert.DoesNotContain("'", rows.Text.Replace("toUnixTimestamp64Milli", ""))
    Assert.DoesNotContain("\"", rows.Text)

    Assert.Equal<SqlValue list>(
        [ SqlValue.String "default"
          SqlValue.Int64(search.From.ToUnixTimeMilliseconds())
          SqlValue.Int64(search.To.ToUnixTimeMilliseconds())
          SqlValue.String "nginx"
          SqlValue.String "connection"
          SqlValue.String "refused"
          SqlValue.String "connection refused"
          SqlValue.String "k\"ey'"
          SqlValue.String "v"
          // The limit is capped.
          SqlValue.Int64 1000L ],
        values rows
    )

    Assert.StartsWith("SELECT count() FROM logs WHERE tenant_id = {p0:String}", count.Text)
    Assert.Equal(9, count.Parameters.Length)

[<Fact>]
let ``lookups bound their limits`` () =
    Assert.Contains(SqlValue.Int64 500L, values (listMetrics tenant "" 0))
    Assert.Contains(SqlValue.Int64 500L, values (listMetrics tenant "cpu" 5000))
    Assert.Contains(SqlValue.Int64 50L, values (listMetrics tenant "cpu" 50))
    Assert.Contains("positionCaseInsensitive(metric, {p1:String}) > 0", (listMetrics tenant "cpu" 50).Text)
    Assert.Contains(SqlValue.Int64 200L, values (listTagValues tenant "m" "env" "" 0))
    Assert.Contains("GROUP BY v ORDER BY c DESC, v LIMIT 50", (logFacet tenant noon noon ByStatus).Text)
