module NinjaCat.Api.Engine.Tests.MetricsTests

open System
open Xunit
open NinjaCat.Api.Engine

let private req host : Metrics.ActiveMetricsRequest =
    { Tenant = TenantId "acme"
      From = DateTimeOffset.FromUnixTimeSeconds 1_700_000_000L
      Host = host }

[<Fact>]
let ``tenant and from are bound, never inlined`` () =
    let sql = Metrics.activeMetrics (req None)
    Assert.Contains("tenant_id = {tenant:String}", sql.Text)
    Assert.Contains("fromUnixTimestamp({from:Int64})", sql.Text)
    Assert.DoesNotContain("acme", sql.Text)
    Assert.Equal<(string * SqlValue) list>([ "tenant", String "acme"; "from", Int64 1_700_000_000L ], sql.Parameters)

[<Fact>]
let ``host filter appears only when a host is given`` () =
    Assert.DoesNotContain("host", (Metrics.activeMetrics (req None)).Text)

    let sql = Metrics.activeMetrics (req (Some "web-1"))
    Assert.Contains("AND host = {host:String}", sql.Text)
    Assert.Contains(("host", String "web-1"), sql.Parameters)

[<Fact>]
let ``every placeholder has a parameter and vice versa`` () =
    let sql = Metrics.activeMetrics (req (Some "web-1"))

    let placeholders =
        Text.RegularExpressions.Regex.Matches(sql.Text, @"\{(\w+):")
        |> Seq.map _.Groups[1].Value
        |> Set.ofSeq

    Assert.Equal<Set<string>>(placeholders, sql.Parameters |> List.map fst |> Set.ofList)
