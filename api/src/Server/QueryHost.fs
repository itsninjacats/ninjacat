/// `ninjacat-api query`: the process the panel reads from. `/internal/*` for
/// the panel and Datadog's query API, on the internal port.
///
/// It only reads: its ClickHouse client cannot write, and it holds no
/// writers and no API keys. Every route but the probes asks for the panel's
/// token (QueryToken), which names the tenant; the port still belongs on a
/// network only the panel can reach.
module NinjaCat.Api.Server.QueryHost

open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Hosting
open Microsoft.Extensions.DependencyInjection
open Oxpecker
open NinjaCat.Api.Platform

let addServices (cfg: Config.Config) (services: IServiceCollection) : unit =
    // One client per database for the process: each owns its connection
    // pool. Registered through factories so the container disposes them.
    services
        .AddSingleton<ClickHouse.Driver.ClickHouseClient>(fun _ -> ClickHouse.createClient cfg)
        .AddNpgsqlDataSource(Postgres.connectionString cfg.DatabaseUrl)
        .AddSingleton<Config.Config>(cfg)
        .AddRouting()
        .AddOxpecker()
        // Registered after AddOxpecker so it wins: responses are written
        // with the same options requests are read with.
        .AddSingleton<IJsonSerializer>(SystemTextJsonSerializer(NinjaCat.Api.Engine.Serialization.options))
    |> ignore

    QueryToken.addTo services cfg.QueryTokenSecret

let configure (app: IApplicationBuilder) : unit =
    let guarded =
        Handlers.endpoints @ PanelHandlers.endpoints
        |> List.map (configureEndpoint (fun endpoint -> endpoint.RequireAuthorization()))

    app.UseRouting().UseAuthentication().UseAuthorization().UseOxpecker(Handlers.probes @ guarded)
    |> ignore

let run (cfg: Config.Config) (args: string[]) : int =
    let builder = Hosting.createBuilder args

    builder.WebHost.ConfigureKestrel(fun options -> Hosting.listen options cfg.InternalAddr ignore)
    |> ignore

    addServices cfg builder.Services
    let app = builder.Build()
    configure app
    app.Run()
    0
