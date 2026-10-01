/// What a profiles row keeps of a pprof attachment, read the way the Go
/// intake's library (github.com/google/pprof/profile) reads one.
///
/// That library has its own small protobuf decoder, and this follows it
/// rather than the generated classes: strings are bytes there (a profile of
/// a Latin-1 codebase is still a profile), a field of the wrong wire type is
/// an error, and so is a second time_nanos, which marks two profiles glued
/// together. The legacy text formats that library also reads are not read
/// here: no profiler sends them to this intake.
module NinjaCat.Api.Intake.Routers.Pprof

open System
open System.Buffers.Binary
open System.Collections.Generic
open System.Text

type Summary =
    { SampleTypes: string[]
      SampleUnits: string[]
      SampleCount: uint64
      TimeNanos: int64
      DurationNanos: int64
      /// "type/unit" of the sampling period.
      PeriodType: string
      Period: int64
      MappingCount: uint32
      LocationCount: uint32
      FunctionCount: uint32 }

/// Raised anywhere in the walk when the bytes are not a profile; `read`
/// turns it into None.
exception private NotAProfile

/// One field of a message: a number (wire types 0, 1, 5) or a run of bytes
/// (wire type 2) at data[Start .. Start + Length).
[<Struct>]
type private Field =
    { Number: uint64
      WireType: int
      Value: uint64
      Start: int
      Length: int }

/// An index into the string table, and what refers to others by id.
type private ValueType = { Type: int64; Unit: int64 }
type private Label = { Key: int64; Str: int64; NumUnit: int64 }
type private Sample = { LocationIds: ResizeArray<uint64>; Values: ResizeArray<uint64>; Labels: ResizeArray<Label> }
type private Mapping = { Id: uint64; Filename: int64; BuildId: int64 }
type private Location = { Id: uint64; FunctionIds: ResizeArray<uint64> }
type private Function = { Id: uint64; Name: int64; SystemName: int64; Filename: int64 }

/// The varint at `position` and the position after it.
let private varint (data: byte[]) (position: int) (stop: int) : uint64 * int =
    let mutable value = 0UL
    let mutable length = 0
    let mutable finished = false

    while not finished do
        if length >= 10 || position + length >= stop then
            raise NotAProfile

        let b = data[position + length]
        value <- value ||| (uint64 (b &&& 0x7Fuy) <<< (7 * length))
        length <- length + 1
        finished <- b < 0x80uy

    value, position + length

/// The fields of the message at data[start .. stop).
let private fields (data: byte[]) (start: int) (stop: int) : ResizeArray<Field> =
    let found = ResizeArray<Field>()
    let mutable position = start

    while position < stop do
        let tag, afterTag = varint data position stop
        let number = tag >>> 3
        let wireType = int (tag &&& 7UL)

        match wireType with
        | 0 ->
            let value, next = varint data afterTag stop
            found.Add { Number = number; WireType = 0; Value = value; Start = 0; Length = 0 }
            position <- next
        | 1 when stop - afterTag >= 8 ->
            let value = BinaryPrimitives.ReadUInt64LittleEndian(ReadOnlySpan(data, afterTag, 8))
            found.Add { Number = number; WireType = 1; Value = value; Start = 0; Length = 0 }
            position <- afterTag + 8
        | 5 when stop - afterTag >= 4 ->
            let value = uint64 (BinaryPrimitives.ReadUInt32LittleEndian(ReadOnlySpan(data, afterTag, 4)))
            found.Add { Number = number; WireType = 5; Value = value; Start = 0; Length = 0 }
            position <- afterTag + 4
        | 2 ->
            let length, afterLength = varint data afterTag stop

            if length > uint64 (stop - afterLength) then
                raise NotAProfile

            found.Add { Number = number; WireType = 2; Value = 0UL; Start = afterLength; Length = int length }
            position <- afterLength + int length
        | _ -> raise NotAProfile

    found

/// A field that must be a single varint.
let private number (field: Field) : uint64 =
    if field.WireType <> 0 then
        raise NotAProfile

    field.Value

/// A repeated number, packed into one field or one field per value.
let private numbers (data: byte[]) (field: Field) (into: ResizeArray<uint64>) : unit =
    if field.WireType = 2 then
        let stop = field.Start + field.Length
        let mutable position = field.Start

        while position < stop do
            let value, next = varint data position stop
            into.Add value
            position <- next
    else
        into.Add(number field)

/// The fields of a nested message.
let private nested (data: byte[]) (field: Field) : ResizeArray<Field> =
    if field.WireType <> 2 then
        raise NotAProfile

    fields data field.Start (field.Start + field.Length)

let private valueType (data: byte[]) (field: Field) : ValueType =
    let mutable typeIndex = 0L
    let mutable unitIndex = 0L

    for f in nested data field do
        match f.Number with
        | 1UL -> typeIndex <- int64 (number f)
        | 2UL -> unitIndex <- int64 (number f)
        | _ -> ()

    { Type = typeIndex; Unit = unitIndex }

let private label (data: byte[]) (field: Field) : Label =
    let mutable key = 0L
    let mutable str = 0L
    let mutable numUnit = 0L

    for f in nested data field do
        match f.Number with
        | 1UL -> key <- int64 (number f)
        | 2UL -> str <- int64 (number f)
        | 3UL -> number f |> ignore
        | 4UL -> numUnit <- int64 (number f)
        | _ -> ()

    { Key = key; Str = str; NumUnit = numUnit }

let private sample (data: byte[]) (field: Field) : Sample =
    let decoded =
        { LocationIds = ResizeArray()
          Values = ResizeArray()
          Labels = ResizeArray() }

    for f in nested data field do
        match f.Number with
        | 1UL -> numbers data f decoded.LocationIds
        | 2UL -> numbers data f decoded.Values
        | 3UL -> decoded.Labels.Add(label data f)
        | _ -> ()

    decoded

let private mapping (data: byte[]) (field: Field) : Mapping =
    let mutable id = 0UL
    let mutable filename = 0L
    let mutable buildId = 0L

    for f in nested data field do
        match f.Number with
        | 1UL -> id <- number f
        | 5UL -> filename <- int64 (number f)
        | 6UL -> buildId <- int64 (number f)
        | n when n >= 2UL && n <= 10UL -> number f |> ignore
        | _ -> ()

    { Id = id; Filename = filename; BuildId = buildId }

let private location (data: byte[]) (field: Field) : Location =
    let mutable id = 0UL
    let functionIds = ResizeArray<uint64>()

    for f in nested data field do
        match f.Number with
        | 1UL -> id <- number f
        | 4UL ->
            // A line: the function it belongs to, then line and column.
            let mutable functionId = 0UL

            for l in nested data f do
                match l.Number with
                | 1UL -> functionId <- number l
                | 2UL
                | 3UL -> number l |> ignore
                | _ -> ()

            functionIds.Add functionId
        | 2UL
        | 3UL
        | 5UL -> number f |> ignore
        | _ -> ()

    { Id = id; FunctionIds = functionIds }

let private func (data: byte[]) (field: Field) : Function =
    let mutable id = 0UL
    let mutable name = 0L
    let mutable systemName = 0L
    let mutable filename = 0L

    for f in nested data field do
        match f.Number with
        | 1UL -> id <- number f
        | 2UL -> name <- int64 (number f)
        | 3UL -> systemName <- int64 (number f)
        | 4UL -> filename <- int64 (number f)
        | 5UL -> number f |> ignore
        | _ -> ()

    { Id = id; Name = name; SystemName = systemName; Filename = filename }

/// The ids as a set; None unless they are all different and none is 0,
/// which pprof reserves.
let private distinctAndNonZero (ids: uint64 seq) (count: int) : HashSet<uint64> option =
    let seen = HashSet<uint64> ids
    if seen.Count = count && not (seen.Contains 0UL) then Some seen else None

let private summarise (data: byte[]) : Summary =
    let sampleTypes = ResizeArray<ValueType>()
    let samples = ResizeArray<Sample>()
    let mappings = ResizeArray<Mapping>()
    let locations = ResizeArray<Location>()
    let functions = ResizeArray<Function>()
    let strings = ResizeArray<Field>()
    let comments = ResizeArray<uint64>()
    // Indexes into the string table that only have to resolve.
    let mutable dropFrames = 0L
    let mutable keepFrames = 0L
    let mutable defaultSampleType = 0L
    let mutable docUrl = 0L
    let mutable timeNanos = 0L
    let mutable durationNanos = 0L
    let mutable period = 0L
    let mutable periodType = { Type = 0L; Unit = 0L }

    for f in fields data 0 data.Length do
        match f.Number with
        | 1UL -> sampleTypes.Add(valueType data f)
        | 2UL -> samples.Add(sample data f)
        | 3UL -> mappings.Add(mapping data f)
        | 4UL -> locations.Add(location data f)
        | 5UL -> functions.Add(func data f)
        | 6UL ->
            if f.WireType <> 2 || (strings.Count = 0 && f.Length > 0) then
                // The first string must be "".
                raise NotAProfile

            strings.Add f
        | 7UL -> dropFrames <- int64 (number f)
        | 8UL -> keepFrames <- int64 (number f)
        | 9UL ->
            if timeNanos <> 0L then
                // Two profiles glued together.
                raise NotAProfile

            timeNanos <- int64 (number f)
        | 10UL -> durationNanos <- int64 (number f)
        | 11UL -> periodType <- valueType data f
        | 12UL -> period <- int64 (number f)
        | 13UL -> numbers data f comments
        | 14UL -> defaultSampleType <- int64 (number f)
        | 15UL -> docUrl <- int64 (number f)
        | _ -> ()

    let resolves (index: int64) = index >= 0L && index < int64 strings.Count
    let valueTypeResolves (t: ValueType) = resolves t.Type && resolves t.Unit

    // A label's unit is looked up only when it has no string value.
    let labelResolves (l: Label) =
        resolves l.Key
        && (if l.Str <> 0L then resolves l.Str
            elif l.NumUnit <> 0L then resolves l.NumUnit
            else true)

    // Even an empty profile looks up index 0, so the table cannot be empty.
    let stringsResolve =
        resolves dropFrames
        && resolves keepFrames
        && resolves defaultSampleType
        && resolves docUrl
        && comments |> Seq.forall (fun c -> resolves (int64 c))
        && valueTypeResolves periodType
        && Seq.forall valueTypeResolves sampleTypes
        && mappings |> Seq.forall (fun m -> resolves m.Filename && resolves m.BuildId)
        && functions |> Seq.forall (fun f -> resolves f.Name && resolves f.SystemName && resolves f.Filename)
        && samples |> Seq.forall (fun s -> Seq.forall labelResolves s.Labels)

    let mappingIds = distinctAndNonZero (mappings |> Seq.map _.Id) mappings.Count
    let functionIds = distinctAndNonZero (functions |> Seq.map _.Id) functions.Count
    let locationIds = distinctAndNonZero (locations |> Seq.map _.Id) locations.Count

    match stringsResolve, mappingIds, functionIds, locationIds with
    | true, Some _, Some knownFunctions, Some knownLocations ->
        let samplesAreSound =
            (sampleTypes.Count > 0 || samples.Count = 0)
            && samples
               |> Seq.forall (fun s -> s.Values.Count = sampleTypes.Count && Seq.forall knownLocations.Contains s.LocationIds)

        // A line must name a function, and one the profile has.
        let linesAreSound =
            locations |> Seq.forall (fun l -> Seq.forall knownFunctions.Contains l.FunctionIds)

        if not (samplesAreSound && linesAreSound) then
            raise NotAProfile

        let text (index: int64) =
            let s = strings[int index]
            Encoding.UTF8.GetString(data, s.Start, s.Length)

        { SampleTypes = sampleTypes |> Seq.map (fun t -> text t.Type) |> Array.ofSeq
          SampleUnits = sampleTypes |> Seq.map (fun t -> text t.Unit) |> Array.ofSeq
          SampleCount = uint64 samples.Count
          TimeNanos = timeNanos
          DurationNanos = durationNanos
          // Without a period type both names are "", so this reads "/".
          PeriodType = text periodType.Type + "/" + text periodType.Unit
          Period = period
          MappingCount = uint32 mappings.Count
          LocationCount = uint32 locations.Count
          FunctionCount = uint32 functions.Count }
    | _ -> raise NotAProfile

/// The envelope of an uncompressed pprof profile; None when the bytes are
/// not one.
let read (data: byte[]) : Summary option =
    if data.Length = 0 then
        None
    else
        try
            Some(summarise data)
        with NotAProfile ->
            None
