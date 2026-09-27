module NinjaCat.Api.Engine.Tests.PlanTests

open System
open Xunit
open NinjaCat.Api.Engine.Api.V2.Timeseries
open NinjaCat.Api.Engine.Query.Plan

let private request (queries: (string option * string) list) (formulas: string list) =
    let parse (body: string) =
        match read body with
        | Ok r -> r
        | Error es -> failwith (String.Join("\n", es))

    let qs =
        queries
        |> List.map (fun (name, q) ->
            let n = name |> Option.map (sprintf ",\"name\":\"%s\"") |> Option.defaultValue ""
            $"""{{"data_source":"metrics","query":"{q}"{n}}}""")
        |> String.concat ","

    let fs = formulas |> List.map (sprintf "{\"formula\":\"%s\"}") |> String.concat ","

    parse
        $"""{{"data":{{"type":"timeseries_request","attributes":{{"from":1711977600000,"to":1711981200000,"queries":[{qs}],"formulas":[{fs}]}}}}}}"""

let private planOf queries formulas =
    match plan (request queries formulas) with
    | Ok p -> p
    | Error es -> failwith (String.Join("\n", es))

let private errorsOf queries formulas =
    match plan (request queries formulas) with
    | Ok p -> failwith $"expected errors, got {p}"
    | Error es -> es

// --- the step ---------------------------------------------------------------------

/// Every row of Datadog's table for line graphs (docs: dashboards/functions/rollup).
[<Theory>]
[<InlineData(60.0, 1.0)>] // past minute → 1s
[<InlineData(300.0, 2.0)>] // past 5 minutes → 2s
[<InlineData(900.0, 5.0)>] // past 15 minutes → 5s
[<InlineData(1800.0, 10.0)>] // past 30 minutes → 10s
[<InlineData(3600.0, 20.0)>] // past hour → 20s
[<InlineData(14400.0, 60.0)>] // past 4 hours → 1m
[<InlineData(86400.0, 300.0)>] // past day → 5m
[<InlineData(604800.0, 3600.0)>] // past week → 1h
[<InlineData(2592000.0, 14400.0)>] // past month (30 days) → 4h
[<InlineData(31536000.0, 86400.0)>] // past year → 1d
let ``default step follows Datadog's table`` (windowSeconds: float, stepSeconds: float) =
    Assert.Equal(TimeSpan.FromSeconds stepSeconds, step (TimeSpan.FromSeconds windowSeconds) None)

[<Fact>]
let ``an interval hint is honoured`` () =
    // The recorded Datadog request: one hour, interval 5000 ms.
    Assert.Equal(TimeSpan.FromSeconds 5.0, step (TimeSpan.FromHours 1.0) (Some(TimeSpan.FromMilliseconds 5000.0)))

[<Fact>]
let ``a hint that would exceed 1500 points is widened`` () =
    // A day at 1 s would be 86 400 points; 86 400 / 1500 = 57.6 → 58 s.
    Assert.Equal(TimeSpan.FromSeconds 58.0, step (TimeSpan.FromDays 1.0) (Some(TimeSpan.FromSeconds 1.0)))

[<Fact>]
let ``a sub-second hint is rounded up to a second`` () =
    Assert.Equal(TimeSpan.FromSeconds 1.0, step (TimeSpan.FromMinutes 1.0) (Some(TimeSpan.FromMilliseconds 200.0)))

// --- queries ------------------------------------------------------------------------

[<Fact>]
let ``aggregator defaults to avg`` () =
    let p = planOf [ None, "system.cpu.user{*}" ] []
    Assert.Equal(Avg, (Assert.Single p.Queries).SpaceAgg)

[<Fact>]
let ``aggregator, filter and group by are carried over`` () =
    let q = Assert.Single (planOf [ None, "sum:system.cpu.user{env:prod} by {host,role}" ] []).Queries
    Assert.Equal(Sum, q.SpaceAgg)
    Assert.Equal("system.cpu.user", q.Metric)
    Assert.Equal<string list>([ "host"; "role" ], q.GroupBy)

[<Fact>]
let ``what is not supported yet is named`` () =
    let es = errorsOf [ Some "a", "p95:x{*}.fill(zero)" ] []
    Assert.Contains("queries[0] (a): aggregator 'p95:' is not supported yet", es)
    Assert.Contains("queries[0] (a): .fill() is not supported yet", es)

[<Fact>]
let ``unsubstituted template variables are refused`` () =
    let es = errorsOf [ None, "x{$env}" ] []
    Assert.Contains("queries[0]: template variable '$env' was not substituted", es)

// --- outputs ------------------------------------------------------------------------

[<Fact>]
let ``no formulas: every query is an output`` () =
    let p = planOf [ Some "a", "x{*}"; Some "b", "y{*}" ] []
    Assert.Equal<int list>([ 0; 1 ], p.Outputs |> List.map _.QueryIndex)
    Assert.Equal<int list>([ 0; 1 ], p.Outputs |> List.map _.Source)

[<Fact>]
let ``formulas name their query and set query_index`` () =
    let p = planOf [ Some "a", "x{*}"; Some "b", "y{*}" ] [ "b" ]
    let o = Assert.Single p.Outputs
    Assert.Equal(0, o.QueryIndex)
    Assert.Equal(1, o.Source)

[<Fact>]
let ``a formula naming nothing is refused`` () =
    Assert.Contains("formulas[0]: no query is named 'c'", errorsOf [ Some "a", "x{*}" ] [ "c" ])

[<Fact>]
let ``arithmetic in formulas is not supported yet`` () =
    let es = errorsOf [ Some "a", "x{*}"; Some "b", "y{*}" ] [ "a / b" ]
    Assert.Contains("formulas[0]: only a single query name is supported yet, e.g. \"query1\"", es)

[<Fact>]
let ``duplicate query names are refused`` () =
    Assert.Contains("query name 'a' is used more than once", errorsOf [ Some "a", "x{*}"; Some "a", "y{*}" ] [])

// --- rollup and type modifiers -------------------------------------------------------

let private onlyQuery q = Assert.Single (planOf [ None, q ] []).Queries

[<Fact>]
let ``no modifier: avg over the request's step`` () =
    let q = onlyQuery "sum:requests{*}"
    Assert.Equal(Plain Avg, q.TimeAgg)
    Assert.Equal(TimeSpan.FromSeconds 20.0, q.Step) // one hour → 20 s

[<Fact>]
let ``rollup with a method keeps the step`` () =
    let q = onlyQuery "x{*}.rollup(max)"
    Assert.Equal(Plain Max, q.TimeAgg)
    Assert.Equal(TimeSpan.FromSeconds 20.0, q.Step)

[<Fact>]
let ``rollup with an interval sets this query's step`` () =
    let q = onlyQuery "x{*}.rollup(sum, 60)"
    Assert.Equal(Plain Sum, q.TimeAgg)
    Assert.Equal(TimeSpan.FromSeconds 60.0, q.Step)

[<Fact>]
let ``rollup count is a method`` () = Assert.Equal(Plain Count, (onlyQuery "x{*}.rollup(count, 60)").TimeAgg)

[<Fact>]
let ``a rollup interval over 1500 points is widened`` () =
    // One hour at 1 s is 3600 points; 3600 / 1500 = 2.4 → 3 s.
    Assert.Equal(TimeSpan.FromSeconds 3.0, (onlyQuery "x{*}.rollup(avg, 1)").Step)

[<Fact>]
let ``as_count and as_rate`` () =
    Assert.Equal(AsCount, (onlyQuery "sum:x{*}.as_count()").TimeAgg)
    Assert.Equal(AsRate, (onlyQuery "sum:x{*}.as_rate()").TimeAgg)

[<Fact>]
let ``as_count with a sum rollup, in either order`` () =
    // Datadog docs: sum:requests.count{*}.as_count().rollup(sum, …)
    let q = onlyQuery "sum:requests{*}.as_count().rollup(sum, 120)"
    Assert.Equal(AsCount, q.TimeAgg)
    Assert.Equal(TimeSpan.FromSeconds 120.0, q.Step)
    Assert.Equal(AsCount, (onlyQuery "sum:requests{*}.rollup(sum, 120).as_count()").TimeAgg)

[<Fact>]
let ``contradictions and unsupported forms are named`` () =
    Assert.Contains("queries[0]: .as_count() with .rollup(max) is not supported yet", errorsOf [ None, "x{*}.as_count().rollup(max)" ] [])
    Assert.Contains("queries[0]: .as_rate() cannot follow .as_count()", errorsOf [ None, "x{*}.as_count().as_rate()" ] [])
    Assert.Contains("queries[0]: .rollup() method must be one of avg, sum, min, max, count", errorsOf [ None, "x{*}.rollup(median)" ] [])
    Assert.Contains(
        "queries[0]: calendar .rollup() (daily, weekly, monthly, alignment, timezone) is not supported yet",
        errorsOf [ None, "x{*}.rollup(sum, monthly)" ] []
    )
