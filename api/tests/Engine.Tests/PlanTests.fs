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
    let es = errorsOf [ Some "a", "p95:x{*}.weighted()" ] []
    Assert.Contains("queries[0] (a): aggregator 'p95:' is not supported yet", es)
    Assert.Contains("queries[0] (a): .weighted() is not supported yet", es)

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
    let es = errorsOf [ Some "a", "x{*}" ] [ "anomalies(a, 'basic', 2)"; "abs(b)" ]
    Assert.Contains("formulas[0]: function 'anomalies' is not supported yet", es)
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

// --- shifts ------------------------------------------------------------------------------

[<Fact>]
let ``fixed shifts`` () =
    Assert.Equal(Shift(-3600000L, Fetch 0), nodeOf "timeshift(a, -3600)")
    Assert.Equal(Shift(-3600000L, Fetch 0), nodeOf "hour_before(a)")
    Assert.Equal(Shift(-86400000L, Fetch 0), nodeOf "day_before(a)")
    Assert.Equal(Shift(-604800000L, Fetch 0), nodeOf "week_before(a)")

/// Plans one formula with a window starting at `from`.
let private shiftAt (from: string) formula =
    let fromMs = DateTimeOffset.Parse(from).ToUnixTimeMilliseconds()
    let body =
        $"""{{"data":{{"type":"timeseries_request","attributes":{{"from":{fromMs},"to":{fromMs + 3600000L},"queries":[{{"data_source":"metrics","name":"a","query":"x{{*}}"}}],"formulas":[{{"formula":"{formula}"}}]}}}}}}"""

    match read body |> Result.bind plan with
    | Ok p ->
        match (List.exactlyOne p.Outputs).Node with
        | Shift(offset, Fetch 0) -> offset
        | other -> failwith $"unexpected {other}"
    | Error es -> failwith (String.Join("\n", es))

[<Fact>]
let ``calendar_shift by weeks and days`` () =
    Assert.Equal(-2L * 604800000L, shiftAt "2026-03-10T12:00:00Z" "calendar_shift(a, \\\"-2w\\\")")
    Assert.Equal(-86400000L, shiftAt "2026-03-10T12:00:00Z" "calendar_shift(a, \\\"-1d\\\", \\\"UTC\\\")")

[<Fact>]
let ``calendar_shift keeps wall-clock time across DST`` () =
    // Europe/Warsaw moved to summer time at 02:00 on 2026-03-29. Noon that day
    // (10:00 UTC) minus one calendar day is noon on the 28th, still winter
    // time (11:00 UTC): 23 hours earlier, not 24.
    Assert.Equal(-23L * 3600000L, shiftAt "2026-03-29T10:00:00Z" "calendar_shift(a, \\\"-1d\\\", \\\"Europe/Warsaw\\\")")

[<Fact>]
let ``a month back is a calendar month`` () =
    // 31 March → 28 February (2026 is not a leap year): 31 days back.
    Assert.Equal(-31L * 86400000L, shiftAt "2026-03-31T12:00:00Z" "month_before(a)")
    Assert.Equal(-31L * 86400000L, shiftAt "2026-03-31T12:00:00Z" "calendar_shift(a, \\\"-1mo\\\")")

[<Fact>]
let ``shift arguments are checked`` () =
    let err formula = errorsOf [ Some "a", "x{*}" ] [ formula ]
    Assert.Contains("formulas[0]: timeshift() takes a query and a whole, non-zero number of seconds: timeshift(query, -3600)", err "timeshift(a)")
    Assert.Contains("formulas[0]: day_before() takes one argument: day_before(query)", err "day_before(a, 2)")
    Assert.Contains("formulas[0]: calendar_shift() shift must look like \"-1d\", \"-2w\" or \"-1mo\", got \"1d\"", err "calendar_shift(a, '1d')")
    Assert.Contains("formulas[0]: calendar_shift() timezone 'Mars/Olympus' is not a known IANA zone", err "calendar_shift(a, '-1d', 'Mars/Olympus')")

[<Fact>]
let ``a shift keeps the lookback of what it wraps`` () =
    let p = planOf [ Some "a", "x{*}" ] [ "week_before(diff(a))" ]
    Assert.Equal(1, (List.exactlyOne p.Queries).Lookback)

// --- fill ----------------------------------------------------------------------------------

[<Fact>]
let ``interpolation is on by default, off under as_count and as_rate`` () =
    Assert.Equal(FillWithin(Linear, 300), (onlyQuery "avg:x{*}").Fill)
    Assert.Equal(NoFill, (onlyQuery "sum:x{*}.as_count()").Fill)
    Assert.Equal(NoFill, (onlyQuery "sum:x{*}.as_rate()").Fill)

[<Fact>]
let ``the fill modifier`` () =
    Assert.Equal(NoFill, (onlyQuery "avg:x{*}.fill(null)").Fill)
    Assert.Equal(FillWithin(Zero, 300), (onlyQuery "avg:x{*}.fill(zero)").Fill)
    Assert.Equal(FillWithin(Last, 120), (onlyQuery "avg:x{*}.fill(last, 120)").Fill)
    Assert.Equal(FillWithin(Linear, 60), (onlyQuery "sum:x{*}.as_count().fill(linear, 60)").Fill)

[<Fact>]
let ``fill arguments are checked`` () =
    Assert.Contains("queries[0]: .fill() method must be one of null, zero, last, linear", errorsOf [ None, "x{*}.fill(mean)" ] [])
    Assert.Contains("queries[0]: .fill() limit must be a whole number of seconds, e.g. .fill(linear, 300)", errorsOf [ None, "x{*}.fill(zero, 1.5)" ] [])

[<Fact>]
let ``default_zero resolves`` () = Assert.Equal(DefaultZero(Fetch 0), nodeOf "default_zero(a)")

// --- algorithms --------------------------------------------------------------------------

[<Fact>]
let ``regressions and autosmooth resolve`` () =
    Assert.Equal(Timewise(TrendLine, Fetch 0), nodeOf "trend_line(a)")
    Assert.Equal(Timewise(RobustTrend, Fetch 0), nodeOf "robust_trend(a)")
    Assert.Equal(Timewise(PiecewiseConstant, Fetch 0), nodeOf "piecewise_constant(a)")
    Assert.Equal(Timewise(Autosmooth, Fetch 0), nodeOf "autosmooth(a)")

[<Fact>]
let ``outliers resolve, any case`` () =
    Assert.Equal(Outliers(Fetch 0, Dbscan false, 3.0), nodeOf "outliers(a, 'DBSCAN', 3)")
    Assert.Equal(Outliers(Fetch 0, Dbscan true, 2.5), nodeOf "outliers(a, 'scaledbscan', 2.5)")
    Assert.Equal(Outliers(Fetch 0, Mad(false, 20.0), 3.0), nodeOf "outliers(a, 'mad', 3, 20)")
    Assert.Equal(Outliers(Fetch 0, Mad(true, 10.0), 2.0), nodeOf "outliers(a, 'scaledMAD', 2, 10)")

[<Fact>]
let ``outliers arguments are checked`` () =
    let err formula = errorsOf [ Some "a", "x{*}" ] [ formula ]
    Assert.Contains("formulas[0]: outliers() with MAD needs a percentage: outliers(query, 'mad', 3, 20)", err "outliers(a, 'mad', 3)")
    Assert.Contains(
        "formulas[0]: outliers() takes a query, an algorithm, a tolerance and for MAD a percentage: outliers(query, 'dbscan', 3) or outliers(query, 'mad', 3, 20)",
        err "outliers(a, 'kmeans', 3)"
    )
