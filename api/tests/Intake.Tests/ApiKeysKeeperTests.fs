/// The keeper on a Postgres the panel has not migrated yet. Skipped when the
/// dev Postgres (DATABASE_URL, default the compose stack's) is down.
module NinjaCat.Api.Intake.Tests.ApiKeysKeeperTests

open System
open System.Threading
open Microsoft.Extensions.Logging.Abstractions
open Npgsql
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Platform
open NinjaCat.Api.Server

[<Fact>]
let ``starts with no keys when there is no api_key table yet`` () =
    let url =
        match Environment.GetEnvironmentVariable "DATABASE_URL" with
        | null
        | "" -> "postgres://root:mysecretpassword@localhost:5433/local"
        | value -> value

    // A schema that does not exist hides every table, the way a fresh database has none.
    let connection =
        NpgsqlConnectionStringBuilder(Postgres.connectionString url, SearchPath = "no_such_schema")

    use db = NpgsqlDataSource.Create connection.ConnectionString

    if not ((Postgres.ping db CancellationToken.None).GetAwaiter().GetResult()) then
        Assert.Skip "Postgres is not reachable"

    let store = ApiKeys.Store()
    use keeper = new ApiKeysKeeper(db, store, NullLogger<ApiKeysKeeper>.Instance)
    keeper.FetchNow()
    Assert.Equal(0, store.Current.ByHash.Count)
