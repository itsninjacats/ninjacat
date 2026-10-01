namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One `agent flare` upload: the archive and the form fields sent with it.
type AgentFlareRow =
    { TenantID: string
      ReceivedAt: DateTime
      Hostname: string
      CaseID: string
      Email: string
      Source: string
      AgentVersion: string
      Filename: string
      SizeBytes: uint64
      /// Form fields without a column of their own.
      Fields: Map<string, string>
      Archive: byte[] }

module AgentFlares =
    let table: Table<AgentFlareRow> =
        { Table.create
              "storage_agent_flares"
              "agent_flares"
              [ "tenant_id"; "received_at"; "hostname"; "case_id"; "email"; "source"; "agent_version"
                "filename"; "size_bytes"; "fields"; "archive" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.Hostname; r.CaseID; r.Email; r.Source; r.AgentVersion
                     r.Filename; r.SizeBytes; r.Fields; r.Archive |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }
