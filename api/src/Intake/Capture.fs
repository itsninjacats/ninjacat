/// Dumps every incoming request to disk. A debugging tool, switched on with
/// DEBUG=true, and not part of the request path.
///
/// This is how the Datadog protocol was worked out: point an agent here and
/// read what actually arrives — decompressed, JSON pretty-printed, protobuf
/// walked field by field without a schema, a hexdump as the last resort.
/// Each request gets a file in NINJACAT_CAPTURE_DIR (default `captures`) and
/// a line in its `index.log`.
module NinjaCat.Api.Intake.Capture

open System
open System.Buffers.Binary
open System.IO
open System.Text
open System.Text.Json
open System.Threading
open Microsoft.AspNetCore.Http

let private strictUtf8 = UTF8Encoding(false, true)

/// Text a person can read: valid UTF-8 with no control characters but
/// newlines and tabs.
let private printable (data: ReadOnlySpan<byte>) : string option =
    try
        let text = strictUtf8.GetString data
        let readable = text |> Seq.forall (fun c -> c = '\n' || c = '\r' || c = '\t' || not (Char.IsControl c))
        if readable then Some text else None
    with :? DecoderFallbackException ->
        None

/// A varint at `position`: its value and the position after it.
let private varint (data: ReadOnlySpan<byte>) (position: int) : (uint64 * int) option =
    let mutable value = 0UL
    let mutable shift = 0
    let mutable i = position
    let mutable result = None
    let mutable failed = false

    while result.IsNone && not failed do
        if i >= data.Length || shift > 63 then
            failed <- true
        else
            let b = data[i]
            value <- value ||| (uint64 (b &&& 0x7fuy) <<< shift)
            i <- i + 1
            shift <- shift + 7
            if b < 0x80uy then result <- Some(value, i)

    result

let private hexPreview (data: ReadOnlySpan<byte>) : string =
    if data.Length > 16 then Convert.ToHexStringLower(data.Slice(0, 16)) + "..." else Convert.ToHexStringLower data

/// Walks protobuf's wire format with no schema: `field_number: value` per
/// line, nested messages indented. False when the bytes are not protobuf.
let rec private dumpProtoInto (out: StringBuilder) (data: ReadOnlySpan<byte>) (depth: int) : bool =
    if data.Length = 0 || depth > 8 then
        false
    else
        let pad = String(' ', depth * 2)
        let mutable i = 0
        let mutable valid = true

        while valid && i < data.Length do
            match varint data i with
            | None -> valid <- false
            | Some(tag, afterTag) ->
                i <- afterTag
                let field = tag >>> 3

                if field = 0UL then
                    valid <- false
                else
                    match int (tag &&& 7UL) with
                    | 0 ->
                        match varint data i with
                        | None -> valid <- false
                        | Some(value, next) ->
                            out.Append($"{pad}{field}: {value}\n") |> ignore
                            i <- next
                    | 1 when i + 8 <= data.Length ->
                        let bits = BinaryPrimitives.ReadUInt64LittleEndian(data.Slice(i, 8))
                        out.Append($"{pad}{field}: {bits} (f64 {BitConverter.UInt64BitsToDouble bits})\n") |> ignore
                        i <- i + 8
                    | 5 when i + 4 <= data.Length ->
                        let bits = BinaryPrimitives.ReadUInt32LittleEndian(data.Slice(i, 4))
                        out.Append($"{pad}{field}: {bits} (f32 {BitConverter.UInt32BitsToSingle bits})\n") |> ignore
                        i <- i + 4
                    | 2 ->
                        match varint data i with
                        | Some(length, next) when length <= uint64 (data.Length - next) ->
                            let value = data.Slice(next, int length)
                            i <- next + int length
                            let nested = StringBuilder()

                            // A length-delimited field is text, a nested
                            // message, or bytes; tried in that order.
                            if value.Length = 0 then
                                out.Append($"{pad}{field}: \"\"\n") |> ignore
                            else
                                match printable value with
                                | Some text -> out.Append($"{pad}{field}: {JsonSerializer.Serialize text}\n") |> ignore
                                | None ->
                                    if dumpProtoInto nested value (depth + 1) then
                                        out.Append($"{pad}{field}: {{\n{nested}{pad}}}\n") |> ignore
                                    else
                                        out.Append($"{pad}{field}: <{value.Length} B> {hexPreview value}\n") |> ignore
                        | _ -> valid <- false
                    | _ -> valid <- false

        valid

/// The bytes as a schema-less protobuf dump, or None when they are not protobuf.
let dumpProto (data: byte[]) : string option =
    let out = StringBuilder()
    if dumpProtoInto out (ReadOnlySpan data) 0 && out.Length > 0 then Some(out.ToString()) else None

/// The classic hexdump: offset, sixteen bytes, their ASCII. First 4096 bytes.
let hexDump (data: byte[]) : string =
    let limit = 4096
    let shown = if data.Length > limit then data[.. limit - 1] else data
    let out = StringBuilder()

    for offset in 0..16 .. shown.Length - 1 do
        let row = shown[offset .. min (offset + 15) (shown.Length - 1)]
        out.Append($"{offset:x8}  ") |> ignore

        for i in 0..15 do
            out.Append(if i < row.Length then $"{row[i]:x2} " else "   ") |> ignore

        out.Append(" |") |> ignore

        for b in row do
            out.Append(if b >= 0x20uy && b < 0x7fuy then char b else '.') |> ignore

        out.Append("|\n") |> ignore

    if data.Length > limit then
        out.Append($"... (cut at {limit} B)\n") |> ignore

    out.ToString()

/// A body as something to read: JSON, then protobuf, then text, then hex.
let render (body: byte[]) : string =
    if body.Length = 0 then
        "(empty body)"
    else
        let asJson =
            try
                use doc = JsonDocument.Parse(ReadOnlyMemory body)
                Some(JsonSerializer.Serialize(doc.RootElement, JsonSerializerOptions(WriteIndented = true)))
            with _ ->
                None

        match asJson, dumpProto body, printable (ReadOnlySpan body) with
        | Some json, _, _ -> json
        | None, Some proto, _ -> "(protobuf, walked without a schema: field_number: value)\n\n" + proto
        | None, None, Some text -> text
        | None, None, None -> hexDump body

/// A path as part of a file name.
let slug (path: string) : string =
    match path.Trim '/' with
    | "" -> "root"
    | trimmed -> String(trimmed |> Seq.map (fun c -> if Char.IsAsciiLetterOrDigit c then c else '_') |> Array.ofSeq)

let private sequence = ref 0L
let private indexLock = obj ()

/// Headers that carry a key are not written to disk.
let private secretHeaders = set [ "dd-api-key"; "authorization"; "dd-application-key" ]

let private orDash (text: string) = if text = "" then "-" else text

/// Writes one request to its own file and adds a line to index.log. Returns
/// the file's path.
let write (directory: string) (http: HttpContext) (raw: byte[]) (decoded: byte[]) (status: int) (started: DateTime) : string =
    Directory.CreateDirectory directory |> ignore
    let request = http.Request
    let encoding = request.Headers.ContentEncoding.ToString()
    let number = Interlocked.Increment &sequence.contents
    let stamp = started.ToString "yyyyMMdd'T'HHmmss.fff"
    let name = $"{stamp}_{request.Method}_{slug request.Path.Value}_{number:D4}.txt"
    let file = Path.Combine(directory, name)

    let out = StringBuilder()
    out.Append($"# time:     {started:O}\n") |> ignore
    out.Append($"# client:   {http.Connection.RemoteIpAddress}\n") |> ignore
    out.Append($"# status:   {status}\n") |> ignore
    out.Append($"# took:     {(DateTime.UtcNow - started).TotalMilliseconds:F1} ms\n") |> ignore
    out.Append($"# body:     {raw.Length} B raw -> {decoded.Length} B decompressed ({orDash encoding})\n\n") |> ignore
    out.Append($"{request.Method} {request.Path}{request.QueryString} {request.Protocol}\n") |> ignore
    out.Append($"Host: {request.Host}\n") |> ignore

    for header in request.Headers do
        if not (header.Key.Equals("Host", StringComparison.OrdinalIgnoreCase)) then
            let value = if secretHeaders.Contains(header.Key.ToLowerInvariant()) then "<redacted>" else header.Value.ToString()
            out.Append($"{header.Key}: {value}\n") |> ignore

    out.Append("\n---------------- BODY ----------------\n") |> ignore
    out.Append(render decoded).Append('\n') |> ignore
    File.WriteAllText(file, out.ToString())

    let time = started.ToString "yyyy-MM-dd'T'HH:mm:ss'Z'"
    let line = $"{time}\t{request.Method}\t{request.Path}\t{status}\t{raw.Length}\t{decoded.Length}\t{orDash encoding}\t{name}\n"
    lock indexLock (fun () -> File.AppendAllText(Path.Combine(directory, "index.log"), line))

    file
