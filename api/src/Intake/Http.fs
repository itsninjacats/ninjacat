namespace NinjaCat.Api.Intake

open System
open System.IO
open System.IO.Compression
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open Microsoft.AspNetCore.Http
open Microsoft.AspNetCore.WebUtilities
open Microsoft.Extensions.Logging
open Microsoft.Net.Http.Headers
open NinjaCat.Api.Storage

/// What a handler answers with. Handlers return it; the engine writes it.
type Response =
    { Status: int
      Headers: (string * string) list
      Body: byte[] }

module Response =
    let private jsonOptions =
        JsonSerializerOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping)

    let status (code: int) : Response = { Status = code; Headers = []; Body = [||] }

    let bytes (code: int) (contentType: string) (body: byte[]) : Response =
        { Status = code
          Headers = [ "Content-Type", contentType ]
          Body = body }

    /// JSON given as text, e.g. `Response.json 202 "{}"`.
    let json (code: int) (text: string) : Response =
        bytes code "application/json; charset=utf-8" (Encoding.UTF8.GetBytes text)

    /// JSON from a value, e.g. an anonymous record.
    let jsonOf (code: int) (value: 'a) : Response =
        bytes code "application/json; charset=utf-8" (JsonSerializer.SerializeToUtf8Bytes(value, jsonOptions))

    /// The error envelope Datadog's own intake uses.
    let errors (code: int) (messages: string list) : Response = jsonOf code {| errors = messages |}

    let withHeader (name: string) (value: string) (response: Response) : Response =
        { response with Headers = response.Headers @ [ name, value ] }

/// One request as a handler sees it. The body is already read and
/// decompressed.
type Request =
    { Http: HttpContext
      Body: byte[]
      /// Path parameters of the matched route (`:case_id`).
      Params: Map<string, string>
      /// The API key the request was admitted with; None on public routes.
      Key: ApiKeys.Key option
      Sink: ISink
      Log: ILogger }

    /// The tenant the request's data belongs to, or "" on a public route.
    member r.Tenant: string =
        match r.Key with
        | Some key -> key.TenantID
        | None -> ""

    member r.Method: string = r.Http.Request.Method
    member r.Path: string = r.Http.Request.Path.Value
    member r.Host: string = r.Http.Request.Host.Value

    /// The header's first value, or "".
    member r.Header(name: string) : string =
        match r.Http.Request.Headers.TryGetValue name with
        | true, values when values.Count > 0 -> values[0]
        | _ -> ""

    /// The query parameter's first value, or "".
    member r.Query(name: string) : string =
        match r.Http.Request.Query.TryGetValue name with
        | true, values when values.Count > 0 -> values[0]
        | _ -> ""

    /// The path parameter, or "".
    member r.Param(name: string) : string =
        match r.Params.TryFind name with
        | Some value -> value
        | None -> ""

type Handler = Request -> Response

type Route =
    { Method: string
      /// Gin's syntax: `/support/flare/:case_id`, `/v2/*path`.
      Pattern: string
      Handler: Handler }

module Route =
    let get (pattern: string) (handler: Handler) : Route =
        { Method = "GET"; Pattern = pattern; Handler = handler }

    let post (pattern: string) (handler: Handler) : Route =
        { Method = "POST"; Pattern = pattern; Handler = handler }

    let put (pattern: string) (handler: Handler) : Route =
        { Method = "PUT"; Pattern = pattern; Handler = handler }

    let head (pattern: string) (handler: Handler) : Route =
        { Method = "HEAD"; Pattern = pattern; Handler = handler }

/// Request bodies arrive compressed three ways, one per agent subsystem: zstd
/// for metrics, gzip for traces, deflate for distribution points. No proxy
/// undoes that for us: Content-Encoding is end to end.
module Body =
    let private readAll (stream: Stream) : byte[] =
        use buffer = new MemoryStream()
        stream.CopyTo buffer
        buffer.ToArray()

    let private inflate (wrap: Stream -> Stream) (body: byte[]) : byte[] =
        use input = new MemoryStream(body)
        use decoder = wrap input
        readAll decoder

    /// The body decompressed. An unknown encoding, or a body that does not
    /// decode, comes back unchanged: a mislabelled Content-Encoding should not
    /// lose a payload the handler could still read.
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

    /// Every part of a multipart body, in order. A body that breaks off
    /// midway gives the parts before the break.
    let parts (boundary: string) (body: byte[]) : MultipartPart list =
        let reader = MultipartReader(boundary, new MemoryStream(body))
        // The reader's defaults are for form posts; flares and profiles are larger.
        reader.HeadersLengthLimit <- 1024 * 1024
        reader.BodyLengthLimit <- Nullable()
        let found = ResizeArray<MultipartPart>()

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
        with _ ->
            ()

        List.ofSeq found
