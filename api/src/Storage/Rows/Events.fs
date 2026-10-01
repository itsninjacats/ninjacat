namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One event: a deploy, a restart, a config change.
///
/// Two sources feed the table: the public /api/v1/events request and the
/// agent's own events on /intake/. They fill different subsets of it, and one
/// table spares every chart a UNION.
type EventRow =
    { TenantID: string
      Timestamp: DateTime
      EventID: Guid
      /// The numeric id the API handed back to the caller, who keeps it to
      /// build links.
      EventIDNum: uint64
      Title: string
      Text: string
      Host: string
      AlertType: string
      Priority: string
      AggregationKey: string
      SourceTypeName: string
      DeviceName: string
      Tags: Map<string, string[]>
      /// Only the /intake/ source sends it.
      EventType: string
      /// Datadog's ids start at 1, so "no parent" must not be stored as 0.
      RelatedEventID: int64 option
      /// What the sender wrote, before the coercion that fills AlertType and
      /// Priority; "" when nothing was sent.
      AlertTypeRaw: string
      PriorityRaw: string
      /// Undeclared keys, values JSON-encoded.
      Extra: Map<string, string> }

module Events =
    let table: Table<EventRow> =
        { Table.create
              "storage_events"
              "events"
              [ "tenant_id"; "timestamp"; "event_id"; "event_id_num"; "title"; "text"; "host"
                "alert_type"; "priority"; "aggregation_key"; "source_type_name"; "device_name"; "tags"
                "event_type"; "related_event_id"; "alert_type_raw"; "priority_raw"; "extra" ]
              (fun r ->
                  [| r.TenantID; r.Timestamp; r.EventID; r.EventIDNum; r.Title; r.Text; r.Host
                     r.AlertType; r.Priority; r.AggregationKey; r.SourceTypeName; r.DeviceName; r.Tags
                     r.EventType; Col.opt r.RelatedEventID; r.AlertTypeRaw; r.PriorityRaw; r.Extra |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 2.0 }
