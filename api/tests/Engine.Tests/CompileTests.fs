module NinjaCat.Api.Engine.Tests.CompileTests

open System
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Query.Plan
open NinjaCat.Api.Engine.Query.Compile

let private plan =
    { From = DateTimeOffset.FromUnixTimeMilliseconds 1711977600000L
      To = DateTimeOffset.FromUnixTimeMilliseconds 1711981200000L
      Step = TimeSpan.FromSeconds 20.0
      Queries = []
      Outputs = [] }

let private query filter groupBy =
    { Metric = "system.cpu.user"
      Filter = filter
      GroupBy = groupBy
      SpaceAgg = Sum
      TimeAgg = Plain Avg
      Step = TimeSpan.FromSeconds 20.0
      Lookback = 0 }

let private compiled filter groupBy = compile (TenantId "acme") plan (query filter groupBy)

/// The value bound to the placeholder that follows `before` in the SQL text.
let private boundAfter (before: string) (sql: Sql) =
    let i = sql.Text.IndexOf before
    Assert.True(i >= 0, $"'{before}' not in:\n{sql.Text}")
    let start = i + before.Length + 1
    let name = sql.Text.Substring(start, sql.Text.IndexOf(':', start) - start)
    sql.Parameters |> List.find (fst >> (=) name) |> snd

[<Fact>]
let ``time first, then space`` () =
    let sql = compiled All []
    // Inner query: per series, the time aggregator; outer: across series, the space one.
    Assert.Contains("SELECT series_id, toStartOfInterval(timestamp, toIntervalSecond({step", sql.Text)
    Assert.Contains("avg(value) AS v", sql.Text)
    Assert.Contains("sum(v) AS value", sql.Text)
    Assert.Contains("GROUP BY series_id, bucket", sql.Text)

[<Fact>]
let ``request values are bound, never inlined`` () =
    let sql = compiled (Tag("env", "prod'; DROP TABLE metrics; --")) []
    Assert.DoesNotContain("DROP", sql.Text)
    Assert.DoesNotContain("acme", sql.Text)
    Assert.DoesNotContain("system.cpu.user", sql.Text)
    Assert.Equal(String "acme", boundAfter "tenant_id = " sql)
    Assert.Equal(String "system.cpu.user", boundAfter "metric = " sql)
    Assert.Equal(Int64 1711977600000L, boundAfter "fromUnixTimestamp64Milli(" sql)
    Assert.Equal(Int64 20L, boundAfter "toIntervalSecond(" sql)

[<Fact>]
let ``host is a column, other tags are in the map`` () =
    Assert.Contains("host = {p", (compiled (Tag("host", "web-1")) []).Text)
    let env = compiled (Tag("env", "prod")) []
    Assert.Contains("has(tags[{k", env.Text)
    Assert.Equal(String "env", boundAfter "has(tags[" env)

[<Fact>]
let ``wildcards become LIKE, with LIKE's own wildcards escaped`` () =
    let sql = compiled (Tag("kube_namespace", "prod_*")) []
    Assert.Contains("arrayExists(x -> x LIKE {p", sql.Text)
    Assert.Equal(String "prod\\_%", boundAfter "x LIKE " sql)

[<Fact>]
let ``a key-less tag asks for the key`` () =
    Assert.Contains("mapContains(tags, {k", (compiled (Bare "canary") []).Text)

[<Fact>]
let ``boolean structure is kept`` () =
    let sql = compiled (And [ Tag("env", "prod"); Not(Or [ Tag("host", "a"); In("zone", [ "x"; "y" ]) ]) ]) []
    Assert.Contains(") AND (NOT ((host = {p", sql.Text)
    Assert.Contains(") OR (hasAny(tags[{k", sql.Text)

[<Fact>]
let ``group by: host as a column, tags through arrayJoin, missing as NULL`` () =
    let sql = compiled All [ "host"; "role" ]
    Assert.Contains("nullIf(toString(host), '') AS g0", sql.Text)
    Assert.Contains("arrayJoin(if(empty(tags[{k", sql.Text)
    Assert.Contains("[NULL], CAST(tags[{k", sql.Text)
    Assert.Contains("GROUP BY g0, g1, bucket", sql.Text)
    Assert.Contains("GROUP BY series_id, g0, g1, bucket", sql.Text)

// --- time aggregation ------------------------------------------------------------

let private withTime agg (step: float) =
    compile (TenantId "acme") plan { query All [] with TimeAgg = agg; Step = TimeSpan.FromSeconds step }

[<Fact>]
let ``the query's own step is bound`` () =
    Assert.Equal(Int64 60L, boundAfter "toIntervalSecond(" (withTime (Plain Sum) 60.0))

[<Fact>]
let ``rollup methods`` () =
    Assert.Contains("sum(value) AS v", (withTime (Plain Sum) 20.0).Text)
    Assert.Contains("toFloat64(count()) AS v", (withTime (Plain Count) 20.0).Text)

[<Fact>]
let ``as_count follows the metric type`` () =
    let sql = (withTime AsCount 20.0).Text
    Assert.Contains("any(metric_type) = 'RATE', sum(value * greatest(interval, 1))", sql)
    Assert.Contains("any(metric_type) = 'COUNT', sum(value)", sql)
    Assert.Contains(", avg(value)) AS v", sql) // GAUGE: unaffected

[<Fact>]
let ``as_rate divides by the bucket, floored to a rate's interval`` () =
    let sql = withTime AsRate 20.0
    Assert.Contains("/ greatest({step", sql.Text)
    Assert.Contains("}, max(interval))", sql.Text)
    Assert.Contains("any(metric_type) = 'COUNT', sum(value) / {step", sql.Text)

[<Fact>]
let ``wildcards inside IN are patterns, the rest stay exact`` () =
    let sql = compiled (In("container_name", [ "ninjacat*"; "postgres" ])) []
    Assert.Contains("arrayExists(x -> x LIKE {p", sql.Text)
    Assert.Contains(") OR (hasAny(tags[{k", sql.Text)
    Assert.Equal(String "ninjacat%", boundAfter "x LIKE " sql)
    Assert.Equal(StringArray [ "postgres" ], boundAfter "], " sql)

[<Fact>]
let ``host IN with a wildcard`` () =
    let sql = compiled (Not(In("host", [ "web-*" ]))) []
    Assert.Contains("NOT ((host LIKE {p", sql.Text)

[<Fact>]
let ``lookback reads earlier, one bucket more`` () =
    let sql = compile (TenantId "acme") plan { query All [] with Lookback = 3 }
    // from − (3 + 1) × 20 s
    Assert.Equal(Int64(1711977600000L - 80000L), boundAfter "fromUnixTimestamp64Milli(" sql)
    Assert.Equal(1711977600000L - 60000L, keepFromMs plan { query All [] with Lookback = 3 })
