/// The latency summaries of APM stats and Data Streams.
///
/// The agent's concentrator and the tracers build these with sketches-go and
/// ship them as protobuf, so this follows sketches-go (ddsketch.FromProto and
/// the few questions the intake asks a sketch). NOT the metric sketches: those
/// follow the agent's own quantile package (DDSketch.fs), and the two cannot
/// be mixed. APM values are span durations in nanoseconds; Data Streams
/// latencies are seconds.
module NinjaCat.Api.Intake.Routers.TraceSketch

open System
open System.Collections.Generic
open Google.Protobuf
open NinjaCat.Api.Storage.Rows

/// One of a sketch's two bin stores, answering what sketches-go's DenseStore
/// answers. Not a dense array: two far-apart indexes in a hostile sketch would
/// ask for gigabytes.
type private Store() =
    let bins = Dictionary<int64, float>()
    let mutable total = 0.0
    let mutable minIndex = Int64.MaxValue
    let mutable maxIndex = Int64.MinValue

    member _.Add(index: int64, count: float) : unit =
        if count <> 0.0 then
            let before =
                match bins.TryGetValue index with
                | true, existing -> existing
                | false, _ -> 0.0

            bins[index] <- before + count
            total <- total + count
            minIndex <- min minIndex index
            maxIndex <- max maxIndex index

    member _.Total: float = total
    member _.IsEmpty: bool = (total = 0.0)
    member _.MinIndex: int64 = minIndex
    member _.MaxIndex: int64 = maxIndex

    /// The bins that hold something, by ascending index.
    member _.Bins: (int64 * float)[] =
        bins
        |> Seq.filter (fun bin -> bin.Value > 0.0)
        |> Seq.map (fun bin -> bin.Key, bin.Value)
        |> Seq.sortBy fst
        |> Array.ofSeq

let private store (sent: Test.Store) : Store =
    let filled = Store()

    if not (isNull sent) then
        for bin in sent.BinCounts do
            filled.Add(int64 bin.Key, bin.Value)

        for i in 0 .. sent.ContiguousBinCounts.Count - 1 do
            filled.Add(int64 i + int64 sent.ContiguousBinIndexOffset, sent.ContiguousBinCounts[i])

    filled

/// Rebuilds a float from its exponent and its significand in [1, 2).
let private buildFloat (exponent: float) (significandPlusOne: float) : float =
    let exponentBits = (uint64 ((int64 exponent + 1023L) <<< 52)) &&& 0x7FF0000000000000UL
    let significandBits = BitConverter.DoubleToUInt64Bits significandPlusOne &&& 0x000FFFFFFFFFFFFFUL
    BitConverter.UInt64BitsToDouble(exponentBits ||| significandBits)

/// The value a bin index stands for, or why the mapping is unusable.
///
/// Three mappings exist: the exact logarithm, and two that approximate it
/// from the float's binary form (linear and cubic interpolation).
let private indexToValue (mapping: Test.IndexMapping) : Result<int64 -> float, string> =
    if isNull mapping then
        Error "the sketch has no index mapping"
    else
        let gamma = mapping.Gamma
        let offset = mapping.IndexOffset

        match mapping.Interpolation with
        | Test.IndexMapping.Types.Interpolation.None
        | Test.IndexMapping.Types.Interpolation.Linear
        | Test.IndexMapping.Types.Interpolation.Cubic when gamma <= 1.0 -> Error "the sketch's gamma must be greater than 1"
        | Test.IndexMapping.Types.Interpolation.None ->
            let multiplier = 1.0 / Math.Log gamma
            let accuracy = 1.0 - 2.0 / (1.0 + gamma)
            Ok(fun index -> Math.Exp ((float index - offset) / multiplier) * (1.0 + accuracy))
        | Test.IndexMapping.Types.Interpolation.Linear ->
            let multiplier = 1.0 / Math.Log2 gamma
            let accuracy = 1.0 - 2.0 / (1.0 + Math.Exp (Math.Log2 gamma))

            Ok(fun index ->
                let x = (float index - offset) / multiplier
                let exponent = Math.Floor x
                buildFloat exponent (x - exponent + 1.0) * (1.0 + accuracy))
        | Test.IndexMapping.Types.Interpolation.Cubic ->
            let multiplier = 1.0 / Math.Log2 gamma
            let accuracy = 1.0 - 2.0 / (1.0 + Math.Exp (0.7 * Math.Log2 gamma))

            // The cubic's coefficients are A = 6/35, B = -3/5, C = 10/7; the
            // constants below are sketches-go's expressions of them.
            let d0 = -459.0 / 1225.0

            Ok(fun index ->
                let x = (float index - offset) / multiplier
                let exponent = Math.Floor x
                let d1 = 5454.0 / 6125.0 - 972.0 / 1225.0 * (x - exponent)
                let p = Math.Cbrt ((d1 - Math.Sqrt(d1 * d1 - 4.0 * d0 * d0 * d0)) / 2.0)
                let significandPlusOne = -(-0.6 + p + d0 / p) / (18.0 / 35.0) + 1.0
                buildFloat exponent significandPlusOne * (1.0 + accuracy))
        | other -> Error $"interpolation not supported: {int other}"

/// Turns one summary into the columns the tables keep: the bytes, the state,
/// and the numbers when it really decoded. Bytes that do not decode are a
/// state with its reason, never a reason to store nothing.
let summary (raw: byte[]) : SketchSummary =
    let blank: SketchSummary =
        { Raw = raw
          State = SketchState.Absent
          Count = None
          Sum = None
          Min = None
          Max = None
          BinKeys = [||]
          BinCounts = [||] }

    if raw.Length = 0 then
        blank
    else
        let parsed =
            try
                Ok(Test.DDSketch.Parser.ParseFrom raw)
            with :? InvalidProtocolBufferException as e ->
                Error $"undecodable: {e.Message}"

        match parsed with
        | Error problem -> { blank with State = SketchState.Undecodable problem }
        | Ok sketch ->
            match indexToValue sketch.Mapping with
            // Bytes that arrived and are not a DDSketch stay in the raw
            // column: usually version skew, and this is the evidence.
            | Error problem -> { blank with State = SketchState.Undecodable $"not a DDSketch: {problem}" }
            | Ok value ->
                let positive = store sketch.PositiveValues
                let negative = store sketch.NegativeValues
                let zeroCount = sketch.ZeroCount

                if zeroCount = 0.0 && positive.IsEmpty && negative.IsEmpty then
                    // A valid sketch with no values: not the same as no sketch.
                    { blank with State = SketchState.Empty }
                else
                    let positiveBins = positive.Bins
                    // sketches-go counts the zero bin into the sum as 0 × count,
                    // which is not 0 for an infinite count.
                    let mutable sum = if zeroCount <> 0.0 then 0.0 * zeroCount else 0.0

                    for index, count in positiveBins do
                        sum <- sum + value index * count

                    for index, count in negative.Bins do
                        sum <- sum + -(value index) * count

                    let minValue =
                        if not negative.IsEmpty then Some(-(value negative.MaxIndex))
                        elif zeroCount > 0.0 then Some 0.0
                        elif not positive.IsEmpty then Some(value positive.MinIndex)
                        else None

                    let maxValue =
                        if not positive.IsEmpty then Some(value positive.MaxIndex)
                        elif zeroCount > 0.0 then Some 0.0
                        elif not negative.IsEmpty then Some(-(value negative.MinIndex))
                        else None

                    // Only the positive store is broken out: these are
                    // durations and sizes. The raw bytes keep the rest.
                    { blank with
                        State = SketchState.Ok
                        Count = Some(zeroCount + positive.Total + negative.Total)
                        Sum = Some sum
                        Min = minValue
                        Max = maxValue
                        BinKeys = positiveBins |> Array.map (fun (index, _) -> int32 index)
                        BinCounts = positiveBins |> Array.map snd }
