namespace NinjaCat.Api.Intake

open System
open System.Globalization
open System.IO
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open System.Text.RegularExpressions
open System.Threading.Tasks
open Google.Protobuf
open Microsoft.Extensions.Logging
open Microsoft.AspNetCore.Http
open Oxpecker
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// Datadog's "key:value" tags.
module Tags =
    /// Splits at the FIRST colon: values legitimately contain colons
    /// ("url:http://x"). A bare tag keeps its key with an empty value.
    let split (tag: string) : string * string =
        match tag.IndexOf ':' with
        | -1 -> tag, ""
        | i -> tag.Substring(0, i), tag.Substring(i + 1)

    /// Key → every value seen, in arrival order. Tags are a multiset: two
    /// tags may share a key (kube_service:a and kube_service:b), and both
    /// must survive.
    let toMultiMap (tags: string seq) : Map<string, string[]> =
        let mutable byKey: Map<string, string list> = Map.empty

        for tag in tags do
            let key, value = split tag
            let seen = byKey.TryFind key |> Option.defaultValue []
            byKey <- byKey.Add(key, value :: seen)

        byKey |> Map.map (fun _ values -> values |> List.rev |> Array.ofList)

    /// For "key:value" lists whose keys are unique by contract (Kubernetes
    /// labels and annotations): the last value of a key wins.
    let toMap (pairs: string seq) : Map<string, string> = pairs |> Seq.map split |> Map.ofSeq

    /// `ddtags` is one comma-separated string.
    let splitDDTags (text: string) : string list =
        if String.IsNullOrEmpty text then [] else List.ofArray (text.Split ',')

module Text =
    let firstNonEmpty (values: string list) : string =
        values |> List.tryFind (fun v -> v <> "") |> Option.defaultValue ""

    let utf8 (bytes: byte[]) : string = Encoding.UTF8.GetString bytes

    /// ClickHouse has no Bool in these tables; flags are UInt8.
    let flag (value: bool) : uint8 = if value then 1uy else 0uy

    // .NET's "R" gives the shortest digits that round-trip; only the layout
    // is changed here, to the one the columns have held from the start
    // (printf's %g): plain digits between 1e-4 and 1e6, d.ddde±XX outside.
    let private floatLayout (roundTrip: string) : string =
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
            // The number is 0.<digits> × 10^point.
            let point = whole.Length + exponent - leadingZeros
            let power = point - 1

            if digits = "" then
                sign + "0"
            elif power < -4 || power >= 6 then
                let rest = if digits.Length > 1 then "." + digits.Substring 1 else ""
                let powerSign = if power < 0 then "-" else "+"
                sign + digits.Substring(0, 1) + rest + "e" + powerSign + (abs power).ToString "00"
            elif point <= 0 then
                sign + "0." + String('0', -point) + digits
            elif point >= digits.Length then
                sign + digits + String('0', point - digits.Length)
            else
                sign + digits.Substring(0, point) + "." + digits.Substring point

    /// A float as the text a column holds: exact, the shortest digits that
    /// give the same float back.
    let ofFloat (value: float) : string =
        floatLayout (value.ToString("R", CultureInfo.InvariantCulture))

    let ofFloat32 (value: float32) : string =
        floatLayout (value.ToString("R", CultureInfo.InvariantCulture))

/// Protobuf's variable-length integers, where they appear outside a message
/// the generated classes read: the dictionaries of metrics v3, the DNS
/// buffers of a connections payload.
module Varint =
    /// The varint at `position`: its value and the bytes it took. None when
    /// the bytes run out, or it is longer than a varint can be.
    let read (data: byte[]) (position: int) : (uint64 * int) option =
        if position < 0 || position >= data.Length then
            None
        else
            use input = new CodedInputStream(data, position, data.Length - position)
            let before = input.Position

            try
                let value = input.ReadUInt64()
                Some(value, int (input.Position - before))
            with :? InvalidProtocolBufferException ->
                None

module Time =
    let private rfc3339 =
        Regex(@"^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2}):(\d{2})(?:[.,](\d+))?(?:Z|([+-])(\d{2}):(\d{2}))$", RegexOptions.Compiled)

    /// An RFC 3339 timestamp as UTC, or None: a 'T', seconds and a zone are
    /// required, a fraction is optional (after '.' or ',', as ISO 8601
    /// allows), any offset up to 23:59. Digits past 100 ns are dropped, not
    /// rounded.
    let tryRfc3339 (text: string) : DateTime option =
        let m = rfc3339.Match text

        if not m.Success then
            None
        else
            let number (group: int) = int m.Groups[group].Value
            let year, month, day = number 1, number 2, number 3
            let hour, minute, second = number 4, number 5, number 6
            let offsetHours, offsetMinutes = (if m.Groups[9].Success then number 9 else 0), (if m.Groups[10].Success then number 10 else 0)

            let valid =
                year >= 1
                && month >= 1
                && month <= 12
                && day >= 1
                && day <= DateTime.DaysInMonth(year, month)
                && hour < 24
                && minute < 60
                && second < 60
                && offsetHours < 24
                && offsetMinutes < 60

            if not valid then
                None
            else
                let fraction =
                    if m.Groups[7].Success then
                        int64 (m.Groups[7].Value.PadRight(7, '0').Substring(0, 7))
                    else
                        0L

                let offset = TimeSpan(offsetHours, offsetMinutes, 0).Ticks * (if m.Groups[8].Value = "-" then -1L else 1L)
                let local = DateTime(year, month, day, hour, minute, second, DateTimeKind.Utc).Ticks + fraction
                // A time whose UTC form falls outside years 1 to 9999 is kept at the limit.
                let utc = max DateTime.MinValue.Ticks (min DateTime.MaxValue.Ticks (local - offset))
                Some(DateTime(utc, DateTimeKind.Utc))

    let private maxSeconds = DateTimeOffset.MaxValue.ToUnixTimeSeconds()
    let private minSeconds = DateTimeOffset.MinValue.ToUnixTimeSeconds()

    /// A timestamp past what DateTime holds (year 9999) is kept at that
    /// limit: one absurd value in a payload must not fail the request.
    let fromUnixSeconds (seconds: int64) : DateTime =
        DateTimeOffset.FromUnixTimeSeconds(max minSeconds (min maxSeconds seconds)).UtcDateTime

    let fromUnixMillis (ms: int64) : DateTime =
        DateTimeOffset.FromUnixTimeMilliseconds(max (minSeconds * 1000L) (min (maxSeconds * 1000L + 999L) ms)).UtcDateTime

    /// A timestamp off the wire, in seconds; the receive time when the sender
    /// left it out. Zero does not mean 1970, it means "not supplied" — and a
    /// row stamped 1970 falls out of retention before anyone sees it.
    let wireSeconds (seconds: int64) : DateTime =
        if seconds <= 0L then DateTime.UtcNow else fromUnixSeconds seconds

    /// The same, for milliseconds.
    let wireMillis (millis: int64) : DateTime =
        if millis <= 0L then DateTime.UtcNow else fromUnixMillis millis

    /// A protobuf Timestamp as a time, to 100 ns. None when it is absent or
    /// not a valid Timestamp: seconds outside the years 1 to 9999, or nanos
    /// outside one second.
    let ofTimestamp (timestamp: Google.Protobuf.WellKnownTypes.Timestamp) : DateTime option =
        if isNull timestamp then
            None
        else
            try
                Some(timestamp.ToDateTime())
            with :? InvalidOperationException ->
                None

    /// A time that may be absent: None unless `present` and positive.
    let optionalSeconds (present: bool) (seconds: int64) : DateTime option =
        if present && seconds > 0L then Some(fromUnixSeconds seconds) else None

module Json =
    // A valid surrogate pair, half of one, or any other escape. Matching
    // every escape is what keeps `\\ud800` (a backslash, then text) apart.
    let private escapes =
        Regex(
            @"\\u[dD][89abAB][0-9a-fA-F]{2}\\u[dD][c-fC-F][0-9a-fA-F]{2}|\\u[dD][89a-fA-F][0-9a-fA-F]{2}|\\.",
            RegexOptions.Compiled ||| RegexOptions.Singleline
        )

    /// Senders do write strings holding bytes that are not UTF-8, or half
    /// of a surrogate pair (Python writes `\udc80` for a byte it could not
    /// decode). System.Text.Json parses such a body and then throws when the
    /// string is read, which would turn one odd character into a 500. So a
    /// body is repaired before it is parsed: U+FFFD takes their place.
    let repair (body: byte[]) : byte[] =
        let span = ReadOnlySpan body
        let mayHoldHalfPair = span.IndexOf(ReadOnlySpan "\\ud"B) >= 0 || span.IndexOf(ReadOnlySpan "\\uD"B) >= 0

        if System.Text.Unicode.Utf8.IsValid span && not mayHoldHalfPair then
            body
        else
            let text = Encoding.UTF8.GetString body
            // Of the three alternatives only half a pair is six characters long.
            Encoding.UTF8.GetBytes(escapes.Replace(text, (fun found -> if found.Length = 6 then "\\ufffd" else found.Value)))

    /// How deep a JSON body may nest, for every parser of the intake.
    ///
    /// A limit has to exist: several functions here walk a document by
    /// recursion, and a stack overflow in .NET ends the process. It also has
    /// to stay below what Utf8JsonWriter will write back (1000), or a body
    /// that parsed would throw when a column is written. No telemetry nests
    /// anywhere near this; a body that does is kept in raw_payloads.
    let maxDepth = 512

    let private documentOptions = JsonDocumentOptions(MaxDepth = maxDepth)

    /// The body as JSON, or the parser's message.
    let tryParse (body: byte[]) : Result<JsonElement, string> =
        try
            use doc = JsonDocument.Parse(ReadOnlyMemory(repair body), documentOptions)
            Ok(doc.RootElement.Clone())
        with e ->
            Error e.Message

    /// The first JSON value of a body; what follows it is not looked at.
    let tryParseFirst (body: byte[]) : Result<JsonElement, string> =
        try
            let options = JsonReaderOptions(AllowMultipleValues = true, MaxDepth = maxDepth)
            let mutable reader = Utf8JsonReader(ReadOnlySpan(repair body), options)

            if reader.Read() then Ok(JsonElement.ParseValue(&reader)) else Error "the body is empty"
        with e ->
            Error e.Message

    /// The body as a JSON object; None when it is anything else.
    let tryParseObject (body: byte[]) : JsonElement option =
        match tryParse body with
        | Ok value when value.ValueKind = JsonValueKind.Object -> Some value
        | _ -> None

    /// A member of an object: of two with the same name, the last. None
    /// when the value is not an object or has no such member.
    let tryProperty (name: string) (object: JsonElement) : JsonElement option =
        if object.ValueKind <> JsonValueKind.Object then
            None
        else
            match object.TryGetProperty name with
            | true, value -> Some value
            | false, _ -> None

    /// The name of a value's type, as it appears in a note.
    let kind (value: JsonElement) : string =
        match value.ValueKind with
        | JsonValueKind.Object -> "object"
        | JsonValueKind.Array -> "array"
        | JsonValueKind.String -> "string"
        | JsonValueKind.Number -> "number"
        | JsonValueKind.True
        | JsonValueKind.False -> "bool"
        | _ -> "null"

    /// An object's members by name; of two with the same name the last one
    /// counts. Anything but an object has none.
    let members (value: JsonElement) : Map<string, JsonElement> =
        if value.ValueKind <> JsonValueKind.Object then
            Map.empty
        else
            value.EnumerateObject() |> Seq.map (fun p -> p.Name, p.Value) |> Map.ofSeq

    /// A member's value; None when there is none or it is null.
    let field (name: string) (object: JsonElement) : JsonElement option =
        tryProperty name object |> Option.filter (fun value -> value.ValueKind <> JsonValueKind.Null)

    let private numberSyntax =
        Regex(@"\A-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?\z", RegexOptions.Compiled)

    /// A number as it was written, so no digit is lost. A string that holds
    /// a number is taken too.
    let numberText (value: JsonElement) : string option =
        match value.ValueKind with
        | JsonValueKind.Number -> Some(value.GetRawText())
        | JsonValueKind.String ->
            let text = value.GetString()
            if numberSyntax.IsMatch text then Some text else None
        | _ -> None

    let stringOf (value: JsonElement) : string option =
        if value.ValueKind = JsonValueKind.String then Some(value.GetString()) else None

    let int64Of (value: JsonElement) : int64 option =
        if value.ValueKind <> JsonValueKind.Number then
            None
        else
            match Int64.TryParse(value.GetRawText(), NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture) with
            | true, n -> Some n
            | false, _ -> None

    let int32Of (value: JsonElement) : int32 option =
        int64Of value |> Option.filter (fun n -> n >= int64 Int32.MinValue && n <= int64 Int32.MaxValue) |> Option.map int32

    let uint64Of (value: JsonElement) : uint64 option =
        if value.ValueKind <> JsonValueKind.Number then
            None
        else
            match UInt64.TryParse(value.GetRawText(), NumberStyles.None, CultureInfo.InvariantCulture) with
            | true, n -> Some n
            | false, _ -> None

    /// A number as a float; one too large for a float is refused.
    let floatOf (value: JsonElement) : float option =
        if value.ValueKind <> JsonValueKind.Number then
            None
        else
            match Double.TryParse(value.GetRawText(), NumberStyles.Float, CultureInfo.InvariantCulture) with
            | true, f when Double.IsFinite f -> Some f
            | _ -> None

    /// A number as an integer where producers write whole quantities as
    /// floats: one with a fraction or an exponent is truncated rather than
    /// lost.
    let truncatedInt64Of (value: JsonElement) : int64 option =
        match int64Of value, floatOf value with
        | Some n, _ -> Some n
        | None, Some f -> Some(int64 f)
        | None, None -> None

    let boolOf (value: JsonElement) : bool option =
        match value.ValueKind with
        | JsonValueKind.True -> Some true
        | JsonValueKind.False -> Some false
        | _ -> None

    let objectOf (value: JsonElement) : JsonElement option =
        if value.ValueKind = JsonValueKind.Object then Some value else None

    /// The elements read with `read`, a null element as `zero`; None when the
    /// value is not an array or an element does not fit.
    let listOf (zero: 'a) (read: JsonElement -> 'a option) (value: JsonElement) : 'a[] option =
        if value.ValueKind <> JsonValueKind.Array then
            None
        else
            let items =
                value.EnumerateArray()
                |> Seq.map (fun item -> if item.ValueKind = JsonValueKind.Null then Some zero else read item)
                |> Array.ofSeq

            if items |> Array.forall Option.isSome then Some(items |> Array.map Option.get) else None

    let stringsOf (value: JsonElement) : string[] option = listOf "" stringOf value

    let floatsOf (value: JsonElement) : float[] option = listOf 0.0 floatOf value

    /// An object whose values are all strings; a null value is "".
    let stringMapOf (value: JsonElement) : Map<string, string> option =
        if value.ValueKind <> JsonValueKind.Object then
            None
        else
            let entries =
                members value |> Map.map (fun _ v -> if v.ValueKind = JsonValueKind.Null then Some "" else stringOf v)

            if entries |> Map.forall (fun _ v -> v.IsSome) then Some(entries |> Map.map (fun _ v -> v.Value)) else None

    /// The bytes a value arrived as.
    let rawBytes (value: JsonElement) : byte[] = Encoding.UTF8.GetBytes(value.GetRawText())

    // --- writing ---

    // Relaxed escaping: only what JSON requires is escaped, so the text of a
    // column stays searchable (`<`, `>`, `&` and non-ASCII as they are).
    let private writerOptions =
        JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping)

    /// The text as a JSON string literal.
    let quoted (text: string) : string =
        "\"" + JsonEncodedText.Encode(text, JavaScriptEncoder.UnsafeRelaxedJsonEscaping).ToString() + "\""

    /// What `write` writes, as JSON text.
    let write (write: Utf8JsonWriter -> unit) : string =
        use buffer = new MemoryStream()

        do
            use writer = new Utf8JsonWriter(buffer, writerOptions)
            write writer

        Encoding.UTF8.GetString(buffer.ToArray())

    /// The value written again without whitespace, its members in the order
    /// they came. Numbers keep their digits.
    let compact (value: JsonElement) : string = write value.WriteTo

    /// The values as one JSON array.
    let compactArray (values: JsonElement seq) : string =
        "[" + String.Join(",", values |> Seq.map compact) + "]"

    /// The members as one JSON object, sorted by name; "{}" when there are none.
    let compactObject (values: Map<string, JsonElement>) : string =
        write (fun writer ->
            writer.WriteStartObject()

            for pair in values do
                writer.WritePropertyName pair.Key
                pair.Value.WriteTo writer

            writer.WriteEndObject())

    let rec private writeSorted (writer: Utf8JsonWriter) (value: JsonElement) : unit =
        match value.ValueKind with
        | JsonValueKind.Object ->
            writer.WriteStartObject()

            for pair in members value do
                writer.WritePropertyName pair.Key
                writeSorted writer pair.Value

            writer.WriteEndObject()
        | JsonValueKind.Array ->
            writer.WriteStartArray()

            for item in value.EnumerateArray() do
                writeSorted writer item

            writer.WriteEndArray()
        | _ -> value.WriteTo writer

    /// The same with the keys of every object sorted, and of two equal keys
    /// only the last: one text for one value, however it was written.
    let compactSorted (value: JsonElement) : string =
        write (fun writer -> writeSorted writer value)

    // --- reading without notes ---
    //
    // For documents with no schema to hold them to: a value is a
    // `JsonElement option`, None when the member is missing or null. What
    // fits is kept, anything else reads as empty, and nothing is noted.

    /// A member of a value that may itself be missing.
    let child (name: string) (parent: JsonElement option) : JsonElement option =
        parent |> Option.bind (field name)

    /// The value at the end of a path of member names.
    let at (path: string list) (root: JsonElement) : JsonElement option =
        List.fold (fun current name -> child name current) (Some root) path
        |> Option.filter (fun value -> value.ValueKind <> JsonValueKind.Null)

    let lenientString (value: JsonElement option) : string =
        value |> Option.bind stringOf |> Option.defaultValue ""

    let lenientInt64 (value: JsonElement option) : int64 =
        value |> Option.bind int64Of |> Option.defaultValue 0L

    /// A list of strings, an element that is not a string as "". None when
    /// the value is absent, null or not a list.
    let lenientStrings (value: JsonElement option) : string[] option =
        match value with
        | Some v when v.ValueKind = JsonValueKind.Array ->
            Some [| for item in v.EnumerateArray() -> lenientString (Some item) |]
        | _ -> None

    /// An object of string lists: a key whose value is not a list keeps the
    /// key, with no values.
    let lenientStringLists (value: JsonElement option) : Map<string, string[]> =
        match value with
        | Some v -> members v |> Map.map (fun _ list -> lenientStrings (Some list) |> Option.defaultValue [||])
        | None -> Map.empty

    /// The members of an object, else none.
    let lenientMembers (value: JsonElement option) : Map<string, JsonElement> =
        match value with
        | Some v -> members v
        | None -> Map.empty

    /// The elements of a list, else none.
    let lenientItems (value: JsonElement option) : JsonElement list =
        match value with
        | Some list when list.ValueKind = JsonValueKind.Array -> List.ofSeq (list.EnumerateArray())
        | _ -> []

    /// A JSON bool as 1 or 0; None for anything else: "never said" is not
    /// "false".
    let flag (value: JsonElement option) : uint8 option =
        value |> Option.bind boolOf |> Option.map Text.flag

    /// A value as the text a column holds: a string as it is, a number digit
    /// for digit (an id above 2^53 must not pass through a float), true or
    /// false, an object or a list as JSON with sorted keys; "" when there is
    /// none.
    let text (value: JsonElement option) : string =
        match value with
        | None -> ""
        | Some v ->
            match v.ValueKind with
            | JsonValueKind.String -> v.GetString()
            | JsonValueKind.Null -> ""
            | JsonValueKind.Object
            | JsonValueKind.Array -> compactSorted v
            | _ -> v.GetRawText()

    /// A value as JSON text for a column, compact and in the order it was
    /// sent; "" when there is none.
    let compactOrEmpty (value: JsonElement option) : string =
        match value with
        | Some v -> compact v
        | None -> ""

    /// The same with sorted keys.
    let compactSortedOrEmpty (value: JsonElement option) : string =
        match value with
        | Some v -> compactSorted v
        | None -> ""

    /// A list as texts. A lone string that is not empty is a list of one:
    /// several producers send a single tag as a bare string.
    let textList (value: JsonElement option) : string[] =
        match value with
        | Some v when v.ValueKind = JsonValueKind.Array -> [| for item in v.EnumerateArray() -> text (Some item) |]
        | Some v when v.ValueKind = JsonValueKind.String && v.GetString() <> "" -> [| v.GetString() |]
        | _ -> [||]

    /// A tag list in either spelling: a list of strings, or one
    /// comma-separated string, as some producers send `ddtags`. Anything
    /// else is no tags at all.
    let tagList (value: JsonElement option) : string list =
        match value with
        | Some v when v.ValueKind = JsonValueKind.String -> Tags.splitDDTags (v.GetString())
        | Some v -> stringsOf v |> Option.map List.ofArray |> Option.defaultValue []
        | None -> []

    /// The JSON text of a value exactly as it arrived, `null` included; ""
    /// when the member was not sent at all.
    let rawOrEmpty (value: JsonElement option) : string =
        match value with
        | Some v -> v.GetRawText()
        | None -> ""

    /// Members as the texts of a Map(String, String) column, each value as
    /// the JSON `write` gives it. A null member is the text `null`: an empty
    /// string is a value too, and not the one that was sent.
    let memberTexts (write: JsonElement -> string) (members: Map<string, JsonElement>) : Map<string, string> =
        members |> Map.map (fun _ value -> write value)

    /// The members of an object that have no column of their own, those not
    /// in `known`, as such texts.
    let otherMembers (known: Set<string>) (write: JsonElement -> string) (object: JsonElement option) : Map<string, string> =
        lenientMembers object |> Map.filter (fun name _ -> not (known.Contains name)) |> memberTexts write

    /// A body as its items: the elements of a list, or the one object of a
    /// sender that skipped the batch. `null` is no items.
    let tryParseList (body: byte[]) : Result<JsonElement list, string> =
        match tryParse body with
        | Error e -> Error e
        | Ok root ->
            match root.ValueKind with
            | JsonValueKind.Array -> Ok(List.ofSeq (root.EnumerateArray()))
            | JsonValueKind.Object -> Ok [ root ]
            | JsonValueKind.Null -> Ok []
            | _ -> Error $"expected a list or an object, got {kind root}"

    /// A body that has to be a list, as its elements. `null` is no elements;
    /// one bare object is refused, which is what keeps the agent's `{}` probe
    /// from becoming an empty row.
    let tryParseArray (body: byte[]) : Result<JsonElement list, string> =
        match tryParse body with
        | Error e -> Error e
        | Ok root ->
            match root.ValueKind with
            | JsonValueKind.Array -> Ok(List.ofSeq (root.EnumerateArray()))
            | JsonValueKind.Null -> Ok []
            | _ -> Error $"expected a list, got {kind root}"

    /// The elements of a JSON list, each as the bytes it had on the wire: for
    /// bodies whose elements are stored as they were sent. `null` is no
    /// elements.
    let tryParseElementBytes (body: byte[]) : Result<byte[] list, string> =
        try
            let mutable reader = Utf8JsonReader(ReadOnlySpan body, JsonReaderOptions(MaxDepth = maxDepth))
            reader.Read() |> ignore

            match reader.TokenType with
            | JsonTokenType.StartArray ->
                let elements = ResizeArray<byte[]>()

                while reader.Read() && reader.TokenType <> JsonTokenType.EndArray do
                    let start = int reader.TokenStartIndex
                    reader.Skip()
                    elements.Add(body[start .. int reader.BytesConsumed - 1])

                // Anything after the list is an error, raised by this read.
                reader.Read() |> ignore
                Ok(List.ofSeq elements)
            | first ->
                reader.Skip()
                reader.Read() |> ignore

                match first with
                | JsonTokenType.Null -> Ok []
                | JsonTokenType.StartObject -> Error "expected a list, got object"
                | JsonTokenType.String -> Error "expected a list, got string"
                | JsonTokenType.Number -> Error "expected a list, got number"
                | _ -> Error "expected a list, got bool"
        with :? JsonException as e ->
            Error e.Message

    /// The object's property names, sorted and comma-separated, for a log
    /// line or a note that says what was there.
    let keys (object: JsonElement) : string =
        if object.ValueKind <> JsonValueKind.Object then
            "(none)"
        else
            let names = object.EnumerateObject() |> Seq.map _.Name |> Seq.sort |> List.ofSeq
            if names.IsEmpty then "(none)" else String.Join(", ", names)

/// Protobuf messages as JSON text, written and read by the protobuf library
/// in proto3's JSON mapping: lowerCamelCase names, 64-bit integers as
/// strings, enums by name, fields at their default left out.
module ProtoJson =
    let private parser = JsonParser(JsonParser.Settings.Default.WithIgnoreUnknownFields true)

    /// A JSON body as the message of its shape. The parser takes a field
    /// under the name the schema gives it or in lowerCamelCase, an enum by
    /// name or by number, and passes over keys the schema does not have.
    let tryParse<'message when 'message :> IMessage and 'message: (new: unit -> 'message)> (body: byte[]) : Result<'message, string> =
        try
            Ok(parser.Parse<'message>(Encoding.UTF8.GetString(Json.repair body)))
        with e ->
            Error e.Message

    /// "" for a message that is absent.
    let message (value: IMessage) : string =
        if isNull value then "" else JsonFormatter.Default.Format value

    /// A list of messages as a JSON array; "" for an empty list.
    let messages (values: seq<'message> when 'message :> IMessage) : string =
        if Seq.isEmpty values then
            ""
        else
            "[" + String.Join(",", values |> Seq.map (fun value -> JsonFormatter.Default.Format value)) + "]"

    /// A map of messages as a JSON object, in the map's own order; "" for an
    /// empty map.
    let messageMap (entries: seq<Collections.Generic.KeyValuePair<'key, 'message>> when 'message :> IMessage) : string =
        if Seq.isEmpty entries then
            ""
        else
            let entry (pair: Collections.Generic.KeyValuePair<'key, 'message>) =
                JsonSerializer.Serialize(string pair.Key) + ":" + JsonFormatter.Default.Format pair.Value

            "{" + String.Join(",", entries |> Seq.map entry) + "}"

/// Enum values of the generated protobuf classes, as text.
module ProtoEnum =
    let private namesByType =
        Collections.Concurrent.ConcurrentDictionary<Type, Collections.Generic.Dictionary<int, string>>()

    /// Number to name, read off the attribute the generator puts on every
    /// value. Of two names for one number, the first declared.
    let private namesOf (enumType: Type) : Collections.Generic.Dictionary<int, string> =
        let names = Collections.Generic.Dictionary<int, string>()

        for field in enumType.GetFields(Reflection.BindingFlags.Public ||| Reflection.BindingFlags.Static) do
            let original = Reflection.CustomAttributeExtensions.GetCustomAttribute<Google.Protobuf.Reflection.OriginalNameAttribute> field

            if not (isNull original) then
                names.TryAdd(Convert.ToInt32(field.GetValue null), original.Name) |> ignore

        names

    /// The value under the name the .proto gives it. Names, never numbers: a
    /// renumbering upstream must not rewrite stored history. A value the
    /// schema does not have is its number.
    let name (value: 'enum when 'enum: enum<int32>) : string =
        let number = LanguagePrimitives.EnumToValue value

        match namesByType.GetOrAdd(typeof<'enum>, namesOf).TryGetValue number with
        | true, name -> name
        | false, _ -> string number

/// Small conversions shared by the wire formats.
module Wire =
    /// A metric type number as its name. Zero is UNSPECIFIED and not rare:
    /// Datadog's own documentation example sends it. It is stored as sent;
    /// resolving it is a decision for query time.
    let metricTypeName (value: int) : string =
        match value with
        | 1 -> "COUNT"
        | 2 -> "RATE"
        | 3 -> "GAUGE"
        | _ -> "UNSPECIFIED"

    /// Node counts per version, as the unsigned numbers the column holds. A
    /// negative count cannot be real and would wrap to billions if cast.
    let versionSpread (counts: seq<string * int>) : Map<string, uint32> =
        counts |> Seq.map (fun (version, nodes) -> version, uint32 (max nodes 0)) |> Map.ofSeq

module Raw =
    /// Request headers kept with a raw payload. An allowlist: a blocklist is
    /// one forgotten header away from writing Dd-Api-Key into a table. These
    /// identify the sender; none is a secret.
    let private keptHeaders =
        [ "Via"; "User-Agent"; "Dd-Evp-Origin"; "Dd-Evp-Origin-Version"; "Dd-Request-Id"
          "Dd-Agent-Hostname"; "Dd-Agent-Env"; "Datadog-Container-Id"; "X-Datadog-Additional-Tags"
          "X-Datadog-Container-Tags"; "X-Requested-With"; "X-Dd-Hostname"; "X-Dd-Processagentversion"
          "X-Dd-Request-Id" ]

    /// The agent probes every intake at startup with nothing, `{}` or `[]`.
    let isProbe (body: byte[]) : bool =
        let text = (Text.utf8 body).Trim()
        text = "" || text = "{}" || text = "[]"

    /// The note of a payload with no published schema: the event platform's
    /// origin headers and the Content-Type are all the context there is
    /// beside the opaque bytes.
    let originNote (ctx: HttpContext) : string =
        let header (name: string) = Json.quoted (Ctx.header ctx name)
        $"""DD-EVP-ORIGIN={header "DD-EVP-ORIGIN"} DD-EVP-ORIGIN-VERSION={header "DD-EVP-ORIGIN-VERSION"} Content-Type={header "Content-Type"}"""

    /// Keeps a request that could not be turned into rows.
    ///
    /// The rule of this project: a handler that cannot decode a body, or gets
    /// a shape it has no schema for, hands the bytes here. A payload that
    /// would show what an intake sends arrives once, from someone's real
    /// agent; keeping it turns reverse-engineering into a SELECT. Bodies that
    /// did become rows are never stored here as well.
    ///
    ///   intake  the endpoint family ("logs", "trace", "rum")
    ///   reason  "no_schema" | "decode_error" | "unexpected_shape"
    ///   note    the decoder's error, the field that was missing
    ///   body    what the handler tried to decode, already decompressed
    let store (ctx: HttpContext) (intake: string) (reason: string) (note: string) (body: byte[]) : unit =
        let tenant = Ctx.tenant ctx
        let log = Ctx.log ctx
        let sink = Ctx.sink ctx

        if tenant = "" then
            log.LogWarning("[raw] {Intake} {Path}: no tenant, payload dropped ({Bytes} B)", intake, Secrets.pathWithoutKey ctx.Request.Path.Value, body.Length)
        elif isProbe body then
            log.LogDebug("[raw] {Intake} {Path}: empty probe body, not stored", intake, Secrets.pathWithoutKey ctx.Request.Path.Value)
        else
            let headers =
                keptHeaders
                |> List.choose (fun name ->
                    match Ctx.header ctx name with
                    | "" -> None
                    | value -> Some(name, value))
                |> Map.ofList

            // Without the key: a table is no place for a credential.
            let query =
                ctx.Request.Query
                |> Seq.filter (fun pair -> not (Secrets.queryParameters.Contains(pair.Key.ToLowerInvariant())))
                |> Seq.map (fun pair -> pair.Key, pair.Value.ToArray())
                |> Map.ofSeq

            let row: RawPayloadRow =
                { TenantID = tenant
                  ReceivedAt = DateTime.UtcNow
                  Intake = intake
                  Reason = reason
                  Method = ctx.Request.Method
                  // The Host header is what the whole intake dispatches on.
                  Host = ctx.Request.Host.Value
                  Path = Secrets.pathWithoutKey ctx.Request.Path.Value
                  Query = query
                  ContentType = Ctx.header ctx "Content-Type"
                  ContentEncoding = Ctx.header ctx "Content-Encoding"
                  Headers = headers
                  Body = body
                  BodyBytes = uint64 body.Length
                  Note = note }

            Sink.write sink RawPayloads.table [| row |]

            // The one line about a kept payload; handlers do not log it again.
            // An intake with no schema keeps everything it gets, which is
            // its ordinary work and not worth a warning each time.
            if reason = "no_schema" then
                log.LogDebug("[raw] {Intake} {Path}: {Reason}: {Note} ({Bytes} B)", intake, row.Path, reason, note, body.Length)
            else
                log.LogWarning("[raw] {Intake} {Path}: {Reason}: {Note} ({Bytes} B)", intake, row.Path, reason, note, body.Length)

/// Counters the intake keeps about itself, stored as ordinary metrics
/// (`ninjacat.intake.<name>`) so they can be graphed like any other.
module SelfMetrics =
    let imagesSkipped = "images.skipped"

    let count (ctx: HttpContext) (host: string) (name: string) (value: float) (tags: Map<string, string[]>) : unit =
        if value <> 0.0 then
            let point =
                { Metrics.point (Ctx.tenant ctx) DateTime.UtcNow ("ninjacat.intake." + name) value with
                    Host = host
                    MetricType = "COUNT"
                    SourceType = "ninjacat"
                    Tags = tags }

            Ctx.write ctx Metrics.table [| point |]

module Diagnose =
    /// The agent's connectivity sweep at startup: an empty body or `{}`,
    /// nothing to decode.
    let isSweep (ctx: HttpContext) : bool =
        Ctx.header ctx "X-Requested-With" = "datadog-agent-diagnose"
