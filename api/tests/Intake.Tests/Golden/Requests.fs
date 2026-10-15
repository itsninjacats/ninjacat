/// What the hand-written tests share: one way to describe a request, to
/// send it, to build a multipart body and to read a test's own JSON.
module NinjaCat.Api.Intake.Tests.Golden.Requests

open System.IO
open System.Text
open System.Text.Json
open System.Text.Json.Nodes
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open NinjaCat.Api.Intake

let utf8 (text: string) : byte[] = Encoding.UTF8.GetBytes text

/// A test's own JSON, parsed as the intake parses a body.
let json (text: string) : JsonElement =
    match Json.tryParse (utf8 text) with
    | Ok value -> value
    | Error e -> failwith $"the test's JSON does not parse: {e}"

/// Equal as JSON: spacing and the order of keys are not what is tested.
let sameJson (expected: string) (actual: string) : unit =
    Xunit.Assert.True(JsonNode.DeepEquals(JsonNode.Parse expected, JsonNode.Parse actual), $"expected {expected}\nactual   {actual}")

/// The header a sender with the test key adds.
let withKey: (string * string) list = [ "Dd-Api-Key", Replay.testKey ]

/// The test keys, and a sink that remembers what was written.
let deps (sink: CapturingSink) : Deps =
    { Store = Replay.testStore ()
      Sink = sink
      Log = NullLogger.Instance
      AckUnknown = false }

/// A request: `url` is a path, with or without a query. Of two headers with
/// one name the later one counts.
let httpContext (method: string) (host: string) (url: string) (headers: (string * string) list) : HttpContext =
    let http = DefaultHttpContext()
    http.Request.Method <- method
    http.Request.Host <- HostString host

    match url.IndexOf '?' with
    | -1 -> http.Request.Path <- PathString url
    | i ->
        http.Request.Path <- PathString(url.Substring(0, i))
        http.Request.QueryString <- QueryString(url.Substring i)

    for name, value in headers do
        http.Request.Headers[name] <- StringValues value

    http

/// Sends one request to the intake made of the named Go route sets, the way
/// the golden replay does. No key is added: a test that wants one says
/// `withKey`. Returns the answer and what was written.
let send
    (routeSets: string list)
    (method: string)
    (url: string)
    (headers: (string * string) list)
    (body: byte[])
    : Response * CapturingSink =
    let sink = CapturingSink()
    Replay.byGoNames (deps sink) routeSets (httpContext method "example.com" url headers) body, sink

/// One part of a multipart body. FileName and ContentType are left out of
/// the part's headers when they are "".
type Part =
    { Name: string
      FileName: string
      ContentType: string
      Data: byte[] }

/// A part that is a plain form field.
let field (name: string) (data: byte[]) : Part =
    { Name = name; FileName = ""; ContentType = ""; Data = data }

/// A multipart/form-data body of the given parts, and its Content-Type.
let multipart (boundary: string) (parts: Part list) : string * byte[] =
    use body = new MemoryStream()
    let write (text: string) = body.Write(System.ReadOnlySpan(utf8 text))

    for part in parts do
        write $"--{boundary}\r\n"
        write $"Content-Disposition: form-data; name=\"{part.Name}\""

        if part.FileName <> "" then
            write $"; filename=\"{part.FileName}\""

        write "\r\n"

        if part.ContentType <> "" then
            write $"Content-Type: {part.ContentType}\r\n"

        write "\r\n"
        body.Write(System.ReadOnlySpan part.Data)
        write "\r\n"

    write $"--{boundary}--\r\n"
    $"multipart/form-data; boundary={boundary}", body.ToArray()
