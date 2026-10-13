/// What the panel reads of monitors: each monitor's state, its groups, its
/// transitions and its evaluations, from what the monitor service stored.
/// Every statement binds the tenant it is given; the server takes that from
/// the panel's token and runs the SQL.
module NinjaCat.Api.Engine.Monitor.Read

open System
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Monitor.State

let private tenantOf (TenantId tenant) = SqlValue.String tenant

let private millis (t: DateTimeOffset) = SqlValue.Int64(t.ToUnixTimeMilliseconds())

let private closed = SqlValue.StringArray(closes |> List.map _.Text)

/// The most monitors one status request may name: 200 ids of 19 digits and
/// their encoded commas keep the request line under Kestrel's 8 KB.
let maxIds = 200

/// `ids=1,2&ids=3`: monitor ids, comma-separated or repeated, in the order
/// first given.
let parseIds (raw: string list) : Result<int64 list, string> =
    let parts = raw |> List.collect (fun value -> value.Split(',', StringSplitOptions.TrimEntries ||| StringSplitOptions.RemoveEmptyEntries) |> List.ofArray)

    let parsed =
        parts
        |> List.map (fun part ->
            match Int64.TryParse part with
            | true, id when id > 0L -> Ok id
            | _ -> Error $"bad monitor id \"{part}\"")

    match parsed |> List.tryPick (fun p -> match p with Error e -> Some e | Ok _ -> None) with
    | Some e -> Error e
    | None ->
        match parsed |> List.choose Result.toOption |> List.distinct with
        | [] -> Error "ids is required"
        | ids when ids.Length > maxIds -> Error $"at most {maxIds} ids at once"
        | ids -> Ok ids

// --- status ----------------------------------------------------------------------------

/// Groups of one monitor that share a state and are open or closed alike.
type StatusRow =
    { Monitor: int64
      State: string
      Open: bool
      Groups: int64
      LastTriggeredAt: DateTimeOffset option }

type MonitorStatus =
    { Id: int64
      /// The most severe open group's; None when no group is open, which a
      /// multi alert shows as "no groups found".
      State: string option
      /// Open groups by state, every state present.
      Groups: Map<string, int64>
      /// When a group last went into Alert or Warn, closed groups included.
      LastTriggeredAt: DateTimeOffset option }

let status (tenant: TenantId) (ids: int64 list) : Sql =
    let bound = SqlParams()

    // The aliases differ from the column names: ClickHouse would put an
    // alias in place of a column of the same name inside another aggregate.
    let latest =
        "SELECT monitor_id, argMax(to_state, at) AS group_state, "
        + $"argMax(reason, at) NOT IN {bound.Add(closed)} AS is_open, max(last_triggered_at) AS group_triggered "
        + $"FROM monitor_transitions WHERE tenant_id = {bound.Add(tenantOf tenant)} "
        + $"AND monitor_id IN {bound.Add(SqlValue.Int64Array ids)} GROUP BY monitor_id, group_key"

    { Text =
        "SELECT monitor_id, group_state, is_open, count(), toUnixTimestamp64Milli(max(group_triggered)) "
        + $"FROM ({latest}) GROUP BY monitor_id, group_state, is_open"
      Parameters = bound.All }

let private severityOf (state: string) =
    match Status.ofText state with
    | Some s -> severity s
    | None -> -1

/// One status per id asked for, in that order; a monitor with nothing stored
/// has no state and no groups.
let statuses (ids: int64 list) (rows: StatusRow list) : MonitorStatus list =
    let zero = [ OK; Warn; Alert; NoData ] |> List.map (fun s -> s.Text, 0L) |> Map.ofList

    [ for id in ids ->
          let mine = rows |> List.filter (fun r -> r.Monitor = id)
          let opened = mine |> List.filter _.Open

          { Id = id
            State =
              match opened with
              | [] -> None
              | rows -> Some (rows |> List.maxBy (fun r -> severityOf r.State)).State
            Groups = zero |> Map.map (fun state _ -> opened |> List.filter (fun r -> r.State = state) |> List.sumBy _.Groups)
            LastTriggeredAt =
              match mine |> List.choose _.LastTriggeredAt with
              | [] -> None
              | times -> Some(List.max times) } ]

// --- groups ----------------------------------------------------------------------------

type GroupStatus =
    { Group: string
      Tags: string list
      State: string
      /// When the group came into its state.
      Since: DateTimeOffset
      /// Why: the reason of the transition into it.
      Reason: string
      /// None when OK: the trigger it carries is the one the recovery ended.
      FirstTriggeredAt: DateTimeOffset option
      LastTriggeredAt: DateTimeOffset option
      /// The value of the group's latest evaluation that judged it, and when;
      /// None when no evaluation judged it in the last day.
      Value: float option
      EvaluatedAt: DateTimeOffset option }

/// The open groups of a monitor, with their latest evaluated value.
let groups (tenant: TenantId) (monitor: int64) : Sql =
    let bound = SqlParams()

    let scope () =
        $"tenant_id = {bound.Add(tenantOf tenant)} AND monitor_id = {bound.Add(SqlValue.Int64 monitor)}"

    // argMax skips a NULL argument and would return an older trigger time;
    // inside a tuple the NULL is kept.
    let latest =
        "SELECT group_key, argMax(group_tags, at) AS latest_tags, argMax(to_state, at) AS group_state, "
        + "argMax(reason, at) AS group_reason, max(at) AS since, "
        + "tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 1) AS first_triggered, "
        + "tupleElement(argMax(tuple(first_triggered_at, last_triggered_at), at), 2) AS last_triggered "
        + $"FROM monitor_transitions WHERE {scope ()} GROUP BY group_key"

    // A group unseen for a day is closed as expired, so older rows never
    // belong to an open group.
    let evaluated =
        "SELECT group_key, argMax(value, at) AS last_value, max(at) AS evaluated_at "
        + $"FROM monitor_evaluations WHERE {scope ()} AND outcome = 'evaluated' "
        + "AND at >= now64(3) - INTERVAL 1 DAY GROUP BY group_key"

    { Text =
        "SELECT t.group_key, t.latest_tags, t.group_state, toUnixTimestamp64Milli(t.since), t.group_reason, "
        + "toUnixTimestamp64Milli(t.first_triggered), toUnixTimestamp64Milli(t.last_triggered), "
        + "e.last_value, toUnixTimestamp64Milli(e.evaluated_at) "
        + $"FROM ({latest}) AS t LEFT JOIN ({evaluated}) AS e ON e.group_key = t.group_key "
        + $"WHERE t.group_reason NOT IN {bound.Add(closed)} ORDER BY t.group_key "
        + "SETTINGS join_use_nulls = 1"
      Parameters = bound.All }

// --- over a time range -----------------------------------------------------------------

/// The most rows a transitions or evaluations request returns.
let rowLimit = 10_000

/// The newest rows of a range, oldest first: the SQL asks for one row more
/// than the limit, and the oldest is the one dropped. True when rows were left
/// out.
let newest (rows: 'T list) : 'T list * bool =
    match rows with
    | _ :: newer when rows.Length > rowLimit -> newer, true
    | _ -> rows, false

/// `columns` of `table` in [from, to), the rows of `groups` or all, newest
/// rowLimit + 1 rows, oldest first.
let private range (table: string) (columns: string) (tenant: TenantId) (monitor: int64) (from: DateTimeOffset) (until: DateTimeOffset) (groups: string list) =
    let bound = SqlParams()

    let where =
        $"tenant_id = {bound.Add(tenantOf tenant)} AND monitor_id = {bound.Add(SqlValue.Int64 monitor)}"
        + $" AND at >= fromUnixTimestamp64Milli({bound.Add(millis from)}) AND at < fromUnixTimestamp64Milli({bound.Add(millis until)})"
        + (if groups.IsEmpty then "" else $" AND group_key IN {bound.Add(SqlValue.StringArray groups)}")

    { Text =
        $"SELECT * FROM (SELECT {columns} FROM {table} WHERE {where} ORDER BY at DESC, group_key LIMIT {rowLimit + 1}) "
        + "ORDER BY at_ms, group_key"
      Parameters = bound.All }

type TransitionRecord =
    { Group: string
      Tags: string list
      At: DateTimeOffset
      /// None for the group's first transition.
      FromState: string option
      ToState: string
      Reason: string
      /// The user whose change caused it.
      Actor: string option
      Value: float option
      Crossed: string option
      Comparator: string
      Critical: float option
      Warning: float option
      CriticalRecovery: float option
      WarningRecovery: float option
      /// The window judged; None for a transition outside a tick.
      WindowFrom: DateTimeOffset option
      WindowTo: DateTimeOffset option
      FirstTriggeredAt: DateTimeOffset option
      LastTriggeredAt: DateTimeOffset option
      MonitorVersion: int64 }

let private transitionColumns =
    "group_key, group_tags, toUnixTimestamp64Milli(at) AS at_ms, from_state, to_state, reason, actor, value, crossed, "
    + "comparator, critical, warning, critical_recovery, warning_recovery, "
    + "toUnixTimestamp64Milli(window_from), toUnixTimestamp64Milli(window_to), "
    + "toUnixTimestamp64Milli(first_triggered_at), toUnixTimestamp64Milli(last_triggered_at), monitor_version"

/// A monitor's transitions in a time range, one group or all. FINAL: a
/// retried insert or two owners during a hand-off write the same row twice.
let transitions (tenant: TenantId) (monitor: int64) (from: DateTimeOffset) (until: DateTimeOffset) (group: string) : Sql =
    range "monitor_transitions FINAL" transitionColumns tenant monitor from until (if group = "" then [] else [ group ])

/// Each open group's latest transition before `from`, one group or all: the
/// state the groups enter the range in, which no transition inside it may say.
let initial (tenant: TenantId) (monitor: int64) (from: DateTimeOffset) (group: string) : Sql =
    let bound = SqlParams()

    let where =
        $"tenant_id = {bound.Add(tenantOf tenant)} AND monitor_id = {bound.Add(SqlValue.Int64 monitor)}"
        + $" AND at < fromUnixTimestamp64Milli({bound.Add(millis from)})"
        + (if group = "" then "" else $" AND group_key = {bound.Add(SqlValue.String group)}")

    { Text =
        $"SELECT * FROM (SELECT {transitionColumns} FROM monitor_transitions WHERE {where} "
        + $"ORDER BY group_key, at DESC LIMIT 1 BY group_key) WHERE reason NOT IN {bound.Add(closed)} ORDER BY group_key"
      Parameters = bound.All }

type EvaluationRecord =
    { /// Empty for an evaluation skipped or failed as a whole.
      Group: string
      At: DateTimeOffset
      Value: float option
      /// The group's state after it; "none" before its first transition.
      State: string
      /// evaluated, skipped or error.
      Outcome: string
      Detail: string option }

/// A monitor's evaluations in a time range, one group or all: the values for
/// its graph. One group's come with the monitor's own rows, the evaluations
/// skipped or failed as a whole, so a gap in its graph has its reason.
let evaluations (tenant: TenantId) (monitor: int64) (from: DateTimeOffset) (until: DateTimeOffset) (group: string) : Sql =
    range
        "monitor_evaluations"
        "group_key, toUnixTimestamp64Milli(at) AS at_ms, value, state, outcome, detail"
        tenant
        monitor
        from
        until
        (if group = "" then [] else [ group; "" ])
