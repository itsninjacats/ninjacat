namespace NinjaCat.Api.Intake

open System
open System.IO
open System.IO.Compression
open System.Text
open System.Text.Json
open System.Threading.Tasks
open Microsoft.AspNetCore.Http
open Microsoft.AspNetCore.WebUtilities
open Microsoft.Extensions.Logging
open Microsoft.Net.Http.Headers
open Oxpecker
open NinjaCat.Api.Storage

/// What a handler reads from the request's context. Each is one line over
/// ASP.NET's own API, named so that a handler reads plainly.
module Ctx =
    /// The intake's logger.
    let log (ctx: HttpContext) : ILogger = ctx.GetLogger "NinjaCat.Api.Intake"

    /// Where rows are written.
    let sink (ctx: HttpContext) : ISink = ctx.GetService<ISink>()

    /// The tenant of the API key the request was admitted with, or "" on a
    /// route that asks for none.
    let tenant (ctx: HttpContext) : string =
        match ctx.User.FindFirst "tenant" with
        | null -> ""
        | claim -> claim.Value

    /// The header's first value, or "".
    let header (ctx: HttpContext) (name: string) : string =
        match ctx.Request.Headers.TryGetValue name with
        | true, values when values.Count > 0 -> values[0]
        | _ -> ""

    /// The query parameter's first value, or "".
    let query (ctx: HttpContext) (name: string) : string =
        match ctx.Request.Query.TryGetValue name with
        | true, values when values.Count > 0 -> values[0]
        | _ -> ""

    /// Hands rows to the table's writer. A request with no tenant (a route
    /// that asks for no key) writes nothing: a row belongs to a tenant.
    let write (ctx: HttpContext) (table: Table<'row>) (rows: 'row[]) : unit =
        if tenant ctx <> "" then
            Sink.write (sink ctx) table rows

/// Request bodies arrive compressed three ways, one per agent subsystem: zstd
/// for metrics, gzip for traces, deflate for distribution points. No proxy
/// undoes that for us: Content-Encoding is end to end.
/// Where a key travels outside a header, so that what is stored or dumped
/// can leave it out.
module Secrets =
    /// The query parameters that carry a key.
    let queryParameters = set [ "api_key"; "dd-api-key" ]

    /// The path without a key in it: older log clients put theirs in
    /// /v1/input/<key>.
    let pathWithoutKey (path: string) : string =
        if path.StartsWith("/v1/input/", StringComparison.OrdinalIgnoreCase) then "/v1/input/<key>" else path

module Body =
    /// The most a body may inflate to. Agents send a few megabytes; this
    /// only has to stop a few kilobytes that inflate to gigabytes.
    let maxInflatedBytes = 64 * 1024 * 1024

    /// Everything a decoder gives, up to that limit; past it the read fails.
    let readInflated (stream: Stream) : byte[] =
        use buffer = new MemoryStream()
        let chunk = Array.zeroCreate<byte> 81920
        let mutable read = stream.Read(chunk, 0, chunk.Length)

        while read > 0 do
            if buffer.Length + int64 read > int64 maxInflatedBytes then
                raise (InvalidDataException $"it inflates past {maxInflatedBytes} bytes")

            buffer.Write(chunk, 0, read)
            read <- stream.Read(chunk, 0, chunk.Length)

        buffer.ToArray()

    /// The request's body as it arrived, still compressed.
    let read (http: HttpContext) : Task<byte[]> =
        task {
            use buffer = new MemoryStream()
            do! http.Request.Body.CopyToAsync buffer
            return buffer.ToArray()
        }

    let private inflate (wrap: Stream -> Stream) (body: byte[]) : byte[] =
        use input = new MemoryStream(body)
        use decoder = wrap input
        readInflated decoder

    /// The body decompressed. An unknown encoding, a body that does not
    /// decode, or one that inflates past the limit comes back unchanged: a
    /// mislabelled Content-Encoding should not lose a payload the handler
    /// could still read, and the handler keeps what it cannot.
    let decompress (log: ILogger) (encoding: string) (body: byte[]) : byte[] =
        try
            match encoding with
            | "zstd" -> inflate (fun s -> new ZstdSharp.DecompressionStream(s)) body
            | "gzip" -> inflate (fun s -> new GZipStream(s, CompressionMode.Decompress)) body
            | "deflate"
            | "zlib" ->
                // The RFC says zlib; some clients send bare deflate.
                try
                    inflate (fun s -> new ZLibStream(s, CompressionMode.Decompress)) body
                with _ ->
                    inflate (fun s -> new DeflateStream(s, CompressionMode.Decompress)) body
            | _ -> body
        with e ->
            log.LogWarning("[body] {Encoding} decompression failed, passing through: {Error}", encoding, e.Message)
            body

    /// The format of a body: from Content-Type, and when that is missing or
    /// says nothing, from the first significant byte.
    let isJson (contentType: string) (body: byte[]) : bool =
        if contentType.Contains "json" then
            true
        elif contentType.Contains "protobuf" || contentType.Contains "octet-stream" then
            false
        else
            let first = body |> Array.tryFind (fun b -> b <> ' 'B && b <> '\t'B && b <> '\r'B && b <> '\n'B)
            first = Some '{'B || first = Some '['B

/// What Datadog's intakes answer once a body has arrived, whatever became of
/// it: 202 and an empty object.
[<AutoOpen>]
module Accepted =
    let accepted: EndpointHandler = setStatusCode 202 >=> json {||}

/// Oxpecker's bindJson, for a body that is not JSON to bind: reads the body
/// whole, undoes its Content-Encoding, and runs the handler with the bytes.
[<AutoOpen>]
module BindBody =
    let bindBody (handler: byte[] -> EndpointHandler) : EndpointHandler =
        fun ctx ->
            task {
                let! raw = Body.read ctx
                let encoding = ctx.Request.Headers.ContentEncoding.ToString()
                let body = if encoding = "" then raw else Body.decompress (Ctx.log ctx) encoding raw
                return! handler body ctx
            }

type MultipartPart =
    { /// The form field's name, or "".
      Name: string
      /// The uploaded file's name, or "" for a plain field.
      FileName: string
      ContentType: string
      Data: byte[] }

module Multipart =
    /// The boundary of a multipart Content-Type, or None when it is not one.
    let boundary (contentType: string) : string option =
        match MediaTypeHeaderValue.TryParse contentType with
        | true, media when media.Boundary.HasValue && media.Boundary.Value <> "" ->
            Some(HeaderUtilities.RemoveQuotes(media.Boundary).Value)
        | _ -> None

    /// The parts read, in order and with repeats, and why reading stopped
    /// before the end of the body, when it did.
    let private read (boundary: string) (body: byte[]) : MultipartPart list * string option =
        let reader = MultipartReader(boundary, new MemoryStream(body))
        // The reader's defaults are for form posts; flares and profiles are larger.
        reader.HeadersLengthLimit <- 1024 * 1024
        reader.BodyLengthLimit <- Nullable()
        let found = ResizeArray<MultipartPart>()

        let problem =
            try
                // Synchronous on purpose: the stream is memory, nothing waits.
                let mutable section = reader.ReadNextSectionAsync().GetAwaiter().GetResult()

                while not (isNull section) do
                    use data = new MemoryStream()
                    section.Body.CopyTo data

                    let name, fileName =
                        match ContentDispositionHeaderValue.TryParse section.ContentDisposition with
                        | true, disposition ->
                            let file =
                                if disposition.FileNameStar.HasValue then disposition.FileNameStar.Value
                                elif disposition.FileName.HasValue then HeaderUtilities.RemoveQuotes(disposition.FileName).Value
                                else ""

                            HeaderUtilities.RemoveQuotes(disposition.Name).Value |> Option.ofObj |> Option.defaultValue "", file
                        | _ -> "", ""

                    found.Add
                        { Name = name
                          FileName = fileName
                          ContentType = (if isNull section.ContentType then "" else section.ContentType)
                          Data = data.ToArray() }

                    section <- reader.ReadNextSectionAsync().GetAwaiter().GetResult()

                None
            with
            | :? InvalidDataException as e -> Some e.Message
            | _ -> Some "the multipart body breaks off"

        List.ofSeq found, problem

    /// Every part of a multipart body. An Error when the content type is not
    /// multipart, names no boundary, or the body breaks off: the caller then
    /// keeps the request whole instead of storing half a submission.
    let tryParts (contentType: string) (body: byte[]) : Result<MultipartPart list, string> =
        match MediaTypeHeaderValue.TryParse contentType with
        | false, _ -> Error "the content type is not a media type"
        | true, media when not (media.MediaType.StartsWith("multipart/", StringComparison.OrdinalIgnoreCase)) ->
            Error $"the content type is {media.MediaType.Value.ToLowerInvariant()}, not multipart"
        | true, _ ->
            match boundary contentType with
            | None -> Error "the content type is multipart without a boundary"
            | Some boundary ->
                match read boundary body with
                | parts, None -> Ok parts
                | _, Some problem -> Error problem

    /// The parts before the break, for an upload worth keeping in part (a
    /// flare whose last fields were cut off still has its archive).
    let parts (boundary: string) (body: byte[]) : MultipartPart list = fst (read boundary body)
