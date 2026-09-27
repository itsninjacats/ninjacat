/// The engine as F# code calls it: strings in, records out.
module NinjaCat.Api.Engine.Tests.EngineTests

open System
open System.Threading.Tasks
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Api.V2.Scalar
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Query.Compile
open NinjaCat.Api.Engine.Query.Engine

let private t0 = DateTimeOffset.FromUnixTimeMilliseconds 0L

/// Query `cpu`: host a at 10, 20, 30 and host b at 1, 2, 3, every 20 s; one
/// series of host c without a `role` tag for grouped queries.
let private execute (sql: Sql) =
    let groups = sql.Text.Split("AS g").Length - 1
    let row g t v = { Groups = g; BucketMs = t; Value = v; SeriesId = None }

    Task.FromResult
        [ for t, v in [ 0L, 1.0; 20000L, 2.0; 40000L, 3.0 ] do
              row (if groups = 0 then [] else [ Some "a" ]) t (v * 10.0)
              if groups > 0 then row [ Some "b" ] t v ]

let private run request =
    match (timeseries execute (TenantId "t") request).Result with
    | Ok r -> r
    | Error es -> failwith (String.Join("\n", es))

[<Fact>]
let ``a query in, series with tags and points out`` () =
    let result =
        run
            { From = t0; To = t0.AddSeconds 60.0; Interval = Some(TimeSpan.FromSeconds 20.0)
              Queries = [ { Name = "cpu"; Query = "avg:cpu{*} by {host}.fill(null)" } ]
              Formulas = [] }

    let cpu = Assert.Single result
    Assert.Equal("cpu", cpu.Formula)
    Assert.Equal<Tag list list>([ [ { Key = "host"; Value = Some "a" } ]; [ { Key = "host"; Value = Some "b" } ] ], cpu.Series |> List.map _.Tags)
    let a = cpu.Series.Head
    Assert.Equal<float list>([ 10.0; 20.0; 30.0 ], a.Points |> List.map _.Value)
    Assert.Equal(t0.AddSeconds 20.0, a.Points[1].Time)
    Assert.Empty a.Forecast
    Assert.True(a.Points |> List.forall (fun p -> p.Expected.IsNone))

[<Fact>]
let ``formulas label their results`` () =
    let result =
        run
            { From = t0; To = t0.AddSeconds 60.0; Interval = Some(TimeSpan.FromSeconds 20.0)
              Queries = [ { Name = "cpu"; Query = "avg:cpu{*} by {host}.fill(null)" } ]
              Formulas = [ "cpu * 2"; "top(cpu, 5, 'max', 'desc')" ] }

    Assert.Equal<string list>([ "cpu * 2"; "top(cpu, 5, 'max', 'desc')" ], result |> List.map _.Formula)
    Assert.Equal<float list>([ 20.0; 40.0; 60.0 ], result[0].Series.Head.Points |> List.map _.Value)

[<Fact>]
let ``problems come back named`` () =
    let result =
        (timeseries execute (TenantId "t")
            { From = t0; To = t0.AddSeconds 60.0; Interval = None
              Queries = [ { Name = "cpu"; Query = "avg:cpu{host:a" } ]
              Formulas = [ "nope" ] }).Result

    match result with
    | Ok r -> Assert.Fail $"expected errors, got {r}"
    | Error es ->
        Assert.Contains(es, fun e -> e.StartsWith "queries[0] (cpu):")

[<Fact>]
let ``a scalar is one value per group`` () =
    let result =
        (scalar execute (TenantId "t")
            { From = t0; To = t0.AddSeconds 60.0
              Queries = [ { Name = "cpu"; Query = "avg:cpu{*} by {host}.fill(null)"; Aggregator = ScalarMax } ]
              Formulas = [] }).Result

    match result with
    | Error es -> Assert.Fail(String.Join("\n", es))
    | Ok [ cpu ] ->
        Assert.Equal<(string option * float) list>([ Some "a", 30.0; Some "b", 3.0 ], cpu.Values |> List.map (fun v -> v.Tags.Head.Value, v.Value))
    | Ok other -> Assert.Fail $"one result expected, got {other}"
