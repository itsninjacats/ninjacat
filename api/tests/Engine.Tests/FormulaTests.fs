module NinjaCat.Api.Engine.Tests.FormulaTests

open Xunit
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.MetricQuery.Parser

let private parses (input: string) (expected: Formula) =
    match parseFormula input with
    | Ok actual -> Assert.Equal(expected, actual)
    | Error e -> Assert.Fail $"'{input}' did not parse:\n{e}"

let private fails (input: string) =
    match parseFormula input with
    | Ok actual -> Assert.Fail $"'{input}' should not parse, got {actual}"
    | Error _ -> ()

let private a = Leaf "a"
let private q1 = Leaf "query1"
let private q2 = Leaf "query2"

// Every formula below is from the corpus of Datadog client and Terraform
// fixtures unless noted.

[<Fact>]
let ``a single query name`` () =
    // The most common formula by far: a widget shows only formulas, so even
    // one query is displayed through the formula that names it.
    parses "query1" q1

[<Fact>]
let ``arithmetic between names`` () =
    parses "query2/query1" (Binary(Div, q2, q1))
    parses "a+b" (Binary(Add, a, Leaf "b"))

[<Fact>]
let ``parentheses and precedence`` () =
    parses "(query1-query2)/query1" (Binary(Div, Binary(Sub, q1, q2), q1))

[<Fact>]
let ``redundant parentheses and decimals`` () =
    parses "(((errors * 0.2)) / (query * 0.3))"
        (Binary(Div, Binary(Mul, Leaf "errors", Number 0.2), Binary(Mul, Leaf "query", Number 0.3)))

[<Fact>]
let ``functions over names`` () =
    parses "hour_before(query1)" (Call("hour_before", [ Value q1 ]))
    parses "default_zero(query1)" (Call("default_zero", [ Value q1 ]))

[<Fact>]
let ``top takes quoted words`` () =
    // Datadog docs, rank functions, written over a formula.
    parses "top(a, 10, 'mean', 'desc')" (Call("top", [ Value a; Value(Number 10.0); Lit(Quoted "mean"); Lit(Quoted "desc") ]))

[<Fact>]
let ``bare words are read as names`` () =
    // `mean` could be a query name or a literal; the parser cannot tell, so it
    // takes it as a name and analysis decides from what `top` expects.
    parses "top(a, 10, mean, desc)" (Call("top", [ Value a; Value(Number 10.0); Value(Leaf "mean"); Value(Leaf "desc") ]))

[<Fact>]
let ``negative shift`` () =
    parses "timeshift(a, -3600)" (Call("timeshift", [ Value a; Value(Number -3600.0) ]))

[<Fact>]
let ``a metric query is not a formula`` () =
    // Formulas name queries; the queries themselves go in `queries[]`.
    fails "avg:system.cpu.user{*}"

[<Fact>]
let ``dangling operator`` () = fails "a +"
