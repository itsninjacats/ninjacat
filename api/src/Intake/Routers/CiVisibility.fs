/// CI Visibility / Test Optimization.
///
///   citestcycle-intake.<site>  POST /api/v2/citestcycle   msgpack (or JSON)
///   citestcov-intake.<site>    POST /api/v2/citestcov     multipart + msgpack
///   api.<site>                 what a tracer consults, git metadata, the datadog-ci CLI
///
/// Most of the api.<site> endpoints are READS: the tracer asks what to do —
/// is coverage on, which tests may be skipped — and then behaves according to
/// the answer. A wrong answer here changes what a customer's test suite does,
/// so every answer keeps the field names of the client struct that parses it,
/// and the safe answer is always "feature off, list empty".
///
/// A tracer reaches these directly, or through the local agent's EVP proxy,
/// which rewrites the host and adds headers of its own (see `agentHeaders`).
///
/// The backend's own field catalogue is not public, so the decoders know
/// every field a shipped client writes (dd-trace-go, dd-trace-py, datadog-ci),
/// and each table keeps a raw column for the rest.
module NinjaCat.Api.Intake.Routers.CiVisibility

open System
open System.Globalization
open System.IO
open System.Security.Cryptography
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open System.Threading.Tasks
open MessagePack
open Microsoft.Extensions.Logging
open Microsoft.AspNetCore.Http
open Oxpecker
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// A decoded payload value. A test-cycle payload arrives as msgpack or as
/// JSON, and both become this, so the row builders are written once.
///
/// The rule that shapes it: a 64-bit id never passes through a float. A
/// session id above 2^53 would come back rounded, and the row would then join
/// to nothing. So a JSON number is kept as its literal, and msgpack integers
/// stay integers.
[<RequireQualifiedAccess>]
type Value =
    | Null
    | Bool of bool
    | Int of int64
    | UInt of uint64
    | Float32 of float32
    | Float of float
    /// A JSON number, as it was written.
    | Number of literal: string
    | String of string
    | Bytes of byte[]
    | List of Value list
    | Object of Map<string, Value>
    /// A msgpack extension.
    | Ext of extType: sbyte * data: byte[]

module Value =
    let rec ofMsgpack (value: MsgValue) : Value =
        match value with
        | MsgNil -> Value.Null
        | MsgBool b -> Value.Bool b
        | MsgInt n -> Value.Int n
        | MsgUInt n -> Value.UInt n
        | MsgFloat32 f -> Value.Float32 f
        | MsgFloat f -> Value.Float f
        | MsgStr s -> Value.String s
        | MsgBin b -> Value.Bytes b
        | MsgArray items -> Value.List(List.map ofMsgpack items)
        // A repeated key keeps its last value.
        | MsgMap entries -> Value.Object(entries |> List.fold (fun found (key, v) -> Map.add key (ofMsgpack v) found) Map.empty)
        | MsgExt(extType, data) -> Value.Ext(extType, data)

    let rec ofJson (value: JsonElement) : Value =
        match value.ValueKind with
        | JsonValueKind.Object ->
            Value.Object(value.EnumerateObject() |> Seq.fold (fun found p -> Map.add p.Name (ofJson p.Value) found) Map.empty)
        | JsonValueKind.Array -> Value.List(value.EnumerateArray() |> Seq.map ofJson |> List.ofSeq)
        | JsonValueKind.String -> Value.String(value.GetString())
        | JsonValueKind.Number -> Value.Number(value.GetRawText())
        | JsonValueKind.True -> Value.Bool true
        | JsonValueKind.False -> Value.Bool false
        | _ -> Value.Null

    /// One key of an object. Null when the key is absent, and when the value
    /// is not an object at all.
    let field (key: string) (value: Value) : Value =
        match value with
        | Value.Object fields -> fields.TryFind key |> Option.defaultValue Value.Null
        | _ -> Value.Null

    /// Whether an object has the key, whatever its value.
    let has (key: string) (value: Value) : bool =
        match value with
        | Value.Object fields -> fields.ContainsKey key
        | _ -> false

    let rec private write (writer: Utf8JsonWriter) (value: Value) : unit =
        match value with
        | Value.Null -> writer.WriteNullValue()
        | Value.Bool b -> writer.WriteBooleanValue b
        | Value.Int n -> writer.WriteNumberValue n
        | Value.UInt n -> writer.WriteNumberValue n
        | Value.Float32 f -> writer.WriteNumberValue f
        | Value.Float f -> writer.WriteNumberValue f
        | Value.Number literal -> writer.WriteRawValue(literal, true)
        | Value.String s -> writer.WriteStringValue s
        | Value.Bytes data -> writer.WriteBase64StringValue(ReadOnlySpan data)
        | Value.List items ->
            writer.WriteStartArray()

            for item in items do
                write writer item

            writer.WriteEndArray()
        | Value.Object fields ->
            writer.WriteStartObject()

            for pair in fields do
                writer.WritePropertyName pair.Key
                write writer pair.Value

            writer.WriteEndObject()
        | Value.Ext(extType, data) ->
            // An extension has no JSON form: its bytes and its type number.
            writer.WriteStartObject()
            writer.WriteBase64String("Data", ReadOnlySpan data)
            writer.WriteNumber("Type", int extType)
            writer.WriteEndObject()

    /// The value as JSON text, for a column that keeps it whole. Null is ""
    /// (no value at all).
    let toJson (value: Value) : string =
        match value with
        | Value.Null -> ""
        | _ ->
            try
                Json.write (fun writer -> write writer value)
            with :? ArgumentException ->
                // NaN and the infinities have no JSON form, so a value that
                // holds one has none either.
                ""

    /// The value as the text a column holds: exact, never rounded.
    let text (value: Value) : string =
        match value with
        | Value.Null -> ""
        | Value.String s -> s
        | Value.Bytes data -> Text.utf8 data
        | Value.Number literal -> literal
        | Value.Bool true -> "true"
        | Value.Bool false -> "false"
        | Value.Int n -> string n
        | Value.UInt n -> string n
        | Value.Float f -> Text.ofFloat f
        | Value.Float32 f -> Text.ofFloat32 f
        | Value.List _
        | Value.Object _
        | Value.Ext _ -> toJson value

    let fieldText (key: string) (value: Value) : string = text (field key value)

    /// The value as the bytes a column holds: binary data as it is, anything
    /// else as its text.
    let bytes (value: Value) : byte[] =
        match value with
        | Value.Bytes data -> data
        | other -> Encoding.UTF8.GetBytes(text other)

    let private twoTo53 = 9007199254740992.0

    /// The value as an id.
    ///
    /// A float is taken only when it is a whole number below 2^53: past that
    /// the float is already not the number that was sent, and 0 (which reads
    /// as "absent") beats a number that is almost right.
    let toUInt64 (value: Value) : uint64 =
        match value with
        | Value.UInt n -> n
        | Value.Int n -> if n < 0L then 0UL else uint64 n
        | Value.Number digits
        | Value.String digits ->
            match UInt64.TryParse(digits, NumberStyles.None, CultureInfo.InvariantCulture) with
            | true, n -> n
            | false, _ -> 0UL
        | Value.Float f -> if f < 0.0 || f <> Math.Truncate f || f >= twoTo53 then 0UL else uint64 f
        | _ -> 0UL

    /// The value as a signed integer, with the same rule for floats.
    let toInt64 (value: Value) : int64 =
        match value with
        | Value.Int n -> n
        | Value.UInt n -> if n > uint64 Int64.MaxValue then 0L else int64 n
        | Value.Number digits
        | Value.String digits ->
            match Int64.TryParse(digits, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
            | true, n -> n
            | false, _ -> 0L
        | Value.Float f -> if f <> Math.Truncate f || f >= twoTo53 || f <= -twoTo53 then 0L else int64 f
        | _ -> 0L

    /// For a Nullable column: None when the value is absent, so "the producer
    /// never said" stays apart from zero.
    let tryInt64 (value: Value) : int64 option =
        match value with
        | Value.Null -> None
        | other -> Some(toInt64 other)

    /// The value as a float, for a metrics map.
    let toFloat (value: Value) : float =
        match value with
        | Value.Float f -> f
        | Value.Float32 f -> float f
        | Value.Int n -> float n
        | Value.UInt n -> float n
        | Value.Number digits
        | Value.String digits ->
            let style = NumberStyles.AllowLeadingSign ||| NumberStyles.AllowDecimalPoint ||| NumberStyles.AllowExponent

            match digits.ToLowerInvariant() with
            | "inf"
            | "+inf"
            | "infinity"
            | "+infinity" -> infinity
            | "-inf"
            | "-infinity" -> -infinity
            | "nan" -> nan
            | _ ->
                match Double.TryParse(digits, style, CultureInfo.InvariantCulture) with
                | true, f -> f
                | false, _ -> 0.0
        | _ -> 0.0

    /// The elements of an array; none for anything else.
    let items (value: Value) : Value list =
        match value with
        | Value.List items -> items
        | _ -> []

    /// The value as a list of strings. A bare string counts as a list of one:
    /// several producers send a single label unwrapped.
    let strings (value: Value) : string[] =
        match value with
        | Value.List items -> items |> List.map text |> Array.ofList
        | Value.String "" -> [||]
        | Value.String s -> [| s |]
        | _ -> [||]

    /// An object as a string map, each value rendered exactly.
    let stringMap (value: Value) : Map<string, string> =
        match value with
        | Value.Object fields -> Map.map (fun _ v -> text v) fields
        | _ -> Map.empty

    /// An object as a numeric map, for a metrics column.
    let floatMap (value: Value) : Map<string, float> =
        match value with
        | Value.Object fields -> Map.map (fun _ v -> toFloat v) fields
        | _ -> Map.empty

    /// The keys of an object that `known` does not name, each value as JSON
    /// so a nested object survives.
    let unknown (known: Set<string>) (value: Value) : Map<string, string> =
        match value with
        | Value.Object fields -> fields |> Map.filter (fun key _ -> not (known.Contains key)) |> Map.map (fun _ v -> toJson v)
        | _ -> Map.empty

let private orUnknown (contentType: string) : string =
    if contentType = "" then "(no content-type)" else contentType

/// What a payload carries about the hop it took: direct, or through the
/// agent's EVP proxy.
type AgentHeaders =
    { Subdomain: string
      ContainerID: string
      Hostname: string
      AgentVersion: string }

/// The header names are the agent's real ones (pkg/trace/api/evp_proxy.go),
/// not the obvious ones: the container id has no X- prefix, and the version
/// is only in "Via: trace-agent <version>". X-Datadog-Container-Id and
/// X-Datadog-AgentVersion are sent by no agent; they are read in case a proxy
/// invented them. The proxy consumes X-Datadog-EVP-Subdomain, so a value
/// there means something other than a datadog-agent sent the request.
let agentHeaders (ctx: HttpContext) : AgentHeaders =
    let via = Ctx.header ctx "Via"

    let viaVersion =
        if via.StartsWith("trace-agent ", StringComparison.Ordinal) then
            via.Substring("trace-agent ".Length).Trim()
        else
            ""

    { Subdomain = Ctx.header ctx "X-Datadog-EVP-Subdomain"
      ContainerID = Text.firstNonEmpty [ Ctx.header ctx "Datadog-Container-Id"; Ctx.header ctx "X-Datadog-Container-Id" ]
      Hostname = Ctx.header ctx "X-Datadog-Hostname"
      AgentVersion = Text.firstNonEmpty [ Ctx.header ctx "X-Datadog-Agentversion"; viaVersion ] }

let private looksJson (body: byte[]) : bool =
    let first = body |> Array.tryFind (fun b -> b <> ' 'B && b <> '\t'B && b <> '\r'B && b <> '\n'B)
    first = Some '{'B || first = Some '['B

/// Decodes a body that may be msgpack or JSON; says which it was taken for.
///
/// Content-Type decides when it names one of the two, otherwise the first
/// byte does: JSON starts with '{' or '[', and no msgpack map or array header
/// shares those bytes. Sniffing matters because the two shipped clients label
/// the coverage part differently.
let decodeAny (contentType: string) (body: byte[]) : string * Result<Value, string> =
    let msgpack () = "msgpack", Msgpack.decode body |> Result.map Value.ofMsgpack
    let json () = "json", Json.tryParseFirst body |> Result.map Value.ofJson
    let declared = contentType.ToLowerInvariant()

    if declared.Contains "msgpack" then msgpack ()
    elif declared.Contains "json" then json ()
    elif looksJson body then json ()
    else msgpack ()

/// The envelope's metadata: event type → tags, where "*" holds the defaults
/// for every type (dd-trace-py puts language, runtime-id, library_version and
/// env there). Entries that are not a non-empty object are dropped.
let private cycleMetadata (value: Value) : Map<string, Map<string, string>> =
    match value with
    | Value.Object entries -> entries |> Map.map (fun _ entry -> Value.stringMap entry) |> Map.filter (fun _ tags -> not tags.IsEmpty)
    | _ -> Map.empty

/// The "*" entry with the event type's own entry on top: the more specific
/// statement wins. Flattened onto each row so a row can be read without the
/// payload it came in.
let private metadataFor (metadata: Map<string, Map<string, string>>) (eventType: string) : Map<string, string> =
    let entry (name: string) = metadata.TryFind name |> Option.defaultValue Map.empty
    Map.fold (fun merged key value -> Map.add key value merged) (entry "*") (entry eventType)

/// A boolean the tracer wrote into the string meta map. Absent is None, not
/// false: a tracer with early flake detection off sends no test.is_new at
/// all, and "was not looking" must stay apart from "is not new".
let private flagTag (meta: Map<string, string>) (key: string) : uint8 option =
    match meta.TryFind key with
    | None -> None
    | Some written ->
        let value = written.Trim().ToLowerInvariant()
        Some(Text.flag (value = "true" || value = "1" || value = "yes"))

/// A number the tracer may have put in either map: a numeric tag goes to
/// metrics, a string one to meta, and test.source.start has been seen in both.
let private numberTag (meta: Map<string, string>) (metrics: Map<string, float>) (key: string) : int64 option =
    match metrics.TryFind key, meta.TryFind key with
    | Some number, _ -> Some(int64 number)
    | None, Some written ->
        match Int64.TryParse(written.Trim(), NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
        | true, n -> Some n
        | false, _ -> None
    | None, None -> None

/// One event as a row. The tag columns are copies of meta entries, for query
/// speed: a tracer that renames a tag loses a column and keeps the data.
let testEventRow
    (tenant: string)
    (receivedAt: DateTime)
    (headers: AgentHeaders)
    (payloadVersion: int32)
    (metadata: Map<string, Map<string, string>>)
    (event: Value)
    : CITestEventRow =
    let eventType = Value.fieldText "type" event
    let content = Value.field "content" event
    let meta = Value.stringMap (Value.field "meta" content)
    let metrics = Value.floatMap (Value.field "metrics" content)
    let merged = metadataFor metadata eventType

    let tag (key: string) : string =
        meta.TryFind key |> Option.defaultValue ""

    // These belong to the envelope; the span's own tag wins when it set one.
    let tagOrMetadata (key: string) : string =
        Text.firstNonEmpty [ tag key; merged.TryFind key |> Option.defaultValue "" ]

    { TenantID = tenant
      ReceivedAt = receivedAt
      EventType = eventType
      EventVersion = int32 (Value.toInt64 (Value.field "version" event))
      PayloadVersion = payloadVersion
      SessionID = Value.toUInt64 (Value.field "test_session_id" content)
      ModuleID = Value.toUInt64 (Value.field "test_module_id" content)
      SuiteID = Value.toUInt64 (Value.field "test_suite_id" content)
      TraceID = Value.toUInt64 (Value.field "trace_id" content)
      SpanID = Value.toUInt64 (Value.field "span_id" content)
      ParentID = Value.toUInt64 (Value.field "parent_id" content)
      ITRCorrelationID = Value.fieldText "itr_correlation_id" content
      Service = Value.fieldText "service" content
      Env = tagOrMetadata "env"
      Name = Value.fieldText "name" content
      Resource = Value.fieldText "resource" content
      SpanType = Value.fieldText "type" content
      // Not defaulted to now when absent: a row dated 1970 says the producer
      // sent no start, and an invented time would look like a real one.
      Start = UnixNanos(Value.toInt64 (Value.field "start" content))
      DurationNs = Value.toInt64 (Value.field "duration" content)
      Error = int32 (Value.toInt64 (Value.field "error" content))
      TestName = tag "test.name"
      TestSuite = tag "test.suite"
      TestModule = tag "test.module"
      TestFramework = tag "test.framework"
      TestFrameworkVersion = tag "test.framework_version"
      TestStatus = tag "test.status"
      TestType = tag "test.type"
      TestSourceFile = tag "test.source.file"
      TestSourceStart = numberTag meta metrics "test.source.start"
      TestSourceEnd = numberTag meta metrics "test.source.end"
      TestParameters = tag "test.parameters"
      TestCodeowners = tag "test.codeowners"
      TestCommand = tag "test.command"
      TestSessionName = tag "test_session.name"
      GitRepositoryURL = tag "git.repository_url"
      GitBranch = tag "git.branch"
      GitTag = tag "git.tag"
      GitCommitSHA = tag "git.commit.sha"
      GitCommitMessage = tag "git.commit.message"
      GitCommitAuthorName = tag "git.commit.author.name"
      GitCommitAuthorEmail = tag "git.commit.author.email"
      GitCommitAuthorDate = tag "git.commit.author.date"
      GitCommitCommitterName = tag "git.commit.committer.name"
      GitCommitCommitterEmail = tag "git.commit.committer.email"
      GitCommitCommitterDate = tag "git.commit.committer.date"
      CIProviderName = tag "ci.provider.name"
      CIPipelineID = tag "ci.pipeline.id"
      CIPipelineName = tag "ci.pipeline.name"
      CIPipelineNumber = tag "ci.pipeline.number"
      CIPipelineURL = tag "ci.pipeline.url"
      CIJobID = tag "ci.job.id"
      CIJobName = tag "ci.job.name"
      CIJobURL = tag "ci.job.url"
      CIStageName = tag "ci.stage.name"
      CIWorkspacePath = tag "ci.workspace_path"
      CINodeName = tag "ci.node.name"
      CINodeLabels = tag "ci.node.labels"
      OSPlatform = tag "os.platform"
      OSVersion = tag "os.version"
      OSArchitecture = tag "os.architecture"
      RuntimeName = tag "runtime.name"
      RuntimeVersion = tag "runtime.version"
      Language = tagOrMetadata "language"
      RuntimeID = tagOrMetadata "runtime-id"
      LibraryVersion = tagOrMetadata "library_version"
      TestIsNew = flagTag meta "test.is_new"
      TestIsRetry = flagTag meta "test.is_retry"
      TestIsModified = flagTag meta "test.is_modified"
      TestSkippedByITR = flagTag meta "test.skipped_by_itr"
      ITRUnskippable = flagTag meta "test.itr.unskippable"
      ITRForcedRun = flagTag meta "test.itr.forced_run"
      CodeCoverageEnabled = flagTag meta "test.code_coverage.enabled"
      TestRetryReason = tag "test.retry_reason"
      EarlyFlakeAbortReason = tag "test.early_flake.abort_reason"
      EVPSubdomain = headers.Subdomain
      ContainerID = headers.ContainerID
      AgentHostname = headers.Hostname
      AgentVersion = headers.AgentVersion
      Meta = meta
      Metrics = metrics
      Metadata = merged
      // JSON whatever the request's format was: it is the spelling the
      // tracer's own documentation uses for an event.
      Content = Value.toJson content }

let private storeTestCycle (body: byte[]) (ctx: HttpContext) : unit =
    let tenant = Ctx.tenant ctx

    let contentType = Ctx.header ctx "Content-Type"
    let format, decoded = decodeAny contentType body

    match decoded with
    | Error e ->
        Raw.store ctx "citestcycle" "decode_error" $"{format} envelope, content-type {orUnknown contentType}: {e}" body
    | Ok(Value.Object _ as envelope) ->
        let events = Value.items (Value.field "events" envelope)

        if events.IsEmpty then
            // Not an error — the encoder emits one when a flush finds nothing —
            // but not a shape with a published meaning either, so it is kept.
            Raw.store ctx "citestcycle" "unexpected_shape" "envelope carries no events" body
        else
            let payloadVersion = int32 (Value.toInt64 (Value.field "version" envelope))
            let metadata = cycleMetadata (Value.field "metadata" envelope)
            let now = DateTime.UtcNow
            let headers = agentHeaders ctx
            let rows = ResizeArray<CITestEventRow>()

            let notMaps = ResizeArray<int>()

            events
            |> List.iteri (fun i event ->
                match event with
                | Value.Object _ ->
                    rows.Add(testEventRow tenant now headers payloadVersion metadata event)
                | _ -> notMaps.Add i)

            // The body is kept once, however many of its events are bad.
            if notMaps.Count > 0 then
                let places = notMaps |> Seq.truncate 8 |> Seq.map string |> String.concat ", "
                let more = if notMaps.Count > 8 then ", …" else ""
                Raw.store ctx "citestcycle" "decode_error" $"{notMaps.Count} of {events.Length} events are not maps: {places}{more}" body

            Ctx.write ctx CITestEvents.table (rows.ToArray())
    | Ok _ -> Raw.store ctx "citestcycle" "unexpected_shape" $"payload is not a {format} map" body

/// A test-cycle payload: {version, metadata, events[]}, whose events are the
/// tests, suites, modules and sessions of a CI run.
///
/// The tracer's transport treats any status below 400 as success and never
/// retries, so a payload refused here is simply gone. A body that does not
/// decode is therefore answered 202 and kept raw.
let handleTestCycle (body: byte[]) (ctx: HttpContext) : Task =
    if body.Length > 0 then
        storeTestCycle body ctx

    accepted ctx

/// One coverages[] element: its decoded value and the bytes it occupied.
/// The bytes are kept per entry, not per request: a payload holds one entry
/// per test, and storing the whole part on every row would cost the square of
/// its size.
type CoverageEntry = { Value: Value; Raw: byte[] }

let private coverageEntriesJson (data: byte[]) : Result<int32 * CoverageEntry list, string> =
    match Json.tryParseFirst data with
    | Error e -> Error e
    | Ok root ->
        let bad = JsonFields.Mismatches()
        let document = JsonFields.fields bad "" root
        // Read like the msgpack form: a version nobody can read is 0, and
        // never a reason to lose the coverage that came with it.
        let version =
            match document.Find "version" with
            | Some found -> int32 (Value.toInt64 (Value.ofJson found))
            | None -> 0

        let entries =
            document.Items "coverages"
            |> List.map (fun item -> { Value = Value.ofJson item; Raw = Json.rawBytes item })

        if bad.Count > 0 then Error bad[0] else Ok(version, entries)

/// Walks the msgpack document key by key, so each entry's exact bytes can be
/// cut out of the part: the bytes between the reader's position before and
/// after one value.
let coverageEntriesMsgpack (data: byte[]) : Result<int32 * CoverageEntry list, string> =
    let entries = ResizeArray<CoverageEntry>()
    let mutable version = 0
    let mutable reading = "envelope map header"
    let mutable problem: string option = None

    try
        let mutable reader = MessagePackReader(ReadOnlyMemory data)
        let keys = reader.ReadMapHeader()
        let mutable i = 0

        while problem.IsNone && i < keys do
            reading <- $"envelope key {i}"

            if reader.NextMessagePackType <> MessagePackType.String then
                problem <- Some "the key is not a string"
            else
                match reader.ReadString() with
                | "version" ->
                    // Read as any value: an encoder that changed this field's
                    // width or type must not make real coverage undecodable.
                    reading <- "version"
                    version <- int32 (Value.toInt64 (Value.ofMsgpack (Msgpack.read &reader)))
                | "coverages" ->
                    reading <- "coverages array header"
                    let count = reader.ReadArrayHeader()

                    for j in 0 .. count - 1 do
                        reading <- $"coverages[{j}]"
                        let start = int reader.Consumed
                        let value = Value.ofMsgpack (Msgpack.read &reader)

                        entries.Add
                            { Value = value
                              Raw = data[start .. int reader.Consumed - 1] }
                | other ->
                    // A key this decoder does not know is skipped, not refused: a
                    // newer encoder adding a field must not cost the coverage.
                    reading <- $"skipping \"{other}\""
                    reader.Skip()

            i <- i + 1

        match problem with
        | Some text -> Error $"{reading}: {text}"
        | None -> Ok(version, List.ofSeq entries)
    with
    | :? MessagePackSerializationException as e -> Error $"{reading}: {e.Message}"
    | :? EndOfStreamException -> Error $"{reading}: {Msgpack.truncated}"
    | :? OverflowException -> Error $"{reading}: {Msgpack.forged}"

/// Splits the coverage payload into its entries; says which format it was
/// taken for.
let private coverageEntries (contentType: string) (data: byte[]) : string * Result<int32 * CoverageEntry list, string> =
    let declared = contentType.ToLowerInvariant()

    if declared.Contains "json" || (not (declared.Contains "msgpack") && looksJson data) then
        "json", coverageEntriesJson data
    else
        "msgpack", coverageEntriesMsgpack data

let private coverageKnownKeys = set [ "test_session_id"; "test_suite_id"; "span_id"; "files" ]

/// One entry as a row. The bitmap is never decoded: by dd-trace-go's own
/// contract the backend stores it and hands it back as the bytes it got.
let coverageRow
    (tenant: string)
    (receivedAt: DateTime)
    (headers: AgentHeaders)
    (event: byte[])
    (payloadVersion: int32)
    (format: string)
    (entry: CoverageEntry)
    : CICoverageRow =
    let files = Value.items (Value.field "files" entry.Value)

    { TenantID = tenant
      ReceivedAt = receivedAt
      PayloadVersion = payloadVersion
      SessionID = Value.toUInt64 (Value.field "test_session_id" entry.Value)
      SuiteID = Value.toUInt64 (Value.field "test_suite_id" entry.Value)
      // The encoder leaves span_id out in suite-skipping mode.
      SpanID =
        match Value.field "span_id" entry.Value with
        | Value.Null -> None
        | id -> Some(Value.toUInt64 id)
      FilesFilename = files |> List.map (Value.fieldText "filename") |> Array.ofList
      FilesBitmap = files |> List.map (fun file -> Value.bytes (Value.field "bitmap" file)) |> Array.ofList
      Raw = entry.Raw
      RawFormat = format
      Event = event
      Extra = Value.unknown coverageKnownKeys entry.Value
      EVPSubdomain = headers.Subdomain
      ContainerID = headers.ContainerID
      AgentHostname = headers.Hostname
      AgentVersion = headers.AgentVersion }

let private partNames (parts: MultipartPart list) : string =
    if parts.IsEmpty then "none" else String.Join(", ", parts |> List.map _.Name)

let private storeCoverage (body: byte[]) (ctx: HttpContext) : unit =
    let tenant = Ctx.tenant ctx

    let contentType = Ctx.header ctx "Content-Type"

    match Multipart.tryParts contentType body with
    | Error e ->
        Raw.store ctx "citestcov" "decode_error" $"multipart, content-type {orUnknown contentType}: {e}" body
    | Ok parts ->
        // The coverage part is "coveragex" to dd-trace-go and "coverage1" to
        // dd-trace-py, so it cannot be found by name: it is the first part
        // that is not the event.
        match parts |> List.tryFind (fun p -> p.Name <> "event") with
        | None ->
            Raw.store ctx "citestcov" "unexpected_shape" $"multipart carries no coverage part (only {partNames parts})" body
        | Some coverage ->
            let format, entries = coverageEntries coverage.ContentType coverage.Data

            match entries with
            | Error e ->
                Raw.store ctx "citestcov" "decode_error" $"coverage part ({orUnknown coverage.ContentType}): {e}" body
            | Ok(payloadVersion, entries) ->
                let event =
                    match parts |> List.tryFindBack (fun p -> p.Name = "event") with
                    | Some part -> part.Data
                    | None -> [||]

                let now = DateTime.UtcNow
                let headers = agentHeaders ctx
                let rows = entries |> List.map (coverageRow tenant now headers event payloadVersion format)
                Ctx.write ctx CICoverage.table (Array.ofList rows)

/// A code-coverage upload: multipart with a dummy JSON "event" part and the
/// coverage payload, msgpack or JSON:
///
///   {"version": 2, "coverages": [{"test_session_id", "test_suite_id",
///     "span_id", "files": [{"filename", "bitmap"}]}]}
let handleTestCov (body: byte[]) (ctx: HttpContext) : Task =
    if body.Length > 0 then
        storeCoverage body ctx

    accepted ctx

/// A tracer's question: the JSON:API envelope {"data": {"id", "type",
/// "attributes"}} and the attributes the five configuration endpoints send.
/// They overlap and no endpoint sends all of them.
type ConfigRequest =
    { ID: string
      Type: string
      Service: string
      Env: string
      RepositoryURL: string
      Branch: string
      Sha: string
      TestLevel: string
      Module: string
      CommitMessage: string
      /// The configurations object as it was written.
      Configurations: string
      PageState: string }

let private noConfigRequest: ConfigRequest =
    { ID = ""
      Type = ""
      Service = ""
      Env = ""
      RepositoryURL = ""
      Branch = ""
      Sha = ""
      TestLevel = ""
      Module = ""
      CommitMessage = ""
      Configurations = ""
      PageState = "" }

/// Reads a question; also says which part did not decode ("envelope" or
/// "attributes") and why. What did decode is returned either way, except
/// that attributes are not read under a broken envelope.
let readConfigRequest (body: byte[]) : ConfigRequest * (string * string) option =
    match Json.tryParse body with
    | Error e -> noConfigRequest, Some("envelope", e)
    | Ok root ->
        let bad = JsonFields.Mismatches()
        let data = (JsonFields.fields bad "" root).Object "data"

        let envelope =
            { noConfigRequest with
                ID = data.String "id"
                Type = data.String "type" }

        if bad.Count > 0 then
            envelope, Some("envelope", bad[0])
        else
            let attributes = data.Object "attributes"

            let request =
                { envelope with
                    Service = attributes.String "service"
                    Env = attributes.String "env"
                    RepositoryURL = attributes.String "repository_url"
                    Branch = attributes.String "branch"
                    Sha = attributes.String "sha"
                    TestLevel = attributes.String "test_level"
                    Module = attributes.String "module"
                    CommitMessage = attributes.String "commit_message"
                    Configurations = attributes.Raw "configurations"
                    PageState = (attributes.Object "page_info").String "page_state" }

            request, (if bad.Count > 0 then Some("attributes", bad[0]) else None)

/// Answers a configuration question and keeps both the question and the
/// answer. A body that does not parse still gets a row and a well-formed
/// answer: "a tracer sends something we cannot read" is what the table is
/// for, and a 4xx here is terminal for the client.
let answerConfig (endpoint: string) (answer: ConfigRequest -> string) (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    let request, problem = readConfigRequest body

    match problem with
    | Some(part, error) ->
        Raw.store ctx "cisettings" "decode_error" $"{endpoint} {part}: {error}" body
    | None -> ()

    let response = answer request

    Ctx.write
        ctx
        CISettingsRequests.table
        [| { TenantID = tenant
             ReceivedAt = DateTime.UtcNow
             Endpoint = endpoint
             RequestID = request.ID
             RequestType = request.Type
             Service = request.Service
             Env = request.Env
             RepositoryURL = request.RepositoryURL
             Branch = request.Branch
             SHA = request.Sha
             TestLevel = request.TestLevel
             Module = request.Module
             CommitMessage = request.CommitMessage
             PageState = request.PageState
             Configurations = request.Configurations
             RequestBody = body
             ResponseBody = response } |]

    // The answer was built as JSON text, so that the row keeps exactly what was sent.
    (setContentType "application/json; charset=utf-8" >=> bytes (Encoding.UTF8.GetBytes response)) ctx

/// Which Test Optimization features are on: none, on purpose. Each flag
/// gates one of the other configuration endpoints in the tracer, so with all
/// of them false a tracer still posts its results but never asks what to
/// skip. Turning one on before the feature exists makes a customer's suite
/// skip tests on data we do not have. `require_git` false also stops a
/// tracer unshallowing its clone to upload packfiles.
///
/// The keys are dd-trace-go's settingsResponse (settings_api.go).
let settingsAnswer (request: ConfigRequest) : string =
    """{"data":{"id":""" + Json.quoted request.ID + ""","type":"ci_app_test_service_libraries_settings","attributes":{"code_coverage":false,"coverage_report_upload_enabled":false,"early_flake_detection":{"enabled":false,"slow_test_retries":{"5s":0,"10s":0,"30s":0,"5m":0},"faulty_session_threshold":null},"flaky_test_retries_enabled":false,"itr_enabled":false,"require_git":false,"tests_skipping":false,"known_tests_enabled":false,"impacted_tests_enabled":false,"test_management":{"enabled":false,"attempt_to_fix_retries":0}}}}"""

/// Which tests may be skipped: none. `meta` is sent although empty, because
/// dd-trace-py treats a missing meta as a malformed response. When this is
/// implemented, meta.coverage holds base64 of the bitmaps of a citestcov
/// upload, unchanged: they are in ci_coverage.files_bitmap.
///
/// The keys are dd-trace-go's skippableResponse (skippable.go).
let skippableAnswer (_: ConfigRequest) : string =
    """{"meta":{"correlation_id":"","coverage":{}},"data":[]}"""

/// Which tests has this service run before: none.
///
/// `has_next` MUST be false: the client keeps POSTing for as long as it is
/// true, and dd-trace-go has no iteration cap, so a true that never moves
/// the cursor spins a customer's test process forever.
///
/// The flaky-tests path gets the same answer. No current client names that
/// path (older dd-trace-py did); an empty list is harmless whatever its real
/// shape is, a 404 in the middle of a test run is not.
///
/// The keys are dd-trace-go's knownTestsResponse (known_tests_api.go).
let testListAnswer (request: ConfigRequest) : string =
    """{"data":{"id":""" + Json.quoted request.ID + ""","type":"ci_app_libraries_tests","attributes":{"tests":{},"page_info":{"cursor":"","size":0,"has_next":false}}}}"""

/// Which tests are quarantined, disabled or being fixed: none. The keys are
/// dd-trace-go's testManagementTestsResponse.
let testManagementAnswer (request: ConfigRequest) : string =
    """{"data":{"id":""" + Json.quoted request.ID + ""","type":"ci_app_libraries_tests","attributes":{"modules":{}}}}"""

/// The commit list of a search_commits request: the repository and the shas
/// the client asked about.
let private readSearchCommits (body: byte[]) : Result<string * string list, string> =
    match Json.tryParse body with
    | Error e -> Error e
    | Ok root ->
        let bad = JsonFields.Mismatches()
        let document = JsonFields.fields bad "" root

        let shas =
            document.Objects "data"
            |> List.map (fun commit ->
                // Read only so that a type that is not text is noted.
                commit.String "type" |> ignore
                commit.String "id")

        let repository = (document.Object "meta").String "repository_url"

        if bad.Count > 0 then Error bad[0] else Ok(repository, shas)

/// Records the shas a client asked about and answers that we hold none.
///
/// "Which of these do you have" is a read, and the intake has no read path.
/// Answering none makes the client upload its packs, which is what we want
/// while git_packfiles is the only place those objects can come from.
///
/// The answer is JSON even for a request that does not decode: the client
/// retries a 200 of any other content type, and a 4xx is terminal.
let handleSearchCommits (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx

    match readSearchCommits body with
    | Error e ->
        Raw.store ctx "gitmeta" "decode_error" $"search_commits: {e}" body
        json {| data = List.empty<string>; meta = {| repository_url = "" |} |} ctx
    | Ok(repository, shas) ->
        let now = DateTime.UtcNow

        let rows: GitCommitRow list =
            shas
            |> List.filter (fun sha -> sha <> "")
            |> List.map (fun sha ->
                { TenantID = tenant
                  RepositoryURL = repository
                  SHA = sha
                  SeenAt = now
                  // The client is ASKING about this sha, which says
                  // nothing about whether we hold its objects.
                  PackfileID = None
                  Source = "search_commits" })

        Ctx.write ctx GitCommits.table (Array.ofList rows)

        json {| data = List.empty<string>; meta = {| repository_url = repository |} |} ctx

/// The JSON part of a packfile upload: {"data": {"id", "type"}, "meta":
/// {"repository_url"}}. The sha and the repository are None when the part
/// does not set them.
type private PushedSha =
    { Sha: string option
      Repository: string option
      /// False when any of the part did not decode.
      Decoded: bool }

let private readPushedSha (data: byte[]) : PushedSha =
    match Json.tryParse data with
    | Error _ -> { Sha = None; Repository = None; Decoded = false }
    | Ok root ->
        let bad = JsonFields.Mismatches()
        let document = JsonFields.fields bad "" root
        let commit = document.Object "data"
        commit.String "type" |> ignore
        let sha = commit.OptionalString "id"
        let repository = (document.Object "meta").OptionalString "repository_url"
        { Sha = sha; Repository = repository; Decoded = bad.Count = 0 }

/// One git packfile: multipart with a "pushedSha" JSON part and a "packfile"
/// part, one request per .pack file.
///
/// The answer is 204 with no body: dd-trace-go accepts any 2xx, dd-trace-py
/// checks for exactly 204, so 204 is the one status both call success.
let handlePackfile (body: byte[]) (ctx: HttpContext) : Task =
    let tenant = Ctx.tenant ctx
    let log = Ctx.log ctx

    let contentType = Ctx.header ctx "Content-Type"

    match Multipart.tryParts contentType body with
    | Error e ->
        Raw.store ctx "gitmeta" "decode_error" $"packfile multipart, content-type {orUnknown contentType}: {e}" body
        setStatusCode 204 ctx
    | Ok parts ->
        let mutable sha = ""
        let mutable repository = ""
        let mutable pack: byte[] option = None
        let mutable filename = ""
        let mutable other: Map<string, byte[]> = Map.empty

        for part in parts do
            match part.Name with
            | "pushedSha" ->
                // A repeated part changes only what it sets.
                let pushed = readPushedSha part.Data
                sha <- defaultArg pushed.Sha sha
                repository <- defaultArg pushed.Repository repository

                if not pushed.Decoded then
                    log.LogWarning("[gitmeta] packfile pushedSha did not decode")
                    other <- other.Add("pushedSha.undecodable", part.Data)
            | "packfile" ->
                pack <- Some part.Data
                filename <- part.FileName
            | name -> other <- other.Add(name, part.Data)

        match pack with
        | None ->
            Raw.store ctx "gitmeta" "unexpected_shape" "packfile upload without a packfile part" body
        | Some pack ->
            let packfileID = Convert.ToHexStringLower(SHA256.HashData pack)
            let now = DateTime.UtcNow

            Ctx.write
                ctx
                GitPackfiles.table
                [| { TenantID = tenant
                     ReceivedAt = now
                     RepositoryURL = repository
                     PushedSHA = sha
                     PackfileID = packfileID
                     Filename = filename
                     Packfile = pack
                     SizeBytes = uint64 pack.Length
                     OtherParts = other } |]

            // The pushed sha also becomes a git_commits row, this time WITH a
            // packfile id: the one path where "we hold the objects" is true.
            // ReplacingMergeTree lets it replace the NULL a search_commits
            // sighting left behind.
            if sha <> "" then
                Ctx.write
                    ctx
                    GitCommits.table
                    [| { TenantID = tenant
                         RepositoryURL = repository
                         SHA = sha
                         SeenAt = now
                         PackfileID = Some packfileID
                         Source = "packfile" } |]

        setStatusCode 204 ctx
let private pipelineKnownKeys =
    set [ "ci_env"; "ci_level"; "provider"; "tags"; "metrics"; "measures"; "name"; "start_time"; "end_time" ]

let private storePipelineEvent (kind: string) (body: byte[]) (ctx: HttpContext) : unit =
    let tenant = Ctx.tenant ctx

    let problems = JsonFields.Mismatches()

    let data =
        match Json.tryParse body with
        | Ok root -> (JsonFields.fields problems "" root).Object "data"
        | Error e ->
            problems.Add e
            JsonFields.Fields(problems, "data", JsonElement())

    let dataType = data.String "type"

    if problems.Count > 0 then
        Raw.store ctx "cipipeline" "decode_error" $"{kind} envelope: {problems[0]}" body
    else
        let attributes =
            match data.Find "attributes" with
            | Some written -> Value.ofJson written
            | None -> Value.Null

        // `measure` calls the map "metrics"; a custom span may call the same
        // idea "measures".
        let metrics = Value.floatMap (Value.field "metrics" attributes)

        Ctx.write
            ctx
            CIPipelineEvents.table
            [| { TenantID = tenant
                 ReceivedAt = DateTime.UtcNow
                 Kind = kind
                 DataType = dataType
                 Provider = Value.fieldText "provider" attributes
                 CILevel = Value.tryInt64 (Value.field "ci_level" attributes)
                 CIEnv = Value.stringMap (Value.field "ci_env" attributes)
                 Tags = Value.stringMap (Value.field "tags" attributes)
                 Metrics = (if metrics.IsEmpty then Value.floatMap (Value.field "measures" attributes) else metrics)
                 SpanName = Value.fieldText "name" attributes
                 SpanStartRaw = Value.fieldText "start_time" attributes
                 SpanEndRaw = Value.fieldText "end_time" attributes
                 Attributes = data.Raw "attributes"
                 Body = body
                 Extra = Value.unknown pipelineKnownKeys attributes } |]

/// One datadog-ci call: `tag`, `measure` or `trace`. All three share the
/// envelope {"data": {"type", "attributes"}}; `kind` says which route it
/// arrived on. The custom-span attributes were not read from the CLI's
/// source: the span columns are filled from the obvious key names.
///
/// 202 whatever happened: the CLI retries five times on an error, so a 5xx
/// would stall somebody's CI job for half a minute.
let handlePipelineEvent (kind: string) (body: byte[]) (ctx: HttpContext) : Task =
    if body.Length > 0 then
        storePipelineEvent kind body ctx

    accepted ctx
