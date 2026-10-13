/// `ninjacat-monitors`: evaluates the metric monitors.
module NinjaCat.Api.Monitors.Program

open System
open System.Threading
open Akka.Actor
open Akka.Cluster.Hosting
open Akka.Hosting
open Akka.Remote.Hosting
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.Hosting
open Microsoft.Extensions.Logging
open Npgsql
open NinjaCat.Api.Platform
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Entity

/// ClickHouse's limit for one evaluation's query: a model's history read can
/// pass the query process's 7 s.
let private queryLimitSeconds = 30

type private Live =
    | Live of MonitorRow list
    | LiveFailed of string
    | ReadAgain

/// One entity per live monitor, started from one read of the table.
///
/// It stands in for the manager and the shard region, which keep the
/// entities in step with Postgres (LISTEN and a resync); until they exist a
/// monitor created after the start waits for the next one.
type LocalEntities(db: NpgsqlDataSource, services: Services) =
    inherit UntypedActor()

    member private this.ReadLive() =
        let read =
            task {
                try
                    let! rows = live db CancellationToken.None
                    return Live rows
                with e ->
                    return LiveFailed e.Message
            }

        read.PipeTo(this.Self) |> ignore

    member private this.Route(monitor: string, message: obj) =
        match MonitorKey.Parse monitor with
        | None -> services.Log.LogWarning("Not a monitor key: {Monitor}", monitor)
        | Some key ->
            let name = Uri.EscapeDataString key.Text

            let entity =
                match UntypedActor.Context.Child name with
                | child when child.IsNobody() -> UntypedActor.Context.ActorOf(MonitorEntity.Props(key, services), name)
                | child -> child

            entity.Forward message

    override this.PreStart() = this.ReadLive()

    override this.OnReceive(message: obj) =
        match message with
        | :? Live as result ->
            match result with
            | Live rows ->
                services.Log.LogInformation("Starting {Count} monitors", rows.Length)

                for r in rows do
                    this.Route(r.Key.Text, { Ensure.Monitor = r.Key.Text; Version = r.Version })
            | LiveFailed error ->
                services.Log.LogWarning("Monitors could not be read, retrying in 30 s: {Error}", error)
                this.timers.StartSingleTimer("live", ReadAgain, TimeSpan.FromSeconds 30.0)
            | ReadAgain -> this.ReadLive()
        | :? Ensure as ensure -> this.Route(ensure.Monitor, ensure)
        | :? Delete as delete -> this.Route(delete.Monitor, delete)
        | _ -> this.Unhandled message

    // Set by Akka from the base constructor; see MonitorEntity.
    [<DefaultValue>]
    val mutable private timers: ITimerScheduler

    interface IWithTimers with
        member this.Timers
            with get () = this.timers
            and set value = this.timers <- value

let private servicesOf (settings: Settings.Settings) (provider: IServiceProvider) : Services =
    let db = provider.GetRequiredService<NpgsqlDataSource>()
    let sink = provider.GetRequiredService<ClickHouseSink>()
    let reader = provider.GetRequiredService<ClickHouse.Driver.ClickHouseClient>()

    { Definition = byKey db
      History = Store.history reader
      Execute = fun ct sql -> ClickHouse.metricRows reader ct sql
      Insert = fun rows ct -> sink.InsertAsync(MonitorTransitions.table, rows, ct)
      Record = fun rows -> Sink.write (sink :> ISink) MonitorEvaluations.table rows
      Gate = provider.GetRequiredService<SemaphoreSlim>()
      GateWait = id
      Clock = fun () -> DateTimeOffset.UtcNow
      Retention = settings.Platform.MetricsRetention
      MaxGroups = settings.MaxGroups
      Log = provider.GetRequiredService<ILoggerFactory>().CreateLogger "NinjaCat.Api.Monitors"
      Retry = TimeSpan.FromSeconds 30.0 }

let run (settings: Settings.Settings) (args: string[]) : int =
    let cfg = settings.Platform
    let builder = Host.CreateApplicationBuilder(args)
    builder.Logging.AddFilter("Npgsql", LogLevel.Warning) |> ignore

    // CoordinatedShutdown runs inside the host's stop; .NET's default 30 s
    // would cut a hand-off short.
    builder.Services.Configure<HostOptions>(fun (options: HostOptions) -> options.ShutdownTimeout <- TimeSpan.FromSeconds 45.0)
    |> ignore

    let self = $"akka.tcp://ninjacat-monitors@{settings.Host}:{settings.Port}"

    builder.Services
        .AddNpgsqlDataSource(Postgres.connectionString cfg.DatabaseUrl)
        .AddSingleton<ClickHouse.Driver.ClickHouseClient>(fun _ -> ClickHouse.createReadClient cfg queryLimitSeconds)
        .AddSingleton<ClickHouseSink>(fun services ->
            ClickHouseSink(
                ClickHouse.createWriteClient cfg,
                services.GetRequiredService<ILoggerFactory>().CreateLogger "NinjaCat.Api.Storage"
            ))
        .AddSingleton<SemaphoreSlim>(new SemaphoreSlim(settings.Concurrency, settings.Concurrency))
        // Registered before Akka, so stopped after it: the entities' last
        // evaluations are flushed.
        .AddHostedService<Hosting.SinkLifetime>()
        .AddAkka(
            "ninjacat-monitors",
            fun (akka: AkkaConfigurationBuilder) (provider: IServiceProvider) ->
                akka
                    .ConfigureLoggers(fun loggers -> loggers.ClearLoggers().AddLoggerFactory() |> ignore)
                    .WithRemoting(RemoteOptions(HostName = "0.0.0.0", PublicHostName = settings.Host, Port = Nullable settings.Port))
                    // One node that seeds itself; a cluster forms through
                    // discovery instead.
                    .WithClustering(ClusterOptions(SeedNodes = [| self |], Roles = [| "metrics"; "notifier" |]))
                    .WithActors(fun (system: ActorSystem) (registry: IActorRegistry) ->
                        let services = servicesOf settings provider
                        let db = provider.GetRequiredService<NpgsqlDataSource>()
                        let entities = system.ActorOf(Props.Create(typeof<LocalEntities>, [| box db; box services |]), "monitors")
                        registry.Register<LocalEntities>(entities))
                |> ignore
        )
    |> ignore

    builder.Build().Run()
    0

[<EntryPoint>]
let main args = run (Settings.load ()) args
