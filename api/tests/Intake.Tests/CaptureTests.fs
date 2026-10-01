module NinjaCat.Api.Intake.Tests.CaptureTests

open System
open System.IO
open System.Text
open Microsoft.AspNetCore.Http
open Xunit
open NinjaCat.Api.Intake

[<Fact>]
let ``JSON is pretty-printed, text is left alone, nothing says so`` () =
    Assert.Equal("(empty body)", Capture.render [||])
    Assert.Contains("\"a\": 1", Capture.render (Encoding.UTF8.GetBytes """{"a":1}"""))
    Assert.Equal("plain text, not JSON", Capture.render (Encoding.UTF8.GetBytes "plain text, not JSON"))

[<Fact>]
let ``protobuf is walked without a schema`` () =
    // field 1 = "hi", field 2 = 150, field 3 = { field 1 = 1.5 as a double }
    let message =
        [| 0x0auy; 2uy; byte 'h'; byte 'i'; 0x10uy; 0x96uy; 0x01uy; 0x1auy; 9uy; 0x09uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0xf8uy; 0x3fuy |]

    match Capture.dumpProto message with
    | None -> Assert.Fail "not recognised as protobuf"
    | Some dump ->
        Assert.Contains("1: \"hi\"", dump)
        Assert.Contains("2: 150", dump)
        Assert.Contains("3: {", dump)
        Assert.Contains("(f64 1.5)", dump)

    Assert.StartsWith("(protobuf", Capture.render message)

[<Fact>]
let ``bytes that are nothing else become a hexdump`` () =
    let dump = Capture.render [| 0xffuy; 0xfeuy; 0x00uy; byte 'A' |]
    Assert.StartsWith("00000000  ff fe 00 41 ", dump)
    Assert.EndsWith("|...A|\n", dump)
    Assert.Contains("(cut at 4096 B)", Capture.hexDump (Array.create 5000 0xffuy))

[<Fact>]
let ``a path becomes a file name`` () =
    Assert.Equal("api_v2_series", Capture.slug "/api/v2/series")
    Assert.Equal("root", Capture.slug "/")
    Assert.Equal("v1_input_abc_def", Capture.slug "/v1/input/abc-def")

[<Fact>]
let ``a capture is a file and an index line, with keys left out`` () =
    let directory = Path.Combine(Path.GetTempPath(), "ninjacat-capture-" + Guid.NewGuid().ToString "N")

    try
        let http = DefaultHttpContext()
        http.Request.Method <- "POST"
        http.Request.Host <- HostString "api.ninjacat.local"
        http.Request.Path <- PathString "/api/v2/series"
        http.Request.Headers["Dd-Api-Key"] <- "0123456789abcdef0123456789abcdef"
        http.Request.Headers["User-Agent"] <- "datadog-agent/7.60.0"
        let body = Encoding.UTF8.GetBytes """{"series":[]}"""

        let file = Capture.write directory http body body 202 DateTime.UtcNow
        let text = File.ReadAllText file
        Assert.Contains("POST /api/v2/series", text)
        Assert.Contains("# status:   202", text)
        Assert.Contains("User-Agent: datadog-agent/7.60.0", text)
        Assert.Contains("\"series\": []", text)
        Assert.DoesNotContain("0123456789abcdef", text)
        Assert.Contains("Dd-Api-Key: <redacted>", text)

        let index = File.ReadAllText(Path.Combine(directory, "index.log"))
        Assert.Contains("\tPOST\t/api/v2/series\t202\t13\t13\t-\t" + Path.GetFileName file, index)
    finally
        if Directory.Exists directory then Directory.Delete(directory, true)
