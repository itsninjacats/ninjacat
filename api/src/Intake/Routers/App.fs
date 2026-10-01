/// app.<site> — columnar metrics, intake v3.
///
///   agent config: dd_url, plus use_v3_api.series.enabled: true. The default,
///   "datadog_only", keeps a third-party dd_url on /api/v2/series (Api.fs).
///   Sketches need the URL listed under
///   serializer_experimental_use_v3_api.sketches.endpoints.
///
/// Three routes, one message: intake_v3.Payload. Series and sketches differ
/// only in the metricType nibble of each Types entry, and land in the same
/// two tables the v1/v2 handlers feed: it is the same signal, denser.
///
/// The body is not one compressed blob. The agent compresses every column as
/// its own zstd/gzip frame and concatenates them, relying on the decompressor
/// to splice consecutive frames. Length prefixes inside describe the
/// UNCOMPRESSED data, so the body is decompressed first and parsed second.
///
/// A body that does not decode, a payload with no series, a per-series column
/// that is short or missing, and a walk that runs off the end of a value
/// column all go to raw_payloads.
module NinjaCat.Api.Intake.Routers.App

open System
open System.Text
open Google.Protobuf
open Microsoft.Extensions.Logging
open Datadoghq.Api.Metrics.V3
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// The two ways a column can be wrong. Notes in raw_payloads quote them.
let truncated = "column ends mid-element"
let badReference = "reference outside its dictionary"

let private sketchType = uint64 metricType.Sketch
let private zeroValue = uint64 valueType.Zero
let private sint64Value = uint64 valueType.Sint64
let private float32Value = uint64 valueType.Float32
let private float64Value = uint64 valueType.Float64
let private flagHasUnit = uint64 metricFlags.FlagHasUnit

/// The largest integer a Float64 holds exactly. The value column is a
/// Float64, so a sint64 past this is rounded on the way in.
let private maxExactInt = 1L <<< 53

/// Go's binary.Uvarint: the number at `start` and the bytes it took. None
/// when the input ends inside the number or the number overflows 64 bits.
let private uvarint (raw: byte[]) (start: int) : (uint64 * int) option =
    let mutable value = 0UL
    let mutable shift = 0
    let mutable index = 0
    let mutable result = None
    let mutable finished = false

    while not finished && start + index < raw.Length do
        let b = raw[start + index]

        if index = 10 then
            finished <- true
        elif b < 0x80uy then
            if not (index = 9 && b > 1uy) then
                result <- Some(value ||| (uint64 b <<< shift), index + 1)

            finished <- true
        else
            value <- value ||| (uint64 (b &&& 0x7Fuy) <<< shift)
            shift <- shift + 7
            index <- index + 1

    result

/// A "varint length + bytes" string dictionary.
///
/// Every reference column is base-1 with the empty string implicit at 0, so
/// the array is laid out to be indexed directly. Truncated input gives the
/// entries that were read AND the problem: the good entries are still used,
/// and the payload is kept raw.
let strDict (raw: byte[]) : string[] * string option =
    let entries = ResizeArray<string>([ "" ])
    let mutable at = 0
    let mutable problem = None

    while problem.IsNone && at < raw.Length do
        match uvarint raw at with
        | Some(length, taken) when length <= uint64 (raw.Length - at - taken) ->
            entries.Add(Encoding.UTF8.GetString(raw, at + taken, int length))
            at <- at + taken + int length
        | _ -> problem <- Some truncated

    entries.ToArray(), problem

/// A base-1 dictionary entry, or None for a reference outside it.
let private lookup (dict: string[]) (reference: int64) : string option =
    if reference < 0L || reference >= int64 dict.Length then None else Some dict[int reference]

/// The tagset dictionary: a flat sequence of "length, then that many
/// delta-encoded indexes into the tag strings".
///
/// The delta accumulator restarts at every set. A NEGATIVE accumulated index
/// is not a tag but an earlier tagset, included whole: that is how the agent
/// avoids repeating the tags every series of a host shares. Entry 0 is the
/// implicit empty set, so back-references are base-1 as well.
let tagsets (packed: int64[]) (tagStr: string[]) : string[][] * string option =
    let sets = ResizeArray<string[]>([ [||] ])
    let mutable at = 0
    let mutable problem = None

    while problem.IsNone && at < packed.Length do
        let size = packed[at]
        at <- at + 1

        if size < 0L || size > int64 (packed.Length - at) then
            problem <- Some truncated
        else
            let tags = ResizeArray<string>()
            let mutable index = 0L
            let mutable i = 0

            while problem.IsNone && i < int size do
                index <- index + packed[at + i]

                if index < 0L then
                    if index = Int64.MinValue || -index >= int64 sets.Count then
                        problem <- Some badReference
                    else
                        tags.AddRange sets[int (-index)]
                elif index >= int64 tagStr.Length then
                    problem <- Some badReference
                else
                    tags.Add tagStr[int index]

                i <- i + 1

            if problem.IsNone then
                sets.Add(tags.ToArray())
                at <- at + int size

    sets.ToArray(), problem

/// One [Type, Name] pair of a resource set.
type Resource = { Type: string; Name: string }

/// The resource-set dictionary: `lens` gives the size of each set, `typeCol`
/// and `nameCol` are parallel delta-encoded columns covering all sets end to
/// end.
///
/// The accumulators restart at each SET — the one delta column of this format
/// that does not run across the whole array. Read the other way, every host
/// after the first gets the wrong name.
let resources (lens: int64[]) (typeCol: int64[]) (nameCol: int64[]) (strs: string[]) : Resource[][] * string option =
    let sets = ResizeArray<Resource[]>([ [||] ])
    let mutable start = 0
    let mutable problem = None
    let mutable s = 0

    while problem.IsNone && s < lens.Length do
        let size = lens[s]

        if size < 0L || size > int64 (typeCol.Length - start) || size > int64 (nameCol.Length - start) then
            problem <- Some truncated
        else
            let set = ResizeArray<Resource>()
            let mutable typeRef = 0L
            let mutable nameRef = 0L
            let mutable i = 0

            while problem.IsNone && i < int size do
                typeRef <- typeRef + typeCol[start + i]
                nameRef <- nameRef + nameCol[start + i]

                match lookup strs typeRef, lookup strs nameRef with
                | Some resourceType, Some name -> set.Add { Type = resourceType; Name = name }
                | _ -> problem <- Some badReference

                i <- i + 1

            if problem.IsNone then
                sets.Add(set.ToArray())
                start <- start + int size

        s <- s + 1

    sets.ToArray(), problem

/// One entry of DictOriginInfo. The numbers are Datadog's private
/// origin.proto enums, the same ones a v2 series carries.
type Origin =
    { Product: uint32
      Category: uint32
      Service: uint32 }

/// DictOriginInfo: flattened (product, category, service) triples, base-1
/// like every other dictionary.
let origins (raw: int32[]) : Origin[] * string option =
    let triples = ResizeArray<Origin>([ { Product = 0u; Category = 0u; Service = 0u } ])
    let mutable i = 0

    while i + 2 < raw.Length do
        triples.Add
            { Product = uint32 raw[i]
              Category = uint32 raw[i + 1]
              Service = uint32 raw[i + 2] }

        i <- i + 3

    triples.ToArray(), (if raw.Length % 3 <> 0 then Some truncated else None)

/// How the UnitRefs column is laid out.
///
/// THE ONE AMBIGUITY OF THIS FORMAT. The proto says only "value present if
/// flagHasUnit is set, entire array is delta encoded", which reads either
/// way: one entry per series, like every other ref column, or one entry per
/// FLAGGED series. The agent's reference reader predates the column and
/// settles nothing, and choosing wrong shifts every unit of the payload onto
/// the wrong metric.
type UnitLayout =
    | NoUnits
    /// One entry per series.
    | Parallel
    /// One entry per series with flagHasUnit.
    | Compact

/// The length decides, per payload. A length that fits neither reading means
/// no units at all and a trip to raw_payloads: a wrong unit is worse than a
/// missing one.
let unitLayout (types: uint64[]) (refs: int) : UnitLayout * string option =
    if refs = 0 then
        NoUnits, None
    else
        let flagged = types |> Array.filter (fun packed -> packed &&& flagHasUnit <> 0UL) |> Array.length

        if refs = types.Length then
            Parallel, None
        elif refs = flagged then
            Compact, None
        else
            NoUnits,
            Some
                $"unitRefs has {refs} entries: neither one per series ({types.Length}) nor one per flagHasUnit series ({flagged}), units dropped"

/// One payload turned into rows.
type Decoded =
    { Points: MetricPoint[]
      Sketches: SketchRow[]
      /// sint64 values beyond 2^53, stored rounded.
      WideInts: int
      /// The FIRST inconsistency the walk met. It never stops the rows
      /// already decoded from being stored; it means the payload ALSO goes to
      /// raw_payloads, so the part that could not be read is not gone.
      Problem: string option }

/// Where the walk stands in the columns all series share. A short column
/// therefore spoils the rest of the walk, not one series: the walk stops at
/// the first one.
type private Cursors =
    { mutable Point: int
      mutable Sint64: int
      mutable Float32: int
      mutable Float64: int
      mutable NumBins: int
      mutable Bins: int
      mutable Unit: int
      mutable WideInts: int }

type private SketchPoint =
    { Keys: int32[]
      Counts: uint32[]
      Sum: float
      Min: float
      Max: float
      Count: uint64 }

/// Walks the columnar payload and builds the rows.
///
/// The encoding has no published reader outside the agent, so the walk
/// follows the agent's own MetricDataReader (comp/dogstatsd/http/impl/
/// internal/reader), because these rules cannot be guessed from the .proto:
///
///   - every dictionary index is base-1, the empty value implicit at 0;
///   - delta columns accumulate across the whole array, except the resource
///     Type/Name columns (restart per set) and SketchBinKeys (per POINT);
///   - the value column is chosen by the ValueType nibble, and Zero consumes
///     nothing at all;
///   - a sketch point spends three consecutive entries of its value column on
///     sum, min, max and always one entry of ValsSint64 on the count. Avg is
///     not on the wire; the intake reconstructs it as sum/count.
///
/// It is deliberately more forgiving than the agent's reader, which drops the
/// payload on the first error: losing forty good series because the
/// forty-first column is short is the failure this project exists to avoid.
let decode (tenant: string) (payload: Payload) : Decoded =
    let md = if isNull payload || isNull payload.MetricData then MetricData() else payload.MetricData
    let metadata = if isNull payload || isNull payload.Metadata then Metadata() else payload.Metadata
    // Only the first problem is kept: it is the one the later ones follow from.
    let problems = ResizeArray<string>()

    let fail (text: string) =
        if problems.Count = 0 then
            problems.Add text

    let keep(column: string) (entries: 'a, problem: string option) : 'a =
        match problem with
        | Some text -> fail $"{column}: {text}"
        | None -> ()

        entries

    // A truncated dictionary is used as far as it goes: the series that
    // reference the part that was read are still good rows.
    let names = keep "dictNameStr" (strDict (md.DictNameStr.ToByteArray()))
    let tagStr = keep "dictTagStr" (strDict (md.DictTagStr.ToByteArray()))
    let tagsetDict = keep "dictTagsets" (tagsets (Array.ofSeq md.DictTagsets) tagStr)
    let resourceStr = keep "dictResourceStr" (strDict (md.DictResourceStr.ToByteArray()))

    let resourceSets =
        keep
            "dictResource"
            (resources (Array.ofSeq md.DictResourceLen) (Array.ofSeq md.DictResourceType) (Array.ofSeq md.DictResourceName) resourceStr)

    let sourceTypes = keep "dictSourceTypeName" (strDict (md.DictSourceTypeName.ToByteArray()))
    let units = keep "dictUnitStr" (strDict (md.DictUnitStr.ToByteArray()))
    let originDict = keep "dictOriginInfo" (origins (Array.ofSeq md.DictOriginInfo))

    // Metadata.Tags apply to EVERY series, and Metadata.Resources carries the
    // host of a payload whose series do not name one themselves.
    let metaTags = Array.ofSeq metadata.Tags
    let metaResources = Array.ofSeq metadata.Resources

    let metaHost =
        metaResources
        |> Array.chunkBySize 2
        |> Array.tryPick (fun pair -> if pair.Length = 2 && pair[0] = "host" && pair[1] <> "" then Some pair[1] else None)
        |> Option.defaultValue ""

    if metaResources.Length % 2 <> 0 then
        fail $"metadata.resources has {metaResources.Length} elements, want [Type, Name] pairs"

    let types = Array.ofSeq md.Types_
    let nameRefs = Array.ofSeq md.NameRefs
    let tagsetRefs = Array.ofSeq md.TagsetRefs
    let resourceRefs = Array.ofSeq md.ResourcesRefs
    let numPoints = Array.ofSeq md.NumPoints
    let sourceRefs = Array.ofSeq md.SourceTypeNameRefs
    let originRefs = Array.ofSeq md.OriginInfoRefs
    let unitRefs = Array.ofSeq md.UnitRefs
    let intervals = Array.ofSeq md.Intervals

    // Each of these carries one entry per series, unconditionally: the
    // agent's reader refuses the whole payload when any of them is shorter,
    // an empty one included. Reading an empty column as "empty for every
    // series" corrupts silently — no nameRefs files every series under the
    // empty name, no numPoints walks no points and acks a body that
    // contributed nothing. unitRefs is the one column that may be absent.
    for column, length in
        [ "nameRefs", nameRefs.Length
          "tagsetRefs", tagsetRefs.Length
          "resourcesRefs", resourceRefs.Length
          "numPoints", numPoints.Length
          "sourceTypeNameRefs", sourceRefs.Length
          "originInfoRefs", originRefs.Length
          "intervals", intervals.Length ] do
        if length < types.Length then
            fail $"{column} has {length} entries for {types.Length} series"

    let layout, layoutProblem = unitLayout types unitRefs.Length

    match layoutProblem with
    | Some text -> fail text
    | None -> ()

    let timestamps = Array.ofSeq md.Timestamps
    let valsSint64 = Array.ofSeq md.ValsSint64
    let valsFloat32 = Array.ofSeq md.ValsFloat32
    let valsFloat64 = Array.ofSeq md.ValsFloat64
    let numBins = Array.ofSeq md.SketchNumBins
    let binKeys = Array.ofSeq md.SketchBinKeys
    let binCounts = Array.ofSeq md.SketchBinCnts

    let at: Cursors =
        { Point = 0
          Sint64 = 0
          Float32 = 0
          Float64 = 0
          NumBins = 0
          Bins = 0
          Unit = 0
          WideInts = 0 }

    let nextSint64 () : float option =
        if at.Sint64 >= valsSint64.Length then
            None
        else
            let value = valsSint64[at.Sint64]
            at.Sint64 <- at.Sint64 + 1

            if value > maxExactInt || value < -maxExactInt then
                at.WideInts <- at.WideInts + 1

            Some(float value)

    /// A point's value, or the note of the column that ran out.
    let scalar (valueType: uint64) (series: int) (point: uint64) : Result<float, string> =
        if valueType = zeroValue then
            Ok 0.0
        elif valueType = sint64Value then
            match nextSint64 () with
            | Some value -> Ok value
            | None -> Error $"valsSint64 ran out at series {series} point {point}: {truncated}"
        elif valueType = float32Value then
            if at.Float32 >= valsFloat32.Length then
                Error $"valsFloat32 ran out at series {series} point {point}: {truncated}"
            else
                at.Float32 <- at.Float32 + 1
                Ok(float valsFloat32[at.Float32 - 1])
        elif valueType = float64Value then
            if at.Float64 >= valsFloat64.Length then
                Error $"valsFloat64 ran out at series {series} point {point}: {truncated}"
            else
                at.Float64 <- at.Float64 + 1
                Ok valsFloat64[at.Float64 - 1]
        else
            fail $"series {series}: unknown valueType 0x{valueType:x}"
            Ok 0.0

    /// A sketch point's sum, min and max.
    let summary (valueType: uint64) (series: int) (point: uint64) : Result<float * float * float, string> =
        if valueType = zeroValue then
            Ok(0.0, 0.0, 0.0)
        elif valueType = sint64Value then
            let sum = nextSint64 ()
            let min = if sum.IsSome then nextSint64 () else None
            let max = if min.IsSome then nextSint64 () else None

            match sum, min, max with
            | Some sum, Some min, Some max -> Ok(sum, min, max)
            | _ -> Error $"valsSint64 ran out in a sketch summary at series {series} point {point}: {truncated}"
        elif valueType = float32Value then
            if at.Float32 + 3 > valsFloat32.Length then
                Error $"valsFloat32 ran out in a sketch summary at series {series} point {point}: {truncated}"
            else
                let first = at.Float32
                at.Float32 <- first + 3
                Ok(float valsFloat32[first], float valsFloat32[first + 1], float valsFloat32[first + 2])
        elif valueType = float64Value then
            if at.Float64 + 3 > valsFloat64.Length then
                Error $"valsFloat64 ran out in a sketch summary at series {series} point {point}: {truncated}"
            else
                let first = at.Float64
                at.Float64 <- first + 3
                Ok(valsFloat64[first], valsFloat64[first + 1], valsFloat64[first + 2])
        else
            fail $"series {series}: unknown valueType 0x{valueType:x}"
            Ok(0.0, 0.0, 0.0)

    let sketchPoint (valueType: uint64) (series: int) (point: uint64) : Result<SketchPoint, string> =
        if at.NumBins >= numBins.Length then
            Error $"sketchNumBins ran out at series {series} point {point}: {truncated}"
        else
            let bins = numBins[at.NumBins]
            at.NumBins <- at.NumBins + 1

            if bins > uint64 (binKeys.Length - at.Bins) || bins > uint64 (binCounts.Length - at.Bins) then
                Error $"sketch bins ran out at series {series} point {point}: {truncated}"
            else
                // The keys are delta-encoded per point: each starts from zero.
                let keys = Array.zeroCreate<int32> (int bins)
                let mutable key = 0

                for i in 0 .. keys.Length - 1 do
                    key <- key + binKeys[at.Bins + i]
                    keys[i] <- key

                let counts = Array.sub binCounts at.Bins keys.Length
                at.Bins <- at.Bins + keys.Length

                match summary valueType series point with
                | Error text -> Error text
                | Ok(sum, min, max) ->
                    // The count is ALWAYS a sint64, whatever the summary's
                    // value type: the one part of the sketch layout the
                    // ValueType nibble does not tell.
                    match nextSint64 () with
                    | None -> Error $"valsSint64 ran out reading a sketch count at series {series} point {point}: {truncated}"
                    | Some count ->
                        Ok
                            { Keys = keys
                              Counts = counts
                              Sum = sum
                              Min = min
                              Max = max
                              Count = (if count > 0.0 then uint64 count else 0UL) }

    let points = ResizeArray<MetricPoint>()
    let sketches = ResizeArray<SketchRow>()

    // Accumulators of the columns delta-encoded across the whole array.
    let mutable nameRef = 0L
    let mutable tagsetRef = 0L
    let mutable resourceRef = 0L
    let mutable sourceRef = 0L
    let mutable originRef = 0L
    let mutable unitRef = 0L
    let mutable timestamp = 0L

    // A column too short for the series array was reported above; here it
    // only has to give a neutral delta.
    let delta (column: int64[]) (series: int) : int64 =
        if series < column.Length then column[series] else 0L

    let mutable stopped = false
    let mutable i = 0

    while not stopped && i < types.Length do
        let packed = types[i]
        let kind = packed &&& 0xFUL
        let valueType = packed &&& 0xF0UL

        nameRef <- nameRef + delta nameRefs i
        tagsetRef <- tagsetRef + delta tagsetRefs i
        resourceRef <- resourceRef + delta resourceRefs i
        sourceRef <- sourceRef + delta sourceRefs i
        originRef <- originRef + delta originRefs i

        let metric =
            match lookup names nameRef with
            | Some name -> name
            | None ->
                fail $"series {i} nameRef {nameRef}: {badReference}"
                ""

        let tags =
            if tagsetRef < 0L || tagsetRef >= int64 tagsetDict.Length then
                fail $"series {i} tagsetRef {tagsetRef}: {badReference}"
                [||]
            else
                tagsetDict[int tagsetRef]

        // A key may appear in both lists; the multiset keeps both values.
        let tagMap = Tags.toMultiMap (Array.append tags metaTags)

        let host =
            if resourceRef < 0L || resourceRef >= int64 resourceSets.Length then
                fail $"series {i} resourcesRef {resourceRef}: {badReference}"
                metaHost
            else
                match resourceSets[int resourceRef] |> Array.tryFindBack (fun resource -> resource.Type = "host") with
                | Some resource -> resource.Name
                | None -> metaHost

        let sourceType =
            match lookup sourceTypes sourceRef with
            | Some name -> name
            | None ->
                fail $"series {i} sourceTypeNameRef {sourceRef}: {badReference}"
                ""

        // Checked but not stored: as in the Go intake, a v3 row carries
        // neither its origin nor its non-host resources yet.
        if originRef < 0L || originRef >= int64 originDict.Length then
            fail $"series {i} originInfoRef {originRef}: {badReference}"

        let hasUnit = packed &&& flagHasUnit <> 0UL

        let unit =
            match layout with
            | Parallel ->
                unitRef <- unitRef + unitRefs[i]
                if hasUnit then lookup units unitRef |> Option.defaultValue "" else ""
            | Compact when hasUnit && at.Unit < unitRefs.Length ->
                unitRef <- unitRef + unitRefs[at.Unit]
                at.Unit <- at.Unit + 1
                lookup units unitRef |> Option.defaultValue ""
            | Compact
            | NoUnits -> ""

        let interval = if i < intervals.Length then uint32 intervals[i] else 0u
        let pointCount = if i < numPoints.Length then numPoints[i] else 0UL
        let mutable p = 0UL

        while not stopped && p < pointCount do
            if at.Point >= timestamps.Length then
                fail $"timestamps ran out at series {i} point {p}: {truncated}"
                stopped <- true
            else
                timestamp <- timestamp + timestamps[at.Point]
                at.Point <- at.Point + 1
                let time = Time.wireSeconds timestamp

                if kind <> sketchType then
                    match scalar valueType i p with
                    | Error text ->
                        fail text
                        stopped <- true
                    | Ok value ->
                        points.Add
                            { Metrics.point tenant time metric value with
                                Host = host
                                // The v3 numbers are the v2 ones for Count,
                                // Rate and Gauge.
                                MetricType = Wire.metricTypeName (int kind)
                                SourceType = sourceType
                                Unit = unit
                                Interval = interval
                                Tags = tagMap }
                else
                    match sketchPoint valueType i p with
                    | Error text ->
                        fail text
                        stopped <- true
                    | Ok sketch ->
                        sketches.Add
                            { TenantID = tenant
                              Timestamp = time
                              Metric = metric
                              Host = host
                              Tags = tagMap
                              Count = sketch.Count
                              Min = sketch.Min
                              Max = sketch.Max
                              Avg = (if sketch.Count > 0UL then sketch.Sum / float sketch.Count else 0.0)
                              Sum = sketch.Sum
                              BucketKeys = sketch.Keys
                              BucketCounts = sketch.Counts
                              OriginProduct = 0u
                              OriginCategory = 0u
                              OriginService = 0u
                              Resources = Map.empty
                              Extra = Map.empty }

            p <- p + 1UL

        i <- i + 1

    { Points = points.ToArray()
      Sketches = sketches.ToArray()
      WideInts = at.WideInts
      Problem = Seq.tryHead problems }

let private parse (body: byte[]) : Result<Payload, string> =
    try
        Ok(Payload.Parser.ParseFrom body)
    with :? InvalidProtocolBufferException as e ->
        Error e.Message

/// `label` names the route in raw_payloads and in the log.
let private handle (label: string) (r: Request) : Response =
    match parse r.Body with
    | Error message ->
        r.Log.LogWarning("[{Label}] protobuf: {Error} ({Bytes} bytes)", label, message, r.Body.Length)
        Raw.store r label "decode_error" message r.Body
    | Ok payload ->
        let series = if isNull payload.MetricData then 0 else payload.MetricData.Types_.Count

        if series = 0 then
            // A protobuf parser fails OPEN: a body that is still compressed,
            // or meant for another intake, often parses as a valid Payload
            // with no columns. The bytes are the only evidence of what the
            // sender meant.
            let tags = if isNull payload.Metadata then 0 else payload.Metadata.Tags.Count
            let resources = if isNull payload.Metadata then 0 else payload.Metadata.Resources.Count

            r.Log.LogWarning(
                "[{Label}] decoded to zero series ({Bytes} bytes) — wrong payload type or still compressed?",
                label,
                r.Body.Length
            )

            Raw.store
                r
                label
                "unexpected_shape"
                $"protobuf decoded but MetricData carries no series; metadata tags={tags} resources={resources}"
                r.Body
        elif r.Tenant <> "" then
            let decoded = decode r.Tenant payload
            Sink.write r.Sink Metrics.table decoded.Points
            Sink.write r.Sink Sketches.table decoded.Sketches

            // The rows above are what the walk could read; the two notes
            // below keep what it could not. A payload that decodes cleanly is
            // never mirrored into raw_payloads.
            match decoded.Problem with
            | Some problem ->
                r.Log.LogWarning("[{Label}] columnar walk: {Problem} — payload kept raw", label, problem)
                Raw.store r label "unexpected_shape" problem r.Body
            | None -> ()

            if decoded.WideInts > 0 then
                // The row is still written (a rounded value beats no value)
                // and the payload is kept, so the exact integer stays
                // recoverable.
                r.Log.LogWarning(
                    "[{Label}] {Count} sint64 values beyond 2^53 — stored rounded, payload kept raw",
                    label,
                    decoded.WideInts
                )

                Raw.store r label "int64_precision" $"{decoded.WideInts} sint64 values beyond 2^53 widened to Float64" r.Body

    Response.json 202 "{}"

let routes: Route list =
    [ Route.post "/api/intake/metrics/v3/series" (handle "v3series")
      Route.post "/api/intake/metrics/v3/sketches" (handle "v3sketches")
      // A shadow copy of the v2 traffic on /api/beta/sketches.
      Route.post "/api/intake/metrics/v3beta/sketches" (handle "v3beta") ]
