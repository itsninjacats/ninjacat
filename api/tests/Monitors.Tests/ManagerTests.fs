/// The manager with the region replaced by a probe: Postgres by functions over
/// values the test sets, and once by the dev Postgres itself.
module NinjaCat.Api.Monitors.Tests.ManagerTests

open System
open System.Threading
open System.Threading.Tasks
open Xunit
open Akka.Actor
open Akka.Hosting
open Akka.TestKit
open Microsoft.Extensions.Logging.Abstractions
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Entity
open NinjaCat.Api.Monitors.Messages
open NinjaCat.Api.Monitors.Manager
open NinjaCat.Api.Monitors.Tests.TestPostgres

let private t0 = DateTimeOffset(2026, 10, 13, 12, 0, 0, TimeSpan.Zero)
let private wait = TimeSpan.FromSeconds 5.0
let private quiet = TimeSpan.FromMilliseconds 300.0

let private seen (tenant: string) (id: int64) (version: int) : Seen =
    { Key = { Tenant = tenant; Id = id }
      Version = version }

let private empty = { At = t0; Live = []; Deleted = [] }

/// What the fake Postgres answers, and what the manager asked of it.
type Fakes() =
    /// Snapshots handed out in turn; the last one is repeated.
    member val Snapshots: Result<Snapshot, string> list = [ Ok empty ] with get, set
    /// Every read waits for it.
    member val SnapshotGate: Task = Task.CompletedTask with get, set
    member val Since = ResizeArray<DateTimeOffset option>()
    member val Notify: (string -> unit) option = None with get, set
    /// The LISTEN connections opened; the next one fails when set.
    member val Listens = 0 with get, set
    member val ListenFails = false with get, set

let private servicesOf (fakes: Fakes) (registry: IReadOnlyActorRegistry) (interval: TimeSpan) : ManagerServices =
    { Snapshot =
        fun since _ ->
            let next =
                lock fakes (fun () ->
                    fakes.Since.Add since

                    match fakes.Snapshots with
                    | [ last ] -> last
                    | next :: rest ->
                        fakes.Snapshots <- rest
                        next
                    | [] -> Ok empty)

            task {
                do! fakes.SnapshotGate

                match next with
                | Ok snapshot -> return snapshot
                | Error error -> return raise (Exception error)
            }
      Listen =
        fun notify ct ->
            lock fakes (fun () ->
                fakes.Listens <- fakes.Listens + 1

                if fakes.ListenFails then
                    fakes.ListenFails <- false
                    Task.FromException(Exception "the connection broke")
                else
                    fakes.Notify <- Some notify
                    Task.Delay(Timeout.Infinite, ct))
      Registry = registry
      Interval = interval
      Log = NullLogger.Instance }

let private sinceAsked (fakes: Fakes) : DateTimeOffset option list = lock fakes (fun () -> List.ofSeq fakes.Since)

let private listens (fakes: Fakes) : int = lock fakes (fun () -> fakes.Listens)

type ManagerTests() =
    inherit Akka.TestKit.Xunit.TestKit()

    let fakes = Fakes()

    /// Starts the manager with the region already registered as a probe.
    member this.Start(interval: TimeSpan) : TestProbe =
        let region = this.CreateTestProbe "region"
        let registry = ActorRegistry()
        registry.Register<MonitorRegion>(region.Ref)
        this.Sys.ActorOf(MonitorManager.Props(servicesOf fakes registry interval)) |> ignore
        region

    [<Fact>]
    member this.``nothing is read before the region is registered``() =
        fakes.Snapshots <- [ Ok { empty with Live = [ seen "acme" 1L 1 ] } ]
        let region = this.CreateTestProbe "region"
        let registry = ActorRegistry()
        this.Sys.ActorOf(MonitorManager.Props(servicesOf fakes registry (TimeSpan.FromHours 1.0))) |> ignore
        Thread.Sleep quiet
        Assert.Empty(sinceAsked fakes)

        registry.Register<MonitorRegion>(region.Ref)
        Assert.Equal({ Ensure.Monitor = "acme/1"; Version = 1 }, region.ExpectMsg<Ensure>(wait))

    [<Fact>]
    member this.``a resync ensures every live monitor and deletes every recent deletion``() =
        fakes.Snapshots <-
            [ Ok
                  { At = t0
                    Live = [ seen "acme" 1L 1; seen "a/b" 2L 5 ]
                    Deleted = [ seen "acme" 3L 4 ] } ]

        let region = this.Start(TimeSpan.FromHours 1.0)
        Assert.Equal({ Ensure.Monitor = "acme/1"; Version = 1 }, region.ExpectMsg<Ensure>(wait))
        Assert.Equal({ Ensure.Monitor = "a/b/2"; Version = 5 }, region.ExpectMsg<Ensure>(wait))
        Assert.Equal({ Delete.Monitor = "acme/3"; Version = 4 }, region.ExpectMsg<Delete>(wait))
        region.ExpectNoMsg quiet
        // The first resync after a start reaches a day back.
        Assert.Equal<DateTimeOffset option list>([ None ], sinceAsked fakes)

    [<Fact>]
    member this.``each resync reads deletions from one interval before the last snapshot``() =
        let interval = TimeSpan.FromMilliseconds 200.0
        let t1 = t0.AddSeconds 30.0

        fakes.Snapshots <-
            [ Ok empty
              Error "Postgres is down"
              Ok { empty with At = t1 } ]

        this.Start interval |> ignore
        this.AwaitAssert((fun () -> Assert.True((sinceAsked fakes).Length >= 4)), wait)

        // A failed read moves nothing: the next one starts from the same place.
        Assert.Equal<DateTimeOffset option list>(
            [ None; Some(t0 - interval); Some(t0 - interval); Some(t1 - interval) ],
            sinceAsked fakes |> List.take 4
        )

    [<Fact>]
    member this.``every resync ensures the live monitors again``() =
        fakes.Snapshots <- [ Ok { empty with Live = [ seen "acme" 1L 1 ] } ]
        let region = this.Start(TimeSpan.FromMilliseconds 200.0)

        for _ in 1..3 do
            Assert.Equal({ Ensure.Monitor = "acme/1"; Version = 1 }, region.ExpectMsg<Ensure>(wait))

    [<Fact>]
    member this.``a resync does not start while the last read is still running``() =
        let release = TaskCompletionSource()
        fakes.SnapshotGate <- release.Task
        fakes.Snapshots <- [ Ok { empty with Live = [ seen "acme" 1L 1 ] } ]
        let region = this.Start(TimeSpan.FromMilliseconds 100.0)

        Thread.Sleep 500
        Assert.Equal(1, (sinceAsked fakes).Length)

        release.SetResult()
        region.ExpectMsg<Ensure>(wait) |> ignore
        this.AwaitAssert((fun () -> Assert.True((sinceAsked fakes).Length >= 2)), wait)

    [<Fact>]
    member this.``a notification ensures its version at once``() =
        let region = this.Start(TimeSpan.FromHours 1.0)
        this.AwaitAssert((fun () -> Assert.True(fakes.Notify.IsSome)), wait)

        fakes.Notify.Value "not a monitor"
        fakes.Notify.Value "acme/7/4"
        Assert.Equal({ Ensure.Monitor = "acme/7"; Version = 4 }, region.ExpectMsg<Ensure>(wait))
        region.ExpectNoMsg quiet

    [<Fact>]
    member this.``a broken LISTEN connection is opened again after one interval``() =
        fakes.ListenFails <- true
        let region = this.Start(TimeSpan.FromSeconds 1.0)
        this.AwaitAssert((fun () -> Assert.Equal(1, listens fakes)), wait)

        Thread.Sleep 400
        Assert.Equal(1, listens fakes)
        this.AwaitAssert((fun () -> Assert.Equal(2, listens fakes)), wait)

        fakes.Notify.Value "acme/7/4"
        region.FishForMessage<Ensure>((fun (e: Ensure) -> e.Monitor = "acme/7"), Nullable wait) |> ignore

    [<Fact>]
    member this.``the dev Postgres reaches the region through a resync and through LISTEN``() =
        withTenant (fun db tenant ->
            // Already deleted, so the dev stack's own manager, which reads the
            // same table, at most starts an entity that has nothing to close.
            let id = insert db tenant "avg(last_5m):avg:system.cpu.user{*} > 90" "now()"
            let key = $"{tenant}/{id}"
            // A channel of its own, so the dev stack's manager hears nothing.
            let testChannel = "monitors_test_" + Guid.NewGuid().ToString "N"
            let region = this.CreateTestProbe "region"
            let registry = ActorRegistry()
            registry.Register<MonitorRegion>(region.Ref)

            let services =
                { Snapshot = snapshot db
                  Listen = listen db testChannel
                  Registry = registry
                  Interval = TimeSpan.FromHours 1.0
                  Log = NullLogger.Instance }

            this.Sys.ActorOf(MonitorManager.Props services) |> ignore
            region.FishForMessage<Delete>((fun (d: Delete) -> d.Monitor = key && d.Version = 3), Nullable wait) |> ignore

            // The LISTEN connection opens asynchronously and a notification
            // sent before it is lost, so one is sent until one is heard.
            this.AwaitAssert(
                (fun () ->
                    execute db "SELECT pg_notify($1, $2)" [ testChannel; $"{key}/4" ] |> ignore

                    region.FishForMessage<Ensure>((fun (e: Ensure) -> e.Monitor = key && e.Version = 4), Nullable(TimeSpan.FromMilliseconds 500.0))
                    |> ignore),
                wait
            ))
