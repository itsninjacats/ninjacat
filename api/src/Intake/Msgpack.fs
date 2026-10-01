/// MessagePack, read. Tracers send traces and CI events in it.
///
/// Two ways in: `Msgpack.decode` for a whole body as a tree, and
/// `MsgpackReader` for decoders that walk a known layout field by field.
namespace NinjaCat.Api.Intake

open System
open System.Buffers.Binary
open System.Text

exception MsgpackError of message: string

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
    /// Entries in wire order. Keys are strings (a binary key is read as one).
    | MsgMap of (string * MsgValue) list
    | MsgExt of extType: sbyte * data: byte[]

type MsgType =
    | NilType
    | BoolType
    | IntType
    | UIntType
    | FloatType
    | StrType
    | BinType
    | ArrayType
    | MapType
    | ExtType

type MsgpackReader(data: byte[]) =
    let mutable position = 0

    let fail (message: string) = raise (MsgpackError message)

    let need (count: int) =
        if count < 0 || position + count > data.Length then
            fail $"unexpected end of data at byte {position} (need {count} more)"

    let take (count: int) : ReadOnlySpan<byte> =
        need count
        let span = ReadOnlySpan(data, position, count)
        position <- position + count
        span

    let u8 () = (take 1)[0]
    let u16 () = BinaryPrimitives.ReadUInt16BigEndian(take 2)
    let u32 () = BinaryPrimitives.ReadUInt32BigEndian(take 4)
    let u64 () = BinaryPrimitives.ReadUInt64BigEndian(take 8)

    /// A length that cannot be real is refused before anything is allocated:
    /// every element takes at least one byte.
    let length (n: uint32) : int =
        if int64 n > int64 (data.Length - position) then
            fail $"length {n} at byte {position} exceeds the {data.Length - position} bytes left"

        int n

    member _.Position = position
    member _.AtEnd = position >= data.Length

    member _.PeekType() : MsgType =
        need 1
        let b = data[position]

        if b <= 0x7fuy then UIntType
        elif b <= 0x8fuy then MapType
        elif b <= 0x9fuy then ArrayType
        elif b <= 0xbfuy then StrType
        elif b >= 0xe0uy then IntType
        else
            match b with
            | 0xc0uy -> NilType
            | 0xc2uy
            | 0xc3uy -> BoolType
            | 0xc4uy
            | 0xc5uy
            | 0xc6uy -> BinType
            | 0xc7uy
            | 0xc8uy
            | 0xc9uy
            | 0xd4uy
            | 0xd5uy
            | 0xd6uy
            | 0xd7uy
            | 0xd8uy -> ExtType
            | 0xcauy
            | 0xcbuy -> FloatType
            | 0xccuy
            | 0xcduy
            | 0xceuy
            | 0xcfuy -> UIntType
            | 0xd0uy
            | 0xd1uy
            | 0xd2uy
            | 0xd3uy -> IntType
            | 0xd9uy
            | 0xdauy
            | 0xdbuy -> StrType
            | 0xdcuy
            | 0xdduy -> ArrayType
            | 0xdeuy
            | 0xdfuy -> MapType
            | other -> fail $"invalid type byte 0x{other:x2} at byte {position}"

    /// Consumes a nil if one is next; says whether it did.
    member r.TryReadNil() : bool =
        if position < data.Length && data[position] = 0xc0uy then
            position <- position + 1
            true
        else
            false

    member _.ReadBool() : bool =
        match u8 () with
        | 0xc2uy -> false
        | 0xc3uy -> true
        | other -> fail $"expected a bool, got 0x{other:x2} at byte {position - 1}"

    /// Any integer format, as int64. An unsigned value past int64 is an error.
    member _.ReadInt64() : int64 =
        let b = u8 ()

        if b <= 0x7fuy then int64 b
        elif b >= 0xe0uy then int64 (sbyte b)
        else
            match b with
            | 0xccuy -> int64 (u8 ())
            | 0xcduy -> int64 (u16 ())
            | 0xceuy -> int64 (u32 ())
            | 0xcfuy ->
                let v = u64 ()
                if v > uint64 Int64.MaxValue then fail $"integer {v} overflows int64"
                int64 v
            | 0xd0uy -> int64 (sbyte (u8 ()))
            | 0xd1uy -> int64 (int16 (u16 ()))
            | 0xd2uy -> int64 (int32 (u32 ()))
            | 0xd3uy -> int64 (u64 ())
            | other -> fail $"expected an integer, got 0x{other:x2} at byte {position - 1}"

    /// Any integer format, as uint64. A negative value is an error.
    member r.ReadUInt64() : uint64 =
        need 1

        if data[position] = 0xcfuy then
            position <- position + 1
            u64 ()
        else
            let v = r.ReadInt64()
            if v < 0L then fail $"integer {v} is negative, expected unsigned"
            uint64 v

    /// A float32, a float64, or an integer, as float.
    member r.ReadFloat() : float =
        need 1

        match data[position] with
        | 0xcauy ->
            position <- position + 1
            float (BinaryPrimitives.ReadSingleBigEndian(take 4))
        | 0xcbuy ->
            position <- position + 1
            BinaryPrimitives.ReadDoubleBigEndian(take 8)
        | 0xcfuy -> float (r.ReadUInt64())
        | _ -> float (r.ReadInt64())

    /// The raw bytes of a str or bin value.
    member _.ReadBytes() : byte[] =
        let b = u8 ()

        let count =
            if b >= 0xa0uy && b <= 0xbfuy then int (b &&& 0x1fuy)
            else
                match b with
                | 0xd9uy
                | 0xc4uy -> int (u8 ())
                | 0xdauy
                | 0xc5uy -> int (u16 ())
                | 0xdbuy
                | 0xc6uy -> length (u32 ())
                | other -> fail $"expected a string or bytes, got 0x{other:x2} at byte {position - 1}"

        (take count).ToArray()

    /// A str (or bin) value as text.
    member r.ReadString() : string = Encoding.UTF8.GetString(r.ReadBytes())

    member _.ReadArrayHeader() : int =
        let b = u8 ()

        if b >= 0x90uy && b <= 0x9fuy then int (b &&& 0x0fuy)
        else
            match b with
            | 0xdcuy -> int (u16 ())
            | 0xdduy -> length (u32 ())
            | other -> fail $"expected an array, got 0x{other:x2} at byte {position - 1}"

    member _.ReadMapHeader() : int =
        let b = u8 ()

        if b >= 0x80uy && b <= 0x8fuy then int (b &&& 0x0fuy)
        else
            match b with
            | 0xdeuy -> int (u16 ())
            | 0xdfuy -> length (u32 ())
            | other -> fail $"expected a map, got 0x{other:x2} at byte {position - 1}"

    member private r.ReadExt() : MsgValue =
        let b = u8 ()

        let count =
            match b with
            | 0xd4uy -> 1
            | 0xd5uy -> 2
            | 0xd6uy -> 4
            | 0xd7uy -> 8
            | 0xd8uy -> 16
            | 0xc7uy -> int (u8 ())
            | 0xc8uy -> int (u16 ())
            | 0xc9uy -> length (u32 ())
            | other -> fail $"expected an extension, got 0x{other:x2} at byte {position - 1}"

        let extType = sbyte (u8 ())
        MsgExt(extType, (take count).ToArray())

    /// The next value, whatever it is, as a tree.
    member r.ReadValue() : MsgValue =
        match r.PeekType() with
        | NilType ->
            position <- position + 1
            MsgNil
        | BoolType -> MsgBool(r.ReadBool())
        | IntType -> MsgInt(r.ReadInt64())
        | UIntType -> MsgUInt(r.ReadUInt64())
        | FloatType ->
            if data[position] = 0xcauy then
                position <- position + 1
                MsgFloat32(BinaryPrimitives.ReadSingleBigEndian(take 4))
            else
                MsgFloat(r.ReadFloat())
        | StrType -> MsgStr(r.ReadString())
        | BinType -> MsgBin(r.ReadBytes())
        | ArrayType ->
            let count = r.ReadArrayHeader()
            MsgArray [ for _ in 1..count -> r.ReadValue() ]
        | MapType ->
            let count = r.ReadMapHeader()

            MsgMap
                [ for _ in 1..count ->
                      let key = r.ReadString()
                      key, r.ReadValue() ]
        | ExtType -> r.ReadExt()

    /// Skips the next value.
    member r.Skip() : unit = r.ReadValue() |> ignore

module Msgpack =
    /// A whole body as one value.
    let decode (body: byte[]) : Result<MsgValue, string> =
        if body.Length = 0 then
            Error "empty body"
        else
            try
                Ok(MsgpackReader(body).ReadValue())
            with
            | MsgpackError message -> Error message
            | :? DecoderFallbackException as e -> Error e.Message
