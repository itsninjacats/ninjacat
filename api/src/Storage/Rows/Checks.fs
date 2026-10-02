namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One service check result.
type CheckRunRow =
    { TenantID: string
      Timestamp: DateTime
      CheckName: string
      Host: string
      /// OK, WARNING, CRITICAL or UNKNOWN: the column is an Enum8.
      Status: string
      Message: string
      Tags: Map<string, string[]>
      /// Keys Datadog's ServiceCheck does not declare, values JSON-encoded.
      Extra: Map<string, string> }

module Checks =
    let table: Table<CheckRunRow> =
        { Table.create
              "storage_checks"
              "check_runs"
              [ "tenant_id"; "timestamp"; "check_name"; "host"; "status"; "message"; "tags"; "extra" ]
              (fun r -> [| r.TenantID; r.Timestamp; r.CheckName; r.Host; r.Status; r.Message; r.Tags; r.Extra |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 5.0 }
