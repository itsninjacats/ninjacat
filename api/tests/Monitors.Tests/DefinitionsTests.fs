/// The `monitor` reader against the schema Drizzle migrated, so a change to
/// the table that breaks the reader fails here.
module NinjaCat.Api.Monitors.Tests.DefinitionsTests

open System
open System.Threading
open Xunit
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Monitors.Definitions
open NinjaCat.Api.Monitors.Tests.TestPostgres

[<Fact>]
let ``a monitor is read by its key, deleted or not`` () =
    withTenant (fun db tenant ->
        let query = "avg(last_5m):avg:system.cpu.user{*} > 90"
        let live = insert db tenant query "NULL"
        let deleted = insert db tenant query "now()"

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

let private ours (tenant: string) (seen: Seen list) : int64 list =
    seen |> List.filter (fun s -> s.Key.Tenant = tenant) |> List.map _.Key.Id

[<Fact>]
let ``a snapshot holds the live monitors and those deleted in the last day`` () =
    withTenant (fun db tenant ->
        let query = "avg(last_5m):avg:system.cpu.user{*} > 90"
        let live = insert db tenant query "NULL"
        let deleted = insert db tenant query "now()"
        insert db tenant query "now() - interval '2 days'" |> ignore

        let first = (snapshot db None CancellationToken.None).Result
        Assert.Equal<int64 list>([ live ], ours tenant first.Live)
        Assert.Equal<int64 list>([ deleted ], ours tenant first.Deleted)
        Assert.Equal(3, (first.Live |> List.find (fun s -> s.Key.Id = live)).Version)
        Assert.Equal(TimeSpan.Zero, first.At.Offset)

        // Deletions are counted from the time given.
        let earlier = (snapshot db (Some(first.At.AddHours -1.0)) CancellationToken.None).Result
        Assert.Equal<int64 list>([ deleted ], ours tenant earlier.Deleted)
        let later = (snapshot db (Some(first.At.AddSeconds 1.0)) CancellationToken.None).Result
        Assert.Empty(ours tenant later.Deleted)
        Assert.Equal<int64 list>([ live ], ours tenant later.Live))

[<Fact>]
let ``a notification reads back from the panel's payload`` () =
    Assert.Equal(Some { Key = { Tenant = "acme"; Id = 42L }; Version = 3 }, parseNotification "acme/42/3")
    Assert.Equal(Some { Key = { Tenant = "a/b"; Id = 42L }; Version = 3 }, parseNotification "a/b/42/3")
    Assert.Equal(None, parseNotification "acme/42")
    Assert.Equal(None, parseNotification "acme/42/x")
    Assert.Equal(None, parseNotification "")

[<Fact>]
let ``a key reads back from its text`` () =
    Assert.Equal(Some { Tenant = "acme"; Id = 42L }, MonitorKey.Parse "acme/42")
    Assert.Equal(Some { Tenant = "a/b"; Id = 42L }, MonitorKey.Parse "a/b/42")
    Assert.Equal(None, MonitorKey.Parse "acme")
    Assert.Equal(None, MonitorKey.Parse "/42")
    Assert.Equal(None, MonitorKey.Parse "acme/x")
