/// MessagePack, read. Tracers send APM stats, Data Streams and CI events in
/// it. The bytes are read by the MessagePack library; this file only says
/// what a value is to us and how a decoder takes fields out of one.
namespace NinjaCat.Api.Intake

open System
open System.Buffers
open System.IO
open MessagePack

type MsgValue =
    | MsgNil
    | MsgBool of bool
    /// A negative integer, or any integer written in a signed format.
    | MsgInt of int64
    | MsgUInt of uint64
    | MsgFloat32 of float32
    | MsgFloat of float
    | MsgStr of string
    | MsgBin of byte[]
    | MsgArray of MsgValue list
    /// Entries in wire order. Keys are strings.
    | MsgMap of (string * MsgValue) list
    | MsgExt of extType: sbyte * data: byte[]

module Msgpack =
    /// How deep a value may nest. The walk below is recursive and a stack
    /// overflow ends the process, so a body of nothing but nested array
    /// headers has to end in an error.
    let maxDepth = 512

    let private isUnsigned (code: byte) : bool =
        code <= MessagePackCode.MaxFixInt || (code >= MessagePackCode.UInt8 && code <= MessagePackCode.UInt64)

    /// The value at the reader's position. Throws what the library throws
    /// when the bytes are not MessagePack; `decode` is where that is caught.
    let rec private value (reader: byref<MessagePackReader>) (depth: int) : MsgValue =
        if depth > maxDepth then
            raise (MessagePackSerializationException $"nesting deeper than {maxDepth} levels")

        match reader.NextMessagePackType with
        | MessagePackType.Nil ->
            reader.ReadNil() |> ignore
            MsgNil
        | MessagePackType.Boolean -> MsgBool(reader.ReadBoolean())
        | MessagePackType.Integer -> if isUnsigned reader.NextCode then MsgUInt(reader.ReadUInt64()) else MsgInt(reader.ReadInt64())
        | MessagePackType.Float ->
            if reader.NextCode = MessagePackCode.Float32 then MsgFloat32(reader.ReadSingle()) else MsgFloat(reader.ReadDouble())
        | MessagePackType.String -> MsgStr(reader.ReadString())
        | MessagePackType.Binary ->
            let bytes = reader.ReadBytes()
            MsgBin(if bytes.HasValue then bytes.Value.ToArray() else [||])
        | MessagePackType.Array ->
            let count = reader.ReadArrayHeader()
            let items = ResizeArray<MsgValue>()

            for _ in 1..count do
                items.Add(value &reader (depth + 1))

            MsgArray(List.ofSeq items)
        | MessagePackType.Map ->
            let count = reader.ReadMapHeader()
            let entries = ResizeArray<string * MsgValue>()

            for _ in 1..count do
                if reader.NextMessagePackType <> MessagePackType.String then
                    raise (MessagePackSerializationException $"map key at byte {reader.Consumed} is not a string")

                let key = reader.ReadString()
                entries.Add(key, value &reader (depth + 1))

            MsgMap(List.ofSeq entries)
        | MessagePackType.Extension ->
            let extension = reader.ReadExtensionFormat()
            MsgExt(extension.TypeCode, extension.Data.ToArray())
        | _ -> raise (MessagePackSerializationException $"byte 0x{reader.NextCode:x2} at position {reader.Consumed} starts no MessagePack value")

    let truncated = "the data ends before the value does"
    let forged = "a length in the data is larger than any value could be"

    /// The next value of a reader a decoder is walking by hand.
    let read (reader: byref<MessagePackReader>) : MsgValue = value &reader 0

    /// A whole body as one value.
    let decode (body: byte[]) : Result<MsgValue, string> =
        if body.Length = 0 then
            Error "empty body"
        else
            try
                let mutable reader = MessagePackReader(ReadOnlyMemory body)
                Ok(value &reader 0)
            with
            | :? MessagePackSerializationException as e -> Error e.Message
            | :? EndOfStreamException -> Error truncated
            // A header announcing more elements than an int holds.
            | :? OverflowException -> Error forged

    /// What a value is, for a note about one that does not fit.
    let kind (value: MsgValue) : string =
        match value with
        | MsgNil -> "nil"
        | MsgBool _ -> "a bool"
        | MsgInt n -> $"the integer {n}"
        | MsgUInt n -> $"the integer {n}"
        | MsgFloat32 _
        | MsgFloat _ -> "a float"
        | MsgStr _ -> "a string"
        | MsgBin _ -> "bytes"
        | MsgArray _ -> "an array"
        | MsgMap _ -> "a map"
        | MsgExt _ -> "an extension value"

/// The fields of a MessagePack map, read by name, the way a decoder
/// generated for a struct reads them: the last of two equal keys wins, and
/// nil is the same as absent. A value of the wrong type is noted in
/// `problems` with its path and the field keeps its default; the caller
/// looks at `problems` once the whole document has been read.
type MsgFields(problems: ResizeArray<string>, path: string, value: MsgValue) =
    let entries =
        match value with
        | MsgMap entries -> entries
        | MsgNil -> []
        | other ->
            problems.Add(if path = "" then $"expected a map, got {Msgpack.kind other}" else $"{path}: expected a map, got {Msgpack.kind other}")
            []

    let where (name: string) : string = if path = "" then name else path + "/" + name

    let find (name: string) : MsgValue option =
        entries
        |> List.tryFindBack (fun (key, _) -> key = name)
        |> Option.map snd
        |> Option.filter (fun found -> found <> MsgNil)

    let refuse (name: string) (wanted: string) (found: MsgValue) : unit =
        problems.Add $"{where name}: expected {wanted}, got {Msgpack.kind found}"

    /// The field read with `read`; `zero` when it is absent or does not fit.
    let field (name: string) (wanted: string) (zero: 'a) (read: MsgValue -> 'a option) : 'a =
        match find name with
        | None -> zero
        | Some found ->
            match read found with
            | Some result -> result
            | None ->
                refuse name wanted found
                zero

    let unsigned (found: MsgValue) : uint64 option =
        match found with
        | MsgUInt n -> Some n
        | MsgInt n when n >= 0L -> Some(uint64 n)
        | _ -> None

    let signed (found: MsgValue) : int64 option =
        match found with
        | MsgInt n -> Some n
        | MsgUInt n when n <= uint64 Int64.MaxValue -> Some(int64 n)
        | _ -> None

    let items (found: MsgValue) : MsgValue list option =
        match found with
        | MsgArray items -> Some items
        | _ -> None

    member _.String(name: string) : string =
        field name "a string" "" (fun found ->
            match found with
            | MsgStr text -> Some text
            | _ -> None)

    member _.Bool(name: string) : bool =
        field name "a bool" false (fun found ->
            match found with
            | MsgBool flag -> Some flag
            | _ -> None)

    member _.UInt64(name: string) : uint64 = field name "an unsigned integer" 0UL unsigned

    member _.UInt32(name: string) : uint32 =
        field name "an unsigned 32-bit integer" 0u (fun found ->
            unsigned found |> Option.filter (fun n -> n <= uint64 UInt32.MaxValue) |> Option.map uint32)

    member _.Int64(name: string) : int64 = field name "an integer" 0L signed

    member _.Int32(name: string) : int32 =
        field name "a 32-bit integer" 0 (fun found ->
            signed found |> Option.filter (fun n -> n >= int64 Int32.MinValue && n <= int64 Int32.MaxValue) |> Option.map int32)

    member _.Bytes(name: string) : byte[] =
        field name "bytes" [||] (fun found ->
            match found with
            | MsgBin bytes -> Some bytes
            | _ -> None)

    member _.Strings(name: string) : string[] =
        field name "an array of strings" [||] (fun found ->
            match items found with
            | Some list when list |> List.forall (fun item -> item.IsMsgStr) ->
                Some [| for item in list do
                            match item with
                            | MsgStr text -> text
                            | _ -> () |]
            | _ -> None)

    /// An array of maps, each as its fields. A nil element is left out: an
    /// encoder writes one for a nil pointer. Unlike a scalar, a list that
    /// appears under the same key twice is the two lists one after the other.
    member _.Maps(name: string) : MsgFields list =
        [ for key, found in entries do
              if key = name then
                  match found with
                  | MsgNil -> ()
                  | MsgArray list ->
                      for i, item in List.indexed list do
                          if item <> MsgNil then
                              MsgFields(problems, $"{where name}/{i}", item)
                  | other -> refuse name "an array" other ]

    /// The fields whose names are not in `known`, with their values.
    member _.Unknown(known: Set<string>) : Map<string, MsgValue> =
        entries |> List.filter (fun (key, _) -> not (known.Contains key)) |> Map.ofList
