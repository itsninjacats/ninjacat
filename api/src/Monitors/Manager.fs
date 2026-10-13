/// Keeps the entities in step with Postgres: every resync tells the region
/// about each live monitor and each recent deletion, and a notification from
/// the panel tells it about one change at once.
///
/// It keeps nothing Postgres does not have, so a new owner after a hand-over
/// starts from the table alone.
module NinjaCat.Api.Monitors.Manager

open System
open System.Threading
open System.Threading.Tasks
open Akka.Actor
open Akka.Hosting
open Microsoft.Extensions.Logging
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Entity

/// The key the region of monitor entities is registered under.
type MonitorRegion = class end

/// What the manager reaches outside itself. The host passes Postgres; tests
/// pass functions over values they set.
type ManagerServices =
    { Snapshot: DateTimeOffset option -> CancellationToken -> Task<Snapshot>
      /// Hands each notification's payload to its callback until the
      /// connection fails or the token is cancelled.
      Listen: (string -> unit) -> CancellationToken -> Task
      Registry: IReadOnlyActorRegistry
      /// How often the table is read again; also how long after a broken
      /// connection LISTEN is tried again.
      Interval: TimeSpan
      Log: ILogger }

type private Message =
    | RegionFound of IActorRef
    | Resync
    | Read of Result<Snapshot, string>
    | Notified of string
    | ListenEnded of string
    | ListenAgain

type MonitorManager(services: ManagerServices) =
    inherit UntypedActor()

    let cancel = new CancellationTokenSource()
    let mutable region: IActorRef = ActorRefs.Nobody
    let mutable reading = false
    /// Postgres's clock at the last snapshot read.
    let mutable lastRead: DateTimeOffset option = None

    member private this.Listen() =
        let self = this.Self

        Detached.run cancel.Token self (fun e -> ListenEnded e.Message) (fun ct ->
            task {
                do! services.Listen (fun payload -> self.Tell(Notified payload)) ct
                return ListenEnded "the connection closed"
            })

    /// Deletions are read from one interval before the last snapshot, so a
    /// deletion committed while that one was read is not missed. The first
    /// snapshot after a start reaches a day back: the previous owner may have
    /// missed some before it handed over.
    member private this.Resync() =
        if not reading then
            reading <- true
            let since = lastRead |> Option.map (fun at -> at - services.Interval)

            Detached.run cancel.Token this.Self (fun e -> Read(Error e.Message)) (fun ct ->
                task {
                    let! snapshot = services.Snapshot since ct
                    return Read(Ok snapshot)
                })

    // The region starts asynchronously, so nothing is sent before it is
    // registered. The lookup fails only when the manager stops.
    override this.PreStart() =
        services.Registry
            .GetAsync<MonitorRegion>(cancel.Token)
            .PipeTo(this.Self, success = fun found -> box (RegionFound found))
        |> ignore

    override _.PostStop() =
        cancel.Cancel()
        cancel.Dispose()

    override this.OnReceive(message: obj) =
        match message with
        | :? Message as m ->
            match m with
            | RegionFound found ->
                region <- found
                this.Listen()
                this.Resync()
                this.timers.StartPeriodicTimer("resync", Resync, services.Interval)
            | Resync -> this.Resync()
            | Read(Ok snapshot) ->
                reading <- false
                lastRead <- Some snapshot.At

                for s in snapshot.Live do
                    region.Tell({ Ensure.Monitor = s.Key.Text; Version = s.Version }, this.Self)

                for s in snapshot.Deleted do
                    region.Tell({ Delete.Monitor = s.Key.Text; Version = s.Version }, this.Self)
            | Read(Error error) ->
                reading <- false
                services.Log.LogWarning("Monitors could not be read, retrying in {Interval}: {Error}", services.Interval, error)
            | Notified payload ->
                // The payload does not say whether it was a deletion; the
                // entity reads the row either way, and a deleted one closes.
                match parseNotification payload with
                | Some s -> region.Tell({ Ensure.Monitor = s.Key.Text; Version = s.Version }, this.Self)
                | None -> services.Log.LogWarning("Not a monitor notification: {Payload}", payload)
            | ListenEnded error ->
                services.Log.LogWarning("Not listening for monitor changes, retrying in {Interval}: {Error}", services.Interval, error)
                this.timers.StartSingleTimer("listen", ListenAgain, services.Interval)
            | ListenAgain -> this.Listen()
        | _ -> this.Unhandled message

    // Set by Akka from the base constructor; see MonitorEntity.
    [<DefaultValue>]
    val mutable private timers: ITimerScheduler

    interface IWithTimers with
        member this.Timers
            with get () = this.timers
            and set value = this.timers <- value

    static member Props(services: ManagerServices) : Props =
        Props.Create(typeof<MonitorManager>, [| box services |])
