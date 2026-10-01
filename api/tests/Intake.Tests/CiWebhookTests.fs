/// Tests of Routers/CiWebhook.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.CiWebhookTests

open System
open Xunit
open NinjaCat.Api.Intake.Tests.CiVisibilityTests
open NinjaCat.Api.Storage.Rows

let private post (url: string) (headers: (string * string) list) (body: string) =
    send "routeCIWebhook" "POST" url (("Content-Type", "application/json") :: headers) (utf8 body)

[<Fact>]
let ``a bare null is an empty batch`` () =
    let response, sink = post "/api/v2/webhook" [] "null"
    Assert.Equal(202, response.Status)
    Assert.Empty sink.Writes

[<Fact>]
let ``an empty body is answered and nothing is stored`` () =
    let response, sink = post "/api/v2/webhook/" [] ""
    Assert.Equal(202, response.Status)
    Assert.Empty sink.Writes

[<Fact>]
let ``an element that is not an object goes raw, the others become rows`` () =
    let response, sink = post "/api/v2/webhook" [] """[null, 7, {"level":"job","name":"x"}]"""
    Assert.Equal(202, response.Status)

    let raw = sink.Rows<RawPayloadRow>()
    Assert.Equal<string list>([ "null"; "7" ], raw |> List.map (fun row -> Text.Encoding.UTF8.GetString row.Body))

    for row in raw do
        Assert.Equal("webhook", row.Intake)
        Assert.Equal("unexpected_shape", row.Reason)
        Assert.Equal("element is not a JSON object", row.Note)

    Assert.Equal("job", (Assert.Single(sink.Rows<CIWebhookEventRow>())).Level)
    // Raw elements are written as they are met, the rows after the loop.
    Assert.Equal("storage_ci_webhook_events", (List.last sink.Writes).Writer)

[<Fact>]
let ``a bare string or number is a batch of one element that is not an object`` () =
    let _, sink = post "/api/v2/webhook" [] "\"pipeline\""
    Assert.Empty(sink.Rows<CIWebhookEventRow>())
    Assert.Equal("unexpected_shape", (Assert.Single(sink.Rows<RawPayloadRow>())).Reason)

[<Fact>]
let ``the element's own provider wins over the header, the header over the shape`` () =
    let headers = [ "DD-CI-PROVIDER-NAME", "  Jenkins " ]

    let _, sink = post "/api/v2/webhook" headers """[{"provider":"GitLab","object_kind":"build"},{"object_kind":"build"}]"""
    let rows = sink.Rows<CIWebhookEventRow>()
    Assert.Equal<string list>([ "gitlab"; "jenkins" ], rows |> List.map _.Provider)
    // No "level", so GitLab's object_kind is the level.
    Assert.Equal<string list>([ "build"; "build" ], rows |> List.map _.Level)

    let _, sink = post "/api/v2/webhook" [] """[{"object_kind":null},{"provider":""}]"""
    Assert.Equal<string list>([ "gitlab"; "unknown" ], sink.Rows<CIWebhookEventRow>() |> List.map _.Provider)

[<Fact>]
let ``a numeric id keeps every digit, and absent is not false`` () =
    let body =
        """{"level":"pipeline","trace_id":5678901234567890123,"span_id":1.50,"payload_version":"2",
            "queue_time":1500,"partial_retry":"false","is_manual":false,"unique_id":true}"""

    let _, sink = post "/api/v2/webhook" [] body
    let row = Assert.Single(sink.Rows<CIWebhookEventRow>())
    Assert.Equal("5678901234567890123", row.TraceID)
    Assert.Equal("1.50", row.SpanID)
    Assert.Equal("true", row.UniqueID)
    Assert.Equal(Some 2L, row.PayloadVersion)
    Assert.Equal(Some 1500L, row.QueueTimeMs)
    // A string is not a bool: the provider did not say.
    Assert.Equal<uint8 option>(None, row.PartialRetry)
    Assert.Equal(Some 0uy, row.IsManual)
    Assert.Equal(body, row.Body)

[<Fact>]
let ``a time that is not RFC 3339 keeps its text and has no parse`` () =
    let _, sink = post "/api/v2/webhook" [] """{"start":"2026-09-23 10:00:00","end":"2026-09-23T10:06:30.250+02:00"}"""
    let row = Assert.Single(sink.Rows<CIWebhookEventRow>())
    Assert.Equal("2026-09-23 10:00:00", row.StartRaw)
    Assert.Equal<DateTime option>(None, row.StartParsed)
    Assert.Equal(Some(DateTime(2026, 9, 23, 8, 6, 30, 250, DateTimeKind.Utc)), row.EndParsed)

[<Fact>]
let ``a single label or tag sent unwrapped still counts`` () =
    let _, sink = post "/api/v2/webhook" [] """{"node":{"labels":"linux"},"tags":"team:ci","parameters":{"N":3,"deep":{"a":1}}}"""
    let row = Assert.Single(sink.Rows<CIWebhookEventRow>())
    Assert.Equal<string[]>([| "linux" |], row.NodeLabels)
    Assert.Equal<Map<string, string[]>>(Map [ "team", [| "ci" |] ], row.Tags)
    Assert.Equal<Map<string, string>>(Map [ "N", "3"; "deep", """{"a":1}""" ], row.Parameters)

[<Fact>]
let ``the delivery id falls back from GitLab to GitHub to Datadog's request id`` () =
    let element = """{"level":"job"}"""

    let delivery (headers: (string * string) list) =
        let _, sink = post "/api/v2/webhook" headers element
        (Assert.Single(sink.Rows<CIWebhookEventRow>())).DeliveryID

    Assert.Equal("", delivery [])
    Assert.Equal("dd", delivery [ "Dd-Request-Id", "dd" ])
    Assert.Equal("gh", delivery [ "Dd-Request-Id", "dd"; "X-GitHub-Delivery", "gh" ])
    Assert.Equal("gl", delivery [ "Dd-Request-Id", "dd"; "X-GitHub-Delivery", "gh"; "X-Gitlab-Event-UUID", "gl" ])

[<Fact>]
let ``the synthetics poller's answer does not depend on its query`` () =
    let response, sink = send "routeSyntheticsAgent" "GET" "/api/unstable/synthetics/agents/tests" [] [||]
    Assert.Equal(200, response.Status)
    Assert.Equal("""{"tests":[]}""", Text.Encoding.UTF8.GetString response.Body)
    Assert.Empty sink.Writes
