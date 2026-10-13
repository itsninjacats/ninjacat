/// What each group of a monitor is in, and how an evaluation moves it: one
/// pure step from the groups and an evaluation to the new groups and the
/// transitions to store.
module NinjaCat.Api.Engine.Monitor.State

open System
open System.Security.Cryptography
open System.Text
open System.Text.Json
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Monitor.Definition
open NinjaCat.Api.Engine.Monitor.Evaluate

type Status =
    | OK
    | Warn
    | Alert
    | NoData

    member s.Text =
        match s with
        | OK -> "OK"
        | Warn -> "Warn"
        | Alert -> "Alert"
        | NoData -> "No Data"

[<RequireQualifiedAccess>]
type Reason =
    | New
    | Threshold
    | Recovery
    | NoData
    | Timeout
    | Expired
    | DefinitionChanged
    | Deleted

    member r.Text =
        match r with
        | Reason.New -> "new"
        | Reason.Threshold -> "threshold"
        | Reason.Recovery -> "recovery"
        | Reason.NoData -> "no_data"
        | Reason.Timeout -> "timeout"
        | Reason.Expired -> "expired"
        | Reason.DefinitionChanged -> "definition_changed"
        | Reason.Deleted -> "deleted"

/// The threshold a transition's value crossed.
type Crossed =
    | Critical
    | Warning
    | CriticalRecovery
    | WarningRecovery

    member c.Text =
        match c with
        | Critical -> "critical"
        | Warning -> "warning"
        | CriticalRecovery -> "critical_recovery"
        | WarningRecovery -> "warning_recovery"

type Group =
    { Key: string
      Tags: string list
      /// None until the group is first judged: its transition from `none` is
      /// still to come.
      State: Status option
      Since: DateTimeOffset
      /// When the current trigger began; None outside one.
      FirstTriggeredAt: DateTimeOffset option
      LastTriggeredAt: DateTimeOffset option
      /// Ticks at which the group was in the result.
      FirstSeen: DateTimeOffset
      LastSeen: DateTimeOffset
      LastPoint: DateTimeOffset }

type Transition =
    { Key: string
      Tags: string list
      At: DateTimeOffset
      /// None: the group's first transition.
      From: Status option
      To: Status
      Reason: Reason
      Value: float option
      Crossed: Crossed option
      /// The window judged; None for a transition outside a tick.
      Period: Period option
      FirstTriggeredAt: DateTimeOffset option
      LastTriggeredAt: DateTimeOffset option }

/// A group unseen this long is dropped.
let private forgetAfter = TimeSpan.FromHours 24.0

// --- thresholds ------------------------------------------------------------------------

let private holds (c: Comparator) (value: float) (threshold: float) =
    match c with
    | Gt -> value > threshold
    | Ge -> value >= threshold
    | Lt -> value < threshold
    | Le -> value <= threshold
    | Eq -> value = threshold
    | Ne -> value <> threshold

/// Datadog's recovery table: past the recovery threshold, on the side the
/// monitor recovers to. Without one, when the condition stops holding.
let private recovered (c: Comparator) (value: float) (threshold: float) (recovery: float option) =
    match recovery, c with
    | Some r, Gt -> value <= r
    | Some r, Ge -> value < r
    | Some r, Lt -> value >= r
    | Some r, Le -> value > r
    | _ -> not (holds c value threshold)

let private triggered (s: Status option) = s = Some Alert || s = Some Warn

/// The state a value puts a group in, from the state it was in, and the
/// threshold that decided it.
let private judge (q: MonitorQuery) (t: Thresholds) (current: Status option) (value: float) : Status * Crossed option =
    let c = q.Comparator
    let alerting = current = Some Alert

    let leaving =
        match current with
        | Some Alert when t.CriticalRecovery.IsSome -> Some CriticalRecovery
        | Some Warn when t.WarningRecovery.IsSome -> Some WarningRecovery
        | _ -> None

    if holds c value q.Threshold then
        Alert, Some Critical
    elif alerting && not (recovered c value q.Threshold t.CriticalRecovery) then
        Alert, Some Critical
    else
        match t.Warning with
        | Some w when holds c value w -> Warn, Some Warning
        // WARNING(undocumented): a group leaving Alert passes through Warn, so
        // it recovers from there by warning_recovery.
        | Some w when triggered current && not (recovered c value w t.WarningRecovery) -> Warn, leaving
        | _ -> OK, leaving

// --- moving a group --------------------------------------------------------------------

/// `g` moved to `next` at `at`, and the transition that records it. The
/// transition to OK carries the trigger it ends.
let private move (g: Group) (next: Status) (reason: Reason) (value: float option) (crossed: Crossed option) (period: Period option) (at: DateTimeOffset) =
    let first, last =
        match next with
        | Alert
        | Warn -> (if triggered g.State then g.FirstTriggeredAt else Some at), Some at
        | OK
        | NoData -> g.FirstTriggeredAt, g.LastTriggeredAt

    let moved =
        { g with
            State = Some next
            Since = at
            FirstTriggeredAt = (if next = OK then None else first)
            LastTriggeredAt = last }

    moved,
    { Key = g.Key
      Tags = g.Tags
      At = at
      From = g.State
      To = next
      Reason = reason
      Value = value
      Crossed = crossed
      Period = period
      FirstTriggeredAt = first
      LastTriggeredAt = last }

let private newGroupDelay (d: Definition) =
    if d.Query.GroupBy.IsEmpty then TimeSpan.Zero else d.Options.NewGroupDelay

/// A judged group with no point in the window read: after timeout_h in Alert
/// or Warn it goes to OK, after no_data_timeframe to No Data. Silence runs
/// from its last point to the end of that window, so an evaluation delay does
/// not count as silence.
///
/// WARNING(undocumented): from the last point, not from the last evaluation
/// that had the group.
let private silent (d: Definition) (g: Group) (p: Period) (at: DateTimeOffset) : Group * Transition option =
    let silence = p.To - g.LastPoint
    let noDataAfter = defaultArg d.Options.NoDataTimeframe (d.Query.Window.Length * 2.0)

    match g.State with
    | None -> g, None
    | Some state ->
        match d.Options.Timeout with
        | Some timeout when triggered g.State && silence >= timeout ->
            let moved, transition = move g OK Reason.Timeout None None (Some p) at
            moved, Some transition
        | _ when d.Options.NotifyNoData && state <> NoData && silence >= noDataAfter ->
            let moved, transition = move g NoData Reason.NoData None None (Some p) at
            moved, Some transition
        | _ -> g, None

let private seen (d: Definition) (g: Group) (r: GroupResult) (p: Period) (at: DateTimeOffset) : Group * Transition option =
    let g =
        { g with
            LastSeen = at
            LastPoint = max g.LastPoint (defaultArg r.LastPoint g.LastPoint) }

    match r.Reading, g.State with
    | Skipped _, _ when r.LastPoint.IsNone -> silent d g p at
    | Skipped _, _ -> g, None
    | Judged _, None when at - g.FirstSeen < newGroupDelay d -> g, None
    | Judged value, current ->
        let next, crossed = judge d.Query d.Options.Thresholds current value

        match current with
        | Some state when state = next -> g, None
        | Some _ ->
            let reason = if next = OK then Reason.Recovery else Reason.Threshold
            let moved, transition = move g next reason (Some value) crossed (Some p) at
            moved, Some transition
        | None ->
            let moved, transition = move g next Reason.New (Some value) crossed (Some p) at
            moved, Some transition

/// A known group missing from the result: silent, and forgotten after 24 h
/// unseen.
let private absent (d: Definition) (g: Group) (p: Period) (at: DateTimeOffset) : Group option * Transition option =
    match g.State with
    | None when at - g.LastSeen >= forgetAfter -> None, None
    | Some _ when at - g.LastSeen >= forgetAfter ->
        let _, transition = move g OK Reason.Expired None None None at
        None, Some transition
    | _ ->
        let g, transition = silent d g p at
        Some g, transition

/// One evaluation applied at the tick `at`: the groups after it and the
/// transitions to store, at most one per group.
let step (d: Definition) (groups: Map<string, Group>) (evaluation: Evaluation) (at: DateTimeOffset) : Map<string, Group> * Transition list =
    let results = evaluation.Groups |> List.map (fun r -> r.Key, r) |> Map.ofList
    let keys = Seq.append groups.Keys results.Keys |> Seq.distinct |> Seq.sort |> List.ofSeq

    let outcomes =
        [ for key in keys ->
              match groups.TryFind key, results.TryFind key with
              | Some g, Some r ->
                  let g, t = seen d g r evaluation.Period at
                  key, Some g, t
              | None, Some r ->
                  let fresh =
                      { Key = key
                        Tags = r.Tags
                        State = None
                        Since = at
                        FirstTriggeredAt = None
                        LastTriggeredAt = None
                        FirstSeen = at
                        LastSeen = at
                        LastPoint = defaultArg r.LastPoint evaluation.Period.From }

                  let g, t = seen d fresh r evaluation.Period at
                  key, Some g, t
              | Some g, None ->
                  let g, t = absent d g evaluation.Period at
                  key, g, t
              | None, None -> key, None, None ]

    let kept = outcomes |> List.choose (fun (key, g, _) -> g |> Option.map (fun g -> key, g)) |> Map.ofList
    kept, outcomes |> List.choose (fun (_, _, t) -> t)

// --- outside a tick --------------------------------------------------------------------

/// Closes every judged group with a transition to OK, not notified: the query
/// changed, or the monitor was deleted. `at` is when that was applied; the
/// caller keeps it with the pending rows, so a retry writes the same key.
let close (reason: Reason) (at: DateTimeOffset) (groups: Map<string, Group>) : Map<string, Group> * Transition list =
    Map.empty,
    [ for g in groups.Values do
          if g.State.IsSome then
              snd (move g OK reason None None None at) ]

let rec private sortedFilter (f: TagFilter) : TagFilter =
    match f with
    | In(key, values) -> In(key, List.sort values)
    | Not inner -> Not(sortedFilter inner)
    | And filters -> And(filters |> List.map sortedFilter |> List.sort)
    | Or filters -> Or(filters |> List.map sortedFilter |> List.sort)
    | All
    | Tag _
    | Bare _ -> f

/// The expression with what is written in any order sorted: filters and `by`
/// keys.
let rec private canonical (e: Expr<MetricQuery>) : Expr<MetricQuery> =
    match e with
    | Leaf q -> Leaf { q with Filter = sortedFilter q.Filter; GroupBy = List.sort q.GroupBy }
    | Number _ -> e
    | Neg inner -> Neg(canonical inner)
    | Binary(op, l, r) -> Binary(op, canonical l, canonical r)
    | Call(name, args) ->
        Call(
            name,
            args
            |> List.map (fun arg ->
                match arg with
                | Value v -> Value(canonical v)
                | Lit _
                | Named _ -> arg)
        )

/// A hash of what the monitor measures and how: the metric, its scope, its
/// `by`, the functions over it, and the detection method with its arguments.
/// A new version with another hash closes the groups; thresholds, the time
/// aggregator, the window and the message do not change it. A test pins one
/// hash, as a JSON library upgrade that changed it would reset every monitor.
let queryHash (q: MonitorQuery) : string =
    let measured = JsonSerializer.Serialize((canonical q.Measured, q.Detection), Serialization.options)
    Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes measured))

/// A new version applied at `at`: the groups go on under the same query, and
/// are closed (definition_changed) under another. `previous` is the hash the
/// groups were judged under.
let redefine (previous: string) (d: Definition) (at: DateTimeOffset) (groups: Map<string, Group>) : Map<string, Group> * Transition list =
    if queryHash d.Query = previous then groups, [] else close Reason.DefinitionChanged at groups

/// The monitor's state, its most severe judged group; None when no group is
/// judged yet, which a multi alert shows as "no groups found".
let overall (groups: Map<string, Group>) : Status option =
    let severity (s: Status) =
        match s with
        | Alert -> 3
        | Warn -> 2
        | NoData -> 1
        | OK -> 0

    match groups.Values |> Seq.choose _.State |> List.ofSeq with
    | [] -> None
    | states -> Some(List.maxBy severity states)
