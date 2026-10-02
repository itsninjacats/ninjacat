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
open System.Threading.Tasks
open Microsoft.Net.Http.Headers
open Microsoft.Extensions.Logging
open Microsoft.AspNetCore.Http
open Oxpecker
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// An epoch number as a time. Producers send seconds, milliseconds,
/// microseconds or nanoseconds; the magnitude says which, as it would to a
/// person reading the number.
let private epochTime (n: int64) : DateTime =
    if n >= 1_000_000_000_000_000_000L then
        (Time.fromUnixSeconds (n / 1_000_000_000L)).AddTicks(n % 1_000_000_000L / 100L)
    elif n >= 1_000_000_000_000_000L then
        (Time.fromUnixSeconds (n / 1_000_000L)).AddTicks(n % 1_000_000L * 10L)
    elif n >= 1_000_000_000_000L then
        Time.fromUnixMillis n
    else
        Time.fromUnixSeconds n

/// A profiler event's start or end, or a diagnostic's timestamp: RFC 3339
/// text from Datadog's profiler, an epoch number from other producers. None
/// means "could not tell", never a guessed time: the row keeps the original
/// value next to this one.
let parseTimestamp (value: JsonElement option) : DateTime option =
    match value with
    | Some v when v.ValueKind = JsonValueKind.String -> Time.tryRfc3339 (v.GetString())
    | Some v when v.ValueKind = JsonValueKind.Number ->
        match v.TryGetInt64() with
        | true, n when n > 0L -> Some(epochTime n)
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
        // end, without complaint, and half a profile must not pass for a
        // whole one. A whole stream ends with the size of what it inflates to.
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
      StartRaw = Json.text (Json.child "start" event)
      StartParsed = parseTimestamp (Json.child "start" event)
      EndRaw = Json.text (Json.child "end" event)
      EndParsed = parseTimestamp (Json.child "end" event)
      Family = Json.lenientString (Json.child "family" event)
      Version = Json.lenientString (Json.child "version" event)
      Runtime = Json.lenientString (Json.child "runtime" event)
      Language = Json.lenientString (Json.child "language" event)
      // A third tag convention: one comma-joined string.
      TagsProfiler = Tags.toMultiMap (Tags.splitDDTags (Json.lenientString (Json.child "tags_profiler" event)))
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

let private orUnknown (contentType: string) : string =
    if contentType = "" then "(no content-type)" else contentType

/// The parts of a multipart request by form name, or why it could not be
/// read. A repeated name keeps its last part; no producer here repeats one,
/// so the warning is the alarm.
let private readParts (body: byte[]) (ctx: HttpContext) (label: string) : Result<Map<string, byte[]>, string> =
    let log = Ctx.log ctx

    match Multipart.tryParts (Ctx.header ctx "Content-Type") body with
    | Error problem -> Error problem
    | Ok parts ->
        let mutable byName = Map.empty

        for part in parts do
            if byName.ContainsKey part.Name then
                log.LogWarning("[{Label}] part {Name} repeats, the later one replaces it", label, part.Name)

            byName <- byName.Add(part.Name, part.Data)

        Ok byName

let private isMultipartForm (contentType: string) : bool =
    match MediaTypeHeaderValue.TryParse contentType with
    | true, media -> media.MediaType.Equals("multipart/form-data", StringComparison.OrdinalIgnoreCase)
    | false, _ -> false

/// A part that should be a JSON object, decoded; a warning when it is not.
let private decodeObject (ctx: HttpContext) (what: string) (data: byte[]) : JsonElement option =
    let log = Ctx.log ctx

    let decoded = Json.tryParseObject data

    if decoded.IsNone then
        log.LogWarning("[{What}] not a JSON object ({Bytes} B)", what, data.Length)

    decoded

/// POST of a profile; `label` names the path it came by and is stored as the
/// row's variant.
let handleProfile (label: string) : byte[] -> EndpointHandler =
    fun body ctx ->
        let tenant = Ctx.tenant ctx
        let log = Ctx.log ctx

        match readParts body ctx label with
        | Error problem ->
            let contentType = Ctx.header ctx "Content-Type"
            Raw.store ctx "profiling" "decode_error" $"not multipart, content-type {orUnknown contentType}: {problem}" body
        | Ok parts ->
            // The event part carries the submission: which attachments
            // follow, the interval they cover, the tags of the process.
            let event = parts.TryFind "event" |> Option.bind (decodeObject ctx (label + " event"))
            let mutable profiles = Map.empty

            for part in parts do
                if part.Key <> "event" then
                    match readPprof part.Value with
                    | Some summary -> profiles <- profiles.Add(part.Key, summary)
                    | None -> log.LogDebug("[{Label}] {Part} is not pprof ({Bytes} B)", label, part.Key, part.Value.Length)

            let row =
                profileRow tenant label (Ctx.header ctx "Dd-Evp-Origin") (Ctx.header ctx "Dd-Evp-Origin-Version") parts event profiles

            Ctx.write ctx Profiles.table [| row |]

        accepted ctx

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
    | Some start when body[start] = '['B -> Json.tryParseElementBytes body |> Result.toOption
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
        |> Seq.map _.Name
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
    let diagnostics = decoded |> Json.child "debugger" |> Json.child "diagnostics"

    let failure =
        Json.child "exception" diagnostics |> Option.filter (fun e -> e.ValueKind = JsonValueKind.Object)

    { TenantID = tenant
      ReceivedAt = arrival
      Timestamp = parseTimestamp (Json.child "timestamp" decoded)
      Service = Json.lenientString (Json.child "service" decoded)
      DDSource = Json.lenientString (Json.child "ddsource" decoded)
      DDTags = tags
      RuntimeID = Json.lenientString (Json.child "runtimeId" diagnostics)
      ProbeID = Json.lenientString (Json.child "probeId" diagnostics)
      Status = Json.lenientString (Json.child "status" diagnostics)
      ProbeVersion = Json.lenientString (Json.child "probeVersion" diagnostics)
      ExceptionType = failure |> Option.map (fun _ -> Json.lenientString (Json.child "type" failure))
      ExceptionMessage = failure |> Option.map (fun _ -> Json.lenientString (Json.child "message" failure))
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
        let envelope = Json.tryParseObject inflated

        let scopeCount =
            match Json.child "scopes" envelope with
            | Some scopes when scopes.ValueKind = JsonValueKind.Array ->
                let scopes = List.ofSeq (scopes.EnumerateArray())

                let allScopes =
                    scopes
                    |> List.forall (fun s -> s.ValueKind = JsonValueKind.Object || s.ValueKind = JsonValueKind.Null)

                if allScopes then Some scopes.Length else None
            | _ -> None

        { Envelope = envelope; ScopeCount = scopeCount; InflatedSize = inflated.Length }

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
      Service = Json.lenientString (Json.child "service" decoded)
      Version = Json.lenientString (Json.child "version" decoded)
      Language = Json.lenientString (Json.child "language" decoded)
      RuntimeID = Json.lenientString (Json.child "runtimeId" decoded)
      // Identifiers, read as the text on the wire: a producer that sends
      // uploadId as a bare number must not lose it.
      UploadID = Json.text (Json.child "uploadId" decoded)
      BatchNum = Json.text (Json.child "batchNum" decoded)
      Final = Json.flag (Json.child "final" decoded)
      AttachmentSize = Json.child "attachmentSize" decoded |> Option.bind Json.uint64Of
      File = file
      InflatedSize = uint64 symdb.InflatedSize
      ScopeCount = uint32 (defaultArg symdb.ScopeCount 0)
      ScopesOK = Text.flag symdb.ScopeCount.IsSome
      EnvService = Json.text (Json.child "service" symdb.Envelope)
      EnvVersion = Json.text (Json.child "version" symdb.Envelope)
      EnvLanguage = Json.text (Json.child "language" symdb.Envelope)
      EnvUploadID = Json.text (Json.child "upload_id" symdb.Envelope)
      EnvBatchNum = Json.text (Json.child "batch_num" symdb.Envelope)
      EnvFinal = Json.text (Json.child "final" symdb.Envelope) }

let private storeLogs (ctx: HttpContext) (ddtags: string) (entries: byte[] list) : unit =
    let tenant = Ctx.tenant ctx

    let tags = Tags.toMultiMap (Tags.splitDDTags ddtags)
    let arrival = DateTime.UtcNow

    let rows: DebuggerLogRow list =
        entries
        |> List.map (fun entry ->
            // Best effort: an entry that is not an object keeps its bytes
            // and has no columns.
            let decoded = Json.tryParseObject entry

            { TenantID = tenant
              ReceivedAt = arrival
              Service = Json.lenientString (Json.child "service" decoded)
              DDSource = Json.lenientString (Json.child "ddsource" decoded)
              DDTags = tags
              Entry = entry
              ExtraKeys = extraKeys decoded })

    Ctx.write ctx DebuggerLogs.table (Array.ofList rows)

let private storeDiagnostics (ctx: HttpContext) (ddtags: string) (messages: byte[] list) : unit =
    let tenant = Ctx.tenant ctx
    let log = Ctx.log ctx

    let tags = Tags.toMultiMap (Tags.splitDDTags ddtags)
    let arrival = DateTime.UtcNow

    let rows =
        messages
        |> List.mapi (fun i message ->
            let decoded = Json.tryParseObject message

            if decoded.IsNone then
                log.LogWarning("[debugger] diagnostics: entry {Index} is not a JSON object", i)

            diagnosticRow tenant arrival tags message decoded)

    Ctx.write ctx DebuggerDiagnostics.table (Array.ofList rows)

/// POST /api/v2/debugger: one path, three producers, told apart by
/// Content-Type and part names.
///
///   not multipart          Dynamic Instrumentation snapshots and logs
///   multipart: event only  probe diagnostics, a JSON array
///   multipart: file+event  symbol database: gzip JSON, then its metadata
///
/// Each variant has a table of its own.
let handleDebugger: byte[] -> EndpointHandler =
    fun body ctx ->
        let tenant = Ctx.tenant ctx
        let log = Ctx.log ctx

        let contentType = Ctx.header ctx "Content-Type"
        let ddtags = Ctx.query ctx "ddtags"

        if not (isMultipartForm contentType) then
            match decodeLogs body with
            | None ->
                Raw.store ctx "debugger" "decode_error" $"logs variant: not a JSON array or NDJSON, content-type {orUnknown contentType}" body
            | Some entries ->
                storeLogs ctx ddtags entries
        else
            match readParts body ctx "debugger" with
            | Error problem ->
                Raw.store ctx "debugger" "decode_error" $"multipart, content-type {orUnknown contentType}: {problem}" body
            | Ok parts ->
                match parts.TryFind "file", parts.TryFind "event" with
                | Some file, Some event ->
                    let decoded = decodeObject ctx "debugger symdb event" event
                    let symdb = decodeSymdbFile file

                    if symdb.Envelope.IsNone then
                        log.LogWarning("[debugger] symdb file is not a gzip-compressed JSON object ({Bytes} B)", file.Length)

                    Ctx.write ctx SymdbUploads.table [| symdbUploadRow tenant event file ddtags decoded symdb |]
                | None, Some event ->
                    match Json.tryParseElementBytes event |> Result.toOption with
                    | None ->
                        // Not an array at all, unlike an empty batch: `[]`
                        // stores nothing and is not an error.
                        Raw.store ctx "debugger" "decode_error" "diagnostics variant: event part is not a JSON array" body
                    | Some messages ->
                        storeDiagnostics ctx ddtags messages
                | _ ->
                    let names = String.Join(",", parts.Keys)
                    Raw.store ctx "debugger" "unexpected_shape" $"multipart with neither file nor event parts: {names}" body

        accepted ctx

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
      Type = Json.lenientString (Json.child "type" meta)
      Arch = Json.lenientString (Json.child "arch" meta)
      GNUBuildID = Json.lenientString (Json.child "gnu_build_id" meta)
      GoBuildID = Json.lenientString (Json.child "go_build_id" meta)
      FileHash = Json.lenientString (Json.child "file_hash" meta)
      SymbolSource = Json.lenientString (Json.child "symbol_source" meta)
      Origin = Json.lenientString (Json.child "origin" meta)
      OriginVersion = Json.lenientString (Json.child "origin_version" meta)
      Filename = Json.lenientString (Json.child "filename" meta)
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
let handleSourcemap (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    match readParts body ctx "srcmap" with
    | Error problem ->
        let contentType = Ctx.header ctx "Content-Type"
        Raw.store ctx "srcmap" "decode_error" $"multipart, content-type {orUnknown contentType}: {problem}" body
    | Ok parts ->
        let meta = parts.TryFind "event" |> Option.bind (decodeObject ctx "srcmap event")

        Ctx.write ctx SymbolUploads.table [| symbolUploadRow tenant parts meta |]

    accepted ctx
