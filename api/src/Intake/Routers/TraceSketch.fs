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

/// Go's math.Exp, Log, Log2 and Cbrt, operation for operation.
///
/// A summary's min, max and sum come out of these, and .NET's own (the C
/// library's) round the last bits differently. The Go intake and this one
/// must store the same numbers for the same sketch, so the arithmetic is
/// Go's: its x86-64 routines, exp in the fused-multiply-add form Go picks on
/// any CPU that has the instruction.
module private GoMath =
    let private fma (a: float) (b: float) (c: float) : float = Math.FusedMultiplyAdd(a, b, c)

    let private bits (value: uint64) : float = BitConverter.UInt64BitsToDouble value

    let log (x: float) : float =
        if Double.IsNaN x || Double.IsPositiveInfinity x then
            x
        elif x < 0.0 then
            Double.NaN
        elif x = 0.0 then
            Double.NegativeInfinity
        else
            let raw = BitConverter.DoubleToUInt64Bits x
            // x = fraction × 2^k, fraction in [0.5, 1)…
            let fraction = bits ((raw &&& 0x000FFFFFFFFFFFFFUL) ||| 0x3FE0000000000000UL)
            let k = float (int ((raw >>> 52) &&& 0x7FFUL) - 0x3FE)
            // …then moved to [sqrt(2)/2, sqrt(2)), where the series converges.
            let low = fraction <= 7.07106781186547524401e-01
            let k = if low then k - 1.0 else k
            let f = (if low then fraction * 2.0 else fraction) - 1.0

            let s = f / (2.0 + f)
            let s2 = s * s
            let s4 = s2 * s2

            let t1 =
                s2
                * (6.666666666666735130e-01
                   + s4 * (2.857142874366239149e-01 + s4 * (1.818357216161805012e-01 + s4 * 1.479819860511658591e-01)))

            let t2 = s4 * (3.999999999940941908e-01 + s4 * (2.222219843214978396e-01 + s4 * 1.531383769920937332e-01))
            let r = t1 + t2
            let hfsq = 0.5 * f * f
            k * 6.93147180369123816490e-01 - ((hfsq - (s * (hfsq + r) + k * 1.90821492927058770002e-10)) - f)

    /// For a finite x above zero, which is all a sketch's gamma can be.
    let log2 (x: float) : float =
        if not (Double.IsFinite x) then
            x
        else
            let raw = BitConverter.DoubleToUInt64Bits x
            let fraction = bits ((raw &&& 0x000FFFFFFFFFFFFFUL) ||| 0x3FE0000000000000UL)
            let exponent = int ((raw >>> 52) &&& 0x7FFUL) - 0x3FE

            if fraction = 0.5 then
                float (exponent - 1)
            else
                log fraction * 1.4426950408889634 + float exponent

    let exp (x: float) : float =
        if Double.IsNaN x || Double.IsPositiveInfinity x then
            x
        elif Double.IsNegativeInfinity x then
            0.0
        elif x > 7.09782712893384e+02 then
            Double.PositiveInfinity
        else
            // x = k·ln2 + r: e^x is e^r scaled by 2^k.
            let rounded = Math.Round(1.4426950408889634 * x, MidpointRounding.ToEven)
            let k = if rounded >= -2147483648.0 && rounded <= 2147483647.0 then int rounded else Int32.MinValue
            let kf = float k
            let r = fma -kf 0.69314718055966295651160180568695068359375 x
            let r = fma -kf 0.28235290563031577122588448175013436025525412068e-12 r
            // A sixteenth of r in the series, undone by squaring four times.
            let r = r * 0.0625

            let p = 2.4801587301587301587e-5
            let p = fma r p 1.9841269841269841270e-4
            let p = fma r p 1.3888888888888888889e-3
            let p = fma r p 8.3333333333333333333e-3
            let p = fma r p 4.1666666666666666667e-2
            let p = fma r p 1.6666666666666666667e-1
            let p = fma r p 0.5
            let p = fma r p 1.0

            let y = r * p
            let y = y * (y + 2.0)
            let y = y * (y + 2.0)
            let y = y * (y + 2.0)
            let y = fma (y + 2.0) y 1.0

            let biased = k + 0x3FF

            if biased >= 0x7FF then
                Double.PositiveInfinity
            elif biased > 0 then
                y * bits (uint64 biased <<< 52)
            elif biased < -52 then
                0.0
            else
                y * bits (uint64 (biased + 0x3FE) <<< 52) * bits (1UL <<< 52)

    let cbrt (x: float) : float =
        if x = 0.0 || not (Double.IsFinite x) then
            x
        else
            let magnitude = abs x

            // A rough root to 5 bits, from the exponent…
            let rough =
                if magnitude < 2.22507385850720138309e-308 then
                    bits (BitConverter.DoubleToUInt64Bits(18014398509481984.0 * magnitude) / 3UL + (696219795UL <<< 32))
                else
                    bits (BitConverter.DoubleToUInt64Bits magnitude / 3UL + (715094163UL <<< 32))

            // …refined to 23 bits…
            let r = rough * rough / magnitude
            let s = 5.42857142857142815906e-01 + r * rough

            let t =
                rough
                * (3.57142857142857150787e-01
                   + 1.60714285714285720630e+00 / (s + 1.41428571428571436819e+00 + -7.05306122448979611050e-01 / s))

            let t = bits ((BitConverter.DoubleToUInt64Bits t &&& (0xFFFFFFFFCUL <<< 28)) + (1UL <<< 30))

            // …and one Newton step to 53.
            let s = t * t
            let r = magnitude / s
            let r = (r - t) / (t + t + r)
            let t = t + t * r
            if x < 0.0 then -t else t

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
        Error "cannot create IndexMapping from nil protobuf index mapping"
    else
        let gamma = mapping.Gamma
        let offset = mapping.IndexOffset

        match mapping.Interpolation with
        | Test.IndexMapping.Types.Interpolation.None
        | Test.IndexMapping.Types.Interpolation.Linear
        | Test.IndexMapping.Types.Interpolation.Cubic when gamma <= 1.0 -> Error "Gamma must be greater than 1."
        | Test.IndexMapping.Types.Interpolation.None ->
            let multiplier = 1.0 / GoMath.log gamma
            let accuracy = 1.0 - 2.0 / (1.0 + gamma)
            Ok(fun index -> GoMath.exp ((float index - offset) / multiplier) * (1.0 + accuracy))
        | Test.IndexMapping.Types.Interpolation.Linear ->
            let multiplier = 1.0 / GoMath.log2 gamma
            let accuracy = 1.0 - 2.0 / (1.0 + GoMath.exp (GoMath.log2 gamma))

            Ok(fun index ->
                let x = (float index - offset) / multiplier
                let exponent = Math.Floor x
                buildFloat exponent (x - exponent + 1.0) * (1.0 + accuracy))
        | Test.IndexMapping.Types.Interpolation.Cubic ->
            let multiplier = 1.0 / GoMath.log2 gamma
            let accuracy = 1.0 - 2.0 / (1.0 + GoMath.exp (0.7 * GoMath.log2 gamma))

            // The cubic's coefficients are A = 6/35, B = -3/5, C = 10/7; the
            // constants below are sketches-go's expressions of them, folded
            // exactly as the Go compiler folds them.
            let d0 = -459.0 / 1225.0

            Ok(fun index ->
                let x = (float index - offset) / multiplier
                let exponent = Math.Floor x
                let d1 = 5454.0 / 6125.0 - 972.0 / 1225.0 * (x - exponent)
                let p = GoMath.cbrt ((d1 - Math.Sqrt(d1 * d1 - 4.0 * d0 * d0 * d0)) / 2.0)
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
