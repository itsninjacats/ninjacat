/// A service check monitor's SQL against real `check_runs` rows, in a scratch database
/// (TestClickHouse).
module NinjaCat.Api.Monitors.Tests.CheckTests

open System
open System.Threading
open ClickHouse.Driver
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Monitor
open NinjaCat.Api.Engine.Monitor.Definition
open NinjaCat.Api.Engine.Monitor.Evaluate
open NinjaCat.Api.Platform
open NinjaCat.Api.Monitors.Tests.TestClickHouse

let private now = DateTimeOffset.UtcNow
let private at = DateTimeOffset(now.Year, now.Month, now.Day, now.Hour, 0, 0, TimeSpan.Zero)

/// One run `secondsAgo` before `at`; tags as ClickHouse's map literal.
let private run (tenant: string) (secondsAgo: int) (host: string) (status: string) (message: string) (tags: string) =
    $"('{tenant}', toDateTime({at.ToUnixTimeSeconds() - int64 secondsAgo}), 'c', '{host}', '{status}', '{message}', {tags})"

let private insert (client: ClickHouseClient) (runs: string list) =
    let values = String.concat ", " runs

    client.ExecuteNonQueryAsync($"INSERT INTO check_runs (tenant_id, timestamp, check_name, host, status, message, tags) VALUES {values}").GetAwaiter().GetResult()
    |> ignore

let private seeded (client: ClickHouseClient) =
    let prod = "map('env', ['prod'])"

    insert
        client
        [ run "acme" 10 "h1" "CRITICAL" "down" prod
          run "acme" 25 "h1" "CRITICAL" "down" prod
          run "acme" 40 "h1" "OK" "up" prod
          run "acme" 55 "h1" "OK" "up" prod
          run "acme" 10 "h2" "OK" "up" "map('env', ['dev'])"
          run "acme" 10 "" "UNKNOWN" "no answer" "map('kube_namespace', ['ns'], 'cluster', [''])"
          // Newest of all, the same host and tags, another tenant's.
          run "other" 5 "h1" "WARNING" "slow" prod ]

let private orFail (result: Result<'T, string list>) : 'T =
    match result with
    | Ok v -> v
    | Error problems -> failwith (String.concat "\n" problems)

let private evaluated (client: ClickHouseClient) (query: string) =
    let c = Check.definition query "" |> orFail

    match (Check.evaluate (ClickHouse.metricRows client CancellationToken.None) (TenantId "acme") 100 c at).GetAwaiter().GetResult() with
    | Ok e ->
        [ for g in e.Groups ->
              match g.Reading with
              | Streaks s -> g.Key, s
              | other -> failwith $"{other}" ]
    | Error e -> failwith e

[<Fact>]
let ``the newest runs of the tenant's matching series, statuses as the agent sends them`` () =
    inScratch (fun client ->
        seeded client

        Assert.Equal<(string * Streak list) list>(
            [ "host:h1", [ { Latest = 2; Critical = 2; Warning = 0; Ok = 0; Unknown = 0; OkSinceUnknown = 0 } ] ],
            evaluated client """"c".over("env:prod").by("host").last(3).count_by_status()"""
        )

        Assert.Equal<(string * Streak list) list>(
            [ "host:h1", [ { Latest = 2; Critical = 2; Warning = 0; Ok = 0; Unknown = 0; OkSinceUnknown = 0 } ] ],
            evaluated client """"c".over("*").exclude("env:dev").by("host").last(2).count_by_status()"""
        ))

[<Fact>]
let ``a run with no host is N/A by host, and grouped by its tags`` () =
    inScratch (fun client ->
        seeded client

        Assert.Equal<(string * Streak list) list>(
            [ "kube_namespace:ns", [ { Latest = 3; Critical = 0; Warning = 0; Ok = 0; Unknown = 1; OkSinceUnknown = 0 } ] ],
            evaluated client """"c".over("*").by("kube_namespace").last(2).count_by_status()"""
        ))

[<Fact>]
let ``by star is a group per series, and a run stored twice counts once`` () =
    inScratch (fun client ->
        seeded client
        insert client [ run "acme" 10 "h1" "CRITICAL" "down" "map('env', ['prod'])" ]

        Assert.Equal<(string * Streak list) list>(
            [ "cluster,kube_namespace:ns", [ { Latest = 3; Critical = 0; Warning = 0; Ok = 0; Unknown = 1; OkSinceUnknown = 0 } ]
              "env:dev,host:h2", [ { Latest = 0; Critical = 0; Warning = 0; Ok = 1; Unknown = 0; OkSinceUnknown = 1 } ]
              "env:prod,host:h1", [ { Latest = 2; Critical = 2; Warning = 0; Ok = 0; Unknown = 0; OkSinceUnknown = 0 } ] ],
            evaluated client """"c".by("*").last(2).count_by_status()""" |> List.sort
        ))

[<Fact>]
let ``N known runs of each series are read past UNKNOWN ones, and the newest UNKNOWN in a row`` () =
    inScratch (fun client ->
        let prod = "map('env', ['prod'])"
        let statuses = [ "UNKNOWN"; "UNKNOWN"; "CRITICAL"; "UNKNOWN"; "CRITICAL"; "UNKNOWN"; "CRITICAL"; "OK"; "OK" ]
        insert client (statuses |> List.mapi (fun i status -> run "acme" (10 + 15 * i) "h1" status "" prod))

        let streak (last: int) =
            match evaluated client $""""c".by("host").last({last}).count_by_status()""" with
            | [ "host:h1", [ s ] ] -> s
            | other -> failwith $"{other}"

        let q = Check.query """"c".last(3).count_by_status()""" |> orFail

        let rows =
            (ClickHouse.metricRows client CancellationToken.None (Check.sql (TenantId "acme") q { From = at.AddHours -2.0; To = at }))
                .GetAwaiter()
                .GetResult()

        // last(3): three CRITICAL runs past the UNKNOWN ones, and three of the four UNKNOWN.
        Assert.Equal<float list>([ 3.0; 3.0; 3.0; 2.0; 2.0; 2.0 ], rows |> List.map _.Value |> List.sortDescending)
        Assert.Equal({ Latest = 3; Critical = 3; Warning = 0; Ok = 0; Unknown = 2; OkSinceUnknown = 0 }, streak 3)
        Assert.Equal({ Latest = 3; Critical = 1; Warning = 0; Ok = 0; Unknown = 2; OkSinceUnknown = 0 }, streak 1))

[<Fact>]
let ``the check's runs come with their messages and tags`` () =
    inScratch (fun client ->
        seeded client

        let q = Check.query """"c".over("*").exclude("env:prod").last(1).count_by_status()""" |> orFail

        let rows =
            ClickHouse.query
                client
                CancellationToken.None
                (fun r -> r.GetInt64 0, r.GetString 1, r.GetString 2, r.GetString 3, (r.GetValue 4 :?> string[]) |> List.ofArray |> List.sort)
                (Check.runs (TenantId "acme") q (at.AddHours -1.0) at)
            |> _.GetAwaiter().GetResult()

        let at10 = (at.ToUnixTimeSeconds() - 10L) * 1000L

        Assert.Equal<(int64 * string * string * string * string list) list>(
            [ at10, "h2", "OK", "up", [ "env:dev" ]; at10, "", "UNKNOWN", "no answer", [ "cluster"; "kube_namespace:ns" ] ]
            |> List.sortBy (fun (_, host, _, _, _) -> host),
            rows |> List.sortBy (fun (_, host, _, _, _) -> host)
        ))
