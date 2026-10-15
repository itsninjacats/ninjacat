/// The dev Postgres (DATABASE_URL, default the compose stack's), for tests
/// that read the migrated `monitor` table. A test is skipped when there is
/// none, and works under a tenant of its own, removed afterwards.
module NinjaCat.Api.Monitors.Tests.TestPostgres

open System
open System.Threading
open Npgsql
open Xunit
open NinjaCat.Api.Platform

let url =
    match Environment.GetEnvironmentVariable "DATABASE_URL" with
    | null
    | "" -> "postgres://root:mysecretpassword@localhost:5433/local"
    | value -> value

let execute (db: NpgsqlDataSource) (sql: string) (parameters: obj list) : obj =
    use command = db.CreateCommand sql

    for value in parameters do
        command.Parameters.Add(NpgsqlParameter(Value = value)) |> ignore

    command.ExecuteScalar()

let withTenant (test: NpgsqlDataSource -> string -> unit) : unit =
    use db = NpgsqlDataSource.Create(Postgres.connectionString url)

    if not ((Postgres.ping db CancellationToken.None).GetAwaiter().GetResult()) then
        Assert.Skip "Postgres is not reachable"

    let tenant = "monitors-test-" + Guid.NewGuid().ToString "N"
    execute db "INSERT INTO organization (id, name, slug, created_at) VALUES ($1, $1, $1, now())" [ tenant ] |> ignore

    try
        test db tenant
    finally
        execute db "DELETE FROM monitor WHERE tenant_id = $1" [ tenant ] |> ignore
        execute db "DELETE FROM organization WHERE id = $1" [ tenant ] |> ignore

/// `deletedAt` is SQL: NULL, now(), …
let insert (db: NpgsqlDataSource) (tenant: string) (query: string) (deletedAt: string) : int64 =
    let sql =
        "INSERT INTO monitor (tenant_id, name, type, query, options, version, deleted_at) "
        + "VALUES ($1, 'cpu', 'query alert', $2, '{\"thresholds\": {\"critical\": 90}}'::jsonb, 3, "
        + deletedAt
        + ") RETURNING id"

    execute db sql [ tenant; query ] :?> int64
