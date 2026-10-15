namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

type LogRow =
    { TenantID: string
      Timestamp: DateTime
      Host: string
      Service: string
      Source: string
      Status: string
      Message: string
      Tags: Map<string, string[]>
      /// Everything that did not become a column, as JSON text.
      Attributes: string
      /// Which wire field the timestamp came from, or "arrival".
      TimestampSource: string }

module Logs =
    let table: Table<LogRow> =
        { Table.create
              "storage_logs"
              "logs"
              [ "tenant_id"; "timestamp"; "host"; "service"; "source"; "status"; "message"; "tags"
                "attributes"; "timestamp_source" ]
              (fun r ->
                  [| r.TenantID; r.Timestamp; r.Host; r.Service; r.Source; r.Status; r.Message; r.Tags
                     Col.jsonObject r.Attributes; r.TimestampSource |])
          with
              MaxRows = 20000
              FlushInterval = TimeSpan.FromSeconds 1.0
              BufferLimit = 500_000 }
