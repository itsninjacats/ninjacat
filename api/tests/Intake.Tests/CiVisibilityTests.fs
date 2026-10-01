/// Tests of Routers/CiVisibility.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.CiVisibilityTests

open System
open System.IO
open System.Text
open Xunit
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers.CiVisibility
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows

/// Sends one request, with the test key, to the engine of one Go route set.
let send (routeSet: string) (method: string) (url: string) (headers: (string * string) list) (body: byte[]) : Response * CapturingSink =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let path, query =
        match url.IndexOf '?' with
        | -1 -> url, ""
        | i -> url.Substring(0, i), url.Substring i

    let http = DefaultHttpContext()
    http.Request.Method <- method
    http.Request.Host <- HostString "example.com"
    http.Request.Path <- PathString path
    http.Request.QueryString <- QueryString query
    http.Request.Headers["Dd-Api-Key"] <- StringValues Replay.testKey

    for name, value in headers do
        http.Request.Headers[name] <- StringValues value

    Replay.byGoNames deps [ routeSet ] http body, sink

let utf8 (text: string) : byte[] = Encoding.UTF8.GetBytes text

type private Part =
    { Name: string
      FileName: string
      ContentType: string
      Data: byte[] }

/// A multipart body and its Content-Type.
let private multipart (parts: Part list) : string * byte[] =
    use body = new MemoryStream()

    let write (text: string) =
        let bytes = utf8 text
        body.Write(bytes, 0, bytes.Length)

    for part in parts do
        write "--frontier\r\n"
        write $"Content-Disposition: form-data; name=\"{part.Name}\""

        if part.FileName <> "" then
            write $"; filename=\"{part.FileName}\""

        write "\r\n"

        if part.ContentType <> "" then
            write $"Content-Type: {part.ContentType}\r\n"

        write "\r\n"
        body.Write(part.Data, 0, part.Data.Length)
        write "\r\n"

    write "--frontier--\r\n"
    "multipart/form-data; boundary=frontier", body.ToArray()

let private json (text: string) : Value =
    match decodeAny "application/json" (utf8 text) with
    | _, Ok value -> value
    | _, Error e -> failwith e

[<Fact>]
let ``a float where text was expected is written as Go wrote it`` () =
    let cases =
        [ 1234567.0, "1.234567e+06"
          100000.0, "100000"
          1e6, "1e+06"
          1e20, "1e+20"
          1e21, "1e+21"
          123456789012345678.0, "1.2345678901234568e+17"
          0.0001, "0.0001"
          0.00001, "1e-05"
          0.000123, "0.000123"
          1.5, "1.5"
          120.0, "120"
          -0.0, "-0"
          nan, "NaN"
          infinity, "+Inf"
          -infinity, "-Inf"
          1e-7, "1e-07"
          5e-324, "5e-324"
          1.7976931348623157e308, "1.7976931348623157e+308"
          12345.678, "12345.678"
          -12345.678, "-12345.678"
          1e100, "1e+100"
          1.5e-10, "1.5e-10" ]

    for number, expected in cases do
        Assert.Equal(expected, Value.text (Value.Float number))

    let singles =
        [ 0.1f, "0.1"; 1234567f, "1.234567e+06"; 1e6f, "1e+06"; 16777216f, "1.6777216e+07"; 3.4e38f, "3.4e+38"; 1e-5f, "1e-05" ]

    for number, expected in singles do
        Assert.Equal(expected, Value.text (Value.Float32 number))

[<Fact>]
let ``text renders every wire type exactly`` () =
    Assert.Equal("", Value.text Value.Null)
    Assert.Equal("s", Value.text (Value.String "s"))
    Assert.Equal("raw", Value.text (Value.Bytes(utf8 "raw")))
    Assert.Equal("1.50", Value.text (Value.Number "1.50"))
    Assert.Equal("true", Value.text (Value.Bool true))
    Assert.Equal("false", Value.text (Value.Bool false))
    Assert.Equal("-5", Value.text (Value.Int -5L))
    Assert.Equal("18446744073709551615", Value.text (Value.UInt UInt64.MaxValue))
    Assert.Equal("""[1,"a"]""", Value.text (json """[1, "a"]"""))
    Assert.Equal("""{"a":{"b":null}}""", Value.text (json """{"a": {"b": null}}"""))

[<Fact>]
let ``an id is never read through a float`` () =
    Assert.Equal(UInt64.MaxValue, Value.toUInt64 (Value.UInt UInt64.MaxValue))
    Assert.Equal(7UL, Value.toUInt64 (Value.Int 7L))
    Assert.Equal(0UL, Value.toUInt64 (Value.Int -1L))
    // 2^53 + 1: the first integer a float cannot hold.
    Assert.Equal(9007199254740993UL, Value.toUInt64 (Value.Number "9007199254740993"))
    Assert.Equal(0UL, Value.toUInt64 (Value.Number "1.0"))
    Assert.Equal(0UL, Value.toUInt64 (Value.Number "1e3"))
    Assert.Equal(0UL, Value.toUInt64 (Value.Number "-1"))
    Assert.Equal(42UL, Value.toUInt64 (Value.String "42"))
    Assert.Equal(0UL, Value.toUInt64 (Value.String "+42"))
    Assert.Equal(0UL, Value.toUInt64 (Value.String " 42"))
    Assert.Equal(120UL, Value.toUInt64 (Value.Float 120.0))
    Assert.Equal(0UL, Value.toUInt64 (Value.Float 120.5))
    Assert.Equal(0UL, Value.toUInt64 (Value.Float -1.0))
    Assert.Equal(0UL, Value.toUInt64 (Value.Float 9007199254740992.0))
    Assert.Equal(0UL, Value.toUInt64 (Value.Float nan))
    Assert.Equal(0UL, Value.toUInt64 (Value.Float32 5f))
    Assert.Equal(0UL, Value.toUInt64 (Value.Bool true))
    Assert.Equal(0UL, Value.toUInt64 Value.Null)

[<Fact>]
let ``a signed integer follows the same rules`` () =
    Assert.Equal(-7L, Value.toInt64 (Value.Int -7L))
    Assert.Equal(Int64.MaxValue, Value.toInt64 (Value.UInt(uint64 Int64.MaxValue)))
    Assert.Equal(0L, Value.toInt64 (Value.UInt UInt64.MaxValue))
    Assert.Equal(1654698415668011500L, Value.toInt64 (Value.Number "1654698415668011500"))
    Assert.Equal(0L, Value.toInt64 (Value.Number "1e3"))
    Assert.Equal(0L, Value.toInt64 (Value.Number "9223372036854775808"))
    Assert.Equal(-5L, Value.toInt64 (Value.String "-5"))
    Assert.Equal(5L, Value.toInt64 (Value.String "+5"))
    Assert.Equal(0L, Value.toInt64 (Value.String "5 "))
    Assert.Equal(-120L, Value.toInt64 (Value.Float -120.0))
    Assert.Equal(0L, Value.toInt64 (Value.Float 0.5))
    Assert.Equal(0L, Value.toInt64 (Value.Float -9007199254740992.0))
    Assert.Equal(0L, Value.toInt64 (Value.Bytes [| 1uy |]))
    Assert.Equal<int64 option>(None, Value.tryInt64 Value.Null)
    Assert.Equal(Some 0L, Value.tryInt64 (Value.Number "0"))
    Assert.Equal(Some 0L, Value.tryInt64 (Value.String "not a number"))

[<Fact>]
let ``a metric is read from every numeric form`` () =
    Assert.Equal(1.5, Value.toFloat (Value.Float 1.5))
    Assert.Equal(1.5, Value.toFloat (Value.Float32 1.5f))
    Assert.Equal(-3.0, Value.toFloat (Value.Int -3L))
    Assert.Equal(3.0, Value.toFloat (Value.UInt 3UL))
    Assert.Equal(42.5, Value.toFloat (Value.Number "42.5"))
    Assert.Equal(infinity, Value.toFloat (Value.Number "1e999"))
    Assert.Equal(3.0, Value.toFloat (Value.String "3"))
    Assert.Equal(0.0, Value.toFloat (Value.String " 3"))
    Assert.Equal(0.0, Value.toFloat (Value.String "abc"))
    Assert.Equal(0.0, Value.toFloat (Value.Bool true))
    // The words Go's ParseFloat takes.
    Assert.Equal(infinity, Value.toFloat (Value.String "inf"))
    Assert.Equal(infinity, Value.toFloat (Value.String "+Inf"))
    Assert.Equal(-infinity, Value.toFloat (Value.String "-Infinity"))
    Assert.True(Double.IsNaN(Value.toFloat (Value.String "NaN")))

[<Fact>]
let ``a bare string counts as a list of one`` () =
    Assert.Equal<string[]>([| "linux"; "7" |], Value.strings (json """["linux", 7]"""))
    Assert.Equal<string[]>([| "linux" |], Value.strings (Value.String "linux"))
    Assert.Equal<string[]>([||], Value.strings (Value.String ""))
    Assert.Equal<string[]>([||], Value.strings (json "[]"))
    Assert.Equal<string[]>([||], Value.strings (Value.Number "7"))

[<Fact>]
let ``maps render each value and ignore what is not an object`` () =
    let object = json """{"a": "x", "b": 7, "c": null, "d": {"e": 1.50}}"""
    Assert.Equal<Map<string, string>>(Map [ "a", "x"; "b", "7"; "c", ""; "d", """{"e":1.50}""" ], Value.stringMap object)
    Assert.Equal<Map<string, float>>(Map [ "a", 0.0; "b", 7.0; "c", 0.0; "d", 0.0 ], Value.floatMap object)
    Assert.Equal<Map<string, string>>(Map.empty, Value.stringMap (json "[1]"))
    Assert.Equal<Map<string, float>>(Map.empty, Value.floatMap Value.Null)
    Assert.True(Value.has "c" object)
    Assert.False(Value.has "z" object)
    Assert.Equal(Value.Null, Value.field "a" (Value.String "not an object"))

[<Fact>]
let ``toJson keeps every digit and says nothing for null`` () =
    Assert.Equal("", Value.toJson Value.Null)

    let msgpack =
        Value.Object(
            Map [ "id", Value.UInt UInt64.MaxValue
                  "n", Value.Int -1L
                  "bitmap", Value.Bytes [| 1uy; 2uy; 3uy |]
                  "half", Value.Float 0.5
                  "none", Value.Null ]
        )

    Assert.Equal("""{"bitmap":"AQID","half":0.5,"id":18446744073709551615,"n":-1,"none":null}""", Value.toJson msgpack)
    Assert.Equal("""{"id":9007199254740993,"price":1.50}""", Value.toJson (json """{"id": 9007199254740993, "price": 1.50}"""))
    Assert.Equal("""{"Data":"AQ==","Type":5}""", Value.toJson (Value.Ext(5y, [| 1uy |])))

[<Fact>]
let ``a value JSON cannot express gives no text at all`` () =
    Assert.Equal("", Value.toJson (Value.Object(Map [ "ok", Value.Float 1.0; "bad", Value.Float nan ])))

[<Fact>]
let ``unknown keeps the keys nothing names, as JSON`` () =
    let found = Value.unknown (set [ "known" ]) (json """{"known": 1, "new": {"nested": true}, "text": "t", "nothing": null}""")
    Assert.Equal<Map<string, string>>(Map [ "new", """{"nested":true}"""; "text", "\"t\""; "nothing", "" ], found)
    Assert.Equal<Map<string, string>>(Map.empty, Value.unknown Set.empty (Value.String "x"))

[<Fact>]
let ``a repeated key keeps its last value in both formats`` () =
    Assert.Equal("2", Value.fieldText "a" (json """{"a": 1, "a": 2}"""))
    // {"a": 1, "a": 2}
    let body = [| 0x82uy; 0xa1uy; byte 'a'; 0x01uy; 0xa1uy; byte 'a'; 0x02uy |]

    match decodeAny "application/msgpack" body with
    | "msgpack", Ok value -> Assert.Equal("2", Value.fieldText "a" value)
    | other -> Assert.Fail $"decoded {other}"

[<Fact>]
let ``Content-Type picks the decoder, and the first byte when it says nothing`` () =
    let jsonBody = utf8 """{"a":1}"""
    let msgpackBody = [| 0x81uy; 0xa1uy; byte 'a'; 0x01uy |]

    Assert.Equal("json", fst (decodeAny "application/json" jsonBody))
    Assert.Equal("msgpack", fst (decodeAny "application/msgpack" msgpackBody))
    Assert.Equal("json", fst (decodeAny "" jsonBody))
    Assert.Equal("json", fst (decodeAny "text/plain" (utf8 "  \n[1]")))
    Assert.Equal("msgpack", fst (decodeAny "" msgpackBody))

    // A JSON body labelled msgpack is read as msgpack: '{' is the integer 123.
    Assert.Equal(("msgpack", Ok(Value.UInt 123UL)), decodeAny "application/msgpack" jsonBody)

[<Fact>]
let ``what follows the first JSON value is not looked at`` () =
    match decodeAny "application/json" (utf8 """{"a":1} trailing""") with
    | "json", Ok value -> Assert.Equal("1", Value.fieldText "a" value)
    | other -> Assert.Fail $"decoded {other}"

    Assert.True(Result.isError (snd (decodeAny "application/json" (utf8 "  "))))
    Assert.True(Result.isError (snd (decodeAny "application/json" (utf8 "[1,2"))))

[<Fact>]
let ``JSON that Go took and .NET refuses still decodes`` () =
    // Half a surrogate pair written as an escape, and a byte that is not
    // UTF-8: Go read both as U+FFFD.
    let body =
        Array.concat
            [ utf8 """{"half":"x\ud83dy","low":"\udc00","pair":"\ud83d\ude00","slash":"\\ud83d","k\ud800":1,"byte":"z"""
              [| 0xffuy |]
              utf8 "\"}" ]

    match decodeAny "application/json" body with
    | _, Error e -> Assert.Fail e
    | _, Ok value ->
        Assert.Equal("x\uFFFDy", Value.fieldText "half" value)
        Assert.Equal("\uFFFD", Value.fieldText "low" value)
        Assert.Equal("\U0001F600", Value.fieldText "pair" value)
        Assert.Equal("\\ud83d", Value.fieldText "slash" value)
        Assert.Equal("1", Value.fieldText "k\uFFFD" value)
        Assert.Equal("z\uFFFD", Value.fieldText "byte" value)

    // Nested deeper than .NET's default limit of 64.
    let deep = utf8 ("{\"deep\":" + String('[', 80) + String(']', 80) + "}")
    Assert.True(Result.isOk (snd (decodeAny "application/json" deep)))

[<Fact>]
let ``a coverages array header longer than the document does not decode`` () =
    // {"version": 2, "coverages": <array32 announcing 2^32-2 elements>}
    let forged =
        Array.concat
            [ [| 0x82uy; 0xa7uy |]
              utf8 "version"
              [| 0x02uy; 0xa9uy |]
              utf8 "coverages"
              [| 0xdduy; 0xffuy; 0xffuy; 0xffuy; 0xfeuy |] ]

    Assert.True(Result.isError (coverageEntriesMsgpack forged))

[<Fact>]
let ``each coverage entry keeps its own bytes`` () =
    let first = Array.concat [ [| 0x81uy; 0xa7uy |]; utf8 "span_id"; [| 0x07uy |] ]
    let second = Array.concat [ [| 0x81uy; 0xaduy |]; utf8 "test_suite_id"; [| 0x09uy |] ]

    // {"extra": [1, 2], "version": "2", "coverages": [first, second]}
    let body =
        Array.concat
            [ [| 0x83uy; 0xa5uy |]
              utf8 "extra"
              [| 0x92uy; 0x01uy; 0x02uy; 0xa7uy |]
              utf8 "version"
              [| 0xa1uy; byte '2'; 0xa9uy |]
              utf8 "coverages"
              [| 0x92uy |]
              first
              second ]

    match coverageEntriesMsgpack body with
    | Error e -> Assert.Fail e
    | Ok(version, entries) ->
        // An envelope key nobody knows is skipped, and a version that changed
        // type still reads.
        Assert.Equal(2, version)
        Assert.Equal(2, entries.Length)
        Assert.Equal<byte[]>(first, entries[0].Raw)
        Assert.Equal<byte[]>(second, entries[1].Raw)
        Assert.Equal(7UL, Value.toUInt64 (Value.field "span_id" entries[0].Value))

[<Fact>]
let ``a coverage envelope whose key is not a string does not decode`` () =
    // {1: 2}
    match coverageEntriesMsgpack [| 0x81uy; 0x01uy; 0x02uy |] with
    | Error e -> Assert.StartsWith("envelope key 0: ", e)
    | Ok _ -> Assert.Fail "decoded"

[<Fact>]
let ``a truncated coverage upload is kept raw, not stored in part`` () =
    let contentType, body =
        multipart
            [ { Name = "event"; FileName = ""; ContentType = "application/json"; Data = utf8 """{"dummy": true}""" }
              { Name = "coveragex"; FileName = ""; ContentType = "application/json"; Data = utf8 """{"version":2,"coverages":[{"test_suite_id":9}]}""" } ]

    // Cut inside the closing boundary: both parts are whole, the body is not.
    let cut = body[.. body.Length - 8]
    let response, sink = send "routeCITestCov" "POST" "/api/v2/citestcov" [ "Content-Type", contentType ] cut

    Assert.Equal(202, response.Status)
    Assert.Empty(sink.Rows<CICoverageRow>())
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("citestcov", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.StartsWith("multipart, content-type multipart/form-data; boundary=frontier: ", raw.Note)

[<Fact>]
let ``a JSON coverage payload of the wrong shape is kept raw`` () =
    for payload in [ """[1,2]"""; """{"version":2,"coverages":{"not":"an array"}}"""; """{"version":"two","coverages":[]}""" ] do
        let contentType, body =
            multipart [ { Name = "coverage1"; FileName = ""; ContentType = "application/json"; Data = utf8 payload } ]

        let response, sink = send "routeCITestCov" "POST" "/api/v2/citestcov" [ "Content-Type", contentType ] body
        Assert.Equal(202, response.Status)
        Assert.Empty(sink.Rows<CICoverageRow>())
        Assert.Equal("decode_error", (Assert.Single(sink.Rows<RawPayloadRow>())).Reason)

[<Fact>]
let ``a JSON coverage version past int64 is the nearest limit, as Go's ParseInt gave`` () =
    let contentType, body =
        multipart
            [ { Name = "c"; FileName = ""; ContentType = "application/json"; Data = utf8 """{"version":99999999999999999999,"coverages":[{}]}""" } ]

    let _, sink = send "routeCITestCov" "POST" "/api/v2/citestcov" [ "Content-Type", contentType ] body
    // int32 of Int64.MaxValue.
    Assert.Equal(-1, (Assert.Single(sink.Rows<CICoverageRow>())).PayloadVersion)

[<Fact>]
let ``a JSON coverage entry keeps the base64 text of its bitmap and its unknown keys`` () =
    let contentType, body =
        multipart
            [ { Name = "coverage1"
                FileName = ""
                ContentType = ""
                Data = utf8 """{"version":"2","coverages":[{"test_session_id":1,"span_id":0,"files":[{"filename":"a.go","bitmap":"AQID"}],"later":[1]}]}""" } ]

    let _, sink = send "routeCITestCov" "POST" "/api/v2/citestcov" [ "Content-Type", contentType ] body
    let row = Assert.Single(sink.Rows<CICoverageRow>())
    Assert.Equal(2, row.PayloadVersion)
    Assert.Equal("json", row.RawFormat)
    // Zero is a legal span id, and it was sent.
    Assert.Equal(Some 0UL, row.SpanID)
    Assert.Equal<byte[][]>([| utf8 "AQID" |], row.FilesBitmap)
    Assert.Equal<Map<string, string>>(Map [ "later", "[1]" ], row.Extra)
    Assert.Empty row.Event

[<Fact>]
let ``an empty test-cycle body is answered and nothing is stored`` () =
    let response, sink = send "routeCITestCycle" "POST" "/api/v2/citestcycle" [ "Content-Type", "application/msgpack" ] [||]
    Assert.Equal(202, response.Status)
    Assert.Empty sink.Writes

[<Fact>]
let ``events that are not maps keep the body raw once, the others become rows`` () =
    let body = utf8 """{"version":3,"events":[5,{"type":"test","version":2,"content":{"service":"s","meta":{"test.is_new":" YES "}}},"x"]}"""
    let response, sink = send "routeCITestCycle" "POST" "/api/v2/citestcycle" [ "Content-Type", "application/json" ] body

    Assert.Equal(202, response.Status)
    Assert.Equal<string list>([ "storage_raw_payloads"; "storage_ci_test_events" ], sink.Writes |> List.map _.Writer)
    Assert.Equal("2 of 3 events are not maps: 0, 2", (Assert.Single(sink.Rows<RawPayloadRow>())).Note)

    let row = Assert.Single(sink.Rows<CITestEventRow>())
    Assert.Equal(3, row.PayloadVersion)
    Assert.Equal(2, row.EventVersion)
    Assert.Equal("s", row.Service)
    Assert.Equal(Some 1uy, row.TestIsNew)
    Assert.Equal<uint8 option>(None, row.TestIsRetry)

[<Fact>]
let ``a source line is taken from metrics first, then from meta`` () =
    let event (content: string) =
        json ("""{"type":"test","content":""" + content + "}")

    let headers: AgentHeaders = { Subdomain = ""; ContainerID = ""; Hostname = ""; AgentVersion = "" }
    let row (content: string) = testEventRow "t" DateTime.UnixEpoch headers 1 Map.empty (event content)

    let both = row """{"meta":{"test.source.start":"7"},"metrics":{"test.source.start":120.9}}"""
    Assert.Equal(Some 120L, both.TestSourceStart)
    Assert.Equal<int64 option>(None, both.TestSourceEnd)

    let metaOnly = row """{"meta":{"test.source.start":" 7 ","test.source.end":"seven"}}"""
    Assert.Equal(Some 7L, metaOnly.TestSourceStart)
    Assert.Equal<int64 option>(None, metaOnly.TestSourceEnd)

[<Fact>]
let ``the agent's own headers win over the fallbacks`` () =
    let body = utf8 """{"events":[{"type":"test","content":{}}]}"""

    let _, sink =
        send
            "routeCITestCycle"
            "POST"
            "/api/v2/citestcycle"
            [ "Content-Type", "application/json"
              "X-Datadog-Container-Id", "invented"
              "X-Datadog-AgentVersion", "7.90.0"
              "Via", "trace-agent 7.83.0" ]
            body

    let row = Assert.Single(sink.Rows<CITestEventRow>())
    Assert.Equal("invented", row.ContainerID)
    Assert.Equal("7.90.0", row.AgentVersion)

    let _, viaOther = send "routeCITestCycle" "POST" "/api/v2/citestcycle" [ "Content-Type", "application/json"; "Via", "1.1 varnish" ] body
    Assert.Equal("", (Assert.Single(viaOther.Rows<CITestEventRow>())).AgentVersion)

[<Fact>]
let ``a question keeps what decoded when a field has the wrong type`` () =
    let request, problem =
        readConfigRequest (utf8 """{"data":{"id":5,"type":"x","attributes":{"service":"s"}}}""")

    Assert.Equal("", request.ID)
    Assert.Equal("x", request.Type)
    // The attributes are not read under a broken envelope.
    Assert.Equal("", request.Service)
    Assert.Equal(Some "envelope", problem |> Option.map fst)

    let request, problem =
        readConfigRequest (
            utf8 """{"data":{"id":"a","attributes":{"service":5,"env":"ci","configurations":{"a": 1},"page_info":{"page_state":"p"}}}}"""
        )

    Assert.Equal("a", request.ID)
    Assert.Equal("", request.Service)
    Assert.Equal("ci", request.Env)
    Assert.Equal("""{"a": 1}""", request.Configurations)
    Assert.Equal("p", request.PageState)
    Assert.Equal(Some "attributes", problem |> Option.map fst)

[<Fact>]
let ``a question is read as Go read it`` () =
    // Names match whatever their case.
    let request, problem = readConfigRequest (utf8 """{"DATA":{"ID":"upper","Attributes":{"Service":"s"}}}""")
    Assert.Equal("upper", request.ID)
    Assert.Equal("s", request.Service)
    Assert.True problem.IsNone

    // null is no document, and no error.
    let request, problem = readConfigRequest (utf8 "null")
    Assert.Equal("", request.ID)
    Assert.True problem.IsNone

    let request, problem = readConfigRequest (utf8 """{"data":{"id":null,"attributes":{"configurations":null,"page_info":null}}}""")
    Assert.Equal("", request.ID)
    Assert.Equal("null", request.Configurations)
    Assert.True problem.IsNone

    for body in [ "[1]"; "\"text\""; """{"data":[1,2]}"""; """{"data":{"id":"x"}} trailing""" ] do
        let request, problem = readConfigRequest (utf8 body)
        Assert.Equal("", request.ID)
        Assert.Equal(Some "envelope", problem |> Option.map fst)

    let _, problem = readConfigRequest (utf8 """{"data":{"attributes":[1]}}""")
    Assert.Equal(Some "attributes", problem |> Option.map fst)

[<Fact>]
let ``a repeated key is applied in order, as Go applied it`` () =
    // The wrong type is an error, and leaves what the earlier one set.
    let request, problem = readConfigRequest (utf8 """{"data":{"id":"a","id":5}}""")
    Assert.Equal("a", request.ID)
    Assert.Equal(Some "envelope", problem |> Option.map fst)

    let request, problem = readConfigRequest (utf8 """{"data":{"id":"a","id":null}}""")
    Assert.Equal("a", request.ID)
    Assert.True problem.IsNone

    // A repeated object adds to the earlier one.
    let request, problem = readConfigRequest (utf8 """{"data":{"id":"a"},"data":{"type":"t"}}""")
    Assert.Equal("a", request.ID)
    Assert.Equal("t", request.Type)
    Assert.True problem.IsNone

    let request, problem = readConfigRequest (utf8 """{"data":{"attributes":{"service":"s","Service":null,"SERVICE":7}}}""")
    Assert.Equal("s", request.Service)
    Assert.Equal(Some "attributes", problem |> Option.map fst)

[<Fact>]
let ``an id that needs escaping is echoed as valid JSON`` () =
    let body = utf8 """{"data":{"id":"a\"b\\c"}}"""
    let response, sink = send "routeCIVisibilityAPI" "POST" "/api/v2/ci/libraries/tests" [] body
    let answer = System.Text.Json.JsonDocument.Parse(ReadOnlyMemory response.Body)
    Assert.Equal("a\"b\\c", answer.RootElement.GetProperty("data").GetProperty("id").GetString())
    Assert.Equal(Encoding.UTF8.GetString response.Body, (Assert.Single(sink.Rows<CISettingsRequestRow>())).ResponseBody)

[<Fact>]
let ``a commit search that does not decode still gets an empty JSON answer`` () =
    for body in [ """{"data":"""; """{"data":{"id":"a"}}"""; """{"data":[5]}"""; """{"data":[{"id":7}]}"""; """{"meta":[]}""" ] do
        let response, sink = send "routeCIVisibilityAPI" "POST" "/api/v2/git/repository/search_commits" [] (utf8 body)
        Assert.Equal(200, response.Status)
        Assert.Contains(("Content-Type", "application/json; charset=utf-8"), response.Headers)
        Assert.Equal("""{"data":[],"meta":{"repository_url":""}}""", Encoding.UTF8.GetString response.Body)
        Assert.Empty(sink.Rows<GitCommitRow>())
        Assert.Equal("gitmeta", (Assert.Single(sink.Rows<RawPayloadRow>())).Intake)

[<Fact>]
let ``a commit without an id is not stored`` () =
    let body = utf8 """{"data":[{"id":"aaa"},null,{"type":"commit"},{"id":""}],"meta":{"repository_url":"r"}}"""
    let response, sink = send "routeCIVisibilityAPI" "POST" "/api/v2/git/repository/search_commits" [] body
    Assert.Equal(200, response.Status)
    Assert.Equal("aaa", (Assert.Single(sink.Rows<GitCommitRow>())).SHA)
    Assert.Empty(sink.Rows<RawPayloadRow>())

[<Fact>]
let ``a packfile upload that is not multipart is kept raw and still answered 204`` () =
    let response, sink =
        send "routeCIVisibilityAPI" "POST" "/api/v2/git/repository/packfile" [ "Content-Type", "application/octet-stream" ] (utf8 "PACK")

    Assert.Equal(204, response.Status)
    Assert.Empty response.Body
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("gitmeta", raw.Intake)
    Assert.Equal("packfile multipart, content-type application/octet-stream: content-type application/octet-stream is not multipart", raw.Note)
    Assert.Empty(sink.Rows<GitPackfileRow>())

[<Fact>]
let ``a packfile upload without a packfile part is kept raw and stores no pack`` () =
    let contentType, body =
        multipart [ { Name = "pushedSha"; FileName = ""; ContentType = "application/json"; Data = utf8 """{"data":{"id":"abc"}}""" } ]

    let response, sink = send "routeCIVisibilityAPI" "POST" "/api/v2/git/repository/packfile" [ "Content-Type", contentType ] body
    Assert.Equal(204, response.Status)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal(("unexpected_shape", "packfile upload without a packfile part"), (raw.Reason, raw.Note))
    Assert.Empty(sink.Rows<GitPackfileRow>())
    Assert.Empty(sink.Rows<GitCommitRow>())

[<Fact>]
let ``a pushedSha part that does not decode is kept beside the pack`` () =
    let pushedSha = utf8 """{"data":{"id":"abc"},"meta":5}"""

    let contentType, body =
        multipart
            [ { Name = "pushedSha"; FileName = ""; ContentType = "application/json"; Data = pushedSha }
              { Name = "packfile"; FileName = "objects/pack-1.pack"; ContentType = "application/octet-stream"; Data = [| 0xffuy; 0x00uy |] }
              { Name = "note"; FileName = ""; ContentType = ""; Data = utf8 "hello" } ]

    let response, sink = send "routeCIVisibilityAPI" "POST" "/api/v2/git/repository/packfile" [ "Content-Type", contentType ] body
    Assert.Equal(204, response.Status)

    let pack = Assert.Single(sink.Rows<GitPackfileRow>())
    // What did decode is kept: the sha was readable, the repository was not.
    Assert.Equal("abc", pack.PushedSHA)
    Assert.Equal("", pack.RepositoryURL)
    Assert.Equal("pack-1.pack", pack.Filename)
    Assert.Equal<byte[]>([| 0xffuy; 0x00uy |], pack.Packfile)
    Assert.Equal<Map<string, byte[]>>(Map [ "pushedSha.undecodable", pushedSha; "note", utf8 "hello" ], pack.OtherParts)

    let commit = Assert.Single(sink.Rows<GitCommitRow>())
    Assert.Equal(Some pack.PackfileID, commit.PackfileID)

[<Fact>]
let ``a repeated pushedSha part changes only what it sets`` () =
    let contentType, body =
        multipart
            [ { Name = "pushedSha"; FileName = ""; ContentType = "application/json"; Data = utf8 """{"data":{"id":"one"},"meta":{"repository_url":"r1"}}""" }
              { Name = "pushedSha"; FileName = ""; ContentType = "application/json"; Data = utf8 """{"data":{"type":"commit"}}""" }
              { Name = "packfile"; FileName = "p.pack"; ContentType = "application/octet-stream"; Data = utf8 "PACK" } ]

    let _, sink = send "routeCIVisibilityAPI" "POST" "/api/v2/git/repository/packfile" [ "Content-Type", contentType ] body
    let pack = Assert.Single(sink.Rows<GitPackfileRow>())
    Assert.Equal("one", pack.PushedSHA)
    Assert.Equal("r1", pack.RepositoryURL)
    Assert.Empty pack.OtherParts
    Assert.Equal("one", (Assert.Single(sink.Rows<GitCommitRow>())).SHA)

[<Fact>]
let ``measures fill the metrics column only when metrics are absent`` () =
    let body = utf8 """{"data":{"type":"t","attributes":{"metrics":{"a":1},"measures":{"b":2},"ci_level":"2"}}}"""
    let _, sink = send "routeCIVisibilityAPI" "POST" "/api/intake/ci/custom_spans" [] body
    let row = Assert.Single(sink.Rows<CIPipelineEventRow>())
    Assert.Equal<Map<string, float>>(Map [ "a", 1.0 ], row.Metrics)
    Assert.Equal(Some 2L, row.CILevel)
    // "measures" is a named key, so it is not repeated in extra.
    Assert.Empty row.Extra

[<Fact>]
let ``a datadog-ci call whose attributes are not an object is still a row`` () =
    for body in [ """{"data":{"type":"t","attributes":[1,2]}}"""; "null"; """{"data":null}""" ] do
        let response, sink = send "routeCIVisibilityAPI" "POST" "/api/v2/ci/pipeline/tags" [] (utf8 body)
        Assert.Equal(202, response.Status)
        let row = Assert.Single(sink.Rows<CIPipelineEventRow>())
        Assert.Equal("tag", row.Kind)
        Assert.Equal<byte[]>(utf8 body, row.Body)
        Assert.Equal<int64 option>(None, row.CILevel)
        Assert.Empty row.Tags

[<Fact>]
let ``a datadog-ci envelope of the wrong shape is kept raw`` () =
    for body in [ "[1]"; """{"data":5}"""; """{"data":{"type":7}}""" ] do
        let response, sink = send "routeCIVisibilityAPI" "POST" "/api/v2/ci/pipeline/metrics" [] (utf8 body)
        Assert.Equal(202, response.Status)
        Assert.Empty(sink.Rows<CIPipelineEventRow>())
        Assert.Equal("cipipeline", (Assert.Single(sink.Rows<RawPayloadRow>())).Intake)
