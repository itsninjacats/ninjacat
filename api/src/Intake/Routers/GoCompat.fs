namespace NinjaCat.Api.Intake.Routers

open System
open System.Globalization
open System.IO
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open System.Text.RegularExpressions
open System.Text.Unicode
open NinjaCat.Api.Intake

/// Unix times as DateTime. Go's time.Time holds any int64; a DateTime does
/// not, and a garbage timestamp must not fail the request, so a value out of
/// range becomes the nearest instant a DateTime can hold.
module GoTime =
    let private minSeconds = -62135596800L
    let private maxSeconds = 253402300799L

    let fromUnixSeconds (seconds: int64) : DateTime =
        Time.fromUnixSeconds (Math.Clamp(seconds, minSeconds, maxSeconds))

    let fromUnixMillis (millis: int64) : DateTime =
        Time.fromUnixMillis (Math.Clamp(millis, minSeconds * 1000L, maxSeconds * 1000L + 999L))

/// Reads JSON under the rules of Go's encoding/json, which the Go intake
/// decoded with. The rules decide what is stored and what is refused:
///
///   - a key matches a field whatever its case, and the last match wins;
///   - null, like a missing key, leaves a field at its zero value;
///   - an integer is read from the literal's digits: 1.0 and 1e2 are refused;
///   - a value of the wrong type does not stop the read. The rest is still
///     read, and the document as a whole is refused afterwards.
module GoJson =
    // Go nests to 10000; System.Text.Json stops at 64 unless told otherwise.
    let private documentOptions = JsonDocumentOptions(MaxDepth = 10_000)

    // A valid surrogate pair, half of one, or any other escape. Matching
    // every escape is what keeps `\\ud800` (a backslash, then text) apart.
    let private escapes =
        Regex(
            @"\\u[dD][89abAB][0-9a-fA-F]{2}\\u[dD][c-fC-F][0-9a-fA-F]{2}|\\u[dD][89a-fA-F][0-9a-fA-F]{2}|\\.",
            RegexOptions.Compiled ||| RegexOptions.Singleline
        )

    /// Go reads a string holding bytes that are not UTF-8, or half of a
    /// surrogate pair (Python writes `\udc80` for a byte it could not decode),
    /// with U+FFFD in their place. System.Text.Json parses such a body and
    /// then throws when the string is read. So the body is repaired first.
    let private repaired (body: byte[]) : byte[] =
        let span = ReadOnlySpan body
        let mayHoldHalfPair = span.IndexOf(ReadOnlySpan "\\ud"B) >= 0 || span.IndexOf(ReadOnlySpan "\\uD"B) >= 0

        if Utf8.IsValid span && not mayHoldHalfPair then
            body
        else
            let text = Encoding.UTF8.GetString body
            // Of the three alternatives only half a pair is six characters long.
            Encoding.UTF8.GetBytes(escapes.Replace(text, (fun found -> if found.Length = 6 then "\\ufffd" else found.Value)))

    /// The body as JSON, or the parser's message.
    let parse (body: byte[]) : Result<JsonElement, string> =
        try
            use doc = JsonDocument.Parse(ReadOnlyMemory(repaired body), documentOptions)
            Ok(doc.RootElement.Clone())
        with e ->
            Error e.Message

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

    /// An object's members by name. As in a Go map, the last of two equal
    /// keys wins. Anything but an object has none.
    let members (value: JsonElement) : Map<string, JsonElement> =
        if value.ValueKind <> JsonValueKind.Object then
            Map.empty
        else
            value.EnumerateObject() |> Seq.map (fun p -> p.Name, p.Value) |> Map.ofSeq

    let private numberSyntax =
        Regex(@"\A-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?\z", RegexOptions.Compiled)

    /// Go's json.Number: the number as it was written, so no digit is lost.
    /// A string that holds a number is taken too.
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

    let boolOf (value: JsonElement) : bool option =
        match value.ValueKind with
        | JsonValueKind.True -> Some true
        | JsonValueKind.False -> Some false
        | _ -> None

    /// The elements read with `read`, a null element as `zero`; None when the
    /// value is not an array or an element does not fit.
    let private listOf (zero: 'a) (read: JsonElement -> 'a option) (value: JsonElement) : 'a[] option =
        if value.ValueKind <> JsonValueKind.Array then
            None
        else
            let items =
                value.EnumerateArray()
                |> Seq.map (fun item -> if item.ValueKind = JsonValueKind.Null then Some zero else read item)
                |> Array.ofSeq

            if items |> Array.forall Option.isSome then Some(items |> Array.map Option.get) else None

    let stringsOf (value: JsonElement) : string[] option = listOf "" stringOf value

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
        | JsonValueKind.Number -> writer.WriteRawValue(value.GetRawText(), true)
        | _ -> value.WriteTo writer

    /// The value as Go's json.Marshal writes back what it decoded: compact,
    /// object keys sorted, numbers digit for digit.
    let compact (value: JsonElement) : string =
        use buffer = new MemoryStream()

        do
            use writer = new Utf8JsonWriter(buffer, JsonWriterOptions(Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping))
            writeSorted writer value

        Encoding.UTF8.GetString(buffer.ToArray())

    /// What did not fit while one document was read, in reading order.
    type Mismatches = ResizeArray<string>

    /// A JSON object read as a Go struct. `path` says where it sits in the
    /// document, for the notes. A value that is not an object has no members.
    type Fields(bad: Mismatches, path: string, value: JsonElement) =
        let where (name: string) : string = if path = "" then name else path + "." + name

        /// The member's value, null included; None when there is none.
        member _.Find(name: string) : JsonElement option =
            if value.ValueKind <> JsonValueKind.Object then
                None
            else
                let mutable found = None

                for property in value.EnumerateObject() do
                    if property.Name.Equals(name, StringComparison.OrdinalIgnoreCase) then
                        found <- Some property.Value

                found

        /// Notes a member whose value does not fit.
        member _.Refuse(name: string, expected: string, got: JsonElement) : unit =
            bad.Add $"{where name}: expected {expected}, got {kind got}"

        /// The member read with `read`. A missing member and a null give
        /// `zero`; so does a value that does not fit, which is also noted.
        member this.Read(name: string, expected: string, zero: 'a, read: JsonElement -> 'a option) : 'a =
            match this.Find name with
            | None -> zero
            | Some found when found.ValueKind = JsonValueKind.Null -> zero
            | Some found ->
                match read found with
                | Some result -> result
                | None ->
                    this.Refuse(name, expected, found)
                    zero

        member this.String(name: string) : string = this.Read(name, "a string", "", stringOf)

        /// A json.Number: the digits as written, or "".
        member this.Number(name: string) : string = this.Read(name, "a number", "", numberText)

        member this.Int64(name: string) : int64 = this.Read(name, "an integer", 0L, int64Of)

        member this.OptionalInt32(name: string) : int32 option =
            let read (found: JsonElement) =
                match int64Of found with
                | Some n when n >= int64 Int32.MinValue && n <= int64 Int32.MaxValue -> Some(Some(int32 n))
                | _ -> None

            this.Read(name, "a 32-bit integer", None, read)

        member this.Int32(name: string) : int32 = this.OptionalInt32 name |> Option.defaultValue 0

        member this.UInt64(name: string) : uint64 = this.Read(name, "an unsigned integer", 0UL, uint64Of)

        member this.UInt32(name: string) : uint32 =
            let read (found: JsonElement) =
                uint64Of found |> Option.filter (fun n -> n <= uint64 UInt32.MaxValue) |> Option.map uint32

            this.Read(name, "an unsigned 32-bit integer", 0u, read)

        member this.UInt16(name: string) : uint16 =
            let read (found: JsonElement) =
                uint64Of found |> Option.filter (fun n -> n <= uint64 UInt16.MaxValue) |> Option.map uint16

            this.Read(name, "an unsigned 16-bit integer", 0us, read)

        member this.Float(name: string) : float = this.Read(name, "a number", 0.0, floatOf)

        member this.Float32(name: string) : float32 =
            let read (found: JsonElement) =
                floatOf found |> Option.map float32 |> Option.filter Single.IsFinite

            this.Read(name, "a 32-bit float", 0.0f, read)

        member this.Bool(name: string) : bool = this.Read(name, "a bool", false, boolOf)

        member this.OptionalBool(name: string) : bool option =
            this.Read(name, "a bool", None, (fun found -> boolOf found |> Option.map Some))

        member this.Strings(name: string) : string[] = this.Read(name, "a list of strings", [||], stringsOf)

        member this.Floats(name: string) : float[] = this.Read(name, "a list of numbers", [||], listOf 0.0 floatOf)

        /// A json.RawMessage: the member's text as it was written, null
        /// included; "" when there is no such member.
        member this.Raw(name: string) : string =
            match this.Find name with
            | Some found -> found.GetRawText()
            | None -> ""

        /// The elements of a list member, each still to be read.
        member this.Items(name: string) : JsonElement list =
            let read (found: JsonElement) =
                if found.ValueKind = JsonValueKind.Array then Some(List.ofSeq (found.EnumerateArray())) else None

            this.Read(name, "a list", [], read)

        /// A value as a struct: null reads as an empty one, anything but an
        /// object is noted and reads as an empty one too.
        member private _.AsStruct(at: string, found: JsonElement) : Fields =
            match found.ValueKind with
            | JsonValueKind.Object -> Fields(bad, at, found)
            | JsonValueKind.Null -> Fields(bad, at, JsonElement())
            | _ ->
                bad.Add $"{at}: expected an object, got {kind found}"
                Fields(bad, at, JsonElement())

        member this.Object(name: string) : Fields =
            match this.Find name with
            | Some found -> this.AsStruct(where name, found)
            | None -> Fields(bad, where name, JsonElement())

        /// A member that is a pointer to a struct: None when missing or null.
        member this.OptionalObject(name: string) : Fields option =
            match this.Find name with
            | Some found when found.ValueKind <> JsonValueKind.Null -> Some(this.AsStruct(where name, found))
            | _ -> None

        member this.Objects(name: string) : Fields list =
            this.Items name |> List.mapi (fun i item -> this.AsStruct($"{where name}.{i}", item))

    /// A document's root, or one element of a root list, as a struct.
    let fields (bad: Mismatches) (path: string) (value: JsonElement) : Fields =
        match value.ValueKind with
        | JsonValueKind.Object -> Fields(bad, path, value)
        | JsonValueKind.Null -> Fields(bad, path, JsonElement())
        | _ ->
            bad.Add(if path = "" then $"expected an object, got {kind value}" else $"{path}: expected an object, got {kind value}")
            Fields(bad, path, JsonElement())
