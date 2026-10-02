/// Tests of Routers/Install.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.InstallTests

open System.Text
open System.Text.Json
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Intake.Tests.Golden

let private header (name: string) (response: Response) : string list =
    response.Headers |> List.filter (fun (key, _) -> key = name) |> List.map snd

[<Fact>]
let ``an unhosted blob is the registry's own BLOB_UNKNOWN, with no API key asked`` () =
    let response, sink = AppTests.send [ "routeInstall" ] "GET" "/v2/datadog-agent/blobs/sha256:abc" [] [||]

    Assert.Equal(404, response.Status)
    Assert.Equal<string list>([ "registry/2.0" ], header "Docker-Distribution-Api-Version" response)

    use body = JsonDocument.Parse(response.Body)
    let error = Assert.Single(body.RootElement.GetProperty("errors").EnumerateArray())
    // go-containerregistry parses the code, so the exact string matters.
    Assert.Equal("BLOB_UNKNOWN", error.GetProperty("code").GetString())
    Assert.Equal("ninjacat does not host packages: datadog-agent@sha256:abc", error.GetProperty("message").GetString())
    Assert.Empty sink.Writes

[<Fact>]
let ``a host that asks for no key takes no upload and answers no key check`` () =
    // What every keyed host also serves (a flare, /api/v1/validate) is not
    // served here: it would be a body anyone could make the intake read.
    let flare, sink = AppTests.send [ "routeInstall" ] "POST" "/support/flare" [] (Encoding.UTF8.GetBytes "x")
    // 405, not 404: the path exists here only as the HEAD probe.
    Assert.Equal(405, flare.Status)
    Assert.Empty sink.Writes

    let validate, _ = AppTests.send [ "routeInstall" ] "GET" "/api/v1/validate" [] [||]
    Assert.Equal(404, validate.Status)

[<Fact>]
let ``a BTF archive is not hosted, so system-probe falls back to its next source`` () =
    let response, sink =
        AppTests.send [ "routeInstall" ] "GET" "/btfs/ubuntu/22.04/x86_64/5.15.0-91-generic.btf.tar.xz" [] [||]

    Assert.Equal(404, response.Status)
    Assert.Equal("""{"errors":["BTF archives are not hosted here"]}""", Encoding.UTF8.GetString response.Body)
    Assert.Empty(header "Docker-Distribution-Api-Version" response)
    Assert.Empty sink.Writes

[<Fact>]
let ``an llmobs body sent by the diagnose sweep is a probe, not a payload`` () =
    let response, sink =
        AppTests.send
            [ "routeLLMObs" ]
            "POST"
            "/api/v2/llmobs"
            [ "Dd-Api-Key", Replay.testKey; "X-Requested-With", "datadog-agent-diagnose" ]
            (Encoding.UTF8.GetBytes """{"span":{}}""")

    Assert.Equal(202, response.Status)
    Assert.Empty sink.Writes

[<Fact>]
let ``the note of an aiusage payload quotes the proxy's origin headers`` () =
    let _, sink =
        AppTests.send
            [ "routeAIUsage" ]
            "POST"
            "/api/v2/aiusage"
            [ "Dd-Api-Key", Replay.testKey
              "DD-EVP-ORIGIN", "ai \"prompt\" logger"
              "DD-EVP-ORIGIN-VERSION", "1.2.3"
              "Content-Type", "application/json" ]
            (Encoding.UTF8.GetBytes """{"prompt":"hi"}""")

    let row = Assert.Single(sink.Rows<RawPayloadRow>())

    Assert.Equal(
        "DD-EVP-ORIGIN=\"ai \\\"prompt\\\" logger\" DD-EVP-ORIGIN-VERSION=\"1.2.3\" Content-Type=\"application/json\"",
        row.Note
    )

[<Theory>]
[<InlineData("", "\"\"")>]
[<InlineData("plain text", "\"plain text\"")>]
[<InlineData("a\"b\\c", "\"a\\\"b\\\\c\"")>]
[<InlineData("zażółć — ✓", "\"zażółć — ✓\"")>]
let ``a header in a note is quoted as a JSON string`` (text: string, expected: string) =
    Assert.Equal(expected, Json.quoted text)
