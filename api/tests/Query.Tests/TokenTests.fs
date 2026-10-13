/// The query port's door: every route but the probes asks for the panel's
/// token, and a query runs for the tenant the token names, never another.
module NinjaCat.Api.Query.Tests.TokenTests

open System
open System.Net
open System.Net.Http
open System.Threading.Tasks
open Microsoft.Extensions.DependencyInjection
open Microsoft.IdentityModel.JsonWebTokens
open Microsoft.IdentityModel.Tokens
open Xunit
open NinjaCat.Api.Server
open NinjaCat.Api.Query.Tests.TestServer

/// One request to each kind of guarded route that queries: the panel's,
/// the monitors', and Datadog's query API, read and planned.
let private guarded: (HttpMethod * string * string) list =
    [ HttpMethod.Get, "/internal/metrics/names", ""
      HttpMethod.Get, "/internal/metrics/tags?metric=system.cpu.user", ""
      HttpMethod.Get, "/internal/metrics/tag-values?metric=system.cpu.user&key=host", ""
      HttpMethod.Get, "/internal/metrics/query?metric=system.cpu.user", ""
      HttpMethod.Get, "/internal/logs/search", ""
      HttpMethod.Get, "/internal/logs/facets", ""
      HttpMethod.Get, "/internal/monitors/status?ids=1,2", ""
      HttpMethod.Get, "/internal/monitors/7/groups", ""
      HttpMethod.Get, "/internal/monitors/7/transitions?from=-1d&group=host:a", ""
      HttpMethod.Get, "/internal/monitors/7/evaluations?from=-1d", ""
      HttpMethod.Get, "/api/v1/metrics?from=0", ""
      HttpMethod.Post,
      "/api/v2/query/timeseries",
      """{"data":{"type":"timeseries_request","attributes":{"from":1700000000000,"to":1700003600000,"queries":[{"data_source":"metrics","name":"a","query":"avg:system.cpu.user{*}"}]}}}"""
      HttpMethod.Post,
      "/api/v2/query/scalar",
      """{"data":{"type":"scalar_request","attributes":{"from":1700000000000,"to":1700003600000,"queries":[{"data_source":"metrics","name":"a","query":"avg:system.cpu.user{*}","aggregator":"avg"}]}}}""" ]

/// Validation reads nothing, but answers only the panel.
let private validate =
    HttpMethod.Post, "/internal/monitors/validate", """{"type":"query alert","query":"avg(last_5m):avg:system.cpu.user{*} > 90"}"""

/// Every guarded route answers 401 with this bearer (or none), and nothing
/// reaches ClickHouse.
let private refusedEverywhere (bearer: string option) : Task =
    withServer config (fun client clickhouse ->
        task {
            for route in validate :: guarded do
                use! response = send client bearer route
                let _, path, _ = route
                Assert.True(response.StatusCode = HttpStatusCode.Unauthorized, $"{path}: {response.StatusCode}")

            Assert.Empty clickhouse.Parameters
        })

[<Fact>]
let ``without a token every route but the probes is 401`` () = refusedEverywhere None

[<Fact>]
let ``a token signed with another key is 401`` () =
    refusedEverywhere (Some(mint { panelToken () with Key = "another-key-another-key-another-key" }))

[<Fact>]
let ``a token for another audience or from another issuer is 401`` () =
    task {
        do! refusedEverywhere (Some(mint { panelToken () with Audience = "ninjacat-intake" }))
        do! refusedEverywhere (Some(mint { panelToken () with Issuer = "someone-else" }))
    }

[<Fact>]
let ``a token signed with the right key but not HS256 is 401`` () =
    task {
        // HS512 wants a key of 64 bytes or more.
        let longSecret = secret + secret

        do!
            withServer
                { config with QueryTokenSecret = longSecret }
                (fun client clickhouse ->
                    task {
                        let token = mint { panelToken () with Key = longSecret; Algorithm = SecurityAlgorithms.HmacSha512 }
                        use! response = send client (Some token) (HttpMethod.Get, "/internal/metrics/names", "")
                        Assert.Equal(HttpStatusCode.Unauthorized, response.StatusCode)
                        Assert.Empty clickhouse.Parameters
                    })
    }

[<Fact>]
let ``an expired token is 401`` () =
    refusedEverywhere (Some(mint { panelToken () with Expires = DateTime.UtcNow.AddMinutes -1.0 }))

[<Fact>]
let ``an unsigned token is 401`` () =
    let unsigned =
        JsonWebTokenHandler()
            .CreateToken(
                SecurityTokenDescriptor(
                    Issuer = QueryToken.issuer,
                    Audience = QueryToken.audience,
                    Expires = Nullable(DateTime.UtcNow.AddSeconds 60.0),
                    Claims = dict [ "tenant", box "acme" ]
                )
            )

    refusedEverywhere (Some unsigned)

[<Fact>]
let ``a token that names no tenant is 401`` () =
    task {
        do! refusedEverywhere (Some(mint { panelToken () with Claims = [ "sub", box "user-1"; "role", box "member" ] }))
        do! refusedEverywhere (Some(mint { panelToken () with Claims = [ "sub", box "user-1"; "tenant", box "" ] }))
    }

[<Fact>]
let ``a token that names more than one tenant is 401`` () =
    refusedEverywhere (Some(mint { panelToken () with Claims = [ "sub", box "user-1"; "tenant", box [| "acme"; "default" |] ] }))

[<Fact>]
let ``every guarded route queries for the tenant the token names`` () =
    withServer config (fun client clickhouse ->
        task {
            let token = mint (panelToken ())

            for route in guarded do
                clickhouse.Parameters.Clear()
                use! response = send client (Some token) route
                let _, path, _ = route
                // Not OK everywhere: a log search with no rows from the count
                // query is a 500, which says nothing about the door.
                Assert.True(response.StatusCode <> HttpStatusCode.Unauthorized, $"{path}: {response.StatusCode}")
                Assert.True(Seq.contains "acme" clickhouse.Parameters, $"{path}: the query was not bound to the token's tenant")
        })

[<Fact>]
let ``a tenant in the query string is not believed`` () =
    withServer config (fun client clickhouse ->
        task {
            use! response = send client (Some(mint (panelToken ()))) (HttpMethod.Get, "/internal/metrics/names?tenant=default", "")
            Assert.Equal(HttpStatusCode.OK, response.StatusCode)
            Assert.Contains("acme", clickhouse.Parameters)
            Assert.DoesNotContain("default", clickhouse.Parameters)
        })

[<Fact>]
let ``the probes need no token`` () =
    withServer config (fun client _ ->
        task {
            use! ping = client.GetAsync "/ping"
            Assert.Equal(HttpStatusCode.OK, ping.StatusCode)
            let! body = ping.Content.ReadAsStringAsync()
            Assert.Equal("pong", body)

            // 503 here, Postgres being unreachable; what matters is that the
            // handler answered.
            use! health = client.GetAsync "/health"
            let! body = health.Content.ReadAsStringAsync()
            Assert.NotEqual(HttpStatusCode.Unauthorized, health.StatusCode)
            Assert.Contains("\"postgres\"", body)
        })

[<Fact>]
let ``the query process does not start with a short secret`` () =
    let services = ServiceCollection()
    let error = Assert.Throws<Exception>(fun () -> QueryHost.addServices { config with QueryTokenSecret = "too-short" } services)
    Assert.Contains("NINJACAT_QUERY_TOKEN_SECRET", error.Message)
