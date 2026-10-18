module NinjaCat.Api.Monitors.Store

open System
open System.Data.Common
open System.Threading
open System.Threading.Tasks
open ClickHouse.Driver
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Engine.Monitor.Definition
open NinjaCat.Api.Engine.Monitor.Evaluate
open NinjaCat.Api.Engine.Monitor.State
open NinjaCat.Api.Platform
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Monitors.Definitions

type History =
    { Groups: Map<string, Group>
      QueryHash: string option }

let empty = { Groups = Map.empty; QueryHash = None }

let private stateText (s: Status option) =
    match s with
    | Some s -> s.Text
    | None -> "none"

/// The thresholds are kept on the row because Postgres has only the latest definition.
let transitionRow
    (key: MonitorKey)
    (version: int)
    (queryHash: string)
    (definition: Monitor option)
    (actor: string option)
    (t: Transition)
    : MonitorTransitionRow =
    { TenantID = key.Tenant
      MonitorID = uint64 key.Id
      MonitorVersion = uint32 version
      QueryHash = queryHash
      MonitorKind =
        match definition with
        | Some(ServiceCheck _) -> "service_check"
        | _ -> "metric"
      GroupKey = t.Key
      GroupTags = Array.ofList t.Tags
      At = t.At.UtcDateTime
      FromState = stateText t.From
      ToState = t.To.Text
      Reason = t.Reason.Text
      Actor = actor
      Value = t.Value
      Crossed =
        match t.Crossed with
        | Some c -> c.Text
        | None -> "none"
      Comparator =
        match definition with
        | Some(Metric d) -> d.Query.Comparator.Text
        | _ -> ""
      Critical = definition |> Option.bind _.Options.Thresholds.Critical
      Warning = definition |> Option.bind _.Options.Thresholds.Warning
      CriticalRecovery = definition |> Option.bind _.Options.Thresholds.CriticalRecovery
      WarningRecovery = definition |> Option.bind _.Options.Thresholds.WarningRecovery
      // A transition outside a tick judged no window; the zero time is
      // stored as the epoch.
      WindowFrom = t.Period |> Option.map _.From.UtcDateTime |> Option.defaultValue DateTime.MinValue
      WindowTo = t.Period |> Option.map _.To.UtcDateTime |> Option.defaultValue DateTime.MinValue
      FirstTriggeredAt = t.FirstTriggeredAt |> Option.map _.UtcDateTime
      LastTriggeredAt = t.LastTriggeredAt |> Option.map _.UtcDateTime }

let evaluationRows (key: MonitorKey) (at: DateTimeOffset) (groups: Map<string, Group>) (e: Evaluation) : MonitorEvaluationRow[] =
    [| for r in e.Groups ->
           let value, outcome, detail =
               match r.Reading with
               | Judged v -> Some v, "evaluated", ""
               | Skipped(_, reason) -> None, "skipped", reason
               | Streaks s -> Some(Check.worst s), "evaluated", ""

           { TenantID = key.Tenant
             MonitorID = uint64 key.Id
             GroupKey = r.Key
             At = at.UtcDateTime
             Value = value
             State = groups.TryFind r.Key |> Option.bind _.State |> stateText
             Outcome = outcome
             Detail = detail } |]

/// An evaluation skipped or failed as a whole; its group key is empty, which no group has.
let monitorRow (key: MonitorKey) (at: DateTimeOffset) (groups: Map<string, Group>) (outcome: string) (detail: string) : MonitorEvaluationRow =
    { TenantID = key.Tenant
      MonitorID = uint64 key.Id
      GroupKey = ""
      At = at.UtcDateTime
      Value = None
      State = stateText (overall groups)
      Outcome = outcome
      Detail = detail }

let private closing = closes |> List.map _.Text |> set

let private keyParameters (key: MonitorKey) =
    [ "tenant", SqlValue.String key.Tenant; "monitor", SqlValue.Int64 key.Id ]

let time (r: DbDataReader) (i: int) : DateTimeOffset =
    DateTimeOffset(DateTime.SpecifyKind(r.GetDateTime i, DateTimeKind.Utc))

let optionalTime (r: DbDataReader) (i: int) : DateTimeOffset option =
    if r.IsDBNull i then None else Some(time r i)

/// Only the last two days: a group unseen longer is dropped at the next evaluation anyway.
let private lastSeen (client: ClickHouseClient) (key: MonitorKey) (now: DateTimeOffset) (ct: CancellationToken) : Task<Map<string, DateTimeOffset>> =
    task {
        let! rows =
            ClickHouse.query
                client
                ct
                (fun r -> r.GetString 0, time r 1)
                { Text =
                    "SELECT group_key, max(at) FROM monitor_evaluations "
                    + "WHERE tenant_id = {tenant:String} AND monitor_id = {monitor:Int64} AND group_key != '' "
                    + "AND at >= fromUnixTimestamp64Milli({since:Int64}) GROUP BY group_key"
                  Parameters = keyParameters key @ [ "since", SqlValue.Int64((now.AddDays -2.0).ToUnixTimeMilliseconds()) ] }

        return Map.ofList rows
    }

let history (client: ClickHouseClient) (key: MonitorKey) (now: DateTimeOffset) (ct: CancellationToken) : Task<History> =
    task {
        let! seen = lastSeen client key now ct

        // A repeated row (a retried insert, two owners during a hand-off) has
        // the same `at` and the same values.
        let! latest =
            ClickHouse.query
                client
                ct
                (fun r ->
                    let groupKey = r.GetString 0
                    let state = Status.ofText (r.GetString 2)
                    let reason = r.GetString 3
                    let at = time r 4
                    let lastSeen = seen.TryFind groupKey |> Option.defaultValue at |> max at

                    let group: Group =
                        { Key = groupKey
                          Tags = (r.GetValue 1 :?> string[]) |> List.ofArray
                          State = state
                          Since = at
                          FirstTriggeredAt = (if state = Some OK then None else optionalTime r 6)
                          LastTriggeredAt = optionalTime r 7
                          FirstSeen = time r 5
                          LastSeen = lastSeen
                          // Points are not stored with the evaluation; the last tick that
                          // saw the group is the nearest bound. It errs late, so No Data
                          // comes at most a window and the evaluation delay later.
                          LastPoint = lastSeen
                          Interrupted =
                            if state = Some NoData && reason = Reason.NoDataShown.Text then
                                Status.ofText (r.GetString 9)
                            else
                                None }

                    reason, r.GetString 8, group)
                { Text =
                    // argMax skips a NULL argument and would return an older trigger
                    // time; inside a tuple the NULL is kept.
                    "SELECT group_key, argMax(group_tags, at), argMax(to_state, at), argMax(reason, at), max(at), min(at), "
                    + "tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 1), "
                    + "tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 2), argMax(query_hash, at), "
                    + "argMax(from_state, at) "
                    + "FROM monitor_transitions WHERE tenant_id = {tenant:String} AND monitor_id = {monitor:Int64} "
                    + "GROUP BY group_key"
                  Parameters = keyParameters key }

        let openGroups =
            latest |> List.filter (fun (reason, _, g) -> g.State.IsSome && not (closing.Contains reason))

        return
            { Groups = openGroups |> List.map (fun (_, _, g) -> g.Key, g) |> Map.ofList
              QueryHash =
                match openGroups with
                | [] -> None
                | groups ->
                    let _, hash, _ = groups |> List.maxBy (fun (_, _, g) -> g.Since)
                    Some hash }
    }
