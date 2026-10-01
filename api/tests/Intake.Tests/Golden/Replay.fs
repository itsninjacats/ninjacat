/// Replays one golden fixture — a request the Go server's tests sent — and
/// reports where the F# intake answers or stores differently.
module NinjaCat.Api.Intake.Tests.Golden.Replay

open System
open System.IO
open System.Text.Json.Nodes
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
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

/// The fixtures that can be replayed: served by route sets the intake has.
/// (A few Go tests built an engine around a route of their own; those are
/// ported by hand instead.)
let replayable: Fixture list =
    let known = Routes.routeSets |> List.map _.GoName |> Set.ofList
    all |> List.filter (fun f -> not f.RouteSets.IsEmpty && f.RouteSets |> List.forall known.Contains)

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
            Routes.create deps
        elif fixture.RouteSets = [ "routeRUM" ] then
            // Go's RUM tests went through what sits above that router too.
            Routers.Rum.wrap (Routes.byGoNames deps fixture.RouteSets)
        else
            Routes.byGoNames deps fixture.RouteSets

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

        if header.Key <> "Content-Length" && not (ignored.Contains path) then
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
