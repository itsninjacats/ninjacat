/// The query process as `ninjacat-api query` builds it, in memory, and the
/// panel's token to call it with. ClickHouse is either a stand-in that keeps
/// the bound parameters, or a scratch database on the dev ClickHouse.
module NinjaCat.Api.Query.Tests.TestServer

open System
open System.Collections.Concurrent
open System.Net
open System.Net.Http
open System.Net.Http.Headers
open System.Text
open System.Threading.Tasks
open System.Web
open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Hosting
open Microsoft.AspNetCore.TestHost
open Microsoft.Extensions.DependencyInjection
open Microsoft.IdentityModel.JsonWebTokens
open Microsoft.IdentityModel.Tokens
open Xunit
open ClickHouse.Driver
open ClickHouse.Driver.ADO
open NinjaCat.Api.Server
open NinjaCat.Api.Platform

let secret = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"

let private env (name: string) (fallback: string) =
    match Environment.GetEnvironmentVariable name with
    | null
    | "" -> fallback
    | value -> value

let config: Config.Config =
    { IntakeAddr = ":8080"
      InternalAddr = ":8081"
      QueryTokenSecret = secret
      LogsTcpAddr = None
      TlsCertFile = ""
      TlsKeyFile = ""
      ClickHouseHttpAddr = env "CLICKHOUSE_HTTP_ADDR" "localhost:8123"
      ClickHouseDb = "ninjacat"
      ClickHouseUser = env "CLICKHOUSE_USER" "ninjacat"
      ClickHousePassword = env "CLICKHOUSE_PASSWORD" "ninjacat"
      // Nothing listens there: /health is the only route that connects,
      // and it is tested only for being open.
      DatabaseUrl = "postgres://test:test@127.0.0.1:1/test"
      AutoMigrate = false
      MetricsRetention = NinjaCat.Api.Engine.Query.Plan.defaultRetention
      TrustedProxies = []
      MaxBodyBytes = 1024L
      AckUnknown = false
      Debug = false
      CaptureDir = ""
      SelfMonitorInterval = TimeSpan.FromSeconds 15.0
      SelfMonitorTenant = "default"
      SelfMonitorHost = "" }

/// Stands in for ClickHouse's HTTP interface: keeps the bound parameters of
/// every query (`param_<name>` in the URL) and answers each with no rows.
type FakeClickHouse() =
    inherit HttpMessageHandler()

    member val Parameters = ConcurrentQueue<string>()

    override this.SendAsync(request: HttpRequestMessage, _) =
        let query = HttpUtility.ParseQueryString request.RequestUri.Query

        for name in query.AllKeys do
            if name.StartsWith "param_" then
                this.Parameters.Enqueue query[name]

        // RowBinaryWithNamesAndTypes with no columns: a zero column count.
        Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK, Content = new ByteArrayContent [| 0uy |]))

/// The server on the ClickHouse `config` names, or on `fake` when given.
let private serve (config: Config.Config) (fake: FakeClickHouse option) (test: HttpClient -> Task) : Task =
    task {
        let builder = WebApplication.CreateEmptyBuilder(WebApplicationOptions())
        builder.WebHost.UseTestServer() |> ignore
        QueryHost.addServices config builder.Services

        fake
        |> Option.iter (fun fake ->
            builder.Services.AddSingleton<ClickHouseClient>(fun _ ->
                new ClickHouseClient(ClickHouseClientSettings(Host = "clickhouse.test", HttpClient = new HttpClient(fake))))
            |> ignore)

        use app = builder.Build()
        QueryHost.configure app
        do! app.StartAsync()
        use client = app.GetTestClient()
        do! test client
        do! app.StopAsync()
    }

let withServer (config: Config.Config) (test: HttpClient -> FakeClickHouse -> Task) : Task =
    let clickhouse = new FakeClickHouse()
    serve config (Some clickhouse) (fun client -> test client clickhouse)

let private clickhouseClient (database: string) : ClickHouseClient =
    let address = config.ClickHouseHttpAddr.Split ':'

    new ClickHouseClient(
        ClickHouseClientSettings(
            Host = address[0],
            Port = UInt16.Parse address[1],
            Database = database,
            Username = config.ClickHouseUser,
            Password = config.ClickHousePassword,
            Timeout = TimeSpan.FromMinutes 1.0
        )
    )

/// The server on a database of its own on the dev ClickHouse, built from the
/// migrations and dropped afterwards; `fill` writes to it first. Skipped when
/// there is no ClickHouse.
let withScratchServer (fill: ClickHouseClient -> Task) (test: HttpClient -> Task) : Task =
    task {
        let admin = clickhouseClient "default"

        try
            let! _ = admin.PingAsync()
            ()
        with _ ->
            Assert.Skip "ClickHouse is not reachable"

        let database = "ninjacat_query_" + Guid.NewGuid().ToString "N"
        let! _ = admin.ExecuteNonQueryAsync($"CREATE DATABASE {database}")

        try
            let scratch = clickhouseClient database

            match! NinjaCat.Api.Storage.Migrations.apply scratch with
            | Error e -> Assert.Fail e
            | Ok _ -> ()

            do! fill scratch
            do! serve { config with ClickHouseDb = database } None test
        finally
            admin.ExecuteNonQueryAsync($"DROP DATABASE IF EXISTS {database}").GetAwaiter().GetResult() |> ignore
    }

/// A token as the panel mints it (frontend/src/lib/server/ninjacat.ts), and
/// what each test changes in it.
type Token =
    { Key: string
      Issuer: string
      Audience: string
      Expires: DateTime
      Algorithm: string
      Claims: (string * obj) list }

let panelToken () : Token =
    { Key = secret
      Issuer = QueryToken.issuer
      Audience = QueryToken.audience
      Expires = DateTime.UtcNow.AddSeconds 60.0
      Algorithm = SecurityAlgorithms.HmacSha256
      Claims = [ "sub", box "user-1"; "tenant", box "acme"; "role", box "member" ] }

let mint (token: Token) : string =
    JsonWebTokenHandler()
        .CreateToken(
            SecurityTokenDescriptor(
                Issuer = token.Issuer,
                Audience = token.Audience,
                IssuedAt = Nullable(token.Expires.AddSeconds -60.0),
                NotBefore = Nullable(token.Expires.AddSeconds -60.0),
                Expires = Nullable token.Expires,
                SigningCredentials = SigningCredentials(SymmetricSecurityKey(Encoding.UTF8.GetBytes token.Key), token.Algorithm),
                Claims = dict token.Claims
            )
        )

let send (client: HttpClient) (bearer: string option) (method: HttpMethod, path: string, body: string) : Task<HttpResponseMessage> =
    let request = new HttpRequestMessage(method, path)

    if body <> "" then
        request.Content <- new StringContent(body, Encoding.UTF8, "application/json")

    bearer |> Option.iter (fun token -> request.Headers.Authorization <- AuthenticationHeaderValue("Bearer", token))
    client.SendAsync request
