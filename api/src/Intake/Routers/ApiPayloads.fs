/// The wire formats of api.<site>.
///
/// Every body here is decoded the way the Go intake decoded it, and Go
/// decoded each into a type Datadog publishes:
///
///   /api/v1/series               datadogV1.MetricsPayload
///   /api/v2/series               agent-payload's MetricPayload (protobuf, or JSON of the same shape)
///   /api/beta/sketches           agent-payload's SketchPayload (protobuf, or JSON of the same shape)
///   /api/v1/check_run            a list of datadogV1.ServiceCheck
///   /api/v1/events               datadogV1.EventCreateRequest
///   /api/v1/distribution_points  datadogV1.DistributionPointsPayload
///
/// The datadogV1 models share one behaviour worth knowing: an item with a
/// declared field of the wrong type is not an error. The model keeps the
/// whole object aside ("UnparsedObject") and leaves its fields empty; the
/// handler then stores the item raw. Only a missing required field, or an
/// item that is not an object, is an error.
module NinjaCat.Api.Intake.Routers.ApiPayloads

open System
open System.IO
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open System.Text.Unicode
open Google.Protobuf
open Google.Protobuf.Reflection
open Datadog.Agentpayload
open NinjaCat.Api.Intake

/// JSON read with the rules of Go's encoding/json, where they differ from
/// System.Text.Json's and the difference decides what is stored.
module GoJson =
    // Go nests to 10000; System.Text.Json stops at 64 unless told otherwise.
    let private documentOptions = JsonDocumentOptions(MaxDepth = 10000)

    let private writerOptions =
        JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping, MaxDepth = 10000)

    let private hexDigit (b: byte) : int =
        if b >= '0'B && b <= '9'B then int (b - '0'B)
        elif b >= 'a'B && b <= 'f'B then int (b - 'a'B) + 10
        elif b >= 'A'B && b <= 'F'B then int (b - 'A'B) + 10
        else -1

    /// The code unit of a \uXXXX escape starting at `i`, or -1.
    let private escapedUnit (data: byte[]) (i: int) : int =
        if i + 5 < data.Length && data[i] = '\\'B && data[i + 1] = 'u'B then
            let digits = [ hexDigit data[i + 2]; hexDigit data[i + 3]; hexDigit data[i + 4]; hexDigit data[i + 5] ]
            if List.contains -1 digits then -1 else digits |> List.fold (fun unit digit -> unit * 16 + digit) 0
        else
            -1

    /// Go reads a \u escape that is half of a surrogate pair as U+FFFD;
    /// System.Text.Json throws when such a string is read. The escape is
    /// rewritten to \ufffd, which is as long, before parsing.
    let private replaceLoneSurrogates (body: byte[]) : byte[] =
        let mutable result = body
        let mutable i = 0

        while i < body.Length do
            if body[i] <> '\\'B then
                i <- i + 1
            else
                let unit = escapedUnit body i
                let next = escapedUnit body (i + 6)

                if unit >= 0xD800 && unit <= 0xDBFF && next >= 0xDC00 && next <= 0xDFFF then
                    i <- i + 12
                elif unit >= 0xD800 && unit <= 0xDFFF then
                    if obj.ReferenceEquals(result, body) then
                        result <- Array.copy body

                    Array.blit "fffd"B 0 result (i + 2) 4
                    i <- i + 6
                else
                    i <- i + 2

        result

    /// The body as JSON, or the parser's message. Go turns invalid UTF-8
    /// inside a string into U+FFFD; System.Text.Json throws when such a
    /// string is read, so the replacement is done before parsing.
    let parse (body: byte[]) : Result<JsonElement, string> =
        let valid =
            if Utf8.IsValid(ReadOnlySpan body) then body else Encoding.UTF8.GetBytes(Encoding.UTF8.GetString body)

        let clean = replaceLoneSurrogates valid

        try
            use doc = JsonDocument.Parse(ReadOnlyMemory clean, documentOptions)
            Ok(doc.RootElement.Clone())
        with e ->
            Error e.Message

    /// What Go's decoder calls a value in its type errors.
    let kindName (value: JsonElement) : string =
        match value.ValueKind with
        | JsonValueKind.String -> "string"
        | JsonValueKind.Number -> "number"
        | JsonValueKind.True
        | JsonValueKind.False -> "bool"
        | JsonValueKind.Array -> "array"
        | JsonValueKind.Object -> "object"
        | _ -> "null"

    /// The text of Go's error for a value that cannot go into a Go type.
    let mismatch (value: JsonElement) (goType: string) : string =
        $"json: cannot unmarshal {kindName value} into Go value of type {goType}"

    /// An object's members by name, as a Go map holds them: of two with the
    /// same name the later wins. Anything but an object has none.
    let members (value: JsonElement) : Map<string, JsonElement> =
        if value.ValueKind <> JsonValueKind.Object then
            Map.empty
        else
            value.EnumerateObject() |> Seq.map (fun p -> p.Name, p.Value) |> Map.ofSeq

    /// The member a Go struct field tagged `name` is filled from: names match
    /// without regard to case, the last match wins, null is the same as
    /// absent.
    let field (name: string) (object: JsonElement) : JsonElement option =
        let mutable found = None

        if object.ValueKind = JsonValueKind.Object then
            for p in object.EnumerateObject() do
                if p.Name.Equals(name, StringComparison.OrdinalIgnoreCase) then
                    found <- Some p.Value

        match found with
        | Some value when value.ValueKind = JsonValueKind.Null -> None
        | other -> other

    let asString (value: JsonElement) : string option =
        if value.ValueKind = JsonValueKind.String then Some(value.GetString()) else None

    /// An integer as Go reads one: a number written with a fraction or an
    /// exponent is not one.
    let asInt64 (value: JsonElement) : int64 option =
        if value.ValueKind <> JsonValueKind.Number then
            None
        else
            match value.TryGetInt64() with
            | true, n -> Some n
            | false, _ -> None

    let asInt32 (value: JsonElement) : int32 option =
        if value.ValueKind <> JsonValueKind.Number then
            None
        else
            match value.TryGetInt32() with
            | true, n -> Some n
            | false, _ -> None

    let asUInt32 (value: JsonElement) : uint32 option =
        if value.ValueKind <> JsonValueKind.Number then
            None
        else
            match value.TryGetUInt32() with
            | true, n -> Some n
            | false, _ -> None

    /// A float64. A literal beyond its range does not fit, as in Go.
    let asFloat (value: JsonElement) : float option =
        if value.ValueKind <> JsonValueKind.Number then
            None
        else
            match value.TryGetDouble() with
            | true, x when not (Double.IsInfinity x) -> Some x
            | _ -> None

    /// A Go slice: an array whose every element fits. A null element keeps
    /// the zero value.
    let asArray (zero: 'a) (read: JsonElement -> 'a option) (value: JsonElement) : 'a[] option =
        if value.ValueKind <> JsonValueKind.Array then
            None
        else
            let items = ResizeArray<'a>()
            let mutable fits = true

            for item in value.EnumerateArray() do
                if item.ValueKind = JsonValueKind.Null then
                    items.Add zero
                else
                    match read item with
                    | Some x -> items.Add x
                    | None -> fits <- false

            if fits then Some(items.ToArray()) else None

    let asStrings (value: JsonElement) : string[] option = asArray "" asString value
    let asFloats (value: JsonElement) : float[] option = asArray 0.0 asFloat value
    let asInt32s (value: JsonElement) : int32[] option = asArray 0 asInt32 value
    let asUInt32s (value: JsonElement) : uint32[] option = asArray 0u asUInt32 value

    /// A map[string]string: an object whose values are strings (null is "").
    let asStringMap (value: JsonElement) : Map<string, string> option =
        if value.ValueKind <> JsonValueKind.Object then
            None
        else
            let entries = members value |> Map.map (fun _ v -> if v.ValueKind = JsonValueKind.Null then Some "" else asString v)

            if entries |> Map.forall (fun _ v -> v.IsSome) then
                Some(entries |> Map.map (fun _ v -> v.Value))
            else
                None

    // The lenient readers are `json.Unmarshal(raw, &x)` with the error
    // ignored: what fits is kept, what does not stays at its zero value.

    let lenientString (value: JsonElement option) : string =
        match value with
        | Some v when v.ValueKind = JsonValueKind.String -> v.GetString()
        | _ -> ""

    let lenientInt64 (value: JsonElement option) : int64 =
        value |> Option.bind asInt64 |> Option.defaultValue 0L

    /// A []string: an element that is not a string is "". None when Go's
    /// slice would stay nil — the value is absent, null or not an array.
    let lenientStrings (value: JsonElement option) : string[] option =
        match value with
        | Some v when v.ValueKind = JsonValueKind.Array ->
            Some [| for item in v.EnumerateArray() -> lenientString (Some item) |]
        | _ -> None

    /// A map[string][]string: a key whose value is not a list keeps the key,
    /// with no values.
    let lenientStringLists (value: JsonElement option) : Map<string, string[]> =
        match value with
        | Some v when v.ValueKind = JsonValueKind.Object ->
            members v |> Map.map (fun _ list -> lenientStrings (Some list) |> Option.defaultValue [||])
        | _ -> Map.empty

    /// A map[string]json.RawMessage: the members of an object, else none.
    let lenientMembers (value: JsonElement option) : Map<string, JsonElement> =
        match value with
        | Some v -> members v
        | None -> Map.empty

    /// The first number in the value that is beyond float64, as written. Go
    /// cannot hold such a value in an `interface{}`, which is what the
    /// models keep an item they could not fit in.
    let rec overflowingNumber (value: JsonElement) : string option =
        match value.ValueKind with
        | JsonValueKind.Number -> if (asFloat value).IsSome then None else Some(value.GetRawText())
        | JsonValueKind.Array -> value.EnumerateArray() |> Seq.tryPick overflowingNumber
        | JsonValueKind.Object -> value.EnumerateObject() |> Seq.tryPick (fun p -> overflowingNumber p.Value)
        | _ -> None

    /// The bytes a value arrived as.
    let rawBytes (value: JsonElement) : byte[] = Encoding.UTF8.GetBytes(value.GetRawText())

    /// The value re-encoded without whitespace. Numbers keep their digits.
    let compact (value: JsonElement) : string =
        use buffer = new MemoryStream()

        do
            use writer = new Utf8JsonWriter(buffer, writerOptions)
            value.WriteTo writer

        Encoding.UTF8.GetString(buffer.ToArray())

    /// The values as one JSON array.
    let compactArray (values: JsonElement seq) : string =
        "[" + String.Join(",", values |> Seq.map compact) + "]"

    /// The members as one JSON object; "{}" when there are none.
    let compactObject (values: Map<string, JsonElement>) : string =
        use buffer = new MemoryStream()

        do
            use writer = new Utf8JsonWriter(buffer, writerOptions)
            writer.WriteStartObject()

            for pair in values do
                writer.WritePropertyName pair.Key
                pair.Value.WriteTo writer

            writer.WriteEndObject()

        Encoding.UTF8.GetString(buffer.ToArray())

/// Why a body gave nothing to store.
type Problem =
    /// It cannot be decoded. The text goes into the raw row's note.
    | Undecodable of error: string
    /// The Go handler panics on this body, after its reply is queued: the
    /// sender gets the usual answer and nothing is stored.
    | Panics

// ---------------------------------------------------------------------------
// /api/v2/series and /api/beta/sketches: protobuf, or JSON of the same shape
// ---------------------------------------------------------------------------

/// A value of the wrong JSON type for the field it would fill.
exception Misfit of field: string * found: string

let private misfit (field: string) (value: JsonElement) : 'a =
    raise (Misfit(field, GoJson.kindName value))

/// Fills one scalar field from the object; an absent or null key leaves it.
let private fill
    (object: JsonElement)
    (path: string)
    (name: string)
    (read: JsonElement -> 'a option)
    (assign: 'a -> unit)
    : unit =
    match GoJson.field name object with
    | None -> ()
    | Some value ->
        match read value with
        | Some x -> assign x
        | None -> misfit $"{path}.{name}" value

/// A Go slice of structs: every element an object, or null for the zero
/// value.
let private structs (path: string) (decode: string -> JsonElement -> 'a) (zero: unit -> 'a) (list: JsonElement) : 'a list =
    if list.ValueKind <> JsonValueKind.Array then
        misfit path list
    else
        [ for item in list.EnumerateArray() do
              match item.ValueKind with
              | JsonValueKind.Object -> decode path item
              | JsonValueKind.Null -> zero ()
              | _ -> misfit path item ]

let private metadataOfJson (path: string) (value: JsonElement) : Metadata =
    if value.ValueKind <> JsonValueKind.Object then
        misfit path value

    let metadata = Metadata()

    match GoJson.field "origin" value with
    | None -> ()
    | Some from ->
        let originPath = path + ".origin"

        if from.ValueKind <> JsonValueKind.Object then
            misfit originPath from

        let origin = Origin()
        fill from originPath "origin_product" GoJson.asUInt32 (fun x -> origin.OriginProduct <- x)
        fill from originPath "origin_category" GoJson.asUInt32 (fun x -> origin.OriginCategory <- x)
        fill from originPath "origin_service" GoJson.asUInt32 (fun x -> origin.OriginService <- x)
        metadata.Origin <- origin

    metadata

let private resourceOfJson (path: string) (value: JsonElement) : MetricPayload.Types.Resource =
    let resource = MetricPayload.Types.Resource()
    fill value path "type" GoJson.asString (fun x -> resource.Type <- x)
    fill value path "name" GoJson.asString (fun x -> resource.Name <- x)
    resource

let private pointOfJson (path: string) (value: JsonElement) : MetricPayload.Types.MetricPoint =
    let point = MetricPayload.Types.MetricPoint()
    fill value path "value" GoJson.asFloat (fun x -> point.Value <- x)
    fill value path "timestamp" GoJson.asInt64 (fun x -> point.Timestamp <- x)
    point

let private seriesOfJson (path: string) (value: JsonElement) : MetricPayload.Types.MetricSeries =
    let series = MetricPayload.Types.MetricSeries()

    match GoJson.field "resources" value with
    | Some list -> series.Resources.AddRange(structs (path + ".resources") resourceOfJson MetricPayload.Types.Resource list)
    | None -> ()

    fill value path "metric" GoJson.asString (fun x -> series.Metric <- x)
    fill value path "tags" GoJson.asStrings (fun x -> series.Tags.AddRange x)

    match GoJson.field "points" value with
    | Some list -> series.Points.AddRange(structs (path + ".points") pointOfJson MetricPayload.Types.MetricPoint list)
    | None -> ()

    // The type is the enum's number here, not its name: Go reads this JSON
    // with encoding/json, not with protobuf's JSON mapping.
    fill value path "type" GoJson.asInt32 (fun x -> series.Type <- enum<MetricPayload.Types.MetricType> x)
    fill value path "unit" GoJson.asString (fun x -> series.Unit <- x)
    fill value path "source_type_name" GoJson.asString (fun x -> series.SourceTypeName <- x)
    fill value path "interval" GoJson.asInt64 (fun x -> series.Interval <- x)

    match GoJson.field "metadata" value with
    | Some metadata -> series.Metadata <- metadataOfJson (path + ".metadata") metadata
    | None -> ()

    series

/// A null in a resource list, before any resource of type "host".
let private nullBeforeHost (resources: JsonElement) : bool =
    let mutable hostSeen = false
    let mutable found = false

    for item in resources.EnumerateArray() do
        if item.ValueKind = JsonValueKind.Null then
            if not hostSeen then
                found <- true
        elif GoJson.field "type" item |> Option.bind GoJson.asString = Some "host" then
            hostSeen <- true

    found

/// Go decodes a null series or resource into a nil pointer and its handler
/// dereferences it: a null series always, a null resource while it looks for
/// the host.
let private seriesPanicInGo (root: JsonElement) : bool =
    match GoJson.field "series" root with
    | Some list when list.ValueKind = JsonValueKind.Array ->
        list.EnumerateArray()
        |> Seq.exists (fun series ->
            match series.ValueKind, GoJson.field "resources" series with
            | JsonValueKind.Null, _ -> true
            | _, Some resources when resources.ValueKind = JsonValueKind.Array -> nullBeforeHost resources
            | _ -> false)
    | _ -> false

/// The varint at `at` and the index after it; nothing when the bytes run out
/// before `stop`.
let private varint (data: byte[]) (at: int) (stop: int) : struct (uint64 * int) voption =
    let mutable value = 0UL
    let mutable shift = 0
    let mutable i = at
    let mutable result = ValueNone

    while result.IsNone && i < stop && shift < 64 do
        let b = data[i]
        value <- value ||| (uint64 (b &&& 0x7Fuy) <<< shift)
        shift <- shift + 7
        i <- i + 1

        if b < 0x80uy then
            result <- ValueSome(struct (value, i))

    result

/// gogo's generated Unmarshal — what Go parses these payloads with — refuses
/// a known field sent with the wrong wire type. Google.Protobuf keeps such a
/// field as an unknown one and carries on, so the bytes are walked once more
/// here, without copying them. The text is gogo's.
let rec private wrongWireType (descriptor: MessageDescriptor) (data: byte[]) (start: int) (stop: int) : string option =
    let mutable found = None
    let mutable at = start

    while found.IsNone && at < stop do
        match varint data at stop with
        | ValueNone -> at <- stop
        | ValueSome(struct (tag, afterTag)) ->
            let wireType = int (tag &&& 7UL)

            // Where the field's value starts and ends; -1 when it cannot be
            // told, which ParseFrom has already ruled out.
            let valueStart, valueEnd =
                match wireType with
                | 0 ->
                    match varint data afterTag stop with
                    | ValueSome(struct (_, after)) -> afterTag, after
                    | ValueNone -> afterTag, -1
                | 1 -> afterTag, afterTag + 8
                | 5 -> afterTag, afterTag + 4
                | 2 ->
                    match varint data afterTag stop with
                    | ValueSome(struct (length, after)) when length <= uint64 (stop - after) -> after, after + int length
                    | _ -> afterTag, -1
                | _ -> afterTag, -1

            if valueEnd < 0 || valueEnd > stop then
                at <- stop
            else
                let field = descriptor.FindFieldByNumber(int (tag >>> 3))

                if not (isNull field) then
                    let expected =
                        match field.FieldType with
                        | FieldType.Message
                        | FieldType.String
                        | FieldType.Bytes -> 2
                        | FieldType.Double
                        | FieldType.Fixed64
                        | FieldType.SFixed64 -> 1
                        | FieldType.Float
                        | FieldType.Fixed32
                        | FieldType.SFixed32 -> 5
                        | _ -> 0

                    // A repeated number arrives one by one, or packed in a block.
                    let packed = field.IsRepeated && wireType = 2

                    if wireType <> expected && not packed then
                        found <- Some $"proto: wrong wireType = {wireType} for field {field.PropertyName}"
                    elif field.FieldType = FieldType.Message then
                        found <- wrongWireType field.MessageType data valueStart valueEnd

                at <- valueEnd

    found

let private parseProtobuf
    (name: string)
    (parser: MessageParser<'message>)
    (descriptor: MessageDescriptor)
    (body: byte[])
    : Result<'message, string> =
    try
        let message = parser.ParseFrom body

        match wrongWireType descriptor body 0 body.Length with
        | Some error -> Error $"protobuf {name}: {error}"
        | None -> Ok message
    with :? InvalidProtocolBufferException as e ->
        Error $"protobuf {name}: {e.Message}"

let parseSeriesV2Protobuf (body: byte[]) : Result<MetricPayload, string> =
    parseProtobuf "MetricPayload" MetricPayload.Parser MetricPayload.Descriptor body

let parseSeriesV2Json (body: byte[]) : Result<MetricPayload, Problem> =
    match GoJson.parse body with
    | Error e -> Error(Undecodable $"JSON v2 series: {e}")
    | Ok root when root.ValueKind = JsonValueKind.Null -> Ok(MetricPayload())
    | Ok root when root.ValueKind <> JsonValueKind.Object ->
        Error(Undecodable $"""JSON v2 series: {GoJson.mismatch root "gogen.MetricPayload"}""")
    | Ok root ->
        try
            let payload = MetricPayload()

            match GoJson.field "series" root with
            | None -> ()
            | Some list when list.ValueKind = JsonValueKind.Array ->
                for item in list.EnumerateArray() do
                    match item.ValueKind with
                    | JsonValueKind.Object -> payload.Series.Add(seriesOfJson "series" item)
                    // Left out here; seriesPanicInGo answers for it below.
                    | JsonValueKind.Null -> ()
                    | _ -> misfit "series" item
            | Some other -> misfit "series" other

            if seriesPanicInGo root then Error Panics else Ok payload
        with Misfit(field, found) ->
            Error(Undecodable $"JSON v2 series: cannot unmarshal {found} into field {field}")

let private floatField (object: JsonElement) (path: string) (name: string) (assign: float -> unit) : unit =
    fill object path name GoJson.asFloat assign

let private dogsketchOfJson (path: string) (value: JsonElement) : SketchPayload.Types.Sketch.Types.Dogsketch =
    let sketch = SketchPayload.Types.Sketch.Types.Dogsketch()
    fill value path "ts" GoJson.asInt64 (fun x -> sketch.Ts <- x)
    fill value path "cnt" GoJson.asInt64 (fun x -> sketch.Cnt <- x)
    floatField value path "min" (fun x -> sketch.Min <- x)
    floatField value path "max" (fun x -> sketch.Max <- x)
    floatField value path "avg" (fun x -> sketch.Avg <- x)
    floatField value path "sum" (fun x -> sketch.Sum <- x)
    fill value path "k" GoJson.asInt32s (fun x -> sketch.K.AddRange x)
    fill value path "n" GoJson.asUInt32s (fun x -> sketch.N.AddRange x)
    sketch

let private distributionOfJson (path: string) (value: JsonElement) : SketchPayload.Types.Sketch.Types.Distribution =
    let distribution = SketchPayload.Types.Sketch.Types.Distribution()
    fill value path "ts" GoJson.asInt64 (fun x -> distribution.Ts <- x)
    fill value path "cnt" GoJson.asInt64 (fun x -> distribution.Cnt <- x)
    floatField value path "min" (fun x -> distribution.Min <- x)
    floatField value path "max" (fun x -> distribution.Max <- x)
    floatField value path "avg" (fun x -> distribution.Avg <- x)
    floatField value path "sum" (fun x -> distribution.Sum <- x)
    fill value path "v" GoJson.asFloats (fun x -> distribution.V.AddRange x)
    fill value path "g" GoJson.asUInt32s (fun x -> distribution.G.AddRange x)
    fill value path "delta" GoJson.asUInt32s (fun x -> distribution.Delta.AddRange x)
    fill value path "buf" GoJson.asFloats (fun x -> distribution.Buf.AddRange x)
    distribution

let private sketchOfJson (path: string) (value: JsonElement) : SketchPayload.Types.Sketch =
    let sketch = SketchPayload.Types.Sketch()
    fill value path "metric" GoJson.asString (fun x -> sketch.Metric <- x)
    fill value path "host" GoJson.asString (fun x -> sketch.Host <- x)

    match GoJson.field "distributions" value with
    | Some list ->
        sketch.Distributions.AddRange(
            structs (path + ".distributions") distributionOfJson SketchPayload.Types.Sketch.Types.Distribution list
        )
    | None -> ()

    fill value path "tags" GoJson.asStrings (fun x -> sketch.Tags.AddRange x)

    match GoJson.field "dogsketches" value with
    | Some list ->
        sketch.Dogsketches.AddRange(
            structs (path + ".dogsketches") dogsketchOfJson SketchPayload.Types.Sketch.Types.Dogsketch list
        )
    | None -> ()

    match GoJson.field "metadata" value with
    | Some metadata -> sketch.Metadata <- metadataOfJson (path + ".metadata") metadata
    | None -> ()

    sketch

let parseSketchesProtobuf (body: byte[]) : Result<SketchPayload, string> =
    parseProtobuf "SketchPayload" SketchPayload.Parser SketchPayload.Descriptor body

let parseSketchesJson (body: byte[]) : Result<SketchPayload, string> =
    match GoJson.parse body with
    | Error e -> Error $"JSON sketches: {e}"
    | Ok root when root.ValueKind = JsonValueKind.Null -> Ok(SketchPayload())
    | Ok root when root.ValueKind <> JsonValueKind.Object ->
        Error $"""JSON sketches: {GoJson.mismatch root "gogen.SketchPayload"}"""
    | Ok root ->
        try
            let payload = SketchPayload()

            match GoJson.field "sketches" root with
            | Some list -> payload.Sketches.AddRange(structs "sketches" sketchOfJson SketchPayload.Types.Sketch list)
            | None -> ()

            match GoJson.field "metadata" root with
            | None -> ()
            | Some from ->
                if from.ValueKind <> JsonValueKind.Object then
                    misfit "metadata" from

                let sender = CommonMetadata()
                fill from "metadata" "agent_version" GoJson.asString (fun x -> sender.AgentVersion <- x)
                fill from "metadata" "timezone" GoJson.asString (fun x -> sender.Timezone <- x)
                floatField from "metadata" "current_epoch" (fun x -> sender.CurrentEpoch <- x)
                fill from "metadata" "internal_ip" GoJson.asString (fun x -> sender.InternalIp <- x)
                fill from "metadata" "public_ip" GoJson.asString (fun x -> sender.PublicIp <- x)
                fill from "metadata" "api_key" GoJson.asString (fun x -> sender.ApiKey <- x)
                payload.Metadata <- sender

            Ok payload
        with Misfit(field, found) ->
            Error $"JSON sketches: cannot unmarshal {found} into field {field}"

/// True when the message carries fields its schema does not declare — what
/// Go reads off XXX_unrecognized. The generated C# class keeps unknown fields
/// and writes them back out, but does not expose them, so the message is
/// parsed again without them and the two sizes are compared.
let hasUnknownFields<'message when 'message :> IMessage<'message>>
    (parser: MessageParser<'message>)
    (message: 'message)
    : bool =
    let known = parser.WithDiscardUnknownFields(true).ParseFrom(MessageExtensions.ToByteArray message)
    known.CalculateSize() <> message.CalculateSize()

/// One sketch as JSON, with the keys and the omissions of Go's encoding of
/// the same struct: what a raw row holds for a sketch that has no row shape.
let sketchJson (sketch: SketchPayload.Types.Sketch) : byte[] =
    use buffer = new MemoryStream()

    do
        use writer =
            new Utf8JsonWriter(buffer, JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping))

        // JSON has no NaN or infinity; they are written as text.
        let floatValue (value: float) =
            if Double.IsFinite value then writer.WriteNumberValue value else writer.WriteStringValue(string value)

        let writeFloat (name: string) (value: float) =
            if BitConverter.DoubleToInt64Bits value <> 0L then
                writer.WritePropertyName name
                floatValue value

        let writeInt64 (name: string) (value: int64) =
            if value <> 0L then
                writer.WriteNumber(name, value)

        let writeUInt32 (name: string) (value: uint32) =
            if value <> 0u then
                writer.WriteNumber(name, value)

        let writeList (name: string) (count: int) (writeItems: unit -> unit) =
            if count > 0 then
                writer.WriteStartArray name
                writeItems ()
                writer.WriteEndArray()

        let writeFloats (name: string) (values: float seq) =
            writeList name (Seq.length values) (fun () ->
                for value in values do
                    floatValue value)

        let writeUInt32s (name: string) (values: uint32 seq) =
            writeList name (Seq.length values) (fun () ->
                for value in values do
                    writer.WriteNumberValue value)

        writer.WriteStartObject()

        if sketch.Metric <> "" then
            writer.WriteString("metric", sketch.Metric)

        if sketch.Host <> "" then
            writer.WriteString("host", sketch.Host)

        writer.WriteStartArray "distributions"

        for d in sketch.Distributions do
            writer.WriteStartObject()
            writeInt64 "ts" d.Ts
            writeInt64 "cnt" d.Cnt
            writeFloat "min" d.Min
            writeFloat "max" d.Max
            writeFloat "avg" d.Avg
            writeFloat "sum" d.Sum
            writeFloats "v" d.V
            writeUInt32s "g" d.G
            writeUInt32s "delta" d.Delta
            writeFloats "buf" d.Buf
            writer.WriteEndObject()

        writer.WriteEndArray()

        writeList "tags" sketch.Tags.Count (fun () ->
            for tag in sketch.Tags do
                writer.WriteStringValue tag)

        // Go writes a list it never filled as null, and `dogsketches` is
        // always written.
        if sketch.Dogsketches.Count = 0 then
            writer.WriteNull "dogsketches"
        else
            writer.WriteStartArray "dogsketches"

            for d in sketch.Dogsketches do
                writer.WriteStartObject()
                writeInt64 "ts" d.Ts
                writeInt64 "cnt" d.Cnt
                writeFloat "min" d.Min
                writeFloat "max" d.Max
                writeFloat "avg" d.Avg
                writeFloat "sum" d.Sum

                writeList "k" d.K.Count (fun () ->
                    for key in d.K do
                        writer.WriteNumberValue key)

                writeUInt32s "n" d.N
                writer.WriteEndObject()

            writer.WriteEndArray()

        if not (isNull sketch.Metadata) then
            writer.WriteStartObject "metadata"

            if not (isNull sketch.Metadata.Origin) then
                writer.WriteStartObject "origin"
                writeUInt32 "origin_product" sketch.Metadata.Origin.OriginProduct
                writeUInt32 "origin_category" sketch.Metadata.Origin.OriginCategory
                writeUInt32 "origin_service" sketch.Metadata.Origin.OriginService
                writer.WriteEndObject()

            writer.WriteEndObject()

        writer.WriteEndObject()

    buffer.ToArray()

// ---------------------------------------------------------------------------
// The datadogV1 models
// ---------------------------------------------------------------------------

/// The declared fields of one item, read as a generated UnmarshalJSON reads
/// them. One field of the wrong type makes the whole item "unparsed", so the
/// reader remembers whether that happened.
type private Declared(item: JsonElement) =
    let mutable misfit = false

    member _.Misfit: bool = misfit

    member _.Read(name: string, read: JsonElement -> 'a option) : 'a option =
        match GoJson.field name item with
        | None -> None
        | Some value ->
            match read value with
            | Some x -> Some x
            | None ->
                misfit <- true
                None

/// The members of an item that the model does not declare.
let private additional (declared: string list) (item: JsonElement) : Map<string, JsonElement> =
    GoJson.members item |> Map.filter (fun name _ -> not (List.contains name declared))

/// What a model does with an item it cannot fit: it keeps the whole object
/// aside — which Go can only do when every number in it fits a float64.
let private unparsed (item: JsonElement) (kept: 'a) : Result<'a, string> =
    match GoJson.overflowingNumber item with
    | Some number -> Error $"json: cannot unmarshal number {number} into Go value of type float64"
    | None -> Ok kept

let private notAnObject (item: JsonElement) : string =
    GoJson.mismatch item "map[string]interface {}"

/// A v1 point as sent: a [timestamp, value] pair where either may be null.
type SeriesPoint =
    { Pair: float option[]
      Raw: JsonElement }

/// datadogV1.Series.
type SeriesV1 =
    { Host: string
      Interval: int64 option
      Metric: string
      Points: SeriesPoint[]
      Tags: string[]
      Type: string
      /// The keys the model does not declare. The agent's v1 encoder sends
      /// three: device, source_type_name and unit.
      Additional: Map<string, JsonElement>
      /// The model could not fit the item; its fields are empty.
      Unparsed: bool
      /// The item as it arrived.
      Raw: JsonElement }

let private asSeriesPoints (list: JsonElement) : SeriesPoint[] option =
    let pair (item: JsonElement) : SeriesPoint option =
        if item.ValueKind = JsonValueKind.Null then
            Some { Pair = [||]; Raw = item }
        elif item.ValueKind <> JsonValueKind.Array then
            None
        else
            let numbers =
                [| for number in item.EnumerateArray() ->
                       if number.ValueKind = JsonValueKind.Null then Some None else GoJson.asFloat number |> Option.map Some |]

            if numbers |> Array.forall Option.isSome then
                Some { Pair = numbers |> Array.map Option.get; Raw = item }
            else
                None

    if list.ValueKind <> JsonValueKind.Array then
        None
    else
        let points = [| for item in list.EnumerateArray() -> pair item |]
        if points |> Array.forall Option.isSome then Some(points |> Array.map Option.get) else None

let decodeSeriesV1 (item: JsonElement) : Result<SeriesV1, string> =
    let empty: SeriesV1 =
        { Host = ""
          Interval = None
          Metric = ""
          Points = [||]
          Tags = [||]
          Type = ""
          Additional = Map.empty
          Unparsed = true
          Raw = item }

    match item.ValueKind with
    | JsonValueKind.Object
    | JsonValueKind.Null ->
        let declared = Declared item
        let host = declared.Read("host", GoJson.asString)
        let interval = declared.Read("interval", GoJson.asInt64)
        let metric = declared.Read("metric", GoJson.asString)
        let points = declared.Read("points", asSeriesPoints)
        let tags = declared.Read("tags", GoJson.asStrings)
        let kind = declared.Read("type", GoJson.asString)

        if declared.Misfit then
            unparsed item empty
        else
            match metric, points with
            | None, _ -> Error "required field metric missing"
            | _, None -> Error "required field points missing"
            | Some metric, Some points ->
                Ok
                    { Host = defaultArg host ""
                      Interval = interval
                      Metric = metric
                      Points = points
                      Tags = defaultArg tags [||]
                      Type = defaultArg kind ""
                      Additional = additional [ "host"; "interval"; "metric"; "points"; "tags"; "type" ] item
                      Unparsed = false
                      Raw = item }
    | _ -> Error(notAnObject item)

/// One element of a distribution point: the model takes it as a timestamp
/// or as the value list, never both.
type DistributionItem =
    | Timestamp of float
    | Values of float[]
    /// null: neither, and nothing for the model to keep.
    | Absent
    /// Anything else. The model keeps it unparsed.
    | Unfit

type DistributionPoint =
    { Items: DistributionItem[]
      Raw: JsonElement }

/// datadogV1.DistributionPointsSeries.
type DistributionSeries =
    { Host: string
      Metric: string
      Points: DistributionPoint[]
      Tags: string[]
      Additional: Map<string, JsonElement>
      /// Set in two unrelated cases: the item fit no shape (fields empty), or
      /// its `type` was not "distribution" (fields filled, only that word
      /// lost).
      Unparsed: bool
      Raw: JsonElement }

let private asDistributionItem (item: JsonElement) : DistributionItem option =
    match item.ValueKind with
    | JsonValueKind.Null -> Some Absent
    | JsonValueKind.Number -> GoJson.asFloat item |> Option.map Timestamp
    | _ ->
        match GoJson.asFloats item with
        | Some values -> Some(Values values)
        | None -> if (GoJson.overflowingNumber item).IsSome then None else Some Unfit

let private asDistributionPoints (list: JsonElement) : DistributionPoint[] option =
    let point (item: JsonElement) : DistributionPoint option =
        if item.ValueKind = JsonValueKind.Null then
            Some { Items = [||]; Raw = item }
        elif item.ValueKind <> JsonValueKind.Array then
            None
        else
            let items = [| for element in item.EnumerateArray() -> asDistributionItem element |]

            if items |> Array.forall Option.isSome then
                Some { Items = items |> Array.map Option.get; Raw = item }
            else
                None

    if list.ValueKind <> JsonValueKind.Array then
        None
    else
        let points = [| for item in list.EnumerateArray() -> point item |]
        if points |> Array.forall Option.isSome then Some(points |> Array.map Option.get) else None

let decodeDistributionSeries (item: JsonElement) : Result<DistributionSeries, string> =
    let empty: DistributionSeries =
        { Host = ""
          Metric = ""
          Points = [||]
          Tags = [||]
          Additional = Map.empty
          Unparsed = true
          Raw = item }

    match item.ValueKind with
    | JsonValueKind.Object
    | JsonValueKind.Null ->
        let declared = Declared item
        let host = declared.Read("host", GoJson.asString)
        let metric = declared.Read("metric", GoJson.asString)
        let points = declared.Read("points", asDistributionPoints)
        let tags = declared.Read("tags", GoJson.asStrings)
        let kind = declared.Read("type", GoJson.asString)

        if declared.Misfit then
            unparsed item empty
        else
            match metric, points with
            | None, _ -> Error "required field metric missing"
            | _, None -> Error "required field points missing"
            | Some metric, Some points ->
                let fitted =
                    { Host = defaultArg host ""
                      Metric = metric
                      Points = points
                      Tags = defaultArg tags [||]
                      Additional = additional [ "host"; "metric"; "points"; "tags"; "type" ] item
                      Unparsed = false
                      Raw = item }

                match kind with
                | Some word when word <> "distribution" -> unparsed item { fitted with Unparsed = true }
                | _ -> Ok fitted
    | _ -> Error(notAnObject item)

/// datadogV1.MetricsPayload and DistributionPointsPayload: `series`, and
/// whatever else the sender put beside it.
type SeriesPayload<'series> =
    { Series: 'series[]
      Additional: Map<string, JsonElement>
      /// The body fit no series list at all; Series is empty.
      Unparsed: bool }

let private parsePayload
    (label: string)
    (decode: JsonElement -> Result<'series, string>)
    (body: byte[])
    : Result<SeriesPayload<'series>, string> =
    match GoJson.parse body with
    | Error e -> Error $"{label}: {e}"
    | Ok root ->
        let whole () =
            match unparsed root { Series = [||]; Additional = Map.empty; Unparsed = true } with
            | Ok payload -> Ok payload
            | Error e -> Error $"{label}: {e}"

        match root.ValueKind with
        | JsonValueKind.Object
        | JsonValueKind.Null ->
            match GoJson.field "series" root with
            | None -> Error $"{label}: required field series missing"
            | Some list when list.ValueKind = JsonValueKind.Array ->
                // One series that is an error (not an object, or without a
                // required field) fails the list, and the model then keeps
                // the whole body unparsed.
                let decoded = [| for item in list.EnumerateArray() -> decode item |]

                let fitted =
                    decoded
                    |> Array.choose (fun result ->
                        match result with
                        | Ok series -> Some series
                        | Error _ -> None)

                if fitted.Length = decoded.Length then
                    Ok
                        { Series = fitted
                          Additional = additional [ "series" ] root
                          Unparsed = false }
                else
                    whole ()
            | Some _ -> whole ()
        | _ -> Error $"{label}: {notAnObject root}"

let parseSeriesV1 (body: byte[]) : Result<SeriesPayload<SeriesV1>, string> =
    parsePayload "JSON v1 series" decodeSeriesV1 body

let parseDistributionPoints (body: byte[]) : Result<SeriesPayload<DistributionSeries>, string> =
    parsePayload "JSON distribution_points" decodeDistributionSeries body

/// datadogV1.ServiceCheck.
type ServiceCheck =
    { Check: string
      HostName: string
      Message: string
      Status: int
      Tags: string[]
      Timestamp: int64
      Additional: Map<string, JsonElement>
      /// Nothing decoded, or the status is outside 0..3 — which leaves it at
      /// 0, and an unreadable check must never be stored as OK.
      Unparsed: bool
      Raw: JsonElement }

type CheckRuns =
    { /// The checks the model took, fitted or unparsed, in order.
      Runs: ServiceCheck list
      /// The items that are not checks at all, as they arrived.
      Undecodable: JsonElement list }

/// Decodes one check. The model marks `tags` required because Datadog's
/// OpenAPI spec does; the agent disagrees and sends `"tags": null` on checks
/// without any (containerd.health, the kubelet checks). With `supplyTags` an
/// empty list stands in for a missing or null one.
let decodeServiceCheck (supplyTags: bool) (item: JsonElement) : Result<ServiceCheck, string> =
    if item.ValueKind <> JsonValueKind.Object then
        Error(notAnObject item)
    else
        let empty: ServiceCheck =
            { Check = ""
              HostName = ""
              Message = ""
              Status = 0
              Tags = [||]
              Timestamp = 0L
              Additional = Map.empty
              Unparsed = true
              Raw = item }

        let hasTags =
            match (GoJson.members item).TryFind "tags" with
            | Some value -> value.ValueKind <> JsonValueKind.Null
            | None -> false

        let declared = Declared item
        let check = declared.Read("check", GoJson.asString)
        let hostName = declared.Read("host_name", GoJson.asString)
        let message = declared.Read("message", GoJson.asString)
        let status = declared.Read("status", GoJson.asInt32)
        let tags = if supplyTags && not hasTags then Some [||] else declared.Read("tags", GoJson.asStrings)
        let timestamp = declared.Read("timestamp", GoJson.asInt64)

        if declared.Misfit then
            unparsed item empty
        else
            match check, hostName, status, tags with
            | None, _, _, _ -> Error "required field check missing"
            | _, None, _, _ -> Error "required field host_name missing"
            | _, _, None, _ -> Error "required field status missing"
            | _, _, _, None -> Error "required field tags missing"
            | Some check, Some hostName, Some status, Some tags ->
                let valid = status >= 0 && status <= 3

                let fitted =
                    { Check = check
                      HostName = hostName
                      Message = defaultArg message ""
                      Status = if valid then status else 0
                      Tags = tags
                      Timestamp = defaultArg timestamp 0L
                      Additional = additional [ "check"; "host_name"; "message"; "status"; "tags"; "timestamp" ] item
                      Unparsed = not valid
                      Raw = item }

                if valid then Ok fitted else unparsed item fitted

/// Decodes a check_run batch item by item, so one odd check does not discard
/// the checks beside it. A body that is not an array is read as one check —
/// datadogpy posts a bare object — and there no tags are supplied, as in Go.
let parseCheckRuns (body: byte[]) : Result<CheckRuns, Problem> =
    match GoJson.parse body with
    | Error e -> Error(Undecodable $"JSON check_run: {e}")
    | Ok root when root.ValueKind = JsonValueKind.Null -> Ok { Runs = []; Undecodable = [] }
    | Ok root when root.ValueKind = JsonValueKind.Array ->
        let items = List.ofSeq (root.EnumerateArray())

        // Go's withTags writes into a nil map for a null item.
        if items |> List.exists (fun item -> item.ValueKind = JsonValueKind.Null) then
            Error Panics
        else
            let decoded = items |> List.map (fun item -> item, decodeServiceCheck true item)

            Ok
                { Runs =
                    decoded
                    |> List.choose (fun (_, result) ->
                        match result with
                        | Ok run -> Some run
                        | Error _ -> None)
                  Undecodable =
                    decoded
                    |> List.choose (fun (item, result) ->
                        match result with
                        | Ok _ -> None
                        | Error _ -> Some item) }
    | Ok root ->
        match decodeServiceCheck false root with
        | Ok run -> Ok { Runs = [ run ]; Undecodable = [] }
        | Error e -> Error(Undecodable $"JSON check_run: {e}")

/// The alert types and priorities Datadog's enums allow.
let eventAlertTypes =
    set [ "error"; "warning"; "info"; "success"; "user_update"; "recommendation"; "snapshot" ]

let eventPriorities = set [ "normal"; "low" ]

/// datadogV1.EventCreateRequest.
type EventRequest =
    { AggregationKey: string
      /// "" when not sent.
      AlertType: string
      DateHappened: int64
      DeviceName: string
      Host: string
      Priority: string option
      RelatedEventId: int64 option
      SourceTypeName: string
      /// None when not sent: the reply then says null, not [].
      Tags: string[] option
      Text: string
      Title: string
      Additional: Map<string, JsonElement>
      /// Nothing decoded (fields empty), or alert_type or priority is outside
      /// the enum (fields filled, that one left out).
      Unparsed: bool
      Raw: JsonElement }

let decodeEvent (item: JsonElement) : Result<EventRequest, string> =
    let empty: EventRequest =
        { AggregationKey = ""
          AlertType = ""
          DateHappened = 0L
          DeviceName = ""
          Host = ""
          Priority = None
          RelatedEventId = None
          SourceTypeName = ""
          Tags = None
          Text = ""
          Title = ""
          Additional = Map.empty
          Unparsed = true
          Raw = item }

    match item.ValueKind with
    | JsonValueKind.Object
    | JsonValueKind.Null ->
        let declared = Declared item
        let aggregationKey = declared.Read("aggregation_key", GoJson.asString)
        let alertType = declared.Read("alert_type", GoJson.asString)
        let dateHappened = declared.Read("date_happened", GoJson.asInt64)
        let deviceName = declared.Read("device_name", GoJson.asString)
        let host = declared.Read("host", GoJson.asString)
        let priority = declared.Read("priority", GoJson.asString)
        let relatedEventId = declared.Read("related_event_id", GoJson.asInt64)
        let sourceTypeName = declared.Read("source_type_name", GoJson.asString)
        let tags = declared.Read("tags", GoJson.asStrings)
        let text = declared.Read("text", GoJson.asString)
        let title = declared.Read("title", GoJson.asString)

        if declared.Misfit then
            unparsed item empty
        else
            match text, title with
            | None, _ -> Error "required field text missing"
            | _, None -> Error "required field title missing"
            | Some text, Some title ->
                let alertValid = alertType |> Option.forall eventAlertTypes.Contains
                let priorityValid = priority |> Option.forall eventPriorities.Contains

                let fitted =
                    { AggregationKey = defaultArg aggregationKey ""
                      AlertType = if alertValid then defaultArg alertType "" else ""
                      DateHappened = defaultArg dateHappened 0L
                      DeviceName = defaultArg deviceName ""
                      Host = defaultArg host ""
                      Priority = if priorityValid then priority else None
                      RelatedEventId = relatedEventId
                      SourceTypeName = defaultArg sourceTypeName ""
                      Tags = tags
                      Text = text
                      Title = title
                      Additional =
                        additional
                            [ "aggregation_key"; "alert_type"; "date_happened"; "device_name"; "host"; "priority"
                              "related_event_id"; "source_type_name"; "tags"; "text"; "title" ]
                            item
                      Unparsed = not (alertValid && priorityValid)
                      Raw = item }

                if fitted.Unparsed then unparsed item fitted else Ok fitted
    | _ -> Error(notAnObject item)

/// The events of a body: a list, or one bare object. One event that is an
/// error fails the whole body, and for a list the error reported is that
/// event's, not "this is not an object".
let parseEvents (body: byte[]) : Result<EventRequest list, string> =
    match GoJson.parse body with
    | Error e -> Error $"JSON events: {e}"
    | Ok root when root.ValueKind = JsonValueKind.Null -> Ok []
    | Ok root when root.ValueKind = JsonValueKind.Array ->
        let decoded = [ for item in root.EnumerateArray() -> decodeEvent item ]

        let firstError =
            decoded
            |> List.tryPick (fun result ->
                match result with
                | Error e -> Some e
                | Ok _ -> None)

        match firstError with
        | Some e -> Error $"JSON events: {e}"
        | None ->
            Ok(
                decoded
                |> List.choose (fun result ->
                    match result with
                    | Ok event -> Some event
                    | Error _ -> None)
            )
    | Ok root ->
        match decodeEvent root with
        | Ok event -> Ok [ event ]
        | Error e -> Error $"JSON events: {e}"
