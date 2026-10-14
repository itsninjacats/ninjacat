/// One monitor, alive: it evaluates on its ticks, moves its groups through
/// the state machine and stores the transitions before it adopts them.
///
/// No handler awaits, so ticks, Ensure and a hand-off Stop never queue
/// behind a slow query: every slow step runs detached (`Detached.run`).
module NinjaCat.Api.Monitors.Entity

open System
open System.Diagnostics
open System.Threading
open System.Threading.Tasks
open Akka.Actor
open Microsoft.Extensions.Logging
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Engine.Monitor.Definition
open NinjaCat.Api.Engine.Monitor.Evaluate
open NinjaCat.Api.Engine.Monitor.State
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Messages
open NinjaCat.Api.Monitors.Telemetry

/// The aligned tick `at`, the evaluation's identity.
type Tick = | Tick of at: DateTimeOffset

/// What an entity reaches outside itself. The host passes Postgres,
/// ClickHouse and the sink; tests pass functions over lists.
type Services =
    { Definition: MonitorKey -> CancellationToken -> Task<MonitorRow option>
      History: MonitorKey -> DateTimeOffset -> CancellationToken -> Task<Store.History>
      /// Runs the engine's SQL.
      Execute: CancellationToken -> Sql -> Task<Compile.Row list>
      /// Stores transitions, and fails if they were not stored.
      Insert: MonitorTransitionRow[] -> CancellationToken -> Task
      /// Hands evaluations to the sink; losing one is acceptable.
      Record: MonitorEvaluationRow[] -> unit
      /// The node's slots for loads and evaluations.
      Gate: SemaphoreSlim
      /// How long an evaluation waits for a slot, given its interval; the
      /// host waits the whole interval.
      GateWait: TimeSpan -> TimeSpan
      Clock: unit -> DateTimeOffset
      Retention: Plan.Retention
      MaxGroups: int
      Log: ILogger
      /// How long after a failed read of Postgres or ClickHouse it is read
      /// again.
      Retry: TimeSpan
      /// How often the manager sends Ensure. After three or four resyncs
      /// without one the entity reads its definition itself, so a lost Delete
      /// still closes it.
      Resync: TimeSpan
      Counts: Counters }

/// Fits the 10 s the region's shutdown gives a hand-off.
let private insertTimeout = TimeSpan.FromSeconds 7.0

type private Outcome =
    | Loaded of MonitorRow option * Store.History
    | LoadFailed of string
    | RetryLoad
    | Reloaded of MonitorRow option
    | ReloadFailed of string
    | RetryReload
    | Evaluated of at: DateTimeOffset * Result<Evaluation, string> * Fit option
    | GateRefused of at: DateTimeOffset
    | Committed
    | CommitFailed of string
    | Unheard

/// Transitions waiting to be stored, and the groups adopted once they are.
/// A failed insert is retried with exactly these rows. The evaluations go to
/// the sink only then, so they never show a state that was not stored.
type private Pending =
    { Rows: MonitorTransitionRow[]
      Groups: Map<string, Group>
      QueryHash: string option
      Evaluations: MonitorEvaluationRow[] }

let private problemsText (problems: string list) = String.concat "; " problems

type MonitorEntity(key: MonitorKey, services: Services) =
    inherit UntypedActor()

    let cancel = new CancellationTokenSource()

    let mutable loaded = false
    let mutable row: MonitorRow option = None
    let mutable definition: Result<Definition, string list> = Error [ "not loaded" ]
    let mutable groups: Map<string, Group> = Map.empty
    /// The hash the groups were judged under.
    let mutable queryHash: string option = None
    let mutable lastAt: DateTimeOffset option = None
    let mutable models: Fit option = None
    /// Evaluating, storing or reading the definition.
    let mutable busy = false
    let mutable pending: Pending option = None
    /// A newer version or a deletion was announced: read the row again once
    /// idle.
    let mutable reloadDue = false
    /// The monitor is gone; only its last transitions are left to store.
    let mutable closing = false
    let mutable stopping = false
    let mutable lastAttemptMade = false
    let mutable reloadFailing = false

    /// One resync's Ensure reaches every entity within milliseconds, so
    /// without a spread they would all read Postgres in the same instant once
    /// the manager falls silent.
    let unheardAfter =
        let spread = float ((key.Text.GetHashCode() &&& 0x7fffffff) % 1000) / 1000.0
        services.Resync * (3.0 + spread)

    let validDefinition () =
        match definition with
        | Ok d -> Some d
        | Error _ -> None

    let counts = services.Counts

    let record (rows: MonitorEvaluationRow[]) = if rows.Length > 0 then services.Record rows

    let recordMonitor (at: DateTimeOffset) (outcome: string) (detail: string) =
        record [| Store.monitorRow key at groups outcome detail |]

    let skipped (counter: Counter) (at: DateTimeOffset) (detail: string) =
        counter.Add 1L
        recordMonitor at "skipped" detail

    let failed (at: DateTimeOffset) (detail: string) =
        counts.Errored.Add 1L
        recordMonitor at "error" detail

    /// When a change outside a tick is stored: now, but after the last tick,
    /// so it is the group's latest row and never shares a tick's key.
    let changeAt () =
        let now = services.Clock()

        match lastAt with
        | Some last when last >= now -> last.AddMilliseconds 1.0
        | _ -> now

    let rowsOf (actor: string option) (hash: string option) (transitions: Transition list) =
        let version = row |> Option.map _.Version |> Option.defaultValue 0

        transitions
        |> List.map (Store.transitionRow key version (defaultArg hash "") (validDefinition ()) actor)
        |> Array.ofList

    member private this.Arm() =
        let frequency =
            match definition with
            | Ok d -> frequency d.Query
            | Error _ -> TimeSpan.FromMinutes 1.0

        let now = services.Clock()
        let at, fireAt = nextTick key.Text frequency lastAt now
        this.timers.StartSingleTimer("tick", Tick at, max (fireAt - now) TimeSpan.Zero)

    member private this.Heard() =
        this.timers.StartSingleTimer("unheard", Unheard, unheardAfter)

    member private this.Load() =
        let gate = services.Gate
        let now = services.Clock()

        Detached.run cancel.Token this.Self (fun e -> LoadFailed e.Message) (fun ct ->
            task {
                do! gate.WaitAsync ct

                try
                    let! found = services.Definition key ct
                    let! history = services.History key now ct
                    return Loaded(found, history)
                finally
                    gate.Release() |> ignore
            })

    member private this.Reload() =
        busy <- true
        reloadDue <- false

        let gate = services.Gate

        Detached.run cancel.Token this.Self (fun e -> ReloadFailed e.Message) (fun ct ->
            task {
                do! gate.WaitAsync ct

                try
                    let! found = services.Definition key ct
                    return Reloaded found
                finally
                    gate.Release() |> ignore
            })

    member private this.InsertPending() =
        match pending with
        | None -> ()
        | Some p ->
            busy <- true

            Detached.run cancel.Token this.Self (fun e -> CommitFailed e.Message) (fun ct ->
                task {
                    use timeout = CancellationTokenSource.CreateLinkedTokenSource ct
                    timeout.CancelAfter insertTimeout
                    do! services.Insert p.Rows timeout.Token
                    return Committed
                })

    /// Stores the transitions and adopts `next` once they are stored; with
    /// none, adopts at once.
    member private this.Commit
        (rows: MonitorTransitionRow[], next: Map<string, Group>, hash: string option, evaluations: MonitorEvaluationRow[])
        =
        if rows.Length = 0 then
            groups <- next
            queryHash <- hash
            record evaluations
        else
            counts.TransitionsPending.Add(int64 rows.Length)

            pending <-
                Some
                    { Rows = rows
                      Groups = next
                      QueryHash = hash
                      Evaluations = evaluations }

            this.InsertPending()

    /// The monitor is gone: every judged group goes to OK, not notified, and
    /// the entity stops once that is stored.
    member private this.Close(actor: string option) =
        let next, transitions = close Reason.Deleted (changeAt ()) groups
        closing <- true
        this.Commit(rowsOf actor queryHash transitions, next, queryHash, [||])

    /// A definition read from Postgres becomes the one evaluated. A changed
    /// query closes the groups judged under the old one.
    member private this.Apply(r: MonitorRow) =
        row <- Some r
        definition <- Validate.definition services.Retention (services.Clock()) r.Type r.Query r.Options

        match definition with
        | Error problems ->
            services.Log.LogWarning("Monitor {Monitor} version {Version} is not valid: {Problems}", key.Text, r.Version, problemsText problems)
        | Ok d ->
            let hash = Some(State.queryHash d.Query)

            match queryHash with
            | Some previous ->
                let at = changeAt ()
                let next, transitions = redefine previous d at groups

                // The next tick must come after the change, or a tick already
                // armed would store an older `at` under the new query.
                if not transitions.IsEmpty then
                    lastAt <- Some at

                this.Commit(rowsOf r.UpdatedBy hash transitions, next, hash, [||])
            | None -> queryHash <- hash

        // Re-arming also drops a tick of the old timer still in the mailbox,
        // and a new window may tick at another frequency.
        this.Arm()

    member private this.Read(found: MonitorRow option) =
        match found with
        | None -> this.Close None
        | Some r when r.DeletedAt.IsSome ->
            // Deleted before it was ever read: its open groups were judged by
            // this definition as far as anything says.
            if row.IsNone then
                definition <- Validate.definition services.Retention (services.Clock()) r.Type r.Query r.Options

            row <- Some r
            this.Close r.DeletedBy
        | Some r when row.IsNone || r.Version > row.Value.Version -> this.Apply r
        | Some _ -> ()

    /// What is due once nothing is running.
    member private this.Settle() =
        if busy then
            ()
        elif stopping then
            match pending with
            | Some _ when not lastAttemptMade ->
                lastAttemptMade <- true
                this.InsertPending()
            | _ -> UntypedActor.Context.Stop this.Self
        elif closing then
            if pending.IsNone then UntypedActor.Context.Stop this.Self
        elif reloadDue && pending.IsNone then
            this.Reload()

    member private this.Evaluate(d: Definition, at: DateTimeOffset) =
        busy <- true
        let gate = services.Gate
        let wait = services.GateWait(frequency d.Query)
        let previous = models

        Detached.run cancel.Token this.Self (fun e -> Evaluated(at, Error e.Message, previous)) (fun ct ->
            task {
                let! granted = gate.WaitAsync(wait, ct)

                if not granted then
                    return GateRefused at
                else
                    let started = Stopwatch.GetTimestamp()
                    let execute = services.Execute ct
                    let tenant = TenantId key.Tenant

                    try
                        match! fit execute tenant services.Retention d previous at with
                        | Error e -> return Evaluated(at, Error e, previous)
                        | Ok current ->
                            try
                                let! result = evaluate execute tenant services.Retention services.MaxGroups current d at
                                return Evaluated(at, result, current)
                            with e ->
                                return Evaluated(at, Error e.Message, current)
                    finally
                        counts.EvaluationTicks.Add(Stopwatch.GetTimestamp() - started)
                        gate.Release() |> ignore
            })

    member private this.OnTick(at: DateTimeOffset) =
        lastAt <- Some at
        this.Arm()

        if closing then
            if not busy then this.InsertPending()
        else
            match definition with
            | Error problems -> failed at ("the definition is not valid: " + problemsText problems)
            | Ok _ when busy -> skipped counts.SkippedBusy at "busy: an evaluation, its insert or a definition read is still running"
            | Ok _ when pending.IsSome ->
                skipped counts.SkippedPending at "transitions of a failed insert are stored first"
                this.InsertPending()
            | Ok d -> this.Evaluate(d, at)

    member private this.OnOutcome(outcome: Outcome) =
        match outcome with
        | Loaded(found, history) ->
            loaded <- true
            groups <- history.Groups
            queryHash <- history.QueryHash
            this.Read found
            this.Arm()
            this.Heard()
            this.stash.UnstashAll()
            this.Settle()
        | LoadFailed error ->
            services.Log.LogWarning("Monitor {Monitor} could not be loaded, retrying in {Retry}: {Error}", key.Text, services.Retry, error)
            this.timers.StartSingleTimer("load", RetryLoad, services.Retry)
        | RetryLoad -> this.Load()
        | Reloaded found ->
            busy <- false
            reloadFailing <- false
            this.Heard()
            this.Read found
            this.Settle()
        | ReloadFailed error ->
            busy <- false
            reloadDue <- true

            // With Postgres down every entity fails here; one warning each
            // is enough.
            if reloadFailing then
                services.Log.LogDebug("Monitor {Monitor}: its definition could not be read again: {Error}", key.Text, error)
            else
                services.Log.LogWarning("Monitor {Monitor}: its definition could not be read, retrying every {Retry}: {Error}", key.Text, services.Retry, error)

            reloadFailing <- true
            this.timers.StartSingleTimer("reload", RetryReload, services.Retry)
        | RetryReload -> this.Settle()
        | Evaluated(at, Ok evaluation, fitted) ->
            busy <- false
            models <- fitted

            // Groups found but none judged is a skip, as the evaluation's rows
            // say; no group found at all is an evaluation (No Data).
            let causes =
                evaluation.Groups
                |> List.choose (fun r ->
                    match r.Reading with
                    | Skipped(cause, _) -> Some cause
                    | Judged _ -> None)

            if not evaluation.Groups.IsEmpty && causes.Length = evaluation.Groups.Length then
                if List.contains Learning causes then counts.SkippedLearning.Add 1L
                elif List.contains FewGroups causes then counts.SkippedFewGroups.Add 1L
                else counts.SkippedWindow.Add 1L
            else
                counts.Evaluated.Add 1L

            counts.EvaluationLag.Observe (services.Clock() - evaluation.Period.To).TotalSeconds

            // A definition is replaced only while nothing runs, so the one
            // evaluated is still the one held.
            match validDefinition () with
            | Some d ->
                let next, transitions = step d groups evaluation at
                this.Commit(rowsOf None queryHash transitions, next, queryHash, Store.evaluationRows key at next evaluation)
            | None -> ()

            this.Settle()
        | Evaluated(at, Error error, fitted) ->
            busy <- false
            models <- fitted
            failed at error
            this.Settle()
        | GateRefused at ->
            busy <- false
            skipped counts.SkippedGate at "gate: no evaluation slot on this node within one interval"
            this.Settle()
        | Committed ->
            busy <- false

            match pending with
            | Some p ->
                counts.TransitionsWritten.Add(int64 p.Rows.Length)
                counts.TransitionsPending.Add(int64 -p.Rows.Length)
                groups <- p.Groups
                queryHash <- p.QueryHash
                pending <- None
                record p.Evaluations
            | None -> ()

            this.Settle()
        | CommitFailed error ->
            busy <- false
            pending |> Option.iter (fun p -> counts.TransitionsFailed.Add(int64 p.Rows.Length))
            services.Log.LogWarning("Monitor {Monitor}: transitions not stored, retried at the next tick: {Error}", key.Text, error)
            this.Settle()
        | Unheard ->
            reloadDue <- true
            this.Settle()

    override this.PreStart() =
        counts.Entities.Add 1L
        this.Load()

    override _.PostStop() =
        counts.Entities.Add -1L
        pending |> Option.iter (fun p -> counts.TransitionsPending.Add(int64 -p.Rows.Length))
        cancel.Cancel()
        cancel.Dispose()

    override this.OnReceive(message: obj) =
        match message with
        | :? Outcome as outcome -> this.OnOutcome outcome
        // Matched before the stash, so a hand-off never waits for a load.
        // With nothing to store, a running load, read or evaluation is
        // cancelled in PostStop; the next owner evaluates the next tick.
        | :? Stop when pending.IsNone -> UntypedActor.Context.Stop this.Self
        | _ when not loaded -> this.stash.Stash()
        | :? Tick as tick ->
            let (Tick at) = tick
            this.OnTick at
        | :? Ensure as ensure ->
            this.Heard()

            match row with
            | Some r when ensure.Version <= r.Version -> ()
            | _ ->
                reloadDue <- true
                this.Settle()
        | :? Delete ->
            reloadDue <- true
            this.Settle()
        | :? Stop ->
            stopping <- true
            this.Settle()
        | _ -> this.Unhandled message

    // Akka sets these from the base constructor, before a `let` initializer
    // would run and overwrite them; a DefaultValue field is not initialized.
    [<DefaultValue>]
    val mutable private timers: ITimerScheduler

    [<DefaultValue>]
    val mutable private stash: IStash

    interface IWithTimers with
        member this.Timers
            with get () = this.timers
            and set value = this.timers <- value

    interface IWithUnboundedStash with
        member this.Stash
            with get () = this.stash
            and set value = this.stash <- value

    static member Props(key: MonitorKey, services: Services) : Props =
        Props.Create(typeof<MonitorEntity>, [| box key; box services |])
