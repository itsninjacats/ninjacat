/// config.<site> — remote configuration.
///
///   agent config: DD_REMOTE_CONFIGURATION_ENABLED (on/off only, no dd_url)
///
/// The only intake where data flows DOWN: the agent polls it and applies what
/// comes back — integration configs, sampling rates, and in Kubernetes the
/// actions the cluster agent then performs. The agent verifies the answer
/// against a TUF root, so nothing can be served before signing exists
/// (server/docs/zadania/remote-config-tuf.md). Until then 404 is the honest
/// answer: the agent keeps the configuration it has instead of acting on an
/// empty one.
///
///   POST /api/v0.1/configurations  LatestConfigsRequest → LatestConfigsResponse
///   GET  /api/v0.1/org             (no body)            → OrgDataResponse
///   GET  /api/v0.1/status          (no body)            → OrgStatusResponse
module NinjaCat.Api.Intake.Routers.Config

open Google.Protobuf
open Microsoft.Extensions.Logging
open Datadog.Config
open NinjaCat.Api.Intake

/// The poll for new configuration. The request is the agent's full picture
/// of itself: identity, the TUF versions it holds, the products it wants and
/// every client registered with it. Nothing of it is stored yet.
let private handleConfigurations (r: Request) : Response =
    try
        let request = LatestConfigsRequest.Parser.ParseFrom r.Body

        if request.Hostname = "" && request.Products.Count = 0 && request.ActiveClients.Count = 0 then
            // A protobuf parser fails OPEN: a payload meant for another
            // endpoint decodes into an empty message.
            r.Log.LogWarning("[remote-config] decoded to an empty request ({Bytes} bytes) — wrong payload type?", r.Body.Length)
        elif request.HasError then
            // The agent could not apply what it got last time. Worth hearing
            // even while nothing is served.
            r.Log.LogWarning("[remote-config] agent reports error: {Error}", request.Error)
    with :? InvalidProtocolBufferException as e ->
        r.Log.LogWarning("[remote-config] protobuf: {Error} ({Bytes} bytes)", e.Message, r.Body.Length)

    Response.json 404 """{"error":"remote configuration not served"}"""

/// The org identity lookup and the org/key status check: a GET with no body.
let private notServed (_: Request) : Response =
    Response.json 404 """{"error":"not served"}"""

let routes: Route list =
    [ Route.post "/api/v0.1/configurations" handleConfigurations
      // The agent sends GET; POST is registered so anything else that tries
      // gets the same explicit answer.
      Route.get "/api/v0.1/org" notServed
      Route.post "/api/v0.1/org" notServed
      Route.get "/api/v0.1/status" notServed
      Route.post "/api/v0.1/status" notServed ]
