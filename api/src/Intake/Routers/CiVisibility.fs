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
open System.Numerics
open System.IO
open System.Security.Cryptography
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open System.Text.RegularExpressions
open System.Text.Unicode
open MessagePack
open Microsoft.AspNetCore.WebUtilities
open Microsoft.Extensions.Logging
open Microsoft.Net.Http.Headers
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

/// JSON parsed the way Go's decoder took it, where .NET's differs.
///
/// Go reads bytes that are not UTF-8, and an escape that is half a surrogate
/// pair, as U+FFFD. .NET fails on both, and only when the string is read,
/// long after the document parsed. So such a body is repaired before parsing.
/// Go also nests far deeper than .NET's default of 64.
module GoJson =
    /// A whole surrogate pair, half of one, or any other escape.
    let private escape =
        Regex(
            @"\\u(d[89ab][0-9a-f]{2})\\u(d[c-f][0-9a-f]{2})|\\u(d[89a-f][0-9a-f]{2})|\\.",
            RegexOptions.IgnoreCase ||| RegexOptions.Singleline ||| RegexOptions.Compiled
        )

    let private maxDepth = Json.maxDepth

    let private repaired (body: byte[]) : byte[] =
        let span = ReadOnlySpan<byte> body

        let mayHoldSurrogate =
            MemoryExtensions.IndexOf(span, ReadOnlySpan<byte> "\\ud"B) >= 0
            || MemoryExtensions.IndexOf(span, ReadOnlySpan<byte> "\\uD"B) >= 0

        if Utf8.IsValid span && not mayHoldSurrogate then
            body
        else
            let text = Encoding.UTF8.GetString body
            Encoding.UTF8.GetBytes(escape.Replace(text, fun m -> if m.Groups[3].Success then "\\ufffd" else m.Value))

    /// A whole body as one JSON document.
    let parse (body: byte[]) : Result<JsonElement, string> =
        try
            use doc = JsonDocument.Parse(ReadOnlyMemory(repaired body), JsonDocumentOptions(MaxDepth = maxDepth))
            Ok(doc.RootElement.Clone())
        with :? JsonException as e ->
            Error e.Message

    /// The first JSON value of a body. What follows it is not looked at, as
    /// with Go's json.Decoder.
    let first (body: byte[]) : Result<JsonElement, string> =
        try
            let options = JsonReaderOptions(AllowMultipleValues = true, MaxDepth = maxDepth)
            let mutable reader = Utf8JsonReader(ReadOnlySpan(repaired body), options)

            if reader.Read() then Ok(JsonElement.ParseValue(&reader)) else Error "EOF"
        with :? JsonException as e ->
            Error e.Message

/// Go's strconv.FormatFloat(x, 'g', -1, bits), built from .NET's shortest
/// round-trip text of the same number: the same digits in Go's layout. It is
/// what the Go server stored when a float arrived where text was expected.
let private goFloatText (roundTrip: string) : string =
    match roundTrip with
    | "NaN" -> "NaN"
    | "Infinity" -> "+Inf"
    | "-Infinity" -> "-Inf"
    | _ ->
        let sign = if roundTrip.StartsWith '-' then "-" else ""
        let unsigned = roundTrip.TrimStart '-'

        let mantissa, exponent =
            match unsigned.IndexOf 'E' with
            | -1 -> unsigned, 0
            | i -> unsigned.Substring(0, i), int (unsigned.Substring(i + 1))

        let whole, fraction =
            match mantissa.IndexOf '.' with
            | -1 -> mantissa, ""
            | i -> mantissa.Substring(0, i), mantissa.Substring(i + 1)

        let written = whole + fraction
        let leadingZeros = written.Length - written.TrimStart('0').Length
        let digits = written.Trim '0'

        if digits = "" then
            sign + "0"
        else
            // The number is 0.<digits> × 10^point.
            let point = whole.Length + exponent - leadingZeros
            let exp = point - 1

            if exp < -4 || exp >= 6 then
                let rest = if digits.Length > 1 then "." + digits.Substring 1 else ""
                let expSign = if exp < 0 then "-" else "+"
                sign + digits.Substring(0, 1) + rest + "e" + expSign + (abs exp).ToString "00"
            elif point <= 0 then
                sign + "0." + String('0', -point) + digits
            elif point >= digits.Length then
                sign + digits + String('0', point - digits.Length)
            else
                sign + digits.Substring(0, point) + "." + digits.Substring point

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
        // A repeated key keeps its last value, as in a Go map.
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

    let private jsonWriterOptions =
        JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping)

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
            // The shape Go's encoder gave msgp's RawExtension.
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
                use buffer = new MemoryStream()

                do
                    use writer = new Utf8JsonWriter(buffer, jsonWriterOptions)
                    write writer value

                Encoding.UTF8.GetString(buffer.ToArray())
            with :? ArgumentException ->
                // NaN and the infinities have no JSON form; Go's encoder
                // refused the whole value too.
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
        | Value.Float f -> goFloatText (f.ToString("R", CultureInfo.InvariantCulture))
        | Value.Float32 f -> goFloatText (f.ToString("R", CultureInfo.InvariantCulture))
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

/// Go's encoding/json filling a struct, which is how the Go server read the
/// JSON documents below: property names match whatever their case, null
/// leaves a field as it was, and a value of the wrong type is an error that
/// does NOT stop the rest from being read. Callers keep what did decode; the
/// first entry of `problems` is the error.
module private GoStruct =
    /// The properties of an object in document order, names lower-cased. A
    /// name may repeat.
    type Fields = (string * JsonElement) list

    let private kindName (value: JsonElement) : string =
        match value.ValueKind with
        | JsonValueKind.Object -> "an object"
        | JsonValueKind.Array -> "an array"
        | JsonValueKind.String -> "a string"
        | JsonValueKind.Number -> "a number"
        | JsonValueKind.Null -> "null"
        | _ -> "a bool"

    let mismatch (problems: ResizeArray<string>) (value: JsonElement) (path: string) (expected: string) : unit =
        problems.Add $"json: {path} is {kindName value}, expected {expected}"

    let fields (problems: ResizeArray<string>) (path: string) (value: JsonElement) : Fields =
        match value.ValueKind with
        | JsonValueKind.Object -> value.EnumerateObject() |> Seq.map (fun p -> p.Name.ToLowerInvariant(), p.Value) |> List.ofSeq
        | JsonValueKind.Null -> []
        | _ ->
            mismatch problems value path "an object"
            []

    /// The last value given for a name, null included.
    let last (name: string) (parent: Fields) : JsonElement option =
        parent |> List.filter (fun (n, _) -> n = name) |> List.tryLast |> Option.map snd

    /// Every value given for a name that is not null, in order.
    let private given (name: string) (parent: Fields) : JsonElement list =
        parent |> List.filter (fun (n, v) -> n = name && v.ValueKind <> JsonValueKind.Null) |> List.map snd

    /// The last value given for a name that is not null.
    let lastSet (name: string) (parent: Fields) : JsonElement option = List.tryLast (given name parent)

    /// A nested object. A repeated name adds to what the earlier one set.
    let child (problems: ResizeArray<string>) (path: string) (name: string) (parent: Fields) : Fields =
        given name parent |> List.collect (fields problems path)

    /// A string property; None when the document does not set it.
    let tryText (problems: ResizeArray<string>) (path: string) (name: string) (parent: Fields) : string option =
        let mutable found = None

        for value in given name parent do
            if value.ValueKind = JsonValueKind.String then
                found <- Some(value.GetString())
            else
                mismatch problems value (path + name) "a string"

        found

    let text (problems: ResizeArray<string>) (path: string) (name: string) (parent: Fields) : string =
        tryText problems path name parent |> Option.defaultValue ""

    /// A property kept as it was written (Go's json.RawMessage), or "".
    let raw (name: string) (parent: Fields) : string =
        match last name parent with
        | Some value -> value.GetRawText()
        | None -> ""

let private accepted = Response.json 202 "{}"

let private orUnknown (contentType: string) : string =
    if contentType = "" then "(no content-type)" else contentType

/// A JSON string literal, for building an answer by hand.
let private quoted (text: string) : string =
    "\"" + JsonEncodedText.Encode(text, JavaScriptEncoder.UnsafeRelaxedJsonEscaping).ToString() + "\""

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
let agentHeaders (r: Request) : AgentHeaders =
    let via = r.Header "Via"

    let viaVersion =
        if via.StartsWith("trace-agent ", StringComparison.Ordinal) then
            via.Substring("trace-agent ".Length).Trim()
        else
            ""

    { Subdomain = r.Header "X-Datadog-EVP-Subdomain"
      ContainerID = Text.firstNonEmpty [ r.Header "Datadog-Container-Id"; r.Header "X-Datadog-Container-Id" ]
      Hostname = r.Header "X-Datadog-Hostname"
      AgentVersion = Text.firstNonEmpty [ r.Header "X-Datadog-Agentversion"; viaVersion ] }

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
    let json () = "json", GoJson.first body |> Result.map Value.ofJson
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

let private storeTestCycle (r: Request) : unit =
    let contentType = r.Header "Content-Type"
    let format, decoded = decodeAny contentType r.Body

    match decoded with
    | Error e ->
        r.Log.LogWarning("[citestcycle] {Format} decode: {Error} ({Bytes} bytes)", format, e, r.Body.Length)
        Raw.store r "citestcycle" "decode_error" $"{format} envelope, content-type {orUnknown contentType}: {e}" r.Body
    | Ok(Value.Object _ as envelope) ->
        let events = Value.items (Value.field "events" envelope)

        if events.IsEmpty then
            // Not an error — the encoder emits one when a flush finds nothing —
            // but not a shape with a published meaning either, so it is kept.
            Raw.store r "citestcycle" "unexpected_shape" "envelope carries no events" r.Body
        else
            let payloadVersion = int32 (Value.toInt64 (Value.field "version" envelope))
            let metadata = cycleMetadata (Value.field "metadata" envelope)
            let now = DateTime.UtcNow
            let headers = agentHeaders r
            let rows = ResizeArray<CITestEventRow>()

            let notMaps = ResizeArray<int>()

            events
            |> List.iteri (fun i event ->
                match event with
                | Value.Object _ ->
                    if r.Tenant <> "" then
                        rows.Add(testEventRow r.Tenant now headers payloadVersion metadata event)
                | _ -> notMaps.Add i)

            // The body is kept once, however many of its events are bad.
            if notMaps.Count > 0 then
                let places = notMaps |> Seq.truncate 8 |> Seq.map string |> String.concat ", "
                let more = if notMaps.Count > 8 then ", …" else ""
                Raw.store r "citestcycle" "decode_error" $"{notMaps.Count} of {events.Length} events are not maps: {places}{more}" r.Body

            Sink.write r.Sink CITestEvents.table (rows.ToArray())
    | Ok _ -> Raw.store r "citestcycle" "unexpected_shape" $"payload is not a {format} map" r.Body

/// A test-cycle payload: {version, metadata, events[]}, whose events are the
/// tests, suites, modules and sessions of a CI run.
///
/// The tracer's transport treats any status below 400 as success and never
/// retries, so a payload refused here is simply gone. A body that does not
/// decode is therefore answered 202 and kept raw.
let handleTestCycle (r: Request) : Response =
    if r.Body.Length > 0 then
        storeTestCycle r

    accepted

/// Like `Multipart.parts`, with two differences the Go server had. A body
/// that breaks off is an error, so the upload is kept raw whole instead of
/// being stored in part. And a part's name counts only on a form-data
/// disposition, its file name without any directory.
let readParts (contentType: string) (body: byte[]) : Result<MultipartPart list, string> =
    match MediaTypeHeaderValue.TryParse contentType with
    | false, _ -> Error(if contentType = "" then "mime: no media type" else "mime: malformed media type")
    | true, media ->
        let mediaType = media.MediaType.Value.ToLowerInvariant()

        if not (mediaType.StartsWith("multipart/", StringComparison.Ordinal)) then
            Error $"content-type {mediaType} is not multipart"
        else
            match Multipart.boundary contentType with
            | None -> Error "multipart without boundary"
            | Some boundary ->
                let reader = MultipartReader(boundary, new MemoryStream(body))
                reader.HeadersLengthLimit <- 1024 * 1024
                reader.BodyLengthLimit <- Nullable()
                let found = ResizeArray<MultipartPart>()

                try
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

                                let name =
                                    if disposition.DispositionType.Equals("form-data", StringComparison.OrdinalIgnoreCase) then
                                        HeaderUtilities.RemoveQuotes(disposition.Name).Value |> Option.ofObj |> Option.defaultValue ""
                                    else
                                        ""

                                name, Path.GetFileName file
                            | _ -> "", ""

                        found.Add
                            { Name = name
                              FileName = fileName
                              ContentType = (if isNull section.ContentType then "" else section.ContentType)
                              Data = data.ToArray() }

                        section <- reader.ReadNextSectionAsync().GetAwaiter().GetResult()

                    Ok(List.ofSeq found)
                with e ->
                    Error $"multipart: {e.Message}"

/// One coverages[] element: its decoded value and the bytes it occupied.
/// The bytes are kept per entry, not per request: a payload holds one entry
/// per test, and storing the whole part on every row would cost the square of
/// its size.
type CoverageEntry = { Value: Value; Raw: byte[] }

let private jsonNumber =
    Regex(@"^-?(0|[1-9]\d*)(\.\d+)?([eE][+-]?\d+)?$", RegexOptions.Compiled)

/// The payload version of a JSON coverage upload. Go's ParseInt gives the
/// nearest limit for an integer past int64, and the Go server took it.
let private jsonVersion (literal: string) : int64 =
    match Int64.TryParse(literal, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
    | true, n -> n
    | false, _ ->
        match BigInteger.TryParse(literal, NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
        | true, big -> if big.Sign < 0 then Int64.MinValue else Int64.MaxValue
        | false, _ -> 0L

let private coverageEntriesJson (data: byte[]) : Result<int32 * CoverageEntry list, string> =
    match GoJson.first data with
    | Error e -> Error e
    | Ok root ->
        let problems = ResizeArray<string>()
        let document = GoStruct.fields problems "the payload" root

        let version =
            match GoStruct.lastSet "version" document with
            | None -> 0L
            | Some v when v.ValueKind = JsonValueKind.Number -> jsonVersion (v.GetRawText())
            // Go's json.Number also takes a number written as a string.
            | Some v when v.ValueKind = JsonValueKind.String && jsonNumber.IsMatch(v.GetString()) -> jsonVersion (v.GetString())
            | Some v ->
                GoStruct.mismatch problems v "version" "a number"
                0L

        let entries =
            match GoStruct.last "coverages" document with
            | None -> []
            | Some v when v.ValueKind = JsonValueKind.Null -> []
            | Some v when v.ValueKind = JsonValueKind.Array ->
                v.EnumerateArray()
                |> Seq.map (fun item ->
                    { Value = Value.ofJson item
                      Raw = Encoding.UTF8.GetBytes(item.GetRawText()) })
                |> List.ofSeq
            | Some v ->
                GoStruct.mismatch problems v "coverages" "an array"
                []

        if problems.Count > 0 then Error problems[0] else Ok(int32 version, entries)

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

let private storeCoverage (r: Request) : unit =
    let contentType = r.Header "Content-Type"

    match readParts contentType r.Body with
    | Error e ->
        r.Log.LogWarning("[citestcov] multipart: {Error}", e)
        Raw.store r "citestcov" "decode_error" $"multipart, content-type {orUnknown contentType}: {e}" r.Body
    | Ok parts ->
        // The coverage part is "coveragex" to dd-trace-go and "coverage1" to
        // dd-trace-py, so it cannot be found by name: it is the first part
        // that is not the event.
        match parts |> List.tryFind (fun p -> p.Name <> "event") with
        | None ->
            Raw.store r "citestcov" "unexpected_shape" $"multipart carries no coverage part (only {partNames parts})" r.Body
        | Some coverage ->
            let format, entries = coverageEntries coverage.ContentType coverage.Data

            match entries with
            | Error e ->
                r.Log.LogWarning("[citestcov] {Format} coverage payload: {Error} ({Bytes} bytes)", format, e, coverage.Data.Length)
                Raw.store r "citestcov" "decode_error" $"coverage part ({orUnknown coverage.ContentType}): {e}" r.Body
            | Ok(payloadVersion, entries) ->
                if r.Tenant <> "" then
                    let event =
                        match parts |> List.tryFindBack (fun p -> p.Name = "event") with
                        | Some part -> part.Data
                        | None -> [||]

                    let now = DateTime.UtcNow
                    let headers = agentHeaders r
                    let rows = entries |> List.map (coverageRow r.Tenant now headers event payloadVersion format)
                    Sink.write r.Sink CICoverage.table (Array.ofList rows)

/// A code-coverage upload: multipart with a dummy JSON "event" part and the
/// coverage payload, msgpack or JSON:
///
///   {"version": 2, "coverages": [{"test_session_id", "test_suite_id",
///     "span_id", "files": [{"filename", "bitmap"}]}]}
let handleTestCov (r: Request) : Response =
    if r.Body.Length > 0 then
        storeCoverage r

    accepted

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
    match GoJson.parse body with
    | Error e -> noConfigRequest, Some("envelope", e)
    | Ok root ->
        let problems = ResizeArray<string>()
        let document = GoStruct.fields problems "the document" root
        let data = GoStruct.child problems "data" "data" document

        let envelope =
            { noConfigRequest with
                ID = GoStruct.text problems "data." "id" data
                Type = GoStruct.text problems "data." "type" data }

        if problems.Count > 0 then
            envelope, Some("envelope", problems[0])
        else
            let attributes =
                match GoStruct.last "attributes" data with
                | Some written -> GoStruct.fields problems "data.attributes" written
                | None -> []

            let text (name: string) = GoStruct.text problems "data.attributes." name attributes

            let pageInfo =
                match GoStruct.last "page_info" attributes with
                | Some written -> GoStruct.fields problems "data.attributes.page_info" written
                | None -> []

            let request =
                { envelope with
                    Service = text "service"
                    Env = text "env"
                    RepositoryURL = text "repository_url"
                    Branch = text "branch"
                    Sha = text "sha"
                    TestLevel = text "test_level"
                    Module = text "module"
                    CommitMessage = text "commit_message"
                    Configurations = GoStruct.raw "configurations" attributes
                    PageState = GoStruct.text problems "data.attributes.page_info." "page_state" pageInfo }

            request, (if problems.Count > 0 then Some("attributes", problems[0]) else None)

/// Answers a configuration question and keeps both the question and the
/// answer. A body that does not parse still gets a row and a well-formed
/// answer: "a tracer sends something we cannot read" is what the table is
/// for, and a 4xx here is terminal for the client.
let answerConfig (endpoint: string) (answer: ConfigRequest -> string) (r: Request) : Response =
    let request, problem = readConfigRequest r.Body

    match problem with
    | Some(part, error) ->
        r.Log.LogWarning("[cisettings] {Endpoint}: {Part}: {Error} ({Bytes} bytes)", endpoint, part, error, r.Body.Length)
        Raw.store r "cisettings" "decode_error" $"{endpoint} {part}: {error}" r.Body
    | None -> ()

    let response = answer request

    if r.Tenant <> "" then
        Sink.write
            r.Sink
            CISettingsRequests.table
            [| { TenantID = r.Tenant
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
                 RequestBody = r.Body
                 ResponseBody = response } |]

    Response.json 200 response

/// Which Test Optimization features are on: none, on purpose. Each flag
/// gates one of the other configuration endpoints in the tracer, so with all
/// of them false a tracer still posts its results but never asks what to
/// skip. Turning one on before the feature exists makes a customer's suite
/// skip tests on data we do not have. `require_git` false also stops a
/// tracer unshallowing its clone to upload packfiles.
///
/// The keys are dd-trace-go's settingsResponse (settings_api.go).
let settingsAnswer (request: ConfigRequest) : string =
    """{"data":{"id":""" + quoted request.ID + ""","type":"ci_app_test_service_libraries_settings","attributes":{"code_coverage":false,"coverage_report_upload_enabled":false,"early_flake_detection":{"enabled":false,"slow_test_retries":{"5s":0,"10s":0,"30s":0,"5m":0},"faulty_session_threshold":null},"flaky_test_retries_enabled":false,"itr_enabled":false,"require_git":false,"tests_skipping":false,"known_tests_enabled":false,"impacted_tests_enabled":false,"test_management":{"enabled":false,"attempt_to_fix_retries":0}}}}"""

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
    """{"data":{"id":""" + quoted request.ID + ""","type":"ci_app_libraries_tests","attributes":{"tests":{},"page_info":{"cursor":"","size":0,"has_next":false}}}}"""

/// Which tests are quarantined, disabled or being fixed: none. The keys are
/// dd-trace-go's testManagementTestsResponse.
let testManagementAnswer (request: ConfigRequest) : string =
    """{"data":{"id":""" + quoted request.ID + ""","type":"ci_app_libraries_tests","attributes":{"modules":{}}}}"""

/// The commit list of a search_commits request: the repository and the shas
/// the client asked about.
let private readSearchCommits (body: byte[]) : Result<string * string list, string> =
    match GoJson.parse body with
    | Error e -> Error e
    | Ok root ->
        let problems = ResizeArray<string>()
        let document = GoStruct.fields problems "the document" root
        let shas = ResizeArray<string>()

        match GoStruct.last "data" document with
        | None -> ()
        | Some v when v.ValueKind = JsonValueKind.Null -> ()
        | Some v when v.ValueKind = JsonValueKind.Array ->
            let mutable i = 0

            for item in v.EnumerateArray() do
                let commit = GoStruct.fields problems $"data[{i}]" item
                GoStruct.text problems $"data[{i}]." "type" commit |> ignore
                shas.Add(GoStruct.text problems $"data[{i}]." "id" commit)
                i <- i + 1
        | Some v -> GoStruct.mismatch problems v "data" "an array"

        let meta = GoStruct.child problems "meta" "meta" document
        let repository = GoStruct.text problems "meta." "repository_url" meta

        if problems.Count > 0 then Error problems[0] else Ok(repository, List.ofSeq shas)

/// Records the shas a client asked about and answers that we hold none.
///
/// "Which of these do you have" is a read, and the intake has no read path.
/// Answering none makes the client upload its packs, which is what we want
/// while git_packfiles is the only place those objects can come from.
///
/// The answer is JSON even for a request that does not decode: the client
/// retries a 200 of any other content type, and a 4xx is terminal.
let handleSearchCommits (r: Request) : Response =
    match readSearchCommits r.Body with
    | Error e ->
        r.Log.LogWarning("[gitmeta] search_commits: {Error} ({Bytes} bytes)", e, r.Body.Length)
        Raw.store r "gitmeta" "decode_error" $"search_commits: {e}" r.Body
        Response.json 200 """{"data":[],"meta":{"repository_url":""}}"""
    | Ok(repository, shas) ->
        if r.Tenant <> "" then
            let now = DateTime.UtcNow

            let rows: GitCommitRow list =
                shas
                |> List.filter (fun sha -> sha <> "")
                |> List.map (fun sha ->
                    { TenantID = r.Tenant
                      RepositoryURL = repository
                      SHA = sha
                      SeenAt = now
                      // The client is ASKING about this sha, which says
                      // nothing about whether we hold its objects.
                      PackfileID = None
                      Source = "search_commits" })

            Sink.write r.Sink GitCommits.table (Array.ofList rows)

        Response.json 200 ("""{"data":[],"meta":{"repository_url":""" + quoted repository + "}}")

/// The JSON part of a packfile upload: {"data": {"id", "type"}, "meta":
/// {"repository_url"}}. The sha and the repository are None when the part
/// does not set them.
type private PushedSha =
    { Sha: string option
      Repository: string option
      /// False when any of the part did not decode.
      Decoded: bool }

let private readPushedSha (data: byte[]) : PushedSha =
    match GoJson.parse data with
    | Error _ -> { Sha = None; Repository = None; Decoded = false }
    | Ok root ->
        let problems = ResizeArray<string>()
        let document = GoStruct.fields problems "the document" root
        let commit = GoStruct.child problems "data" "data" document
        GoStruct.text problems "data." "type" commit |> ignore
        let sha = GoStruct.tryText problems "data." "id" commit
        let meta = GoStruct.child problems "meta" "meta" document
        let repository = GoStruct.tryText problems "meta." "repository_url" meta
        { Sha = sha; Repository = repository; Decoded = problems.Count = 0 }

/// One git packfile: multipart with a "pushedSha" JSON part and a "packfile"
/// part, one request per .pack file.
///
/// The answer is 204 with no body: dd-trace-go accepts any 2xx, dd-trace-py
/// checks for exactly 204, so 204 is the one status both call success.
let handlePackfile (r: Request) : Response =
    let contentType = r.Header "Content-Type"

    match readParts contentType r.Body with
    | Error e ->
        r.Log.LogWarning("[gitmeta] packfile multipart: {Error}", e)
        Raw.store r "gitmeta" "decode_error" $"packfile multipart, content-type {orUnknown contentType}: {e}" r.Body
        Response.status 204
    | Ok parts ->
        let mutable sha = ""
        let mutable repository = ""
        let mutable pack: byte[] option = None
        let mutable filename = ""
        let mutable other: Map<string, byte[]> = Map.empty

        for part in parts do
            match part.Name with
            | "pushedSha" ->
                // A repeated part changes only what it sets, as Go's decoder
                // did when it filled the same struct twice.
                let pushed = readPushedSha part.Data
                sha <- defaultArg pushed.Sha sha
                repository <- defaultArg pushed.Repository repository

                if not pushed.Decoded then
                    r.Log.LogWarning("[gitmeta] packfile pushedSha did not decode")
                    other <- other.Add("pushedSha.undecodable", part.Data)
            | "packfile" ->
                pack <- Some part.Data
                filename <- part.FileName
            | name -> other <- other.Add(name, part.Data)

        match pack with
        | None ->
            r.Log.LogWarning("[gitmeta] packfile upload without a packfile part")
            Raw.store r "gitmeta" "unexpected_shape" "packfile upload without a packfile part" r.Body
        | Some pack when r.Tenant <> "" ->
            let packfileID = Convert.ToHexStringLower(SHA256.HashData pack)
            let now = DateTime.UtcNow

            Sink.write
                r.Sink
                GitPackfiles.table
                [| { TenantID = r.Tenant
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
                Sink.write
                    r.Sink
                    GitCommits.table
                    [| { TenantID = r.Tenant
                         RepositoryURL = repository
                         SHA = sha
                         SeenAt = now
                         PackfileID = Some packfileID
                         Source = "packfile" } |]
        | Some _ -> ()

        Response.status 204

let private pipelineKnownKeys =
    set [ "ci_env"; "ci_level"; "provider"; "tags"; "metrics"; "measures"; "name"; "start_time"; "end_time" ]

let private storePipelineEvent (kind: string) (r: Request) : unit =
    let problems = ResizeArray<string>()

    let data =
        match GoJson.parse r.Body with
        | Error e ->
            problems.Add e
            []
        | Ok root -> GoStruct.child problems "data" "data" (GoStruct.fields problems "the document" root)

    let dataType = GoStruct.text problems "data." "type" data

    if problems.Count > 0 then
        r.Log.LogWarning("[cipipeline] {Kind}: {Error} ({Bytes} bytes)", kind, problems[0], r.Body.Length)
        Raw.store r "cipipeline" "decode_error" $"{kind} envelope: {problems[0]}" r.Body
    elif r.Tenant <> "" then
        let attributes =
            match GoStruct.last "attributes" data with
            | Some written -> Value.ofJson written
            | None -> Value.Null

        // `measure` calls the map "metrics"; a custom span may call the same
        // idea "measures".
        let metrics = Value.floatMap (Value.field "metrics" attributes)

        Sink.write
            r.Sink
            CIPipelineEvents.table
            [| { TenantID = r.Tenant
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
                 Attributes = GoStruct.raw "attributes" data
                 Body = r.Body
                 Extra = Value.unknown pipelineKnownKeys attributes } |]

/// One datadog-ci call: `tag`, `measure` or `trace`. All three share the
/// envelope {"data": {"type", "attributes"}}; `kind` says which route it
/// arrived on. The custom-span attributes were not read from the CLI's
/// source: the span columns are filled from the obvious key names.
///
/// 202 whatever happened: the CLI retries five times on an error, so a 5xx
/// would stall somebody's CI job for half a minute.
let handlePipelineEvent (kind: string) (r: Request) : Response =
    if r.Body.Length > 0 then
        storePipelineEvent kind r

    accepted
