namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// A request that could not be turned into rows, kept as it arrived.
type RawPayloadRow =
    { TenantID: string
      ReceivedAt: DateTime
      Intake: string
      Reason: string
      Method: string
      Host: string
      Path: string
      Query: Map<string, string[]>
      ContentType: string
      ContentEncoding: string
      Headers: Map<string, string>
      Body: byte[]
      BodyBytes: uint64
      Note: string }

module RawPayloads =
    let table: Table<RawPayloadRow> =
        { Table.create
              "storage_raw_payloads"
              "raw_payloads"
              [ "tenant_id"; "received_at"; "intake"; "reason"; "method"; "host"; "path"; "query"
                "content_type"; "content_encoding"; "headers"; "body"; "body_bytes"; "note" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.Intake; r.Reason; r.Method; r.Host; r.Path; r.Query
                     r.ContentType; r.ContentEncoding; r.Headers; r.Body; r.BodyBytes; r.Note |])
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }
