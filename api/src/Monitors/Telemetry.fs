/// `ninjacat.monitor.*`, written from the actors' threads and read by the sampler from another.
module NinjaCat.Api.Monitors.Telemetry

open System
open System.Diagnostics
open System.Threading
open Akka.Actor
open Akka.Cluster
open NinjaCat.Api.Platform

type Counter() =
    let mutable value = 0L

    member _.Add(n: int64) = Interlocked.Add(&value, n) |> ignore

    member _.Value = float (Interlocked.Read &value)

/// The largest value seen since the last sample. NaN, which self-monitoring
/// sends as no point, when nothing was seen.
type Peak() =
    let gate = obj ()
    let mutable peak = nan

    member _.Observe(value: float) =
        lock gate (fun () -> if Double.IsNaN peak || value > peak then peak <- value)

    member _.Take() : float =
        lock gate (fun () ->
            let value = peak
            peak <- nan
            value)

type Counters() =
    member val Evaluated = Counter()
    member val Errored = Counter()
    /// The previous evaluation, its insert or a definition read was still running.
    member val SkippedBusy = Counter()
    /// No gate slot within one interval: the node is overloaded.
    member val SkippedGate = Counter()
    /// A tick spent storing the transitions of a failed insert.
    member val SkippedPending = Counter()
    /// The engine judged no group: the window was not full, or had no value.
    member val SkippedWindow = Counter()
    /// A model or a forecast has too little history.
    member val SkippedLearning = Counter()
    /// An outlier monitor has fewer than 3 groups to compare.
    member val SkippedFewGroups = Counter()
    /// Stopwatch ticks, the wait for the gate left out.
    member val EvaluationTicks = Counter()
    /// Seconds from the window's end to the result.
    member val EvaluationLag = Peak()
    member val Entities = Counter()
    member val TransitionsWritten = Counter()
    member val TransitionsFailed = Counter()
    member val TransitionsPending = Counter()
    /// Taken at the poll, so a notifier that keeps failing reports a growing lag rather than none.
    member val NotifierLag = Peak()
    member val NotifierFailed = Counter()

let private reading (name: string) (unit: string) (kind: ReadingKind) (tags: (string * string) list) (read: unit -> float) : Reading =
    { Name = "ninjacat.monitor." + name
      Unit = unit
      Kind = kind
      Tags = tags |> List.map (fun (k, v) -> k, [| v |]) |> Map.ofList
      Read = read }

let readings (counts: Counters) : Reading list =
    let evaluations (tags: (string * string) list) (counter: Counter) =
        reading "evaluations" "" Count tags (fun () -> counter.Value)

    [ evaluations [ "outcome", "evaluated" ] counts.Evaluated
      evaluations [ "outcome", "error" ] counts.Errored
      evaluations [ "outcome", "skipped"; "cause", "busy" ] counts.SkippedBusy
      evaluations [ "outcome", "skipped"; "cause", "gate" ] counts.SkippedGate
      evaluations [ "outcome", "skipped"; "cause", "pending" ] counts.SkippedPending
      evaluations [ "outcome", "skipped"; "cause", "window" ] counts.SkippedWindow
      evaluations [ "outcome", "skipped"; "cause", "learning" ] counts.SkippedLearning
      evaluations [ "outcome", "skipped"; "cause", "outlier_groups" ] counts.SkippedFewGroups
      reading "evaluation.time" "second" Count [] (fun () -> counts.EvaluationTicks.Value / float Stopwatch.Frequency)
      reading "evaluation.lag" "second" Gauge [] counts.EvaluationLag.Take
      reading "entities" "" Gauge [] (fun () -> counts.Entities.Value)
      reading "transitions.written" "" Count [] (fun () -> counts.TransitionsWritten.Value)
      reading "transitions.failed" "" Count [] (fun () -> counts.TransitionsFailed.Value)
      reading "transitions.pending" "" Gauge [] (fun () -> counts.TransitionsPending.Value)
      reading "notifier.lag" "second" Gauge [] counts.NotifierLag.Take
      reading "notifier.failed" "" Count [] (fun () -> counts.NotifierFailed.Value) ]

// The system is asked for at each sample: it starts after the readings are made.
let clusterMembers (system: unit -> ActorSystem) : Reading =
    reading "cluster.members" "" Gauge [] (fun () ->
        Cluster.Get(system ()).State.Members
        |> Seq.filter (fun m -> m.Status = MemberStatus.Up)
        |> Seq.length
        |> float)

let clusterUnreachable (system: unit -> ActorSystem) : Reading =
    reading "cluster.unreachable" "" Gauge [] (fun () -> float (Cluster.Get(system ()).State.Unreachable.Count))
