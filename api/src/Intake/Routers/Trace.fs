/// trace.agent.<site> — the trace-agent.
///
///   agent config: apm_config.apm_dd_url
///
/// A separate binary from the node agent, which is why dd_url does not move it.
///
///   /api/v0.2/traces               -> spans                    one row per span
///   /api/v0.2/stats                -> apm_stats                one row per grouped stat
///   /api/v0.1/pipeline_stats       -> dsm_pipeline_stats       one row per StatsPoint
///                                     dsm_backlogs             one row per Backlog
///                                     dsm_bucket_transactions  one row per bucket with blobs
///   /api/v2/data_streams_messages  -> dsm_messages             one row per array element
///
/// A body that does not decode goes to raw_payloads under the intake "trace".
/// Not handled: /api/v2/apmtelemetry.
module NinjaCat.Api.Intake.Routers.Trace

open System
open System.Collections.Generic
open System.Globalization
open System.IO
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open Google.Protobuf
open Google.Protobuf.Collections
open Microsoft.Extensions.Logging
open Datadog.Trace
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers.TraceMsgp
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// The label of everything on this host in raw_payloads.
let private intake = "trace"

// --- JSON for the *_json columns ---

/// Go's encoding/json refuses NaN and the infinities. Carries the value as Go prints it.
exception private UnsupportedNumber of name: string

/// Writes one of the *_json columns. A number JSON cannot hold makes the
/// whole column an error marker, as it did in Go: an empty column would read
/// as "no attributes".
let private jsonText (write: Utf8JsonWriter -> unit) : string =
    use buffer = new MemoryStream()

    try
        do
            use writer = new Utf8JsonWriter(buffer, JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping))
            write writer

        Encoding.UTF8.GetString(buffer.ToArray())
    with UnsupportedNumber name ->
        $"{{\"_encode_error\":\"json: unsupported value: {name}\"}}"

let private writeDouble (writer: Utf8JsonWriter) (value: float) : unit =
    if Double.IsNaN value then raise (UnsupportedNumber "NaN")
    elif Double.IsPositiveInfinity value then raise (UnsupportedNumber "+Inf")
    elif Double.IsNegativeInfinity value then raise (UnsupportedNumber "-Inf")
    else writer.WriteNumberValue value

/// Bytes as base64, the only portable way to put them in JSON.
let private writeBytes (writer: Utf8JsonWriter) (bytes: byte[]) : unit =
    writer.WriteBase64StringValue(ReadOnlySpan bytes)

/// An attribute value tagged with its wire type: {"int":5}. The tag is the
/// point: a bare JSON value would turn 5, 5.0 and "5" into the same thing.
let private writeTagged (writer: Utf8JsonWriter) (tag: string) (writeValue: unit -> unit) : unit =
    writer.WriteStartObject()
    writer.WritePropertyName tag
    writeValue ()
    writer.WriteEndObject()

/// A JSON object of the entries, keys sorted, so two identical attribute sets
/// are the same text.
let private writeObject (writer: Utf8JsonWriter) (entries: Map<string, 'v>) (writeValue: 'v -> unit) : unit =
    writer.WriteStartObject()

    for entry in entries do
        writer.WritePropertyName entry.Key
        writeValue entry.Value

    writer.WriteEndObject()

/// A float as Go's strconv.FormatFloat(v, 'g', -1, 64) writes it: the
/// shortest digits that round-trip, with an exponent below 1e-4 and from 1e6.
let formatFloat (value: float) : string =
    if Double.IsNaN value then
        "NaN"
    elif Double.IsPositiveInfinity value then
        "+Inf"
    elif Double.IsNegativeInfinity value then
        "-Inf"
    else
        let sign = if Double.IsNegative value then "-" else ""

        // .NET's "R" is also the shortest round-trip form; only its layout
        // differs, so take its digits and lay them out again.
        let text = (abs value).ToString("R", CultureInfo.InvariantCulture)

        let mantissa, exponent =
            match text.IndexOf 'E' with
            | -1 -> text, 0
            | i -> text.Substring(0, i), int (text.Substring(i + 1))

        let whole, fraction =
            match mantissa.IndexOf '.' with
            | -1 -> mantissa, ""
            | i -> mantissa.Substring(0, i), mantissa.Substring(i + 1)

        let allDigits = whole + fraction
        let leadingZeros = allDigits.Length - allDigits.TrimStart('0').Length
        let digits = allDigits.Trim '0'
        // The value is 0.<digits> × 10^point.
        let point = whole.Length + exponent - leadingZeros

        if digits = "" then
            sign + "0"
        elif point - 1 < -4 || point - 1 >= 6 then
            let rest = if digits.Length > 1 then "." + digits.Substring 1 else ""
            let power = point - 1
            let powerSign = if power < 0 then "-" else "+"
            sign + digits.Substring(0, 1) + rest + "e" + powerSign + (abs power).ToString "00"
        elif point <= 0 then
            sign + "0." + String('0', -point) + digits
        elif digits.Length <= point then
            sign + digits + String('0', point - digits.Length)
        else
            sign + digits.Substring(0, point) + "." + digits.Substring point

// --- /api/v0.2/traces -> spans ---

let private toMap (field: MapField<string, 'v>) : Map<string, 'v> =
    field |> Seq.map (fun pair -> pair.Key, pair.Value) |> Map.ofSeq

/// Splits an idx 128-bit trace id into (high, low). The wire carries
/// big-endian bytes; a short id is left-padded and a long one keeps its last
/// sixteen bytes, rather than dropping an id that could be read.
let splitTraceId (bytes: byte[]) : uint64 * uint64 =
    if bytes.Length = 0 then
        0UL, 0UL
    else
        let padded = Array.zeroCreate<byte> 16

        if bytes.Length >= 16 then
            Array.blit bytes (bytes.Length - 16) padded 0 16
        else
            Array.blit bytes 0 padded (16 - bytes.Length) bytes.Length

        Buffers.Binary.BinaryPrimitives.ReadUInt64BigEndian(ReadOnlySpan(padded, 0, 8)),
        Buffers.Binary.BinaryPrimitives.ReadUInt64BigEndian(ReadOnlySpan(padded, 8, 8))

/// The hex upper half of a 128-bit trace id, from meta["_dd.p.tid"]. Anything
/// unparseable is 0, and stays in meta where a reader can still see it.
let parseHex64 (text: string) : uint64 =
    let mutable value = 0UL
    let mutable valid = text <> ""

    for c in text do
        let digit =
            if c >= '0' && c <= '9' then int c - int '0'
            elif c >= 'a' && c <= 'f' then int c - int 'a' + 10
            elif c >= 'A' && c <= 'F' then int c - int 'A' + 10
            else -1

        if digit < 0 || value > (UInt64.MaxValue >>> 4) then
            valid <- false
        else
            value <- (value <<< 4) ||| uint64 digit

    if valid then value else 0UL

let private writeV04ArrayValue (writer: Utf8JsonWriter) (value: AttributeArrayValue) : unit =
    match value.Type with
    | AttributeArrayValue.Types.AttributeArrayValueType.StringValue ->
        writeTagged writer "string" (fun () -> writer.WriteStringValue value.StringValue)
    | AttributeArrayValue.Types.AttributeArrayValueType.BoolValue ->
        writeTagged writer "bool" (fun () -> writer.WriteBooleanValue value.BoolValue)
    | AttributeArrayValue.Types.AttributeArrayValueType.IntValue ->
        writeTagged writer "int" (fun () -> writer.WriteNumberValue value.IntValue)
    | AttributeArrayValue.Types.AttributeArrayValueType.DoubleValue ->
        writeTagged writer "double" (fun () -> writeDouble writer value.DoubleValue)
    | _ -> writer.WriteNullValue()

/// AttributeAnyValue is a hand-rolled union: `Type` says which field is
/// meaningful, and the others are zero, not absent.
let private writeV04Value (writer: Utf8JsonWriter) (value: AttributeAnyValue) : unit =
    match value.Type with
    | AttributeAnyValue.Types.AttributeAnyValueType.StringValue ->
        writeTagged writer "string" (fun () -> writer.WriteStringValue value.StringValue)
    | AttributeAnyValue.Types.AttributeAnyValueType.BoolValue ->
        writeTagged writer "bool" (fun () -> writer.WriteBooleanValue value.BoolValue)
    | AttributeAnyValue.Types.AttributeAnyValueType.IntValue ->
        writeTagged writer "int" (fun () -> writer.WriteNumberValue value.IntValue)
    | AttributeAnyValue.Types.AttributeAnyValueType.DoubleValue ->
        writeTagged writer "double" (fun () -> writeDouble writer value.DoubleValue)
    | AttributeAnyValue.Types.AttributeAnyValueType.ArrayValue ->
        writeTagged writer "array" (fun () ->
            writer.WriteStartArray()

            if not (isNull value.ArrayValue) then
                for item in value.ArrayValue.Values do
                    writeV04ArrayValue writer item

            writer.WriteEndArray())
    | _ -> writer.WriteNullValue()

/// A v0.4 span event's typed attributes, in the same tagged form as idx.
let private v04EventAttributes (attributes: MapField<string, AttributeAnyValue>) : string =
    if attributes.Count = 0 then
        ""
    else
        jsonText (fun writer -> writeObject writer (toMap attributes) (writeV04Value writer))

/// A v0.4 span link's plain string attributes, in the tagged form too, so the
/// link_attributes column has one shape across both wire formats.
let private v04LinkAttributes (attributes: MapField<string, string>) : string =
    if attributes.Count = 0 then
        ""
    else
        jsonText (fun writer ->
            writeObject writer (toMap attributes) (fun value ->
                writeTagged writer "string" (fun () -> writer.WriteStringValue value)))

let private v04SpanRow (chunk: SpanRow) (span: Span) : SpanRow =
    let meta = toMap span.Meta

    { chunk with
        Service = span.Service
        Name = span.Name
        Resource = span.Resource
        SpanType = span.Type
        TraceID = span.TraceID
        // v0.4 has no field for the upper half of a 128-bit trace id: it
        // travels as hex in meta. Parsed so both formats fill the same pair
        // of columns, and left in meta as it arrived.
        TraceIDHigh = parseHex64 (meta.TryFind "_dd.p.tid" |> Option.defaultValue "")
        SpanID = span.SpanID
        ParentID = span.ParentID
        Start = UnixNanos span.Start
        DurationNs = span.Duration
        Error = span.Error
        // v0.4 has no kind field either; the tracer puts it in meta.
        Kind = meta.TryFind "span.kind" |> Option.defaultValue ""
        Meta = meta
        Metrics = toMap span.Metrics
        MetaStruct = span.MetaStruct |> Seq.map (fun pair -> pair.Key, pair.Value.ToByteArray()) |> Map.ofSeq
        LinkTraceID = span.SpanLinks |> Seq.map _.TraceID |> Array.ofSeq
        LinkTraceIDHigh = span.SpanLinks |> Seq.map _.TraceIDHigh |> Array.ofSeq
        LinkSpanID = span.SpanLinks |> Seq.map _.SpanID |> Array.ofSeq
        LinkAttributes = span.SpanLinks |> Seq.map (fun link -> v04LinkAttributes link.Attributes) |> Array.ofSeq
        LinkTracestate = span.SpanLinks |> Seq.map _.Tracestate |> Array.ofSeq
        LinkFlags = span.SpanLinks |> Seq.map _.Flags |> Array.ofSeq
        EventTime = span.SpanEvents |> Seq.map (fun event -> UnixNanos(int64 event.TimeUnixNano)) |> Array.ofSeq
        EventName = span.SpanEvents |> Seq.map _.Name |> Array.ofSeq
        EventAttributes = span.SpanEvents |> Seq.map (fun event -> v04EventAttributes event.Attributes) |> Array.ofSeq }

let private v04TracerRows (agent: SpanRow) (tracer: TracerPayload) : SpanRow list =
    let debug = tracer.ContainerDebug
    // The agent sets ContainerDebug only when resolving the container tags
    // gave it trouble, so absent stays NULL: a zero latency would read as
    // "the lookup was instant".
    let hasDebug = not (isNull debug)

    let tracerRow =
        { agent with
            WireFormat = "v04"
            ContainerID = tracer.ContainerID
            Language = tracer.LanguageName
            LanguageVersion = tracer.LanguageVersion
            TracerVersion = tracer.TracerVersion
            RuntimeID = tracer.RuntimeID
            TracerEnv = tracer.Env
            TracerHostname = tracer.Hostname
            AppVersion = tracer.AppVersion
            TracerTags = toMap tracer.Tags
            ContainerDebugError = if hasDebug then Some debug.Error else None
            ContainerDebugLatencyMs = if hasDebug then Some debug.LatencyMs else None
            ContainerDebugWasBuffered = if hasDebug then Some(Text.flag debug.WasBuffered) else None
            ContainerDebugBufferMs = if hasDebug then Some debug.BufferMs else None
            ContainerDebugBufferEvictionReason = if hasDebug then Some debug.BufferEvictionReason else None }

    [ for chunk in tracer.Chunks do
          // v0.4 has no SamplingMechanism field; the same information is in
          // meta["_dd.p.dm"], kept in the meta map untouched.
          let chunkRow =
              { tracerRow with
                  Priority = chunk.Priority
                  Origin = chunk.Origin
                  DroppedTrace = Text.flag chunk.DroppedTrace
                  ChunkTags = toMap chunk.Tags }

          for span in chunk.Spans do
              v04SpanRow chunkRow span ]

/// Resolves a reference into an idx payload's string table. Index 0 is the
/// empty string and means "unset"; a reference past the end is a broken
/// payload and resolves to empty, so one bad reference does not cost the batch.
let stringAt (strings: IList<string>) (index: uint32) : string =
    if index = 0u || int64 index >= int64 strings.Count then "" else strings[int index]

/// How far a nested attribute is followed. AnyValue is recursive on the wire
/// and nothing bounds it; real tracers nest two or three levels.
let attributeMaxDepth = 32

let rec private writeIdxValue (strings: IList<string>) (writer: Utf8JsonWriter) (value: Idx.AnyValue) (depth: int) : unit =
    if depth >= attributeMaxDepth then
        // Truncated, and said so: an empty value would read as "the tracer
        // sent nothing".
        writer.WriteStartObject()
        writer.WriteNumber("_depth_exceeded", attributeMaxDepth)
        writer.WriteEndObject()
    elif isNull value then
        writer.WriteNullValue()
    else
        match value.ValueCase with
        | Idx.AnyValue.ValueOneofCase.StringValueRef ->
            writeTagged writer "string" (fun () -> writer.WriteStringValue(stringAt strings value.StringValueRef))
        | Idx.AnyValue.ValueOneofCase.BoolValue ->
            writeTagged writer "bool" (fun () -> writer.WriteBooleanValue value.BoolValue)
        | Idx.AnyValue.ValueOneofCase.IntValue -> writeTagged writer "int" (fun () -> writer.WriteNumberValue value.IntValue)
        | Idx.AnyValue.ValueOneofCase.DoubleValue ->
            writeTagged writer "double" (fun () -> writeDouble writer value.DoubleValue)
        | Idx.AnyValue.ValueOneofCase.BytesValue ->
            writeTagged writer "bytes" (fun () -> writeBytes writer (value.BytesValue.ToByteArray()))
        | Idx.AnyValue.ValueOneofCase.ArrayValue ->
            writeTagged writer "array" (fun () ->
                writer.WriteStartArray()

                for item in value.ArrayValue.Values do
                    writeIdxValue strings writer item (depth + 1)

                writer.WriteEndArray())
        | Idx.AnyValue.ValueOneofCase.KeyValueList ->
            // A list, not an object: two entries may share a key, and the
            // order is the tracer's.
            writeTagged writer "kvlist" (fun () ->
                writer.WriteStartArray()

                for entry in value.KeyValueList.KeyValues do
                    writer.WriteStartObject()
                    writer.WriteString("key", stringAt strings entry.Key)
                    writer.WritePropertyName "value"
                    writeIdxValue strings writer entry.Value (depth + 1)
                    writer.WriteEndObject()

                writer.WriteEndArray())
        // A kind of value this build does not know: null, so the key still
        // shows the attribute existed.
        | _ -> writer.WriteNullValue()

/// An idx attribute map as JSON, every value tagged with its wire type:
/// {"k":{"string":"v"},"n":{"int":5}}. "" when there are none.
let idxAttributesJson (strings: IList<string>) (attributes: MapField<uint32, Idx.AnyValue>) : string =
    if attributes.Count = 0 then
        ""
    else
        let byName = attributes |> Seq.map (fun pair -> stringAt strings pair.Key, pair.Value) |> Map.ofSeq
        jsonText (fun writer -> writeObject writer byName (fun value -> writeIdxValue strings writer value 0))

/// The scalar attributes as plain strings, for the tracer_tags and chunk_tags
/// columns. The others survive in the matching *_attributes_json column.
let private idxScalarAttributes (strings: IList<string>) (attributes: MapField<uint32, Idx.AnyValue>) : Map<string, string> =
    let mutable found: Map<string, string> = Map.empty

    for pair in attributes do
        let key = stringAt strings pair.Key
        let value = pair.Value

        match value.ValueCase with
        | Idx.AnyValue.ValueOneofCase.StringValueRef -> found <- found.Add(key, stringAt strings value.StringValueRef)
        | Idx.AnyValue.ValueOneofCase.BoolValue -> found <- found.Add(key, (if value.BoolValue then "true" else "false"))
        | Idx.AnyValue.ValueOneofCase.IntValue -> found <- found.Add(key, value.IntValue.ToString CultureInfo.InvariantCulture)
        | Idx.AnyValue.ValueOneofCase.DoubleValue -> found <- found.Add(key, formatFloat value.DoubleValue)
        | _ -> ()

    found

/// Splits an idx span's typed attributes into the three v0.4 maps, so a query
/// written for v0.4 reads both formats: strings and bools to meta, numbers to
/// metrics, bytes to meta_struct.
///
/// An int goes to metrics as a float, which loses precision above 2^53;
/// attributes_json keeps the exact value beside it.
let private idxProjectAttributes
    (strings: IList<string>)
    (attributes: MapField<uint32, Idx.AnyValue>)
    : Map<string, string> * Map<string, float> * Map<string, byte[]> =
    let mutable meta: Map<string, string> = Map.empty
    let mutable metrics: Map<string, float> = Map.empty
    let mutable metaStruct: Map<string, byte[]> = Map.empty

    for pair in attributes do
        let key = stringAt strings pair.Key
        let value = pair.Value

        match value.ValueCase with
        | Idx.AnyValue.ValueOneofCase.StringValueRef -> meta <- meta.Add(key, stringAt strings value.StringValueRef)
        | Idx.AnyValue.ValueOneofCase.BoolValue -> meta <- meta.Add(key, (if value.BoolValue then "true" else "false"))
        | Idx.AnyValue.ValueOneofCase.IntValue -> metrics <- metrics.Add(key, float value.IntValue)
        | Idx.AnyValue.ValueOneofCase.DoubleValue -> metrics <- metrics.Add(key, value.DoubleValue)
        | Idx.AnyValue.ValueOneofCase.BytesValue -> metaStruct <- metaStruct.Add(key, value.BytesValue.ToByteArray())
        | _ -> ()

    meta, metrics, metaStruct

/// The idx SpanKind as the lowercase OTel name v0.4 tracers put in
/// meta["span.kind"]. Unspecified is empty, as an absent meta key is.
let private idxSpanKind (kind: Idx.SpanKind) : string =
    match kind with
    | Idx.SpanKind.Internal -> "internal"
    | Idx.SpanKind.Server -> "server"
    | Idx.SpanKind.Client -> "client"
    | Idx.SpanKind.Producer -> "producer"
    | Idx.SpanKind.Consumer -> "consumer"
    | _ -> ""

let private idxSpanRow (chunk: SpanRow) (strings: IList<string>) (span: Idx.Span) : SpanRow =
    let meta, metrics, metaStruct = idxProjectAttributes strings span.Attributes
    let linkIds = span.Links |> Seq.map (fun link -> splitTraceId (link.TraceID.ToByteArray())) |> Array.ofSeq

    { chunk with
        Service = stringAt strings span.ServiceRef
        Name = stringAt strings span.NameRef
        Resource = stringAt strings span.ResourceRef
        SpanType = stringAt strings span.TypeRef
        SpanID = span.SpanID
        ParentID = span.ParentID
        Start = UnixNanos(int64 span.Start)
        DurationNs = int64 span.Duration
        // v0.4 ships an int32 flag and idx a bool; the column keeps the v0.4
        // width so one query reads both.
        Error = (if span.Error then 1 else 0)
        Kind = idxSpanKind span.Kind
        SpanEnv = stringAt strings span.EnvRef
        SpanVersion = stringAt strings span.VersionRef
        Component = stringAt strings span.ComponentRef
        Meta = meta
        Metrics = metrics
        MetaStruct = metaStruct
        AttributesJSON = idxAttributesJson strings span.Attributes
        LinkTraceID = linkIds |> Array.map snd
        LinkTraceIDHigh = linkIds |> Array.map fst
        LinkSpanID = span.Links |> Seq.map _.SpanID |> Array.ofSeq
        LinkAttributes = span.Links |> Seq.map (fun link -> idxAttributesJson strings link.Attributes) |> Array.ofSeq
        LinkTracestate = span.Links |> Seq.map (fun link -> stringAt strings link.TracestateRef) |> Array.ofSeq
        LinkFlags = span.Links |> Seq.map _.Flags |> Array.ofSeq
        EventTime = span.Events |> Seq.map (fun event -> UnixNanos(int64 event.Time)) |> Array.ofSeq
        EventName = span.Events |> Seq.map (fun event -> stringAt strings event.NameRef) |> Array.ofSeq
        EventAttributes = span.Events |> Seq.map (fun event -> idxAttributesJson strings event.Attributes) |> Array.ofSeq }

/// One v1.0 (idx) tracer payload. Every string in this format is an index
/// into the payload's own table, so nothing is readable until resolved.
let private idxTracerRows (agent: SpanRow) (tracer: Idx.TracerPayload) : SpanRow list =
    let strings = tracer.Strings :> IList<string>
    let debug = tracer.ContainerDebug
    let hasDebug = not (isNull debug)

    let tracerRow =
        { agent with
            WireFormat = "idx"
            ContainerID = stringAt strings tracer.ContainerIDRef
            Language = stringAt strings tracer.LanguageNameRef
            LanguageVersion = stringAt strings tracer.LanguageVersionRef
            TracerVersion = stringAt strings tracer.TracerVersionRef
            RuntimeID = stringAt strings tracer.RuntimeIDRef
            TracerEnv = stringAt strings tracer.EnvRef
            TracerHostname = stringAt strings tracer.HostnameRef
            AppVersion = stringAt strings tracer.AppVersionRef
            TracerTags = idxScalarAttributes strings tracer.Attributes
            TracerAttributesJSON = idxAttributesJson strings tracer.Attributes
            ContainerDebugError = if hasDebug then Some(stringAt strings debug.ErrorRef) else None
            ContainerDebugLatencyMs = if hasDebug then Some debug.LatencyMs else None
            ContainerDebugWasBuffered = if hasDebug then Some(Text.flag debug.WasBuffered) else None
            ContainerDebugBufferMs = if hasDebug then Some debug.BufferMs else None
            ContainerDebugBufferEvictionReason =
                if hasDebug then Some(stringAt strings debug.BufferEvictionReasonRef) else None }

    [ for chunk in tracer.Chunks do
          // v1.0 moved the trace id to the chunk, as the full 128 bits.
          let high, low = splitTraceId (chunk.TraceID.ToByteArray())

          let chunkRow =
              { tracerRow with
                  Priority = chunk.Priority
                  Origin = stringAt strings chunk.OriginRef
                  DroppedTrace = Text.flag chunk.DroppedTrace
                  SamplingMechanism = chunk.SamplingMechanism
                  ChunkTags = idxScalarAttributes strings chunk.Attributes
                  ChunkAttributesJSON = idxAttributesJson strings chunk.Attributes
                  TraceIDHigh = high
                  TraceID = low }

          for span in chunk.Spans do
              idxSpanRow chunkRow strings span ]

/// Flattens a whole AgentPayload into span rows, both wire shapes, in wire
/// order.
///
/// The agent, tracer and chunk are copied onto every row rather than
/// normalized away: "which agent version shipped this span" is the question
/// asked when a trace looks wrong, and it should not take two joins.
let spanRows (tenant: string) (receivedAt: DateTime) (payload: AgentPayload) : SpanRow[] =
    let agent: SpanRow =
        { TenantID = tenant
          ReceivedAt = receivedAt
          WireFormat = ""
          AgentHostname = payload.HostName
          AgentEnv = payload.Env
          AgentVersion = payload.AgentVersion
          TargetTPS = payload.TargetTPS
          ErrorTPS = payload.ErrorTPS
          RareSamplerEnabled = Text.flag payload.RareSamplerEnabled
          AgentTags = toMap payload.Tags
          ContainerID = ""
          Language = ""
          LanguageVersion = ""
          TracerVersion = ""
          RuntimeID = ""
          TracerEnv = ""
          TracerHostname = ""
          AppVersion = ""
          TracerTags = Map.empty
          TracerAttributesJSON = ""
          ContainerDebugError = None
          ContainerDebugLatencyMs = None
          ContainerDebugWasBuffered = None
          ContainerDebugBufferMs = None
          ContainerDebugBufferEvictionReason = None
          Priority = 0
          Origin = ""
          DroppedTrace = 0uy
          SamplingMechanism = 0u
          ChunkTags = Map.empty
          ChunkAttributesJSON = ""
          Service = ""
          Name = ""
          Resource = ""
          SpanType = ""
          TraceID = 0UL
          TraceIDHigh = 0UL
          SpanID = 0UL
          ParentID = 0UL
          Start = UnixNanos 0L
          DurationNs = 0L
          Error = 0
          Kind = ""
          SpanEnv = ""
          SpanVersion = ""
          Component = ""
          Meta = Map.empty
          Metrics = Map.empty
          MetaStruct = Map.empty
          AttributesJSON = ""
          LinkTraceID = [||]
          LinkTraceIDHigh = [||]
          LinkSpanID = [||]
          LinkAttributes = [||]
          LinkTracestate = [||]
          LinkFlags = [||]
          EventTime = [||]
          EventName = [||]
          EventAttributes = [||] }

    [| for tracer in payload.TracerPayloads do
           yield! v04TracerRows agent tracer

       for tracer in payload.IdxTracerPayloads do
           yield! idxTracerRows agent tracer |]

/// POST /api/v0.2/traces: a protobuf AgentPayload.
///
/// What arrives is a sample: the agent runs its samplers before this writer.
/// The complete counts are in the stats endpoint. The agent parses the answer
/// (rate_by_service feeds its priority sampler), so it keeps exactly this shape.
let handleTraces (r: Request) : Response =
    let answer = Response.json 200 """{"rate_by_service":{}}"""

    if Diagnose.isSweep r then
        answer
    else
        let parsed =
            try
                Ok(AgentPayload.Parser.ParseFrom r.Body)
            with :? InvalidProtocolBufferException as e ->
                Error e.Message

        match parsed with
        | Error problem ->
            r.Log.LogWarning("[traces] protobuf AgentPayload: {Error} ({Bytes} bytes)", problem, r.Body.Length)
            Raw.store r intake "decode_error" $"protobuf AgentPayload: {problem}" r.Body
        | Ok payload when payload.TracerPayloads.Count = 0 && payload.IdxTracerPayloads.Count = 0 ->
            // Protobuf carries no type marker and skips unknown fields, so a
            // body meant for another endpoint decodes "successfully" into an
            // empty payload. Neither list being filled is the one signal.
            r.Log.LogWarning("[traces] protobuf decoded to zero tracer payloads ({Bytes} bytes), wrong payload type?", r.Body.Length)
            Raw.store r intake "unexpected_shape" "AgentPayload decoded with no TracerPayloads and no IdxTracerPayloads" r.Body
        | Ok payload ->
            if r.Tenant <> "" then
                Sink.write r.Sink Spans.table (spanRows r.Tenant DateTime.UtcNow payload)

        answer

// --- /api/v0.2/stats -> apm_stats ---

/// The ceiling msgp's generated decoder puts on any map, array or byte string.
let private statsLimit = 500_000

let private checkLimit (count: int) : unit =
    if count > statsLimit then
        raise (MsgpFailure(Other "msgp: configured reader limit exceeded"))

let private readStrings (r: MsgpReader) (field: string) (into: RepeatedField<string>) : unit =
    let count = Msgp.at field r.ReadArrayHeader
    checkLimit count
    into.Clear()

    for i in 0 .. count - 1 do
        into.Add(Msgp.atIndex field i r.ReadString)

let private readSummary (r: MsgpReader) (field: string) : ByteString =
    let bytes = Msgp.at field r.ReadBin
    checkLimit bytes.Length
    ByteString.CopyFrom bytes

/// Reads an array of maps into `into`. A nil element is skipped: msgp writes
/// one for a nil pointer, and no row comes of it.
let private readList (r: MsgpReader) (into: RepeatedField<'item>) (readItem: MsgpReader -> 'item) : unit =
    let count = Msgp.at "Stats" r.ReadArrayHeader
    checkLimit count
    into.Clear()

    for i in 0 .. count - 1 do
        if r.IsNil then
            r.ReadNil()
        else
            into.Add(Msgp.atIndex "Stats" i (fun () -> readItem r))

let private readGroup (r: MsgpReader) : ClientGroupedStats =
    let group = ClientGroupedStats()
    let fields = r.ReadMapHeader()
    checkLimit fields

    for _ in 1..fields do
        match r.ReadMapKey() with
        | "Service" -> group.Service <- Msgp.at "Service" r.ReadString
        | "Name" -> group.Name <- Msgp.at "Name" r.ReadString
        | "Resource" -> group.Resource <- Msgp.at "Resource" r.ReadString
        | "HTTPStatusCode" -> group.HTTPStatusCode <- Msgp.at "HTTPStatusCode" r.ReadUInt32
        | "Type" -> group.Type <- Msgp.at "Type" r.ReadString
        | "DBType" -> group.DBType <- Msgp.at "DBType" r.ReadString
        | "Hits" -> group.Hits <- Msgp.at "Hits" r.ReadUInt64
        | "Errors" -> group.Errors <- Msgp.at "Errors" r.ReadUInt64
        | "Duration" -> group.Duration <- Msgp.at "Duration" r.ReadUInt64
        | "OkSummary" -> group.OkSummary <- readSummary r "OkSummary"
        | "ErrorSummary" -> group.ErrorSummary <- readSummary r "ErrorSummary"
        | "Synthetics" -> group.Synthetics <- Msgp.at "Synthetics" r.ReadBool
        | "TopLevelHits" -> group.TopLevelHits <- Msgp.at "TopLevelHits" r.ReadUInt64
        | "SpanKind" -> group.SpanKind <- Msgp.at "SpanKind" r.ReadString
        | "PeerTags" -> readStrings r "PeerTags" group.PeerTags
        | "IsTraceRoot" -> group.IsTraceRoot <- enum<Trilean> (Msgp.at "IsTraceRoot" r.ReadInt32)
        | "GRPCStatusCode" -> group.GRPCStatusCode <- Msgp.at "GRPCStatusCode" r.ReadString
        | "HTTPMethod" -> group.HTTPMethod <- Msgp.at "HTTPMethod" r.ReadString
        | "HTTPEndpoint" -> group.HTTPEndpoint <- Msgp.at "HTTPEndpoint" r.ReadString
        // The one key that is not the field's name.
        | "srv_src" -> group.ServiceSource <- Msgp.at "ServiceSource" r.ReadString
        | "SpanDerivedPrimaryTags" -> readStrings r "SpanDerivedPrimaryTags" group.SpanDerivedPrimaryTags
        | "AdditionalMetricTags" -> readStrings r "AdditionalMetricTags" group.AdditionalMetricTags
        | _ -> r.Skip()

    group

let private readBucket (r: MsgpReader) : ClientStatsBucket =
    let bucket = ClientStatsBucket()
    let fields = r.ReadMapHeader()
    checkLimit fields

    for _ in 1..fields do
        match r.ReadMapKey() with
        | "Start" -> bucket.Start <- Msgp.at "Start" r.ReadUInt64
        | "Duration" -> bucket.Duration <- Msgp.at "Duration" r.ReadUInt64
        | "Stats" -> readList r bucket.Stats readGroup
        | "AgentTimeShift" -> bucket.AgentTimeShift <- Msgp.at "AgentTimeShift" r.ReadInt64
        | _ -> r.Skip()

    bucket

let private readClient (r: MsgpReader) : ClientStatsPayload =
    let client = ClientStatsPayload()
    let fields = r.ReadMapHeader()
    checkLimit fields

    for _ in 1..fields do
        match r.ReadMapKey() with
        | "Hostname" -> client.Hostname <- Msgp.at "Hostname" r.ReadString
        | "Env" -> client.Env <- Msgp.at "Env" r.ReadString
        | "Version" -> client.Version <- Msgp.at "Version" r.ReadString
        | "Stats" -> readList r client.Stats readBucket
        | "Lang" -> client.Lang <- Msgp.at "Lang" r.ReadString
        | "TracerVersion" -> client.TracerVersion <- Msgp.at "TracerVersion" r.ReadString
        | "RuntimeID" -> client.RuntimeID <- Msgp.at "RuntimeID" r.ReadString
        | "Sequence" -> client.Sequence <- Msgp.at "Sequence" r.ReadUInt64
        | "AgentAggregation" -> client.AgentAggregation <- Msgp.at "AgentAggregation" r.ReadString
        | "Service" -> client.Service <- Msgp.at "Service" r.ReadString
        | "ContainerID" -> client.ContainerID <- Msgp.at "ContainerID" r.ReadString
        | "Tags" -> readStrings r "Tags" client.Tags
        | "GitCommitSha" -> client.GitCommitSha <- Msgp.at "GitCommitSha" r.ReadString
        | "ImageTag" -> client.ImageTag <- Msgp.at "ImageTag" r.ReadString
        | "ProcessTagsHash" -> client.ProcessTagsHash <- Msgp.at "ProcessTagsHash" r.ReadUInt64
        | "ProcessTags" -> client.ProcessTags <- Msgp.at "ProcessTags" r.ReadString
        | _ -> r.Skip()

    client

/// Reads a msgpack StatsPayload, as the decoder msgp generates for the
/// agent's type does: a map keyed by field name at every level.
let decodeStatsPayload (body: byte[]) : Result<StatsPayload, string> =
    let r = MsgpReader(body, Slice)

    try
        let payload = StatsPayload()
        let fields = r.ReadMapHeader()
        checkLimit fields

        for _ in 1..fields do
            match r.ReadMapKey() with
            | "AgentHostname" -> payload.AgentHostname <- Msgp.at "AgentHostname" r.ReadString
            | "AgentEnv" -> payload.AgentEnv <- Msgp.at "AgentEnv" r.ReadString
            | "Stats" -> readList r payload.Stats readClient
            | "AgentVersion" -> payload.AgentVersion <- Msgp.at "AgentVersion" r.ReadString
            | "ClientComputed" -> payload.ClientComputed <- Msgp.at "ClientComputed" r.ReadBool
            | "SplitPayload" -> payload.SplitPayload <- Msgp.at "SplitPayload" r.ReadBool
            | _ -> r.Skip()

        Ok payload
    with MsgpFailure error ->
        Error(MsgpError.text error)

/// The Trilean's name, as the Enum8 column declares it.
let private trileanName (value: Trilean) : string =
    match value with
    | Trilean.True -> "true"
    | Trilean.False -> "false"
    | _ -> "not_set"

/// Flattens a StatsPayload into one row per grouped stat. A summary that
/// does not decode is logged and still stored: "undecodable" is a state the
/// table records.
let statRows (log: ILogger) (tenant: string) (receivedAt: DateTime) (payload: StatsPayload) : APMStatRow[] =
    let summaryOf (group: ClientGroupedStats) (column: string) (raw: ByteString) : SketchSummary =
        let summary, problem = TraceSketch.summary (raw.ToByteArray())

        match problem with
        | Some error ->
            log.LogWarning(
                "[apm-stats] {Service} {Name} {Column} {Bytes} B: {Error}",
                group.Service,
                group.Name,
                column,
                raw.Length,
                error
            )
        | None -> ()

        summary

    [| for client in payload.Stats do
           // The one flat "key:value" list in the trace protocol: a tag key
           // may repeat, and a plain map would keep whichever came last.
           let clientTags = Tags.toMultiMap client.Tags

           for bucket in client.Stats do
               for group in bucket.Stats do
                   { TenantID = tenant
                     ReceivedAt = receivedAt
                     AgentHostname = payload.AgentHostname
                     AgentEnv = payload.AgentEnv
                     AgentVersion = payload.AgentVersion
                     ClientComputed = Text.flag payload.ClientComputed
                     SplitPayload = Text.flag payload.SplitPayload
                     ClientHostname = client.Hostname
                     ClientEnv = client.Env
                     ClientVersion = client.Version
                     ClientLang = client.Lang
                     ClientTracerVersion = client.TracerVersion
                     ClientRuntimeID = client.RuntimeID
                     ClientSequence = client.Sequence
                     ClientAgentAggregation = client.AgentAggregation
                     ClientService = client.Service
                     ClientContainerID = client.ContainerID
                     ClientTags = clientTags
                     ClientGitCommitSha = client.GitCommitSha
                     ClientImageTag = client.ImageTag
                     ClientProcessTags = client.ProcessTags
                     ClientProcessTagsHash = client.ProcessTagsHash
                     BucketStart = UnixNanos(int64 bucket.Start)
                     BucketDurationNs = bucket.Duration
                     AgentTimeShiftNs = bucket.AgentTimeShift
                     Service = group.Service
                     Name = group.Name
                     Resource = group.Resource
                     HTTPStatusCode = group.HTTPStatusCode
                     SpanType = group.Type
                     DBType = group.DBType
                     Hits = group.Hits
                     Errors = group.Errors
                     DurationNs = group.Duration
                     Synthetics = Text.flag group.Synthetics
                     TopLevelHits = group.TopLevelHits
                     SpanKind = group.SpanKind
                     PeerTags = Array.ofSeq group.PeerTags
                     IsTraceRoot = trileanName group.IsTraceRoot
                     GRPCStatusCode = group.GRPCStatusCode
                     HTTPMethod = group.HTTPMethod
                     HTTPEndpoint = group.HTTPEndpoint
                     ServiceSource = group.ServiceSource
                     SpanDerivedPrimaryTags = Array.ofSeq group.SpanDerivedPrimaryTags
                     AdditionalMetricTags = Array.ofSeq group.AdditionalMetricTags
                     OkSummary = summaryOf group "ok_summary" group.OkSummary
                     ErrorSummary = summaryOf group "error_summary" group.ErrorSummary } |]

/// POST /api/v0.2/stats: msgpack, not protobuf.
///
/// What arrives is NOT a sample: the agent feeds every trace to its
/// concentrator before the sampler runs, so these counts are complete even
/// when 99% of the spans were thrown away.
///
/// The Android SDK from 3.14 may post msgpack here that is not a
/// StatsPayload; it fails the decode and goes to raw_payloads.
let handleStats (r: Request) : Response =
    if not (Diagnose.isSweep r) then
        match decodeStatsPayload r.Body with
        | Error problem ->
            r.Log.LogWarning("[apm-stats] msgpack StatsPayload: {Error} ({Bytes} bytes)", problem, r.Body.Length)
            Raw.store r intake "decode_error" $"msgpack StatsPayload: {problem}" r.Body
        | Ok payload ->
            if r.Tenant <> "" then
                Sink.write r.Sink ApmStats.table (statRows r.Log r.Tenant DateTime.UtcNow payload)

    Response.json 200 "{}"

// --- /api/v0.1/pipeline_stats -> dsm_pipeline_stats, dsm_backlogs, dsm_bucket_transactions ---
//
// The schema is dd-trace-go's, not the agent's: the trace-agent only proxies
// this request. dd-trace-go generates its codec with msgp and no field tags,
// so the wire is a map whose keys are its Go field names. Every level keeps
// the keys it does not know, with their values: dd-trace-go adds fields with
// no version negotiation, and the payload that shows a new one arrives once.

type DsmBacklog = { Tags: string[]; Value: int64 }

type DsmPoint =
    { EdgeTags: string[]
      Hash: uint64
      ParentHash: uint64
      PathwayLatency: byte[]
      EdgeLatency: byte[]
      PayloadSize: byte[]
      TimestampType: string
      Unknown: Map<string, MsgValue> }

type DsmBucket =
    { Start: uint64
      Duration: uint64
      Points: DsmPoint list
      Backlogs: DsmBacklog list
      Transactions: byte[]
      TransactionCheckpointIDs: byte[]
      Unknown: Map<string, MsgValue> }

type DsmPayload =
    { Env: string
      Service: string
      TracerVersion: string
      Lang: string
      Version: string
      ProcessTags: string[]
      ProductMask: uint64
      Buckets: DsmBucket list
      Unknown: Map<string, MsgValue> }

/// A string as Go's %q prints it, near enough for a field name.
let private quoted (text: string) : string =
    let escaped = StringBuilder()

    for c in text do
        match c with
        | '"' -> escaped.Append "\\\"" |> ignore
        | '\\' -> escaped.Append "\\\\" |> ignore
        | '\n' -> escaped.Append "\\n" |> ignore
        | '\r' -> escaped.Append "\\r" |> ignore
        | '\t' -> escaped.Append "\\t" |> ignore
        | c when c < ' ' || c = '\127' -> escaped.Append($"\\x{int c:x2}") |> ignore
        | c -> escaped.Append c |> ignore

    "\"" + escaped.ToString() + "\""

/// Runs a read; an error it raises is reported as `<label> "<key>": <error>`.
let private named (label: string) (key: string) (read: unit -> 'a) : 'a =
    try
        read ()
    with MsgpFailure error ->
        raise (MsgpFailure(Other $"{label} {quoted key}: {MsgpError.text error}"))

/// The number of entries of the map that is next. A nil in place of the map
/// counts as empty: msgp writes nil for an absent value.
let private mapLength (r: MsgpReader) : int =
    if r.IsNil then
        r.ReadNil()
        0
    else
        r.ReadMapHeader()

let private arrayLength (r: MsgpReader) : int =
    if r.IsNil then
        r.ReadNil()
        0
    else
        r.ReadArrayHeader()

let private dsmStrings (r: MsgpReader) : string[] =
    let count = arrayLength r
    [| for _ in 1..count -> r.ReadString() |]

let private dsmBytes (r: MsgpReader) : byte[] =
    if r.IsNil then
        r.ReadNil()
        [||]
    else
        r.ReadBin()

/// Reads a field this decoder does not know, value and all. No count in it
/// may exceed the body's length: every element costs at least a byte, so a
/// forged header fails here instead of asking for gigabytes.
let private dsmUnknown (r: MsgpReader) (bodyLength: int) (key: string) (known: Map<string, MsgValue>) : Map<string, MsgValue> =
    known.Add(key, named "unknown field" key (fun () -> r.ReadAny bodyLength))

let private dsmBacklog (r: MsgpReader) (bodyLength: int) : DsmBacklog =
    let mutable tags: string[] = [||]
    let mutable value = 0L

    for _ in 1 .. mapLength r do
        match r.ReadString() with
        | "Tags" as key -> tags <- named "backlog field" key (fun () -> dsmStrings r)
        | "Value" as key -> value <- named "backlog field" key r.ReadInt64
        | key -> dsmUnknown r bodyLength key Map.empty |> ignore

    { Tags = tags; Value = value }

let private dsmPoint (r: MsgpReader) (bodyLength: int) : DsmPoint =
    let mutable edgeTags: string[] = [||]
    let mutable hash = 0UL
    let mutable parentHash = 0UL
    let mutable pathwayLatency: byte[] = [||]
    let mutable edgeLatency: byte[] = [||]
    let mutable payloadSize: byte[] = [||]
    let mutable timestampType = ""
    let mutable unknown: Map<string, MsgValue> = Map.empty

    for _ in 1 .. mapLength r do
        match r.ReadString() with
        | "EdgeTags" as key -> edgeTags <- named "point field" key (fun () -> dsmStrings r)
        | "Hash" as key -> hash <- named "point field" key r.ReadUInt64
        | "ParentHash" as key -> parentHash <- named "point field" key r.ReadUInt64
        | "PathwayLatency" as key -> pathwayLatency <- named "point field" key (fun () -> dsmBytes r)
        | "EdgeLatency" as key -> edgeLatency <- named "point field" key (fun () -> dsmBytes r)
        | "PayloadSize" as key -> payloadSize <- named "point field" key (fun () -> dsmBytes r)
        | "TimestampType" as key -> timestampType <- named "point field" key r.ReadString
        | key -> unknown <- dsmUnknown r bodyLength key unknown

    { EdgeTags = edgeTags
      Hash = hash
      ParentHash = parentHash
      PathwayLatency = pathwayLatency
      EdgeLatency = edgeLatency
      PayloadSize = payloadSize
      TimestampType = timestampType
      Unknown = unknown }

let private dsmBucket (r: MsgpReader) (bodyLength: int) : DsmBucket =
    let mutable start = 0UL
    let mutable duration = 0UL
    let points = ResizeArray<DsmPoint>()
    let backlogs = ResizeArray<DsmBacklog>()
    let mutable transactions: byte[] = [||]
    let mutable checkpointIds: byte[] = [||]
    let mutable unknown: Map<string, MsgValue> = Map.empty

    for _ in 1 .. mapLength r do
        match r.ReadString() with
        | "Start" as key -> start <- named "bucket field" key r.ReadUInt64
        | "Duration" as key -> duration <- named "bucket field" key r.ReadUInt64
        | "Stats" as key ->
            named "bucket field" key (fun () ->
                for _ in 1 .. arrayLength r do
                    points.Add(dsmPoint r bodyLength))
        | "Backlogs" as key ->
            named "bucket field" key (fun () ->
                for _ in 1 .. arrayLength r do
                    backlogs.Add(dsmBacklog r bodyLength))
        | "Transactions" as key -> transactions <- named "bucket field" key (fun () -> dsmBytes r)
        // "Ids", not "IDs": dd-trace-go spells the field that way to match
        // the Java tracer's wire format.
        | "TransactionCheckpointIds" as key -> checkpointIds <- named "bucket field" key (fun () -> dsmBytes r)
        | key -> unknown <- dsmUnknown r bodyLength key unknown

    { Start = start
      Duration = duration
      Points = List.ofSeq points
      Backlogs = List.ofSeq backlogs
      Transactions = transactions
      TransactionCheckpointIDs = checkpointIds
      Unknown = unknown }

/// Reads one msgpack datastreams.StatsPayload.
let decodeDsmPayload (body: byte[]) : Result<DsmPayload, string> =
    if body.Length = 0 then
        Error "empty body"
    else
        let r = MsgpReader(body, Stream)
        let mutable env = ""
        let mutable service = ""
        let mutable tracerVersion = ""
        let mutable lang = ""
        let mutable version = ""
        let mutable processTags: string[] = [||]
        let mutable productMask = 0UL
        let buckets = ResizeArray<DsmBucket>()
        let mutable unknown: Map<string, MsgValue> = Map.empty

        try
            for _ in 1 .. mapLength r do
                match r.ReadString() with
                | "Env" as key -> env <- named "field" key r.ReadString
                | "Service" as key -> service <- named "field" key r.ReadString
                | "TracerVersion" as key -> tracerVersion <- named "field" key r.ReadString
                | "Lang" as key -> lang <- named "field" key r.ReadString
                | "Version" as key -> version <- named "field" key r.ReadString
                | "ProcessTags" as key -> processTags <- named "field" key (fun () -> dsmStrings r)
                | "ProductMask" as key -> productMask <- named "field" key r.ReadUInt64
                | "Stats" as key ->
                    named "field" key (fun () ->
                        for _ in 1 .. arrayLength r do
                            buckets.Add(dsmBucket r body.Length))
                | key -> unknown <- dsmUnknown r body.Length key unknown

            Ok
                { Env = env
                  Service = service
                  TracerVersion = tracerVersion
                  Lang = lang
                  Version = version
                  ProcessTags = processTags
                  ProductMask = productMask
                  Buckets = List.ofSeq buckets
                  Unknown = unknown }
        with MsgpFailure error ->
            Error(MsgpError.text error)

/// A value of an unknown field as JSON; bytes become base64.
let rec private writeUnknown (writer: Utf8JsonWriter) (value: MsgValue) : unit =
    match value with
    | MsgNil -> writer.WriteNullValue()
    | MsgBool flag -> writer.WriteBooleanValue flag
    | MsgInt number -> writer.WriteNumberValue number
    | MsgUInt number -> writer.WriteNumberValue number
    | MsgFloat32 number ->
        if Single.IsFinite number then writer.WriteNumberValue number else writeDouble writer (float number)
    | MsgFloat number -> writeDouble writer number
    | MsgStr text -> writer.WriteStringValue text
    // msgp hands an empty bin over as a nil slice, which Go writes as null.
    | MsgBin bytes -> if bytes.Length = 0 then writer.WriteNullValue() else writeBytes writer bytes
    | MsgArray items ->
        writer.WriteStartArray()

        for item in items do
            writeUnknown writer item

        writer.WriteEndArray()
    | MsgMap entries -> writeObject writer (Map.ofList entries) (writeUnknown writer)
    | MsgExt(extType, data) ->
        writer.WriteStartObject()
        writer.WritePropertyName "Data"
        if data.Length = 0 then writer.WriteNullValue() else writeBytes writer data
        writer.WriteNumber("Type", int extType)
        writer.WriteEndObject()

/// The four request headers the body does not repeat. The trace-agent adds
/// three of them as it proxies the request, and they hold the only container
/// identity a pipeline payload has.
type DsmHeaders =
    { Via: string
      AdditionalTags: string
      ContainerTags: string
      ContentEncoding: string }

let private prefixed (prefix: string) (unknown: Map<string, MsgValue>) : (string * MsgValue) list =
    unknown |> Map.toList |> List.map (fun (key, value) -> prefix + key, value)

/// Fans one decoded payload out into the three tables it feeds.
let dsmRows
    (tenant: string)
    (receivedAt: DateTime)
    (payload: DsmPayload)
    (headers: DsmHeaders)
    : DSMPipelineStatRow[] * DSMBacklogRow[] * DSMBucketTransactionRow[] =
    let points = ResizeArray<DSMPipelineStatRow>()
    let backlogs = ResizeArray<DSMBacklogRow>()
    let blobs = ResizeArray<DSMBucketTransactionRow>()

    for bucket in payload.Buckets do
        let common: DSMCommon =
            { TenantID = tenant
              ReceivedAt = receivedAt
              Env = payload.Env
              Service = payload.Service
              TracerVersion = payload.TracerVersion
              Lang = payload.Lang
              Version = payload.Version
              ProcessTags = payload.ProcessTags
              ProductMask = payload.ProductMask
              BucketStart = UnixNanos(int64 bucket.Start)
              BucketDurationNs = bucket.Duration }

        for point in bucket.Points do
            // Unknown keys from all three levels ride the point row, prefixed
            // by where they were found: one column pair for the whole payload
            // keeps the schema still while dd-trace-go grows fields.
            let unknown =
                Map.ofList (prefixed "payload." payload.Unknown @ prefixed "bucket." bucket.Unknown @ prefixed "point." point.Unknown)

            points.Add
                { DSMCommon = common
                  EdgeTags = point.EdgeTags
                  Hash = point.Hash
                  ParentHash = point.ParentHash
                  TimestampType = point.TimestampType
                  PathwayLatency = fst (TraceSketch.summary point.PathwayLatency)
                  EdgeLatency = fst (TraceSketch.summary point.EdgeLatency)
                  PayloadSize = fst (TraceSketch.summary point.PayloadSize)
                  Via = headers.Via
                  AdditionalTags = headers.AdditionalTags
                  ContainerTags = headers.ContainerTags
                  ContentEncoding = headers.ContentEncoding
                  UnknownKeys = unknown |> Map.toArray |> Array.map fst
                  UnknownJSON =
                    if unknown.IsEmpty then
                        ""
                    else
                        jsonText (fun writer -> writeObject writer unknown (writeUnknown writer)) }

        for backlog in bucket.Backlogs do
            backlogs.Add
                { DSMCommon = common
                  Tags = backlog.Tags
                  Value = backlog.Value }

        // A table of their own because these hang off the bucket: a bucket
        // can carry them with no stats points at all.
        if bucket.Transactions.Length > 0 || bucket.TransactionCheckpointIDs.Length > 0 then
            blobs.Add
                { DSMCommon = common
                  Transactions = bucket.Transactions
                  TransactionCheckpointIDs = bucket.TransactionCheckpointIDs }

    points.ToArray(), backlogs.ToArray(), blobs.ToArray()

/// POST /api/v0.1/pipeline_stats: Data Streams Monitoring stats straight from
/// the tracer. The trace-agent is a reverse proxy here: it adds Via,
/// X-Datadog-Additional-Tags and X-Datadog-Container-Tags and forwards the
/// body untouched.
let handlePipelineStats (r: Request) : Response =
    if not (Diagnose.isSweep r) then
        match decodeDsmPayload r.Body with
        | Error problem ->
            r.Log.LogWarning("[pipeline-stats] msgpack datastreams.StatsPayload: {Error} ({Bytes} bytes)", problem, r.Body.Length)
            Raw.store r intake "decode_error" $"msgpack datastreams.StatsPayload: {problem}" r.Body
        | Ok payload ->
            if r.Tenant <> "" then
                let headers =
                    { Via = r.Header "Via"
                      AdditionalTags = r.Header "X-Datadog-Additional-Tags"
                      ContainerTags = r.Header "X-Datadog-Container-Tags"
                      // The engine decompresses the body and leaves the
                      // header, so this is what the tracer put on the wire.
                      ContentEncoding = r.Header "Content-Encoding" }

                let points, backlogs, blobs = dsmRows r.Tenant DateTime.UtcNow payload headers
                Sink.write r.Sink DsmPipelineStats.table points
                Sink.write r.Sink DsmBacklogs.table backlogs
                Sink.write r.Sink DsmBucketTransactions.table blobs

    Response.json 202 "{}"

// --- /api/v2/data_streams_messages -> dsm_messages ---

/// Go's JSON depth limit; .NET's default of 64 would refuse what Go took.
let private jsonDepth = 10_000

/// The body as the messages of the array the forwarder sends, each as the
/// bytes it arrived as, or why the body is not one. `null` is an empty batch,
/// as it was to Go's decoder.
let parseMessages (body: byte[]) : Result<byte[] list, string> =
    let wrongKind (kind: string) =
        Error $"json: cannot unmarshal {kind} into Go value of type []jsontext.Value"

    try
        let mutable reader = Utf8JsonReader(ReadOnlySpan body, JsonReaderOptions(MaxDepth = jsonDepth))
        reader.Read() |> ignore

        match reader.TokenType with
        | JsonTokenType.StartArray ->
            let messages = ResizeArray<byte[]>()

            while reader.Read() && reader.TokenType <> JsonTokenType.EndArray do
                let start = int reader.TokenStartIndex
                reader.Skip()
                messages.Add(body[start .. int reader.BytesConsumed - 1])

            // Anything after the array is an error, raised by this read.
            reader.Read() |> ignore
            Ok(List.ofSeq messages)
        | kind ->
            reader.Skip()
            reader.Read() |> ignore

            match kind with
            | JsonTokenType.Null -> Ok []
            | JsonTokenType.StartObject -> wrongKind "object"
            | JsonTokenType.String -> wrongKind "string"
            | JsonTokenType.Number -> wrongKind "number"
            | _ -> wrongKind "bool"
    with :? JsonException as e ->
        Error e.Message

/// A message's top-level key names, sorted. The one thing extractable without
/// guessing at meaning, and enough to turn "what does this track send?" into
/// a GROUP BY. A message that is not an object has none.
let messageKeys (message: byte[]) : string[] =
    let mutable reader = Utf8JsonReader(ReadOnlySpan message, JsonReaderOptions(MaxDepth = jsonDepth))
    let keys = ResizeArray<string>()

    if reader.Read() && reader.TokenType = JsonTokenType.StartObject then
        while reader.Read() && reader.TokenType = JsonTokenType.PropertyName do
            let key =
                try
                    reader.GetString()
                with :? InvalidOperationException ->
                    // Not UTF-8: Go read such a key with replacement characters.
                    Encoding.UTF8.GetString reader.ValueSpan

            keys.Add key
            reader.Read() |> ignore
            reader.Skip()

    keys |> Seq.distinct |> Seq.sortWith (fun a b -> String.CompareOrdinal(a, b)) |> Array.ofSeq

/// One row per message, with its place in the batch.
///
/// The producer of this track is not in the open agent repository, so no
/// shape is invented: a message is stored as the JSON text it arrived as.
let dsmMessageRows
    (tenant: string)
    (receivedAt: DateTime)
    (messages: byte[] list)
    (origin: string)
    (originVersion: string)
    (encoding: string)
    : DSMMessageRow[] =
    messages
    |> List.mapi (fun position message ->
        { TenantID = tenant
          ReceivedAt = receivedAt
          Position = uint32 position
          Message = message
          Keys = messageKeys message
          DDEVPOrigin = origin
          DDEVPOriginVersion = originVersion
          ContentEncoding = encoding })
    |> Array.ofList

/// POST /api/v2/data_streams_messages. Not the trace-agent at all: an Event
/// Platform track that happens to live on this host. JSON, batched by the
/// forwarder into one array.
let handleDataStreamsMessages (r: Request) : Response =
    match parseMessages r.Body with
    | Error problem ->
        r.Log.LogWarning("[data-streams] JSON array: {Error} ({Bytes} bytes)", problem, r.Body.Length)
        Raw.store r intake "unexpected_shape" $"body is not the JSON array the forwarder sends: {problem}" r.Body
    | Ok messages ->
        if r.Tenant <> "" then
            let rows =
                dsmMessageRows
                    r.Tenant
                    DateTime.UtcNow
                    messages
                    (r.Header "Dd-Evp-Origin")
                    (r.Header "Dd-Evp-Origin-Version")
                    (r.Header "Content-Encoding")

            Sink.write r.Sink DsmMessages.table rows

    Response.json 202 "{}"

let routes: Route list =
    [ Route.post "/api/v0.2/traces" handleTraces
      Route.post "/api/v0.2/stats" handleStats
      Route.post "/api/v0.1/pipeline_stats" handlePipelineStats
      Route.post "/api/v2/data_streams_messages" handleDataStreamsMessages ]
