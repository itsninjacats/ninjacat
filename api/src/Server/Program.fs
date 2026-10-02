module NinjaCat.Api.Server.Program

open NinjaCat.Api.Storage

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

let private usage =
    [ "usage: ninjacat-api <command>"
      ""
      "  intake    the agents' intake (NINJACAT_ADDR, default :8080)"
      "  query     the panel's and the query API (NINJACAT_INTERNAL_ADDR, default :8081)"
      "  migrate   apply the ClickHouse migrations and exit" ]
    |> String.concat "\n"

/// One binary, two servers. Which one a process is, is said on the command
/// line and nowhere else: there is no mode that serves both ports.
[<EntryPoint>]
let main args =
    match List.ofArray args with
    | "intake" :: rest -> IntakeHost.run (Config.load ()) (Array.ofList rest)
    | "query" :: rest -> QueryHost.run (Config.load ()) (Array.ofList rest)
    | [ "migrate" ] -> migrate (Config.load ())
    | _ ->
        eprintfn "%s" usage
        2
