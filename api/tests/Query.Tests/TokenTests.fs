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
/// the monitors', and Datadog's query API. Each GET names another tenant in
/// its query string, which must not be believed.
let private guarded: (HttpMethod * string * string) list =
    [ HttpMethod.Get, "/internal/metrics/names?tenant=default", ""
      HttpMethod.Get, "/internal/metrics/tags?metric=system.cpu.user&tenant=default", ""
      HttpMethod.Get, "/internal/metrics/tag-values?metric=system.cpu.user&key=host&tenant=default", ""
      HttpMethod.Get, "/internal/metrics/query?metric=system.cpu.user&tenant=default", ""
      HttpMethod.Get, "/internal/logs/search?tenant=default", ""
      HttpMethod.Get, "/internal/logs/facets?tenant=default", ""
      HttpMethod.Get, "/internal/monitors/status?ids=1,2&tenant=default", ""
      HttpMethod.Get, "/internal/monitors/7/groups?tenant=default", ""
      HttpMethod.Get, "/internal/monitors/7/transitions?from=-1d&group=host:a&tenant=default", ""
      HttpMethod.Get, "/internal/monitors/7/evaluations?from=-1d&tenant=default", ""
      HttpMethod.Get, "/api/v1/metrics?from=0&tenant=default", ""
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
let private refusedEverywhere (label: string) (bearer: string option) : Task =
    withServer config (fun client clickhouse ->
        task {
            for route in validate :: guarded do
                use! response = send client bearer route
                let _, path, _ = route
                Assert.True(response.StatusCode = HttpStatusCode.Unauthorized, $"{label}, {path}: {response.StatusCode}")

            Assert.Empty clickhouse.Parameters
        })

[<Fact>]
let ``a token that is not the panel's, or names not exactly one tenant, is 401`` () =
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

    let token = panelToken ()

    task {
        for label, bearer in
            [ "no token", None
              "unsigned", Some unsigned
              "another key", Some(mint { token with Key = "another-key-another-key-another-key" })
              "another audience", Some(mint { token with Audience = "ninjacat-intake" })
              "another issuer", Some(mint { token with Issuer = "someone-else" })
              "expired", Some(mint { token with Expires = DateTime.UtcNow.AddMinutes -1.0 })
              "HS512", Some(mint { token with Algorithm = SecurityAlgorithms.HmacSha512 })
              "no tenant", Some(mint { token with Claims = [ "sub", box "user-1"; "role", box "member" ] })
              "empty tenant", Some(mint { token with Claims = [ "sub", box "user-1"; "tenant", box "" ] })
              "two tenants", Some(mint { token with Claims = [ "sub", box "user-1"; "tenant", box [| "acme"; "default" |] ] }) ] do
            do! refusedEverywhere label bearer
    }

[<Fact>]
let ``every guarded route queries for the tenant the token names, not one in the query string`` () =
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
                Assert.False(Seq.contains "default" clickhouse.Parameters, $"{path}: the query string's tenant was bound")
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
    QueryHost.addServices { config with QueryTokenSecret = String('x', 32) } (ServiceCollection())
