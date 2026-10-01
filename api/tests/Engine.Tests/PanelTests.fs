module NinjaCat.Api.Engine.Tests.PanelTests

open System
open Xunit
open NinjaCat.Api.Engine
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
let ``tags of one key become one filter, in order of first appearance`` () =
    Assert.Equal<TagFilter list>([], ok (parseTagFilters []))
    Assert.Equal<TagFilter list>([ { Key = "env"; Values = [ "prod" ] } ], ok (parseTagFilters [ "env:prod" ]))

    Assert.Equal<TagFilter list>(
        [ { Key = "env"; Values = [ "prod" ] }; { Key = "kube_service"; Values = [ "a"; "b" ] } ],
        ok (parseTagFilters [ "env:prod"; "kube_service:a"; "kube_service:b" ])
    )

[<Fact>]
let ``a tag value keeps its colons`` () =
    Assert.Equal<TagFilter list>([ { Key = "image"; Values = [ "nginx:1.25" ] } ], ok (parseTagFilters [ "image:nginx:1.25" ]))

[<Theory>]
[<InlineData("envprod")>]
[<InlineData(":prod")>]
[<InlineData("env:")>]
let ``a tag without key or value is refused`` (tag: string) =
    Assert.True(Result.isError (parseTagFilters [ tag ]))

// --- times and steps ------------------------------------------------------------------

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
let ``the step is honoured, derived, widened and never under a second`` () =
    // 360 points at 60 s: under the ceiling, honoured.
    Assert.Equal(TimeSpan.FromMinutes 1.0, resolveStep "60s" (TimeSpan.FromHours 6.0))
    // A month at 1 s would be 2.6 M points: widened to span / maxPoints.
    Assert.Equal(TimeSpan.FromSeconds 1296.0, resolveStep "1s" (TimeSpan.FromDays 30.0))
    // No step: about 300 points.
    Assert.Equal(TimeSpan.FromMinutes 1.0, resolveStep "" (TimeSpan.FromHours 5.0))
    Assert.Equal(TimeSpan.FromSeconds 1.0, resolveStep "" (TimeSpan.FromMinutes 1.0))

// --- group by --------------------------------------------------------------------------

[<Fact>]
let ``group-by keys are deduplicated and sorted`` () =
    Assert.Equal<string list>([], ok (normalizeGroupBy []))
    Assert.Equal<string list>([ "env"; "service" ], ok (normalizeGroupBy [ "service"; "env" ]))
    Assert.Equal<string list>([ "env"; "service" ], ok (normalizeGroupBy [ "env"; "env"; "service" ]))
    Assert.Equal<string list>([ "a"; "b"; "c"; "d" ], ok (normalizeGroupBy [ "a"; "b"; "c"; "d" ]))

[<Fact>]
let ``five keys are refused, not truncated, and so is an empty key`` () =
    match normalizeGroupBy [ "a"; "b"; "c"; "d"; "pod_name" ] with
    | Error e -> Assert.Contains("too many group-by keys", e)
    | Ok keys -> Assert.Fail $"got %A{keys}"

    match normalizeGroupBy [ "env"; "" ] with
    | Error e -> Assert.Contains("must not be empty", e)
    | Ok keys -> Assert.Fail $"got %A{keys}"

// --- SQL ---------------------------------------------------------------------------------

let private series: SeriesQuery =
    { Tenant = tenant
      Metric = "system.cpu.user"
      Hosts = []
      From = noon.AddHours -1.0
      To = noon
      Step = TimeSpan.FromSeconds 20.0
      Aggregation = "avg"
      Tags = []
      GroupBy = [] }

[<Fact>]
let ``a cold tag key is a membership test on the map, several values hasAny`` () =
    let sql, _ = ok (querySeries { series with Tags = [ { Key = "team"; Values = [ "core" ] }; { Key = "zone"; Values = [ "a"; "b" ] } ] })
    Assert.Contains("AND has(tags[{p5:String}], {p6:String}) AND hasAny(tags[{p7:String}], {p8:Array(String)})", sql.Text)
    Assert.Contains(SqlValue.String "team", values sql)
    Assert.Contains(SqlValue.StringArray [ "a"; "b" ], values sql)

[<Fact>]
let ``a hot tag key uses its column in metrics, and the map in logs`` () =
    let sql, _ = ok (querySeries { series with Tags = [ { Key = "env"; Values = [ "prod" ] }; { Key = "service"; Values = [ "web"; "api" ] } ] })
    Assert.Contains("AND env = {p5:String} AND service IN {p6:Array(String)}", sql.Text)

    let logs: LogSearch =
        { Tenant = tenant; Text = ""; Service = ""; Host = ""; Status = ""; Tags = [ { Key = "env"; Values = [ "prod" ] } ]; From = noon.AddHours -1.0; To = noon; Limit = 0 }

    let rows, _ = ok (searchLogs logs)
    Assert.Contains("AND has(tags[{p3:String}], {p4:String})", rows.Text)

[<Fact>]
let ``a filter without key or values is refused`` () =
    Assert.True(Result.isError (querySeries { series with Tags = [ { Key = ""; Values = [ "x" ] } ] }))
    Assert.True(Result.isError (querySeries { series with Tags = [ { Key = "env"; Values = [] } ] }))

[<Fact>]
let ``the caller's text is bound, never part of the SQL`` () =
    let evil = "env'] , 1); DROP TABLE metrics; --"
    let sql, keys = ok (querySeries { series with Tags = [ { Key = evil; Values = [ "x'y" ] } ]; GroupBy = [ evil ] })
    Assert.DoesNotContain("DROP", sql.Text)
    Assert.DoesNotContain("'", sql.Text)
    Assert.Equal<string list>([ evil ], keys)
    Assert.Contains("arrayJoin(tags[{p0:String}]) AS g0", sql.Text)
    Assert.Contains(SqlValue.String evil, values sql)
    Assert.Contains(SqlValue.String "x'y", values sql)

[<Fact>]
let ``series split by host unless grouped, and an unknown aggregation is avg`` () =
    let byHost, _ = ok (querySeries { series with Aggregation = "median" })
    Assert.StartsWith("SELECT host, ", byHost.Text)
    Assert.Contains("avg(value) AS value", byHost.Text)
    Assert.EndsWith("GROUP BY host, bucket ORDER BY host, bucket", byHost.Text)

    let grouped, keys = ok (querySeries { series with Aggregation = "MAX"; GroupBy = [ "service"; "env" ] })
    Assert.Equal<string list>([ "env"; "service" ], keys)
    Assert.Contains("max(value) AS value", grouped.Text)
    Assert.EndsWith("GROUP BY g0, g1, bucket ORDER BY g0, g1, bucket", grouped.Text)

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
          Tags = [ { Key = "k\"ey'"; Values = [ "v" ] } ]
          From = noon.AddHours -1.0
          To = noon
          Limit = 5000 }

    let rows, count = ok (searchLogs search)
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
