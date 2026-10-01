/// What both processes set up the same way.
module NinjaCat.Api.Server.Hosting

open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Hosting
open Microsoft.Extensions.Logging

/// A host that listens on `url` and nowhere else.
let createBuilder (args: string[]) (url: string) : WebApplicationBuilder =
    let builder = WebApplication.CreateBuilder(args)
    builder.WebHost.UseUrls(url) |> ignore

    // At the default level every request is four lines and every key
    // refresh one; an intake under load would bury its own warnings.
    builder.Logging
        .AddFilter("Microsoft.AspNetCore", LogLevel.Warning)
        .AddFilter("Npgsql", LogLevel.Warning)
    |> ignore

    builder
