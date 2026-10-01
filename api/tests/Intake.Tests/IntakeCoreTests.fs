/// The parts every router stands on: the key guard, the host dispatch, raw
/// payloads, tags. Matching a path is the framework's router's job and is not
/// tested here.
module NinjaCat.Api.Intake.Tests.IntakeCoreTests

open System.Text
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Oxpecker
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

let private key = "0123456789abcdef0123456789abcdef"

let private store =
    let store = ApiKeys.Store()
    store.Publish [ ApiKeys.hash key, ({ ID = "1"; Name = "lab"; TenantID = "t1" }: ApiKeys.Key) ]
    store

let private deps (sink: CapturingSink) : Deps =
    { Store = store; Sink = sink; Log = NullLogger.Instance; AckUnknown = false }

let private request (method: string) (host: string) (target: string) (headers: (string * string) list) : HttpContext =
    let http = DefaultHttpContext()
    http.Request.Method <- method
    http.Request.Host <- HostString host

    match target.IndexOf '?' with
    | -1 -> http.Request.Path <- PathString target
    | i ->
        http.Request.Path <- PathString(target.Substring(0, i))
        http.Request.QueryString <- QueryString(target.Substring i)

    for name, value in headers do
        http.Request.Headers[name] <- StringValues value

    http

let private ok: Handler = fun _ -> Response.status 200

/// One host's router with the given endpoints, as the server mounts it.
let private send (sink: CapturingSink) (auth: Auth) (endpoints: Endpoint list) (http: HttpContext) : Response =
    Replay.through (deps sink) (fun app -> Routes.mount app (Routes.keyed auth) endpoints) http [||]

let private statusOf (auth: Auth) (endpoints: Endpoint list) (method: string) (target: string) (headers: (string * string) list) : int =
    (send (CapturingSink()) auth endpoints (request method "example.com" target headers)).Status

// --- the key guard -------------------------------------------------------------------

[<Fact>]
let ``the standard guard takes the header, then the api_key query parameter`` () =
    let status target headers =
        statusOf Auth.standard [ GET [ route "/x" (Routes.keyed Auth.standard ok) ] ] "GET" target headers

    Assert.Equal(200, status "/x" [ "Dd-Api-Key", key ])
    Assert.Equal(200, status ("/x?api_key=" + key) [])
    Assert.Equal(403, status "/x?api_key=wrong" [])
    Assert.Equal(403, status "/x" [])
    // Bearer is for the one intake whose clients send it.
    Assert.Equal(403, status "/x" [ "Authorization", "Bearer " + key ])

[<Fact>]
let ``key sources are tried in order, and a non-bearer credential is not a key`` () =
    let all = Auth.fromSources [ KeyFromHeader "Dd-Api-Key"; KeyFromBearer; KeyFromQuery "api_key" ]
    let status target headers = statusOf all [ GET [ route "/x" (Routes.keyed all ok) ] ] "GET" target headers
    Assert.Equal(200, status "/x" [ "Dd-Api-Key", key ])
    Assert.Equal(200, status "/x" [ "DD-API-KEY", key ])
    Assert.Equal(200, status "/x" [ "Authorization", "Bearer " + key ])
    Assert.Equal(200, status "/x" [ "Authorization", "bearer " + key ])
    Assert.Equal(403, status "/x" [ "Authorization", "Basic " + key ])
    Assert.Equal(200, status ("/x?api_key=" + key) [])
    Assert.Equal(403, status "/x" [])

[<Fact>]
let ``the probes need no key, an unknown path is answered before the guard`` () =
    let status method target =
        statusOf Auth.standard [ POST [ route "/x" (Routes.keyed Auth.standard ok) ] ] method target []

    Assert.Equal(200, status "GET" "/ping")
    Assert.Equal(200, status "GET" "/_health")
    Assert.Equal(404, status "POST" "/nowhere")
    // A known path under another method is the router's own 405.
    Assert.Equal(405, status "GET" "/x")
    Assert.Equal(403, status "POST" "/x")
    // The router does not tell /x from /x/ or from /X.
    Assert.Equal(403, status "POST" "/x/")
    Assert.Equal(403, status "POST" "/X")

[<Theory>]
[<InlineData("agent-http-intake.logs.x", "/api/v2/logs", "routeLogs", """[{"message":"m","deep":DEEP}]""")>]
[<InlineData("event-management-intake.x", "/api/v2/events", "routeEventManagement", """{"data":{"attributes":{"title":"t","attributes":{"x":DEEP}}}}""")>]
[<InlineData("kubeops-intake.x", "/api/v2/kubeactions", "routeKubeops", """[{"action_id":"a","deep":DEEP}]""")>]
[<InlineData("app.x", "/api/v1/series", "routeAPI", """{"series":[{"metric":"m","points":[[1790000000,2]],"deep":DEEP}]}""")>]
let ``a body nested deeper than any telemetry is kept raw; one inside the limit is stored; neither is a 500``
    (_host: string, path: string, routeSet: string, template: string)
    =
    let send (depth: int) =
        let deep = System.String('[', depth) + System.String(']', depth)
        let sink = Golden.CapturingSink()

        let deps: Deps =
            { Store = Golden.Replay.testStore ()
              Sink = sink
              Log = Microsoft.Extensions.Logging.Abstractions.NullLogger.Instance
              AckUnknown = false }

        let http = Microsoft.AspNetCore.Http.DefaultHttpContext()
        http.Request.Method <- "POST"
        http.Request.Host <- Microsoft.AspNetCore.Http.HostString "example.com"
        http.Request.Path <- Microsoft.AspNetCore.Http.PathString path
        http.Request.Headers["Dd-Api-Key"] <- Microsoft.Extensions.Primitives.StringValues Golden.Replay.testKey
        let body = System.Text.Encoding.UTF8.GetBytes(template.Replace("DEEP", deep))
        let response = Replay.byGoNames deps [ routeSet ] http body
        response.Status, sink.Rows<NinjaCat.Api.Storage.Rows.RawPayloadRow>() |> List.map _.Reason

    // Past the limit the body does not parse at all: kept whole, answered as usual.
    let status, reasons = send (Json.maxDepth + 100)
    Assert.True(status = 200 || status = 202, $"status {status}")
    Assert.Equal<string list>([ "decode_error" ], reasons)

    // Inside it: parsed, and writing it back into a column must not throw.
    let status, reasons = send (Json.maxDepth - 100)
    Assert.True(status = 200 || status = 202, $"status {status}")
    Assert.DoesNotContain("decode_error", reasons)

[<Fact>]
let ``a timestamp beyond year 9999 is kept at the limit, not an error`` () =
    Assert.Equal(9999, (Time.fromUnixSeconds 999_999_999_999_999L).Year)
    Assert.Equal(9999, (Time.fromUnixMillis System.Int64.MaxValue).Year)
    Assert.Equal(1, (Time.fromUnixSeconds System.Int64.MinValue).Year)
    Assert.Equal(System.DateTime(2026, 9, 23, 16, 10, 38, System.DateTimeKind.Utc), Time.fromUnixSeconds 1790179838L)

[<Fact>]
let ``a handler that throws is a 500, not a crash`` () =
    Assert.Equal(500, statusOf Auth.none [ GET [ route "/x" (Routes.keyed Auth.none (fun _ -> failwith "boom")) ] ] "GET" "/x" [])

// --- the host dispatch -------------------------------------------------------------------

[<Theory>]
[<InlineData("nobody.ninjacat.local")>]
[<InlineData("ninjacat.local")>]
// A dash instead of a dot: not our prefix.
[<InlineData("sbom-intake-ninjacat.local")>]
let ``a host no intake serves is refused by name`` (host: string) =
    let intake = Replay.wholeIntake (deps (CapturingSink()))
    let response = intake (request "POST" host "/api/v2/sbom" [ "Dd-Api-Key", key ]) [||]
    Assert.Equal(404, response.Status)
    Assert.Equal("{\"errors\":[\"no intake for host \\\"" + host + "\\\"\"]}\n", Encoding.UTF8.GetString response.Body)
    Assert.Equal<(string * string) list>([ "Content-Type", "application/json" ], response.Headers)

[<Fact>]
let ``an unknown host is refused even for the probes, port or not`` () =
    let intake = Replay.wholeIntake (deps (CapturingSink()))
    let response = intake (request "GET" "nobody.ninjacat.local:8443" "/ping" []) [||]
    Assert.Equal(404, response.Status)
    Assert.Contains("nobody.ninjacat.local", Encoding.UTF8.GetString response.Body)

// --- raw payloads ----------------------------------------------------------------------

let private storing (auth: Auth) (body: string) : Endpoint list =
    let store: Handler =
        fun r ->
            Raw.store r "dbm" "no_schema" "no decoder for this track yet" (Encoding.UTF8.GetBytes body)
            Response.status 202

    [ POST [ route "/api/v2/databasequery" (Routes.keyed auth store) ] ]

[<Fact>]
let ``a raw payload keeps what identifies the request, and no credentials`` () =
    let sink = CapturingSink()

    let http =
        request
            "POST"
            "dbm-metrics-intake.ninjacat.local"
            "/api/v2/databasequery?api-version=2"
            [ "Dd-Api-Key", key
              "Content-Type", "application/json"
              "Content-Encoding", "identity"
              "User-Agent", "datadog-agent/7.58.2"
              "Authorization", "Bearer secret-value" ]

    Assert.Equal(202, (send sink Auth.standard (storing Auth.standard """{"x":1}""") http).Status)
    let row = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("t1", row.TenantID)
    Assert.Equal("dbm", row.Intake)
    Assert.Equal("no_schema", row.Reason)
    // The Host header is what the intake dispatches on; it must be kept.
    Assert.Equal("dbm-metrics-intake.ninjacat.local", row.Host)
    Assert.Equal("/api/v2/databasequery", row.Path)
    Assert.Equal<string[]>([| "2" |], row.Query["api-version"])
    Assert.Equal("identity", row.ContentEncoding)
    Assert.Equal("""{"x":1}""", Encoding.UTF8.GetString row.Body)
    Assert.Equal(7UL, row.BodyBytes)
    Assert.Equal("datadog-agent/7.58.2", row.Headers["User-Agent"])
    Assert.False(row.Headers.ContainsKey "Authorization")
    Assert.False(row.Headers.ContainsKey "Dd-Api-Key")

[<Fact>]
let ``nothing is stored without a tenant`` () =
    let sink = CapturingSink()
    send sink Auth.none (storing Auth.none "body") (request "POST" "example.com" "/api/v2/databasequery" []) |> ignore
    Assert.Empty sink.Writes

[<Theory>]
[<InlineData("")>]
[<InlineData("{}")>]
[<InlineData("[]")>]
[<InlineData(" {} \n")>]
let ``the agent's empty probes are not stored`` (body: string) =
    let sink = CapturingSink()
    send sink Auth.standard (storing Auth.standard body) (request "POST" "example.com" "/api/v2/databasequery" [ "Dd-Api-Key", key ])
    |> ignore
    Assert.Empty sink.Writes
    Assert.False(Raw.isProbe (Encoding.UTF8.GetBytes """{"a":1}"""))
    Assert.False(Raw.isProbe (Encoding.UTF8.GetBytes "x"))

// --- tags ------------------------------------------------------------------------------

[<Fact>]
let ``tags are a multiset: a repeated key keeps every value, in order`` () =
    Assert.Equal<Map<string, string[]>>(
        Map [ "kube_service", [| "a"; "b" |]; "env", [| "prod" |] ],
        Tags.toMultiMap [ "kube_service:a"; "env:prod"; "kube_service:b" ]
    )

    Assert.Equal<Map<string, string[]>>(Map [ "standalone", [| "" |] ], Tags.toMultiMap [ "standalone" ])
    Assert.Equal<Map<string, string[]>>(Map [ "url", [| "http://x" |] ], Tags.toMultiMap [ "url:http://x" ])
    Assert.Equal<Map<string, string[]>>(Map.empty, Tags.toMultiMap [])

[<Fact>]
let ``labels are pairs split at the first colon`` () =
    Assert.Equal<Map<string, string>>(Map [ "app", "web"; "note", "a:b" ], Tags.toMap [ "app:web"; "note:a:b" ])
    Assert.Equal<Map<string, string>>(Map.empty, Tags.toMap [])

[<Fact>]
let ``RFC 3339 is read as Go reads it`` () =
    let utc (y: int) (mo: int) (d: int) (h: int) (mi: int) (s: int) = System.DateTime(y, mo, d, h, mi, s, System.DateTimeKind.Utc)
    Assert.Equal(Some(utc 2026 9 21 9 30 0), Time.tryRfc3339 "2026-09-21T09:30:00Z")
    Assert.Equal(Some(utc 2026 9 21 7 30 0), Time.tryRfc3339 "2026-09-21T09:30:00+02:00")
    Assert.Equal(Some((utc 2026 9 21 9 30 0).AddMilliseconds 500.0), Time.tryRfc3339 "2026-09-21T09:30:00.5Z")
    Assert.Equal(Some((utc 2026 9 21 9 30 0).AddMilliseconds 500.0), Time.tryRfc3339 "2026-09-21T09:30:00,5Z")
    // Past 100 ns the digits are dropped: this stays inside its second.
    Assert.Equal(Some((utc 2026 9 21 9 30 0).AddTicks 9999999L), Time.tryRfc3339 "2026-09-21T09:30:00.99999996Z")
    Assert.Equal(Some(utc 2026 9 20 19 0 0), Time.tryRfc3339 "2026-09-21T09:30:00+14:30")
    Assert.Equal(9999, (Time.tryRfc3339 "9999-12-31T23:59:59.999999999Z").Value.Year)
    Assert.Equal(9999, (Time.tryRfc3339 "9999-12-31T23:59:59-10:00").Value.Year)

    for bad in [ ""; "2026-09-21"; "2026-09-21 09:30:00Z"; "2026-09-21T09:30:00"; "2026-09-21T09:30Z"; "2026-02-30T00:00:00Z"; "2026-09-21T24:00:00Z"; "2026-09-21t09:30:00z" ] do
        Assert.Equal(None, Time.tryRfc3339 bad)

[<Fact>]
let ``a body of one object, an array, or null`` () =
    let count (body: string) =
        match Json.tryParseList (Encoding.UTF8.GetBytes body) with
        | Ok items -> items.Length
        | Error _ -> -1

    Assert.Equal(1, count """{"a":1}""")
    Assert.Equal(2, count """[{"a":1},{"a":2}]""")
    Assert.Equal(0, count "null")
    Assert.Equal(0, count "[]")
    Assert.Equal(-1, count "not json")
