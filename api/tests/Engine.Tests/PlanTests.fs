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
    Assert.Equal<Node list>([ Fetch 0; Fetch 1 ], p.Outputs |> List.map _.Node)

[<Fact>]
let ``formulas name their query and set query_index`` () =
    let p = planOf [ Some "a", "x{*}"; Some "b", "y{*}" ] [ "b" ]
    let o = Assert.Single p.Outputs
    Assert.Equal(0, o.QueryIndex)
    Assert.Equal(Fetch 1, o.Node)

[<Fact>]
let ``a formula naming nothing is refused`` () =
    Assert.Contains("formulas[0]: no query is named 'c'", errorsOf [ Some "a", "x{*}" ] [ "c" ])

[<Fact>]
let ``arithmetic in formulas is not supported yet`` () =
    let es = errorsOf [ Some "a", "x{*}"; Some "b", "y{*}" ] [ "a / b" ]
    Assert.Contains("formulas[0]: arithmetic in formulas is not supported yet", es)

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

// --- functions in formulas ------------------------------------------------------------

let private nodeOf formula = (Assert.Single (planOf [ Some "a", "x{*}" ] [ formula ]).Outputs).Node

[<Fact>]
let ``pointwise functions resolve to typed nodes`` () =
    Assert.Equal(Pointwise(Abs, Fetch 0), nodeOf "abs(a)")
    Assert.Equal(Pointwise(ClampMin 100.0, Fetch 0), nodeOf "clamp_min(a, 100)")
    Assert.Equal(Pointwise(CutoffMax -5.0, Fetch 0), nodeOf "cutoff_max(a, -5)")
    Assert.Equal(Pointwise(Round 0, Fetch 0), nodeOf "round(a)")
    Assert.Equal(Pointwise(Round 2, Fetch 0), nodeOf "round(a, 2)")

[<Fact>]
let ``functions nest`` () =
    Assert.Equal(Pointwise(Abs, Pointwise(Log10, Fetch 0)), nodeOf "abs(log10(a))")

[<Fact>]
let ``wrong arguments are named`` () =
    let err formula = errorsOf [ Some "a", "x{*}" ] [ formula ]
    Assert.Contains("formulas[0]: abs() takes one argument: abs(query)", err "abs(a, 2)")
    Assert.Contains("formulas[0]: clamp_min() takes a query and a number: clamp_min(query, 100)", err "clamp_min(a)")
    Assert.Contains("formulas[0]: clamp_min() takes only numbers after the query", err "clamp_min(a, 'x')")
    Assert.Contains("formulas[0]: the first argument of abs() must be a query", err "abs('a')")
    Assert.Contains("formulas[0]: abs() needs a query to act on", err "abs()")
    Assert.Contains(
        "formulas[0]: round() takes a query and optionally a whole number of decimals from 0 to 15: round(query, 2)",
        err "round(a, 1.5)"
    )

[<Fact>]
let ``unknown functions and names inside functions are reported together`` () =
    let es = errorsOf [ Some "a", "x{*}" ] [ "autosmooth(a)"; "abs(b)" ]
    Assert.Contains("formulas[0]: function 'autosmooth' is not supported yet", es)
    Assert.Contains("formulas[1]: no query is named 'b'", es)

// --- across series -------------------------------------------------------------------

[<Fact>]
let ``top with quoted or bare words`` () =
    Assert.Equal(Top(Fetch 0, 10, ByMean, Desc), nodeOf "top(a, 10, 'mean', 'desc')")
    // Bare words arrive as query names; top wants words there, so they are.
    Assert.Equal(Top(Fetch 0, 5, ByL2norm, Asc), nodeOf "top(a, 5, l2norm, asc)")

[<Fact>]
let ``top shorthands`` () =
    Assert.Equal(Top(Fetch 0, 10, ByMean, Desc), nodeOf "top10_mean(a)")
    Assert.Equal(Top(Fetch 0, 10, ByMin, Asc), nodeOf "bottom10_min(a)")
    Assert.Equal(Top(Fetch 0, 20, ByArea, Desc), nodeOf "top20_area(a)")

[<Fact>]
let ``count and exclude functions`` () =
    Assert.Equal(CountNonzero(Fetch 0), nodeOf "count_nonzero(a)")
    Assert.Equal(CountNonzero(Fetch 0), nodeOf "count_nonzero_finite(a)")
    Assert.Equal(CountNotNull(Fetch 0), nodeOf "count_not_null(a)")
    Assert.Equal(ExcludeNull(Fetch 0), nodeOf "exclude_null(a)")

[<Fact>]
let ``across-series functions combine with pointwise ones`` () =
    Assert.Equal(Top(Pointwise(Abs, Fetch 0), 5, ByMax, Desc), nodeOf "top(abs(a), 5, 'max', 'desc')")

[<Fact>]
let ``top arguments are checked`` () =
    let err formula = errorsOf [ Some "a", "x{*}" ] [ formula ]
    Assert.Contains("formulas[0]: top() limit must be one of 5, 10, 25, 50, 100", err "top(a, 7, 'mean', 'desc')")
    Assert.Contains("formulas[0]: top() ranks by one of 'max', 'mean', 'min', 'sum', 'last', 'l2norm', 'area'", err "top(a, 10, 'median', 'desc')")
    Assert.Contains("formulas[0]: top() direction must be 'asc' or 'desc'", err "top(a, 10, 'mean', 'up')")
    Assert.Contains("formulas[0]: top() takes a query, a limit, a ranking and a direction: top(query, 10, 'mean', 'desc')", err "top(a)")
    Assert.Contains("formulas[0]: top10_mean() takes one argument: top10_mean(query)", err "top10_mean(a, 3)")
    // Not in the documented pattern: 25 is a top() limit, not a shorthand size.
    Assert.Contains("formulas[0]: function 'top25_mean' is not supported yet", err "top25_mean(a)")

// --- along time ------------------------------------------------------------------------

[<Fact>]
let ``time-wise functions resolve, with the documented aliases`` () =
    Assert.Equal(Timewise(Cumsum, Fetch 0), nodeOf "cumsum(a)")
    Assert.Equal(Timewise(PerMinute, Fetch 0), nodeOf "per_minute(a)")
    Assert.Equal(Timewise(Ewma 7, Fetch 0), nodeOf "ewma_7(a)")
    Assert.Equal(Timewise(Ewma 20, Fetch 0), nodeOf "ewma(a)")
    Assert.Equal(Timewise(Median 3, Fetch 0), nodeOf "median(a)")
    Assert.Equal(Timewise(RollingAvg 13, Fetch 0), nodeOf "rollingavg_13(a)")

[<Fact>]
let ``undocumented spans are not guessed`` () =
    Assert.Contains("formulas[0]: function 'ewma_4' is not supported yet", errorsOf [ Some "a", "x{*}" ] [ "ewma_4(a)" ])

[<Fact>]
let ``lookback is the deepest a query is read`` () =
    let p = planOf [ Some "a", "x{*}"; Some "b", "y{*}" ] [ "diff(ewma_5(a))"; "median_7(a)"; "cumsum(b)" ]
    // a: diff 1 + ewma 2×5 = 11, more than median_7's 6. b: cumsum needs none.
    Assert.Equal<int list>([ 11; 0 ], p.Queries |> List.map _.Lookback)
