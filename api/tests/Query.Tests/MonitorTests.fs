/// `/internal/monitors/*`: validation, and what the monitor service stored,
/// read for the token's tenant only.
module NinjaCat.Api.Query.Tests.MonitorTests

open System
open System.Net
open System.Net.Http
open System.Text.Json
open System.Threading
open System.Threading.Tasks
open Microsoft.Extensions.Logging.Abstractions
open Xunit
open ClickHouse.Driver
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Query.Tests.TestServer

let private json (response: HttpResponseMessage) : Task<JsonElement> =
    task {
        let! body = response.Content.ReadAsStringAsync()
        return JsonDocument.Parse(body).RootElement
    }

let private strings (e: JsonElement) = [ for item in e.EnumerateArray() -> item.GetString() ]

let private get (client: HttpClient) (path: string) : Task<HttpResponseMessage> =
    send client (Some(mint (panelToken ()))) (HttpMethod.Get, path, "")

let private validate (client: HttpClient) (body: string) : Task<JsonElement> =
    task {
        use! response = send client (Some(mint (panelToken ()))) (HttpMethod.Post, "/internal/monitors/validate", body)
        Assert.Equal(HttpStatusCode.OK, response.StatusCode)
        return! json response
    }

// --- validate --------------------------------------------------------------------------

[<Fact>]
let ``a definition that would be evaluated has no problems, and names the options not acted on`` () =
    withServer config (fun client clickhouse ->
        task {
            let! answer =
                validate
                    client
                    """{"name":"cpu","type":"query alert","query":"avg(last_5m):avg:system.cpu.user{*} by {host} > 90",
                        "options":{"thresholds":{"critical":90,"warning":80},"renotify_interval":60}}"""

            Assert.Empty(strings (answer.GetProperty "problems"))
            Assert.Contains("renotify_interval", strings (answer.GetProperty "not_acted_on"))
            Assert.Empty clickhouse.Parameters
        })

[<Fact>]
let ``every problem of a definition is listed`` () =
    withServer config (fun client _ ->
        task {
            let! answer =
                validate
                    client
                    """{"type":"query alert","query":"avg(last_5m):avg:system.cpu.user{*} > 90",
                        "options":{"thresholds":{"critical":80,"warning":95}}}"""

            let problems = strings (answer.GetProperty "problems")
            Assert.Equal(2, problems.Length)
            Assert.Contains(problems, fun p -> p.Contains "critical (80)")
            Assert.Contains(problems, fun p -> p.Contains "warning (95)")
        })

[<Theory>]
[<InlineData("not json")>]
[<InlineData("[1]")>]
[<InlineData("""{"type":"query alert"}""")>]
[<InlineData("""{"type":"query alert","query":"avg(last_5m):avg:system.cpu.user{*} > 90","options":"high"}""")>]
let ``a body that is no monitor is a problem, not an error`` (body: string) =
    withServer config (fun client _ ->
        task {
            let! answer = validate client body
            Assert.NotEmpty(strings (answer.GetProperty "problems"))
        })

// --- status ids --------------------------------------------------------------------------

[<Theory>]
[<InlineData("")>]
[<InlineData("?ids=")>]
[<InlineData("?ids=1,x")>]
[<InlineData("?ids=0")>]
let ``status without good ids is 400 and asks nothing`` (query: string) =
    withServer config (fun client clickhouse ->
        task {
            use! response = get client ("/internal/monitors/status" + query)
            Assert.Equal(HttpStatusCode.BadRequest, response.StatusCode)
            Assert.Empty clickhouse.Parameters
        })

[<Fact>]
let ``a tenant in the query string is not believed by the monitor routes`` () =
    withServer config (fun client clickhouse ->
        task {
            for path in [ "/internal/monitors/status?ids=7&tenant=default"; "/internal/monitors/7/groups?tenant=default" ] do
                clickhouse.Parameters.Clear()
                use! response = get client path
                Assert.Equal(HttpStatusCode.OK, response.StatusCode)
                Assert.Contains("acme", clickhouse.Parameters)
                Assert.DoesNotContain("default", clickhouse.Parameters)
        })

// --- what was stored, on a real ClickHouse -------------------------------------------------

let private now = DateTimeOffset.FromUnixTimeMilliseconds(DateTimeOffset.UtcNow.ToUnixTimeMilliseconds())
let private ago (minutes: float) = now.AddMinutes -minutes

let private transition (tenant: string) (monitor: uint64) (group: string) (minutes: float) (from: string) (into: string) (reason: string) : MonitorTransitionRow =
    let at = (ago minutes).UtcDateTime
    let triggered = if into = "Alert" || into = "Warn" then Some at else None

    { TenantID = tenant
      MonitorID = monitor
      MonitorVersion = 1u
      QueryHash = "hash"
      MonitorKind = "metric"
      GroupKey = group
      GroupTags = [| group |]
      At = at
      FromState = from
      ToState = into
      Reason = reason
      Actor = None
      Value = Some 1.0
      Crossed = "none"
      Comparator = ">"
      Critical = Some 90.0
      Warning = None
      CriticalRecovery = None
      WarningRecovery = None
      WindowFrom = at.AddMinutes -5.0
      WindowTo = at
      FirstTriggeredAt = triggered
      LastTriggeredAt = triggered }

let private evaluation (tenant: string) (group: string) (minutes: float) (value: float option) (outcome: string) : MonitorEvaluationRow =
    { TenantID = tenant
      MonitorID = 7UL
      GroupKey = group
      At = (ago minutes).UtcDateTime
      Value = value
      State = "Warn"
      Outcome = outcome
      Detail = (if outcome = "evaluated" then "" else "window not full") }

let private triggered (minutes: float) (row: MonitorTransitionRow) =
    { row with
        FirstTriggeredAt = Some (ago minutes).UtcDateTime
        LastTriggeredAt = Some (ago minutes).UtcDateTime }

/// Monitor 7 of acme:
/// - host:a went to Alert, then Warn (stored twice, as a retried insert would);
/// - host:b triggered and recovered;
/// - host:d triggered, recovered, then went to No Data, whose row carries no
///   trigger time;
/// - host:c triggered after all of them, then expired.
/// Monitor 8 is in Alert. Another tenant's monitor 7 is in Alert later than
/// any of them.
let private stored (client: ClickHouseClient) : Task =
    task {
        let sink = ClickHouseSink(client, NullLogger.Instance)
        let insert table rows = sink.InsertAsync(table, Array.ofList rows, CancellationToken.None)

        // The trigger began with the Alert.
        let warned =
            { transition "acme" 7UL "host:a" 20.0 "Alert" "Warn" "threshold" with FirstTriggeredAt = Some (ago 30.0).UtcDateTime }

        do!
            insert
                MonitorTransitions.table
                [ transition "acme" 7UL "host:a" 30.0 "none" "Alert" "new"
                  warned
                  transition "acme" 7UL "host:b" 28.0 "none" "Alert" "new"
                  transition "acme" 7UL "host:b" 25.0 "Alert" "OK" "recovery" |> triggered 28.0
                  transition "acme" 7UL "host:d" 35.0 "none" "Alert" "new"
                  transition "acme" 7UL "host:d" 32.0 "Alert" "OK" "recovery" |> triggered 35.0
                  { transition "acme" 7UL "host:d" 22.0 "OK" "No Data" "no_data" with LastTriggeredAt = Some (ago 35.0).UtcDateTime }
                  transition "acme" 7UL "host:c" 12.0 "none" "Alert" "new"
                  { transition "acme" 7UL "host:c" 10.0 "Alert" "OK" "expired" with WindowFrom = DateTime.UnixEpoch; WindowTo = DateTime.UnixEpoch }
                  transition "acme" 8UL "*" 15.0 "none" "Alert" "new"
                  transition "globex" 7UL "host:z" 5.0 "none" "Alert" "new" ]

        do! insert MonitorTransitions.table [ warned ]

        do!
            insert
                MonitorEvaluations.table
                [ evaluation "acme" "host:a" 2.0 (Some 42.0) "evaluated"
                  evaluation "acme" "host:a" 1.0 (Some 43.0) "evaluated"
                  evaluation "acme" "host:a" 0.5 None "skipped"
                  evaluation "acme" "" 0.25 None "error"
                  // A sum can overflow to infinity.
                  evaluation "acme" "host:d" 3.0 (Some Double.PositiveInfinity) "evaluated"
                  // Older than a group can stay open unseen.
                  evaluation "acme" "host:b" (2.0 * 24.0 * 60.0) (Some 5.0) "evaluated"
                  evaluation "globex" "host:a" 0.1 (Some 99.0) "evaluated" ]
    }

let private time (e: JsonElement) = e.GetDateTimeOffset()

let private has (name: string) (e: JsonElement) = e.TryGetProperty name |> fst

[<Fact>]
let ``status counts the open groups of each monitor asked for, for the token's tenant`` () =
    withScratchServer stored (fun client ->
        task {
            use! response = get client "/internal/monitors/status?ids=7,8&ids=9"
            Assert.Equal(HttpStatusCode.OK, response.StatusCode)
            let! answer = json response
            let monitors = [ for m in (answer.GetProperty "monitors").EnumerateArray() -> m ]
            Assert.Equal<int64 list>([ 7L; 8L; 9L ], monitors |> List.map (fun m -> (m.GetProperty "id").GetInt64()))

            let seven = monitors[0]
            Assert.Equal("Warn", (seven.GetProperty "state").GetString())
            let count (state: string) = ((seven.GetProperty "groups").GetProperty state).GetInt64()
            Assert.Equal<int64 list>([ 0L; 1L; 1L; 1L ], [ count "Alert"; count "Warn"; count "OK"; count "No Data" ])
            // host:c's Alert, closed since, not the other tenant's later one.
            Assert.Equal(ago 12.0, time (seven.GetProperty "last_triggered_at"))

            Assert.Equal("Alert", (monitors[1].GetProperty "state").GetString())
            Assert.False(has "state" monitors[2])
            Assert.False(has "last_triggered_at" monitors[2])
        })

[<Fact>]
let ``groups are the open ones, with their latest evaluated value`` () =
    withScratchServer stored (fun client ->
        task {
            use! response = get client "/internal/monitors/7/groups"
            Assert.Equal(HttpStatusCode.OK, response.StatusCode)
            let! answer = json response
            let groups = [ for g in (answer.GetProperty "groups").EnumerateArray() -> g ]
            Assert.Equal<string list>([ "host:a"; "host:b"; "host:d" ], groups |> List.map (fun g -> (g.GetProperty "group").GetString()))

            let a = groups[0]
            Assert.Equal("Warn", (a.GetProperty "state").GetString())
            Assert.Equal(ago 20.0, time (a.GetProperty "since"))
            Assert.Equal("threshold", (a.GetProperty "reason").GetString())
            Assert.Equal(ago 30.0, time (a.GetProperty "first_triggered_at"))
            Assert.Equal(43.0, (a.GetProperty "value").GetDouble())
            Assert.Equal(ago 1.0, time (a.GetProperty "evaluated_at"))
            Assert.Equal<string list>([ "host:a" ], strings (a.GetProperty "tags"))

            // Recovered: the trigger it ended is not shown, when it was is.
            let b = groups[1]
            Assert.Equal("OK", (b.GetProperty "state").GetString())
            Assert.False(has "first_triggered_at" b)
            Assert.Equal(ago 28.0, time (b.GetProperty "last_triggered_at"))
            Assert.False(has "value" b)

            // The latest row's NULL trigger time, not an older row's.
            let d = groups[2]
            Assert.Equal("No Data", (d.GetProperty "state").GetString())
            Assert.False(has "first_triggered_at" d)
            Assert.Equal(ago 35.0, time (d.GetProperty "last_triggered_at"))
            Assert.False(has "value" d)
            Assert.Equal(ago 3.0, time (d.GetProperty "evaluated_at"))
        })

let private groupsAndStates (rows: JsonElement) =
    [ for t in rows.EnumerateArray() -> (t.GetProperty "group").GetString(), (t.GetProperty "to_state").GetString() ]

[<Fact>]
let ``transitions are the timeline of a range, a repeated row once`` () =
    withScratchServer stored (fun client ->
        task {
            use! response = get client "/internal/monitors/7/transitions?from=-1h"
            Assert.Equal(HttpStatusCode.OK, response.StatusCode)
            let! answer = json response
            let rows = [ for t in (answer.GetProperty "transitions").EnumerateArray() -> t ]
            Assert.False((answer.GetProperty "truncated").GetBoolean())
            Assert.Empty(groupsAndStates (answer.GetProperty "initial"))

            Assert.Equal<(string * string) list>(
                [ "host:d", "Alert"
                  "host:d", "OK"
                  "host:a", "Alert"
                  "host:b", "Alert"
                  "host:b", "OK"
                  "host:d", "No Data"
                  "host:a", "Warn"
                  "host:c", "Alert"
                  "host:c", "OK" ],
                groupsAndStates (answer.GetProperty "transitions")
            )

            let first = rows[0]
            Assert.False(has "from_state" first)
            Assert.Equal(90.0, (first.GetProperty "critical").GetDouble())
            Assert.Equal(ago 40.0, time (first.GetProperty "window_from"))
            Assert.Equal("OK", (rows[5].GetProperty "from_state").GetString())
            // Expiry judges no window.
            Assert.False(has "window_from" rows[8])

            use! one = get client "/internal/monitors/7/transitions?from=-1h&group=host:a"
            let! answer = json one
            Assert.Equal(2, (answer.GetProperty "transitions").GetArrayLength())

            use! later = get client "/internal/monitors/7/transitions?from=-15m"
            let! answer = json later
            Assert.Equal(2, (answer.GetProperty "transitions").GetArrayLength())
        })

[<Fact>]
let ``a range starts from each open group's latest transition before it`` () =
    withScratchServer stored (fun client ->
        task {
            use! response = get client "/internal/monitors/7/transitions?from=-21m"
            let! answer = json response
            Assert.Equal<(string * string) list>(
                [ "host:a", "Alert"; "host:b", "OK"; "host:d", "No Data" ],
                groupsAndStates (answer.GetProperty "initial")
            )

            // host:c expired before it: closed, so not there.
            use! later = get client "/internal/monitors/7/transitions?from=-5m"
            let! answer = json later
            Assert.Empty(answer.GetProperty("transitions").EnumerateArray())
            Assert.Equal<(string * string) list>(
                [ "host:a", "Warn"; "host:b", "OK"; "host:d", "No Data" ],
                groupsAndStates (answer.GetProperty "initial")
            )

            use! one = get client "/internal/monitors/7/transitions?from=-21m&group=host:a"
            let! answer = json one
            Assert.Equal<(string * string) list>([ "host:a", "Alert" ], groupsAndStates (answer.GetProperty "initial"))
        })

[<Fact>]
let ``evaluations of a range, one group or all, the monitor's own rows among them`` () =
    withScratchServer stored (fun client ->
        task {
            use! response = get client "/internal/monitors/7/evaluations?from=-1h"
            Assert.Equal(HttpStatusCode.OK, response.StatusCode)
            let! answer = json response
            let rows = [ for e in (answer.GetProperty "evaluations").EnumerateArray() -> e ]

            Assert.Equal<(string * string) list>(
                [ "host:d", "evaluated"; "host:a", "evaluated"; "host:a", "evaluated"; "host:a", "skipped"; "", "error" ],
                rows |> List.map (fun e -> (e.GetProperty "group").GetString(), (e.GetProperty "outcome").GetString())
            )

            Assert.False(has "value" rows[0])
            Assert.Equal(42.0, (rows[1].GetProperty "value").GetDouble())
            Assert.Equal("window not full", (rows[3].GetProperty "detail").GetString())

            use! one = get client "/internal/monitors/7/evaluations?from=-1h&group=host:a"
            let! answer = json one
            Assert.Equal<string list>(
                [ "host:a"; "host:a"; "host:a"; "" ],
                [ for e in (answer.GetProperty "evaluations").EnumerateArray() -> (e.GetProperty "group").GetString() ]
            )
        })

[<Fact>]
let ``a bad range is 400`` () =
    withServer config (fun client clickhouse ->
        task {
            for path in [ "/internal/monitors/7/transitions?from=yesterday"; "/internal/monitors/7/evaluations?from=now&to=-1h" ] do
                use! response = get client path
                Assert.Equal(HttpStatusCode.BadRequest, response.StatusCode)

            Assert.Empty clickhouse.Parameters
        })
