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
let ``the note of an aiusage payload quotes the proxy's origin headers as Go's %q does`` () =
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
[<InlineData("tab\there\n", "\"tab\\there\\n\"")>]
[<InlineData("bell\u0007 esc\u001b del\u007f", "\"bell\\a esc\\x1b del\\x7f\"")>]
[<InlineData("zażółć — ✓", "\"zażółć — ✓\"")>]
[<InlineData("nbsp  zwsp​", "\"nbsp\\u00a0 zwsp\\u200b\"")>]
[<InlineData("😀", "\"😀\"")>]
[<InlineData("\u0085 \u2028 \ue000 \U000e0001 \u0378", "\"\\u0085 \\u2028 \\ue000 \\U000e0001 \\u0378\"")>]
let ``goQuote writes what Go's strconv.Quote writes`` (text: string, expected: string) =
    Assert.Equal(expected, Install.goQuote text)
