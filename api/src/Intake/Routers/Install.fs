/// Hosts that do not follow the prefix + site scheme.
///
///   install.datadoghq.com / install.datad0g.com   OCI registry, BTF files, HEAD probe
///   eudm-intake.<site>                            /api/v2/aiusage
///   llmobs-intake.<site>                          /api/v2/llmobs (diagnose probe only)
///
/// The install host stores nothing: every answer is a fixed protocol error
/// or an empty ack. aiusage and llmobs have no published schema, so a real
/// payload on either goes to raw_payloads as "no_schema".
module NinjaCat.Api.Intake.Routers.Install

open System.Globalization
open System.Text
open System.Threading.Tasks
open Microsoft.Extensions.Logging
open Microsoft.AspNetCore.Http
open Oxpecker
open NinjaCat.Api.Intake

/// Marks an answer as coming from a Distribution API v2 registry.
let private fromRegistry: EndpointHandler =
    setHttpHeader "Docker-Distribution-Api-Version" "registry/2.0"

/// The fleet installer speaks OCI Distribution, and no packages are hosted
/// here. The protocol's own "not found" — 404 with an OCI error body — is
/// not a network error, so the installer fails at once with a readable
/// message instead of retrying. go-containerregistry parses `code`.
let private ociError (code: string) (message: string) : EndpointHandler =
    fromRegistry >=> setStatusCode 404 >=> json {| errors = [ {| code = code; message = message |} ] |}

/// The diagnose sweep's HEAD /: any 2xx is "Success".
let handleProbe (_: byte[]) : EndpointHandler = setStatusCode 200

/// The API version check every registry client starts with.
let handleVersionCheck (_: byte[]) (ctx: HttpContext) : Task =
    (fromRegistry >=> json {||}) ctx

let handleManifest (repo: string) (tag: string) (_: byte[]) (ctx: HttpContext) : Task =
    ociError "MANIFEST_UNKNOWN" ("ninjacat does not host packages: " + repo + ":" + tag) ctx

let handleBlob (repo: string) (digest: string) (_: byte[]) (ctx: HttpContext) : Task =
    ociError "BLOB_UNKNOWN" ("ninjacat does not host packages: " + repo + "@" + digest) ctx

/// system-probe fetching a kernel BTF archive. Its SHA256 is checked against
/// the BTF_DD remote config catalog, so no substitute could be served; a
/// non-200 makes the loader fall back to its next BTF source.
let handleBtf (_: byte[]) (ctx: HttpContext) : Task =
    (setStatusCode 404 >=> json {| errors = [ "BTF archives are not hosted here" ] |}) ctx

/// The Rust ai_prompt_logger posts through the local evp_proxy, which adds
/// the API key; it only checks for a 2xx. The producer is not in
/// datadog-agent and publishes no schema, so the body is kept as it arrived.
let handleAIUsage (body: byte[]) (ctx: HttpContext) : Task =
    Raw.store ctx "aiusage" "no_schema" (Raw.originNote ctx) body
    accepted ctx

/// The agent never sends LLM Observability data here; the only caller is the
/// connectivity diagnose, which posts no body and wants a 2xx.
let handleLLMObs (body: byte[]) (ctx: HttpContext) : Task =

    if body.Length > 0 && not (Diagnose.isSweep ctx) then
        Raw.store ctx "llmobs" "no_schema" ("unexpected payload — the agent only probes this host; " + Raw.originNote ctx) body

    accepted ctx
