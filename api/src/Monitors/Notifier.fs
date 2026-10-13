/// The notifier, a stub: it finds the transitions that would be notified,
/// logs them and records a `stub` notification for each. Rendering messages
/// and real channels come later; the cursor and the idempotency key are
/// already the ones they will use.
module NinjaCat.Api.Monitors.Notifier

open System
open System.Data.Common
open System.Threading
open System.Threading.Tasks
open Akka.Actor
open ClickHouse.Driver
open Microsoft.Extensions.Logging
open NinjaCat.Api.Engine
open NinjaCat.Api.Platform
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Monitors.Telemetry

/// A transition to notify.
type Due =
    { Tenant: string
      MonitorId: uint64
      GroupKey: string
      At: DateTimeOffset
      InsertedAt: DateTimeOffset
      FromState: string
      ToState: string
      Reason: string
      Value: float option }

/// What one poll read.
type Polled =
    { /// Transitions to notify, oldest first, those already notified left out.
      Found: Due list
      /// The newest `inserted_at` of any transition read, notified or not, so
      /// the cursor also moves through transitions that are never notified.
      Newest: DateTimeOffset option }

/// The most transitions one poll handles; a full batch is followed by another
/// poll at once.
let batch = 1000

/// What the notifier reaches outside itself. The host passes ClickHouse;
/// tests pass functions over lists.
type NotifierServices =
    { /// The insert time of the newest transition a notification was stored
      /// for; None before the first.
      Cursor: CancellationToken -> Task<DateTimeOffset option>
      /// The transitions inserted after the time given.
      Due: DateTimeOffset -> CancellationToken -> Task<Polled>
      /// Stores the notifications, and fails if they were not stored.
      Write: MonitorNotificationRow[] -> CancellationToken -> Task
      Clock: unit -> DateTimeOffset
      /// How often it polls.
      Interval: TimeSpan
      Counts: Counters
      Log: ILogger }

// --- ClickHouse --------------------------------------------------------------------------

let private time (r: DbDataReader) (i: int) : DateTimeOffset =
    DateTimeOffset(DateTime.SpecifyKind(r.GetDateTime i, DateTimeKind.Utc))

let cursor (client: ClickHouseClient) (ct: CancellationToken) : Task<DateTimeOffset option> =
    task {
        let! rows =
            ClickHouse.query
                client
                ct
                (fun r -> if r.IsDBNull 0 then None else Some(time r 0))
                { Text = "SELECT maxOrNull(transition_inserted_at) FROM monitor_notifications"
                  Parameters = [] }

        return rows |> List.tryHead |> Option.flatten
    }

/// Reads from 30 s before `since`: rows of two inserts become visible in
/// either order, whatever their `inserted_at` says. A transition is taken to
/// be stored within a day of its `at`, which lets ClickHouse skip the older
/// partitions. The reasons in State.neverNotified and a new group that is OK
/// are never notified.
///
/// The newest is read first, so every row it covers is visible to the read
/// of the due ones.
let due (client: ClickHouseClient) (since: DateTimeOffset) (ct: CancellationToken) : Task<Polled> =
    let from = since.AddSeconds -30.0

    let bounds =
        [ "from", SqlValue.Int64(from.ToUnixTimeMilliseconds())
          "earliest", SqlValue.Int64((from.AddDays -1.0).ToUnixTimeMilliseconds()) ]

    let window =
        "inserted_at > fromUnixTimestamp64Milli({from:Int64}) AND at >= fromUnixTimestamp64Milli({earliest:Int64}) "

    task {
        let! newest =
            ClickHouse.query
                client
                ct
                (fun r -> if r.IsDBNull 0 then None else Some(time r 0))
                { Text = "SELECT maxOrNull(inserted_at) FROM monitor_transitions WHERE " + window
                  Parameters = bounds }

        let! found =
            ClickHouse.query
                client
                ct
                (fun r ->
                    { Tenant = r.GetString 0
                      MonitorId = Convert.ToUInt64(r.GetValue 1)
                      GroupKey = r.GetString 2
                      At = time r 3
                      InsertedAt = time r 4
                      FromState = r.GetString 5
                      ToState = r.GetString 6
                      Reason = r.GetString 7
                      Value = if r.IsDBNull 8 then None else Some(r.GetDouble 8) })
                { Text =
                    "SELECT tenant_id, monitor_id, group_key, at, inserted_at, from_state, to_state, reason, value "
                    + "FROM monitor_transitions FINAL WHERE "
                    + window
                    + "AND NOT has({never:Array(String)}, reason) "
                    + "AND NOT (reason = 'new' AND to_state = 'OK') "
                    + "AND (tenant_id, monitor_id, group_key, at) NOT IN ("
                    + "SELECT tenant_id, monitor_id, group_key, transition_at FROM monitor_notifications "
                    + "WHERE transition_at >= fromUnixTimestamp64Milli({earliest:Int64}) AND kind = 'transition') "
                    + "ORDER BY inserted_at, tenant_id, monitor_id, group_key "
                    + $"LIMIT {batch}"
                  Parameters = ("never", SqlValue.StringArray(Monitor.State.neverNotified |> List.map _.Text)) :: bounds }

        return
            { Found = found
              Newest = newest |> List.tryHead |> Option.flatten }
    }

// --- the actor ---------------------------------------------------------------------------

let private stub (now: DateTimeOffset) (d: Due) : MonitorNotificationRow =
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
      HandledAt = now.UtcDateTime }

type private Message =
    | Poll
    | Started of DateTimeOffset
    | Handled of Polled
    | Failed of string

type MonitorNotifier(services: NotifierServices) =
    inherit UntypedActor()

    let cancel = new CancellationTokenSource()
    let mutable cursor: DateTimeOffset option = None
    let mutable polling = false
    let mutable failing = false

    member private this.Start() =
        polling <- true

        Detached.run cancel.Token this.Self (fun e -> Failed e.Message) (fun ct ->
            task {
                let! newest = services.Cursor ct
                // A notifier that never stored one starts a day back: an
                // older transition is history, not news.
                return Started(newest |> Option.defaultWith (fun () -> services.Clock().AddDays -1.0))
            })

    member private this.Notify(since: DateTimeOffset) =
        polling <- true

        Detached.run cancel.Token this.Self (fun e -> Failed e.Message) (fun ct ->
            task {
                let! polled = services.Due since ct

                if not polled.Found.IsEmpty then
                    let now = services.Clock()

                    for d in polled.Found do
                        services.Counts.NotifierLag.Observe (now - d.InsertedAt).TotalSeconds

                    do! services.Write (polled.Found |> List.map (stub now) |> Array.ofList) ct

                return Handled polled
            })

    override this.PreStart() =
        this.timers.StartPeriodicTimer("poll", Poll, services.Interval)
        this.Start()

    override _.PostStop() =
        cancel.Cancel()
        cancel.Dispose()

    override this.OnReceive(message: obj) =
        match message with
        | :? Message as m ->
            match m with
            | Poll when polling -> ()
            | Poll ->
                match cursor with
                | None -> this.Start()
                | Some since -> this.Notify since
            | Started newest ->
                polling <- false
                failing <- false
                cursor <- Some newest
            | Handled polled ->
                polling <- false
                failing <- false

                for d in polled.Found do
                    services.Log.LogInformation(
                        "Monitor {Tenant}/{Monitor} group {Group}: {From} -> {To} ({Reason}, value {Value}) at {At}",
                        d.Tenant,
                        d.MonitorId,
                        d.GroupKey,
                        d.FromState,
                        d.ToState,
                        d.Reason,
                        Option.toNullable d.Value,
                        d.At
                    )

                let handled = List.map _.InsertedAt polled.Found
                let full = polled.Found.Length >= batch

                // A full batch may have left due transitions older than the
                // newest read.
                let passed =
                    if full then handled else Option.toList polled.Newest @ handled

                cursor <- Some(List.max (cursor.Value :: passed))

                if full then
                    this.Self.Tell Poll
            | Failed error ->
                polling <- false
                services.Counts.NotifierFailed.Add 1L

                // A ClickHouse outage fails every poll; one warning is enough.
                if failing then
                    services.Log.LogDebug("Notifier: transitions still not read or not marked: {Error}", error)
                else
                    services.Log.LogWarning("Notifier: transitions not read or not marked, retrying every {Interval}: {Error}", services.Interval, error)

                failing <- true
        | _ -> this.Unhandled message

    // Set by Akka from the base constructor; see MonitorEntity.
    [<DefaultValue>]
    val mutable private timers: ITimerScheduler

    interface IWithTimers with
        member this.Timers
            with get () = this.timers
            and set value = this.timers <- value

    static member Props(services: NotifierServices) : Props =
        Props.Create(typeof<MonitorNotifier>, [| box services |])
