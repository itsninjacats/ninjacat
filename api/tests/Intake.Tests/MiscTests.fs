/// Tests of Routers/Misc.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.MiscTests

open System
open System.Text
open System.Text.Json
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

let private json (text: string) : JsonElement =
    use doc = JsonDocument.Parse text
    doc.RootElement.Clone()

let private value (text: string) : JsonElement option = Some(json text)

/// Posts a body through the engine with the test key; returns the answer and
/// what was written.
let private post (routeSet: string) (path: string) (headers: (string * string) list) (body: string) : Response * CapturingSink =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let http = DefaultHttpContext()
    http.Request.Method <- "POST"
    http.Request.Host <- HostString "example.com"
    http.Request.Path <- PathString path
    http.Request.Headers["Dd-Api-Key"] <- StringValues Replay.testKey

    for name, header in headers do
        http.Request.Headers[name] <- StringValues header

    Replay.byGoNames deps [ routeSet ] http (Encoding.UTF8.GetBytes body), sink

let private telemetry (body: string) : APMTelemetryRow list =
    let response, sink = post "routeTelemetry" "/api/v2/apmtelemetry" [] body
    Assert.Equal(202, response.Status)
    sink.Rows<APMTelemetryRow>()

[<Fact>]
let ``seq_id keeps every digit past 2^53`` () =
    Assert.Equal(Some 9007199254740993L, Misc.seqId (value "9007199254740993"))
    // Absent, not zero.
    Assert.Equal(None, Misc.seqId None)
    Assert.Equal(None, Misc.seqId (value "\"not-a-number\""))

[<Fact>]
let ``seq_id is an integer or nothing: a fraction, a bool and null are nothing, a number in a string is taken`` () =
    Assert.Equal(None, Misc.seqId (value "1.0"))
    Assert.Equal(None, Misc.seqId (value "true"))
    Assert.Equal(None, Misc.seqId (value "null"))
    Assert.Equal(None, Misc.seqId (value "9223372036854775808"))
    Assert.Equal(Some 123L, Misc.seqId (value "\"123\""))

[<Fact>]
let ``a telemetry time is Unix seconds with a fraction, and nothing when it does not parse`` () =
    Assert.Equal(Some((Time.fromUnixSeconds 1732000000L).AddMilliseconds 500.0), Misc.unixTime (value "1732000000.5"))
    Assert.Equal(Some(Time.fromUnixSeconds 1732000000L), Misc.unixTime (value "\"1732000000\""))
    // Absent or garbage, not the epoch.
    Assert.Equal(None, Misc.unixTime None)
    Assert.Equal(None, Misc.unixTime (value "\"not a time\""))
    Assert.Equal(None, Misc.unixTime (value "null"))

[<Fact>]
let ``a telemetry time takes what Go's ParseFloat takes, and no surrounding space`` () =
    Assert.Equal(Some(Time.fromUnixSeconds 5L), Misc.unixTime (value "\"+5\""))
    Assert.Equal(Some(Time.fromUnixSeconds 5L), Misc.unixTime (value "\"5.\""))
    Assert.Equal(Some(Time.fromUnixSeconds 1000L), Misc.unixTime (value "1e3"))
    Assert.Equal(Some((Time.fromUnixSeconds 0L).AddMilliseconds -500.0), Misc.unixTime (value "-0.5"))
    Assert.Equal(None, Misc.unixTime (value "\" 5\""))
    Assert.Equal(None, Misc.unixTime (value "[5]"))

[<Fact>]
let ``a time no DateTime can hold becomes the nearest one, not an exception`` () =
    Assert.Equal(Some(DateTime(9999, 12, 31, 23, 59, 59, DateTimeKind.Utc)), Misc.unixTime (value "1e300"))

[<Fact>]
let ``the raw text of a value is kept whether or not it parses`` () =
    Assert.Equal("\"not a time\"", Misc.rawText (value "\"not a time\""))
    Assert.Equal("", Misc.rawText None)

[<Fact>]
let ``a column's text is the string itself, empty for absent and null, JSON text for anything else`` () =
    Assert.Equal("v2", Misc.text (value "\"v2\""))
    Assert.Equal("", Misc.text None)
    Assert.Equal("", Misc.text (value "null"))
    Assert.Equal("2", Misc.text (value "2"))
    Assert.Equal("""{"a": 1}""", Misc.text (value """{"a": 1}"""))

[<Fact>]
let ``an absent application or host block is empty, not malformed`` () =
    let app = Misc.application None
    Assert.True app.IsSome
    Assert.Equal("", app.Value.ServiceName)
    Assert.Equal("", app.Value.TracerVersion)

    let host = Misc.host None
    Assert.True host.IsSome
    Assert.Equal("", host.Value.Hostname)
    Assert.True host.Value.Extra.IsEmpty

[<Fact>]
let ``an application or host block that is not an object is malformed`` () =
    Assert.Equal(None, Misc.application (value "\"not-an-object\""))
    Assert.Equal(None, Misc.host (value "42"))

[<Fact>]
let ``the host block's undeclared keys are kept by name`` () =
    let host =
        Misc.host (
            value
                """{"hostname": "h1", "os": "linux", "architecture": "amd64",
                    "kernel_version": "5.15.0", "cpu_cores": 8}"""
        )

    Assert.Equal("h1", host.Value.Hostname)
    Assert.Equal("linux", host.Value.OS)
    Assert.Equal("amd64", host.Value.Architecture)
    Assert.Equal<Map<string, string>>(Map [ "cpu_cores", "8"; "kernel_version", "\"5.15.0\"" ], host.Value.Extra)

[<Fact>]
let ``the envelope's undeclared keys are kept by name`` () =
    let envelope =
        Map [ "request_type", json "\"app-started\""; "api_version", json "\"v2\""; "future_field", json "\"surprise\"" ]

    Assert.Equal<Map<string, string>>(Map [ "future_field", "\"surprise\"" ], Misc.extraKeys envelope)

[<Theory>]
[<InlineData("""{"request_type":"apm-onboarding-event"}""", "trace-agent")>]
[<InlineData("""{"request_type":"apm-remote-config-event"}""", "cluster-agent")>]
[<InlineData("""{"request_type":"agent-logs"}""", "agent-telemetry")>]
[<InlineData("""{"request_type":"app-heartbeat"}""", "tracer")>]
[<InlineData("""{"request_type":"agent-bsod","event_time":1}""", "agent-telemetry")>]
[<InlineData("""{"request_type":"logs","origin":"installer","runtime_id":"r"}""", "fleet-installer")>]
[<InlineData("""{"request_type":"logs","runtime_id":"r"}""", "tracer")>]
[<InlineData("""{"request_type":"logs"}""", "unknown")>]
let ``the producer is named from request_type, then from the keys only one producer sets`` (body: string, producer: string) =
    Assert.Equal(producer, (telemetry body |> List.exactlyOne).Producer)

[<Fact>]
let ``the proxy's headers are stored, and a malformed application block lands in extra`` () =
    let _, sink =
        post
            "routeTelemetry"
            "/api/v2/apmtelemetry"
            [ "Via", "trace-agent 7.60.0"
              "DD-Agent-Hostname", "h1"
              "DD-Agent-Env", "prod"
              "Datadog-Container-ID", "c1"
              "X-Datadog-Container-Tags", "a:b" ]
            """{"request_type":"app-started","application":[1,2],"future":{"x":1}}"""

    let row = Assert.Single(sink.Rows<APMTelemetryRow>())
    Assert.Equal("trace-agent 7.60.0", row.Via)
    Assert.Equal("h1", row.DDAgentHostname)
    Assert.Equal("prod", row.DDAgentEnv)
    Assert.Equal("c1", row.DatadogContainerID)
    Assert.Equal("a:b", row.XDatadogContainerTags)
    Assert.Equal<Map<string, string>>(Map [ "application", "[1,2]"; "future", """{"x":1}""" ], row.Extra)

[<Fact>]
let ``a batch that is not a list of objects is kept only on the request's row`` () =
    let notAList = telemetry """{"request_type":"message-batch","payload":{"request_type":"logs"}}"""
    Assert.Equal("""{"request_type":"logs"}""", (Assert.Single notAList).Payload)

    let notObjects = telemetry """{"request_type":"message-batch","payload":[{"request_type":"logs"},7]}"""
    Assert.Equal(None, (Assert.Single notObjects).BatchIndex)

    let absent = telemetry """{"request_type":"message-batch"}"""
    Assert.Equal("", (Assert.Single absent).Payload)

[<Fact>]
let ``a null batch entry is still an entry, with nothing in it`` () =
    let rows = telemetry """{"request_type":"message-batch","runtime_id":"r","payload":[null,{"request_type":"logs","payload":1}]}"""
    Assert.Equal<uint32 option list>([ None; Some 0u; Some 1u ], rows |> List.map _.BatchIndex)
    Assert.Equal<string list>([ "message-batch"; ""; "logs" ], rows |> List.map _.RequestType)
    Assert.Equal<string list>([ "[null,{\"request_type\":\"logs\",\"payload\":1}]"; ""; "1" ], rows |> List.map _.Payload)
    Assert.Equal<string list>([ ""; "message-batch"; "message-batch" ], rows |> List.map _.ParentRequestType)

[<Fact>]
let ``a null body is an empty envelope and still a row, as it was for Go`` () =
    let row = telemetry "null" |> List.exactlyOne
    Assert.Equal("", row.RequestType)
    Assert.Equal("unknown", row.Producer)

[<Fact>]
let ``a body that is JSON but not an object goes to raw_payloads`` () =
    let _, sink = post "routeTelemetry" "/api/v2/apmtelemetry" [] "[1,2,3]"
    Assert.Empty(sink.Rows<APMTelemetryRow>())
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("apmtelemetry", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)

[<Fact>]
let ``genresources quotes the origin headers in the note as Go's %q does`` () =
    let _, sink =
        post
            "routeResources"
            "/api/v2/genresources"
            [ "DD-EVP-ORIGIN", "a\"b\\c"; "Content-Type", "application/x-protobuf" ]
            "opaque"

    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("no_schema", raw.Reason)

    Assert.Equal(
        "DD-EVP-ORIGIN=\"a\\\"b\\\\c\" DD-EVP-ORIGIN-VERSION=\"\" Content-Type=\"application/x-protobuf\"",
        raw.Note
    )
