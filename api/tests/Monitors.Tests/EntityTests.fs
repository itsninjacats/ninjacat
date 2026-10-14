/// The entity driven by hand: ticks sent to it, Postgres, ClickHouse and the
/// sink replaced by functions over values the test sets.
module NinjaCat.Api.Monitors.Tests.EntityTests

open System
open System.Text.RegularExpressions
open System.Threading
open System.Threading.Tasks
open Xunit
open Akka.Actor
open Akka.TestKit
open Microsoft.Extensions.Logging.Abstractions
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Engine.Monitor.Definition
open NinjaCat.Api.Engine.Monitor.State
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Monitors
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Entity
open NinjaCat.Api.Monitors.Messages

let private key = { Tenant = "acme"; Id = 7L }
let private t0 = DateTimeOffset(2026, 10, 13, 12, 0, 0, TimeSpan.Zero)
let private minute = TimeSpan.FromMinutes 1.0
let private wait = TimeSpan.FromSeconds 5.0
let private quiet = TimeSpan.FromMilliseconds 300.0

let private cpu = "avg(last_5m):avg:system.cpu.user{*} > 90"
let private critical90 = """{"thresholds":{"critical":90}}"""

let private monitorRow (version: int) (query: string) (options: string) : MonitorRow =
    { Key = key
      Version = version
      Type = "query alert"
      Query = query
      Options = options
      UpdatedBy = Some "user-1"
      DeletedBy = None
      DeletedAt = None }

let private deletedRow (version: int) (at: DateTimeOffset) =
    Some
        { monitorRow version cpu critical90 with
            DeletedBy = Some "user-3"
            DeletedAt = Some at }

/// One group in Alert since `since`, as a restart would load it.
let private alertSince (since: DateTimeOffset) (lastSeen: DateTimeOffset) (hash: string) : Store.History =
    { Groups =
        Map
            [ "*",
              { Key = "*"
                Tags = []
                State = Some Alert
                Since = since
                FirstTriggeredAt = Some since
                LastTriggeredAt = Some since
                FirstSeen = since
                LastSeen = lastSeen
                LastPoint = lastSeen
                Interrupted = None } ]
      QueryHash = Some hash }

let private hashOf (query: string) =
    match Parse.query query with
    | Ok q -> State.queryHash q
    | Error problems -> failwith (String.concat "; " problems)

/// One insert attempt, and whether the fake stored it.
type Attempt =
    { Rows: MonitorTransitionRow[]
      Stored: bool }

/// What the services answer; a test changes it between ticks.
type Fakes() =
    member val Row = Some(monitorRow 1 cpu critical90) with get, set
    member val History = Store.empty with get, set
    member val Value = 95.0 with get, set
    member val Clock = t0 with get, set
    /// Inserts that fail before one is stored.
    member val FailingInserts = 0 with get, set
    member val ExecuteFails = false with get, set
    /// Execute and the definition read wait for these.
    member val ExecuteGate: Task = Task.CompletedTask with get, set
    member val DefinitionGate: Task = Task.CompletedTask with get, set
    member val Executions = 0 with get, set
    member val Reads = 0 with get, set
    member val Resync = TimeSpan.FromHours 1.0 with get, set
    member val Counts = Telemetry.Counters()

/// Answers the engine's SQL with one point per bucket of the window read, all
/// of `value`, for a query without groups.
let rowsFor (sql: Sql) (value: float) : Compile.Row list =
    let number (name: string) =
        sql.Parameters
        |> List.pick (fun (n, v) ->
            match v with
            | Int64 x when Regex.IsMatch(n, $"^{name}\\d+$") -> Some x
            | _ -> None)

    let fromMs, toMs, stepMs = number "from", number "to", number "step" * 1000L

    [ for bucket in fromMs / stepMs * stepMs .. stepMs .. toMs - 1L do
          if bucket >= fromMs then
              ({ Groups = []
                 SeriesId = Some 1UL
                 BucketMs = bucket
                 Value = value }
              : Compile.Row) ]

let private servicesOf (fakes: Fakes) (gate: SemaphoreSlim) (inserts: IActorRef) (records: IActorRef) : Services =
    { Definition =
        fun _ _ ->
            task {
                lock fakes (fun () -> fakes.Reads <- fakes.Reads + 1)
                do! fakes.DefinitionGate
                return fakes.Row
            }
      History = fun _ _ _ -> Task.FromResult fakes.History
      Execute =
        fun _ sql ->
            task {
                lock fakes (fun () -> fakes.Executions <- fakes.Executions + 1)
                do! fakes.ExecuteGate

                if fakes.ExecuteFails then
                    failwith "ClickHouse is down"

                return rowsFor sql fakes.Value
            }
      Insert =
        fun rows _ ->
            let fails = lock fakes (fun () -> fakes.FailingInserts > 0)

            if fails then
                lock fakes (fun () -> fakes.FailingInserts <- fakes.FailingInserts - 1)
                inserts.Tell { Rows = rows; Stored = false }
                Task.FromException(TimeoutException "the insert timed out")
            else
                inserts.Tell { Rows = rows; Stored = true }
                Task.CompletedTask
      Record = fun rows -> records.Tell rows
      Gate = gate
      GateWait = fun _ -> TimeSpan.FromMilliseconds 200.0
      Clock = fun () -> fakes.Clock
      Retention = Plan.defaultRetention
      MaxGroups = 1000
      Log = NullLogger.Instance
      Retry = TimeSpan.FromMilliseconds 200.0
      Resync = fakes.Resync
      Counts = fakes.Counts }

let private single (attempt: Attempt) : MonitorTransitionRow =
    Assert.Single attempt.Rows

type EntityTests() =
    inherit Akka.TestKit.Xunit.TestKit()

    let fakes = Fakes()
    let gate = new SemaphoreSlim(8, 8)

    member this.Start(?slots: SemaphoreSlim) : IActorRef * TestProbe * TestProbe =
        let inserts = this.CreateTestProbe "inserts"
        let records = this.CreateTestProbe "records"
        let entity = this.Sys.ActorOf(MonitorEntity.Props(key, servicesOf fakes (defaultArg slots gate) inserts.Ref records.Ref))
        entity, inserts, records

    [<Fact>]
    member this.``a tick evaluates and stores the new group's transition, and self-monitoring counts it``() =
        fakes.Clock <- t0.AddSeconds 2.0
        let entity, inserts, records = this.Start()
        this.AwaitAssert(fun () -> Assert.Equal(1.0, fakes.Counts.Entities.Value))
        entity.Tell(Tick t0)

        let row = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal("acme", row.TenantID)
        Assert.Equal(7UL, row.MonitorID)
        Assert.Equal(1u, row.MonitorVersion)
        Assert.Equal(hashOf cpu, row.QueryHash)
        Assert.Equal("*", row.GroupKey)
        Assert.Equal(t0.UtcDateTime, row.At)
        Assert.Equal(("none", "Alert", "new", "critical"), (row.FromState, row.ToState, row.Reason, row.Crossed))
        Assert.Equal(Some 95.0, row.Value)
        Assert.Equal((">", Some 90.0, None), (row.Comparator, row.Critical, row.Warning))
        Assert.Equal(t0.UtcDateTime.AddSeconds -30.0, row.WindowTo)
        Assert.Equal(None, row.Actor)

        let evaluated = records.ExpectMsg<MonitorEvaluationRow[]>(wait)
        let only = Assert.Single evaluated
        Assert.Equal(("*", Some 95.0, "Alert", "evaluated"), (only.GroupKey, only.Value, only.State, only.Outcome))

        let counts = fakes.Counts
        Assert.Equal((1.0, 0.0, 1.0, 0.0), (counts.Evaluated.Value, counts.Errored.Value, counts.TransitionsWritten.Value, counts.TransitionsPending.Value))
        // The window ends 30 s before the tick.
        Assert.Equal(32.0, counts.EvaluationLag.Take())
        Assert.True(Double.IsNaN(counts.EvaluationLag.Take()))

        this.Sys.Stop entity
        this.AwaitAssert(fun () -> Assert.Equal(0.0, counts.Entities.Value))

    [<Fact>]
    member this.``a group moves only when its state changes``() =
        let entity, inserts, _ = this.Start()
        entity.Tell(Tick t0)
        inserts.ExpectMsg<Attempt>(wait) |> ignore

        entity.Tell(Tick(t0 + minute))
        inserts.ExpectNoMsg quiet

        fakes.Value <- 10.0
        entity.Tell(Tick(t0 + minute * 2.0))
        let row = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("Alert", "OK", "recovery"), (row.FromState, row.ToState, row.Reason))
        Assert.Equal(Some t0.UtcDateTime, row.FirstTriggeredAt)

    [<Fact>]
    member this.``a failed insert is retried as the same rows before anything new is evaluated``() =
        fakes.FailingInserts <- 1
        let entity, inserts, records = this.Start()
        entity.Tell(Tick t0)
        let failed = inserts.ExpectMsg<Attempt>(wait)
        Assert.False failed.Stored
        // Not stored, so its evaluation is not recorded either.
        records.ExpectNoMsg quiet
        let counts = fakes.Counts
        this.AwaitAssert(fun () -> Assert.Equal((1.0, 1.0, 0.0), (counts.TransitionsFailed.Value, counts.TransitionsPending.Value, counts.TransitionsWritten.Value)))

        entity.Tell(Tick(t0 + minute))
        let retried = inserts.ExpectMsg<Attempt>(wait)
        Assert.True retried.Stored
        Assert.True((failed.Rows = retried.Rows))
        Assert.Equal(1, fakes.Executions)
        this.AwaitAssert(fun () -> Assert.Equal((1.0, 0.0, 1.0), (counts.TransitionsFailed.Value, counts.TransitionsPending.Value, counts.TransitionsWritten.Value)))
        Assert.Equal(1.0, counts.SkippedPending.Value)

        records.FishForMessage<MonitorEvaluationRow[]>(fun rows ->
            rows |> Array.exists (fun r -> r.GroupKey = "" && r.Outcome = "skipped" && r.At = (t0 + minute).UtcDateTime))
        |> ignore

        // Stored, so adopted: the same value moves nothing.
        entity.Tell(Tick(t0 + minute * 2.0))
        inserts.ExpectNoMsg quiet
        Assert.Equal(2, fakes.Executions)

    [<Fact>]
    member this.``an evaluation that judged no group counts as skipped``() =
        fakes.Value <- nan
        let entity, inserts, records = this.Start()
        entity.Tell(Tick t0)

        let row = records.ExpectMsg<MonitorEvaluationRow[]>(wait) |> Array.exactlyOne
        Assert.Equal(("*", "skipped"), (row.GroupKey, row.Outcome))
        inserts.ExpectNoMsg quiet
        Assert.Equal((0.0, 1.0), (fakes.Counts.Evaluated.Value, fakes.Counts.SkippedWindow.Value))

    [<Fact>]
    member this.``an anomaly monitor keeps its models until they are due for a refit``() =
        let robust =
            "avg(last_4h):anomalies(avg:system.cpu.user{*}, 'robust', 2, alert_window='last_15m', interval=60, seasonality='hourly') >= 1"

        fakes.Row <- Some(monitorRow 1 robust "")
        let entity, _, records = this.Start()

        let executionsAfter (tick: DateTimeOffset) =
            entity.Tell(Tick tick)
            records.FishForMessage<MonitorEvaluationRow[]>(fun rows -> rows |> Array.exists (fun r -> r.At = tick.UtcDateTime)) |> ignore
            lock fakes (fun () -> fakes.Executions)

        // The history read to fit and the window read; then the window alone
        // until the hourly season's ten minutes are up.
        Assert.Equal(2, executionsAfter t0)
        Assert.Equal(3, executionsAfter (t0 + minute))
        Assert.Equal(5, executionsAfter (t0 + minute * 10.0))

    [<Fact>]
    member this.``a tick while the evaluation runs is skipped``() =
        let release = TaskCompletionSource()
        fakes.ExecuteGate <- release.Task
        let entity, inserts, records = this.Start()
        entity.Tell(Tick t0)
        this.AwaitAssert(fun () -> Assert.Equal(1, fakes.Executions))

        entity.Tell(Tick(t0 + minute))

        let skipped =
            records.ExpectMsg<MonitorEvaluationRow[]>(wait) |> Array.exactlyOne

        Assert.Equal(("", "skipped", (t0 + minute).UtcDateTime), (skipped.GroupKey, skipped.Outcome, skipped.At))
        Assert.StartsWith("busy", skipped.Detail)

        release.SetResult()
        let row = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(t0.UtcDateTime, row.At)
        Assert.Equal(1, fakes.Executions)
        Assert.Equal(1.0, fakes.Counts.SkippedBusy.Value)

    [<Fact>]
    member this.``messages before the load are stashed``() =
        let release = TaskCompletionSource()
        fakes.DefinitionGate <- release.Task
        let entity, inserts, _ = this.Start()
        entity.Tell(Tick t0)
        inserts.ExpectNoMsg quiet

        release.SetResult()
        Assert.Equal(t0.UtcDateTime, (single (inserts.ExpectMsg<Attempt>(wait))).At)

    [<Fact>]
    member this.``loads wait for a slot of the node's gate``() =
        use closed = new SemaphoreSlim(0, 1)
        let entity, inserts, _ = this.Start closed
        entity.Tell(Tick t0)
        inserts.ExpectNoMsg quiet

        closed.Release() |> ignore
        inserts.ExpectMsg<Attempt>(wait) |> ignore

    [<Fact>]
    member this.``an evaluation that fails is recorded and changes nothing``() =
        fakes.ExecuteFails <- true
        let entity, inserts, records = this.Start()
        entity.Tell(Tick t0)

        let failed = records.ExpectMsg<MonitorEvaluationRow[]>(wait) |> Array.exactlyOne
        Assert.Equal(("", "error", "none"), (failed.GroupKey, failed.Outcome, failed.State))
        Assert.Contains("ClickHouse is down", failed.Detail)
        inserts.ExpectNoMsg quiet

    [<Fact>]
    member this.``a newer version with another query closes the groups after the running commit``() =
        let release = TaskCompletionSource()
        fakes.ExecuteGate <- release.Task
        let entity, inserts, _ = this.Start()
        entity.Tell(Tick t0)
        this.AwaitAssert(fun () -> Assert.Equal(1, fakes.Executions))

        let changed = "avg(last_5m):avg:system.cpu.system{*} > 90"
        fakes.Row <- Some { monitorRow 2 changed critical90 with UpdatedBy = Some "user-2" }
        entity.Tell { Ensure.Monitor = key.Text; Version = 2 }
        release.SetResult()

        let created = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("none", "Alert", 1u), (created.FromState, created.ToState, created.MonitorVersion))

        let closed = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("Alert", "OK", "definition_changed"), (closed.FromState, closed.ToState, closed.Reason))
        Assert.Equal((Some "user-2", 2u, hashOf changed), (closed.Actor, closed.MonitorVersion, closed.QueryHash))
        Assert.True(closed.At > created.At)
        // Outside a tick no window was judged.
        Assert.Equal(DateTime.MinValue, closed.WindowTo)

        // The closed group starts again under the new query.
        entity.Tell(Tick(t0 + minute))
        let reopened = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("none", "new", hashOf changed), (reopened.FromState, reopened.Reason, reopened.QueryHash))

    [<Fact>]
    member this.``a newer version with the same query keeps the groups``() =
        let entity, inserts, _ = this.Start()
        entity.Tell(Tick t0)
        inserts.ExpectMsg<Attempt>(wait) |> ignore

        fakes.Row <- Some(monitorRow 2 "avg(last_5m):avg:system.cpu.user{*} > 80" """{"thresholds":{"critical":80}}""")
        entity.Tell { Ensure.Monitor = key.Text; Version = 2 }
        inserts.ExpectNoMsg quiet

        // Still Alert, now judged by the new threshold.
        fakes.Value <- 85.0
        entity.Tell(Tick(t0 + minute))
        inserts.ExpectNoMsg quiet

    [<Theory; InlineData true; InlineData false>]
    member this.``a deleted monitor closes its groups and stops, on a Delete or on an Ensure of the deleting version``(viaDelete: bool) =
        let entity, inserts, _ = this.Start()
        this.Watch entity |> ignore
        entity.Tell(Tick t0)
        inserts.ExpectMsg<Attempt>(wait) |> ignore

        fakes.Row <- deletedRow 2 (t0 + minute)
        // An Ensure is what the manager sends for the panel's notification of a delete.
        if viaDelete then
            entity.Tell { Delete.Monitor = key.Text; Version = 2 }
        else
            entity.Tell { Ensure.Monitor = key.Text; Version = 2 }

        let closed = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("Alert", "OK", "deleted", Some "user-3"), (closed.FromState, closed.ToState, closed.Reason, closed.Actor))
        Assert.Equal(2u, closed.MonitorVersion)
        this.ExpectTerminated(entity, wait) |> ignore

    [<Fact>]
    member this.``a monitor missing from Postgres stops``() =
        fakes.Row <- None
        let entity, inserts, _ = this.Start()
        this.Watch entity |> ignore
        this.ExpectTerminated(entity, wait) |> ignore
        inserts.ExpectNoMsg quiet

    [<Fact>]
    member this.``a change at start comes after the tick already begun, and that tick is not evaluated``() =
        let at, fireAt = Evaluate.nextTick key.Text minute None t0
        let applied = fireAt.AddMilliseconds -200.0
        Assert.True(applied > at)
        fakes.Clock <- applied
        // Loaded under another query hash, so the groups are closed at once.
        fakes.History <- alertSince (t0.AddHours -1.0) t0 "an older query"

        let _, inserts, _ = this.Start()
        let closed = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("Alert", "OK", "definition_changed", Some "user-1"), (closed.FromState, closed.ToState, closed.Reason, closed.Actor))
        Assert.Equal(applied.UtcDateTime, closed.At)
        // The next tick is a minute on; `at` would sort before the change.
        inserts.ExpectNoMsg(TimeSpan.FromSeconds 1.0)
        Assert.Equal(0, fakes.Executions)

    [<Fact>]
    member this.``a restart under the same query hash with the same reading stores nothing``() =
        fakes.History <- alertSince (t0.AddHours -1.0) (t0 - minute) (hashOf cpu)

        let entity, inserts, records = this.Start()
        entity.Tell(Tick t0)

        let evaluated = records.ExpectMsg<MonitorEvaluationRow[]>(wait) |> Array.exactlyOne
        Assert.Equal(("*", "Alert", "evaluated"), (evaluated.GroupKey, evaluated.State, evaluated.Outcome))
        inserts.ExpectNoMsg quiet

    [<Fact>]
    member this.``a deletion during an evaluation closes the groups after its commit``() =
        let release = TaskCompletionSource()
        fakes.ExecuteGate <- release.Task
        let entity, inserts, _ = this.Start()
        this.Watch entity |> ignore
        entity.Tell(Tick t0)
        this.AwaitAssert(fun () -> Assert.Equal(1, fakes.Executions))

        fakes.Row <- deletedRow 2 t0

        entity.Tell { Delete.Monitor = key.Text; Version = 2 }
        inserts.ExpectNoMsg quiet
        release.SetResult()

        let created = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("none", "Alert", "new"), (created.FromState, created.ToState, created.Reason))
        let closed = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("Alert", "OK", "deleted"), (closed.FromState, closed.ToState, closed.Reason))
        Assert.True(closed.At > created.At)
        this.ExpectTerminated(entity, wait) |> ignore

    [<Fact>]
    member this.``a newer version waits for the pending rows to be stored``() =
        fakes.FailingInserts <- 1
        let entity, inserts, _ = this.Start()
        entity.Tell(Tick t0)
        let failed = inserts.ExpectMsg<Attempt>(wait)

        let changed = "avg(last_5m):avg:system.cpu.system{*} > 90"
        fakes.Row <- Some(monitorRow 2 changed critical90)
        entity.Tell { Ensure.Monitor = key.Text; Version = 2 }
        inserts.ExpectNoMsg quiet

        entity.Tell(Tick(t0 + minute))
        let retried = inserts.ExpectMsg<Attempt>(wait)
        Assert.True((failed.Rows = retried.Rows))

        let closed = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("Alert", "OK", "definition_changed", hashOf changed), (closed.FromState, closed.ToState, closed.Reason, closed.QueryHash))
        Assert.Equal(1, fakes.Executions)

    [<Fact>]
    member this.``a deleted monitor read first keeps the thresholds on its closing rows``() =
        fakes.Row <- deletedRow 3 t0
        fakes.History <- alertSince (t0.AddHours -1.0) t0 (hashOf cpu)

        let entity, inserts, _ = this.Start()
        this.Watch entity |> ignore
        let closed = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("deleted", ">", Some 90.0), (closed.Reason, closed.Comparator, closed.Critical))
        this.ExpectTerminated(entity, wait) |> ignore

    [<Fact>]
    member this.``a tick without a gate slot within the wait is skipped``() =
        use slots = new SemaphoreSlim(1, 1)
        let entity, inserts, records = this.Start slots
        entity.Tell(Tick t0)
        inserts.ExpectMsg<Attempt>(wait) |> ignore
        records.ExpectMsg<MonitorEvaluationRow[]>(wait) |> ignore

        Assert.True(slots.Wait 0)
        entity.Tell(Tick(t0 + minute))
        let skipped = records.ExpectMsg<MonitorEvaluationRow[]>(wait) |> Array.exactlyOne
        Assert.Equal(("", "skipped"), (skipped.GroupKey, skipped.Outcome))
        Assert.StartsWith("gate", skipped.Detail)
        Assert.Equal(1, fakes.Executions)
        slots.Release() |> ignore

    [<Fact>]
    member this.``a hand-off before the load stops at once``() =
        fakes.DefinitionGate <- TaskCompletionSource().Task
        let entity, _, _ = this.Start()
        this.Watch entity |> ignore
        entity.Tell Stop
        this.ExpectTerminated(entity, wait) |> ignore

    [<Fact>]
    member this.``a hand-off during an evaluation with nothing to store stops at once``() =
        fakes.ExecuteGate <- TaskCompletionSource().Task
        let entity, inserts, _ = this.Start()
        this.Watch entity |> ignore
        entity.Tell(Tick t0)
        this.AwaitAssert(fun () -> Assert.Equal(1, fakes.Executions))

        entity.Tell Stop
        this.ExpectTerminated(entity, wait) |> ignore
        inserts.ExpectNoMsg quiet

    [<Fact>]
    member this.``a hand-off tries the pending rows once more, then stops``() =
        fakes.FailingInserts <- 5
        let entity, inserts, _ = this.Start()
        this.Watch entity |> ignore
        entity.Tell(Tick t0)
        let failed = inserts.ExpectMsg<Attempt>(wait)

        entity.Tell Stop
        let last = inserts.ExpectMsg<Attempt>(wait)
        Assert.True((failed.Rows = last.Rows))
        this.ExpectTerminated(entity, wait) |> ignore
        inserts.ExpectNoMsg quiet

    [<Fact>]
    member this.``the timer fires at the aligned tick``() =
        let at, fireAt = Evaluate.nextTick key.Text minute None t0
        fakes.Clock <- fireAt.AddMilliseconds -200.0
        let _, inserts, _ = this.Start()
        Assert.Equal(at.UtcDateTime, (single (inserts.ExpectMsg<Attempt>(wait))).At)

    [<Fact>]
    member this.``three resyncs without Ensure read the definition again, and a lost deletion closes it``() =
        fakes.Resync <- TimeSpan.FromMilliseconds 100.0
        let entity, inserts, _ = this.Start()
        this.Watch entity |> ignore
        entity.Tell(Tick t0)
        inserts.ExpectMsg<Attempt>(wait) |> ignore

        fakes.Row <- deletedRow 2 (t0 + minute)

        let closed = single (inserts.ExpectMsg<Attempt>(wait))
        Assert.Equal(("Alert", "OK", "deleted"), (closed.FromState, closed.ToState, closed.Reason))
        this.ExpectTerminated(entity, wait) |> ignore

    [<Fact>]
    member this.``an Ensure, even of a version already held, keeps the entity from reading again``() =
        fakes.Resync <- TimeSpan.FromMilliseconds 300.0
        let entity, _, _ = this.Start()
        this.AwaitAssert(fun () -> Assert.Equal(1, fakes.Reads))

        for _ in 1..8 do
            entity.Tell { Ensure.Monitor = key.Text; Version = 1 }
            Thread.Sleep 200

        Assert.Equal(1, fakes.Reads)

        // Unheard for three resyncs, it reads again.
        this.AwaitAssert((fun () -> Assert.Equal(2, fakes.Reads)), wait)
