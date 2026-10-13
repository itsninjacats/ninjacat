/// The `monitor` reader against the schema Drizzle migrated, so a change to
/// the table that breaks the reader fails here.
///
/// Needs the dev Postgres (DATABASE_URL, default the compose stack's);
/// skipped when there is none. Works under a tenant of its own, removed
/// afterwards.
module NinjaCat.Api.Monitors.Tests.DefinitionsTests

open System
open System.Threading
open Npgsql
open Xunit
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Platform
open NinjaCat.Api.Monitors.Definitions

let private url =
    match Environment.GetEnvironmentVariable "DATABASE_URL" with
    | null
    | "" -> "postgres://root:mysecretpassword@localhost:5433/local"
    | value -> value

let private execute (db: NpgsqlDataSource) (sql: string) (parameters: obj list) : obj =
    use command = db.CreateCommand sql

    for value in parameters do
        command.Parameters.Add(NpgsqlParameter(Value = value)) |> ignore

    command.ExecuteScalar()

let private withTenant (test: NpgsqlDataSource -> string -> unit) : unit =
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

let private insert (db: NpgsqlDataSource) (tenant: string) (query: string) (deleted: bool) : int64 =
    let sql =
        "INSERT INTO monitor (tenant_id, name, type, query, options, version, deleted_at) "
        + "VALUES ($1, 'cpu', 'query alert', $2, '{\"thresholds\": {\"critical\": 90}}'::jsonb, 3, "
        + (if deleted then "now()" else "NULL")
        + ") RETURNING id"

    execute db sql [ tenant; query ] :?> int64

[<Fact>]
let ``a monitor is read by its key, deleted or not`` () =
    withTenant (fun db tenant ->
        let query = "avg(last_5m):avg:system.cpu.user{*} > 90"
        let live = insert db tenant query false
        let deleted = insert db tenant query true

        match (byKey db { Tenant = tenant; Id = live } CancellationToken.None).Result with
        | None -> Assert.Fail "the monitor was not read"
        | Some row ->
            Assert.Equal((tenant, live, 3, "query alert", query), (row.Key.Tenant, row.Key.Id, row.Version, row.Type, row.Query))
            Assert.Equal((None, None, None), (row.UpdatedBy, row.DeletedBy, row.DeletedAt))

            // What is read is what the engine takes.
            match Validate.definition Plan.defaultRetention DateTimeOffset.UtcNow row.Type row.Query row.Options with
            | Ok d -> Assert.Equal(Some 90.0, d.Options.Thresholds.Critical)
            | Error problems -> Assert.Fail(String.concat "; " problems)

        let gone = (byKey db { Tenant = tenant; Id = deleted } CancellationToken.None).Result
        Assert.True(gone.Value.DeletedAt.IsSome)

        // Another tenant's id is not this tenant's monitor.
        Assert.Equal(None, (byKey db { Tenant = "no-such-tenant"; Id = live } CancellationToken.None).Result))

[<Fact>]
let ``only monitors not deleted are live`` () =
    withTenant (fun db tenant ->
        let live' = insert db tenant "avg(last_5m):avg:system.cpu.user{*} > 90" false
        insert db tenant "avg(last_5m):avg:system.cpu.user{*} > 90" true |> ignore

        let ours = (live db CancellationToken.None).Result |> List.filter (fun r -> r.Key.Tenant = tenant)
        Assert.Equal<int64 list>([ live' ], ours |> List.map _.Key.Id))

[<Fact>]
let ``a key reads back from its text`` () =
    Assert.Equal(Some { Tenant = "acme"; Id = 42L }, MonitorKey.Parse "acme/42")
    Assert.Equal(Some { Tenant = "a/b"; Id = 42L }, MonitorKey.Parse "a/b/42")
    Assert.Equal(None, MonitorKey.Parse "acme")
    Assert.Equal(None, MonitorKey.Parse "/42")
    Assert.Equal(None, MonitorKey.Parse "acme/x")
