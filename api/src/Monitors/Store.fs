/// What a monitor leaves in ClickHouse: its transitions and evaluations as
/// rows, and the groups an entity starts from, read back from them.
module NinjaCat.Api.Monitors.Store

open System
open System.Data.Common
open System.Threading
open System.Threading.Tasks
open ClickHouse.Driver
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Monitor.Definition
open NinjaCat.Api.Engine.Monitor.Evaluate
open NinjaCat.Api.Engine.Monitor.State
open NinjaCat.Api.Platform
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Monitors.Definitions

/// The groups an entity starts from, and the query hash they were judged
/// under; None when no group is open.
type History =
    { Groups: Map<string, Group>
      QueryHash: string option }

let empty = { Groups = Map.empty; QueryHash = None }

let private stateText (s: Status option) =
    match s with
    | Some s -> s.Text
    | None -> "none"

let private statusOf (text: string) : Status option =
    match text with
    | "OK" -> Some OK
    | "Warn" -> Some Warn
    | "Alert" -> Some Alert
    | "No Data" -> Some NoData
    | _ -> None

let private utc (t: DateTimeOffset) = t.UtcDateTime

/// A transition as stored, with what judged it: the definition's thresholds
/// (Postgres keeps only the latest), its version and query hash, and the user
/// whose change caused it. Without a valid definition the thresholds are left
/// empty.
let transitionRow
    (key: MonitorKey)
    (version: int)
    (queryHash: string)
    (definition: Definition option)
    (actor: string option)
    (t: Transition)
    : MonitorTransitionRow =
    let thresholds = definition |> Option.map _.Options.Thresholds

    let threshold (pick: Thresholds -> float option) = thresholds |> Option.bind pick

    { TenantID = key.Tenant
      MonitorID = uint64 key.Id
      MonitorVersion = uint32 version
      QueryHash = queryHash
      MonitorKind = "metric"
      GroupKey = t.Key
      GroupTags = Array.ofList t.Tags
      At = utc t.At
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
        | Some d -> d.Query.Comparator.Text
        | None -> ""
      Critical = threshold _.Critical
      Warning = threshold _.Warning
      CriticalRecovery = threshold _.CriticalRecovery
      WarningRecovery = threshold _.WarningRecovery
      // A transition outside a tick judged no window; the zero time is
      // stored as the epoch.
      WindowFrom = t.Period |> Option.map (fun p -> utc p.From) |> Option.defaultValue DateTime.MinValue
      WindowTo = t.Period |> Option.map (fun p -> utc p.To) |> Option.defaultValue DateTime.MinValue
      FirstTriggeredAt = t.FirstTriggeredAt |> Option.map utc
      LastTriggeredAt = t.LastTriggeredAt |> Option.map utc }

/// One row per group in the result, with the state the evaluation left it in.
let evaluationRows (key: MonitorKey) (at: DateTimeOffset) (groups: Map<string, Group>) (e: Evaluation) : MonitorEvaluationRow[] =
    [| for r in e.Groups ->
           let value, outcome, detail =
               match r.Reading with
               | Judged v -> Some v, "evaluated", ""
               | Skipped reason -> None, "skipped", reason

           { TenantID = key.Tenant
             MonitorID = uint64 key.Id
             GroupKey = r.Key
             At = utc at
             Value = value
             State = groups.TryFind r.Key |> Option.bind _.State |> stateText
             Outcome = outcome
             Detail = detail } |]

/// The row of an evaluation that judged no group: skipped or failed as a
/// whole. Its group key is empty, which no group has.
let monitorRow (key: MonitorKey) (at: DateTimeOffset) (groups: Map<string, Group>) (outcome: string) (detail: string) : MonitorEvaluationRow =
    { TenantID = key.Tenant
      MonitorID = uint64 key.Id
      GroupKey = ""
      At = utc at
      Value = None
      State = stateText (overall groups)
      Outcome = outcome
      Detail = detail }

// --- reading back ----------------------------------------------------------------------

/// The latest transition of a group says it is closed.
let private closing = set [ Reason.Expired.Text; Reason.DefinitionChanged.Text; Reason.Deleted.Text ]

let private keyParameters (key: MonitorKey) =
    [ "tenant", SqlValue.String key.Tenant; "monitor", SqlValue.Int64 key.Id ]

let private time (r: DbDataReader) (i: int) : DateTimeOffset =
    DateTimeOffset(DateTime.SpecifyKind(r.GetDateTime i, DateTimeKind.Utc))

let private optionalTime (r: DbDataReader) (i: int) : DateTimeOffset option =
    if r.IsDBNull i then None else Some(time r i)

type private Latest =
    { Key: string
      Tags: string list
      State: Status option
      Reason: string
      At: DateTimeOffset
      FirstAt: DateTimeOffset
      FirstTriggeredAt: DateTimeOffset option
      LastTriggeredAt: DateTimeOffset option
      QueryHash: string }

/// Each group's latest transition. A repeated row (a retried insert, two
/// owners during a handoff) has the same `at` and the same values.
let private latest (client: ClickHouseClient) (key: MonitorKey) (ct: CancellationToken) : Task<Latest list> =
    ClickHouse.query
        client
        ct
        (fun r ->
            { Key = r.GetString 0
              Tags = (r.GetValue 1 :?> string[]) |> List.ofArray
              State = statusOf (r.GetString 2)
              Reason = r.GetString 3
              At = time r 4
              FirstAt = time r 5
              FirstTriggeredAt = optionalTime r 6
              LastTriggeredAt = optionalTime r 7
              QueryHash = r.GetString 8 })
        { Text =
            // argMax skips a NULL argument and would return an older trigger
            // time; inside a tuple the NULL is kept.
            "SELECT group_key, argMax(group_tags, at), argMax(to_state, at), argMax(reason, at), max(at), min(at), "
            + "tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 1), "
            + "tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 2), argMax(query_hash, at) "
            + "FROM monitor_transitions WHERE tenant_id = {tenant:String} AND monitor_id = {monitor:Int64} "
            + "GROUP BY group_key"
          Parameters = keyParameters key }

/// When each group was last in a result, from the evaluations of the last
/// day: a group unseen longer is dropped at the next evaluation anyway.
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
                  Parameters = keyParameters key @ [ "since", SqlValue.Int64((now.AddDays -1.0).ToUnixTimeMilliseconds()) ] }

        return Map.ofList rows
    }

/// The open groups of a monitor, as its transitions and evaluations left
/// them. A group whose latest transition closed it is not loaded.
let history (client: ClickHouseClient) (key: MonitorKey) (now: DateTimeOffset) (ct: CancellationToken) : Task<History> =
    task {
        let! transitions = latest client key ct
        let! seen = lastSeen client key now ct
        let openGroups = transitions |> List.filter (fun t -> t.State.IsSome && not (closing.Contains t.Reason))

        let groupOf (t: Latest) : Group =
            let lastSeen = seen.TryFind t.Key |> Option.defaultValue t.At |> max t.At

            { Key = t.Key
              Tags = t.Tags
              State = t.State
              Since = t.At
              FirstTriggeredAt = (if t.State = Some OK then None else t.FirstTriggeredAt)
              LastTriggeredAt = t.LastTriggeredAt
              FirstSeen = t.FirstAt
              LastSeen = lastSeen
              // Points are not stored with the evaluation; the last tick that
              // saw the group is the nearest bound. It errs late, so No Data
              // comes at most a window and the evaluation delay later.
              LastPoint = lastSeen }

        return
            { Groups = openGroups |> List.map (fun t -> t.Key, groupOf t) |> Map.ofList
              QueryHash =
                match openGroups with
                | [] -> None
                | groups -> Some (List.maxBy _.At groups).QueryHash }
    }
