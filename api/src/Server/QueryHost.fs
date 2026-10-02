/// `ninjacat-api query`: the process the panel reads from. `/internal/*` for
/// the panel and Datadog's query API, on the internal port.
///
/// It only reads: its ClickHouse client cannot write, and it holds no
/// writers and no API keys. Nothing on this port asks for a key, so it
/// belongs on a network only the panel can reach.
module NinjaCat.Api.Server.QueryHost

open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Hosting
open Microsoft.Extensions.DependencyInjection
open Oxpecker

let run (cfg: Config.Config) (args: string[]) : int =
    let builder = Hosting.createBuilder args

    builder.WebHost.ConfigureKestrel(fun options -> Hosting.listen options cfg.InternalAddr ignore)
    |> ignore

    // One client per database for the process: each owns its connection
    // pool. Registered through factories so the container disposes them.
    builder.Services
        .AddSingleton<ClickHouse.Driver.ClickHouseClient>(fun _ -> ClickHouse.createClient cfg)
        .AddNpgsqlDataSource(Postgres.connectionString cfg.DatabaseUrl)
        .AddSingleton<Config.Config>(cfg)
        .AddRouting()
        .AddOxpecker()
        // Registered after AddOxpecker so it wins: responses are written
        // with the same options requests are read with.
        .AddSingleton<IJsonSerializer>(SystemTextJsonSerializer(NinjaCat.Api.Engine.Serialization.options))
    |> ignore

    let app = builder.Build()
    app.UseRouting().UseOxpecker(Handlers.endpoints @ PanelHandlers.endpoints) |> ignore
    app.Run()
    0
