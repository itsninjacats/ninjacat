/// `ninjacat-monitors`: evaluates the metric monitors.
module NinjaCat.Api.Monitors.Program

open System
open System.Threading
open Akka.Actor
open Akka.Cluster.Hosting
open Akka.Hosting
open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Diagnostics.HealthChecks
open Microsoft.AspNetCore.Hosting
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.Hosting
open Microsoft.Extensions.Logging
open Npgsql
open NinjaCat.Api.Platform
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Monitors.Settings
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Entity
open NinjaCat.Api.Monitors.Manager
open NinjaCat.Api.Monitors.Notifier
open NinjaCat.Api.Monitors.Telemetry

/// ClickHouse's limit for one evaluation's query: a model's history read can
/// pass the query process's 7 s.
let private queryLimitSeconds = 30

/// How often the manager reads the monitor table again.
let private resyncInterval = TimeSpan.FromSeconds 30.0

/// How often the notifier looks for new transitions.
let private notifyInterval = TimeSpan.FromSeconds 5.0

let private servicesOf (settings: Settings) (counts: Counters) (provider: IServiceProvider) : Services =
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
      Retry = TimeSpan.FromSeconds 30.0
      Resync = resyncInterval
      Counts = counts }

let private notifierOf (counts: Counters) (provider: IServiceProvider) : NotifierServices =
    let sink = provider.GetRequiredService<ClickHouseSink>()
    let reader = provider.GetRequiredService<ClickHouse.Driver.ClickHouseClient>()

    { Cursor = cursor reader
      Due = due reader
      Write = fun rows ct -> sink.InsertAsync(MonitorNotifications.table, rows, ct)
      Clock = fun () -> DateTimeOffset.UtcNow
      Interval = notifyInterval
      Counts = counts
      Log = provider.GetRequiredService<ILoggerFactory>().CreateLogger "NinjaCat.Api.Monitors.Notifier" }

let private systemOf (provider: IServiceProvider) () = provider.GetRequiredService<ActorSystem>()

/// Kubernetes' probes: ready once the node has joined the cluster, live while
/// its actor system runs. The tags are the ones Akka.Hosting gives its checks.
let private probe (tag: string) =
    HealthCheckOptions(Predicate = fun registration -> registration.Tags.Contains tag)

let run (settings: Settings) (args: string[]) : int =
    let cfg = settings.Platform
    let counts = Counters()
    let builder = Hosting.createBuilder args

    // Health is all this process serves over HTTP.
    builder.WebHost.ConfigureKestrel(fun options -> Hosting.listen options settings.HealthAddr ignore)
    |> ignore

    // CoordinatedShutdown runs inside the host's stop; .NET's default 30 s
    // would cut a hand-off short.
    builder.Services.Configure<HostOptions>(fun (options: HostOptions) -> options.ShutdownTimeout <- TimeSpan.FromSeconds 45.0)
    |> ignore

    // The LISTEN connection only waits, so without a keepalive query a link
    // that died silently would leave it waiting for good.
    use listenDb =
        NpgsqlDataSource.Create(NpgsqlConnectionStringBuilder(Postgres.connectionString cfg.DatabaseUrl, KeepAlive = 30))

    builder.Services
        .AddNpgsqlDataSource(Postgres.connectionString cfg.DatabaseUrl)
        .AddSingleton<Config.Config>(cfg)
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
            systemName,
            fun (akka: AkkaConfigurationBuilder) (provider: IServiceProvider) ->
                let manager (registry: IReadOnlyActorRegistry) : ManagerServices =
                    { Snapshot = snapshot (provider.GetRequiredService<NpgsqlDataSource>())
                      Listen = listen listenDb channel
                      Registry = registry
                      Interval = resyncInterval
                      Log = provider.GetRequiredService<ILoggerFactory>().CreateLogger "NinjaCat.Api.Monitors.Manager" }

                akka
                    .ConfigureLoggers(fun loggers -> loggers.ClearLoggers().AddLoggerFactory() |> ignore)
                    .WithActorSystemLivenessCheck()
                    .WithAkkaClusterReadinessCheck()
                |> Cluster.configure settings.Node (servicesOf settings counts provider) manager (notifierOf counts provider)
                |> ignore
        )
        .AddSingleton<Reading>(fun provider -> clusterMembers (systemOf provider))
        .AddSingleton<Reading>(fun provider -> clusterUnreachable (systemOf provider))
        // After Akka, so stopped before it.
        .AddHostedService<SelfMonitor>()
        .AddHealthChecks()
    |> ignore

    for reading in readings counts do
        builder.Services.AddSingleton<Reading>(reading) |> ignore

    let app = builder.Build()
    app.MapHealthChecks("/health/ready", probe "ready") |> ignore
    app.MapHealthChecks("/health/live", probe "liveness") |> ignore
    app.Run()
    0

[<EntryPoint>]
let main args = run (Settings.load ()) args
