module NinjaCat.Api.Engine.Tests.MetricQueryTests

open Xunit
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.MetricQuery.Parser

let private q metric =
    { SpaceAgg = None; Metric = metric; Filter = All; GroupBy = []; Modifiers = [] }

let private m name args = { Name = name; Args = args }

let private parses (input: string) (expected: MetricQuery) =
    match parseMetricQuery input with
    | Ok actual -> Assert.Equal(expected, actual)
    | Error e -> Assert.Fail $"'{input}' did not parse:\n{e}"

let private fails (input: string) (fragment: string) =
    match parseMetricQuery input with
    | Ok actual -> Assert.Fail $"'{input}' should not parse, got {actual}"
    | Error e -> Assert.Contains(fragment, e)

[<Fact>]
let ``bare metric`` () =
    // datadog-api-client-python examples/v1/metrics/QueryMetrics.py.
    parses "system.cpu.idle{*}" (q "system.cpu.idle")

[<Fact>]
let ``space aggregator`` () =
    parses "avg:system.cpu.user{*}" { q "system.cpu.user" with SpaceAgg = Some "avg" }
    parses "p90:dist.dd.dogweb.latency{*}" { q "dist.dd.dogweb.latency" with SpaceAgg = Some "p90" }
    parses "histogram:trace.Load{*}" { q "trace.Load" with SpaceAgg = Some "histogram" }

[<Fact>]
let ``filter is the tag filter parser`` () =
    parses "avg:system.cpu.user{env:prod,!host:a}"
        { q "system.cpu.user" with SpaceAgg = Some "avg"; Filter = And [ Tag("env", "prod"); Not(Tag("host", "a")) ] }

[<Fact>]
let ``group by`` () =
    parses "avg:system.cpu.user{*} by {host}" { q "system.cpu.user" with SpaceAgg = Some "avg"; GroupBy = [ "host" ] }
    parses "sum:x{*} by {service, team, app}" { q "x" with SpaceAgg = Some "sum"; GroupBy = [ "service"; "team"; "app" ] }

[<Fact>]
let ``group by with no spaces at all`` () =
    // The one v1 query recorded with a 200 in datadog-api-client-go (TestMetrics.yaml).
    parses "avg:Test_TestMetrics_1618221195{bar:baz}by{host}"
        { q "Test_TestMetrics_1618221195" with SpaceAgg = Some "avg"; Filter = Tag("bar", "baz"); GroupBy = [ "host" ] }

[<Fact>]
let ``modifiers after group by`` () =
    parses "avg:x{*} by {app_name,bosh_id}.rollup(avg, 3600)"
        { q "x" with
            SpaceAgg = Some "avg"
            GroupBy = [ "app_name"; "bosh_id" ]
            Modifiers = [ m "rollup" [ Word "avg"; Num 3600.0 ] ] }

[<Fact>]
let ``several modifiers, in order`` () =
    parses "sum:x{*}.as_count().rollup(sum, 60).fill(zero)"
        { q "x" with
            SpaceAgg = Some "sum"
            Modifiers =
              [ m "as_count" []
                m "rollup" [ Word "sum"; Num 60.0 ]
                m "fill" [ Word "zero" ] ] }

[<Fact>]
let ``calendar rollup words`` () =
    // Cloud cost queries in datadog-api-client-go examples.
    parses "sum:aws.cost{*}.rollup(sum, monthly)"
        { q "aws.cost" with SpaceAgg = Some "sum"; Modifiers = [ m "rollup" [ Word "sum"; Word "monthly" ] ] }

[<Fact>]
let ``metric segment starting with a digit`` () =
    // terraform-provider-datadog TestAccDatadogMonitor_Updated.
    parses "org.eclipse.jetty.ServletContextHandler.5xx_responses{example}"
        { q "org.eclipse.jetty.ServletContextHandler.5xx_responses" with Filter = Bare "example" }

[<Fact>]
let ``aggregator outside parentheses, for weighted`` () =
    // Datadog docs, smoothing functions.
    parses "sum:(kubernetes.cpu.usage{*}).weighted()"
        { q "kubernetes.cpu.usage" with SpaceAgg = Some "sum"; Modifiers = [ m "weighted" [] ] }

[<Fact>]
let ``missing braces are refused`` () = fails "avg:system.cpu.user" "Expecting: '{'"

[<Fact>]
let ``unknown trailing text is refused`` () = fails "avg:x{*} banana" "end of input"
