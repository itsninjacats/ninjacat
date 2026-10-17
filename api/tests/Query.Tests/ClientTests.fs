/// The write client's `async_insert`. Skipped when there is no ClickHouse.
module NinjaCat.Api.Query.Tests.ClientTests

open System.Threading
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Platform
open NinjaCat.Api.Query.Tests.TestServer

[<Fact>]
let ``writes are synchronous`` () =
    use client = ClickHouse.createWriteClient { config with ClickHouseDb = "default" }

    try
        client.PingAsync().GetAwaiter().GetResult() |> ignore
    with _ ->
        Assert.Skip "ClickHouse is not reachable"

    let setting =
        { Text = "SELECT toString(getSetting('async_insert'))"; Parameters = [] }
        |> ClickHouse.query client CancellationToken.None _.GetString(0)
        |> fun t -> t.GetAwaiter().GetResult()

    Assert.Equal<string list>([ "false" ], setting)
