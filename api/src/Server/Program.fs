module NinjaCat.Api.Server.Program

open System
open System.Net
open System.Net.Sockets
open System.Security.Cryptography.X509Certificates
open System.Threading
open System.Threading.Tasks
open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Hosting
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.Hosting
open Microsoft.Extensions.Logging
open Oxpecker
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage

/// Inserts what the writers still hold when the process stops.
type private SinkLifetime(sink: ClickHouseSink) =
    interface IHostedService with
        member _.StartAsync(_: CancellationToken) = Task.CompletedTask
        member _.StopAsync(_: CancellationToken) = sink.StopAsync()

/// The agent's TCP transport for logs, as a part of the host.
type private LogsTcpListener(deps: Deps, cfg: Config.Config, log: ILogger<LogsTcpListener>) =
    inherit BackgroundService()

    override _.ExecuteAsync(ct: CancellationToken) : Task =
        match cfg.LogsTcpAddr with
        | None -> Task.CompletedTask
        | Some address ->
            let port = int (address.Substring(address.LastIndexOf ':' + 1))

            let certificate =
                if cfg.TlsCertFile = "" && cfg.TlsKeyFile = "" then
                    None
                else
                    Some(X509Certificate2.CreateFromPemFile(cfg.TlsCertFile, cfg.TlsKeyFile))

            log.LogInformation("logs TCP listening on {Address} ({Mode})", address, (if certificate.IsSome then "TLS" else "plain"))
            TcpLogs.serve deps (new TcpListener(IPAddress.Any, port)) certificate ct

/// `ninjacat-api migrate`: apply the ClickHouse migrations and exit. For
/// deployments with several replicas, where applying at start-up would race.
let private migrate (cfg: Config.Config) : int =
    use client = ClickHouse.createWriteClient cfg

    match (Migrations.apply client).GetAwaiter().GetResult() with
    | Ok applied ->
        printfn "migrate: schema up to date (%d applied)" applied
        0
    | Error e ->
        eprintfn "migrate: %s" e
        1

[<EntryPoint>]
let main args =
    let cfg = Config.load ()

    if args |> Array.contains "migrate" then
        migrate cfg
    else
        let builder = WebApplication.CreateBuilder(args)
        builder.WebHost.UseUrls(cfg.IntakeUrl, cfg.InternalUrl) |> ignore

        // Flares and profiles are tens of megabytes; the agent decides the size.
        builder.WebHost.ConfigureKestrel(fun options -> options.Limits.MaxRequestBodySize <- Nullable())
        |> ignore

        let writeClient = ClickHouse.createWriteClient cfg

        if cfg.AutoMigrate then
            match (Migrations.apply writeClient).GetAwaiter().GetResult() with
            | Ok 0 -> ()
            | Ok applied -> printfn "storage: applied %d schema migration(s)" applied
            | Error e -> failwith e

        let store = ApiKeys.Store()

        // One client per database for the process: each owns its connection
        // pool. Registered through factories so the container disposes them.
        builder.Services
            .AddSingleton<ClickHouse.Driver.ClickHouseClient>(fun _ -> ClickHouse.createClient cfg)
            .AddNpgsqlDataSource(Postgres.connectionString cfg.DatabaseUrl)
            .AddSingleton<Config.Config>(cfg)
            .AddSingleton<ApiKeys.Store>(store)
            .AddSingleton<ClickHouseSink>(fun services ->
                ClickHouseSink(writeClient, services.GetRequiredService<ILoggerFactory>().CreateLogger "NinjaCat.Api.Storage"))
            .AddSingleton<Deps>(fun services ->
                { Store = store
                  Sink = services.GetRequiredService<ClickHouseSink>()
                  Log = services.GetRequiredService<ILoggerFactory>().CreateLogger "NinjaCat.Api.Intake"
                  AckUnknown = cfg.AckUnknown })
            // Registered first, so stopped last: the writers flush after
            // everything that feeds them has stopped.
            .AddHostedService<SinkLifetime>()
            .AddSingleton<ApiKeysKeeper>()
            .AddHostedService<ApiKeysKeeper>(fun services -> services.GetRequiredService<ApiKeysKeeper>())
            .AddHostedService<SelfMonitor>()
            .AddHostedService<LogsTcpListener>()
            .AddRouting()
            .AddOxpecker()
            // Registered after AddOxpecker so it wins: responses are written
            // with the same options requests are read with.
            .AddSingleton<IJsonSerializer>(SystemTextJsonSerializer(NinjaCat.Api.Engine.Json.options))
        |> ignore

        let app = builder.Build()
        app.Services.GetRequiredService<ApiKeysKeeper>().FetchNow()

        // Two surfaces, split by port: agents reach the intake, and only the
        // intake; the panel and the query API stay on the internal port.
        let intake = Routes.create (app.Services.GetRequiredService<Deps>())
        let intakePort = cfg.IntakePort

        app.MapWhen(
            (fun http -> http.Connection.LocalPort = intakePort),
            fun branch ->
                branch.Run(fun http ->
                    task {
                        let! body = Engine.readBody http
                        do! Engine.write http (intake http body)
                    })
        )
        |> ignore

        app.UseRouting().UseOxpecker(Handlers.endpoints @ PanelHandlers.endpoints) |> ignore
        app.Run()
        0
