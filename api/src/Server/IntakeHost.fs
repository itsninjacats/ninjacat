/// `ninjacat-api intake`: the process agents send to. It owns every write:
/// the ClickHouse writers, the schema, and the API keys it checks.
///
/// Nothing the panel reads is served here. That lives in the `query`
/// process, on its own port, so the two can sit on different networks.
module NinjaCat.Api.Server.IntakeHost

open System
open System.Net
open System.Net.Sockets
open System.Security.Cryptography.X509Certificates
open System.Threading
open System.Threading.Tasks
open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Hosting
open Microsoft.Extensions.DependencyInjection
open Microsoft.Extensions.Hosting
open Microsoft.Extensions.Logging
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

let run (cfg: Config.Config) (args: string[]) : int =
    let builder = Hosting.createBuilder args cfg.IntakeUrl

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

    builder.Services
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
    |> ignore

    let app = builder.Build()
    app.Services.GetRequiredService<ApiKeysKeeper>().FetchNow()

    let deps = app.Services.GetRequiredService<Deps>()
    let intake = Routes.create deps

    // No routing table: the intake picks the product by the Host header.
    (app :> IApplicationBuilder).Run(fun http ->
        task {
            let started = DateTime.UtcNow
            let! body = Engine.readBody http
            let response = intake http body
            do! Engine.write http response

            if cfg.Debug then
                try
                    let decoded = Body.decompress deps.Log (http.Request.Headers.ContentEncoding.ToString()) body
                    Capture.write cfg.CaptureDir http body decoded response.Status started |> ignore
                with e ->
                    deps.Log.LogWarning("capture not written: {Error}", e.Message)
        }
        :> Task)

    app.Run()
    0
