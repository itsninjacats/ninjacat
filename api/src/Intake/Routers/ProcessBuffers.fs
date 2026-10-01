/// The packed buffers of a connections payload: tag sets and DNS names.
///
/// Ported from github.com/DataDog/agent-payload v5.0.207, process/tags.go,
/// tags_v2.go, dns_v1.go and dns_v2.go: the readers only, and only what the
/// handler uses. A buffer the Go reader would run off the end of gives no
/// tags, or an Error for DNS names, instead of failing the request.
module NinjaCat.Api.Intake.Routers.ProcessBuffers

open System
open System.Buffers.Binary
open System.Text

let private uint16At (buffer: byte[]) (at: int) : int =
    int (BinaryPrimitives.ReadUInt16LittleEndian(ReadOnlySpan(buffer, at, 2)))

let private uint32At (buffer: byte[]) (at: int) : int64 =
    int64 (BinaryPrimitives.ReadUInt32LittleEndian(ReadOnlySpan(buffer, at, 4)))

/// `encodedTags` and `encodedConnectionsTags`: many tag sets in one buffer,
/// each found by the index a connection, host or resource carries. The first
/// byte is the format's version.
module TagBuffer =
    /// Version 1: at the index, a count, then each tag behind its length.
    /// All numbers are 2 bytes, little-endian.
    let private readV1 (buffer: byte[]) (tagIndex: int) : string list =
        if tagIndex >= buffer.Length || buffer.Length - tagIndex < 2 then
            []
        else
            let count = uint16At buffer tagIndex
            let tags = ResizeArray<string>()
            let mutable at = tagIndex + 2

            for _ in 1..count do
                if buffer.Length - at >= 2 then
                    let length = uint16At buffer at
                    at <- at + 2

                    // A tag that runs past the end is dropped, and the reader
                    // goes on from where it stands, as the Go one does.
                    if at + length <= buffer.Length then
                        tags.Add(Encoding.UTF8.GetString(buffer, at, length))
                        at <- at + length

            List.ofSeq tags

    /// Versions 2 and 3: every distinct tag is stored once, behind a 2-byte
    /// length. Bytes 1-4 say where the footer starts; a tag set is a position
    /// in the footer: a 2-byte count, then the 4-byte position of each tag.
    let private readV2 (buffer: byte[]) (tagIndex: int) : string list =
        // The tag whose position is written at `at` in the footer.
        let tagAt (at: int) : string option =
            if buffer.Length - at < 4 then
                None
            else
                let position = uint32At buffer at

                if position >= int64 buffer.Length || int64 buffer.Length - position < 2L then
                    None
                else
                    let length = uint16At buffer (int position)
                    let first = int position + 2

                    if first + length > buffer.Length then
                        None
                    else
                        Some(Encoding.UTF8.GetString(buffer, first, length))

        if buffer.Length < 5 then
            []
        else
            let start = uint32At buffer 1 + int64 tagIndex

            if start >= int64 buffer.Length || int64 buffer.Length - start < 2L then
                []
            else
                let count = uint16At buffer (int start)
                let tags = ResizeArray<string>()
                let mutable at = int start + 2
                let mutable stuck = false

                // A tag that cannot be read ends the set: the Go reader never
                // moves past it.
                while not stuck && tags.Count < count do
                    match tagAt at with
                    | Some tag ->
                        tags.Add tag
                        at <- at + 4
                    | None -> stuck <- true

                List.ofSeq tags

    /// The tags of one set. Nothing for an empty buffer, a negative index
    /// (the encoders' "no tags") or a version this reader does not know.
    let tags (buffer: byte[]) (tagIndex: int) : string list =
        if buffer.Length = 0 || tagIndex < 0 then
            []
        else
            match buffer[0] with
            // Byte 0 of a version 1 buffer is the version, never a tag set:
            // an index of 0 there is one that was not sent.
            | 1uy -> if tagIndex = 0 then [] else readV1 buffer tagIndex
            | 2uy
            | 3uy -> readV2 buffer tagIndex
            | _ -> []

/// The DNS names of a connections payload. Two encodings exist: version 1
/// keeps the names inside `encodedDNS`, version 2 in a buffer of their own,
/// `encodedDomainDatabase`.
module DnsBuffer =
    /// A varint as Go's binary.Uvarint reads one: the value and the bytes it
    /// took. None when the buffer ends inside the number or it overflows 64
    /// bits.
    let private uvarint (buffer: byte[]) (at: int) : (uint64 * int) option =
        let mutable value = 0UL
        let mutable shift = 0
        let mutable i = 0
        let mutable result = None
        let mutable reading = true

        while reading && at + i < buffer.Length do
            let b = buffer[at + i]

            if i = 10 || (i = 9 && b > 1uy && b < 0x80uy) then
                reading <- false
            elif b < 0x80uy then
                result <- Some(value ||| (uint64 b <<< shift), i + 1)
                reading <- false
            else
                value <- value ||| (uint64 (b &&& 0x7Fuy) <<< shift)
                shift <- shift + 7
                i <- i + 1

        result

    /// `count` names from `at`, each behind a varint length, stopping at `stop`.
    let private readNames (buffer: byte[]) (at: int) (stop: int) (count: uint64) : Result<string[], string> =
        let names = ResizeArray<string>()
        let mutable position = at
        let mutable error = None

        while error.IsNone && position < stop && uint64 names.Count < count do
            match uvarint buffer position with
            | None -> error <- Some $"unreadable name length at byte {position}"
            | Some(length, read) ->
                let first = position + read

                if length > uint64 (stop - first) then
                    error <- Some $"name at byte {position} runs past the end of the buffer"
                else
                    names.Add(Encoding.UTF8.GetString(buffer, first, int length))
                    position <- first + int length

        match error with
        | Some message -> Error message
        | None -> Ok(names.ToArray())

    /// Version 1: a 3-byte preamble, then three varints, of which the second
    /// is the length of the name block. The names are the last thing in the
    /// buffer.
    let private namesV1 (buffer: byte[]) : Result<string[], string> =
        if buffer.Length < 3 then
            Error "encodedDNS is shorter than its preamble"
        else
            let positionBlockLength = uvarint buffer 3

            let nameBlockLength =
                match positionBlockLength with
                | Some(_, read) -> uvarint buffer (3 + read)
                | None -> None

            match nameBlockLength with
            // A buffer that ends inside its header has no name block.
            | None -> Ok [||]
            | Some(length, _) when length > uint64 buffer.Length -> Error "encodedDNS: the name block is longer than the buffer"
            | Some(length, _) -> readNames buffer (buffer.Length - int length) buffer.Length UInt64.MaxValue

    /// Version 2: the number of names, a varint that is never used, then the
    /// names.
    let private namesV2 (buffer: byte[]) : Result<string[], string> =
        match uvarint buffer 0 with
        | None -> Ok [||]
        | Some(count, read) ->
            match uvarint buffer read with
            | None when read < buffer.Length -> Error "encodedDomainDatabase: unreadable header"
            | None -> Ok [||]
            | Some(_, readForMiddle) -> readNames buffer (read + readForMiddle) buffer.Length count

    /// Every DNS name of the payload. `encodedDNS` wins when both are there;
    /// one in a version other than 1 gives no names, as does a payload with
    /// neither buffer. Error means a buffer that could not be read through.
    let names (encodedDns: byte[]) (domainDatabase: byte[]) : Result<string[], string> =
        if encodedDns.Length > 0 then
            if encodedDns[0] = 1uy then namesV1 encodedDns else Ok [||]
        elif domainDatabase.Length > 0 then
            namesV2 domainDatabase
        else
            Ok [||]
