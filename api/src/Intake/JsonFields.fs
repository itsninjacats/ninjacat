namespace NinjaCat.Api.Intake

open System
open System.Text
open System.Text.Json

/// Reads the members of a JSON object into typed values. One set of rules
/// for every intake that reads JSON field by field:
///
///   - a member is found by its exact name, and of two with the same name
///     the last one counts;
///   - null reads the same as a missing member: the field stays empty;
///   - an integer is read from the literal's digits: 1.0 and 1e2 are refused,
///     and a 64-bit id never passes through a float;
///   - a value of the wrong type does not stop the read. It is noted, the
///     rest is still read, and the caller decides what to do with a document
///     that has notes.
module JsonFields =
    /// What did not fit while one document was read, in reading order.
    type Mismatches = ResizeArray<string>

    /// A JSON object whose members are read one by one. `path` says where it
    /// sits in the document, for the notes. A value that is not an object
    /// has no members.
    type Fields(bad: Mismatches, path: string, value: JsonElement) =
        let where (name: string) : string = if path = "" then name else path + "." + name

        /// The member's value; None when there is none or it is null.
        member _.Find(name: string) : JsonElement option = Json.field name value

        /// Every member by name, the null ones included.
        member _.Members: Map<string, JsonElement> = Json.members value

        /// Notes a member whose value does not fit.
        member _.Refuse(name: string, expected: string, got: JsonElement) : unit =
            bad.Add $"{where name}: expected {expected}, got {Json.kind got}"

        /// Notes a member that was read and is still not acceptable.
        member _.Invalid(name: string, problem: string) : unit = bad.Add $"{where name}: {problem}"

        /// Notes a member that has to be there and is not.
        member this.Require(name: string) : unit =
            if (this.Find name).IsNone then
                this.Invalid(name, "missing")

        /// The member read with `read`; None when it is missing or null, and
        /// when its value does not fit, which is also noted.
        member this.TryRead(name: string, expected: string, read: JsonElement -> 'a option) : 'a option =
            match this.Find name with
            | None -> None
            | Some found ->
                match read found with
                | Some result -> Some result
                | None ->
                    this.Refuse(name, expected, found)
                    None

        /// The same, with `zero` in place of None.
        member this.Read(name: string, expected: string, zero: 'a, read: JsonElement -> 'a option) : 'a =
            this.TryRead(name, expected, read) |> Option.defaultValue zero

        member this.String(name: string) : string = this.Read(name, "a string", "", Json.stringOf)

        member this.OptionalString(name: string) : string option = this.TryRead(name, "a string", Json.stringOf)

        /// The digits of a number as written, or "".
        member this.Number(name: string) : string = this.Read(name, "a number", "", Json.numberText)

        member this.Int64(name: string) : int64 = this.Read(name, "an integer", 0L, Json.int64Of)

        member this.OptionalInt64(name: string) : int64 option = this.TryRead(name, "an integer", Json.int64Of)

        member this.OptionalInt32(name: string) : int32 option = this.TryRead(name, "a 32-bit integer", Json.int32Of)

        member this.Int32(name: string) : int32 = this.OptionalInt32 name |> Option.defaultValue 0

        member this.UInt64(name: string) : uint64 = this.Read(name, "an unsigned integer", 0UL, Json.uint64Of)

        member this.UInt32(name: string) : uint32 =
            let read (found: JsonElement) =
                Json.uint64Of found |> Option.filter (fun n -> n <= uint64 UInt32.MaxValue) |> Option.map uint32

            this.Read(name, "an unsigned 32-bit integer", 0u, read)

        member this.UInt16(name: string) : uint16 =
            let read (found: JsonElement) =
                Json.uint64Of found |> Option.filter (fun n -> n <= uint64 UInt16.MaxValue) |> Option.map uint16

            this.Read(name, "an unsigned 16-bit integer", 0us, read)

        member this.Float(name: string) : float = this.Read(name, "a number", 0.0, Json.floatOf)

        member this.Float32(name: string) : float32 =
            let read (found: JsonElement) =
                Json.floatOf found |> Option.map float32 |> Option.filter Single.IsFinite

            this.Read(name, "a 32-bit float", 0.0f, read)

        member this.Bool(name: string) : bool = this.Read(name, "a bool", false, Json.boolOf)

        member this.OptionalBool(name: string) : bool option = this.TryRead(name, "a bool", Json.boolOf)

        member this.Strings(name: string) : string[] = this.Read(name, "a list of strings", [||], Json.stringsOf)

        member this.OptionalStrings(name: string) : string[] option = this.TryRead(name, "a list of strings", Json.stringsOf)

        member this.Floats(name: string) : float[] = this.Read(name, "a list of numbers", [||], Json.floatsOf)

        /// The member's text as it was written, `null` included; "" when
        /// it was not sent.
        member _.Raw(name: string) : string = Json.rawOrEmpty (Json.tryProperty name value)

        /// The elements of a list member, each still to be read.
        member this.Items(name: string) : JsonElement list =
            let read (found: JsonElement) =
                if found.ValueKind = JsonValueKind.Array then Some(List.ofSeq (found.EnumerateArray())) else None

            this.Read(name, "a list", [], read)

        /// A value as an object to read: null reads as an empty one, anything
        /// but an object is noted and reads as an empty one too.
        member private _.AsObject(at: string, found: JsonElement) : Fields =
            match found.ValueKind with
            | JsonValueKind.Object -> Fields(bad, at, found)
            | JsonValueKind.Null -> Fields(bad, at, JsonElement())
            | _ ->
                bad.Add $"{at}: expected an object, got {Json.kind found}"
                Fields(bad, at, JsonElement())

        member this.Object(name: string) : Fields =
            match this.Find name with
            | Some found -> this.AsObject(where name, found)
            | None -> Fields(bad, where name, JsonElement())

        /// None when the member is missing or null.
        member this.OptionalObject(name: string) : Fields option =
            this.Find name |> Option.map (fun found -> this.AsObject(where name, found))

        member this.Objects(name: string) : Fields list =
            this.Items name |> List.mapi (fun i item -> this.AsObject($"{where name}[{i}]", item))

    /// A document's root, or one element of a root list, as an object to
    /// read: null is an empty one, anything else is noted.
    let fields (bad: Mismatches) (path: string) (value: JsonElement) : Fields =
        match value.ValueKind with
        | JsonValueKind.Object -> Fields(bad, path, value)
        | JsonValueKind.Null -> Fields(bad, path, JsonElement())
        | _ ->
            bad.Add(if path = "" then $"expected an object, got {Json.kind value}" else $"{path}: expected an object, got {Json.kind value}")
            Fields(bad, path, JsonElement())
