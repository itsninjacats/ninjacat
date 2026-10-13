/// The groups an entity starts from, read back from transitions and
/// evaluations stored through the real sink.
///
/// Needs ClickHouse (CLICKHOUSE_HTTP_ADDR, default localhost:8123); skipped
/// when there is none. Works in a database of its own, built from the
/// migrations and dropped afterwards.
module NinjaCat.Api.Monitors.Tests.StoreTests

open System
open System.Threading
open ClickHouse.Driver
open ClickHouse.Driver.ADO
open Microsoft.Extensions.Logging.Abstractions
open Xunit
open NinjaCat.Api.Engine.Monitor.Evaluate
open NinjaCat.Api.Engine.Monitor.State
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Monitors
open NinjaCat.Api.Monitors.Definitions

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

let private inScratch (test: ClickHouseClient -> unit) : unit =
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

let private key = { Tenant = "acme"; Id = 7L }
let private now = DateTimeOffset(DateTimeOffset.UtcNow.UtcDateTime.Date.AddHours 10.0, TimeSpan.Zero)
let private minutesAgo (n: float) = now.AddMinutes -n

let private transition (group: string) (at: DateTimeOffset) (from: Status option) (into: Status) (reason: Reason) : Transition =
    let triggered = if into = Alert || into = Warn then Some at else None

    { Key = group
      Tags = [ group ]
      At = at
      From = from
      To = into
      Reason = reason
      Value = Some 1.0
      Crossed = None
      Period = Some { From = at.AddMinutes -5.0; To = at }
      FirstTriggeredAt = triggered
      LastTriggeredAt = triggered }

let private stored (sink: ClickHouseSink) (hash: string) (transitions: Transition list) =
    let rows = transitions |> List.map (Store.transitionRow key 1 hash None None) |> Array.ofList
    sink.InsertAsync(MonitorTransitions.table, rows, CancellationToken.None).GetAwaiter().GetResult()

let private evaluated (group: string) (at: DateTimeOffset) : MonitorEvaluationRow =
    { TenantID = key.Tenant
      MonitorID = uint64 key.Id
      GroupKey = group
      At = at.UtcDateTime
      Value = Some 1.0
      State = "Alert"
      Outcome = "evaluated"
      Detail = "" }

[<Fact>]
let ``open groups load with their latest transition, closed ones do not`` () =
    inScratch (fun scratch ->
        let sink = ClickHouseSink(scratch, NullLogger.Instance)

        stored sink "old" [ transition "host:a" (minutesAgo 30.0) None Alert Reason.New ]
        stored sink "new" [ transition "host:a" (minutesAgo 20.0) (Some Alert) Warn Reason.Threshold ]
        // Stored twice, as a retried insert would.
        stored sink "old" [ transition "host:b" (minutesAgo 25.0) None OK Reason.New ]
        stored sink "old" [ transition "host:b" (minutesAgo 25.0) None OK Reason.New ]
        stored sink "old" [ transition "host:c" (minutesAgo 30.0) None Alert Reason.New ]
        stored sink "old" [ transition "host:c" (minutesAgo 10.0) (Some Alert) OK Reason.Expired ]
        // The latest row has no trigger times; an older one has.
        stored sink "new" [ transition "host:d" (minutesAgo 30.0) None Alert Reason.New ]
        stored sink "new" [ transition "host:d" (minutesAgo 15.0) (Some Alert) NoData Reason.NoData ]

        // Another monitor's group is not this one's.
        let other = { Store.transitionRow key 1 "x" None None (transition "host:z" now None Alert Reason.New) with MonitorID = 8UL }
        sink.InsertAsync(MonitorTransitions.table, [| other |], CancellationToken.None).GetAwaiter().GetResult()

        sink.InsertAsync(
            MonitorEvaluations.table,
            [| evaluated "host:a" (minutesAgo 2.0)
               evaluated "host:a" (minutesAgo 1.0)
               // A monitor-level row is no group's sighting.
               { evaluated "" now with Outcome = "error" } |],
            CancellationToken.None
        )
            .GetAwaiter()
            .GetResult()

        let history = (Store.history scratch key now CancellationToken.None).GetAwaiter().GetResult()

        Assert.Equal<string list>([ "host:a"; "host:b"; "host:d" ], history.Groups.Keys |> List.ofSeq)
        Assert.Equal(Some "new", history.QueryHash)

        let a = history.Groups["host:a"]
        Assert.Equal((Some Warn, minutesAgo 20.0, [ "host:a" ]), (a.State, a.Since, a.Tags))
        Assert.Equal((Some(minutesAgo 20.0), Some(minutesAgo 20.0)), (a.FirstTriggeredAt, a.LastTriggeredAt))
        Assert.Equal((minutesAgo 30.0, minutesAgo 1.0), (a.FirstSeen, a.LastSeen))

        let b = history.Groups["host:b"]
        Assert.Equal((Some OK, minutesAgo 25.0, minutesAgo 25.0), (b.State, b.Since, b.LastSeen))
        Assert.Equal(None, b.FirstTriggeredAt)

        let d = history.Groups["host:d"]
        Assert.Equal((Some NoData, None, None), (d.State, d.FirstTriggeredAt, d.LastTriggeredAt)))

[<Fact>]
let ``a monitor with nothing stored starts empty`` () =
    inScratch (fun scratch ->
        let history = (Store.history scratch key now CancellationToken.None).GetAwaiter().GetResult()
        Assert.Equal(Store.empty, history))
