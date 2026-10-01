/// Every row the golden fixtures produce, inserted into a real ClickHouse
/// through the real writer. The fixtures prove the values are the ones Go
/// produced; this proves ClickHouse takes them — that each value's .NET type
/// fits its column.
///
/// Needs ClickHouse (CLICKHOUSE_HTTP_ADDR, default localhost:8123, with the
/// usual CLICKHOUSE_USER/PASSWORD); skipped when there is none. Works in a
/// database of its own, created from the migrations and dropped afterwards.
module NinjaCat.Api.Intake.Tests.ClickHouseRoundTripTests

open System
open System.Collections.Generic
open ClickHouse.Driver
open ClickHouse.Driver.ADO
open Microsoft.Extensions.Logging
open Xunit
open NinjaCat.Api.Storage
open NinjaCat.Api.Intake.Tests.Golden

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
            Timeout = TimeSpan.FromMinutes 2.0
        )
    )

/// Remembers what the writers logged as errors.
type private ErrorLog() =
    let errors = List<string>()
    member _.Errors = List.ofSeq errors

    interface ILogger with
        member _.BeginScope<'state>(_: 'state) : IDisposable = null
        member _.IsEnabled(level: LogLevel) = level >= LogLevel.Error

        member _.Log<'state>(level: LogLevel, _: EventId, state: 'state, error: exn, format: Func<'state, exn, string>) =
            if level >= LogLevel.Error then
                let cause = if isNull error then "" else ": " + error.GetBaseException().Message
                lock errors (fun () -> errors.Add(format.Invoke(state, error) + cause))

[<Fact>]
let ``every row the fixtures produce fits its ClickHouse table`` () =
    let admin = client "default"

    try
        admin.PingAsync().GetAwaiter().GetResult() |> ignore
    with _ ->
        Assert.Skip "ClickHouse is not reachable"

    let database = "ninjacat_roundtrip_" + Guid.NewGuid().ToString "N"
    admin.ExecuteNonQueryAsync($"CREATE DATABASE {database}").GetAwaiter().GetResult() |> ignore

    try
        let scratch = client database

        match (Migrations.apply scratch).GetAwaiter().GetResult() with
        | Error e -> Assert.Fail e
        | Ok _ -> ()

        let writes = Replay.selected |> List.collect (fun fixture -> snd (Replay.send fixture))
        let log = ErrorLog()
        let sink = ClickHouseSink(scratch, log)

        for write in writes do
            // The rows are already column values; the table only has to pass them on.
            let table: Table<obj[]> = Table.create write.Writer write.Table write.Columns id
            (sink :> ISink).Write(table, write.Args)

        sink.StopAsync().GetAwaiter().GetResult()

        let expected = writes |> List.groupBy _.Table |> List.map (fun (table, group) -> table, group |> List.sumBy _.Args.Length) |> Map.ofList
        let failures = sink.Stats |> List.filter (fun stats -> stats.Failed > 0L || int stats.Written <> expected[stats.Table])

        if not failures.IsEmpty || not log.Errors.IsEmpty then
            let lines = failures |> List.map (fun s -> $"{s.Table}: {s.Written} of {expected[s.Table]} rows written, {s.Failed} failed")
            Assert.Fail(String.concat "\n" (lines @ log.Errors))

        Assert.NotEmpty writes
    finally
        admin.ExecuteNonQueryAsync($"DROP DATABASE IF EXISTS {database}").GetAwaiter().GetResult() |> ignore
