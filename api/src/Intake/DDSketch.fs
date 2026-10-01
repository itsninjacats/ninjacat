/// The Datadog agent's DDSketch bucketing.
///
/// Sketches built here must land in the same buckets as the ones the agent
/// sends: bucket counts are merged by key across hosts, and a key that means
/// 47.3 ms here and 12 ms there gives percentiles that look plausible and are
/// wrong.
///
/// Ported from DataDog/datadog-agent, pkg/util/quantile/config.go. NOT the
/// DDSketch of the paper (and of the sketches libraries): that one derives
/// gamma differently and has no index offset, so its keys cannot be merged
/// with the agent's.
module NinjaCat.Api.Intake.DDSketch

open System

/// The guaranteed relative error: 1/128, about 0.78 %.
let eps = 1.0 / 128.0

/// The smallest value with a key of its own; anything smaller is key 0.
let min = 1e-9

/// The ratio between consecutive bucket boundaries.
let gamma = 1.0 + 2.0 * eps

let private gammaLn = Double.LogP1(2.0 * eps)

/// Shifts keys so the smallest supported value gets key 1.
let bias = -(int (Math.Floor(Math.Log min / gammaLn))) + 1

/// The bucket a value belongs to.
///
/// Rounds to NEAREST, not down: gamma^(key - bias) is the middle of the
/// bucket, not its floor. The agent's own comment says otherwise; its code
/// does this.
let rec key (value: float) : int32 =
    if value < 0.0 then -(key -value)
    elif value = 0.0 || value < min then 0
    else int32 (int (Math.Round(Math.Log value / gammaLn, MidpointRounding.ToEven)) + bias)

/// The representative value of a bucket: its middle on the logarithmic scale.
let rec value (k: int32) : float =
    if k = 0 then 0.0
    elif k < 0 then -(value -k)
    else Math.Pow(gamma, float (int k - bias))

/// The range of values that map to a bucket: half a step either side of its
/// value.
let bounds (k: int32) : float * float =
    if k = 0 then
        0.0, min
    else
        let exponent = float (int k - bias)
        Math.Pow(gamma, exponent - 0.5), Math.Pow(gamma, exponent + 0.5)

/// The summary that travels with the buckets.
type Stats =
    { Count: int64
      Min: float
      Max: float
      Avg: float
      Sum: float }

/// Raw measurements as agent-compatible buckets: keys ascending, counts in
/// matching order.
type Sketch =
    { Keys: int32[]
      Counts: uint32[]
      Stats: Stats }

let build (values: float[]) : Sketch =
    if values.Length = 0 then
        { Keys = [||]
          Counts = [||]
          Stats = { Count = 0L; Min = 0.0; Max = 0.0; Avg = 0.0; Sum = 0.0 } }
    else
        let tally = values |> Array.countBy key |> Array.sortBy fst
        let sum = Array.sum values

        { Keys = tally |> Array.map fst
          Counts = tally |> Array.map (fun (_, count) -> uint32 count)
          Stats =
            { Count = int64 values.Length
              Min = Array.min values
              Max = Array.max values
              Avg = sum / float values.Length
              Sum = sum } }
