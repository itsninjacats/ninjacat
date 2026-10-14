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
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows
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

/// Runs `test` against a database of its own, dropped afterwards; with
/// `migrated`, built from the migrations. Skips when ClickHouse is not
/// reachable.
let private inScratch (migrated: bool) (test: ClickHouseClient -> unit) : unit =
    let admin = client "default"

    try
        admin.PingAsync().GetAwaiter().GetResult() |> ignore
    with _ ->
        Assert.Skip "ClickHouse is not reachable"

    let database = "ninjacat_roundtrip_" + Guid.NewGuid().ToString "N"
    admin.ExecuteNonQueryAsync($"CREATE DATABASE {database}").GetAwaiter().GetResult() |> ignore

    try
        let scratch = client database

        if migrated then
            match (Migrations.apply scratch).GetAwaiter().GetResult() with
            | Error e -> Assert.Fail e
            | Ok _ -> ()

        test scratch
    finally
        admin.ExecuteNonQueryAsync($"DROP DATABASE IF EXISTS {database}").GetAwaiter().GetResult() |> ignore

/// Runs `write` against the real sink over a scratch database built from the
/// migrations, then fails for every table that did not take all of its rows.
/// `expected` is the number of rows per table. Skips when ClickHouse is not
/// reachable.
let roundTrip (expected: Map<string, int>) (write: ISink -> unit) : unit =
    inScratch true (fun scratch ->
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
            Assert.Fail(String.concat "\n" (failures @ log.Errors)))

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
    inScratch false (fun scratch ->
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
        ))

// --- the one-minute rollup ----------------------------------------------------------------

/// Half an hour of points every 15 s, off the minute and with some missing,
/// so minutes hold different numbers of points, from three hosts: a gauge, a
/// count and two rates (one with interval 0). web-2 carries two roles, so it
/// sits in two groups.
let private rollupPoints (start: DateTime) : MetricPoint[] =
    let hosts =
        [ "web-1", Map [ "env", [| "prod" |]; "role", [| "api" |] ]
          "web-2", Map [ "env", [| "prod" |]; "role", [| "api"; "web" |] ]
          "web-3", Map [ "env", [| "dev" |]; "role", [| "web" |] ] ]

    let metrics = [ "rt.gauge", "GAUGE", 0u; "rt.count", "COUNT", 10u; "rt.rate", "RATE", 15u; "rt.rate0", "RATE", 0u ]

    [| for metric, kind, interval in metrics do
           for h, (host, tags) in List.indexed hosts do
               for i in 0..119 do
                   if (i * 13 + h) % 5 <> 0 then
                       let at = start.AddSeconds(7.0 + 15.0 * float i)
                       let value = float ((i * 37 + h * 11) % 23) + 0.25 * float h

                       { Metrics.point "default" at metric value with
                           Host = host
                           MetricType = kind
                           Interval = interval
                           Tags = tags } |]

let private writePoints (scratch: ClickHouseClient) (points: MetricPoint[]) =
    let log = ErrorLog()
    let sink = ClickHouseSink(scratch, log)
    (sink :> ISink).Write(Metrics.table, points)
    sink.StopAsync().GetAwaiter().GetResult()
    Assert.Empty log.Errors

[<Fact>]
let ``the rollup answers whole-minute steps as the raw points do`` () =
    inScratch true (fun scratch ->
        // Recent, so the tables' TTLs keep it.
        let now = DateTime.UtcNow
        let start = DateTime(now.Year, now.Month, now.Day, now.Hour, 0, 0, DateTimeKind.Utc).AddHours -2.0
        writePoints scratch (rollupPoints start)

        let window (from: DateTime) (until: DateTime) : Plan.Plan =
            { From = DateTimeOffset from
              To = DateTimeOffset until
              Step = TimeSpan.FromMinutes 1.0
              Queries = []
              Outputs = []
              Retention = Plan.defaultRetention }

        // Aligned; then a `to` inside a minute with points after it, as a
        // shifted read or a window in the past has; then `from` inside one too.
        let windows =
            [ window start (start.AddMinutes 30.0)
              window start (start.AddSeconds(17.0 * 60.0 + 37.0))
              window (start.AddSeconds(3.0 * 60.0 + 10.0)) (start.AddSeconds(17.0 * 60.0 + 37.0)) ]

        // The bucket holding an unaligned `from` is partial in both tables
        // and the engine drops it (keepFromMs); so does this.
        let rows (plan: Plan.Plan) (table: Plan.Table) (query: Plan.QueryPlan) =
            Compile.compileOn table (TenantId "default") plan query
            |> NinjaCat.Api.Platform.ClickHouse.metricRows scratch Threading.CancellationToken.None
            |> fun t -> t.GetAwaiter().GetResult()
            |> List.filter (fun r -> r.BucketMs >= Compile.keepFromMs plan query)
            |> List.sortBy (fun r -> r.Groups, r.SeriesId, r.BucketMs)

        let close (a: float) (b: float) = abs (a - b) <= 1e-9 * max 1.0 (abs a)

        let timeAggs = [ Plan.Plain Plan.Avg; Plan.Plain Plan.Sum; Plan.Plain Plan.Min; Plan.Plain Plan.Max; Plan.Plain Plan.Count; Plan.AsCount; Plan.AsRate ]

        for plan in windows do
            for metric in [ "rt.gauge"; "rt.count"; "rt.rate"; "rt.rate0" ] do
                for timeAgg in timeAggs do
                    for step in [ 1.0; 5.0 ] do
                        for fill in [ Plan.NoFill; Plan.FillWithin(Plan.Linear, 300) ] do
                            let query: Plan.QueryPlan =
                                { Metric = metric
                                  Filter = MetricQuery.Ast.Tag("env", "prod")
                                  GroupBy = [ "role" ]
                                  SpaceAgg = Plan.Sum
                                  TimeAgg = timeAgg
                                  Step = TimeSpan.FromMinutes step
                                  Lookback = 0
                                  Fill = fill }

                            let raw = rows plan Plan.PointsTable query
                            let rollup = rows plan Plan.RollupTable query
                            let label = $"{plan.From.ToUnixTimeMilliseconds()}-{plan.To.ToUnixTimeMilliseconds()} {metric} {timeAgg} {step}m {fill}"
                            Assert.True(raw.Length > 0, label)

                            Assert.Equal<obj list>(
                                raw |> List.map (fun r -> box (r.Groups, r.SeriesId, r.BucketMs)),
                                rollup |> List.map (fun r -> box (r.Groups, r.SeriesId, r.BucketMs))
                            )

                            for r, u in List.zip raw rollup do
                                Assert.True(close r.Value u.Value, $"{label} at {r.BucketMs}: raw {r.Value}, rollup {u.Value}")

        // `last` has no query of its own yet: the stored state against the raw points.
        use reader =
            scratch
                .ExecuteReaderAsync(
                    "SELECT countIf(raw.l != rollup.l), count() FROM "
                    + "(SELECT series_id, toStartOfMinute(timestamp) AS m, argMax(value, timestamp) AS l FROM metrics GROUP BY series_id, m) AS raw "
                    + "FULL JOIN (SELECT series_id, minute AS m, argMaxMerge(value_last) AS l FROM metrics_1m GROUP BY series_id, m) AS rollup "
                    + "USING (series_id, m)"
                )
                .GetAwaiter()
                .GetResult()

        Assert.True(reader.Read())
        Assert.Equal(0UL, reader.GetFieldValue<uint64> 0)
        Assert.Equal(360UL, reader.GetFieldValue<uint64> 1))

[<Fact>]
let ``0002 fills the rollup from the raw points it finds`` () =
    inScratch false (fun scratch ->
        let migrations =
            match Migrations.load () with
            | Ok all -> all
            | Error e -> failwith e

        let run (m: Migrations.Migration) =
            for statement in Migrations.splitStatements m.Sql do
                scratch.ExecuteNonQueryAsync(statement).GetAwaiter().GetResult() |> ignore

        let now = DateTime.UtcNow
        let start = DateTime(now.Year, now.Month, now.Day, now.Hour, 0, 0, DateTimeKind.Utc).AddHours -2.0
        let points = rollupPoints start
        run migrations[0]
        writePoints scratch points[.. 499]
        run migrations[1]
        writePoints scratch points[500..]

        use reader =
            scratch.ExecuteReaderAsync("SELECT countMerge(points) FROM metrics_1m").GetAwaiter().GetResult()

        Assert.True(reader.Read())
        Assert.Equal(uint64 points.Length, reader.GetFieldValue<uint64> 0))

[<Fact>]
let ``the metrics retention is set without rewriting the tables`` () =
    inScratch true (fun scratch ->
        match (Migrations.setMetricsRetention scratch 45 500).GetAwaiter().GetResult() with
        | Error e -> Assert.Fail e
        | Ok() -> ()

        let scalar (sql: string) =
            use reader = scratch.ExecuteReaderAsync(sql).GetAwaiter().GetResult()
            if reader.Read() then string (reader.GetValue 0) else ""

        let engine table = scalar $"SELECT engine_full FROM system.tables WHERE database = currentDatabase() AND name = '{table}'"
        Assert.Contains("TTL toDateTime(timestamp) + toIntervalDay(45)", engine "metrics")
        Assert.Contains("TTL minute + toIntervalDay(500)", engine "metrics_1m")
        Assert.Equal("0", scalar "SELECT count() FROM system.mutations WHERE database = currentDatabase()"))

// --- monitors ------------------------------------------------------------------------------

[<Fact>]
let ``an awaited insert that ClickHouse refuses fails the caller`` () =
    inScratch true (fun scratch ->
        let sink = ClickHouseSink(scratch, ErrorLog())

        let row: MonitorEvaluationRow =
            { TenantID = "default"; MonitorID = 42UL; GroupKey = "*"; At = DateTime.UtcNow; Value = None; State = "OK"; Outcome = "evaluated"; Detail = "" }

        let insert () =
            sink.InsertAsync(MonitorEvaluations.table, [| row |], Threading.CancellationToken.None).GetAwaiter().GetResult()

        // The first insert caches the column types, so the second reaches
        // ClickHouse and is refused there.
        insert ()
        scratch.ExecuteNonQueryAsync("DROP TABLE monitor_evaluations").GetAwaiter().GetResult() |> ignore
        Assert.ThrowsAny<exn>(fun () -> insert ()) |> ignore)

[<Fact>]
let ``evaluations go through the sink`` () =
    let at = DateTime.UtcNow

    roundTrip (Map [ "monitor_evaluations", 3 ]) (fun sink ->
        Sink.write
            sink
            MonitorEvaluations.table
            [| { TenantID = "default"; MonitorID = 42UL; GroupKey = "*"; At = at; Value = Some 1.5; State = "OK"; Outcome = "evaluated"; Detail = "" }
               { TenantID = "default"; MonitorID = 42UL; GroupKey = "host:a"; At = at; Value = None; State = "OK"; Outcome = "skipped"; Detail = "window not full" }
               { TenantID = "default"; MonitorID = 42UL; GroupKey = "host:b"; At = at; Value = None; State = "Alert"; Outcome = "error"; Detail = "timeout" } |])
