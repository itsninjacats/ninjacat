/// Keeps the entities in step with Postgres; it holds nothing the table does not, so a new owner starts from the table alone.
module NinjaCat.Api.Monitors.Manager

open System
open System.Threading
open System.Threading.Tasks
open Akka.Actor
open Akka.Hosting
open Microsoft.Extensions.Logging
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Messages

/// The registry key of the region.
type MonitorRegion = class end

type ManagerServices =
    { Snapshot: DateTimeOffset option -> CancellationToken -> Task<Snapshot>
      Listen: (string -> unit) -> CancellationToken -> Task
      Registry: IReadOnlyActorRegistry
      /// Also the LISTEN retry delay.
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
    let mutable lastRead: DateTimeOffset option = None

    member private this.Listen() =
        let self = this.Self

        Detached.run cancel.Token self (fun e -> ListenEnded e.Message) (fun ct ->
            task {
                do! services.Listen (fun payload -> self.Tell(Notified payload)) ct
                return ListenEnded "the connection closed"
            })

    /// From one interval before the last snapshot, so a deletion committed during that read is not
    /// missed; the first after a start reaches a day back, for what the previous owner missed.
    member private this.Resync() =
        if not reading then
            reading <- true
            let since = lastRead |> Option.map (fun at -> at - services.Interval)

            Detached.run cancel.Token this.Self (fun e -> Read(Error e.Message)) (fun ct ->
                task {
                    let! snapshot = services.Snapshot since ct
                    return Read(Ok snapshot)
                })

    /// Not while shutting down: the last node would hold a message for a handed-off shard until the phase times out.
    member private this.Send(message: obj) =
        if isNull (CoordinatedShutdown.Get(UntypedActor.Context.System).ShutdownReason) then
            region.Tell(message, this.Self)

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
                    this.Send { Ensure.Monitor = s.Key.Text; Version = s.Version }

                for s in snapshot.Deleted do
                    this.Send { Delete.Monitor = s.Key.Text; Version = s.Version }
            | Read(Error error) ->
                reading <- false
                services.Log.LogWarning("Monitors could not be read, retrying in {Interval}: {Error}", services.Interval, error)
            | Notified payload ->
                // The payload does not say whether it was a deletion; the
                // entity reads the row either way, and a deleted one closes.
                match parseNotification payload with
                | Some s -> this.Send { Ensure.Monitor = s.Key.Text; Version = s.Version }
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
