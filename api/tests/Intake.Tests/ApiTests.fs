/// Tests of Routers/Api.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.ApiTests

open System
open System.Text
open System.Text.Json
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Xunit
open Google.Protobuf
open Datadog.Agentpayload
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Intake.Routers.ApiPayloads
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Intake.Tests.Golden.Requests

/// Sends one request to the api.<site> routes with the test key; returns the
/// answer and the sink that took the writes.
let private send (method: string) (url: string) (headers: (string * string) list) (body: byte[]) : Response * CapturingSink =
    Requests.send [ "routeAPI" ] method url (Requests.withKey @ headers) body

let private post (path: string) (json: string) : Response * CapturingSink =
    send "POST" path [ "Content-Type", "application/json" ] (utf8 json)

let private text (response: Response) : string = Encoding.UTF8.GetString response.Body

/// The raw rows of a request, as (reason, note, body).
let private raws (sink: CapturingSink) : (string * string * string) list =
    sink.Rows<RawPayloadRow>() |> List.map (fun raw -> raw.Reason, raw.Note, Encoding.UTF8.GetString raw.Body)

let private parsed = Requests.json

// ---------------------------------------------------------------------------
// Reading JSON
// ---------------------------------------------------------------------------

[<Fact>]
let ``a member is found by its exact name, the last of two counts, null is absent`` () =
    let root = parsed """{"Metric":"a","metric":"b","metric":"c","host":null}"""
    Assert.Equal(Some "c", Json.field "metric" root |> Option.bind Json.stringOf)
    Assert.True((Json.field "METRIC" root).IsNone)
    Assert.True((Json.field "host" root).IsNone)
    Assert.Equal<string list>([ "Metric"; "host"; "metric" ], Json.members root |> Map.toList |> List.map fst)

[<Fact>]
let ``an integer written with a fraction or an exponent is not one`` () =
    let numbers = parsed """[5, 5.0, 5e0, -1, 12345678901234567890, "5"]"""
    let read = [ for n in numbers.EnumerateArray() -> Json.int64Of n ]
    Assert.Equal<int64 option list>([ Some 5L; None; None; Some -1L; None; None ], read)
    Assert.True((Json.uint64Of numbers[3]).IsNone)
    Assert.True((Json.int32Of numbers[1]).IsNone)

[<Fact>]
let ``a number beyond float64 does not fit a float`` () =
    let root = parsed """{"b":1e999,"c":1e308}"""
    Assert.True((Json.floatOf (root.GetProperty "b")).IsNone)
    Assert.Equal(Some 1e308, Json.floatOf (root.GetProperty "c"))

[<Fact>]
let ``re-encoding keeps the digits of a number and drops the whitespace`` () =
    let root = parsed """{ "id": 12345678901234567891, "n": 1.50, "list": [1, 2], "s": "zażółć <é>" }"""
    Assert.Equal("""{"id":12345678901234567891,"n":1.50,"list":[1,2],"s":"zażółć <é>"}""", Json.compact root)
    Assert.Equal("""[1,2]""", Json.compactArray (root.GetProperty("list").EnumerateArray()))
    Assert.Equal("{}", Json.compactObject Map.empty)

[<Fact>]
let ``the lenient readers keep what fits and leave the rest empty`` () =
    let root = parsed """{"lists":{"a":["x",5,null],"b":7,"c":null},"list":["a",5,{"x":1}],"notalist":"s"}"""

    Assert.Equal<(string * string[]) list>(
        [ "a", [| "x"; ""; "" |]; "b", [||]; "c", [||] ],
        Json.lenientStringLists (Some(root.GetProperty "lists")) |> Map.toList
    )

    Assert.Equal<string[] option>(Some [| "a"; ""; "" |], Json.lenientStrings (Some(root.GetProperty "list")))
    Assert.True((Json.lenientStrings (Some(root.GetProperty "notalist"))).IsNone)
    Assert.True((Json.lenientStrings None).IsNone)
    Assert.Equal("", Json.lenientString (Some(root.GetProperty "list")))

[<Fact>]
let ``half a surrogate pair and invalid UTF-8 are read as U+FFFD`` () =
    let lone = parsed """{"a":"x\ud800y","b":"\ud83d\ude00","c":"\\ud800","d":"\udc00"}"""
    Assert.Equal("x\uFFFDy", Json.lenientString (Some(lone.GetProperty "a")))
    Assert.Equal("😀", Json.lenientString (Some(lone.GetProperty "b")))
    Assert.Equal("\\ud800", Json.lenientString (Some(lone.GetProperty "c")))
    Assert.Equal("\uFFFD", Json.lenientString (Some(lone.GetProperty "d")))

    match Json.tryParse (Array.concat [ utf8 """{"host":"h"""; [| 0xFFuy |]; utf8 "\"}" ]) with
    | Ok root -> Assert.Equal("h\uFFFD", Json.lenientString (Some(root.GetProperty "host")))
    | Error e -> Assert.Fail e

[<Fact>]
let ``nesting deeper than System.Text.Json's default is still read`` () =
    let deep = String('[', 200) + String(']', 200)
    Assert.True((Json.tryParse (utf8 deep)).IsOk)

// ---------------------------------------------------------------------------
// The JSON bodies of the v1 API
// ---------------------------------------------------------------------------

[<Fact>]
let ``check runs without tags are decoded, and their neighbours with them`` () =
    let body =
        utf8
            """[
              {"check":"with.tags","host_name":"h1","timestamp":1790002960,"status":0,"message":"","tags":["a:1"]},
              {"check":"containerd.health","host_name":"h1","timestamp":1790002960,"status":0,"message":"","tags":null},
              {"check":"kubernetes.kubelet.check.ping","host_name":"h1","timestamp":1790002960,"status":1,"message":"slow"}
            ]"""

    match parseCheckRuns body with
    | Error problem -> Assert.Fail $"batch rejected: {problem}"
    | Ok checks ->
        Assert.Empty checks.Rejected

        Assert.Equal<string list>(
            [ "with.tags"; "containerd.health"; "kubernetes.kubelet.check.ping" ],
            checks.Runs |> List.map _.Check
        )

        Assert.Equal<string[]>([| "a:1" |], checks.Runs[0].Tags)
        Assert.Empty checks.Runs[1].Tags
        Assert.Equal(1, checks.Runs[2].Status)
        Assert.Equal("slow", checks.Runs[2].Message)

[<Fact>]
let ``check runs hand back the items that do not fit, each with its place, its bytes and why`` () =
    let body =
        utf8
            """[
              "containerd.health",
              null,
              {"check":"ok.check","host_name":"h1","status":0,"tags":["a:1"]},
              {"check":"b","host_name":"h","status":9,"id":12345678901234567891}
            ]"""

    match parseCheckRuns body with
    | Error problem -> Assert.Fail $"batch rejected: {problem}"
    | Ok checks ->
        Assert.Equal<string list>([ "ok.check" ], checks.Runs |> List.map _.Check)

        Assert.Equal<(int * string * string) list>(
            [ 0, "\"containerd.health\"", "[0]: expected an object, got string"
              3, """{"check":"b","host_name":"h","status":9,"id":12345678901234567891}""", "[3].status: expected 0 to 3, got 9" ],
            checks.Rejected |> List.map (fun (at, item, why) -> at, item.GetRawText(), why)
        )

    Assert.Equal(Ok { Runs = []; Rejected = [] }, parseCheckRuns (utf8 "[]"))
    Assert.True((parseCheckRuns (utf8 """[{"a":1}""")).IsError)

[<Fact>]
let ``one event that does not fit refuses the body, and the error says which and why`` () =
    let refused (body: string) = parseEvents (utf8 body) |> Result.map ignore
    Assert.Equal(Error "JSON events: [0].text: missing", refused """[{"title":"t"}]""")
    Assert.Equal(Error "JSON events: [1].title: missing", refused """[{"title":"t","text":"x"},{"text":"x"}]""")
    Assert.Equal(Error "JSON events: [1]: expected an object, got string", refused """[{"title":"t","text":"x"},"x"]""")
    Assert.Equal(Error "JSON events: date_happened: expected an integer, got string", refused """{"title":"t","text":"x","date_happened":"soon"}""")
    Assert.Equal(Error "JSON events: related_event_id: expected an integer, got number", refused """{"title":"t","text":"x","related_event_id":12345678901234567890}""")
    Assert.Equal(Error "JSON events: text: missing; title: missing", refused "{}")
    Assert.Equal(Ok [], parseEvents (utf8 "null"))

[<Fact>]
let ``only a series list gives series`` () =
    match parseSeriesV1 (utf8 """{"series":[{"metric":"a","points":[]},{"metric":"b","points":[]}]}""") with
    | Ok payload -> Assert.Equal<string[]>([| "a"; "b" |], payload.Series |> Array.map _.Metric)
    | Error e -> Assert.Fail e

    let refused (body: string) = parseSeriesV1 (utf8 body) |> Result.map ignore
    Assert.Equal(Error "JSON v1 series: series: expected a list, got object", refused """{"series":{"a":[]}}""")
    Assert.Equal(Error "JSON v1 series: series: missing", refused """{"other":[]}""")
    Assert.Equal(Error "JSON v1 series: series: missing", refused """{"Series":[]}""")
    Assert.Equal(Error "JSON v1 series: expected an object, got array", refused "[1,2]")

[<Fact>]
let ``log items arrive as an array, a bare object or one object per line`` () =
    let cases =
        [ """[{"message":"a"},{"message":"b"}]""", [ "a"; "b" ]
          """{"message":"a"}""", [ "a" ]
          "{\"message\":\"a\"}\n{\"message\":\"b\"}\n", [ "a"; "b" ]
          "{\"message\":\"a\"}\n{\"message\":\"b\"}", [ "a"; "b" ]
          "  \n\t[{\"message\":\"a\"}]", [ "a" ] ]

    for body, expected in cases do
        match Logs.parseItems (utf8 body) with
        | Ok items -> Assert.Equal<string list>(expected, items |> List.map (fun item -> item.GetProperty("message").GetString()))
        | Error e -> Assert.Fail $"rejected {body}: {e}"

[<Fact>]
let ``a broken line of logs is named by its number`` () =
    match Logs.parseItems (utf8 "{\"message\":\"a\"}\n{\"message\":\n") with
    | Error e -> Assert.Contains("item 1", e)
    | Ok _ -> Assert.Fail "expected an error"

[<Fact>]
let ``a v1 element that is not a series is set aside alone, a null is left out`` () =
    let good = """{"metric":"ok","points":[[1790151330,1]]}"""

    let cases =
        [ "\"x\"", "series[0]: expected an object, got string"
          """{"points":[]}""", "series[0].metric: missing"
          """{"metric":"m","points":null}""", "series[0].points: missing"
          """{"metric":"m","points":[[1,2]],"interval":1.5}""", "series[0].interval: expected an integer, got number"
          """{"metric":"m","points":"x"}""", "series[0].points: expected a list of [timestamp, value] pairs, got string"
          """{"metric":"m","points":[[1e999,2]]}""", "series[0].points: expected a list of [timestamp, value] pairs, got array" ]

    for bad, why in cases do
        match parseSeriesV1 (utf8 $"""{{"series":[{bad},null,{good}]}}""") with
        | Ok payload ->
            Assert.Equal<string[]>([| "ok" |], payload.Series |> Array.map _.Metric)
            // The good series stood third in the list as sent.
            Assert.Equal<int[]>([| 2 |], payload.Positions)
            Assert.Equal<(int * string * string) list>([ 0, bad, why ], payload.Rejected |> List.map (fun (at, item, why) -> at, item.GetRawText(), why))
        | Error e -> Assert.Fail $"{bad}: {e}"

[<Fact>]
let ``v1 series: one element that is not a series is kept raw, the others are stored`` () =
    let _, sink =
        post "/api/v1/series" """{"series":[{"metric":"a","points":[[1790151330,1]]},"x",{"metric":"b","points":[[1790151330,2]]}]}"""

    Assert.Equal<string list>([ "a"; "b" ], sink.Rows<MetricPoint>() |> List.map _.Metric)

    Assert.Equal<(string * string * string) list>(
        [ "unexpected_shape", "series[1]: expected an object, got string", "\"x\"" ],
        raws sink
    )

[<Fact>]
let ``a number beyond float64 among the members without a field is kept as written`` () =
    match parseSeriesV1 (utf8 """{"series":[{"metric":"m","points":[[1,2]],"big":1e999}]}""") with
    | Ok payload -> Assert.Equal("1e999", payload.Series[0].Additional["big"].GetRawText())
    | Error e -> Assert.Fail e

[<Fact>]
let ``a v1 point keeps its nulls, and a null pair is an empty one`` () =
    match decodeSeriesV1 "" (parsed """{"metric":"m","points":[[1,2],null,[null,3],[1,2,3]],"tags":[null,"a"],"interval":null}""") with
    | Ok series ->
        Assert.Equal<float option[][]>(
            [| [| Some 1.0; Some 2.0 |]; [||]; [| None; Some 3.0 |]; [| Some 1.0; Some 2.0; Some 3.0 |] |],
            series.Points |> Array.map _.Pair
        )

        Assert.Equal<string[]>([| ""; "a" |], series.Tags)
        Assert.True series.Interval.IsNone
    | Error e -> Assert.Fail e

[<Fact>]
let ``a check that does not fit says which member and why`` () =
    let refused (json: string) = decodeServiceCheck true "" (parsed json) |> Result.map ignore

    // An unreadable check must never be stored as OK.
    Assert.Equal(Error "status: expected 0 to 3, got 42", refused """{"check":"c","host_name":"h","status":42,"tags":[]}""")
    Assert.Equal(Error "status: expected a 32-bit integer, got string", refused """{"check":"c","host_name":"h","status":"x","tags":[]}""")
    Assert.Equal(Error "status: expected a 32-bit integer, got number", refused """{"check":"c","host_name":"h","status":1.0,"tags":[]}""")
    Assert.Equal(Error "status: missing", refused """{"check":"c","host_name":"h","status":null,"tags":[]}""")
    Assert.Equal(Error "host_name: missing", refused """{"check":"c","status":1,"tags":[]}""")
    Assert.Equal(Error "expected an object, got number", refused "5")

[<Fact>]
let ``a check in a batch may come without tags, a bare one may not`` () =
    let item = parsed """{"check":"c","host_name":"h","status":1}"""

    match decodeServiceCheck true "" item with
    | Ok check -> Assert.Empty check.Tags
    | Error e -> Assert.Fail e

    Assert.Equal(Error "tags: missing", decodeServiceCheck false "" item |> Result.map ignore)

[<Fact>]
let ``an event keeps the alert type and priority it was sent with, whatever they say`` () =
    match decodeEvent "" (parsed """{"title":"t","text":"x","alert_type":"Warning","priority":"urgent","host":"h"}""") with
    | Ok event -> Assert.Equal(("t", "Warning", Some "urgent", "h"), (event.Title, event.AlertType, event.Priority, event.Host))
    | Error e -> Assert.Fail e

[<Fact>]
let ``an event's tags are absent, or a list that may be empty`` () =
    match decodeEvent "" (parsed """{"title":"t","text":"x","priority":null,"tags":[]}"""), decodeEvent "" (parsed """{"title":"t","text":"x"}""") with
    | Ok withTags, Ok without ->
        Assert.Equal<string[] option>(Some [||], withTags.Tags)
        Assert.True withTags.Priority.IsNone
        Assert.True without.Tags.IsNone
    | other -> Assert.Fail $"{other}"

[<Fact>]
let ``each element of a distribution point is a timestamp, a value list, or neither`` () =
    let series =
        decodeDistributionSeries
            ""
            (parsed """{"metric":"m","points":[[1,[1,null,3]],[null,[1]],["x",[1]],[1,[]],[1,["a"]],null,[1,{"a":1}],[[1],[2],5,6],[1e999,[1e999]]]}""")

    match series with
    | Error e -> Assert.Fail e
    | Ok series ->
        Assert.Equal<DistributionItem[][]>(
            [| [| Timestamp 1.0; Values [| 1.0; 0.0; 3.0 |] |]
               [| Absent; Values [| 1.0 |] |]
               [| Unfit; Values [| 1.0 |] |]
               [| Timestamp 1.0; Values [||] |]
               [| Timestamp 1.0; Unfit |]
               [||]
               [| Timestamp 1.0; Unfit |]
               [| Values [| 1.0 |]; Values [| 2.0 |]; Timestamp 5.0; Timestamp 6.0 |]
               [| Unfit; Unfit |] |],
            series.Points |> Array.map _.Items
        )

[<Fact>]
let ``a distribution series keeps its type word and its points; other wrong types refuse it`` () =
    match decodeDistributionSeries "" (parsed """{"metric":"m","type":"histogram","points":[[1,[2]]]}""") with
    | Ok series -> Assert.Equal(("m", "histogram", 1), (series.Metric, series.Type, series.Points.Length))
    | Error e -> Assert.Fail e

    let refused (json: string) = decodeDistributionSeries "" (parsed json) |> Result.map ignore
    Assert.Equal(Error "type: expected a string, got number", refused """{"metric":"m","type":5,"points":[]}""")
    Assert.Equal(Error "points: expected a list of [timestamp, [values]] pairs, got array", refused """{"metric":"m","points":[5]}""")

[<Fact>]
let ``of a distribution point the last timestamp and the last values count`` () =
    match Api.distributionPoint [| Values [| 1.0 |]; Values [| 2.0 |]; Timestamp 5.0; Timestamp 1790151330.9 |] with
    | Some(time, values) ->
        Assert.Equal(DateTime(2026, 9, 23, 8, 15, 30, DateTimeKind.Utc), time)
        Assert.Equal<float[]>([| 2.0 |], values)
    | None -> Assert.Fail "expected a point"

    Assert.True((Api.distributionPoint [| Timestamp 1.0; Values [||] |]).IsNone)
    Assert.True((Api.distributionPoint [| Timestamp 1.0; Unfit |]).IsNone)

    // No timestamp is not 1970: the point is stamped on arrival.
    match Api.distributionPoint [| Absent; Values [| 1.0 |] |] with
    | Some(time, _) -> Assert.True(time.Year >= 2026)
    | None -> Assert.Fail "expected a point"

// ---------------------------------------------------------------------------
// router_api_test.go: the pure functions
// ---------------------------------------------------------------------------

[<Fact>]
let ``an intake event takes its host and source from the envelope unless it has its own`` () =
    let row (event: string) (source: string) (host: string) =
        Api.intakeEventRow Replay.testTenant source host (parsed event)

    let fromEnvelope = row """{"msg_title":"t"}""" "System" "agent-host"
    Assert.Equal(("agent-host", "System", "info", "normal", ""), (fromEnvelope.Host, fromEnvelope.SourceTypeName, fromEnvelope.AlertType, fromEnvelope.Priority, fromEnvelope.AlertTypeRaw))
    // No timestamp on the wire means arrival time, never 1970.
    Assert.True(fromEnvelope.Timestamp.Year >= 2026)
    Assert.NotEqual(0UL, fromEnvelope.EventIDNum)

    let own = row """{"msg_title":"t","host":"h9","source_type_name":"kubernetes"}""" "System" "agent-host"
    Assert.Equal(("h9", "kubernetes"), (own.Host, own.SourceTypeName))

    // An invented alert type is coerced AND kept.
    let invented = row """{"msg_title":"t","alert_type":"catastrophe"}""" "" ""
    Assert.Equal(("", "", "info", "normal", "catastrophe"), (invented.Host, invented.SourceTypeName, invented.AlertType, invented.Priority, invented.AlertTypeRaw))

[<Fact>]
let ``an intake event's numeric id is the first half of its id, inside an int64`` () =
    let row = Api.intakeEventRow Replay.testTenant "s" "h" (parsed "{}")
    let half = Convert.ToUInt64(row.EventID.ToString("N").Substring(0, 16), 16)
    Assert.Equal(half >>> 1, row.EventIDNum)

[<Fact>]
let ``a host row leaves absent FIPS keys null and absent sections empty`` () =
    let row = Api.hostRow Replay.testTenant "h1" "7.58.2" "linux" DateTime.UtcNow Map.empty Map.empty Map.empty
    Assert.True(row.FIPSMode.IsNone && row.FIPSProxyEnabled.IsNone)
    // An absent section is the empty string, not "{}".
    Assert.Equal(("", ""), (row.Meta, row.Network))

[<Fact>]
let ``an agent check stops where its array stops`` () =
    let row (json: string) =
        Api.agentCheckRow "t" DateTime.UtcNow "h" "7" "u" "" (Array.ofSeq ((parsed json).EnumerateArray()))

    Assert.True((row """["a","b","c"]""").Status.IsNone)
    Assert.Equal(Some 0L, (row """["a","b","c",0,"m"]""").Status)
    Assert.True((row """["a","b","c",1.5]""").Status.IsNone)
    Assert.True((row """["a","b","c","3"]""").Status.IsNone)
    Assert.Equal("", (row """["a","b","c",0,"m"]""").PositionalExtra)
    Assert.Equal("""[1,{"a":1.0}]""", (row """["n","s","i",2,"m",1,{"a":1.0}]""").PositionalExtra)
    Assert.Equal(("", Some 4L), ((row "[1,2,3,4,5]").CheckName, (row "[1,2,3,4,5]").Status))

[<Fact>]
let ``the small conversions`` () =
    Assert.Equal<string list>([ "GAUGE"; "COUNT"; "RATE"; "UNSPECIFIED"; "UNSPECIFIED" ], [ "Gauge"; "count"; "RATE"; ""; "histogram" ] |> List.map Api.normalizeTypeWord)
    Assert.Equal<string list>([ "OK"; "WARNING"; "CRITICAL"; "UNKNOWN"; "UNKNOWN" ], [ 0; 1; 2; 3; 42 ] |> List.map Api.statusName)
    Assert.Equal<string list>([ "error"; "info"; "info" ], [ "Error"; "catastrophe"; "" ] |> List.map Api.coerceAlertType)
    Assert.Equal<string list>([ "low"; "normal"; "normal" ], [ "LOW"; "urgent"; "" ] |> List.map Api.coercePriority)
    // What the Go server answered for the tenant "test".
    Assert.Equal(9003667037343323539L, Api.tenantOrgId "test")
    Assert.True(Api.tenantOrgId "" > 0L && Api.tenantOrgId "" % 2L = 1L)

// ---------------------------------------------------------------------------
// The handlers
// ---------------------------------------------------------------------------

[<Fact>]
let ``v2 series as JSON: repeated resources, bare tags, a field named either way`` () =
    let response, sink =
        post
            "/api/v2/series"
            """{"series":[{"metric":"m","type":3,"unit":"byte","sourceTypeName":"src","interval":10,"not_in_the_schema":1,
                "resources":[{"type":"host","name":"h"},{"type":"device","name":"eth0"},{"type":"device","name":"eth1"}],
                "points":[{"timestamp":1790151330,"value":2},{"value":3}],"tags":["","a:b","c"]}]}"""

    Assert.Equal((202, """{"errors":[]}"""), (response.Status, text response))
    Assert.Empty(raws sink)
    let points = sink.Rows<MetricPoint>()
    Assert.Equal<float list>([ 2.0; 3.0 ], points |> List.map _.Value)
    let second = points[0]
    Assert.Equal(("h", "GAUGE", "byte", "src", 10u), (second.Host, second.MetricType, second.Unit, second.SourceType, second.Interval))
    Assert.Equal(DateTime(2026, 9, 23, 8, 15, 30, DateTimeKind.Utc), second.Timestamp)
    // Of two devices the last one stays.
    Assert.Equal<(string * string) list>([ "device", "eth1" ], second.Resources |> Map.toList)
    Assert.Equal<(string * string[]) list>([ "", [| "" |]; "a", [| "b" |]; "c", [| "" |] ], second.Tags |> Map.toList)
    // A point without a timestamp is stamped on arrival.
    Assert.True(points[1].Timestamp.Year >= 2026 && points[1].Timestamp <> second.Timestamp)

[<Fact>]
let ``v2 series: the format is taken from Content-Type, then from the first byte`` () =
    let json = utf8 """ {"series":[{"metric":"sniffed","points":[{"timestamp":1790151330,"value":1}]}]}"""
    let _, sniffed = send "POST" "/api/v2/series" [] json
    Assert.Equal<string list>([ "sniffed" ], sniffed.Rows<MetricPoint>() |> List.map _.Metric)

    // Labelled protobuf, it is read as protobuf, and kept raw when that fails.
    let _, lied = send "POST" "/api/v2/series" [ "Content-Type", "application/x-protobuf" ] json
    Assert.Empty(lied.Rows<MetricPoint>())
    Assert.Equal<string list>([ "decode_error" ], raws lied |> List.map (fun (reason, _, _) -> reason))

[<Fact>]
let ``v2 series: wrong types are a decode error, kept raw`` () =
    for body in
        [ """{"series":[{"metric":"m","type":{}}]}"""
          """{"series":[{"metric":"m","interval":1.5}]}"""
          """{"series":[{"metric":"m","points":[{"timestamp":1790151330,"value":"x"}]}]}"""
          // Protobuf's JSON has no null inside a list.
          """{"series":[null,{"metric":"ok","points":[{"timestamp":1790151330,"value":1}]}]}"""
          "[1]" ] do
        let response, sink = post "/api/v2/series" body
        Assert.Equal(202, response.Status)
        Assert.Empty(sink.Rows<MetricPoint>())

        match raws sink with
        | [ "decode_error", note, kept ] ->
            Assert.StartsWith("JSON MetricPayload: ", note)
            Assert.Equal(body, kept)
        | other -> Assert.Fail $"{body}: {other}"

    // The agent's probe bodies are not kept.
    for body in [ "[]"; "{}"; "" ] do
        let _, sink = post "/api/v2/series" body
        Assert.Empty sink.Writes

[<Fact>]
let ``v2 series over protobuf: the first host, an unknown type, an interval that wraps`` () =
    let series = MetricPayload.Types.MetricSeries(Metric = "m.wire", Type = enum<MetricPayload.Types.MetricType> 7, Interval = 4294967297L)
    series.Resources.Add(MetricPayload.Types.Resource(Type = "device", Name = "d0"))
    series.Resources.Add(MetricPayload.Types.Resource(Type = "host", Name = "h"))
    series.Resources.Add(MetricPayload.Types.Resource(Type = "host", Name = "h2"))
    series.Points.Add(MetricPayload.Types.MetricPoint(Timestamp = 1790151330L, Value = 1.0))
    series.Points.Add(MetricPayload.Types.MetricPoint(Timestamp = -5L, Value = 3.0))
    series.Metadata <- Metadata(Origin = Origin(OriginProduct = 10u))
    let payload = MetricPayload()
    payload.Series.Add series

    let response, sink = send "POST" "/api/v2/series" [ "Content-Type", "application/x-protobuf" ] (payload.ToByteArray())
    Assert.Equal(202, response.Status)

    match sink.Rows<MetricPoint>() with
    | [ first; second ] ->
        Assert.Equal(("h", "UNSPECIFIED", 1u, 10u), (first.Host, first.MetricType, first.Interval, first.OriginProduct))
        Assert.Equal<(string * string) list>([ "device", "d0" ], first.Resources |> Map.toList)
        Assert.Equal(DateTime(2026, 9, 23, 8, 15, 30, DateTimeKind.Utc), first.Timestamp)
        Assert.True(second.Timestamp.Year >= 2026)
    | other -> Assert.Fail $"{other}"

[<Fact>]
let ``protobuf: a known field with another wire type is skipped, as protobuf says`` () =
    // series[0].metric (field 2) sent as a varint: to the parser that is a
    // field it does not know, not a broken message.
    let body = [| 0x0Auy; 0x02uy; 0x10uy; 0x05uy |]

    match parseSeriesV2Protobuf body with
    | Ok payload -> Assert.Equal("", (Assert.Single payload.Series).Metric)
    | Error e -> Assert.Fail e

    // Repeated numbers may arrive packed or one by one; neither is an error.
    let sketch = SketchPayload.Types.Sketch.Types.Dogsketch(Ts = 1L)
    sketch.K.AddRange [ 1; 2 ]
    let packed = sketch.ToByteArray()
    let oneByOne = [| 0x08uy; 0x01uy; 0x38uy; 0x02uy; 0x38uy; 0x04uy |]
    Assert.Equal<int list>([ 1; 2 ], SketchPayload.Types.Sketch.Types.Dogsketch.Parser.ParseFrom(oneByOne).K |> List.ofSeq)

    for dogsketch in [ packed; oneByOne ] do
        let wrapped = SketchPayload()
        let holder = SketchPayload.Types.Sketch(Metric = "m")
        holder.Dogsketches.Add(SketchPayload.Types.Sketch.Types.Dogsketch.Parser.ParseFrom dogsketch)
        wrapped.Sketches.Add holder
        Assert.True((parseSketchesProtobuf (wrapped.ToByteArray())).IsOk)

[<Fact>]
let ``v1 series: what the agent's encoder adds, and what is kept raw`` () =
    let _, sink =
        post
            "/api/v1/series"
            """{"series":[{"metric":"m","points":[[1790151330,1],[1790151331.9,2,3],null,[5]],"device":5,"unit":7,"interval":4294967297,"type":"Gauge","host":"h","tags":[null,"a:b"],"n":1.0}],"batch":1,"another":2}"""

    Assert.Equal<(string * string) list>(
        [ "unexpected_shape", "v1 payload carried undeclared top-level keys: another, batch"
          "unexpected_shape", "series[0].points[2]: expected a [timestamp, value] pair of numbers"
          "unexpected_shape", "series[0].points[3]: expected a [timestamp, value] pair of numbers" ],
        raws sink |> List.map (fun (reason, note, _) -> reason, note)
    )

    Assert.Equal<string list>([ "null"; "[5]" ], raws sink |> List.tail |> List.map (fun (_, _, body) -> body))

    match sink.Rows<MetricPoint>() with
    | [ first; second ] ->
        Assert.Equal(("h", "GAUGE", 1u, "", ""), (first.Host, first.MetricType, first.Interval, first.Unit, first.SourceType))
        // A device that is not a string is neither a resource nor an extra.
        Assert.True first.Resources.IsEmpty
        Assert.Equal<(string * string) list>([ "n", "1.0" ], first.Extra |> Map.toList)
        // The fraction of a timestamp is dropped; a third element is ignored.
        Assert.Equal((DateTime(2026, 9, 23, 8, 15, 31, DateTimeKind.Utc), 2.0), (second.Timestamp, second.Value))
    | other -> Assert.Fail $"{other}"

[<Fact>]
let ``v1 series: a long list of undeclared keys is cut at eight`` () =
    let _, sink = post "/api/v1/series" """{"series":[],"k1":1,"k2":1,"k3":1,"k4":1,"k5":1,"k6":1,"k7":1,"k8":1,"k9":1,"k0":1}"""

    Assert.Equal<string list>(
        [ "v1 payload carried undeclared top-level keys: k0, k1, k2, k3, k4, k5, k6, k7 ... 2 more" ],
        raws sink |> List.map (fun (_, note, _) -> note)
    )

[<Fact>]
let ``v1 series: timestamps no int64 holds, and none at all, mean arrival`` () =
    let _, sink = post "/api/v1/series" """{"series":[{"metric":"m","points":[[1e30,1],[-5,2],[0,3]]}]}"""
    let points = sink.Rows<MetricPoint>()
    Assert.Equal(3, points.Length)
    Assert.All(points, (fun point -> Assert.True(point.Timestamp.Year >= 2026 && point.Timestamp.Year < 2100)))

[<Fact>]
let ``the diagnose sweep is answered and nothing is decoded`` () =
    let sweep = [ "Content-Type", "application/json"; "X-Requested-With", "datadog-agent-diagnose" ]

    for path, status, body in
        [ "/api/v2/series", 202, """{"errors":[]}"""
          "/api/v1/series", 202, """{"errors":[]}"""
          "/api/beta/sketches", 202, """{"errors":[]}"""
          "/api/v1/check_run", 202, """{"errors":[]}"""
          "/api/v1/distribution_points", 202, """{"errors":[]}"""
          "/api/v1/events", 202, """{"status":"ok"}"""
          "/intake/", 200, """{"status":"ok"}"""
          "/api/v1/metadata", 200, """{"status":"ok"}""" ] do
        let response, sink = send "POST" path sweep (utf8 """{"x":1}""")
        Assert.Equal((status, body), (response.Status, text response))
        Assert.Empty sink.Writes

[<Fact>]
let ``sketches over protobuf: a legacy distribution is kept raw, the sender's key is not kept at all`` () =
    let legacy = SketchPayload.Types.Sketch(Metric = "s.wire", Host = "h", Metadata = Metadata())
    legacy.Tags.Add "a:b"
    let distribution = SketchPayload.Types.Sketch.Types.Distribution(Ts = 1L, Min = 1.5)
    distribution.V.AddRange [ 1.0; 2.5 ]
    distribution.G.Add 1u
    legacy.Distributions.Add distribution

    let current = SketchPayload.Types.Sketch(Metric = "s2.wire")
    let dogsketch = SketchPayload.Types.Sketch.Types.Dogsketch(Ts = 1790151330L, Cnt = -1L)
    dogsketch.K.AddRange [ -3; 4 ]
    dogsketch.N.AddRange [ 1u; 2u ]
    current.Dogsketches.Add dogsketch

    let payload = SketchPayload(Metadata = CommonMetadata(ApiKey = "only-the-key"))
    payload.Sketches.Add legacy
    payload.Sketches.Add current

    let response, sink = send "POST" "/api/beta/sketches" [ "Content-Type", "application/x-protobuf" ] (payload.ToByteArray())
    Assert.Equal(202, response.Status)

    Assert.Equal<(string * string * string) list>(
        [ "no_schema",
          "legacy pre-DDSketch Distribution for s.wire has no row shape",
          """{ "metric": "s.wire", "host": "h", "distributions": [ { "ts": "1", "min": 1.5, "v": [ 1, 2.5 ], "g": [ 1 ] } ], "tags": [ "a:b" ], "metadata": { } }""" ],
        raws sink
    )

    match sink.Rows<SketchRow>() with
    | [ row ] ->
        Assert.Equal(("s2.wire", UInt64.MaxValue), (row.Metric, row.Count))
        Assert.Equal<int32[]>([| -3; 4 |], row.BucketKeys)
        Assert.Equal<uint32[]>([| 1u; 2u |], row.BucketCounts)
    | other -> Assert.Fail $"{other}"

    // A sender block with nothing but the key is not a row.
    Assert.Empty(sink.Rows<AgentBatchMetadataRow>())

[<Fact>]
let ``sketches as JSON are read as the protobuf message`` () =
    let _, sink =
        post
            "/api/beta/sketches"
            """{"sketches":[{"metric":"m","host":"h","dogsketches":[{"ts":1790151330,"cnt":3,"k":[1,0],"n":[2]}],"tags":["a:b"]}],"metadata":{"timezone":"UTC"}}"""

    Assert.Empty(raws sink)
    Assert.Equal<int32[] list>([ [| 1; 0 |] ], sink.Rows<SketchRow>() |> List.map _.BucketKeys)
    Assert.Equal<string list>([ "UTC" ], sink.Rows<AgentBatchMetadataRow>() |> List.map _.Timezone)

[<Fact>]
let ``check_run: a check that does not fit is kept raw with its place and the reason`` () =
    let _, sink =
        post
            "/api/v1/check_run"
            """[{"check":"a","host_name":"h","status":0},
                {"check":"b","host_name":"h","status":3,"tags":null,"timestamp":1790151330,"message":"m","extra":{"k":[1, 2]}},
                {"check":"c","host_name":"h","status":4,"tags":[]},
                5,
                {"check":"f","status":1}]"""

    Assert.Equal<(string * string) list>(
        [ "[2].status: expected 0 to 3, got 4", """{"check":"c","host_name":"h","status":4,"tags":[]}"""
          "[3]: expected an object, got number", "5"
          "[4].host_name: missing", """{"check":"f","status":1}""" ],
        raws sink |> List.map (fun (_, note, body) -> note, body)
    )

    match sink.Rows<CheckRunRow>() with
    | [ a; b ] ->
        Assert.Equal(("a", "OK"), (a.CheckName, a.Status))
        Assert.True(a.Timestamp.Year >= 2026)
        Assert.Equal(("b", "UNKNOWN", "m"), (b.CheckName, b.Status, b.Message))
        Assert.Equal<(string * string) list>([ "extra", """{"k":[1,2]}""" ], b.Extra |> Map.toList)
    | other -> Assert.Fail $"{other}"

[<Fact>]
let ``check_run: a bare check without tags is rejected, and a null in a batch is left out`` () =
    let _, bare = post "/api/v1/check_run" """{"check":"c","host_name":"h","status":1}"""
    Assert.Equal<(string * string) list>([ "decode_error", "JSON check_run: tags: missing" ], raws bare |> List.map (fun (reason, note, _) -> reason, note))

    let response, withNull = post "/api/v1/check_run" """[{"check":"a","host_name":"h","status":0,"tags":[]},null,"x"]"""
    Assert.Equal((202, """{"errors":[]}"""), (response.Status, text response))
    Assert.Equal<string list>([ "a" ], withNull.Rows<CheckRunRow>() |> List.map _.CheckName)
    Assert.Equal<(string * string * string) list>([ "unexpected_shape", "[2]: expected an object, got string", "\"x\"" ], raws withNull)

    let _, nothing = post "/api/v1/check_run" "null"
    Assert.Empty nothing.Writes

[<Fact>]
let ``events: a body with no event, or with an event that does not fit, is answered 400`` () =
    for body in [ "[]"; "null" ] do
        let response, sink = post "/api/v1/events" body
        Assert.Equal(400, response.Status)
        Assert.Equal("no event with a title", (parsed (text response)).GetProperty("error").GetString())
        Assert.Empty sink.Writes

    let refused (body: string) (error: string) =
        let response, sink = post "/api/v1/events" body
        Assert.Equal(400, response.Status)
        let reply = parsed (text response)
        Assert.Equal(("error", error), (reply.GetProperty("status").GetString(), reply.GetProperty("error").GetString()))
        Assert.Equal<(string * string * string) list>([ "decode_error", error, body ], raws sink)

    refused """{"title":"t"}""" "JSON events: text: missing"
    // One event that does not fit refuses the ones beside it too.
    refused """[{"title":"a","text":"b"},{"title":"c","text":"d","tags":"x"}]""" "JSON events: [1].tags: expected a list of strings, got string"

[<Fact>]
let ``events: an invented alert type is coerced and kept beside the coerced one`` () =
    let response, sink =
        post
            "/api/v1/events"
            """[{"title":"a","text":"b","alert_type":"catastrophe","priority":"URGENT","host":"h1"},
                {"title":"c","text":"d","tags":[],"priority":null,"aggregation_key":"k","source_type_name":"s","device_name":"d","date_happened":0,"alert_type":"error"}]"""

    Assert.Equal(202, response.Status)
    let event = (parsed (text response)).GetProperty "event"
    Assert.Equal(("a", "b", "h1", "info", "normal"), (event.GetProperty("title").GetString(), event.GetProperty("text").GetString(), event.GetProperty("host").GetString(), event.GetProperty("alert_type").GetString(), event.GetProperty("priority").GetString()))
    Assert.Equal(JsonValueKind.Null, event.GetProperty("tags").ValueKind)
    Assert.Equal(string (event.GetProperty("id").GetUInt64()), event.GetProperty("id_str").GetString())
    Assert.Empty(raws sink)

    match sink.Rows<EventRow>() with
    | [ first; second ] ->
        Assert.Equal(("a", "info", "catastrophe", "normal", "URGENT"), (first.Title, first.AlertType, first.AlertTypeRaw, first.Priority, first.PriorityRaw))
        // The reply's id is the first event's.
        Assert.Equal(event.GetProperty("id").GetUInt64(), first.EventIDNum)
        Assert.Equal(("c", "error", "error", "normal", "", "k", "s", "d"), (second.Title, second.AlertType, second.AlertTypeRaw, second.Priority, second.PriorityRaw, second.AggregationKey, second.SourceTypeName, second.DeviceName))
    | other -> Assert.Fail $"{other}"

[<Fact>]
let ``distribution points: every pair that cannot be split is kept raw, the rest become sketches`` () =
    let _, sink =
        post
            "/api/v1/distribution_points"
            """{"series":[{"metric":"m","host":"h","points":[[1790151330,[1,null,3]],[null,[1]],["x",[1]],[1790151330,[]],[1790151330,["a"]],null,[1790151330,{"a":1}],[[1],[2],5,1790151330]],"k":"v"}]}"""

    Assert.Equal<(string * string) list>(
        [ "series[0].points[2]: expected a [timestamp, [values]] pair", """["x",[1]]"""
          "series[0].points[3]: expected a [timestamp, [values]] pair", """[1790151330,[]]"""
          "series[0].points[4]: expected a [timestamp, [values]] pair", """[1790151330,["a"]]"""
          "series[0].points[5]: expected a [timestamp, [values]] pair", "null"
          "series[0].points[6]: expected a [timestamp, [values]] pair", """[1790151330,{"a":1}]""" ],
        raws sink |> List.map (fun (_, note, body) -> note, body)
    )

    let rows = sink.Rows<SketchRow>()
    Assert.Equal<(uint64 * float) list>([ 3UL, 4.0; 1UL, 1.0; 1UL, 2.0 ], rows |> List.map (fun row -> row.Count, row.Sum))
    // A null among the values is a zero.
    Assert.Equal<int32[]>([| 0; 1338; 1409 |], rows[0].BucketKeys)
    Assert.Equal<(string * string) list>([ "k", "\"v\"" ], rows[0].Extra |> Map.toList)

[<Fact>]
let ``intake: a body of no known shape is kept, with the keys it had`` () =
    let notes (body: string) =
        let response, sink = post "/intake/" body
        Assert.Equal((200, """{"status":"ok"}"""), (response.Status, text response))
        raws sink |> List.map (fun (reason, note, _) -> reason, note)

    Assert.Equal<(string * string) list>([ "unexpected_shape", "no known /intake/ variant key, keys: (none)" ], notes "null")
    Assert.Equal<(string * string) list>([ "decode_error", "expected an object, got array" ], notes "[1]")
    // A resources snapshot with nothing in it is not an error and stores nothing.
    Assert.Empty(notes """{"resources":{"processes":{"snaps":[]}}}""")
    Assert.Equal<(string * string) list>([ "unexpected_shape", "resources without processes.snaps" ], notes """{"resources":{"meta":{"host":"h"}}}""")

    Assert.Equal<(string * string) list>(
        [ "unexpected_shape", "resources: 2 snapshots or rows are not the resources check's shape" ],
        notes """{"resources":{"processes":{"snaps":[[1790878655,[["root",0,0.5,10,5,"agent",1],["short"]]],"x"]}}}"""
    )
    Assert.Equal<(string * string) list>([ "unexpected_shape", "no known /intake/ variant key, keys: other, resources" ], notes """{"resources":{},"other":1}""")
    Assert.Empty(notes "")

[<Fact>]
let ``intake events: sources in sorted order, a null event is an empty one, odd fields are absent`` () =
    let _, sink =
        post
            "/intake/"
            """{"events":{"b":[{"msg_title":"t2","timestamp":1.5,"tags":["a:b",5],"host":7,"priority":"LOW","alert_type":"Error"},null],
                          "a":[{"msg_title":"t1","timestamp":"x","x":{"k": 1}}],"c":null},"internalHostname":5}"""

    match sink.Rows<EventRow>() with
    | [ a; b; empty ] ->
        Assert.Equal(("t1", "a", "info", ""), (a.Title, a.SourceTypeName, a.AlertType, a.Host))
        Assert.Equal<(string * string) list>([ "x", """{"k":1}""" ], a.Extra |> Map.toList)
        Assert.Equal(("t2", "b", "error", "Error", "low", "LOW"), (b.Title, b.SourceTypeName, b.AlertType, b.AlertTypeRaw, b.Priority, b.PriorityRaw))
        Assert.Equal<(string * string[]) list>([ "a", [| "b" |] ], b.Tags |> Map.toList)
        Assert.True(b.Timestamp.Year >= 2026)
        Assert.Equal(("", "b"), (empty.Title, empty.SourceTypeName))
    | other -> Assert.Fail $"{other}"

[<Fact>]
let ``intake events: a shape that is not source to events is kept raw, with what did not fit`` () =
    let note (body: string) =
        let _, sink = post "/intake/" body
        raws sink |> List.map (fun (_, note, kept) -> note, kept)

    Assert.Equal<(string * string) list>(
        [ "events: expected an object, got array", "[1]" ],
        note """{"events":[1]}"""
    )

    Assert.Equal<(string * string) list>(
        [ "events.a[2]: expected an object, got string", """{"a":[{"msg_title":"t"},null,"s"],"b":5}""" ],
        note """{"events":{"a":[{"msg_title":"t"},null,"s"],"b":5}}"""
    )

    Assert.Equal<string list>(
        [ "events.b: expected a list, got number" ],
        note """{"events":{"b":5,"a":[{"msg_title":"t"}]}}""" |> List.map fst
    )

[<Fact>]
let ``intake agent checks: entries that are not what they should be are kept raw, the rest stored`` () =
    let _, sink =
        post
            "/intake/"
            """{"agent_checks":[["a"],[],null,"x",["n","s","i",2,"m",1,{"a":1.0}]],
                "external_host_tags":[["h",{"s":["a:b"],"t":5}],["only"],null,[5,{"z":["k:v"]}]],
                "meta":{"a": 1},"uuid":5,"internalHostname":"ah","agentVersion":"7"}"""

    Assert.Equal<(string * string) list>(
        [ "external_host_tags[1] is not a [hostname, tags] pair", """["only"]"""
          "external_host_tags[2] is not a [hostname, tags] pair", "null"
          "agent_checks[2] is not a positional array", "null"
          "agent_checks[3] is not a positional array", "\"x\"" ],
        raws sink |> List.map (fun (_, note, body) -> note, body)
    )

    let checks = sink.Rows<AgentCheckRow>()
    Assert.Equal<(string * int64 option) list>([ "a", None; "", None; "n", Some 2L ], checks |> List.map (fun check -> check.CheckName, check.Status))
    Assert.All(checks, (fun check -> Assert.Equal(("ah", "7", "", """{"a": 1}"""), (check.Hostname, check.AgentVersion, check.UUID, check.Meta))))

    Assert.Equal<(string * string * (string * string[]) list) list>(
        [ "h", "s", [ "a", [| "b" |] ]; "h", "t", []; "", "z", [ "k", [| "v" |] ] ],
        sink.Rows<ExternalHostTagsRow>() |> List.map (fun row -> row.Host, row.Source, Map.toList row.Tags)
    )

    // The writes, in the Go server's order.
    Assert.Equal<string list>(
        [ "storage_raw_payloads"; "storage_raw_payloads"; "storage_raw_payloads"; "storage_raw_payloads"
          "storage_agent_checks"; "storage_external_host_tags" ],
        sink.Writes |> List.map _.Writer
    )

[<Fact>]
let ``intake host: loose sections are read leniently, and a host without a name is no row`` () =
    let _, sink =
        post
            "/intake/"
            """{"systemStats":{"a":1},"internalHostname":"h","gohai":null,"fips_mode":null,"fips_proxy_enabled":"yes",
                "host-tags":{"system":["a:b",5],"x":7},"meta":null,"install-method":[1],"proxy-info":null,"container-meta":{"k":null},"os":5}"""

    match sink.Rows<HostRow>() with
    | [ host ] ->
        // A section kept as JSON text keeps the null that was sent.
        Assert.Equal(("h", "", "null"), (host.Host, host.OS, host.Meta))
        // null is the same as absent, and so is a value that is no boolean.
        Assert.Equal((None, None), (host.FIPSMode, host.FIPSProxyEnabled))
        Assert.Equal<(string * string[]) list>([ "system", [| "a:b"; "" |]; "x", [||] ], host.HostTags |> Map.toList)
        Assert.Equal<(string * string[]) list>([ "", [| "" |]; "a", [| "b" |] ], host.Tags |> Map.toList)
        Assert.Equal<(string * string) list>([ "a", "1" ], host.SystemStats |> Map.toList)
        Assert.Equal<(string * string) list>([ "k", "null" ], host.ContainerMeta |> Map.toList)
        Assert.True(host.InstallMethod.IsEmpty && host.ProxyInfo.IsEmpty && host.IntakeExtra.IsEmpty)
    | other -> Assert.Fail $"{other}"

    let _, gohai =
        post "/intake/" """{"gohai":"{\"platform\":{\"os\":1},\"cpu\":{\"a\":null},\"memory\":null,\"filesystem\":null,\"network\":null}","internalHostname":"h"}"""

    match gohai.Rows<HostRow>() with
    | [ host ] ->
        // One value that is not a string drops the section. A null among
        // strings is ""; in what is kept as JSON text it stays null.
        Assert.True host.Platform.IsEmpty
        Assert.Equal<(string * string) list>([ "a", "" ], host.CPU |> Map.toList)
        Assert.Equal("null", host.Filesystem)
        Assert.Equal<(string * string) list>([ "network", "null" ], host.GohaiExtra |> Map.toList)
    | other -> Assert.Fail $"{other}"

    let _, nameless = post "/intake/" """{"gohai":"{}"}"""
    Assert.Empty nameless.Writes

[<Fact>]
let ``metadata: one row per variant, a key in another case is only an extra`` () =
    let _, sink =
        post
            "/api/v1/metadata"
            """{"hostname":"h","timestamp":5,"agent_metadata":{"a": 1},"host_metadata":null,"clustercheck_status":{"x":1},"uuid":"u","UUID":"other","clustername":"c","cluster_id":"cid"}"""

    let rows = sink.Rows<AgentMetadataRow>()
    Assert.Equal<(string * string) list>([ "agent_metadata", """{"a": 1}"""; "host_metadata", "null" ], rows |> List.map (fun row -> row.Variant, row.Payload))

    Assert.All(
        rows,
        (fun row ->
            Assert.Equal(("h", "c", "cid", "u", Some(DateTime(1970, 1, 1, 0, 0, 5, DateTimeKind.Utc))), (row.Hostname, row.ClusterName, row.ClusterID, row.UUID, row.Timestamp))
            Assert.Equal<(string * string) list>([ "UUID", "\"other\""; "clustercheck_status", """{"x":1}""" ], row.EnvelopeExtra |> Map.toList))
    )

    let _, odd = post "/api/v1/metadata" """{"hostname":5,"timestamp":1.5,"ha_agent_metadata":{}}"""
    Assert.Equal<(string * DateTime option) list>([ "", None ], odd.Rows<AgentMetadataRow>() |> List.map (fun row -> row.Hostname, row.Timestamp))

    let _, array = post "/api/v1/metadata" "[1]"
    Assert.Equal<(string * string) list>([ "decode_error", "expected an object, got array" ], raws array |> List.map (fun (reason, note, _) -> reason, note))

[<Fact>]
let ``GET /api/v1/query answers a well-formed empty result and stores nothing`` () =
    let response, sink = send "GET" "/api/v1/query?from=1&to=2&query=avg:system.cpu.idle{*},sum:x{a:b}" [] [||]
    Assert.Equal(200, response.Status)
    let reply = parsed (text response)
    Assert.Equal(("ok", "time_series", "avg:system.cpu.idle{*},sum:x{a:b}"), (reply.GetProperty("status").GetString(), reply.GetProperty("res_type").GetString(), reply.GetProperty("query").GetString()))
    Assert.Equal(0, reply.GetProperty("series").GetArrayLength())
    Assert.Empty sink.Writes

[<Fact>]
let ``the runner's health check answers 200 with the server's time`` () =
    let response, sink = send "GET" "/api/v2/on-prem-management-service/runner/health-check" [] [||]
    Assert.Equal((200, "{}"), (response.Status, text response))
    let serverTime = response.Headers |> List.find (fun (name, _) -> name = "X-Server-Time") |> snd
    Assert.Matches(@"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$", serverTime)
    Assert.True((Time.tryRfc3339 serverTime).IsSome)
    Assert.Empty sink.Writes

[<Fact>]
let ``/api/v2/validate answers the tenant's org id, the same on every call, and stores nothing`` () =
    let orgId (method: string) =
        let response, sink = send method "/api/v2/validate" [] [||]
        Assert.Equal(200, response.Status)
        Assert.Empty sink.Writes
        (parsed (text response)).GetProperty("data").GetProperty("id").GetString()

    // The agent derives its Org Propagation Marker from the id: it has to be stable.
    let first = orgId "GET"
    Assert.True(fst (Guid.TryParse first))
    Assert.Equal(first, orgId "GET")
    Assert.Equal(first, orgId "POST")

[<Fact>]
let ``intake-key: the scheme is what precedes the first space, and the key echoed is the one that admitted`` () =
    let exchange (url: string) (headers: (string * string) list) =
        let response, sink = send "POST" url headers [||]
        let key = (parsed (text response)).GetProperty("data").GetProperty("attributes").GetProperty("api_key").GetString()
        key, sink.Rows<DelegatedAuthRow>() |> List.map (fun row -> row.Scheme, row.ProofFingerprint.Length, row.APIKeyID)

    Assert.Equal((Replay.testKey, [ "", 0, "test" ]), exchange "/api/v2/intake-key" [])
    Assert.Equal((Replay.testKey, [ "Bearer", 0, "test" ]), exchange "/api/v2/intake-key" [ "Authorization", "Bearer" ])
    Assert.Equal((Replay.testKey, [ "A", 64, "test" ]), exchange "/api/v2/intake-key" [ "Authorization", "A  B C" ])
    // Admitted through the query parameter, that is the key it gets back.
    Assert.Equal((Replay.testKey, [ "Delegated", 64, "test" ]), exchange $"/api/v2/intake-key?api_key={Replay.testKey}" [ "Dd-Api-Key", ""; "Authorization", "Delegated p" ])

[<Fact>]
let ``the runner's requests: a body that is not JSON:API is kept raw and the answer does not move`` () =
    for path, status, body, note in
        [ "/api/v2/on-prem-management-service/workflow-tasks/dequeue", 200, "", "dequeue: not a JSON:API document"
          "/api/v2/on-prem-management-service/workflow-tasks/publish-task-update", 202, "{}", "task-update: not a JSON:API document"
          "/api/v2/on-prem-management-service/workflow-tasks/heartbeat", 200, "{}", "heartbeat: not a JSON:API document"
          "/api/v2/actions/connections", 202, "{}", "connections: not a JSON:API document"
          "/api/v2/profiles/symbols/query", 200, """{"data":[]}""", "not a JSON:API document" ] do
        for sent in [ "not json"; "null"; """{"data":[]}""" ] do
            let response, sink = post path sent
            Assert.Equal((status, body), (response.Status, text response))
            Assert.Equal<(string * string * string) list>([ "decode_error", note, sent ], raws sink)

[<Fact>]
let ``the runner's dequeue: a document with no data is still a poll`` () =
    let response, sink = post "/api/v2/on-prem-management-service/workflow-tasks/dequeue" """{"data":null}"""
    Assert.Equal((200, 0), (response.Status, response.Body.Length))
    Assert.Empty response.Headers
    Assert.Equal<(string * string) list>([ "", "" ], sink.Rows<RunnerDequeueRow>() |> List.map (fun row -> row.RunnerStartedAt, row.LastTaskReceivedAt))

[<Fact>]
let ``the runner's task update: an error code of null reads as 0, one that is no integer as none`` () =
    let update (attributes: string) =
        let _, sink =
            post "/api/v2/on-prem-management-service/workflow-tasks/publish-task-update" ("""{"data":{"id":5,"attributes":""" + attributes + "}}")

        sink.Rows<RunnerTaskUpdateRow>() |> List.exactlyOne

    let withNull = update """{"task_id":"t","client":null,"payload":{"error_code":null,"outputs":[1, 2]}}"""
    Assert.Equal(("", "t", "null", "[1, 2]", Some 0L), (withNull.Outcome, withNull.TaskID, withNull.Client, withNull.Outputs, withNull.ErrorCode))
    Assert.True((update """{"payload":{"error_code":1.5}}""").ErrorCode.IsNone)

    let notAnObject = update """{"payload":"nope","more":1}"""
    Assert.Equal<(string * string) list>([ "more", "1" ], notAnObject.Extra |> Map.toList)

[<Fact>]
let ``runner enrollment: the modes are answered as sent, null when not sent`` () =
    let enroll (attributes: string) =
        let response, sink = post "/api/unstable/on_prem_runners" ("""{"data":{"attributes":""" + attributes + "}}")
        Assert.Equal(200, response.Status)
        Assert.Equal<(string * string) list>([ "Content-Type", "application/vnd.api+json" ], response.Headers)
        let data = (parsed (text response)).GetProperty "data"
        let row = sink.Rows<RunnerEnrollmentRow>() |> List.exactlyOne
        Assert.Equal(row.RunnerID, data.GetProperty("id").GetString())
        Assert.Equal(row.OrgID, data.GetProperty("attributes").GetProperty("org_id").GetInt64())
        data.GetProperty("attributes").GetProperty("runner_modes").GetRawText(), row

    let modes, row = enroll """{"runner_name":"r"}"""
    Assert.Equal(("null", "r", "{\"runner_name\":\"r\"}"), (modes, row.Name, row.Attributes))
    Assert.Empty row.Modes
    Assert.Equal("[]", fst (enroll """{"runner_modes":[],"agent_hostname":5}"""))
    Assert.Equal("""["a",""]""", fst (enroll """{"runner_modes":["a",5]}"""))

    // No attributes at all is still an enrollment, with an empty JSON object.
    let response, sink = post "/api/unstable/on_prem_runners" """{"data":null}"""
    Assert.Equal(200, response.Status)
    Assert.Equal<string list>([ "{}" ], sink.Rows<RunnerEnrollmentRow>() |> List.map _.Attributes)

[<Fact>]
let ``action connections and symbol queries read their lists leniently`` () =
    let _, connection = post "/api/v2/actions/connections" """{"data":{"attributes":{"name":"n","tags":["a:b",5,null],"integration":"x"}}}"""

    match connection.Rows<ActionConnectionRow>() with
    | [ row ] ->
        Assert.Equal(("n", "", ""), (row.Name, row.IntegrationType, row.Credentials))
        Assert.Equal<(string * string[]) list>([ "", [| ""; "" |]; "a", [| "b" |] ], row.Tags |> Map.toList)
    | other -> Assert.Fail $"{other}"

    let _, symbols = post "/api/v2/profiles/symbols/query" """{"data":{"attributes":{"arch":5,"buildIds":["a",5,null]}}}"""
    Assert.Equal<(string * string[]) list>([ "", [| "a"; ""; "" |] ], symbols.Rows<SymbolQueryRow>() |> List.map (fun row -> row.Arch, row.BuildIDs))
