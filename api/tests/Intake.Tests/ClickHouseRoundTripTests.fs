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

/// Runs `write` against the real sink over a scratch database built from the
/// migrations, then fails for every table that did not take all of its rows.
/// `expected` is the number of rows per table. Skips when ClickHouse is not
/// reachable.
let roundTrip (expected: Map<string, int>) (write: ISink -> unit) : unit =
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

        let log = ErrorLog()
        let sink = ClickHouseSink(scratch, log)
        write sink
        sink.StopAsync().GetAwaiter().GetResult()

        let written = sink.Stats |> List.map (fun stats -> stats.Table, stats) |> Map.ofList

        let failures =
            [ for pair in expected do
                  match written.TryFind pair.Key with
                  | Some stats when stats.Failed = 0L && int stats.Written = pair.Value -> ()
                  | Some stats -> $"{pair.Key}: {stats.Written} of {pair.Value} rows written, {stats.Failed} failed"
                  | None -> $"{pair.Key}: nothing written, {pair.Value} rows expected" ]

        if not failures.IsEmpty || not log.Errors.IsEmpty then
            Assert.Fail(String.concat "\n" (failures @ log.Errors))
    finally
        admin.ExecuteNonQueryAsync($"DROP DATABASE IF EXISTS {database}").GetAwaiter().GetResult() |> ignore

[<Fact>]
let ``every row the fixtures produce fits its ClickHouse table`` () =
    let writes = Replay.selected |> List.collect (fun fixture -> snd (Replay.send fixture))
    Assert.NotEmpty writes

    let expected =
        writes |> List.groupBy _.Table |> List.map (fun (table, group) -> table, group |> List.sumBy _.Args.Length) |> Map.ofList

    roundTrip expected (fun sink ->
        for write in writes do
            // The rows are already column values; the table only has to pass them on.
            let table: Table<obj[]> = Table.create write.Writer write.Table write.Columns id
            sink.Write(table, write.Args))

[<Fact>]
let ``times ClickHouse cannot hold are brought into range, not the end of the batch`` () =
    let admin = client "default"

    try
        admin.PingAsync().GetAwaiter().GetResult() |> ignore
    with _ ->
        Assert.Skip "ClickHouse is not reachable"

    let database = "ninjacat_roundtrip_" + Guid.NewGuid().ToString "N"
    admin.ExecuteNonQueryAsync($"CREATE DATABASE {database}").GetAwaiter().GetResult() |> ignore

    try
        let scratch = client database

        scratch
            .ExecuteNonQueryAsync(
                "CREATE TABLE times (id UInt8, plain DateTime('UTC'), milli DateTime64(3, 'UTC'), nano DateTime64(9, 'UTC'), "
                + "maybe Nullable(DateTime64(3, 'UTC')), many Array(Nullable(DateTime64(3, 'UTC')))) ENGINE = MergeTree ORDER BY id"
            )
            .GetAwaiter()
            .GetResult()
        |> ignore

        let log = ErrorLog()
        let sink = ClickHouseSink(scratch, log)
        let table: Table<obj[]> = Table.create "storage_times" "times" [ "id"; "plain"; "milli"; "nano"; "maybe"; "many" ] id
        let year (y: int) = DateTime(y, 6, 1, 0, 0, 0, DateTimeKind.Utc)

        (sink :> ISink)
            .Write(
                table,
                [| // Go's zero time: the Go driver wrote the epoch.
                   [| 1uy; DateTime.MinValue; DateTime.MinValue; UnixNanos 1_790_179_838_123_456_789L; null; [| Nullable DateTime.MinValue; Nullable() |] |]
                   [| 2uy; year 9999; year 9999; UnixNanos 0L; year 2026; [| Nullable(year 2026) |] |]
                   [| 3uy; year 1950; year 1800; UnixNanos 1L; null; Array.empty<Nullable<DateTime>> |] |]
            )

        sink.StopAsync().GetAwaiter().GetResult()
        Assert.Empty log.Errors

        use reader =
            scratch
                .ExecuteReaderAsync("SELECT toString(plain), toString(milli), toString(nano), toString(maybe), toString(many) FROM times ORDER BY id")
                .GetAwaiter()
                .GetResult()

        let rows = [ while reader.Read() do [ for i in 0..4 -> reader.GetString i ] ]

        Assert.Equal<string list list>(
            [ [ "1970-01-01 00:00:00"; "1970-01-01 00:00:00.000"; "2026-09-23 16:10:38.123456789"; ""; "['1970-01-01 00:00:00.000',NULL]" ]
              [ "2105-12-31 23:59:59"; "2262-04-11 23:47:16.000"; "1970-01-01 00:00:00.000000000"; "2026-06-01 00:00:00.000"; "['2026-06-01 00:00:00.000']" ]
              [ "1970-01-01 00:00:00"; "1900-01-01 00:00:00.000"; "1970-01-01 00:00:00.000000001"; ""; "[]" ] ],
            rows
        )
    finally
        admin.ExecuteNonQueryAsync($"DROP DATABASE IF EXISTS {database}").GetAwaiter().GetResult() |> ignore
