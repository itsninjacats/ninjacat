/// The notifier stub: the actor over fakes, and what it reads from and writes
/// to ClickHouse, in a scratch database (TestClickHouse).
module NinjaCat.Api.Monitors.Tests.NotifierTests

open System
open System.Collections.Concurrent
open System.Threading
open System.Threading.Tasks
open Microsoft.Extensions.Logging.Abstractions
open Xunit
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Monitors
open NinjaCat.Api.Monitors.Notifier
open NinjaCat.Api.Monitors.Tests.TestClickHouse

let private t0 = DateTimeOffset(2026, 10, 14, 12, 0, 0, TimeSpan.Zero)
let private wait = TimeSpan.FromSeconds 5.0

let private due (group: string) (insertedAt: DateTimeOffset) : Due =
    { Tenant = "acme"
      MonitorId = 7UL
      GroupKey = group
      At = insertedAt.AddSeconds -3.0
      InsertedAt = insertedAt
      FromState = "OK"
      ToState = "Alert"
      Reason = "threshold"
      Value = Some 95.0 }

/// A poll that read only these transitions.
let private polled (found: Due list) : Polled =
    { Found = found
      Newest = if found.IsEmpty then None else Some(found |> List.map _.InsertedAt |> List.max) }

/// What the services answer; a test sets it before the notifier starts.
type Fakes() =
    member val Cursor: DateTimeOffset option = Some t0 with get, set
    /// Answered in turn, then nothing.
    member val Batches = ConcurrentQueue<Polled>() with get, set
    member val FailingWrites = 0 with get, set
    /// Each poll's `since`.
    member val Polls = ConcurrentQueue<DateTimeOffset>()
    member val Written = ConcurrentQueue<MonitorNotificationRow[]>()
    member val Counts = Telemetry.Counters()

let private servicesOf (fakes: Fakes) : NotifierServices =
    { Cursor = fun _ -> Task.FromResult fakes.Cursor
      Due =
        fun since _ ->
            fakes.Polls.Enqueue since

            match fakes.Batches.TryDequeue() with
            | true, batch -> Task.FromResult batch
            | _ -> Task.FromResult(polled [])
      Write =
        fun rows _ ->
            let fails = lock fakes (fun () -> fakes.FailingWrites > 0)

            if fails then
                lock fakes (fun () -> fakes.FailingWrites <- fakes.FailingWrites - 1)
                Task.FromException(TimeoutException "the insert timed out")
            else
                fakes.Written.Enqueue rows
                Task.CompletedTask
      Clock = fun () -> t0.AddMinutes 5.0
      Interval = TimeSpan.FromMilliseconds 100.0
      Counts = fakes.Counts
      Log = NullLogger.Instance }

type NotifierActorTests() =
    inherit Akka.TestKit.Xunit.TestKit()

    let fakes = Fakes()

    member this.Start() =
        this.Sys.ActorOf(MonitorNotifier.Props(servicesOf fakes)) |> ignore

    [<Fact>]
    member this.``each due transition gets a stub row, and the cursor moves past it``() =
        fakes.Batches.Enqueue(polled [ due "host:a" (t0.AddMinutes 1.0); due "host:b" (t0.AddMinutes 2.0) ])
        this.Start()

        this.AwaitAssert((fun () -> Assert.Single fakes.Written |> ignore), wait)
        let rows = Seq.head fakes.Written
        Assert.Equal<string list>([ "host:a"; "host:b" ], rows |> Array.map _.GroupKey |> List.ofArray)

        let a = rows[0]
        Assert.Equal(("acme", 7UL, "transition", 0u, "stub", "stub", 1u), (a.TenantID, a.MonitorID, a.Kind, a.Seq, a.Channel, a.Status, a.Attempt))
        Assert.Equal((t0.AddSeconds 57.0).UtcDateTime, a.TransitionAt)
        Assert.Equal((t0.AddMinutes 1.0).UtcDateTime, a.TransitionInsertedAt)
        Assert.Equal((t0.AddMinutes 5.0).UtcDateTime, a.HandledAt)

        // The next poll starts past the newest transition handled.
        this.AwaitAssert((fun () -> Assert.Contains(t0.AddMinutes 2.0, fakes.Polls)), wait)
        Assert.Equal(t0, Seq.head fakes.Polls)
        // The oldest waited longest: four minutes.
        Assert.Equal(240.0, fakes.Counts.NotifierLag.Take())

    [<Fact>]
    member this.``a write that fails leaves the cursor where it was, and is counted``() =
        fakes.FailingWrites <- 1
        let batch = polled [ due "host:a" (t0.AddMinutes 1.0) ]
        fakes.Batches.Enqueue batch
        fakes.Batches.Enqueue batch
        this.Start()

        this.AwaitAssert((fun () -> Assert.Single fakes.Written |> ignore), wait)
        Assert.Equal<DateTimeOffset list>([ t0; t0 ], fakes.Polls |> Seq.take 2 |> List.ofSeq)
        Assert.Equal(1.0, fakes.Counts.NotifierFailed.Value)

    [<Fact>]
    member this.``a notifier that cannot write still reports its lag``() =
        fakes.FailingWrites <- 1
        fakes.Batches.Enqueue(polled [ due "host:a" (t0.AddMinutes 1.0) ])
        this.Start()

        this.AwaitAssert((fun () -> Assert.Equal(1.0, fakes.Counts.NotifierFailed.Value)), wait)
        Assert.Empty fakes.Written
        Assert.Equal(240.0, fakes.Counts.NotifierLag.Take())

    [<Fact>]
    member this.``transitions never notified move the cursor too``() =
        fakes.Batches.Enqueue { Found = []; Newest = Some(t0.AddMinutes 3.0) }
        this.Start()

        this.AwaitAssert((fun () -> Assert.Contains(t0.AddMinutes 3.0, fakes.Polls)), wait)
        Assert.Empty fakes.Written

    [<Fact>]
    member this.``a full batch moves the cursor only past what it notified, and polls again at once``() =
        let found = [ for i in 1..batch -> due $"host:{i}" (t0.AddMinutes 1.0) ]
        fakes.Batches.Enqueue { Found = found; Newest = Some(t0.AddMinutes 3.0) }
        this.Start()

        this.AwaitAssert((fun () -> Assert.True(fakes.Polls.Count >= 2)), wait)
        Assert.Equal<DateTimeOffset list>([ t0; t0.AddMinutes 1.0 ], fakes.Polls |> Seq.take 2 |> List.ofSeq)

    [<Fact>]
    member this.``a notifier that never stored one starts a day back``() =
        fakes.Cursor <- None
        this.Start()

        this.AwaitAssert((fun () -> Assert.NotEmpty fakes.Polls), wait)
        Assert.Equal(t0.AddMinutes(5.0).AddDays -1.0, Seq.head fakes.Polls)

// --- ClickHouse ------------------------------------------------------------------------

let private transition (monitor: uint64) (group: string) (at: DateTime) (into: string) (reason: string) : MonitorTransitionRow =
    { TenantID = "acme"
      MonitorID = monitor
      MonitorVersion = 1u
      QueryHash = "h"
      MonitorKind = "metric"
      GroupKey = group
      GroupTags = [||]
      At = at
      FromState = "none"
      ToState = into
      Reason = reason
      Actor = None
      Value = Some 1.0
      Crossed = "none"
      Comparator = ">"
      Critical = Some 90.0
      Warning = None
      CriticalRecovery = None
      WarningRecovery = None
      WindowFrom = at.AddMinutes -5.0
      WindowTo = at
      FirstTriggeredAt = None
      LastTriggeredAt = None }

[<Fact>]
let ``due leaves out what is never notified and what was already notified`` () =
    inScratch (fun scratch ->
        let sink = ClickHouseSink(scratch, NullLogger.Instance)
        let started = DateTimeOffset.UtcNow
        let at = started.UtcDateTime.AddSeconds -10.0

        let rows =
            [| transition 1UL "host:a" at "Alert" "new"
               transition 1UL "host:b" at "OK" "new"
               transition 1UL "host:c" at "Warn" "threshold"
               transition 1UL "host:d" at "OK" "expired"
               transition 1UL "host:e" at "OK" "definition_changed"
               transition 1UL "host:f" at "OK" "deleted"
               transition 1UL "host:g" at "No Data" "no_data_shown"
               transition 1UL "host:h" at "OK" "no_data"
               transition 2UL "*" at "OK" "recovery" |]

        sink.InsertAsync(MonitorTransitions.table, rows, CancellationToken.None).GetAwaiter().GetResult()
        // Retried, as after a timeout: the same key collapses.
        sink.InsertAsync(MonitorTransitions.table, rows[0..0], CancellationToken.None).GetAwaiter().GetResult()

        let read since = (Notifier.due scratch since CancellationToken.None).GetAwaiter().GetResult()
        // A list, not a set: a duplicate FINAL failed to collapse must show.
        let keys (p: Polled) = p.Found |> List.map (fun d -> d.MonitorId, d.GroupKey) |> List.sort

        let first = read (started.AddMinutes -1.0)
        let found = first.Found
        Assert.Equal<(uint64 * string) list>([ 1UL, "host:a"; 1UL, "host:c"; 1UL, "host:h"; 2UL, "*" ], keys first)
        // The newest covers the transitions never notified as well.
        Assert.True(first.Newest.IsSome && first.Newest.Value >= (found |> List.map _.InsertedAt |> List.max))
        Assert.Equal(None, (Notifier.cursor scratch CancellationToken.None).GetAwaiter().GetResult())

        let notified =
            found
            |> List.filter (fun d -> d.GroupKey = "host:a")
            |> List.map (fun d ->
                { TenantID = d.Tenant
                  MonitorID = d.MonitorId
                  GroupKey = d.GroupKey
                  TransitionAt = d.At.UtcDateTime
                  TransitionInsertedAt = d.InsertedAt.UtcDateTime
                  Kind = "transition"
                  Seq = 0u
                  Channel = "stub"
                  Status = "stub"
                  Attempt = 1u
                  HandledAt = DateTime.UtcNow })
            |> Array.ofList

        sink.InsertAsync(MonitorNotifications.table, notified, CancellationToken.None).GetAwaiter().GetResult()

        Assert.Equal<(uint64 * string) list>([ 1UL, "host:c"; 1UL, "host:h"; 2UL, "*" ], keys (read (started.AddMinutes -1.0)))
        Assert.Equal(Some notified[0].TransitionInsertedAt, (Notifier.cursor scratch CancellationToken.None).GetAwaiter().GetResult() |> Option.map _.UtcDateTime)

        // Inserted less than 30 s before `since`: read again, in case an
        // older insert became visible late.
        Assert.Equal<(uint64 * string) list>([ 1UL, "host:c"; 1UL, "host:h"; 2UL, "*" ], keys (read (DateTimeOffset.UtcNow.AddSeconds 10.0)))

        // More than 30 s before: not read again.
        let later = read (DateTimeOffset.UtcNow.AddMinutes 1.0)
        Assert.Empty later.Found
        Assert.Equal(None, later.Newest))
