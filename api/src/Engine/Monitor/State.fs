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

    static member ofText(text: string) : Status option =
        match text with
        | "OK" -> Some OK
        | "Warn" -> Some Warn
        | "Alert" -> Some Alert
        | "No Data" -> Some NoData
        | _ -> None

/// The most severe group decides a monitor's state.
let severity (s: Status) =
    match s with
    | Alert -> 3
    | Warn -> 2
    | NoData -> 1
    | OK -> 0

[<RequireQualifiedAccess>]
type Reason =
    | New
    | Threshold
    | Recovery
    | NoData
    /// Into No Data from a check's UNKNOWN runs, not from silence.
    | UnknownRuns
    /// Into, or back out of, the No Data that show_no_data shows without
    /// notifying.
    | NoDataShown
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
        | Reason.UnknownRuns -> "unknown_runs"
        | Reason.NoDataShown -> "no_data_shown"
        | Reason.Timeout -> "timeout"
        | Reason.Expired -> "expired"
        | Reason.DefinitionChanged -> "definition_changed"
        | Reason.Deleted -> "deleted"

/// Reasons the notifier never notifies. A new group that is OK is not
/// notified either.
let neverNotified = [ Reason.NoDataShown; Reason.Expired; Reason.DefinitionChanged; Reason.Deleted ]

/// A group whose latest transition has one of these reasons is closed: the
/// service does not load it and the panel does not show it.
let closes = [ Reason.Expired; Reason.DefinitionChanged; Reason.Deleted ]

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
      /// None until the group is first judged.
      State: Status option
      Since: DateTimeOffset
      FirstTriggeredAt: DateTimeOffset option
      LastTriggeredAt: DateTimeOffset option
      FirstSeen: DateTimeOffset
      LastSeen: DateTimeOffset
      LastPoint: DateTimeOffset
      /// In a No Data that show_no_data did not notify: the state it interrupted, to resume from.
      Interrupted: Status option }

type Transition =
    { Key: string
      Tags: string list
      At: DateTimeOffset
      From: Status option
      To: Status
      Reason: Reason
      Value: float option
      Crossed: Crossed option
      Period: Period option
      FirstTriggeredAt: DateTimeOffset option
      LastTriggeredAt: DateTimeOffset option }

/// A group unseen this long is dropped.
/// WARNING(undocumented): 48 h for a check monitor that notifies No Data, as Datadog keeps one.
let private forgetAfter (m: Monitor) =
    match m with
    | ServiceCheck c when c.Options.NotifyNoData -> Check.longestNoData
    | _ -> TimeSpan.FromHours 24.0

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

/// The state a group is judged from: the one a silent No Data interrupted, or
/// its own.
let private resumed (g: Group) = Option.orElse g.State g.Interrupted

/// The state a value puts a group in, from the state it was in, and the
/// threshold that decided it.
///
/// WARNING(undocumented): Datadog shows 0 as the usual recovery threshold of
/// an anomaly monitor; its default is not documented.
let private judge (q: MonitorQuery) (t: Thresholds) (current: Status option) (value: float) (recovery: float option) : Status * Crossed option =
    let c = q.Comparator
    let alerting = current = Some Alert

    let isRecovered (threshold: float) (recoveryThreshold: float option) =
        match recovery with
        | Some fraction -> fraction <= defaultArg recoveryThreshold 0.0
        | None -> recovered c value threshold recoveryThreshold

    let leaving =
        match current with
        | Some Alert when t.CriticalRecovery.IsSome -> Some CriticalRecovery
        | Some Warn when t.WarningRecovery.IsSome -> Some WarningRecovery
        | _ -> None

    if holds c value q.Threshold then
        Alert, Some Critical
    elif alerting && not (isRecovered q.Threshold t.CriticalRecovery) then
        Alert, Some Critical
    else
        match t.Warning with
        | Some w when holds c value w -> Warn, Some Warning
        // WARNING(undocumented): a group leaving Alert passes through Warn, so
        // it recovers from there by warning_recovery.
        | Some w when triggered current && not (isRecovered w t.WarningRecovery) -> Warn, leaving
        | _ -> OK, leaving

let private hasKnownRun (s: Streak) = s.Critical + s.Warning + s.Ok > 0

/// Check.definition fills critical, warning and ok.
/// WARNING(undocumented): a series with `unknown` UNKNOWN runs in a row is No Data, whatever came
/// before them, and a No Data resolves on OK runs since the last UNKNOWN. With no threshold reached
/// the group keeps its state; without `unknown` a series with no known run is left out.
let private judgeRuns (t: Thresholds) (current: Status option) (streaks: Streak list) : (Status * Crossed option) option =
    let unknownOut (s: Streak) = t.Unknown |> Option.exists (fun u -> float s.Unknown >= u)
    let outs, rest = streaks |> List.partition unknownOut
    let known = rest |> List.filter hasKnownRun
    let okRuns (s: Streak) = if current = Some NoData then s.OkSinceUnknown else s.Ok

    if known |> List.exists (fun s -> float s.Critical >= t.Critical.Value) then
        Some(Alert, Some Critical)
    elif known |> List.exists (fun s -> float s.Warning >= t.Warning.Value) then
        Some(Warn, Some Warning)
    elif not outs.IsEmpty then
        Some(NoData, None)
    elif not known.IsEmpty && known |> List.forall (fun s -> float (okRuns s) >= t.Ok.Value) then
        Some(OK, (if triggered current then Some CriticalRecovery else None))
    else
        None

// --- moving a group --------------------------------------------------------------------

/// `g` moved to `next` at `at`, and the transition that records it. The
/// transition to OK carries the trigger it ends.
let private move (g: Group) (next: Status) (reason: Reason) (value: float option) (crossed: Crossed option) (period: Period option) (at: DateTimeOffset) =
    let first, last =
        match next with
        | Alert
        | Warn -> (if triggered (resumed g) then g.FirstTriggeredAt else Some at), Some at
        | OK
        | NoData -> g.FirstTriggeredAt, g.LastTriggeredAt

    let moved =
        { g with
            State = Some next
            Since = at
            FirstTriggeredAt = (if next = OK then None else first)
            LastTriggeredAt = last
            Interrupted = (if next = NoData && reason = Reason.NoDataShown then g.State else None) }

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

let private into (g: Group) (next: Status) (reason: Reason) (value: float option) (crossed: Crossed option) (p: Period) (at: DateTimeOffset) =
    if g.State = Some next then
        g, None
    else
        let moved, transition = move g next reason value crossed (Some p) at
        moved, Some transition

let private judged (m: Monitor) (g: Group) (reading: Reading) (recovery: float option) (p: Period) (at: DateTimeOffset) : Group * Transition option =
    let outcome =
        match m, reading with
        | Metric d, Judged value ->
            let next, crossed = judge d.Query d.Options.Thresholds (resumed g) value recovery
            Some(next, crossed, value)
        | ServiceCheck c, Streaks streaks ->
            judgeRuns c.Options.Thresholds (resumed g) streaks |> Option.map (fun (next, crossed) -> next, crossed, Check.worst streaks)
        | _ -> None

    match outcome with
    | None -> g, None
    | Some(next, crossed, value) ->
        let reason =
            match g.State, next with
            | _, NoData -> Reason.UnknownRuns
            | None, _ -> Reason.New
            // WARNING(undocumented): back from a No Data that was not notified,
            // into the state it interrupted, is not notified either; a trigger
            // goes on unbroken. Into another state, it is notified as usual.
            | Some NoData, _ when g.Interrupted = Some next -> Reason.NoDataShown
            | Some _, OK -> Reason.Recovery
            | Some _, _ -> Reason.Threshold

        into g next reason (Some value) crossed p at

/// on_missing_data `default`: a sum over no points is 0 (`.as_count()` takes only sum).
/// WARNING(undocumented): only for a threshold; a missing group of a change monitor keeps its state.
let private zeroWhenMissing (m: Monitor) =
    match m with
    | Metric d -> d.Options.OnMissingData = Some MissingData.Default && d.Query.Detection.IsThreshold && d.Query.Aggregator = Sum
    | ServiceCheck _ -> false

/// A judged group with no point in the window. Silence runs to the window's end, so an evaluation
/// delay does not count as silence.
/// WARNING(undocumented): from the last point, not from the last evaluation that had the group.
let private silent (m: Monitor) (g: Group) (p: Period) (at: DateTimeOffset) : Group * Transition option =
    let silence = p.To - g.LastPoint
    let o = m.Options

    // Datadog's default for a check is 24 hours.
    let noDataAfter =
        match m with
        | Metric d -> defaultArg o.NoDataTimeframe (d.Query.Window.Length * 2.0)
        | ServiceCheck _ -> defaultArg o.NoDataTimeframe (TimeSpan.FromHours 24.0)

    match g.State, o.Timeout with
    | None, _ -> g, None
    // Judged as 0, it is not without data, so timeout_h does not end it.
    | Some _, _ when zeroWhenMissing m -> judged m g (Judged 0.0) None p at
    | Some _, Some timeout when triggered (resumed g) && silence >= timeout -> into g OK Reason.Timeout None None p at
    | Some state, _ ->
        // WARNING(undocumented): Datadog documents on_missing_data for
        // event-based monitors; its migration guide offers it for metrics.
        // `resolve` is notified like a timeout when it ends Alert or Warn,
        // or a No Data that was notified; not when it ends a No Data that
        // was not.
        match o.OnMissingData with
        | Some MissingData.Default -> g, None
        | Some MissingData.ShowNoData -> into g NoData Reason.NoDataShown None None p at
        | Some MissingData.ShowAndNotifyNoData -> into g NoData Reason.NoData None None p at
        | Some MissingData.Resolve when g.Interrupted = Some OK -> into g OK Reason.NoDataShown None None p at
        | Some MissingData.Resolve -> into g OK Reason.NoData None None p at
        | None when o.NotifyNoData && state <> NoData && silence >= noDataAfter -> into g NoData Reason.NoData None None p at
        | None -> g, None

let private seen (m: Monitor) (g: Group) (r: GroupResult) (p: Period) (at: DateTimeOffset) : Group * Transition option =
    let g =
        { g with
            LastSeen = at
            LastPoint = max g.LastPoint (defaultArg r.LastPoint g.LastPoint) }

    let groupBy =
        match m with
        | Metric d -> d.Query.GroupBy
        | ServiceCheck c -> c.Query.GroupBy

    let delay = if groupBy.IsEmpty then TimeSpan.Zero else m.Options.NewGroupDelay

    match r.Reading, g.State with
    | Skipped _, _ when r.LastPoint.IsNone -> silent m g p at
    | Skipped _, _ -> g, None
    | _, None when at - g.FirstSeen < delay -> g, None
    | reading, _ -> judged m g reading r.Recovery p at

/// A known group missing from the result: silent, and forgotten once unseen
/// for forgetAfter.
let private absent (m: Monitor) (g: Group) (p: Period) (at: DateTimeOffset) : Group option * Transition option =
    match g.State with
    | None when at - g.LastSeen >= forgetAfter m -> None, None
    | Some _ when at - g.LastSeen >= forgetAfter m ->
        let _, transition = move g OK Reason.Expired None None None at
        None, Some transition
    | _ ->
        let g, transition = silent m g p at
        Some g, transition

/// One evaluation applied at the tick `at`: the groups after it and the
/// transitions to store, at most one per group.
let step (m: Monitor) (groups: Map<string, Group>) (evaluation: Evaluation) (at: DateTimeOffset) : Map<string, Group> * Transition list =
    let results = evaluation.Groups |> List.map (fun r -> r.Key, r) |> Map.ofList
    let keys = Seq.append groups.Keys results.Keys |> Seq.distinct |> Seq.sort |> List.ofSeq

    let outcomes =
        [ for key in keys ->
              match groups.TryFind key, results.TryFind key with
              | Some g, Some r ->
                  let g, t = seen m g r evaluation.Period at
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
                        LastPoint = defaultArg r.LastPoint evaluation.Period.From
                        Interrupted = None }

                  let g, t = seen m fresh r evaluation.Period at
                  key, Some g, t
              | Some g, None ->
                  let g, t = absent m g evaluation.Period at
                  key, g, t
              | None, None -> key, None, None ]

    let kept = outcomes |> List.choose (fun (key, g, _) -> g |> Option.map (fun g -> key, g)) |> Map.ofList
    kept, outcomes |> List.choose (fun (_, _, t) -> t)

// --- outside a tick --------------------------------------------------------------------

/// Every judged group to OK, not notified. The caller keeps `at` with the pending rows, so a retry writes the same key.
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

let private sha256 (text: string) = Convert.ToHexStringLower(SHA256.HashData(Encoding.UTF8.GetBytes text))

/// What the monitor measures and how, not its thresholds, aggregator, window or message: a new
/// version with another hash closes the groups.
let queryHash (q: MonitorQuery) : string =
    sha256 (JsonSerializer.Serialize((canonical q.Measured, q.Detection), Serialization.options))

/// A check's hash leaves out last(N) and the thresholds, as a metric's leaves out the window.
let monitorHash (m: Monitor) : string =
    match m with
    | Metric d -> queryHash d.Query
    | ServiceCheck c ->
        let q = c.Query
        sha256 (JsonSerializer.Serialize(("service check", q.Check, sortedFilter q.Filter, q.GroupBy), Serialization.options))

/// A new version applied at `at`: the groups go on under the same query, and
/// are closed (definition_changed) under another. `previous` is the hash the
/// groups were judged under.
let redefine (previous: string) (m: Monitor) (at: DateTimeOffset) (groups: Map<string, Group>) : Map<string, Group> * Transition list =
    if monitorHash m = previous then groups, [] else close Reason.DefinitionChanged at groups

/// The monitor's state, its most severe judged group; None when no group is
/// judged yet, which a multi alert shows as "no groups found".
let overall (groups: Map<string, Group>) : Status option =
    match groups.Values |> Seq.choose _.State |> List.ofSeq with
    | [] -> None
    | states -> Some(List.maxBy severity states)
