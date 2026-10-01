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
open Microsoft.Extensions.Logging
open NinjaCat.Api.Intake

/// Go's strconv.Quote, which is what `%q` prints: the text in double quotes,
/// with quotes, backslashes and everything unprintable escaped.
let goQuote (text: string) : string =
    let quoted = StringBuilder("\"")

    for rune in text.EnumerateRunes() do
        match rune.Value with
        | 0x22 -> quoted.Append "\\\"" |> ignore
        | 0x5C -> quoted.Append "\\\\" |> ignore
        | 0x07 -> quoted.Append "\\a" |> ignore
        | 0x08 -> quoted.Append "\\b" |> ignore
        | 0x0C -> quoted.Append "\\f" |> ignore
        | 0x0A -> quoted.Append "\\n" |> ignore
        | 0x0D -> quoted.Append "\\r" |> ignore
        | 0x09 -> quoted.Append "\\t" |> ignore
        | 0x0B -> quoted.Append "\\v" |> ignore
        | 0x20 -> quoted.Append ' ' |> ignore
        | code when code < 0x20 || code = 0x7F -> quoted.Append("\\x" + code.ToString "x2") |> ignore
        | code ->
            // Printable in Go's sense: a letter, mark, number, punctuation
            // or symbol.
            match Rune.GetUnicodeCategory rune with
            | UnicodeCategory.Control
            | UnicodeCategory.Format
            | UnicodeCategory.Surrogate
            | UnicodeCategory.PrivateUse
            | UnicodeCategory.OtherNotAssigned
            | UnicodeCategory.SpaceSeparator
            | UnicodeCategory.LineSeparator
            | UnicodeCategory.ParagraphSeparator ->
                if code < 0x10000 then
                    quoted.Append("\\u" + code.ToString "x4") |> ignore
                else
                    quoted.Append("\\U" + code.ToString "x8") |> ignore
            | _ -> quoted.Append(rune.ToString()) |> ignore

    quoted.Append('"').ToString()

/// Marks an answer as coming from a Distribution API v2 registry.
let private fromRegistry (response: Response) : Response =
    Response.withHeader "Docker-Distribution-Api-Version" "registry/2.0" response

/// The fleet installer speaks OCI Distribution, and no packages are hosted
/// here. The protocol's own "not found" — 404 with an OCI error body — is
/// not a network error, so the installer fails at once with a readable
/// message instead of retrying. go-containerregistry parses `code`.
let private ociError (code: string) (message: string) : Response =
    Response.jsonOf 404 {| errors = [ {| code = code; message = message |} ] |} |> fromRegistry

/// The diagnose sweep's HEAD /: any 2xx is "Success".
let handleProbe (_: Request) : Response = Response.status 200

/// The API version check every registry client starts with.
let handleVersionCheck (_: Request) : Response =
    Response.json 200 "{}" |> fromRegistry

let handleManifest (repo: string) (tag: string) (_: Request) : Response =
    ociError "MANIFEST_UNKNOWN" ("ninjacat does not host packages: " + repo + ":" + tag)

let handleBlob (repo: string) (digest: string) (_: Request) : Response =
    ociError "BLOB_UNKNOWN" ("ninjacat does not host packages: " + repo + "@" + digest)

/// system-probe fetching a kernel BTF archive. Its SHA256 is checked against
/// the BTF_DD remote config catalog, so no substitute could be served; a
/// non-200 makes the loader fall back to its next BTF source.
let handleBtf (_: Request) : Response =
    Response.errors 404 [ "BTF archives are not hosted here" ]


/// The note of a payload with no published schema: the event platform's
/// origin headers and the Content-Type are all the context there is beside
/// the opaque bytes.
let private originNote (r: Request) : string =
    let origin = goQuote (r.Header "DD-EVP-ORIGIN")
    let version = goQuote (r.Header "DD-EVP-ORIGIN-VERSION")
    let contentType = goQuote (r.Header "Content-Type")
    $"DD-EVP-ORIGIN={origin} DD-EVP-ORIGIN-VERSION={version} Content-Type={contentType}"

let private accepted = Response.json 202 "{}"

/// The Rust ai_prompt_logger posts through the local evp_proxy, which adds
/// the API key; it only checks for a 2xx. The producer is not in
/// datadog-agent and publishes no schema, so the body is kept as it arrived.
let handleAIUsage (r: Request) : Response =
    Raw.store r "aiusage" "no_schema" (originNote r) r.Body
    accepted

/// The agent never sends LLM Observability data here; the only caller is the
/// connectivity diagnose, which posts no body and wants a 2xx.
let handleLLMObs (r: Request) : Response =
    if r.Body.Length > 0 && not (Diagnose.isSweep r) then
        r.Log.LogWarning("[llmobs] unexpected payload — the agent only probes this host")
        Raw.store r "llmobs" "no_schema" ("unexpected payload — the agent only probes this host; " + originNote r) r.Body

    accepted
