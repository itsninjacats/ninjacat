/// The groups an entity starts from, read back from transitions and
/// evaluations stored through the real sink, in a scratch database
/// (TestClickHouse).
module NinjaCat.Api.Monitors.Tests.StoreTests

open System
open System.Threading
open ClickHouse.Driver
open Microsoft.Extensions.Logging.Abstractions
open Xunit
open NinjaCat.Api.Engine.Monitor.Evaluate
open NinjaCat.Api.Engine.Monitor.State
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Monitors
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Tests.TestClickHouse

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
        // A No Data not notified remembers what it interrupted.
        stored sink "new" [ transition "host:e" (minutesAgo 30.0) None Warn Reason.New ]
        stored sink "new" [ transition "host:e" (minutesAgo 15.0) (Some Warn) NoData Reason.NoDataShown ]

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

        Assert.Equal<string list>([ "host:a"; "host:b"; "host:d"; "host:e" ], history.Groups.Keys |> List.ofSeq)
        Assert.Equal(Some "new", history.QueryHash)

        let a = history.Groups["host:a"]
        Assert.Equal((Some Warn, minutesAgo 20.0, [ "host:a" ]), (a.State, a.Since, a.Tags))
        Assert.Equal((Some(minutesAgo 20.0), Some(minutesAgo 20.0)), (a.FirstTriggeredAt, a.LastTriggeredAt))
        Assert.Equal((minutesAgo 30.0, minutesAgo 1.0), (a.FirstSeen, a.LastSeen))

        let b = history.Groups["host:b"]
        Assert.Equal((Some OK, minutesAgo 25.0, minutesAgo 25.0), (b.State, b.Since, b.LastSeen))
        Assert.Equal(None, b.FirstTriggeredAt)

        let d = history.Groups["host:d"]
        Assert.Equal((Some NoData, None, None, None), (d.State, d.FirstTriggeredAt, d.LastTriggeredAt, d.Interrupted))

        let e = history.Groups["host:e"]
        Assert.Equal((Some NoData, Some Warn), (e.State, e.Interrupted))
        Assert.Equal(None, a.Interrupted))

[<Fact>]
let ``a monitor with nothing stored starts empty`` () =
    inScratch (fun scratch ->
        let history = (Store.history scratch key now CancellationToken.None).GetAwaiter().GetResult()
        Assert.Equal(Store.empty, history))
