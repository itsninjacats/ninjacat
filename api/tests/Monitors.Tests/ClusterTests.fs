/// Three nodes on localhost, each a host configured as the service configures
/// itself, over fakes that note on which node each monitor was read. Strict
/// serialization is on, as in production, so a message without a serializer
/// fails here.
module NinjaCat.Api.Monitors.Tests.ClusterTests

open System
open System.Collections.Concurrent
open System.Net
open System.Net.Sockets
open System.Runtime.Serialization
open System.Threading
open System.Threading.Tasks
open Xunit
open Akka.Actor
open Akka.Cluster.Sharding
open Akka.Configuration
open Akka.Hosting
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.Hosting
open Microsoft.Extensions.Logging
open Microsoft.Extensions.Logging.Abstractions
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Monitors
open NinjaCat.Api.Monitors.Settings
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Messages
open NinjaCat.Api.Monitors.Entity
open NinjaCat.Api.Monitors.Manager

let private monitors = [ for id in 1L .. 30L -> { Tenant = "acme"; Id = id } ]
let private allKeys = monitors |> List.map _.Text |> Set.ofList
let private interval = TimeSpan.FromSeconds 1.0
let private patience = TimeSpan.FromSeconds 60.0

/// Rebalancing every second instead of ten keeps the tests short.
let private faster =
    ConfigurationFactory.ParseString
        """
        akka.loglevel = WARNING
        akka.cluster.sharding.rebalance-interval = 1s
        """

let private freePort () =
    let listener = new TcpListener(IPAddress.Loopback, 0)
    listener.Start()
    let port = (listener.LocalEndpoint :?> IPEndPoint).Port
    listener.Stop()
    port

let private addressOf (port: int) = $"akka.tcp://{systemName}@localhost:{port}"

type private Unbound = { Monitor: string }

/// What every node's fakes saw: which node read the manager's snapshot, and
/// from when, and on which node the notifier started.
type Reads() =
    member val Snapshots = ConcurrentQueue<string * DateTimeOffset option>()
    /// The nodes whose notifier started.
    member val Notifiers = ConcurrentQueue<string>()

let private entityServices () : Services =
    let row (key: MonitorKey) : MonitorRow =
        { Key = key
          Version = 1
          Type = "query alert"
          Query = "avg(last_5m):avg:system.cpu.user{*} > 90"
          Options = """{"thresholds":{"critical":90}}"""
          UpdatedBy = None
          DeletedBy = None
          DeletedAt = None }

    { Definition = fun key _ -> Task.FromResult(Some(row key))
      History = fun _ _ _ -> Task.FromResult Store.empty
      Execute = fun _ _ -> Task.FromResult []
      Insert = fun _ _ -> Task.CompletedTask
      Record = ignore
      Gate = new SemaphoreSlim(8, 8)
      GateWait = id
      Clock = fun () -> DateTimeOffset.UtcNow
      Retention = Plan.defaultRetention
      MaxGroups = 1000
      Log = NullLogger.Instance
      Retry = interval
      Resync = interval
      Counts = Telemetry.Counters() }

let private managerServices
    (keys: MonitorKey list)
    (name: string)
    (reads: Reads)
    (registry: IReadOnlyActorRegistry)
    : ManagerServices =
    { Snapshot =
        fun since _ ->
            reads.Snapshots.Enqueue((name, since))

            Task.FromResult
                { At = DateTimeOffset.UtcNow
                  Live = [ for key in keys -> { Key = key; Version = 1 } ]
                  Deleted = [] }
      Listen = fun _ ct -> Task.Delay(Timeout.Infinite, ct)
      Registry = registry
      Interval = interval
      Log = NullLogger.Instance }

let private notifierServices (name: string) (reads: Reads) : Notifier.NotifierServices =
    { Cursor =
        fun _ ->
            reads.Notifiers.Enqueue name
            Task.FromResult None
      Due = fun _ _ -> Task.FromResult { Found = []; Newest = None }
      Write = fun _ _ -> Task.CompletedTask
      Clock = fun () -> DateTimeOffset.UtcNow
      Interval = interval
      Counts = Telemetry.Counters()
      Log = NullLogger.Instance }

type private Running =
    { Name: string
      Host: IHost }

    member n.System = n.Host.Services.GetRequiredService<ActorSystem>()

    member n.Region = n.Host.Services.GetRequiredService<ActorRegistry>().Get<MonitorRegion>()

    /// The monitors whose entities run on this node now.
    member n.Entities() : Task<Set<string>> =
        task {
            let! state = n.Region.Ask<CurrentShardRegionState>(GetShardRegionState.Instance, TimeSpan.FromSeconds 5.0)
            return state.Shards |> Seq.collect _.EntityIds |> Set.ofSeq
        }

let private startWith
    (keys: MonitorKey list)
    (entity: Services)
    (name: string)
    (port: int)
    (seeds: string list)
    (reads: Reads)
    : Task<Running> =
    task {
        let builder = Host.CreateApplicationBuilder()
        builder.Logging.ClearProviders() |> ignore

        builder.Services.Configure<HostOptions>(fun (options: HostOptions) -> options.ShutdownTimeout <- TimeSpan.FromSeconds 45.0)
        |> ignore

        let node: Node =
            { Host = "localhost"
              Port = port
              Formation = Seeds seeds
              ManagementPort = 8558 }

        builder.Services.AddAkka(
            systemName,
            fun (akka: AkkaConfigurationBuilder) (_: IServiceProvider) ->
                akka.AddHocon(faster, HoconAddMode.Prepend)
                |> Cluster.configure node entity (managerServices keys name reads) (notifierServices name reads)
                |> ignore
        )
        |> ignore

        let host = builder.Build()
        do! host.StartAsync()
        return { Name = name; Host = host }
    }

let private start = startWith monitors (entityServices ())

/// Polls until `check` holds; its last failure is the test's.
let private eventually (check: unit -> Task) : Task =
    task {
        let deadline = DateTime.UtcNow + patience
        let mutable passed = false

        while not passed do
            try
                do! check ()
                passed <- true
            with _ when DateTime.UtcNow < deadline ->
                do! Task.Delay 250
    }

/// Every monitor runs on exactly one of `nodes`, and on each of them some do.
let private spreadOver (nodes: Running list) () : Task =
    task {
        let! held = nodes |> List.map _.Entities() |> Task.WhenAll

        for entities in held do
            Assert.NotEmpty entities

        Assert.Equal(allKeys.Count, held |> Array.sumBy _.Count)
        Assert.Equal<Set<string>>(allKeys, Set.unionMany held)
    }

/// `node` sees `count` members Up.
let private upMembers (count: int) (node: Running) () : Task =
    task {
        let members = Akka.Cluster.Cluster.Get(node.System).State.Members
        Assert.Equal(count, members |> Seq.filter (fun m -> m.Status = Akka.Cluster.MemberStatus.Up) |> Seq.length)
    }

/// The notifier started on these nodes, in this order, and nowhere else.
let private notifiersOn (names: string list) (reads: Reads) () : Task =
    Assert.Equal<string list>(names, List.ofSeq reads.Notifiers)
    Task.CompletedTask

/// In the order given. The tests list the youngest first, so the oldest node,
/// with the coordinator and the manager, goes last and nothing moves twice.
let private stop (nodes: Running list) : Task =
    task {
        for node in nodes do
            do! node.Host.StopAsync()
            node.Host.Dispose()
    }

/// A monitor whose first insert waits for `Release`, on a clock moved so its
/// first tick fires 15 s from now, once the node runs it.
type private SlowInsert =
    { Services: Services
      Clock: unit -> DateTimeOffset
      Started: TaskCompletionSource
      Release: TaskCompletionSource
      Stored: TaskCompletionSource }

let private slowInsert (key: MonitorKey) : SlowInsert =
    let target = DateTimeOffset.UtcNow + TimeSpan.FromSeconds 15.0
    let offset = NinjaCat.Api.Engine.Monitor.Evaluate.offset key.Text
    let minute = TimeSpan.FromMinutes(1.0).Ticks
    let tick = DateTimeOffset((target - offset).UtcTicks / minute * minute + minute, TimeSpan.Zero)
    let shift = tick + offset - target
    let clock () = DateTimeOffset.UtcNow + shift

    let started = TaskCompletionSource()
    let release = TaskCompletionSource()
    let stored = TaskCompletionSource()

    { Services =
        { entityServices () with
            Execute = fun _ sql -> Task.FromResult(EntityTests.rowsFor sql 95.0)
            Insert =
                fun _ ct ->
                    task {
                        started.TrySetResult() |> ignore
                        do! release.Task.WaitAsync ct
                        stored.TrySetResult() |> ignore
                    }
            Clock = clock }
      Clock = clock
      Started = started
      Release = release
      Stored = stored }

type ClusterTests() =

    [<Fact>]
    member _.``only the monitor messages are serialized, and they survive the trip``() : Task =
        task {
            let port = freePort ()
            let! node = start "a" port [ addressOf port ] (Reads())

            try
                let serialization = node.System.Serialization

                for message in [ box { Ensure.Monitor = "a/b/7"; Version = 3 }; box { Delete.Monitor = "acme/7"; Version = 4 }; box Stop ] do
                    let serializer = serialization.FindSerializerFor message
                    Assert.IsType<MessageSerializer> serializer |> ignore
                    let manifest = Akka.Serialization.Serialization.ManifestFor(serializer, message)
                    let bytes = serializer.ToBinary message
                    Assert.Equal(message, serialization.Deserialize(bytes, serializer.Identifier, manifest))

                Assert.Throws<SerializationException>(fun () -> serialization.FindSerializerFor(box { Unbound.Monitor = "acme/7" }) |> ignore)
                |> ignore
            finally
                (stop [ node ]).Wait()
        }

    [<Fact>]
    member _.``monitors move onto nodes that join, each to exactly one``() : Task =
        task {
            let reads = Reads()
            let seed = freePort ()
            let! a = start "a" seed [ addressOf seed ] reads
            let mutable nodes = [ a ]

            try
                do! eventually (spreadOver [ a ])

                let! b = start "b" (freePort ()) [ addressOf seed ] reads
                nodes <- b :: nodes
                let! c = start "c" (freePort ()) [ addressOf seed ] reads
                nodes <- c :: nodes

                // A shard handed off starts its entities on the new node at
                // the manager's next Ensure.
                do! eventually (spreadOver [ a; b; c ])
            finally
                (stop nodes).Wait()
        }

    [<Fact>]
    member _.``a node that leaves hands over its monitors, the manager and the notifier``() : Task =
        task {
            let reads = Reads()
            let seed = freePort ()
            let! a = start "a" seed [ addressOf seed ] reads
            let! b = start "b" (freePort ()) [ addressOf seed ] reads
            let mutable nodes = [ b; a ]

            try
                // b is Up before c joins, so b is the older of the two and
                // takes the manager over from a.
                do! eventually (upMembers 2 a)
                let! c = start "c" (freePort ()) [ addressOf seed ] reads
                nodes <- [ c; b; a ]
                do! eventually (spreadOver [ a; b; c ])
                // The oldest node runs the manager and the notifier.
                Assert.All(reads.Snapshots, fun (name, _) -> Assert.Equal("a", name))
                do! eventually (notifiersOn [ "a" ] reads)

                do! stop [ a ]
                nodes <- [ c; b ]

                do! eventually (spreadOver [ b; c ])

                // The next oldest took the manager over, and its first read
                // reached a day back for the deletions it may have missed.
                Assert.Contains(("b", None), reads.Snapshots)
                Assert.DoesNotContain(reads.Snapshots, fun (name, _) -> name = "c")
                do! eventually (notifiersOn [ "a"; "b" ] reads)
            finally
                (stop nodes).Wait()
        }

    [<Fact>]
    member _.``a node that leaves stores the transitions in flight before its monitor moves``() : Task =
        task {
            let key = { Tenant = "acme"; Id = 1L }
            let slow = slowInsert key
            let reads = Reads()
            let seed = freePort ()
            let! a = startWith [ key ] slow.Services "a" seed [ addressOf seed ] reads
            let mutable nodes = [ a ]

            let runsOn (node: Running) () : Task =
                task {
                    let! held = node.Entities()
                    Assert.Contains(key.Text, held)
                }

            try
                // b joins once the monitor runs on a; one shard is never
                // rebalanced, so it stays there.
                do! eventually (runsOn a)
                let! b = startWith [ key ] { entityServices () with Clock = slow.Clock } "b" (freePort ()) [ addressOf seed ] reads
                nodes <- [ b; a ]

                do! slow.Started.Task.WaitAsync patience
                let leaving = stop [ a ]
                nodes <- [ b ]
                do! Task.Delay 2000

                // The hand-off waits for the insert; a PoisonPill would have
                // stopped the entity and cancelled it.
                Assert.False(leaving.IsCompleted)
                slow.Release.SetResult()
                do! leaving
                Assert.True(slow.Stored.Task.IsCompleted)

                do! eventually (runsOn b)
            finally
                slow.Release.TrySetResult() |> ignore
                (stop nodes).Wait()
        }

    [<Fact>]
    member _.``the last node stops without waiting out the region's shutdown phase``() : Task =
        task {
            let key = { Tenant = "acme"; Id = 1L }
            let slow = slowInsert key
            let seed = freePort ()
            let! a = startWith [ key ] slow.Services "a" seed [ addressOf seed ] (Reads())
            let mutable nodes = [ a ]

            try
                // The manager resyncs every second while the entity holds the
                // hand-off up, so it would send to a shard handed off.
                do! slow.Started.Task.WaitAsync patience
                let watch = Diagnostics.Stopwatch.StartNew()
                nodes <- []
                let stopping = stop [ a ]
                do! Task.Delay 2000
                slow.Release.SetResult()
                do! stopping

                // The phase gives the region 10 s, all of which a message
                // buffered for that shard would take.
                Assert.True(watch.Elapsed < TimeSpan.FromSeconds 8.0, $"stopped in {watch.Elapsed}")
            finally
                slow.Release.TrySetResult() |> ignore
                (stop nodes).Wait()
        }
