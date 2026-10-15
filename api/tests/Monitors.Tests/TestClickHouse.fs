/// The dev ClickHouse (CLICKHOUSE_HTTP_ADDR, default localhost:8123), for
/// tests of what the service reads and writes there. A test is skipped when
/// there is none, and works in a database of its own, built from the
/// migrations and dropped afterwards.
module NinjaCat.Api.Monitors.Tests.TestClickHouse

open System
open ClickHouse.Driver
open ClickHouse.Driver.ADO
open Xunit
open NinjaCat.Api.Storage

let private env (name: string) (fallback: string) =
    match Environment.GetEnvironmentVariable name with
    | null
    | "" -> fallback
    | value -> value

let private client (database: string) : ClickHouseClient =
    let address = (env "CLICKHOUSE_HTTP_ADDR" "localhost:8123").Split ':'

    new ClickHouseClient(
        ClickHouseClientSettings(
            Host = address[0],
            Port = UInt16.Parse address[1],
            Database = database,
            Username = env "CLICKHOUSE_USER" "ninjacat",
            Password = env "CLICKHOUSE_PASSWORD" "ninjacat",
            Timeout = TimeSpan.FromMinutes 1.0
        )
    )

let inScratch (test: ClickHouseClient -> unit) : unit =
    let admin = client "default"

    try
        admin.PingAsync().GetAwaiter().GetResult() |> ignore
    with _ ->
        Assert.Skip "ClickHouse is not reachable"

    let database = "ninjacat_monitors_" + Guid.NewGuid().ToString "N"
    admin.ExecuteNonQueryAsync($"CREATE DATABASE {database}").GetAwaiter().GetResult() |> ignore

    try
        let scratch = client database

        match (Migrations.apply scratch).GetAwaiter().GetResult() with
        | Error e -> Assert.Fail e
        | Ok _ -> ()

        test scratch
    finally
        admin.ExecuteNonQueryAsync($"DROP DATABASE IF EXISTS {database}").GetAwaiter().GetResult() |> ignore
