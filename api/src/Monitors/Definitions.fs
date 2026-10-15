/// The `monitor` table, read only: the panel owns it.
module NinjaCat.Api.Monitors.Definitions

open System
open System.Data.Common
open System.Threading
open System.Threading.Tasks
open Npgsql

/// The entity id, `<tenant>/<id>`.
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
      Options: string
      UpdatedBy: string option
      DeletedBy: string option
      DeletedAt: DateTimeOffset option }

let private optionalString (r: DbDataReader) (i: int) =
    if r.IsDBNull i then None else Some(r.GetString i)

let byKey (db: NpgsqlDataSource) (key: MonitorKey) (ct: CancellationToken) : Task<MonitorRow option> =
    task {
        use command =
            db.CreateCommand
                "SELECT version, type, query, options::text, updated_by_id, deleted_by_id, deleted_at FROM monitor WHERE tenant_id = $1 AND id = $2"

        command.Parameters.Add(NpgsqlParameter(Value = key.Tenant)) |> ignore
        command.Parameters.Add(NpgsqlParameter(Value = key.Id)) |> ignore
        use! r = command.ExecuteReaderAsync ct
        let! found = r.ReadAsync ct

        if not found then
            return None
        else
            return
                Some
                    { Key = key
                      Version = r.GetInt32 0
                      Type = r.GetString 1
                      Query = r.GetString 2
                      Options = r.GetString 3
                      UpdatedBy = optionalString r 4
                      DeletedBy = optionalString r 5
                      DeletedAt = if r.IsDBNull 6 then None else Some(r.GetFieldValue<DateTimeOffset> 6) }
    }

type Seen = { Key: MonitorKey; Version: int }

type Snapshot =
    { /// Postgres's clock, so no clock of ours is compared with `deleted_at`.
      At: DateTimeOffset
      Live: Seen list
      Deleted: Seen list }

/// Every live monitor, and those deleted after `deletedSince` (None: in the last day).
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

/// frontend/src/lib/server/monitors.ts notifies on it.
let channel = "ninjacat_monitors"

/// `<tenant>/<id>/<version>`
let parseNotification (payload: string) : Seen option =
    match payload.LastIndexOf '/' with
    | -1 -> None
    | slash ->
        match Int32.TryParse(payload.AsSpan(slash + 1)), MonitorKey.Parse(payload.Substring(0, slash)) with
        | (true, version), Some key -> Some { Key = key; Version = version }
        | _ -> None

/// Its own connection, since LISTEN belongs to a session. Only `db`'s keepalive
/// notices a link that died without closing.
let listen (db: NpgsqlDataSource) (channel: string) (notified: string -> unit) (ct: CancellationToken) : Task =
    task {
        use! connection = db.OpenConnectionAsync ct
        connection.Notification.Add(fun e -> notified e.Payload)
        use command = new NpgsqlCommand($"LISTEN {channel}", connection)
        let! _ = command.ExecuteNonQueryAsync ct

        while true do
            do! connection.WaitAsync ct
    }
