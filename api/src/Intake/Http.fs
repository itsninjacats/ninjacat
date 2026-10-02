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

/// Request bodies arrive compressed three ways, one per agent subsystem: zstd
/// for metrics, gzip for traces, deflate for distribution points. No proxy
/// undoes that for us: Content-Encoding is end to end.
module Body =
    let private readAll (stream: Stream) : byte[] =
        use buffer = new MemoryStream()
        stream.CopyTo buffer
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
