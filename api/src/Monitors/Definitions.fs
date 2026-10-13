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

/// A monitor and the version a read of the table or a notification saw.
type Seen = { Key: MonitorKey; Version: int }

/// What one resync read.
type Snapshot =
    { /// Postgres's clock before the rows were read. The next resync counts
      /// deletions from it, so no clock of ours is compared with `deleted_at`.
      At: DateTimeOffset
      Live: Seen list
      Deleted: Seen list }

/// Every live monitor, and those deleted after `deletedSince` (None: in the
/// last day).
let snapshot (db: NpgsqlDataSource) (deletedSince: DateTimeOffset option) (ct: CancellationToken) : Task<Snapshot> =
    task {
        use! connection = db.OpenConnectionAsync ct
        use clock = new NpgsqlCommand("SELECT now()", connection)
        let! now = clock.ExecuteScalarAsync ct
        // Npgsql reads a timestamptz as a UTC DateTime.
        let at = DateTimeOffset(now :?> DateTime)

        use command =
            new NpgsqlCommand(
                "SELECT tenant_id, id, version, deleted_at IS NOT NULL FROM monitor "
                + "WHERE deleted_at IS NULL OR deleted_at > coalesce($1, now() - interval '1 day')",
                connection
            )

        let since =
            match deletedSince with
            | Some t -> NpgsqlParameter(Value = t.ToUniversalTime())
            | None -> NpgsqlParameter(Value = DBNull.Value, NpgsqlDbType = NpgsqlTypes.NpgsqlDbType.TimestampTz)

        command.Parameters.Add since |> ignore
        use! reader = command.ExecuteReaderAsync ct
        let live = ResizeArray()
        let deleted = ResizeArray()
        let mutable more = true

        while more do
            let! next = reader.ReadAsync ct

            if next then
                let seen =
                    { Key = { Tenant = reader.GetString 0; Id = reader.GetInt64 1 }
                      Version = reader.GetInt32 2 }

                if reader.GetBoolean 3 then deleted.Add seen else live.Add seen
            else
                more <- false

        return
            { At = at
              Live = List.ofSeq live
              Deleted = List.ofSeq deleted }
    }

/// The channel the panel notifies on (frontend/src/lib/server/monitors.ts).
let channel = "ninjacat_monitors"

/// A notification's payload, `<tenant>/<id>/<version>`.
let parseNotification (payload: string) : Seen option =
    match payload.LastIndexOf '/' with
    | -1 -> None
    | slash ->
        match Int32.TryParse(payload.AsSpan(slash + 1)), MonitorKey.Parse(payload.Substring(0, slash)) with
        | (true, version), Some key -> Some { Key = key; Version = version }
        | _ -> None

/// LISTENs on `channel` on its own connection, since LISTEN belongs to a
/// session, and hands every payload to `notified` until the connection fails
/// or `ct` is cancelled. Only `db`'s keepalive notices a link that died
/// without closing.
let listen (db: NpgsqlDataSource) (channel: string) (notified: string -> unit) (ct: CancellationToken) : Task =
    task {
        use! connection = db.OpenConnectionAsync ct
        connection.Notification.Add(fun e -> notified e.Payload)
        use command = new NpgsqlCommand($"LISTEN {channel}", connection)
        let! _ = command.ExecuteNonQueryAsync ct

        while true do
            do! connection.WaitAsync ct
    }
