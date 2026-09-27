module NinjaCat.Api.Server.Program

open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Hosting
open Microsoft.Extensions.DependencyInjection
open Oxpecker

[<EntryPoint>]
let main args =
    let cfg = Config.load ()
    let builder = WebApplication.CreateBuilder(args)
    builder.WebHost.UseUrls(cfg.ListenUrl) |> ignore

    // One client per database for the process: each owns its connection pool
    // and is safe to share across requests. Registered through factories so
    // the container disposes them on shutdown, which it skips for instances.
    builder.Services
        .AddSingleton<ClickHouse.Driver.ClickHouseClient>(fun _ -> ClickHouse.createClient cfg)
        .AddNpgsqlDataSource(Postgres.connectionString cfg.DatabaseUrl)
        .AddRouting()
        .AddOxpecker()
        // Registered after AddOxpecker so it wins: responses are written with
        // the same options requests are read with (snake_case, F# types).
        .AddSingleton<IJsonSerializer>(SystemTextJsonSerializer(NinjaCat.Api.Engine.Json.options))
    |> ignore

    let app = builder.Build()
    app.UseRouting().UseOxpecker(Handlers.endpoints) |> ignore
    app.Run()
    0
