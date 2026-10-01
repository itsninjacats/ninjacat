/// Tests of Routers/Config.fs. The Go server had no tests for this router,
/// so no golden fixture covers it.
module NinjaCat.Api.Intake.Tests.ConfigTests

open System.Text
open Google.Protobuf
open Xunit
open Datadog.Config
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Tests.Golden

let private send (method: string) (path: string) (body: byte[]) : Response * CapturingSink =
    AppTests.send [ "routeConfig" ] method path [ "Dd-Api-Key", Replay.testKey ] body

let private assertNotServed (expectedBody: string) (response: Response, sink: CapturingSink) =
    Assert.Equal(404, response.Status)
    Assert.Equal(expectedBody, Encoding.UTF8.GetString response.Body)
    Assert.Empty sink.Writes

let private notServed = """{"error":"remote configuration not served"}"""

[<Fact>]
let ``a configuration poll is answered 404 so the agent keeps the config it has`` () =
    let request =
        LatestConfigsRequest(Hostname = "web-01", AgentVersion = "7.60.0", HasError = true, Error = "apply failed")

    request.Products.Add "APM_SAMPLING"
    request.ActiveClients.Add(Client(Id = "client-1", IsAgent = true))

    send "POST" "/api/v0.1/configurations" (request.ToByteArray()) |> assertNotServed notServed

[<Fact>]
let ``a poll that is not protobuf, or decodes to nothing, gets the same 404`` () =
    send "POST" "/api/v0.1/configurations" [| 0xFFuy; 0xFFuy; 0xFFuy; 0xFFuy |] |> assertNotServed notServed
    send "POST" "/api/v0.1/configurations" [||] |> assertNotServed notServed

[<Theory>]
[<InlineData("GET", "/api/v0.1/org")>]
[<InlineData("POST", "/api/v0.1/org")>]
[<InlineData("GET", "/api/v0.1/status")>]
[<InlineData("POST", "/api/v0.1/status")>]
let ``the org and status lookups are answered 404`` (method: string, path: string) =
    send method path [||] |> assertNotServed """{"error":"not served"}"""

[<Fact>]
let ``remote configuration needs an API key`` () =
    let response, _ = AppTests.send [ "routeConfig" ] "GET" "/api/v0.1/org" [] [||]
    Assert.Equal(403, response.Status)
