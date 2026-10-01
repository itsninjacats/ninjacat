/// The parts every router stands on: the key guard, route matching, the host
/// dispatch, raw payloads, tags.
module NinjaCat.Api.Intake.Tests.IntakeCoreTests

open System.Text
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
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

let private statusOf (auth: Auth) (routes: Route list) (method: string) (target: string) (headers: (string * string) list) : int =
    let engine = Engine.create auth [ routes ]
    (Engine.respond (deps (CapturingSink())) engine (request method "example.com" target headers) [||]).Status

// --- the key guard -------------------------------------------------------------------

[<Fact>]
let ``the standard guard takes the header, then the api_key query parameter`` () =
    let status target headers = statusOf Auth.standard [ Route.get "/x" ok ] "GET" target headers
    Assert.Equal(200, status "/x" [ "Dd-Api-Key", key ])
    Assert.Equal(200, status ("/x?api_key=" + key) [])
    Assert.Equal(403, status "/x?api_key=wrong" [])
    Assert.Equal(403, status "/x" [])
    // Bearer is for the one intake whose clients send it.
    Assert.Equal(403, status "/x" [ "Authorization", "Bearer " + key ])

[<Fact>]
let ``key sources are tried in order, and a non-bearer credential is not a key`` () =
    let all = Auth.fromSources [ KeyFromHeader "Dd-Api-Key"; KeyFromBearer; KeyFromQuery "api_key" ]
    let status target headers = statusOf all [ Route.get "/x" ok ] "GET" target headers
    Assert.Equal(200, status "/x" [ "Dd-Api-Key", key ])
    Assert.Equal(200, status "/x" [ "DD-API-KEY", key ])
    Assert.Equal(200, status "/x" [ "Authorization", "Bearer " + key ])
    Assert.Equal(200, status "/x" [ "Authorization", "bearer " + key ])
    Assert.Equal(403, status "/x" [ "Authorization", "Basic " + key ])
    Assert.Equal(200, status ("/x?api_key=" + key) [])
    Assert.Equal(403, status "/x" [])

[<Fact>]
let ``the probes need no key, an unknown path is answered before the guard`` () =
    let status method target = statusOf Auth.standard [ Route.post "/x" ok ] method target []
    Assert.Equal(200, status "GET" "/ping")
    Assert.Equal(200, status "GET" "/_health")
    Assert.Equal(404, status "POST" "/nowhere")
    // A route under another method is not a route.
    Assert.Equal(404, status "GET" "/x")
    Assert.Equal(403, status "POST" "/x")

// --- route matching --------------------------------------------------------------------

[<Fact>]
let ``routes match literals, parameters and a trailing wildcard`` () =
    let seen = ResizeArray<string>()
    let record (name: string) : Handler =
        fun r ->
            let id = r.Param "id"
            let rest = r.Param "rest"
            seen.Add $"{name} id={id} rest={rest}"
            Response.status 200

    let routes =
        [ Route.get "/v2/items/:id" (record "item")
          Route.get "/v2/items/all" (record "all")
          Route.get "/files/*rest" (record "files")
          Route.post "/api/v2/webhook" (record "plain")
          Route.post "/api/v2/webhook/" (record "slash") ]

    let call method target = statusOf Auth.none routes method target [] |> ignore
    call "GET" "/v2/items/42"
    call "GET" "/v2/items/all"
    call "GET" "/files/a/b.txt"
    call "POST" "/api/v2/webhook"
    call "POST" "/api/v2/webhook/"

    Assert.Equal<string list>(
        [ "item id=42 rest="
          // A literal beats a parameter.
          "all id= rest="
          // As in Gin, the wildcard's value keeps its leading slash.
          "files id= rest=/a/b.txt"
          "plain id= rest="
          "slash id= rest=" ],
        List.ofSeq seen
    )

    Assert.Equal(404, statusOf Auth.none routes "GET" "/v2/items/1/2" [])

[<Fact>]
let ``a path one trailing slash away from a route is redirected to it`` () =
    let engine = Engine.create Auth.standard [ [ Route.get "/v2/" ok; Route.post "/upload" ok ] ]

    let respond method target =
        Engine.respond (deps (CapturingSink())) engine (request method "example.com" target []) [||]

    let toSlash = respond "GET" "/v2?x=1"
    Assert.Equal(301, toSlash.Status)
    Assert.Equal<(string * string) list>([ "Location", "/v2/?x=1" ], toSlash.Headers)

    // Not GET: 307, so the client repeats the method and the body.
    let fromSlash = respond "POST" "/upload/"
    Assert.Equal(307, fromSlash.Status)
    Assert.Equal<(string * string) list>([ "Location", "/upload" ], fromSlash.Headers)

    Assert.Equal(404, (respond "GET" "/v3").Status)

[<Fact>]
let ``a timestamp beyond year 9999 is kept at the limit, not an error`` () =
    Assert.Equal(9999, (Time.fromUnixSeconds 999_999_999_999_999L).Year)
    Assert.Equal(9999, (Time.fromUnixMillis System.Int64.MaxValue).Year)
    Assert.Equal(1, (Time.fromUnixSeconds System.Int64.MinValue).Year)
    Assert.Equal(System.DateTime(2026, 9, 23, 16, 10, 38, System.DateTimeKind.Utc), Time.fromUnixSeconds 1790179838L)

[<Fact>]
let ``a handler that throws is a 500, not a crash`` () =
    Assert.Equal(500, statusOf Auth.none [ Route.get "/x" (fun _ -> failwith "boom") ] "GET" "/x" [])

// --- the host dispatch -------------------------------------------------------------------

[<Theory>]
[<InlineData("nobody.ninjacat.local")>]
[<InlineData("ninjacat.local")>]
// A dash instead of a dot: not our prefix.
[<InlineData("sbom-intake-ninjacat.local")>]
let ``a host no intake serves is refused by name`` (host: string) =
    let intake = Routes.create (deps (CapturingSink()))
    let response = intake (request "POST" host "/api/v2/sbom" [ "Dd-Api-Key", key ]) [||]
    Assert.Equal(404, response.Status)
    Assert.Equal("{\"errors\":[\"no intake for host \\\"" + host + "\\\"\"]}\n", Encoding.UTF8.GetString response.Body)
    Assert.Equal<(string * string) list>([ "Content-Type", "application/json" ], response.Headers)

[<Fact>]
let ``an unknown host is refused even for the probes, port or not`` () =
    let intake = Routes.create (deps (CapturingSink()))
    let response = intake (request "GET" "nobody.ninjacat.local:8443" "/ping" []) [||]
    Assert.Equal(404, response.Status)
    Assert.Contains("nobody.ninjacat.local", Encoding.UTF8.GetString response.Body)

// --- raw payloads ----------------------------------------------------------------------

let private storing (body: string) : Route list =
    [ Route.post "/api/v2/databasequery" (fun r ->
          Raw.store r "dbm" "no_schema" "no decoder for this track yet" (Encoding.UTF8.GetBytes body)
          Response.status 202) ]

[<Fact>]
let ``a raw payload keeps what identifies the request, and no credentials`` () =
    let sink = CapturingSink()
    let engine = Engine.create Auth.standard [ storing """{"x":1}""" ]

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

    Assert.Equal(202, (Engine.respond (deps sink) engine http [||]).Status)
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
    let engine = Engine.create Auth.none [ storing "body" ]
    Engine.respond (deps sink) engine (request "POST" "example.com" "/api/v2/databasequery" []) [||] |> ignore
    Assert.Empty sink.Writes

[<Theory>]
[<InlineData("")>]
[<InlineData("{}")>]
[<InlineData("[]")>]
[<InlineData(" {} \n")>]
let ``the agent's empty probes are not stored`` (body: string) =
    let sink = CapturingSink()
    let engine = Engine.create Auth.standard [ storing body ]
    Engine.respond (deps sink) engine (request "POST" "example.com" "/api/v2/databasequery" [ "Dd-Api-Key", key ]) [||] |> ignore
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
