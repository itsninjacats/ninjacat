/// The parts every router stands on: the key guard, the host dispatch, raw
/// payloads, tags. Matching a path is the framework's router's job and is not
/// tested here.
module NinjaCat.Api.Intake.Tests.IntakeCoreTests

open System.Text
open System.Text.Json
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

let private request = Requests.httpContext

let private ok (_: byte[]) : EndpointHandler = setStatusCode 200

/// The scheme of the agent's hosts, and the one of the host that also takes
/// a Bearer credential.
let private agent = Some ApiKeyAuth.agent
let private lineage = Some ApiKeyAuth.lineage

/// One host's router with the given endpoints, as the server mounts it.
/// `scheme` is the authentication its routes ask for; None for none.
let private send (sink: CapturingSink) (scheme: string option) (endpoints: Endpoint list) (http: HttpContext) : Response =
    Replay.through (deps sink) (fun app -> Routes.mount app scheme endpoints) http [||]

let private statusOf (scheme: string option) (endpoints: Endpoint list) (method: string) (target: string) (headers: (string * string) list) : int =
    (send (CapturingSink()) scheme endpoints (request method "example.com" target headers)).Status

// --- the key guard -------------------------------------------------------------------

[<Fact>]
let ``the standard guard takes the header, then the api_key query parameter`` () =
    let status target headers =
        statusOf agent [ GET [ route "/x" (bindBody ok) ] ] "GET" target headers

    Assert.Equal(200, status "/x" [ "Dd-Api-Key", key ])
    Assert.Equal(200, status ("/x?api_key=" + key) [])
    Assert.Equal(403, status "/x?api_key=wrong" [])
    Assert.Equal(403, status "/x" [])
    // Bearer is for the one intake whose clients send it.
    Assert.Equal(403, status "/x" [ "Authorization", "Bearer " + key ])

[<Fact>]
let ``key sources are tried in order, and a non-bearer credential is not a key`` () =
    let status target headers = statusOf lineage [ GET [ route "/x" (bindBody ok) ] ] "GET" target headers
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
        statusOf agent [ POST [ route "/x" (bindBody ok) ] ] method target []

    Assert.Equal(200, status "GET" "/ping")
    Assert.Equal(200, status "GET" "/_health")
    // The agent's connectivity check sends this one without a key.
    Assert.Equal(200, status "HEAD" "/support/flare")
    Assert.Equal(200, status "HEAD" "/support/flare/1234")
    Assert.Equal(403, status "POST" "/support/flare")
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
        let body = Requests.utf8 (template.Replace("DEEP", deep))
        let response, sink = Requests.send [ routeSet ] "POST" path Requests.withKey body
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
let ``a protobuf Timestamp is a time to 100 ns, or None when absent or invalid`` () =
    let stamp (seconds: int64) (nanos: int) = Google.Protobuf.WellKnownTypes.Timestamp(Seconds = seconds, Nanos = nanos)
    let whole = System.DateTime(2026, 9, 23, 16, 10, 38, System.DateTimeKind.Utc)

    Assert.Equal(Some(whole.AddTicks 1234567L), Time.ofTimestamp (stamp 1790179838L 123_456_789))
    Assert.Equal(Some(System.DateTime.UnixEpoch.AddSeconds -1.0), Time.ofTimestamp (stamp -1L 0))
    Assert.Equal(None, Time.ofTimestamp null)
    Assert.Equal(None, Time.ofTimestamp (stamp 1790179838L -1))
    Assert.Equal(None, Time.ofTimestamp (stamp 1790179838L 1_000_000_000))
    Assert.Equal(None, Time.ofTimestamp (stamp 253402300800L 0))
    Assert.Equal(None, Time.ofTimestamp (stamp System.Int64.MaxValue 0))

[<Fact>]
let ``a protobuf enum value is its name in the schema, its number when the schema has none`` () =
    Assert.Equal("GAUGE", ProtoEnum.name Datadog.Agentpayload.MetricPayload.Types.MetricType.Gauge)
    Assert.Equal("ISSUE_SEVERITY_HIGH", ProtoEnum.name Datadog.Healthplatform.IssueSeverity.High)
    Assert.Equal("77", ProtoEnum.name (enum<Datadog.Agentpayload.MetricPayload.Types.MetricType> 77))

[<Fact>]
let ``a handler that throws is a 500, not a crash`` () =
    Assert.Equal(500, statusOf None [ GET [ route "/x" (bindBody (fun _ _ -> failwith "boom")) ] ] "GET" "/x" [])

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
    let errors = JsonDocument.Parse(response.Body).RootElement.GetProperty "errors"
    Assert.Equal("no intake for host \"" + host + "\"", (Assert.Single(errors.EnumerateArray())).GetString())
    Assert.Equal<(string * string) list>([ "Content-Type", "application/json; charset=utf-8" ], response.Headers)

[<Fact>]
let ``an unknown host is refused even for the probes, port or not`` () =
    let intake = Replay.wholeIntake (deps (CapturingSink()))
    let response = intake (request "GET" "nobody.ninjacat.local:8443" "/ping" []) [||]
    Assert.Equal(404, response.Status)
    Assert.Contains("nobody.ninjacat.local", Encoding.UTF8.GetString response.Body)

// --- raw payloads ----------------------------------------------------------------------

let private storing (body: string) : Endpoint list =
    let store (_: byte[]) : EndpointHandler =
        fun ctx ->
            Raw.store ctx "dbm" "no_schema" "no decoder for this track yet" (Encoding.UTF8.GetBytes body)
            setStatusCode 202 ctx

    [ POST [ route "/api/v2/databasequery" (bindBody store) ] ]

[<Fact>]
let ``a body that inflates past the limit comes back as it was sent`` () =
    let gzip (data: byte[]) =
        use packed = new System.IO.MemoryStream()

        do
            use writer = new System.IO.Compression.GZipStream(packed, System.IO.Compression.CompressionLevel.Fastest)
            writer.Write(data, 0, data.Length)

        packed.ToArray()

    let small = gzip (Encoding.UTF8.GetBytes "hello")
    Assert.Equal("hello", Encoding.UTF8.GetString(Body.decompress NullLogger.Instance "gzip" small))

    // A few kilobytes that would become more memory than the limit allows.
    let bomb = gzip (Array.zeroCreate<byte> (Body.maxInflatedBytes + 1))
    Assert.True(bomb.Length < 1024 * 1024)
    Assert.Equal<byte[]>(bomb, Body.decompress NullLogger.Instance "gzip" bomb)

[<Fact>]
let ``a raw payload keeps what identifies the request, and no credentials`` () =
    let sink = CapturingSink()

    let http =
        request
            "POST"
            "dbm-metrics-intake.ninjacat.local"
            "/api/v2/databasequery?api-version=2&api_key=in-the-query&DD-API-KEY=in-the-query"
            [ "Dd-Api-Key", key
              "Content-Type", "application/json"
              "Content-Encoding", "identity"
              "User-Agent", "datadog-agent/7.58.2"
              "Authorization", "Bearer secret-value" ]

    Assert.Equal(202, (send sink agent (storing """{"x":1}""") http).Status)
    let row = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("t1", row.TenantID)
    Assert.Equal("dbm", row.Intake)
    Assert.Equal("no_schema", row.Reason)
    // The Host header is what the intake dispatches on; it must be kept.
    Assert.Equal("dbm-metrics-intake.ninjacat.local", row.Host)
    Assert.Equal("/api/v2/databasequery", row.Path)
    Assert.Equal<string[]>([| "2" |], row.Query["api-version"])
    // A key sent in the query string is not kept either.
    Assert.Equal<string list>([ "api-version" ], row.Query.Keys |> List.ofSeq)
    Assert.Equal("/v1/input/<key>", Secrets.pathWithoutKey "/v1/input/0123456789abcdef0123456789abcdef")
    Assert.Equal("identity", row.ContentEncoding)
    Assert.Equal("""{"x":1}""", Encoding.UTF8.GetString row.Body)
    Assert.Equal(7UL, row.BodyBytes)
    Assert.Equal("datadog-agent/7.58.2", row.Headers["User-Agent"])
    Assert.False(row.Headers.ContainsKey "Authorization")
    Assert.False(row.Headers.ContainsKey "Dd-Api-Key")

[<Fact>]
let ``nothing is stored without a tenant`` () =
    let sink = CapturingSink()
    send sink None (storing "body") (request "POST" "example.com" "/api/v2/databasequery" []) |> ignore
    Assert.Empty sink.Writes

[<Theory>]
[<InlineData("")>]
[<InlineData("{}")>]
[<InlineData("[]")>]
[<InlineData(" {} \n")>]
let ``the agent's empty probes are not stored`` (body: string) =
    let sink = CapturingSink()
    send sink agent (storing body) (request "POST" "example.com" "/api/v2/databasequery" [ "Dd-Api-Key", key ])
    |> ignore
    Assert.Empty sink.Writes
    Assert.False(Raw.isProbe (Encoding.UTF8.GetBytes """{"a":1}"""))
    Assert.False(Raw.isProbe (Encoding.UTF8.GetBytes "x"))

// --- reading JSON without notes ----------------------------------------------------------

let private element (text: string) : JsonElement option = Some(Requests.json text)

[<Fact>]
let ``a column's text is the string itself, a number digit for digit, JSON for the rest, empty for absent and null`` () =
    Assert.Equal("v2", Json.text (element "\"v2\""))
    Assert.Equal("", Json.text None)
    Assert.Equal("", Json.text (element "null"))
    Assert.Equal("9007199254740993", Json.text (element "9007199254740993"))
    Assert.Equal("true", Json.text (element "true"))
    Assert.Equal("""{"a":1,"b":[2]}""", Json.text (element """{"b": [2], "a": 1}"""))
    Assert.Equal<string[]>([| "a"; "2"; "" |], Json.textList (element """["a", 2, null]"""))
    Assert.Equal<string[]>([| "one" |], Json.textList (element "\"one\""))
    Assert.Equal<string[]>([||], Json.textList (element "\"\""))

[<Fact>]
let ``the raw text of a value is kept as it was sent, null included`` () =
    Assert.Equal("\"not a time\"", Json.rawOrEmpty (element "\"not a time\""))
    Assert.Equal("""{"b": 1}""", Json.rawOrEmpty (element """{"b": 1}"""))
    Assert.Equal("null", Json.rawOrEmpty (element "null"))
    Assert.Equal("", Json.rawOrEmpty None)

[<Fact>]
let ``members without a column are kept as JSON text, a null one as null`` () =
    let extra =
        Json.otherMembers
            (set [ "namespace"; "devices" ])
            _.GetRawText()
            (element """{"namespace":"default","devices":[],"future_field":{"nested":true},"count":3,"gone":null}""")

    Assert.Equal<Map<string, string>>(Map [ "count", "3"; "future_field", """{"nested":true}"""; "gone", "null" ], extra)
    Assert.True((Json.otherMembers Set.empty Json.compact None).IsEmpty)

[<Fact>]
let ``a path is walked through objects only, and ends at a value that is not null`` () =
    let root = (element """{"a": {"b": {"c": 1, "n": null}}, "s": "text"}""").Value
    Assert.Equal(Some "1", Json.at [ "a"; "b"; "c" ] root |> Option.map _.GetRawText())
    Assert.Equal(None, Json.at [ "a"; "b"; "n" ] root)
    Assert.Equal(None, Json.at [ "s"; "b" ] root)
    Assert.Equal(None, Json.at [ "a"; "x"; "c" ] root)
    Assert.Equal(Some 1uy, Json.flag (element "true"))
    Assert.Equal(None, Json.flag (element "1"))

[<Theory>]
[<InlineData("""["env:prod","team:a"]""", "env:prod|team:a")>]
[<InlineData("\"env:prod,team:a\"", "env:prod|team:a")>]
[<InlineData("\"\"", "")>]
[<InlineData("5", "")>]
[<InlineData("""["a", 5]""", "")>]
let ``a tag list is a list of strings or one comma-separated string`` (sent: string, expected: string) =
    Assert.Equal(expected, String.concat "|" (Json.tagList (element sent)))

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
let ``RFC 3339: what is read and what is not`` () =
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
    Assert.Equal(-1, count "5")

    // Where only a list will do, one bare object is refused: the agent's
    // `{}` probe must not become an empty row.
    Assert.Equal(Ok 2, Json.tryParseArray (Encoding.UTF8.GetBytes "[1, 2]") |> Result.map List.length)
    Assert.Equal(Ok 0, Json.tryParseArray (Encoding.UTF8.GetBytes "null") |> Result.map List.length)
    Assert.Equal(Error "expected a list, got object", Json.tryParseArray (Encoding.UTF8.GetBytes "{}") |> Result.map List.length)
