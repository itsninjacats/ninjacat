/// Replays one golden fixture — a request the Go server's tests sent — and
/// reports where the F# intake answers or stores differently.
module NinjaCat.Api.Intake.Tests.Golden.Replay

open System
open System.IO
open System.Text.Json.Nodes
open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Hosting
open Microsoft.AspNetCore.Http
open Microsoft.AspNetCore.HttpOverrides
open Microsoft.AspNetCore.TestHost
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Oxpecker
open NinjaCat.Api.Storage
open Microsoft.Extensions.Logging
open System.Security.Claims
open NinjaCat.Api.Intake

/// The keys the Go tests used: one for handler tests, one for the tests of
/// the host dispatch.
let testKey = "00000000000000000000000000000001"
let testTenant = "test"

let testStore () : ApiKeys.Store =
    let store = ApiKeys.Store()

    store.Publish
        [ ApiKeys.hash testKey, ({ ID = "test"; Name = "test"; TenantID = testTenant }: ApiKeys.Key)
          ApiKeys.hash "routes-test-key", ({ ID = "k1"; Name = "routes-test"; TenantID = "tenant-1" }: ApiKeys.Key) ]

    store

let private fixturesRoot = Path.Combine(AppContext.BaseDirectory, "Fixtures")

type Fixture =
    { /// "TestName/001".
      Id: string
      Json: JsonNode
      /// The Go route sets of the engine that served the request.
      RouteSets: string list }

let all: Fixture list =
    Directory.GetFiles(Path.Combine(fixturesRoot, "go"), "*.json", SearchOption.AllDirectories)
    |> Array.sort
    |> Array.map (fun path ->
        let json = JsonNode.Parse(File.ReadAllText path)

        { Id = Path.GetFileName(Path.GetDirectoryName path) + "/" + Path.GetFileNameWithoutExtension path
          Json = json
          RouteSets = json["routes"].AsArray() |> Seq.map (fun n -> n.GetValue<string>()) |> List.ofSeq })
    |> List.ofArray

/// The services the intake's handlers ask for, as the server registers them.
let private addServices (services: IServiceCollection) (deps: Deps) : unit =
    services
        .AddSingleton<ApiKeys.Store>(deps.Store)
        .AddSingleton<ISink>(deps.Sink)
        .AddSingleton<IntakeSettings>({ AckUnknown = deps.AckUnknown })
        .AddRouting()
        .AddCors()
        .AddOxpecker()
    |> ignore

    ApiKeyAuth.addTo services

    // The peer every recorded request comes from is a proxy the server
    // trusts, as an operator would name theirs.
    services.Configure<ForwardedHeadersOptions>(fun (options: ForwardedHeadersOptions) ->
        options.ForwardedHeaders <- ForwardedHeaders.XForwardedFor
        options.ForwardLimit <- Nullable()
        options.KnownProxies.Add(Net.IPAddress.Parse "192.0.2.1"))
    |> ignore

/// Sends one request through an in-memory server with the given pipeline
/// and returns the answer. `http` describes the request; `body` is its body.
let through (deps: Deps) (configure: IApplicationBuilder -> unit) (http: HttpContext) (body: byte[]) : Response =
    let builder = WebApplication.CreateEmptyBuilder(WebApplicationOptions())
    builder.WebHost.UseTestServer() |> ignore
    addServices builder.Services deps
    use app = builder.Build()
    configure app
    app.StartAsync().GetAwaiter().GetResult()
    let server = app.GetTestServer()

    let answered =
        server
            .SendAsync(fun sent ->
                sent.Connection.RemoteIpAddress <- http.Connection.RemoteIpAddress
                sent.Request.Method <- http.Request.Method
                sent.Request.Host <- http.Request.Host
                sent.Request.Path <- http.Request.Path
                sent.Request.QueryString <- http.Request.QueryString

                for header in http.Request.Headers do
                    sent.Request.Headers[header.Key] <- header.Value

                sent.Request.Body <- new MemoryStream(body))
            .GetAwaiter()
            .GetResult()

    use received = new MemoryStream()
    answered.Response.Body.CopyTo received

    { Status = answered.Response.StatusCode
      // Content-Length is the transport's, not the handler's.
      Headers =
        [ for header in answered.Response.Headers do
              if header.Key <> "Content-Length" then
                  header.Key, header.Value.ToString() ]
      Body = received.ToArray() }

/// A context for calling one handler directly, with no server around it:
/// the services it asks for, and the tenant its key would have given it.
let contextFor (deps: Deps) (tenant: string) : HttpContext =
    let services = ServiceCollection()
    services.AddLogging() |> ignore
    addServices services deps
    let ctx = DefaultHttpContext()
    ctx.RequestServices <- services.BuildServiceProvider()
    ctx.Response.Body <- new MemoryStream()

    if tenant <> "" then
        ctx.User <- ClaimsPrincipal(ClaimsIdentity([ Claim("tenant", tenant); Claim("key_id", "test") ], "test"))

    ctx

/// What a handler called directly wrote into its context.
let answerOf (ctx: HttpContext) : Response =
    { Status = ctx.Response.StatusCode
      Headers =
        [ for header in ctx.Response.Headers do
              if header.Key <> "Content-Length" then
                  header.Key, header.Value.ToString() ]
      Body = (ctx.Response.Body :?> MemoryStream).ToArray() }

/// The Go server's name for each group of routes. The fixtures recorded
/// which groups served a request under these names; nothing else uses them.
/// Beside each: the authentication scheme its host asks for.
let private goRouteSets: Map<string, string option * Endpoint list> =
    let agent = Some ApiKeyAuth.agent

    Map
        [ "routeAPI", (agent, Routes.metricsAndChecks)
          "routeCIVisibilityAPI", (agent, Routes.ciVisibilityApi)
          "routeApp", (agent, Routes.metricsV3)
          "routeTrace", (agent, Routes.trace)
          "routeLogs", (agent, Routes.logs)
          "routeProcess", (agent, Routes.processes)
          "routeKubeops", (agent, Routes.kubeops)
          "routeConfig", (agent, Routes.remoteConfig)
          "routeContainers", (agent, Routes.containers)
          "routeDBM", (agent, Routes.dbm)
          "routeNDM", (agent, Routes.ndm)
          "routeResources", (agent, Routes.resources)
          "routeTelemetry", (agent, Routes.telemetry)
          "routeProfile", (agent, Routes.profile)
          "routeDebugger", (agent, Routes.debugger)
          "routeSourcemap", (agent, Routes.sourcemap)
          "routeCWS", (agent, Routes.cws)
          "routeRuntimeSecurity", (agent, Routes.runtimeSecurity)
          "routeCSPM", (agent, Routes.cspm)
          "routeSBOM", (agent, Routes.sbom)
          "routeSDS", (agent, Routes.sds)
          "routeAgentDiscovery", (agent, Routes.agentDiscovery)
          "routeAgentHealth", (agent, Routes.agentHealth)
          "routeEventManagement", (agent, Routes.eventManagement)
          "routeSoftwareInventory", (agent, Routes.softwareInventory)
          "routeSynthetics", (agent, Routes.syntheticsResults)
          "routeCITestCycle", (agent, Routes.ciTestCycle)
          "routeCITestCov", (agent, Routes.ciTestCoverage)
          "routeCIWebhook", (Some ApiKeyAuth.ciWebhook, Routes.ciWebhook)
          "routeSyntheticsAgent", (agent, Routes.syntheticsPoller)
          "routeDataObs", (Some ApiKeyAuth.lineage, Routes.dataObs)
          "routeRUM", (Some ApiKeyAuth.browser, Routes.rum)
          "routeAIUsage", (agent, Routes.aiUsage)
          "routeLLMObs", (agent, Routes.llmObs)
          "routeInstall", (None, Routes.install) ]

/// The intake made of the named Go route sets, guarded as the first one is.
let byGoNames (deps: Deps) (names: string list) : HttpContext -> byte[] -> Response =
    let sets = names |> List.map (fun name -> goRouteSets[name])
    through deps (fun app -> Routes.mount app (fst sets.Head) (sets |> List.collect snd))

/// browser-intake as the server mounts it: the gate above the router.
let browserIntake (deps: Deps) (allowedOrigins: string list) : HttpContext -> byte[] -> Response =
    through deps (fun app ->
        app.UseForwardedHeaders() |> ignore
        Routes.mountBrowser app allowedOrigins)

/// Every host, as the server mounts them.
let wholeIntake (deps: Deps) : HttpContext -> byte[] -> Response = through deps Routes.configure

/// The fixtures that can be replayed: served by route sets the intake has.
/// (A few Go tests built an engine around a route of their own; those are
/// ported by hand instead.)
let replayable: Fixture list =
    all |> List.filter (fun f -> not f.RouteSets.IsEmpty && f.RouteSets |> List.forall goRouteSets.ContainsKey)

/// The fixtures to replay: every replayable one, or with
/// NINJACAT_GOLDEN_ROUTES set (comma-separated Go route set names) only those
/// served by one of them.
let selected: Fixture list =
    match Environment.GetEnvironmentVariable "NINJACAT_GOLDEN_ROUTES" with
    | null
    | "" -> replayable
    | names ->
        let wanted = names.Split ',' |> Set.ofArray
        replayable |> List.filter (fun f -> f.RouteSets |> List.exists wanted.Contains)

/// Differences accepted for a fixture, each with its reason, from
/// Fixtures/overrides.json.
let private overrides: JsonNode =
    JsonNode.Parse(File.ReadAllText(Path.Combine(fixturesRoot, "overrides.json")))

let private strings (node: JsonNode) : string list =
    if isNull node then [] else node.AsArray() |> Seq.map (fun n -> n.GetValue<string>()) |> List.ofSeq

let private request (fixture: JsonNode) : HttpContext * byte[] =
    let http = DefaultHttpContext()
    let request = fixture["request"]
    let url = request["url"].GetValue<string>()

    let path, query =
        match url.IndexOf '?' with
        | -1 -> url, ""
        | i -> url.Substring(0, i), url.Substring i

    // The peer of every Go httptest request.
    http.Connection.RemoteIpAddress <- Net.IPAddress.Parse "192.0.2.1"
    http.Request.Method <- request["method"].GetValue<string>()
    http.Request.Host <- HostString(request["host"].GetValue<string>())
    http.Request.Path <- PathString(Uri.UnescapeDataString path)
    http.Request.QueryString <- QueryString query

    for header in request["headers"].AsObject() do
        http.Request.Headers[header.Key] <- StringValues(strings header.Value |> Array.ofList)

    http, Convert.FromBase64String(request["body_b64"].GetValue<string>())

/// The column names of an INSERT statement's column list.
let private insertColumns (insert: string) : string list =
    let opening = insert.IndexOf '('
    let closing = insert.IndexOf(')', opening)

    insert.Substring(opening + 1, closing - opening - 1).Split ','
    |> Array.map _.Trim()
    |> List.ofArray

/// Sends the fixture's request to the F# intake; returns the answer and what
/// was written.
let send (fixture: Fixture) : Response * CapturedWrite list =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let http, body = request fixture.Json

    // Most Go tests built one engine and sent to "example.com"; the ones that
    // went through the host dispatch carry a real intake host.
    let intake =
        if http.Request.Host.Host <> "example.com" then
            wholeIntake deps
        elif fixture.RouteSets = [ "routeRUM" ] then
            // Go's RUM tests went through what sits above that router too.
            browserIntake deps []
        else
            byGoNames deps fixture.RouteSets

    intake http body, sink.Writes

/// Runs the fixture's request; returns the differences from what Go did.
let run (fixture: Fixture) : string list =
    let json = fixture.Json
    let response, writes = send fixture

    let ignored =
        Set.ofList (strings json["volatile"] @ strings (overrides[fixture.Id] |> Option.ofObj |> Option.map (fun o -> o["ignore"]) |> Option.toObj))

    let found = ResizeArray<string>()
    let expected = json["response"]

    // --- the response ---

    let expectedStatus = expected["status"].GetValue<int>()

    if expectedStatus <> response.Status then
        found.Add $"/response/status: Go {expectedStatus}, F# {response.Status}"

    for header in expected["headers"].AsObject() do
        let path = $"/response/headers/{header.Key}"

        // A volatile header is listed by the path of its value, `…/<name>/0`.
        if header.Key <> "Content-Length" && not (ignored.Contains path) && not (ignored.Contains(path + "/0")) then
            let want = strings header.Value

            let got =
                response.Headers
                |> List.filter (fun (name, _) -> name.Equals(header.Key, StringComparison.OrdinalIgnoreCase))
                |> List.map snd

            if want <> got then
                found.Add $"{path}: Go %A{want}, F# %A{got}"

    let expectedBody = Convert.FromBase64String(expected["body_b64"].GetValue<string>())

    if not (isNull expected["body_json"]) then
        let actual =
            try
                JsonNode.Parse(ReadOnlySpan response.Body)
            with _ ->
                JsonValue.Create(Text.Encoding.UTF8.GetString response.Body)

        found.AddRange(Compare.differences ignored "/response/body_json" expected["body_json"] actual)
    elif not (ignored.Contains "/response/body_b64") && expectedBody <> response.Body then
        found.Add $"/response/body_b64: Go {expectedBody.Length} bytes, F# {response.Body.Length} bytes, and they differ"

    // --- what was sent to storage ---

    // Null means the Go test ran without a capturing node: nothing to compare.
    if not (isNull json["sends"]) then
        let sends = json["sends"].AsArray()

        if sends.Count <> writes.Length then
            let goTables = sends |> Seq.map (fun s -> s["to"].GetValue<string>()) |> String.concat ", "
            let fsTables = writes |> List.map _.Writer |> String.concat ", "
            found.Add $"/sends: Go wrote {sends.Count} batches [{goTables}], F# wrote {writes.Length} [{fsTables}]"
        else
            for i in 0 .. sends.Count - 1 do
                let send = sends[i]
                let write = writes[i]
                let path = $"/sends/{i}"
                let writer = send["to"].GetValue<string>()

                if writer <> write.Writer then
                    found.Add $"{path}/to: Go {writer}, F# {write.Writer}"
                else
                    let columns = insertColumns (send["insert"].GetValue<string>())

                    if columns <> write.Columns then
                        found.Add $"{path}/insert: Go columns %A{columns}, F# columns %A{write.Columns}"

                    let rows = JsonArray(write.Rows |> Array.map Encode.value)
                    found.AddRange(Compare.differences ignored $"{path}/rows" send["rows"] rows)

                    let args = JsonArray(write.Args |> Array.map (fun values -> JsonArray(values |> Array.map Encode.value) :> JsonNode))
                    found.AddRange(Compare.differences ignored $"{path}/args" send["args"] args)

    List.ofSeq found
