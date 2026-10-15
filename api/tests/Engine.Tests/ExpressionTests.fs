module NinjaCat.Api.Engine.Tests.ExpressionTests

open Xunit
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.MetricQuery.Parser

/// A metric query with only an aggregator and a name — enough to tell the
/// leaves apart in these tests.
let private q agg metric =
    Leaf { SpaceAgg = agg; Metric = metric; Filter = All; GroupBy = []; Modifiers = [] }

let private parses (input: string) (expected: Expr<MetricQuery> list) =
    match parseProgram input with
    | Ok actual -> Assert.Equal<Expr<MetricQuery> list>(expected, actual)
    | Error e -> Assert.Fail $"'{input}' did not parse:\n{e}"

let private fails (input: string) (fragment: string) =
    match parseProgram input with
    | Ok actual -> Assert.Fail $"'{input}' should not parse, got {actual}"
    | Error e -> Assert.Contains(fragment, e)

// --- a function at the root ---------------------------------------------------

[<Fact>]
let ``function wrapping a query`` () =
    // Datadog docs, exclusion functions.
    parses "exclude_null(avg:system.load.1{*} by {host})"
        [ Call("exclude_null",
               [ Value(Leaf { SpaceAgg = Some "avg"; Metric = "system.load.1"; Filter = All; GroupBy = [ "host" ]; Modifiers = [] }) ]) ]

[<Fact>]
let ``functions nest`` () =
    parses "abs(log10(x{*}))" [ Call("abs", [ Value(Call("log10", [ Value(q None "x") ])) ]) ]

[<Fact>]
let ``top with numbers and quoted strings`` () =
    // terraform-provider-datadog TestAccDatadogDashboardTopListWithStyle.
    parses "top(avg:x{*}, 10, 'sum', 'desc')"
        [ Call("top", [ Value(q (Some "avg") "x"); Value(Number 10.0); Lit(Quoted "sum"); Lit(Quoted "desc") ]) ]

[<Fact>]
let ``negative argument is a plain number`` () =
    // Datadog docs, timeshift.
    parses "timeshift(avg:system.load.1{*}, -1209600)"
        [ Call("timeshift", [ Value(q (Some "avg") "system.load.1"); Value(Number -1209600.0) ]) ]

[<Fact>]
let ``double-quoted arguments`` () =
    parses "calendar_shift(x{*}, \"-1d\", \"UTC\")" [ Call("calendar_shift", [ Value(q None "x"); Lit(Quoted "-1d"); Lit(Quoted "UTC") ]) ]

[<Fact>]
let ``named arguments, mixed with positional`` () =
    // datadog-api-client-go TestMonitorLifecycle cassette, no spaces after commas.
    parses "anomalies(sum:m{*}.as_count(),'agile',2,direction='below',interval=300,count_default_zero='true')"
        [ Call("anomalies",
               [ Value(Leaf { SpaceAgg = Some "sum"; Metric = "m"; Filter = All; GroupBy = []
                              Modifiers = [ { Name = "as_count"; Args = [] } ] })
                 Lit(Quoted "agile")
                 Value(Number 2.0)
                 Named("direction", Quoted "below")
                 Named("interval", Num 300.0)
                 Named("count_default_zero", Quoted "true") ]) ]

[<Fact>]
let ``bare word argument`` () =
    parses "top(x{*}, 5, mean, desc)" [ Call("top", [ Value(q None "x"); Value(Number 5.0); Lit(Word "mean"); Lit(Word "desc") ]) ]

// --- arithmetic ------------------------------------------------------------------

[<Fact>]
let ``multiplication before addition`` () =
    parses "a{*} + b{*} * 2" [ Binary(Add, q None "a", Binary(Mul, q None "b", Number 2.0)) ]

[<Fact>]
let ``left to right within one level`` () =
    parses "a{*} - b{*} - c{*}" [ Binary(Sub, Binary(Sub, q None "a", q None "b"), q None "c") ]

[<Fact>]
let ``parentheses override precedence`` () =
    parses "(a{*} + b{*}) * 100" [ Binary(Mul, Binary(Add, q None "a", q None "b"), Number 100.0) ]

[<Fact>]
let ``parenthesised queries with modifiers`` () =
    // terraform-provider-datadog BadEventsToMetricQueryTransition cassette.
    match parseProgram "(sum:my.metric{status:good}.as_count()) + (sum:my.metric{status:bad}.as_count())" with
    | Ok [ Binary(Add, Leaf good, Leaf bad) ] ->
        Assert.Equal(Tag("status", "good"), good.Filter)
        Assert.Equal(Tag("status", "bad"), bad.Filter)
    | other -> Assert.Fail $"unexpected: {other}"

[<Fact>]
let ``arithmetic inside a function`` () =
    parses "abs(a{*} / b{*})" [ Call("abs", [ Value(Binary(Div, q None "a", q None "b")) ]) ]

[<Fact>]
let ``unary minus on a query`` () = parses "-a{*}" [ Neg(q None "a") ]

// --- several expressions -----------------------------------------------------

[<Fact>]
let ``comma separates expressions at the top`` () =
    parses "avg:a{*}, avg:b{x:1,y:2}"
        [ q (Some "avg") "a"
          Leaf { SpaceAgg = Some "avg"; Metric = "b"; Filter = And [ Tag("x", "1"); Tag("y", "2") ]; GroupBy = []; Modifiers = [] } ]

// --- traps -----------------------------------------------------------------------

[<Fact>]
let ``a metric named like NaN is still a metric`` () = parses "nan.errors{*}" [ q None "nan.errors" ]

[<Fact>]
let ``a function name without parenthesis is a metric`` () = parses "abs{*}" [ q None "abs" ]

[<Fact>]
let ``unclosed call`` () = fails "abs(x{*}" "Expecting"
