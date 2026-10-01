/// What a profiles row keeps of a pprof attachment.
///
/// The bytes are read by the classes generated from pprof's own profile.proto.
/// What follows the parse are the checks github.com/google/pprof/profile runs
/// on a profile before it trusts one: the string table, the ids, and that
/// samples and lines point at things the profile has.
module NinjaCat.Api.Intake.Routers.Pprof

open System.Collections.Generic
open Google.Protobuf
open Perftools.Profiles

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

/// The ids as a set; None unless they are all different and none is 0,
/// which pprof reserves.
let private distinctAndNonZero (ids: uint64 seq) (count: int) : HashSet<uint64> option =
    let seen = HashSet<uint64> ids
    if seen.Count = count && not (seen.Contains 0UL) then Some seen else None

let private summarise (profile: Profile) : Summary option =
    let strings = profile.StringTable
    let resolves (index: int64) = index >= 0L && index < int64 strings.Count

    // An absent value type is the one whose two names are both "".
    let periodType = if isNull profile.PeriodType then ValueType() else profile.PeriodType
    let valueTypeResolves (t: ValueType) = resolves t.Type && resolves t.Unit

    // A label's unit is looked up only when it has no string value.
    let labelResolves (l: Label) =
        resolves l.Key
        && (if l.Str <> 0L then resolves l.Str
            elif l.NumUnit <> 0L then resolves l.NumUnit
            else true)

    // Even an empty profile looks up index 0, so the table cannot be empty,
    // and its first string must be "".
    let stringsResolve =
        strings.Count > 0
        && strings[0] = ""
        && resolves profile.DropFrames
        && resolves profile.KeepFrames
        && resolves profile.DefaultSampleType
        && resolves profile.DocUrl
        && Seq.forall resolves profile.Comment
        && valueTypeResolves periodType
        && Seq.forall valueTypeResolves profile.SampleType
        && profile.Mapping |> Seq.forall (fun m -> resolves m.Filename && resolves m.BuildId)
        && profile.Function |> Seq.forall (fun f -> resolves f.Name && resolves f.SystemName && resolves f.Filename)
        && profile.Sample |> Seq.forall (fun s -> Seq.forall labelResolves s.Label)

    let mappingIds = distinctAndNonZero (profile.Mapping |> Seq.map _.Id) profile.Mapping.Count
    let functionIds = distinctAndNonZero (profile.Function |> Seq.map _.Id) profile.Function.Count
    let locationIds = distinctAndNonZero (profile.Location |> Seq.map _.Id) profile.Location.Count

    match stringsResolve, mappingIds, functionIds, locationIds with
    | true, Some _, Some knownFunctions, Some knownLocations ->
        let samplesAreSound =
            (profile.SampleType.Count > 0 || profile.Sample.Count = 0)
            && profile.Sample
               |> Seq.forall (fun s -> s.Value.Count = profile.SampleType.Count && Seq.forall knownLocations.Contains s.LocationId)

        // A line must name a function, and one the profile has.
        let linesAreSound =
            profile.Location
            |> Seq.forall (fun l -> l.Line |> Seq.forall (fun line -> knownFunctions.Contains line.FunctionId))

        if samplesAreSound && linesAreSound then
            let text (index: int64) = strings[int index]

            Some
                { SampleTypes = profile.SampleType |> Seq.map (fun t -> text t.Type) |> Array.ofSeq
                  SampleUnits = profile.SampleType |> Seq.map (fun t -> text t.Unit) |> Array.ofSeq
                  SampleCount = uint64 profile.Sample.Count
                  TimeNanos = profile.TimeNanos
                  DurationNanos = profile.DurationNanos
                  // Without a period type both names are "", so this reads "/".
                  PeriodType = text periodType.Type + "/" + text periodType.Unit
                  Period = profile.Period
                  MappingCount = uint32 profile.Mapping.Count
                  LocationCount = uint32 profile.Location.Count
                  FunctionCount = uint32 profile.Function.Count }
        else
            None
    | _ -> None

/// The envelope of an uncompressed pprof profile; None when the bytes are
/// not one.
let read (data: byte[]) : Summary option =
    if data.Length = 0 then
        None
    else
        try
            summarise (Profile.Parser.ParseFrom data)
        with :? InvalidProtocolBufferException ->
            None
