/// What both processes set up the same way.
module NinjaCat.Api.Server.Hosting

open System.Net
open Microsoft.AspNetCore.Builder
open Microsoft.AspNetCore.Server.Kestrel.Core
open Microsoft.Extensions.Logging

let createBuilder (args: string[]) : WebApplicationBuilder =
    let builder = WebApplication.CreateBuilder(args)

    // At the default level every request is four lines, every key refresh
    // one and every refused key two; an intake under load would bury its own
    // warnings, and anyone without a key could fill the log.
    builder.Logging
        .AddFilter("Microsoft.AspNetCore", LogLevel.Warning)
        .AddFilter("NinjaCat.Api.Intake.ApiKeyHandler", LogLevel.Warning)
        .AddFilter("Npgsql", LogLevel.Warning)
    |> ignore

    builder

/// Makes Kestrel listen on an address as the settings spell it: `:8080`,
/// `*:8080` and `0.0.0.0:8080` are every interface, anything else is the one
/// named. A process listens only where this was called.
let listen (options: KestrelServerOptions) (address: string) (configure: ListenOptions -> unit) : unit =
    let separator = address.LastIndexOf ':'
    let host = address.Substring(0, separator)
    let port = int (address.Substring(separator + 1))

    if host = "" || host = "*" || host = "0.0.0.0" then
        options.ListenAnyIP(port, configure)
    else
        options.Listen(IPAddress.Parse host, port, configure)
