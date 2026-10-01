/// Three hosts fed by the trace-agent's proxies and the host profiler:
///
///   intake.profile.<site>    /api/v2/profile, /v1/input  → profiles
///   debugger-intake.<site>   /api/v2/debugger            → debugger_logs, debugger_diagnostics, symdb_uploads
///   sourcemap-intake.<site>  /api/v2/srcmap              → symbol_uploads
///
///   agent config: apm_config.profiling_dd_url, internal_profiling.profile_dd_url,
///                 apm_config.debugger_diagnostics_dd_url, apm_config.symdb_dd_url,
///                 profiling.symbol_uploader.symbol_endpoints[].site
///
/// The trace-agent only forwards these bodies, so the agent has no type for
/// any of them: the attachments are pprof, the event parts JSON. A body that
/// does not decode goes to raw_payloads.
module NinjaCat.Api.Intake.Routers.Profiling

open System
open System.Buffers
open System.Buffers.Binary
open System.IO
open System.IO.Compression
open System.Text
open System.Text.Json
open Microsoft.AspNetCore.WebUtilities
open Microsoft.Extensions.Logging
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// A Content-Type or Content-Disposition header, read the way Go's
/// mime.ParseMediaType reads it: the notes in raw_payloads quote its verdict,
/// and it decides between the JSON and the multipart variant of the debugger
/// intake. RFC 2231 continuations (`name*0=`) are not decoded.
module private MediaType =
    type Parsed =
        { /// Lower case, without parameters; "" when the header has no usable type.
          Type: string
          Parameters: Map<string, string>
          /// Why the header is malformed, in Go's words; "" when it is not.
          Error: string }

    let private specials = "()<>@,;:\\\"/[]?="

    let private isTokenChar (c: char) : bool =
        c > ' ' && c < '\127' && not (specials.Contains c)

    /// The token the text starts with, and what follows it.
    let private token (text: string) : string * string =
        let mutable length = 0

        while length < text.Length && isTokenChar text[length] do
            length <- length + 1

        text.Substring(0, length), text.Substring length

    let private typeProblem (mediaType: string) : string =
        let kind, rest = token mediaType

        if kind = "" then
            "mime: no media type"
        elif rest = "" then
            ""
        elif not (rest.StartsWith '/') then
            "mime: expected slash after first token"
        else
            let subtype, rest = token (rest.Substring 1)

            if subtype = "" then "mime: expected token after slash"
            elif rest <> "" then "mime: unexpected content after media subtype"
            else ""

    /// A parameter's value — a token or a quoted string — and what follows it.
    let private value (text: string) : (string * string) option =
        if text = "" then
            None
        elif text[0] <> '"' then
            match token text with
            | "", _ -> None
            | found, rest -> Some(found, rest)
        else
            let unquoted = StringBuilder()
            let mutable i = 1
            let mutable result = None
            let mutable broken = false

            while result.IsNone && not broken && i < text.Length do
                let c = text[i]

                if c = '"' then
                    result <- Some(unquoted.ToString(), text.Substring(i + 1))
                elif c = '\\' && i + 1 < text.Length && specials.Contains text[i + 1] then
                    // Only specials are escaped: a backslash before anything
                    // else is a literal one (Windows paths in file names).
                    unquoted.Append text[i + 1] |> ignore
                    i <- i + 2
                elif c = '\r' || c = '\n' then
                    broken <- true
                else
                    unquoted.Append c |> ignore
                    i <- i + 1

            result

    /// `; name=value` at the start of the text, and what follows it.
    type private Parameter =
        { Name: string
          Value: string
          Rest: string }

    let private parameter (text: string) : Parameter option =
        let text = text.TrimStart()

        if not (text.StartsWith ';') then
            None
        else
            let name, rest = token (text.Substring(1).TrimStart())
            let rest = rest.TrimStart()

            if name = "" || not (rest.StartsWith '=') then
                None
            else
                match value (rest.Substring(1).TrimStart()) with
                | Some(found, after) ->
                    Some
                        { Name = name.ToLowerInvariant()
                          Value = found
                          Rest = after }
                | None -> None

    let parse (header: string) : Parsed =
        let beforeParameters =
            match header.IndexOf ';' with
            | -1 -> header
            | i -> header.Substring(0, i)

        let mediaType = beforeParameters.ToLowerInvariant().Trim()
        let problem = typeProblem mediaType

        if problem <> "" then
            { Type = ""; Parameters = Map.empty; Error = problem }
        else
            let mutable rest = header.Substring beforeParameters.Length
            let mutable parameters = Map.empty
            let mutable result = None

            while result.IsNone do
                rest <- rest.TrimStart()

                if rest = "" then
                    result <- Some { Type = mediaType; Parameters = parameters; Error = "" }
                else
                    match parameter rest with
                    | None when rest.Trim() = ";" ->
                        // A trailing semicolon is not an error.
                        result <- Some { Type = mediaType; Parameters = parameters; Error = "" }
                    | None -> result <- Some { Type = mediaType; Parameters = Map.empty; Error = "mime: invalid media parameter" }
                    | Some found ->
                        match parameters.TryFind found.Name with
                        | Some earlier when earlier <> found.Value ->
                            result <- Some { Type = ""; Parameters = Map.empty; Error = "mime: duplicate parameter name" }
                        | _ ->
                            if not (found.Name.Contains '*') then
                                parameters <- parameters.Add(found.Name, found.Value)

                            rest <- found.Rest

            result.Value

/// The elements of a JSON array, each as the bytes it had on the wire; None
/// when the data is not exactly one JSON array.
let private arrayElements (data: byte[]) : byte[] list option =
    let elements = ResizeArray<byte[]>()
    let mutable reader = Utf8JsonReader(ReadOnlySpan data, JsonReaderOptions(MaxDepth = Lenient.jsonDepth))

    try
        if not (reader.Read()) || reader.TokenType <> JsonTokenType.StartArray then
            None
        else
            while reader.Read() && reader.TokenType <> JsonTokenType.EndArray do
                let start = int reader.TokenStartIndex
                reader.Skip()
                elements.Add data[start .. int reader.BytesConsumed - 1]

            // Reading on proves nothing follows the array: more data throws.
            reader.Read() |> ignore
            Some(List.ofSeq elements)
    with :? JsonException ->
        None

/// A member of a decoded object. None when the object did not decode, the
/// member is absent, or it is null: the Go intake read all three as "not sent".
let private field (name: string) (decoded: JsonElement option) : JsonElement option =
    match decoded with
    | Some object when object.ValueKind = JsonValueKind.Object ->
        Lenient.property name object |> Option.filter (fun value -> value.ValueKind <> JsonValueKind.Null)
    | _ -> None

/// A string member's value; "" when the producer sent another type.
let private asString (value: JsonElement option) : string =
    match value with
    | Some v when v.ValueKind = JsonValueKind.String -> Lenient.text v
    | _ -> ""

/// A value as Go's `%v` prints it once decoded: what the Go intake stored
/// for a value that is neither text nor a number. Past `levels` of nesting
/// the JSON is left as it is, so depth cannot exhaust the stack.
let rec private goText (levels: int) (value: JsonElement) : string =
    match value.ValueKind with
    | JsonValueKind.String -> Lenient.text value
    | JsonValueKind.Null -> "<nil>"
    | JsonValueKind.Array when levels > 0 ->
        "[" + String.Join(" ", value.EnumerateArray() |> Seq.map (goText (levels - 1))) + "]"
    | JsonValueKind.Object when levels > 0 ->
        let members = value.EnumerateObject() |> Seq.map (fun p -> Lenient.nameOf p, p.Value) |> Map.ofSeq
        "map[" + String.Join(" ", members |> Seq.map (fun pair -> pair.Key + ":" + goText (levels - 1) pair.Value)) + "]"
    | _ -> value.GetRawText()

/// A value as the text it had on the wire: a string without its quotes, a
/// number digit for digit (never through a float). "" when not sent.
let private rawText (value: JsonElement option) : string =
    match value with
    | Some v -> goText 100 v
    | None -> ""

/// A boolean as 1 or 0; None when it was not sent or is not a boolean, so
/// "not sent" and "sent false" stay apart.
let private asFlag (value: JsonElement option) : uint8 option =
    match value with
    | Some v when v.ValueKind = JsonValueKind.True -> Some 1uy
    | Some v when v.ValueKind = JsonValueKind.False -> Some 0uy
    | _ -> None

/// A byte count; None when it was not sent, is not an integer, or is negative.
let private asCount (value: JsonElement option) : uint64 option =
    match value with
    | Some v when v.ValueKind = JsonValueKind.Number ->
        match v.TryGetInt64() with
        | true, n when n >= 0L -> Some(uint64 n)
        | _ -> None
    | _ -> None

/// An epoch number as a time. Producers send seconds, milliseconds,
/// microseconds or nanoseconds; the magnitude says which, as it would to a
/// person reading the number.
let private epochTime (n: int64) : DateTime option =
    try
        if n >= 1_000_000_000_000_000_000L then
            Some((Time.fromUnixSeconds (n / 1_000_000_000L)).AddTicks(n % 1_000_000_000L / 100L))
        elif n >= 1_000_000_000_000_000L then
            Some((Time.fromUnixSeconds (n / 1_000_000L)).AddTicks(n % 1_000_000L * 10L))
        elif n >= 1_000_000_000_000L then
            Some(Time.fromUnixMillis n)
        else
            Some(Time.fromUnixSeconds n)
    with :? ArgumentOutOfRangeException ->
        // Past the year 9999, which a DateTime cannot hold.
        None

/// A profiler event's start or end, or a diagnostic's timestamp: RFC 3339
/// text from Datadog's profiler, an epoch number from other producers. None
/// means "could not tell", never a guessed time: the row keeps the original
/// value next to this one.
let parseTimestamp (value: JsonElement option) : DateTime option =
    match value with
    | Some v when v.ValueKind = JsonValueKind.String -> Lenient.rfc3339 (Lenient.text v)
    | Some v when v.ValueKind = JsonValueKind.Number ->
        match v.TryGetInt64() with
        | true, n when n > 0L -> epochTime n
        | _ -> None
    | _ -> None

/// The data inflated; None when it is not a whole gzip stream, or inflates
/// past `limit` bytes: a result is never cut off at the limit.
let private gunzip (limit: int64) (data: byte[]) : byte[] option =
    try
        use input = new MemoryStream(data)
        use inflater = new GZipStream(input, CompressionMode.Decompress)
        use output = new MemoryStream()
        let buffer = Array.zeroCreate<byte> 81920
        let mutable read = inflater.Read(buffer, 0, buffer.Length)

        while read > 0 && output.Length <= limit do
            output.Write(buffer, 0, read)
            read <- inflater.Read(buffer, 0, buffer.Length)

        // GZipStream reads a stream that breaks off, or has bytes after its
        // end, without complaint; Go's reader refuses both. A whole stream
        // ends with the size of what it inflates to.
        let declaredSize =
            if data.Length < 18 then None else Some(BinaryPrimitives.ReadUInt32LittleEndian(ReadOnlySpan(data, data.Length - 4, 4)))

        if output.Length > limit || declaredSize <> Some(uint32 output.Length) then
            None
        else
            Some(output.ToArray())
    with
    | :? InvalidDataException
    | :? IOException -> None

/// The envelope of a pprof attachment, or None when the bytes are not
/// pprof. Profilers send them gzip-compressed.
let readPprof (data: byte[]) : Pprof.Summary option =
    if data.Length >= 2 && data[0] = 0x1Fuy && data[1] = 0x8Buy then
        gunzip Int64.MaxValue data |> Option.bind Pprof.read
    else
        Pprof.read data

let private notPprof: Pprof.Summary =
    { SampleTypes = [||]
      SampleUnits = [||]
      SampleCount = 0UL
      TimeNanos = 0L
      DurationNanos = 0L
      PeriodType = ""
      Period = 0L
      MappingCount = 0u
      LocationCount = 0u
      FunctionCount = 0u }

/// One profiles row from a multipart submission. `event` is the decoded
/// "event" part (None when absent or not a JSON object), `profiles` the
/// attachments that are pprof, by part name. Every part other than "event"
/// becomes an attachment whether it parsed or not.
let profileRow
    (tenant: string)
    (label: string)
    (origin: string)
    (originVersion: string)
    (parts: Map<string, byte[]>)
    (event: JsonElement option)
    (profiles: Map<string, Pprof.Summary>)
    : ProfileRow =
    // A Map lists its keys sorted, which is the order the columns promise.
    let attachments = parts |> Map.remove "event" |> Map.toArray
    let parsed = attachments |> Array.map (fun (name, _) -> profiles.TryFind name)
    let shapes = parsed |> Array.map (Option.defaultValue notPprof)

    { TenantID = tenant
      ReceivedAt = DateTime.UtcNow
      Variant = label
      DDEvpOrigin = origin
      DDEvpOriginVersion = originVersion
      Event = parts.TryFind "event" |> Option.defaultValue [||]
      StartRaw = rawText (field "start" event)
      StartParsed = parseTimestamp (field "start" event)
      EndRaw = rawText (field "end" event)
      EndParsed = parseTimestamp (field "end" event)
      Family = asString (field "family" event)
      Version = asString (field "version" event)
      Runtime = asString (field "runtime" event)
      Language = asString (field "language" event)
      // A third tag convention: one comma-joined string.
      TagsProfiler = Tags.toMultiMap (Tags.splitDDTags (asString (field "tags_profiler" event)))
      AttachName = attachments |> Array.map fst
      AttachBytes = attachments |> Array.map snd
      AttachSize = attachments |> Array.map (fun (_, data) -> uint64 data.Length)
      AttachParsed = parsed |> Array.map (fun p -> Text.flag p.IsSome)
      AttachSampleTypes = shapes |> Array.map _.SampleTypes
      AttachSampleUnits = shapes |> Array.map _.SampleUnits
      AttachSampleCount = shapes |> Array.map _.SampleCount
      AttachTimeNanos = shapes |> Array.map _.TimeNanos
      AttachDurationNanos = shapes |> Array.map _.DurationNanos
      AttachPeriodType = shapes |> Array.map _.PeriodType
      AttachPeriod = shapes |> Array.map _.Period
      AttachMappingCount = shapes |> Array.map _.MappingCount
      AttachLocationCount = shapes |> Array.map _.LocationCount
      AttachFunctionCount = shapes |> Array.map _.FunctionCount }

/// Datadog answers these intakes with an empty object and 202, whatever
/// happened to the body.
let private accepted = Response.json 202 "{}"

let private orUnknown (contentType: string) : string =
    if contentType = "" then "(no content-type)" else contentType

/// The form name of a part, from its Content-Disposition.
let private formName (contentDisposition: string) : string =
    let disposition = MediaType.parse (if isNull contentDisposition then "" else contentDisposition)

    if disposition.Type = "form-data" then
        disposition.Parameters.TryFind "name" |> Option.defaultValue ""
    else
        ""

/// The parts of a multipart request by form name, or why it could not be
/// read. A repeated name keeps its last part; no producer here repeats one,
/// so the warning is the alarm.
///
/// Unlike `Multipart.parts`, a body that breaks off is an error: the request
/// is then kept whole in raw_payloads instead of stored as half a submission.
let private readParts (r: Request) (label: string) : Result<Map<string, byte[]>, string> =
    let media = MediaType.parse (r.Header "Content-Type")

    if media.Error <> "" then
        Error media.Error
    elif not (media.Type.StartsWith "multipart/") then
        Error("content-type is " + media.Type)
    else
        match media.Parameters.TryFind "boundary" with
        | None
        | Some "" -> Error "content-type is multipart without boundary"
        | Some boundary ->
            try
                let reader = MultipartReader(boundary, new MemoryStream(r.Body))
                // The reader's defaults are for form posts; profiles and symbol files are larger.
                reader.HeadersLengthLimit <- 1024 * 1024
                reader.BodyLengthLimit <- Nullable()
                let mutable parts = Map.empty
                // Synchronous on purpose: the stream is memory, nothing waits.
                let mutable section = reader.ReadNextSectionAsync().GetAwaiter().GetResult()

                while not (isNull section) do
                    use data = new MemoryStream()
                    section.Body.CopyTo data
                    let name = formName section.ContentDisposition

                    if parts.ContainsKey name then
                        r.Log.LogWarning("[{Label}] part {Name} repeats, the later one replaces it", label, name)

                    parts <- parts.Add(name, data.ToArray())
                    section <- reader.ReadNextSectionAsync().GetAwaiter().GetResult()

                Ok parts
            with
            | :? IOException -> Error "the multipart body breaks off"
            | :? InvalidDataException as e -> Error e.Message

/// A part that should be a JSON object, decoded; a warning when it is not.
let private decodeObject (r: Request) (what: string) (data: byte[]) : JsonElement option =
    let decoded = Lenient.tryObject data

    if decoded.IsNone then
        r.Log.LogWarning("[{What}] not a JSON object ({Bytes} B)", what, data.Length)

    decoded

/// POST of a profile; `label` names the path it came by and is stored as the
/// row's variant.
let handleProfile (label: string) : Handler =
    fun r ->
        match readParts r label with
        | Error problem ->
            let contentType = r.Header "Content-Type"
            r.Log.LogWarning("[{Label}] not multipart ({Problem}), kept raw", label, problem)
            Raw.store r "profiling" "decode_error" $"not multipart, content-type {orUnknown contentType}: {problem}" r.Body
        | Ok parts ->
            // The event part carries the submission: which attachments
            // follow, the interval they cover, the tags of the process.
            let event = parts.TryFind "event" |> Option.bind (decodeObject r (label + " event"))
            let mutable profiles = Map.empty

            for part in parts do
                if part.Key <> "event" then
                    match readPprof part.Value with
                    | Some summary -> profiles <- profiles.Add(part.Key, summary)
                    | None -> r.Log.LogDebug("[{Label}] {Part} is not pprof ({Bytes} B)", label, part.Key, part.Value.Length)

            if r.Tenant <> "" then
                let row =
                    profileRow r.Tenant label (r.Header "Dd-Evp-Origin") (r.Header "Dd-Evp-Origin-Version") parts event profiles

                Sink.write r.Sink Profiles.table [| row |]

        accepted

let private isJsonSpace (b: byte) : bool =
    b = ' 'B || b = '\t'B || b = '\r'B || b = '\n'B

/// The bytes without the white space at either end: Unicode's white space,
/// not ASCII's.
let private trimSpace (data: byte[]) : byte[] =
    let mutable first = 0
    let mutable last = data.Length
    let mutable trimming = true

    while trimming && first < last do
        let status, rune, length = Rune.DecodeFromUtf8(ReadOnlySpan(data, first, last - first))

        if status = OperationStatus.Done && Rune.IsWhiteSpace rune then
            first <- first + length
        else
            trimming <- false

    trimming <- true

    while trimming && first < last do
        let status, rune, length = Rune.DecodeLastFromUtf8(ReadOnlySpan(data, first, last - first))

        if status = OperationStatus.Done && Rune.IsWhiteSpace rune then
            last <- last - length
        else
            trimming <- false

    data[first .. last - 1]

/// The entries of a Dynamic Instrumentation logs batch, in order; None when
/// the body is neither a JSON array nor NDJSON.
///
/// The agent's dyninst uploader and the tracers send an array, the browser
/// SDK's debugger track one object per line. Both reach this endpoint, so
/// the first significant byte decides.
let decodeLogs (body: byte[]) : byte[] list option =
    match body |> Array.tryFindIndex (fun b -> not (isJsonSpace b)) with
    | Some start when body[start] = '['B -> arrayElements body
    | Some start when body[start] = '{'B ->
        let lines = ResizeArray<byte[]>()
        let mutable lineStart = start

        for at in start .. body.Length do
            if at = body.Length || body[at] = '\n'B then
                let line = trimSpace body[lineStart .. at - 1]

                if line.Length > 0 then
                    lines.Add line

                lineStart <- at + 1

        Some(List.ofSeq lines)
    | _ -> None

/// The entry's top-level keys other than the two with a column of their
/// own: there is no schema for these entries, so everything else is "extra".
let private extraKeys (decoded: JsonElement option) : string[] =
    match decoded with
    | Some object ->
        object.EnumerateObject()
        |> Seq.map Lenient.nameOf
        |> Seq.filter (fun name -> name <> "service" && name <> "ddsource")
        |> Seq.distinct
        |> Seq.sort
        |> Array.ofSeq
    | None -> [||]

/// One probe status message as a row. `decoded` is None when that one entry
/// is not a JSON object: its columns stay empty, `message` still has its bytes.
///
/// The shape is the agent's uploader.DiagnosticMessage: service, ddsource,
/// timestamp, and the probe under debugger.diagnostics.
let diagnosticRow
    (tenant: string)
    (arrival: DateTime)
    (tags: Map<string, string[]>)
    (message: byte[])
    (decoded: JsonElement option)
    : DebuggerDiagnosticRow =
    let diagnostics = decoded |> field "debugger" |> field "diagnostics"

    let failure =
        field "exception" diagnostics |> Option.filter (fun e -> e.ValueKind = JsonValueKind.Object)

    { TenantID = tenant
      ReceivedAt = arrival
      Timestamp = parseTimestamp (field "timestamp" decoded)
      Service = asString (field "service" decoded)
      DDSource = asString (field "ddsource" decoded)
      DDTags = tags
      RuntimeID = asString (field "runtimeId" diagnostics)
      ProbeID = asString (field "probeId" diagnostics)
      Status = asString (field "status" diagnostics)
      ProbeVersion = asString (field "probeVersion" diagnostics)
      ExceptionType = failure |> Option.map (fun _ -> asString (field "type" failure))
      ExceptionMessage = failure |> Option.map (fun _ -> asString (field "message" failure))
      Message = message }

/// What the file part of a symdb upload holds.
type SymdbFile =
    { /// The JSON envelope {service, version, language, upload_id,
      /// batch_num, scopes, final}; None when the part is not gzip or not a
      /// JSON object.
      Envelope: JsonElement option
      /// How many top-level scopes; None when "scopes" is missing or
      /// malformed, which the envelope survives.
      ScopeCount: int option
      InflatedSize: int }

/// The most a symdb file may inflate to. The encoder flushes a batch at
/// about 2 MiB compressed; symbol JSON compresses well, but not this well.
let private symdbMaxInflated = 256L * 1024L * 1024L

let decodeSymdbFile (file: byte[]) : SymdbFile =
    match gunzip symdbMaxInflated file with
    | None -> { Envelope = None; ScopeCount = None; InflatedSize = 0 }
    | Some inflated ->
        let envelope = Lenient.tryObject inflated

        let scopeCount =
            match field "scopes" envelope with
            | Some scopes when scopes.ValueKind = JsonValueKind.Array ->
                let scopes = List.ofSeq (scopes.EnumerateArray())

                let allScopes =
                    scopes
                    |> List.forall (fun s -> s.ValueKind = JsonValueKind.Object || s.ValueKind = JsonValueKind.Null)

                if allScopes then Some scopes.Length else None
            | _ -> None

        { Envelope = envelope; ScopeCount = scopeCount; InflatedSize = inflated.Length }

/// A member of the file's envelope as text: a string without its quotes,
/// anything else as the JSON it is.
let private envelopeText (name: string) (envelope: JsonElement option) : string =
    match field name envelope with
    | Some value when value.ValueKind = JsonValueKind.String -> Lenient.text value
    | Some value -> value.GetRawText()
    | None -> ""

/// One symdb_uploads row. `event` is the event part as it arrived, `decoded`
/// what it decoded to (None when it is not a JSON object).
let symdbUploadRow
    (tenant: string)
    (event: byte[])
    (file: byte[])
    (ddtags: string)
    (decoded: JsonElement option)
    (symdb: SymdbFile)
    : SymdbUploadRow =
    { TenantID = tenant
      ReceivedAt = DateTime.UtcNow
      Event = event
      DDTags = Tags.toMultiMap (Tags.splitDDTags ddtags)
      Service = asString (field "service" decoded)
      Version = asString (field "version" decoded)
      Language = asString (field "language" decoded)
      RuntimeID = asString (field "runtimeId" decoded)
      // Identifiers, read as the text on the wire: a producer that sends
      // uploadId as a bare number must not lose it.
      UploadID = rawText (field "uploadId" decoded)
      BatchNum = rawText (field "batchNum" decoded)
      Final = asFlag (field "final" decoded)
      AttachmentSize = asCount (field "attachmentSize" decoded)
      File = file
      InflatedSize = uint64 symdb.InflatedSize
      ScopeCount = uint32 (defaultArg symdb.ScopeCount 0)
      ScopesOK = Text.flag symdb.ScopeCount.IsSome
      EnvService = envelopeText "service" symdb.Envelope
      EnvVersion = envelopeText "version" symdb.Envelope
      EnvLanguage = envelopeText "language" symdb.Envelope
      EnvUploadID = envelopeText "upload_id" symdb.Envelope
      EnvBatchNum = envelopeText "batch_num" symdb.Envelope
      EnvFinal = envelopeText "final" symdb.Envelope }

let private storeLogs (r: Request) (ddtags: string) (entries: byte[] list) : unit =
    let tags = Tags.toMultiMap (Tags.splitDDTags ddtags)
    let arrival = DateTime.UtcNow

    let rows: DebuggerLogRow list =
        entries
        |> List.map (fun entry ->
            // Best effort: an entry that is not an object keeps its bytes
            // and has no columns.
            let decoded = Lenient.tryObject entry

            { TenantID = r.Tenant
              ReceivedAt = arrival
              Service = asString (field "service" decoded)
              DDSource = asString (field "ddsource" decoded)
              DDTags = tags
              Entry = entry
              ExtraKeys = extraKeys decoded })

    Sink.write r.Sink DebuggerLogs.table (Array.ofList rows)

let private storeDiagnostics (r: Request) (ddtags: string) (messages: byte[] list) : unit =
    let tags = Tags.toMultiMap (Tags.splitDDTags ddtags)
    let arrival = DateTime.UtcNow

    let rows =
        messages
        |> List.mapi (fun i message ->
            let decoded = Lenient.tryObject message

            if decoded.IsNone then
                r.Log.LogWarning("[debugger] diagnostics: entry {Index} is not a JSON object", i)

            diagnosticRow r.Tenant arrival tags message decoded)

    Sink.write r.Sink DebuggerDiagnostics.table (Array.ofList rows)

/// POST /api/v2/debugger: one path, three producers, told apart by
/// Content-Type and part names.
///
///   not multipart          Dynamic Instrumentation snapshots and logs
///   multipart: event only  probe diagnostics, a JSON array
///   multipart: file+event  symbol database: gzip JSON, then its metadata
///
/// Each variant has a table of its own.
let handleDebugger: Handler =
    fun r ->
        let contentType = r.Header "Content-Type"
        let ddtags = r.Query "ddtags"

        if (MediaType.parse contentType).Type <> "multipart/form-data" then
            match decodeLogs r.Body with
            | None ->
                r.Log.LogWarning("[debugger] logs: neither a JSON array nor NDJSON, kept raw")
                Raw.store r "debugger" "decode_error" $"logs variant: not a JSON array or NDJSON, content-type {orUnknown contentType}" r.Body
            | Some entries ->
                if r.Tenant <> "" then
                    storeLogs r ddtags entries
        else
            match readParts r "debugger" with
            | Error problem ->
                r.Log.LogWarning("[debugger] multipart: {Problem}, kept raw", problem)
                Raw.store r "debugger" "decode_error" $"multipart, content-type {orUnknown contentType}: {problem}" r.Body
            | Ok parts ->
                match parts.TryFind "file", parts.TryFind "event" with
                | Some file, Some event ->
                    let decoded = decodeObject r "debugger symdb event" event
                    let symdb = decodeSymdbFile file

                    if symdb.Envelope.IsNone then
                        r.Log.LogWarning("[debugger] symdb file is not a gzip-compressed JSON object ({Bytes} B)", file.Length)

                    if r.Tenant <> "" then
                        Sink.write r.Sink SymdbUploads.table [| symdbUploadRow r.Tenant event file ddtags decoded symdb |]
                | None, Some event ->
                    match arrayElements event with
                    | None ->
                        // Not an array at all, unlike an empty batch: `[]`
                        // stores nothing and is not an error.
                        r.Log.LogWarning("[debugger] diagnostics: the event part is not a JSON array, kept raw")
                        Raw.store r "debugger" "decode_error" "diagnostics variant: event part is not a JSON array" r.Body
                    | Some messages ->
                        if r.Tenant <> "" then
                            storeDiagnostics r ddtags messages
                | _ ->
                    let names = String.Join(",", parts.Keys)
                    r.Log.LogWarning("[debugger] multipart with neither file nor event, parts: {Parts}", names)
                    Raw.store r "debugger" "unexpected_shape" $"multipart with neither file nor event parts: {names}" r.Body

        accepted

/// The class and byte order from the ELF ident bytes, without parsing the
/// file. Both "" when the bytes are not ELF.
let elfInfo (data: byte[]) : string * string =
    if data.Length < 6 || data[0] <> 0x7Fuy || data[1] <> 'E'B || data[2] <> 'L'B || data[3] <> 'F'B then
        "", ""
    else
        let elfClass =
            match data[4] with
            | 1uy -> "ELF32"
            | 2uy -> "ELF64"
            | _ -> "ELF?"

        let endianness =
            match data[5] with
            | 1uy -> "LE"
            | 2uy -> "BE"
            | _ -> "?"

        elfClass, endianness

/// One symbol_uploads row. `meta` is the decoded event part (None when
/// absent or not a JSON object): the host profiler's
/// symbolUploadRequestMetadata.
let symbolUploadRow (tenant: string) (parts: Map<string, byte[]>) (meta: JsonElement option) : SymbolUploadRow =
    let elf = parts.TryFind "elf_symbol_file"

    let elfClass, endianness =
        match elf with
        | Some data -> elfInfo data
        | None -> "", ""

    { TenantID = tenant
      ReceivedAt = DateTime.UtcNow
      Type = asString (field "type" meta)
      Arch = asString (field "arch" meta)
      GNUBuildID = asString (field "gnu_build_id" meta)
      GoBuildID = asString (field "go_build_id" meta)
      FileHash = asString (field "file_hash" meta)
      SymbolSource = asString (field "symbol_source" meta)
      Origin = asString (field "origin" meta)
      OriginVersion = asString (field "origin_version" meta)
      Filename = asString (field "filename" meta)
      Meta = parts.TryFind "event" |> Option.defaultValue [||]
      HasELF = Text.flag elf.IsSome
      ELFClass = elfClass
      ELFEndianness = endianness
      ELF = defaultArg elf [||]
      ELFSize = uint64 (defaultArg elf [||]).Length
      // A producer that sends a part this handler does not name still shows.
      OtherParts = parts |> Map.remove "event" |> Map.remove "elf_symbol_file" }

/// POST /api/v2/srcmap: an ELF symbol file (elf_symbol_file) and its
/// metadata (event), usually zstd-compressed as a whole. The agent sends it
/// only after /api/v2/profiles/symbols/query said the symbols are missing.
let handleSourcemap (r: Request) : Response =
    match readParts r "srcmap" with
    | Error problem ->
        let contentType = r.Header "Content-Type"
        r.Log.LogWarning("[srcmap] multipart: {Problem}, kept raw", problem)
        Raw.store r "srcmap" "decode_error" $"multipart, content-type {orUnknown contentType}: {problem}" r.Body
    | Ok parts ->
        let meta = parts.TryFind "event" |> Option.bind (decodeObject r "srcmap event")

        if r.Tenant <> "" then
            Sink.write r.Sink SymbolUploads.table [| symbolUploadRow r.Tenant parts meta |]

    accepted
