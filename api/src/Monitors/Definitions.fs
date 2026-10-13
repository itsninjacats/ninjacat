/// The `monitor` table, read. The panel owns it (Drizzle, frontend/); the
/// service never writes it.
module NinjaCat.Api.Monitors.Definitions

open System
open System.Data.Common
open System.Threading
open System.Threading.Tasks
open Npgsql

/// A monitor's entity id, `<tenant>/<id>`. The tenant travels with the id,
/// so whatever an entity compiles carries it.
type MonitorKey =
    { Tenant: string
      Id: int64 }

    member k.Text = $"{k.Tenant}/{k.Id}"

    /// The id is the last segment, so a tenant id with a slash still parses.
    static member Parse(text: string) : MonitorKey option =
        match text.LastIndexOf '/' with
        | -1 -> None
        | slash ->
            match Int64.TryParse(text.AsSpan(slash + 1)) with
            | true, id when slash > 0 -> Some { Tenant = text.Substring(0, slash); Id = id }
            | _ -> None

type MonitorRow =
    { Key: MonitorKey
      Version: int
      Type: string
      Query: string
      /// The options object as JSON text.
      Options: string
      UpdatedBy: string option
      DeletedBy: string option
      DeletedAt: DateTimeOffset option }

let private columns =
    "tenant_id, id, version, type, query, options::text, updated_by_id, deleted_by_id, deleted_at"

let private optionalString (r: DbDataReader) (i: int) =
    if r.IsDBNull i then None else Some(r.GetString i)

let private rowOf (r: DbDataReader) : MonitorRow =
    { Key = { Tenant = r.GetString 0; Id = r.GetInt64 1 }
      Version = r.GetInt32 2
      Type = r.GetString 3
      Query = r.GetString 4
      Options = r.GetString 5
      UpdatedBy = optionalString r 6
      DeletedBy = optionalString r 7
      DeletedAt = if r.IsDBNull 8 then None else Some(r.GetFieldValue<DateTimeOffset> 8) }

let private readAll (command: NpgsqlCommand) (ct: CancellationToken) : Task<MonitorRow list> =
    task {
        use! reader = command.ExecuteReaderAsync ct
        let rows = ResizeArray()
        let mutable more = true

        while more do
            let! next = reader.ReadAsync ct
            if next then rows.Add(rowOf reader) else more <- false

        return List.ofSeq rows
    }

/// One monitor, deleted or not; None when there is no such row.
let byKey (db: NpgsqlDataSource) (key: MonitorKey) (ct: CancellationToken) : Task<MonitorRow option> =
    task {
        use command = db.CreateCommand $"SELECT {columns} FROM monitor WHERE tenant_id = $1 AND id = $2"
        command.Parameters.Add(NpgsqlParameter(Value = key.Tenant)) |> ignore
        command.Parameters.Add(NpgsqlParameter(Value = key.Id)) |> ignore
        let! rows = readAll command ct
        return List.tryHead rows
    }

/// Every monitor not deleted, of every tenant.
let live (db: NpgsqlDataSource) (ct: CancellationToken) : Task<MonitorRow list> =
    task {
        use command = db.CreateCommand $"SELECT {columns} FROM monitor WHERE deleted_at IS NULL ORDER BY tenant_id, id"
        return! readAll command ct
    }
