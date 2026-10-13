namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One group of a monitor moving from one state to another, with the
/// thresholds it was judged by: Postgres keeps only the latest definition.
type MonitorTransitionRow =
    { TenantID: string
      MonitorID: uint64
      MonitorVersion: uint32
      QueryHash: string
      MonitorKind: string
      GroupKey: string
      GroupTags: string[]
      At: DateTime
      /// "none" for a group seen for the first time.
      FromState: string
      ToState: string
      Reason: string
      Actor: string option
      Value: float option
      /// "none" when no threshold was crossed.
      Crossed: string
      Comparator: string
      Critical: float option
      Warning: float option
      CriticalRecovery: float option
      WarningRecovery: float option
      WindowFrom: DateTime
      WindowTo: DateTime
      FirstTriggeredAt: DateTime option
      LastTriggeredAt: DateTime option }

type MonitorEvaluationRow =
    { TenantID: string
      MonitorID: uint64
      GroupKey: string
      At: DateTime
      Value: float option
      State: string
      /// evaluated, skipped or error.
      Outcome: string
      Detail: string }

module MonitorTransitions =
    /// Written with `ClickHouseSink.InsertAsync`, never through the buffered
    /// writer: a transition must be stored, and the writer drops a failed batch.
    /// `inserted_at` is left to its default, the notifier's cursor.
    let table: Table<MonitorTransitionRow> =
        Table.create
            "storage_monitor_transitions"
            "monitor_transitions"
            [ "tenant_id"; "monitor_id"; "monitor_version"; "query_hash"; "monitor_kind"; "group_key"; "group_tags"; "at"
              "from_state"; "to_state"; "reason"; "actor"; "value"; "crossed"; "comparator"
              "critical"; "warning"; "critical_recovery"; "warning_recovery"
              "window_from"; "window_to"; "first_triggered_at"; "last_triggered_at" ]
            (fun r ->
                [| r.TenantID; r.MonitorID; r.MonitorVersion; r.QueryHash; r.MonitorKind; r.GroupKey; r.GroupTags; r.At
                   r.FromState; r.ToState; r.Reason; Col.opt r.Actor; Col.opt r.Value; r.Crossed; r.Comparator
                   Col.opt r.Critical; Col.opt r.Warning; Col.opt r.CriticalRecovery; Col.opt r.WarningRecovery
                   r.WindowFrom; r.WindowTo; Col.opt r.FirstTriggeredAt; Col.opt r.LastTriggeredAt |])

module MonitorEvaluations =
    let table: Table<MonitorEvaluationRow> =
        Table.create
            "storage_monitor_evaluations"
            "monitor_evaluations"
            [ "tenant_id"; "monitor_id"; "group_key"; "at"; "value"; "state"; "outcome"; "detail" ]
            (fun r -> [| r.TenantID; r.MonitorID; r.GroupKey; r.At; Col.opt r.Value; r.State; r.Outcome; r.Detail |])
