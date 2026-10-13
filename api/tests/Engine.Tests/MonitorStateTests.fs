/// The state machine as a table: the groups before and an evaluation in, the
/// groups after and the transitions out.
module NinjaCat.Api.Engine.Tests.MonitorStateTests

open System
open Xunit
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Engine.Monitor.Definition
open NinjaCat.Api.Engine.Monitor.Evaluate
open NinjaCat.Api.Engine.Monitor.State

let private at = DateTimeOffset(2026, 10, 13, 12, 0, 0, TimeSpan.Zero)
let private period = { From = at.AddMinutes -5.5; To = at.AddSeconds -30.0 }

let private definition (query: string) (options: string) : Definition =
    match Validate.definition Plan.defaultRetention at "query alert" query options with
    | Ok d -> d
    | Error problems -> failwith $"refused:\n{String.Join('\n', problems)}"

let private above90 options = definition "avg(last_5m):avg:cpu{*} by {host} > 90" options
let private simple options = definition "avg(last_5m):avg:cpu{*} > 90" options

let private missing (rule: string) = $"""{{"on_missing_data":"{rule}"}}"""
let private fewRequests options = definition "sum(last_5m):sum:requests{*} by {host} < 10" options
let private fewRequestsSimple options = definition "sum(last_5m):sum:requests{*} < 10" options
let private countedErrors options = definition "sum(last_5m):sum:errors{*} by {host}.as_count() > 5" options
let private trafficDrop options = definition "pct_change(sum(last_5m),last_1h):sum:requests{*} by {host} < -50" options

/// A group judged before, seen at the previous tick, with a point near the
/// end of its window.
let private group (key: string) (state: Status option) : Group =
    { Key = key
      Tags = [ key ]
      State = state
      Since = at.AddHours -1.0
      FirstTriggeredAt = None
      LastTriggeredAt = None
      FirstSeen = at.AddHours -2.0
      LastSeen = at.AddMinutes -1.0
      LastPoint = period.To.AddSeconds -15.0
      Interrupted = None }

/// A group in a No Data that show_no_data did not notify, entered from `from`.
let private shown (key: string) (from: Status) : Group =
    { group key (Some NoData) with
        Interrupted = Some from
        FirstTriggeredAt = (if from = OK then None else Some(at.AddHours -1.0)) }

let private silentFor (span: TimeSpan) (g: Group) = { g with LastPoint = period.To - span }

type private Case =
    { Definition: Definition
      Before: Group list
      Results: (string * Reading) list
      /// Each group's state after, None for one not judged yet; groups not
      /// listed are gone.
      After: (string * Status option) list
      Transitions: (string * Status option * Status * Reason) list }

let private case d before results after transitions =
    { Definition = d
      Before = before
      Results = results
      After = after
      Transitions = transitions }

let private a = "host:a"

let private cases: Map<string, Case> =
    Map
        [
          // --- new groups
          "a new group's first judgement is a transition from none",
          case (above90 """{"new_group_delay":0}""") [] [ a, Judged 10.0 ] [ a, Some OK ] [ a, None, OK, Reason.New ]

          "a new group appearing in Alert: one transition, from none",
          case (above90 """{"new_group_delay":0}""") [] [ a, Judged 95.0 ] [ a, Some Alert ] [ a, None, Alert, Reason.New ]

          "a new group waits new_group_delay",
          case (above90 "") [] [ a, Judged 95.0 ] [ a, None ] []

          "a waiting group is judged once the delay has passed",
          case (above90 "") [ { group a None with FirstSeen = at.AddSeconds -60.0 } ] [ a, Judged 95.0 ] [ a, Some Alert ] [ a, None, Alert, Reason.New ]

          "a waiting group within the delay stays waiting",
          case (above90 "") [ { group a None with FirstSeen = at.AddSeconds -59.0 } ] [ a, Judged 95.0 ] [ a, None ] []

          "a simple alert has no new_group_delay",
          case (simple "") [] [ "*", Judged 95.0 ] [ "*", Some Alert ] [ "*", None, Alert, Reason.New ]

          "a new group with nothing to judge waits",
          case (above90 """{"new_group_delay":0}""") [] [ a, Skipped "not full" ] [ a, None ] []

          // --- thresholds
          "OK to Alert",
          case (above90 "") [ group a (Some OK) ] [ a, Judged 95.0 ] [ a, Some Alert ] [ a, Some OK, Alert, Reason.Threshold ]

          "OK stays OK",
          case (above90 "") [ group a (Some OK) ] [ a, Judged 50.0 ] [ a, Some OK ] []

          "OK to Warn",
          case (above90 """{"thresholds":{"warning":80}}""") [ group a (Some OK) ] [ a, Judged 85.0 ] [ a, Some Warn ] [ a, Some OK, Warn, Reason.Threshold ]

          "Alert to Warn directly",
          case (above90 """{"thresholds":{"warning":80}}""") [ group a (Some Alert) ] [ a, Judged 85.0 ] [ a, Some Warn ] [ a, Some Alert, Warn, Reason.Threshold ]

          "Warn to Alert directly",
          case (above90 """{"thresholds":{"warning":80}}""") [ group a (Some Warn) ] [ a, Judged 95.0 ] [ a, Some Alert ] [ a, Some Warn, Alert, Reason.Threshold ]

          "Warn to OK",
          case (above90 """{"thresholds":{"warning":80}}""") [ group a (Some Warn) ] [ a, Judged 50.0 ] [ a, Some OK ] [ a, Some Warn, OK, Reason.Recovery ]

          "Alert to OK without a recovery threshold, once the condition stops holding",
          case (above90 "") [ group a (Some Alert) ] [ a, Judged 90.0 ] [ a, Some OK ] [ a, Some Alert, OK, Reason.Recovery ]

          // --- the recovery table
          "> stays Alert above critical_recovery",
          case (above90 """{"thresholds":{"critical_recovery":70}}""") [ group a (Some Alert) ] [ a, Judged 70.1 ] [ a, Some Alert ] []

          "> recovers at <= critical_recovery",
          case (above90 """{"thresholds":{"critical_recovery":70}}""") [ group a (Some Alert) ] [ a, Judged 70.0 ] [ a, Some OK ] [ a, Some Alert, OK, Reason.Recovery ]

          ">= stays Alert at critical_recovery",
          case (definition "avg(last_5m):avg:cpu{*} by {host} >= 90" """{"thresholds":{"critical_recovery":70}}""") [ group a (Some Alert) ] [ a, Judged 70.0 ] [ a, Some Alert ] []

          ">= recovers below critical_recovery",
          case (definition "avg(last_5m):avg:cpu{*} by {host} >= 90" """{"thresholds":{"critical_recovery":70}}""") [ group a (Some Alert) ] [ a, Judged 69.9 ] [ a, Some OK ] [ a, Some Alert, OK, Reason.Recovery ]

          "< stays Alert below critical_recovery",
          case (definition "avg(last_5m):avg:free{*} by {host} < 10" """{"thresholds":{"critical_recovery":20}}""") [ group a (Some Alert) ] [ a, Judged 19.9 ] [ a, Some Alert ] []

          "< recovers at >= critical_recovery",
          case (definition "avg(last_5m):avg:free{*} by {host} < 10" """{"thresholds":{"critical_recovery":20}}""") [ group a (Some Alert) ] [ a, Judged 20.0 ] [ a, Some OK ] [ a, Some Alert, OK, Reason.Recovery ]

          "<= stays Alert at critical_recovery",
          case (definition "avg(last_5m):avg:free{*} by {host} <= 10" """{"thresholds":{"critical_recovery":20}}""") [ group a (Some Alert) ] [ a, Judged 20.0 ] [ a, Some Alert ] []

          "<= recovers above critical_recovery",
          case (definition "avg(last_5m):avg:free{*} by {host} <= 10" """{"thresholds":{"critical_recovery":20}}""") [ group a (Some Alert) ] [ a, Judged 20.1 ] [ a, Some OK ] [ a, Some Alert, OK, Reason.Recovery ]

          "Warn stays Warn above warning_recovery",
          case (above90 """{"thresholds":{"warning":80,"warning_recovery":60}}""") [ group a (Some Warn) ] [ a, Judged 70.0 ] [ a, Some Warn ] []

          "Warn recovers at warning_recovery",
          case (above90 """{"thresholds":{"warning":80,"warning_recovery":60}}""") [ group a (Some Warn) ] [ a, Judged 60.0 ] [ a, Some OK ] [ a, Some Warn, OK, Reason.Recovery ]

          "Alert past critical_recovery but short of warning_recovery is Warn",
          case
              (above90 """{"thresholds":{"critical_recovery":85,"warning":80,"warning_recovery":60}}""")
              [ group a (Some Alert) ]
              [ a, Judged 70.0 ]
              [ a, Some Warn ]
              [ a, Some Alert, Warn, Reason.Threshold ]

          "== recovers when the value differs",
          case (definition "avg(last_5m):avg:up{*} by {host} == 0" "") [ group a (Some Alert) ] [ a, Judged 1.0 ] [ a, Some OK ] [ a, Some Alert, OK, Reason.Recovery ]

          // --- nothing to judge
          "a skipped group keeps its state",
          case (above90 "") [ group a (Some Alert) ] [ a, Skipped "not full" ] [ a, Some Alert ] []

          // --- no data
          "no data within no_data_timeframe keeps the state",
          case (above90 """{"notify_no_data":true,"no_data_timeframe":10}""") [ group a (Some OK) |> silentFor (TimeSpan.FromMinutes 9.9) ] [] [ a, Some OK ] []

          "no data for no_data_timeframe is No Data",
          case
              (above90 """{"notify_no_data":true,"no_data_timeframe":10}""")
              [ group a (Some Alert) |> silentFor (TimeSpan.FromMinutes 10.0) ]
              []
              [ a, Some NoData ]
              [ a, Some Alert, NoData, Reason.NoData ]

          "no_data_timeframe defaults to twice the window",
          case (above90 """{"notify_no_data":true}""") [ group a (Some OK) |> silentFor (TimeSpan.FromMinutes 9.9) ] [] [ a, Some OK ] []

          "after twice the window, No Data",
          case (above90 """{"notify_no_data":true}""") [ group a (Some OK) |> silentFor (TimeSpan.FromMinutes 10.0) ] [] [ a, Some NoData ] [ a, Some OK, NoData, Reason.NoData ]

          "without notify_no_data a silent group keeps its state",
          case (above90 "") [ group a (Some Alert) |> silentFor (TimeSpan.FromHours 3.0) ] [] [ a, Some Alert ] []

          "No Data stays No Data",
          case (above90 """{"notify_no_data":true}""") [ group a (Some NoData) |> silentFor (TimeSpan.FromHours 1.0) ] [] [ a, Some NoData ] []

          "data back after No Data",
          case (above90 """{"notify_no_data":true}""") [ group a (Some NoData) ] [ a, Judged 10.0 ] [ a, Some OK ] [ a, Some NoData, OK, Reason.Recovery ]

          "a group not judged yet is never No Data",
          case (above90 """{"notify_no_data":true}""") [ group a None |> silentFor (TimeSpan.FromHours 1.0) ] [] [ a, None ] []

          // --- timeout_h
          "Alert silent for timeout_h is OK",
          case (above90 """{"timeout_h":1}""") [ group a (Some Alert) |> silentFor (TimeSpan.FromHours 1.0) ] [] [ a, Some OK ] [ a, Some Alert, OK, Reason.Timeout ]

          "Warn silent for timeout_h is OK",
          case (above90 """{"timeout_h":1,"thresholds":{"warning":80}}""") [ group a (Some Warn) |> silentFor (TimeSpan.FromHours 1.0) ] [] [ a, Some OK ] [ a, Some Warn, OK, Reason.Timeout ]

          "Alert silent for less than timeout_h stays",
          case (above90 """{"timeout_h":1}""") [ group a (Some Alert) |> silentFor (TimeSpan.FromMinutes 59.0) ] [] [ a, Some Alert ] []

          "timeout_h before a longer no_data_timeframe",
          case
              (above90 """{"timeout_h":1,"notify_no_data":true,"no_data_timeframe":120}""")
              [ group a (Some Alert) |> silentFor (TimeSpan.FromHours 1.0) ]
              []
              [ a, Some OK ]
              [ a, Some Alert, OK, Reason.Timeout ]

          "No Data before a longer timeout_h",
          case
              (above90 """{"timeout_h":2,"notify_no_data":true,"no_data_timeframe":60}""")
              [ group a (Some Alert) |> silentFor (TimeSpan.FromHours 1.0) ]
              []
              [ a, Some NoData ]
              [ a, Some Alert, NoData, Reason.NoData ]

          // --- on_missing_data: missing at once, the window the monitor's own
          "default: a missing avg group keeps Alert",
          case (above90 (missing "default")) [ group a (Some Alert) ] [] [ a, Some Alert ] []

          "default: a missing avg group keeps OK, simple alert",
          case (simple (missing "default")) [ group "*" (Some OK) ] [] [ "*", Some OK ] []

          "default: a missing avg group still times out",
          case
              (above90 """{"on_missing_data":"default","timeout_h":1}""")
              [ group a (Some Alert) |> silentFor (TimeSpan.FromHours 1.0) ]
              []
              [ a, Some OK ]
              [ a, Some Alert, OK, Reason.Timeout ]

          "default: a missing sum group is judged as 0 and alerts",
          case (fewRequests (missing "default")) [ group a (Some OK) ] [] [ a, Some Alert ] [ a, Some OK, Alert, Reason.Threshold ]

          "default: a missing sum group is judged as 0 and alerts, simple alert",
          case (fewRequestsSimple (missing "default")) [ group "*" (Some OK) ] [] [ "*", Some Alert ] [ "*", Some OK, Alert, Reason.Threshold ]

          "default: a missing as_count group is judged as 0 and recovers",
          case (countedErrors (missing "default")) [ group a (Some Alert) ] [] [ a, Some OK ] [ a, Some Alert, OK, Reason.Recovery ]

          "default: a missing sum group judged as 0 stays in Alert",
          case (fewRequests (missing "default")) [ group a (Some Alert) ] [] [ a, Some Alert ] []

          "default: a missing change group is not judged as 0 and keeps OK",
          case (trafficDrop (missing "default")) [ group a (Some OK) ] [] [ a, Some OK ] []

          "default: a missing change group keeps Alert",
          case (trafficDrop (missing "default")) [ group a (Some Alert) ] [] [ a, Some Alert ] []

          "default: judged as 0, timeout_h does not end it",
          case
              (fewRequests """{"on_missing_data":"default","timeout_h":1}""")
              [ group a (Some Alert) |> silentFor (TimeSpan.FromHours 2.0) ]
              []
              [ a, Some Alert ]
              []

          "show_no_data: a missing group is No Data at once, not notified",
          case (above90 (missing "show_no_data")) [ group a (Some OK) ] [] [ a, Some NoData ] [ a, Some OK, NoData, Reason.NoDataShown ]

          "show_no_data: an alerting group missing is No Data, not notified",
          case (above90 (missing "show_no_data")) [ group a (Some Alert) ] [] [ a, Some NoData ] [ a, Some Alert, NoData, Reason.NoDataShown ]

          "show_no_data: simple alert",
          case (simple (missing "show_no_data")) [ group "*" (Some OK) ] [] [ "*", Some NoData ] [ "*", Some OK, NoData, Reason.NoDataShown ]

          "show_no_data: a sum group is No Data too",
          case (fewRequests (missing "show_no_data")) [ group a (Some OK) ] [] [ a, Some NoData ] [ a, Some OK, NoData, Reason.NoDataShown ]

          "show_no_data: No Data stays No Data",
          case (above90 (missing "show_no_data")) [ shown a OK ] [] [ a, Some NoData ] []

          "show_no_data: data back is not notified",
          case (above90 (missing "show_no_data")) [ shown a OK ] [ a, Judged 10.0 ] [ a, Some OK ] [ a, Some NoData, OK, Reason.NoDataShown ]

          "show_no_data: data back ends the trigger the No Data interrupted, notified",
          case (above90 (missing "show_no_data")) [ shown a Alert ] [ a, Judged 10.0 ] [ a, Some OK ] [ a, Some NoData, OK, Reason.Recovery ]

          "show_no_data: data back breaching alerts",
          case (above90 (missing "show_no_data")) [ shown a OK ] [ a, Judged 95.0 ] [ a, Some Alert ] [ a, Some NoData, Alert, Reason.Threshold ]

          "show_no_data: an Alert back breaching after the gap is not notified again",
          case (above90 (missing "show_no_data")) [ shown a Alert ] [ a, Judged 95.0 ] [ a, Some Alert ] [ a, Some NoData, Alert, Reason.NoDataShown ]

          "show_no_data: a Warn back as Warn is not notified again",
          case
              (above90 """{"on_missing_data":"show_no_data","thresholds":{"warning":80}}""")
              [ shown a Warn ]
              [ a, Judged 85.0 ]
              [ a, Some Warn ]
              [ a, Some NoData, Warn, Reason.NoDataShown ]

          "show_no_data: a Warn back as Alert is notified",
          case
              (above90 """{"on_missing_data":"show_no_data","thresholds":{"warning":80}}""")
              [ shown a Warn ]
              [ a, Judged 95.0 ]
              [ a, Some Alert ]
              [ a, Some NoData, Alert, Reason.Threshold ]

          "show_no_data: an Alert back above its recovery threshold stays Alert",
          case
              (above90 """{"on_missing_data":"show_no_data","thresholds":{"critical_recovery":80}}""")
              [ shown a Alert ]
              [ a, Judged 85.0 ]
              [ a, Some Alert ]
              [ a, Some NoData, Alert, Reason.NoDataShown ]

          "show_no_data: a No Data that was notified recovers notified",
          case (above90 (missing "show_no_data")) [ group a (Some NoData) ] [ a, Judged 10.0 ] [ a, Some OK ] [ a, Some NoData, OK, Reason.Recovery ]

          "a No Data not notified stays silent on return after the option changed",
          case (above90 (missing "show_and_notify_no_data")) [ shown a OK ] [ a, Judged 10.0 ] [ a, Some OK ] [ a, Some NoData, OK, Reason.NoDataShown ]

          "show_no_data: an interrupted Alert still times out, notified",
          case
              (above90 """{"on_missing_data":"show_no_data","timeout_h":1}""")
              [ shown a Alert |> silentFor (TimeSpan.FromHours 1.0) ]
              []
              [ a, Some OK ]
              [ a, Some NoData, OK, Reason.Timeout ]

          "show_no_data: a skipped group with points is not missing",
          case (above90 (missing "show_no_data")) [ group a (Some OK) ] [ a, Skipped "not full" ] [ a, Some OK ] []

          "show_and_notify_no_data: a missing group is No Data at once, notified",
          case (above90 (missing "show_and_notify_no_data")) [ group a (Some Warn) ] [] [ a, Some NoData ] [ a, Some Warn, NoData, Reason.NoData ]

          "show_and_notify_no_data: simple alert",
          case (simple (missing "show_and_notify_no_data")) [ group "*" (Some OK) ] [] [ "*", Some NoData ] [ "*", Some OK, NoData, Reason.NoData ]

          "show_and_notify_no_data: data back recovers",
          case (above90 (missing "show_and_notify_no_data")) [ group a (Some NoData) ] [ a, Judged 10.0 ] [ a, Some OK ] [ a, Some NoData, OK, Reason.Recovery ]

          "show_and_notify_no_data: No Data stays No Data",
          case (above90 (missing "show_and_notify_no_data")) [ group a (Some NoData) ] [] [ a, Some NoData ] []

          "resolve: a missing Alert resolves to OK",
          case (above90 (missing "resolve")) [ group a (Some Alert) ] [] [ a, Some OK ] [ a, Some Alert, OK, Reason.NoData ]

          "resolve: a missing Warn resolves to OK",
          case (above90 """{"on_missing_data":"resolve","thresholds":{"warning":80}}""") [ group a (Some Warn) ] [] [ a, Some OK ] [ a, Some Warn, OK, Reason.NoData ]

          "resolve: simple alert",
          case (simple (missing "resolve")) [ group "*" (Some Alert) ] [] [ "*", Some OK ] [ "*", Some Alert, OK, Reason.NoData ]

          "resolve: a No Data not notified resolves without a notice",
          case (above90 (missing "resolve")) [ shown a OK ] [] [ a, Some OK ] [ a, Some NoData, OK, Reason.NoDataShown ]

          "resolve: a No Data that interrupted an Alert resolves notified",
          case (above90 (missing "resolve")) [ shown a Alert ] [] [ a, Some OK ] [ a, Some NoData, OK, Reason.NoData ]

          "resolve: a No Data that was notified resolves notified",
          case (above90 (missing "resolve")) [ group a (Some NoData) ] [] [ a, Some OK ] [ a, Some NoData, OK, Reason.NoData ]

          "resolve: a missing OK group stays OK",
          case (above90 (missing "resolve")) [ group a (Some OK) ] [] [ a, Some OK ] []

          "resolve: a sum group resolves too",
          case (fewRequests (missing "resolve")) [ group a (Some Alert) ] [] [ a, Some OK ] [ a, Some Alert, OK, Reason.NoData ]

          // Datadog: "the monitor must be able to evaluate data before alerting on missing data".
          "show_no_data: a group never judged is never missing",
          case (above90 (missing "show_no_data")) [ group a None ] [] [ a, None ] []

          "show_and_notify_no_data: a group never judged is never missing",
          case (above90 (missing "show_and_notify_no_data")) [ group a None ] [] [ a, None ] []

          "resolve: a group never judged is never missing",
          case (above90 (missing "resolve")) [ group a None ] [] [ a, None ] []

          "default: a sum group never judged is not judged as 0",
          case (fewRequests (missing "default")) [ group a None ] [] [ a, None ] []

          "show_no_data: a group unseen for 24 h still expires",
          case (above90 (missing "show_no_data")) [ { group a (Some NoData) with LastSeen = at.AddHours -24.0 } ] [] [] [ a, Some NoData, OK, Reason.Expired ]

          "default: a sum group judged as 0 still expires after 24 h unseen",
          case (fewRequests (missing "default")) [ { group a (Some Alert) with LastSeen = at.AddHours -24.0 } ] [] [] [ a, Some Alert, OK, Reason.Expired ]

          "resolve: a group unseen for 24 h expires",
          case (above90 (missing "resolve")) [ { group a (Some Alert) with LastSeen = at.AddHours -24.0 } ] [] [] [ a, Some Alert, OK, Reason.Expired ]

          // --- expiry
          "a group unseen for 24 h expires to OK",
          case (above90 "") [ { group a (Some Alert) with LastSeen = at.AddHours -24.0 } ] [] [] [ a, Some Alert, OK, Reason.Expired ]

          "an OK group expires too, closing it",
          case (above90 "") [ { group a (Some OK) with LastSeen = at.AddHours -24.0 } ] [] [] [ a, Some OK, OK, Reason.Expired ]

          "a group unseen for less than 24 h stays",
          case (above90 "") [ { group a (Some OK) with LastSeen = at.AddHours(-24.0).AddSeconds 1.0 } ] [] [ a, Some OK ] []

          "a group never judged is forgotten without a transition",
          case (above90 "") [ { group a None with LastSeen = at.AddHours -24.0 } ] [] [] []

          // --- several groups
          "each group moves on its own",
          case
              (above90 "")
              [ group "host:a" (Some OK); group "host:b" (Some Alert); group "host:c" (Some OK) ]
              [ "host:a", Judged 95.0; "host:b", Judged 95.0; "host:c", Judged 10.0 ]
              [ "host:a", Some Alert; "host:b", Some Alert; "host:c", Some OK ]
              [ "host:a", Some OK, Alert, Reason.Threshold ] ]

let caseNames =
    let data = TheoryData<string>()

    for name in cases.Keys do
        data.Add name

    data

[<Theory>]
[<MemberData(nameof caseNames)>]
let ``the state machine`` (name: string) =
    let c = cases[name]
    let before = c.Before |> List.map (fun g -> g.Key, g) |> Map.ofList

    let evaluation =
        { Period = period
          Groups = c.Results |> List.map (fun (key, r) -> { Key = key; Tags = [ key ]; Reading = r; LastPoint = Some(period.To.AddSeconds -15.0) }) }

    let after, transitions = step c.Definition before evaluation at

    Assert.Equal<(string * Status option) list>(c.After, after |> Map.toList |> List.map (fun (key, g) -> key, g.State))
    Assert.Equal<(string * Status option * Status * Reason) list>(c.Transitions, transitions |> List.map (fun t -> t.Key, t.From, t.To, t.Reason))
    Assert.True(transitions |> List.forall (fun t -> t.At = at))
    Assert.Equal(transitions.Length, transitions |> List.distinctBy _.Key |> List.length)

// --- what transitions carry --------------------------------------------------------------

let private once (d: Definition) (groups: Map<string, Group>) (results: (string * Reading) list) (tick: DateTimeOffset) =
    step d groups { Period = period; Groups = results |> List.map (fun (key, r) -> { Key = key; Tags = [ key ]; Reading = r; LastPoint = Some period.To }) } tick

[<Fact>]
let ``a trigger keeps its start through Alert and Warn; the recovery carries it, the group drops it`` () =
    let d = above90 """{"thresholds":{"warning":80,"critical_recovery":85}}"""
    let t1, t2, t3 = at, at.AddMinutes 1.0, at.AddMinutes 2.0
    let groups = Map [ a, group a (Some OK) ]

    let groups, alerted = once d groups [ a, Judged 95.0 ] t1
    let alert = Assert.Single alerted
    Assert.Equal((Some t1, Some t1), (alert.FirstTriggeredAt, alert.LastTriggeredAt))
    Assert.Equal(Some Critical, alert.Crossed)
    Assert.Equal(Some 95.0, alert.Value)
    Assert.Equal(Some period, alert.Period)
    Assert.Equal(t1, groups[a].Since)

    let groups, warned = once d groups [ a, Judged 82.0 ] t2
    let warn = Assert.Single warned
    Assert.Equal((Some t1, Some t2), (warn.FirstTriggeredAt, warn.LastTriggeredAt))
    Assert.Equal(Some Warning, warn.Crossed)

    let groups, recovered = once d groups [ a, Judged 50.0 ] t3
    let ok = Assert.Single recovered
    Assert.Equal((Some t1, Some t2), (ok.FirstTriggeredAt, ok.LastTriggeredAt))
    Assert.Equal(None, groups[a].FirstTriggeredAt)
    Assert.Equal(Some t2, groups[a].LastTriggeredAt)
    Assert.Equal(t3, groups[a].Since)

[<Fact>]
let ``recovery past a recovery threshold names it`` () =
    let d = above90 """{"thresholds":{"critical_recovery":70}}"""
    let _, transitions = once d (Map [ a, group a (Some Alert) ]) [ a, Judged 60.0 ] at
    Assert.Equal(Some CriticalRecovery, (Assert.Single transitions).Crossed)

[<Fact>]
let ``seeing a group moves its last seen and last point`` () =
    let groups, _ = once (above90 "") (Map [ a, group a (Some OK) ]) [ a, Skipped "not full" ] at
    Assert.Equal(at, groups[a].LastSeen)
    Assert.Equal(period.To, groups[a].LastPoint)

/// A group in the result with no point in the window: the engine kept its
/// series for a function's lookback, or every bucket had no number.
let private pointless (d: Definition) (g: Group) =
    step d (Map [ g.Key, g ]) { Period = period; Groups = [ { Key = g.Key; Tags = g.Tags; Reading = Skipped "no value"; LastPoint = None } ] } at

[<Fact>]
let ``a group with no point in the window goes to No Data like an absent one`` () =
    let d = above90 """{"notify_no_data":true}"""
    let silent = group a (Some OK) |> silentFor (TimeSpan.FromMinutes 10.0)
    let groups, transitions = pointless d silent
    Assert.Equal((a, Some OK, NoData, Reason.NoData), (let t = Assert.Single transitions in t.Key, t.From, t.To, t.Reason))
    Assert.Equal(silent.LastPoint, groups[a].LastPoint)
    Assert.Equal(at, groups[a].LastSeen)

[<Fact>]
let ``a group with no point in the window keeps its last point and its state until then`` () =
    let d = above90 """{"notify_no_data":true}"""
    let quiet = group a (Some Alert) |> silentFor (TimeSpan.FromMinutes 9.0)
    let groups, transitions = pointless d quiet
    Assert.Empty transitions
    Assert.Equal(quiet.LastPoint, groups[a].LastPoint)
    Assert.Equal(Some Alert, groups[a].State)

[<Fact>]
let ``no data and timeout transitions carry the window, expiry does not`` () =
    let d = above90 """{"notify_no_data":true}"""
    let _, noData = once d (Map [ a, group a (Some OK) |> silentFor (TimeSpan.FromHours 1.0) ]) [] at
    Assert.Equal(Some period, (Assert.Single noData).Period)
    let _, expired = once d (Map [ a, { group a (Some OK) with LastSeen = at.AddDays -1.0 } ]) [] at
    Assert.Equal(None, (Assert.Single expired).Period)

[<Fact>]
let ``under on_missing_data a group with no point in the window is missing like an absent one`` () =
    let _, transitions = pointless (above90 (missing "show_and_notify_no_data")) (group a (Some OK))
    Assert.Equal((a, Some OK, NoData, Reason.NoData), (let t = Assert.Single transitions in t.Key, t.From, t.To, t.Reason))

[<Fact>]
let ``a missing sum group judged as 0 carries 0 and the window`` () =
    let _, transitions = once (fewRequests (missing "default")) (Map [ a, group a (Some OK) ]) [] at
    let t = Assert.Single transitions
    Assert.Equal((Some 0.0, Some Critical, Some period), (t.Value, t.Crossed, t.Period))

[<Fact>]
let ``a resolve carries the trigger it ends`` () =
    let triggered = { group a (Some Alert) with FirstTriggeredAt = Some(at.AddHours -1.0); LastTriggeredAt = Some(at.AddMinutes -1.0) }
    let groups, transitions = once (above90 (missing "resolve")) (Map [ a, triggered ]) [] at
    let t = Assert.Single transitions
    Assert.Equal((triggered.FirstTriggeredAt, triggered.LastTriggeredAt, None), (t.FirstTriggeredAt, t.LastTriggeredAt, t.Value))
    Assert.Equal(None, groups[a].FirstTriggeredAt)

[<Fact>]
let ``a silent No Data inside a trigger keeps its start and resumes it`` () =
    let d = above90 (missing "show_no_data")
    let first = at.AddHours -1.0
    let alerting = { group a (Some Alert) with FirstTriggeredAt = Some first; LastTriggeredAt = Some first }

    let groups, _ = once d (Map [ a, alerting ]) [] at
    Assert.Equal((Some NoData, Some Alert, Some first), (groups[a].State, groups[a].Interrupted, groups[a].FirstTriggeredAt))

    let later = at.AddMinutes 1.0
    let groups, back = once d groups [ a, Judged 95.0 ] later
    let t = Assert.Single back
    Assert.Equal((Some first, Some later), (t.FirstTriggeredAt, t.LastTriggeredAt))
    Assert.Equal((Some Alert, None, Some first), (groups[a].State, groups[a].Interrupted, groups[a].FirstTriggeredAt))

// --- outside a tick ----------------------------------------------------------------------

[<Fact>]
let ``closing sends every judged group to OK and keeps none`` () =
    let changedAt = at.AddSeconds 12.5
    let groups = Map [ "host:a", group "host:a" (Some Alert); "host:b", group "host:b" (Some OK); "host:c", group "host:c" None ]
    let after, transitions = close Reason.Deleted changedAt groups
    Assert.Empty after

    Assert.Equal<(string * Status option * Status * Reason) list>(
        [ "host:a", Some Alert, OK, Reason.Deleted; "host:b", Some OK, OK, Reason.Deleted ],
        transitions |> List.map (fun t -> t.Key, t.From, t.To, t.Reason)
    )

    Assert.True(transitions |> List.forall (fun t -> t.At = changedAt && t.Period.IsNone))

[<Fact>]
let ``the query hash follows what is measured, not the thresholds`` () =
    let hash query = queryHash (definition query "").Query
    let first = hash "avg(last_5m):avg:cpu{env:prod} by {host} > 90"
    Assert.Equal(first, hash "avg(last_5m):avg:cpu{env:prod} by {host} > 95")
    Assert.Equal(first, hash "max(last_10m):avg:cpu{env:prod}   by {host} < 10")
    Assert.NotEqual<string>(first, hash "avg(last_5m):avg:cpu{env:dev} by {host} > 90")
    Assert.NotEqual<string>(first, hash "avg(last_5m):avg:cpu{env:prod} by {host,role} > 90")
    Assert.NotEqual<string>(first, hash "avg(last_5m):avg:mem{env:prod} by {host} > 90")

[<Fact>]
let ``the query hash ignores the order filters and by keys are written in`` () =
    let hash query = queryHash (definition query "").Query
    let first = hash "avg(last_5m):avg:cpu{env:prod AND role:web AND host IN (a,b)} by {host,role} > 90"
    Assert.Equal(first, hash "avg(last_5m):avg:cpu{host IN (b,a) AND role:web AND env:prod} by {role,host} > 90")

[<Fact>]
let ``the query hash follows the detection method and its arguments`` () =
    let hash query = queryHash (definition query "").Query
    let threshold = hash "avg(last_5m):avg:cpu{*} by {host} > 90"
    let change = hash "change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 90"
    Assert.NotEqual<string>(threshold, change)
    Assert.NotEqual<string>(change, hash "change(avg(last_5m),10m_ago):avg:cpu{*} by {host} > 90")
    Assert.NotEqual<string>(change, hash "pct_change(avg(last_5m),5m_ago):avg:cpu{*} by {host} > 90")

[<Fact>]
let ``the query hash is stable`` () =
    // It is stored with every transition; if this changes, every monitor's
    // groups are closed when the service next starts.
    Assert.Equal(
        "5ddd55ce98d82c97d948f85a3f8dc8a18d6b5369ae3415a03c6adc557c56936e",
        queryHash (definition "avg(last_5m):avg:cpu{env:prod} by {host} > 90" "").Query
    )

[<Fact>]
let ``a new version under another query closes the groups, under the same query keeps them`` () =
    let old = definition "avg(last_5m):avg:cpu{*} by {host} > 90" ""
    let groups = Map [ a, group a (Some Alert) ]

    let kept, none = redefine (queryHash old.Query) (definition "avg(last_5m):avg:cpu{*} by {host} > 80" "") at groups
    Assert.Equal<Map<string, Group>>(groups, kept)
    Assert.Empty none

    let reset, closed = redefine (queryHash old.Query) (definition "avg(last_5m):avg:cpu{*} by {role} > 90" "") at groups
    Assert.Empty reset
    Assert.Equal(Reason.DefinitionChanged, (Assert.Single closed).Reason)

// --- the monitor's state ----------------------------------------------------------------

[<Theory>]
[<InlineData("", "")>]
[<InlineData("none", "")>]
[<InlineData("OK", "OK")>]
[<InlineData("OK,No Data", "No Data")>]
[<InlineData("No Data,Warn,OK", "Warn")>]
[<InlineData("Warn,Alert,No Data,none", "Alert")>]
let ``the monitor is in its most severe judged group's state`` (states: string, expected: string) =
    let parse text =
        match text with
        | "OK" -> Some OK
        | "Warn" -> Some Warn
        | "Alert" -> Some Alert
        | "No Data" -> Some NoData
        | _ -> None

    let groups =
        states.Split(',', StringSplitOptions.RemoveEmptyEntries)
        |> Array.mapi (fun i s -> $"host:{i}", group $"host:{i}" (parse s))
        |> Map.ofArray

    Assert.Equal(parse expected, overall groups)
