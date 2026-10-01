/// tinylib/msgp's reading rules, on top of MsgpackReader.
///
/// APM stats and Data Streams payloads are written by msgp, and the Go intake
/// read them with it. msgp is stricter than MsgpackReader — a bin is not a
/// str, a negative integer is not a uint, a count has a ceiling — and what it
/// refuses decides whether a payload becomes rows or goes to raw_payloads. Its
/// error texts are kept too: they are the note stored with the raw payload.
module NinjaCat.Api.Intake.Routers.TraceMsgp

open System
open System.Collections.Generic
open NinjaCat.Api.Intake

type MsgpError =
    /// Out of data. msgp never adds a context to it.
    | Short of text: string
    /// A wrong type or an overflow, and the path to it, outermost first.
    | AtPath of text: string * path: string
    /// A negative integer where an unsigned one belongs. msgp keeps only the
    /// outermost context of this one.
    | BelowZero of value: int64 * context: string
    /// Anything else. Every context is appended as text.
    | Other of text: string

exception MsgpFailure of MsgpError

module MsgpError =
    let text (error: MsgpError) : string =
        match error with
        | Short text -> text
        | AtPath(text, "") -> text
        | AtPath(text, path) -> $"{text} at {path}"
        | BelowZero(value, "") -> $"msgp: attempted to cast int {value} to unsigned"
        | BelowZero(value, context) -> $"msgp: attempted to cast int {value} to unsigned at {context}"
        | Other text -> text

    /// msgp.WrapError: says where in the payload the error happened.
    let wrap (context: string) (error: MsgpError) : MsgpError =
        match error with
        | Short _ -> error
        | AtPath(text, "") -> AtPath(text, context)
        | AtPath(text, path) -> AtPath(text, context + "/" + path)
        | BelowZero(value, _) -> BelowZero(value, context)
        | Other text -> Other $"{text} at {context}"

/// msgp has two readers and they word running out of data differently: the
/// byte-slice functions generated decoders use, and the stream Reader.
type MsgpSource =
    | Slice
    | Stream

/// The depth msgp follows nested values to before it gives up.
let private recursionLimit = 100_000

/// How deep ReadAny follows a value. msgp allows 100 000 levels; here every
/// level is a stack frame and the stack does not grow.
let private anyDepthLimit = 512

let private limitExceeded = Other "msgp: configured reader limit exceeded"
let private recursionReached = Other "msgp: recursion limit reached"

type MsgpReader(data: byte[], source: MsgpSource) =
    let reader = MsgpackReader data

    let fail (error: MsgpError) : 'a = raise (MsgpFailure error)

    let short (insideValue: bool) : MsgpError =
        match source with
        | Slice -> Short "msgp: too few bytes left to read object"
        | Stream -> Short(if insideValue then "unexpected EOF" else "EOF")

    /// The first byte of the next value.
    let lead () : byte =
        if reader.AtEnd then
            fail (short false)

        data[reader.Position]

    /// msgp's name for the type a first byte announces; None for 0xc1, the
    /// one byte MessagePack leaves unused.
    let typeName (b: byte) : string option =
        if b <= 0x7fuy || b >= 0xe0uy then Some "int"
        elif b <= 0x8fuy then Some "map"
        elif b <= 0x9fuy then Some "array"
        elif b <= 0xbfuy then Some "str"
        else
            match b with
            | 0xc0uy -> Some "nil"
            | 0xc1uy -> None
            | 0xc2uy
            | 0xc3uy -> Some "bool"
            | 0xc4uy
            | 0xc5uy
            | 0xc6uy -> Some "bin"
            | 0xcauy -> Some "float32"
            | 0xcbuy -> Some "float64"
            | 0xccuy
            | 0xcduy
            | 0xceuy
            | 0xcfuy -> Some "uint"
            | 0xd0uy
            | 0xd1uy
            | 0xd2uy
            | 0xd3uy -> Some "int"
            | 0xd9uy
            | 0xdauy
            | 0xdbuy -> Some "str"
            | 0xdcuy
            | 0xdduy -> Some "array"
            | 0xdeuy
            | 0xdfuy -> Some "map"
            | _ -> Some "ext"

    let wrongType (wanted: string) (b: byte) : MsgpError =
        match typeName b with
        | Some found -> AtPath($"msgp: attempted to decode type \"{found}\" with method for \"{wanted}\"", "")
        | None -> Other $"msgp: unrecognized type prefix 0x{b:x}"

    /// The byte-slice string reader reports even the unused byte as a wrong
    /// type; every other read calls it an unrecognized prefix.
    let notAString (b: byte) : MsgpError =
        match source, typeName b with
        | Slice, None -> AtPath("msgp: attempted to decode type \"<invalid>\" with method for \"str\"", "")
        | _ -> wrongType "str" b

    let isStr (b: byte) = (b >= 0xa0uy && b <= 0xbfuy) || (b >= 0xd9uy && b <= 0xdbuy)
    let isBin (b: byte) = b >= 0xc4uy && b <= 0xc6uy
    let isMap (b: byte) = (b >= 0x80uy && b <= 0x8fuy) || b = 0xdeuy || b = 0xdfuy
    let isArray (b: byte) = (b >= 0x90uy && b <= 0x9fuy) || b = 0xdcuy || b = 0xdduy

    /// Runs a read whose type is already checked: all that can still go wrong
    /// is the data ending early.
    let read (run: unit -> 'a) : 'a =
        try
            run ()
        with MsgpackError _ ->
            fail (short true)

    /// True when the next value is nil.
    member _.IsNil: bool = not reader.AtEnd && data[reader.Position] = 0xc0uy

    member _.ReadNil() : unit =
        let b = lead ()

        if b <> 0xc0uy then
            fail (wrongType "nil" b)

        reader.TryReadNil() |> ignore

    member _.ReadMapHeader() : int =
        let b = lead ()

        if not (isMap b) then
            fail (wrongType "map" b)

        read reader.ReadMapHeader

    member _.ReadArrayHeader() : int =
        let b = lead ()

        if not (isArray b) then
            fail (wrongType "array" b)

        read reader.ReadArrayHeader

    member _.ReadString() : string =
        let b = lead ()

        if not (isStr b) then
            fail (notAString b)

        read reader.ReadString

    /// A map key: msgp's generated decoders take a str or a bin.
    member _.ReadMapKey() : string =
        let b = lead ()

        if not (isStr b || isBin b) then
            fail (notAString b)

        read reader.ReadString

    member _.ReadBin() : byte[] =
        // The stream reader peeks two bytes before it looks at the type.
        if source = Stream && data.Length - reader.Position < 2 then
            fail (Short "EOF")

        let b = lead ()

        if not (isBin b) then
            fail (wrongType "bin" b)

        read reader.ReadBytes

    member _.ReadBool() : bool =
        let b = lead ()

        if b <> 0xc2uy && b <> 0xc3uy then
            fail (wrongType "bool" b)

        read reader.ReadBool

    /// Any integer format that fits an int64.
    member _.ReadInt64() : int64 =
        let b = lead ()

        if b = 0xcfuy then
            let value = read reader.ReadUInt64

            if value > uint64 Int64.MaxValue then
                fail (AtPath($"msgp: {value} overflows uint64", ""))

            int64 value
        elif b <= 0x7fuy || b >= 0xe0uy || (b >= 0xccuy && b <= 0xd3uy) then
            read reader.ReadInt64
        else
            fail (wrongType "int" b)

    member r.ReadInt32() : int32 =
        let value = r.ReadInt64()

        if value > int64 Int32.MaxValue || value < int64 Int32.MinValue then
            fail (AtPath($"msgp: {value} overflows int32", ""))

        int32 value

    /// Any integer format, signed ones included, as long as the value is not
    /// negative.
    member _.ReadUInt64() : uint64 =
        let b = lead ()

        if b = 0xcfuy then
            read reader.ReadUInt64
        elif b <= 0x7fuy || b >= 0xe0uy || (b >= 0xccuy && b <= 0xd3uy) then
            let value = read reader.ReadInt64

            if value < 0L then
                fail (BelowZero(value, ""))

            uint64 value
        else
            fail (wrongType "uint" b)

    member r.ReadUInt32() : uint32 =
        let value = r.ReadUInt64()

        if value > uint64 UInt32.MaxValue then
            fail (AtPath($"msgp: {value} overflows uint32", ""))

        uint32 value

    /// Skips the next value, without recursion: a value can nest as deep as
    /// its sender likes.
    member r.Skip() : unit =
        // How many values are still to skip at each open level.
        let pending = Stack<int64>()
        pending.Push 1L

        while pending.Count > 0 do
            let left = pending.Pop()

            if left > 0L then
                pending.Push(left - 1L)

                if pending.Count > recursionLimit then
                    fail recursionReached

                let b = lead ()

                if isMap b then
                    pending.Push(2L * int64 (r.ReadMapHeader()))
                elif isArray b then
                    pending.Push(int64 (r.ReadArrayHeader()))
                elif b = 0xc1uy then
                    fail (wrongType "" b)
                else
                    read reader.Skip

    /// The next value as a tree, the way msgp's ReadIntf sees it: a map's
    /// keys must be strings, and no count may exceed `maxElements`.
    member r.ReadAny(maxElements: int) : MsgValue =
        let rec value (depth: int) : MsgValue =
            let b = lead ()

            if isMap b then
                let count = r.ReadMapHeader()

                if depth >= anyDepthLimit then fail recursionReached
                if count > maxElements then fail limitExceeded

                MsgMap
                    [ for _ in 1..count ->
                          let key = r.ReadString()
                          key, value (depth + 1) ]
            elif isArray b then
                let count = r.ReadArrayHeader()

                if depth >= anyDepthLimit then fail recursionReached
                if count > maxElements then fail limitExceeded

                MsgArray [ for _ in 1..count -> value (depth + 1) ]
            elif b = 0xc1uy then
                fail (wrongType "" b)
            elif isBin b then
                MsgBin(r.ReadBin())
            else
                read reader.ReadValue

        value 0

module Msgp =
    /// Runs a read; an error it raises is reported at `context`.
    let at (context: string) (run: unit -> 'a) : 'a =
        try
            run ()
        with MsgpFailure error ->
            raise (MsgpFailure(MsgpError.wrap context error))

    /// The same, for element `index` of the array `context`.
    let atIndex (context: string) (index: int) (run: unit -> 'a) : 'a =
        try
            run ()
        with MsgpFailure error ->
            raise (MsgpFailure(MsgpError.wrap $"{context}/{index}" error))
