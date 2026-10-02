module NinjaCat.Api.Engine.Tests.TimeseriesTests

open System
open Xunit
open NinjaCat.Api.Engine.MetricQuery.Ast
open NinjaCat.Api.Engine.Api.V2.Timeseries

let private readJson (json: string) : Result<ParsedTimeseriesRequest, string list> = read json

let private errorsOf json : string list =
    match readJson json with
    | Ok r -> failwith $"expected errors, got {r}"
    | Error es -> es

/// Builds a request body; each test overrides only what it is about.
let private body (attrs: string) =
    $"""{{"data":{{"type":"timeseries_request","attributes":{{{attrs}}}}}}}"""

let private window = "\"from\":1711977601000,\"to\":1711981201000"

[<Fact>]
let ``the recorded Datadog request`` () =
    // datadog-api-client-go, Scenario_Timeseries_cross_product_query_returns_OK_response.
    let json =
        """{"data":{"attributes":{"formulas":[{"formula":"a","limit":{"count":10,"order":"desc"}}],"from":1711977601000,"interval":5000,"queries":[{"data_source":"metrics","name":"a","query":"avg:datadog.estimated_usage.metrics.custom{*}"}],"to":1711981201000},"type":"timeseries_request"}}"""

    match readJson json with
    | Error es -> Assert.Fail(String.Join("\n", es))
    | Ok r ->
        Assert.Equal(DateTimeOffset.FromUnixTimeMilliseconds 1711977601000L, r.From)
        Assert.Equal(Some(TimeSpan.FromSeconds 5.0), r.Interval)
        let q = Assert.Single r.Queries
        Assert.Equal(Some "a", q.Name)
        Assert.Equal("datadog.estimated_usage.metrics.custom", q.Query.Metric)
        let f = Assert.Single r.Formulas
        Assert.Equal(Leaf "a", f.Formula)
        Assert.Equal(Some { Count = Some 10; Order = Desc }, f.Limit)

[<Fact>]
let ``formulas and interval are optional`` () =
    match readJson (body $"""{window},"queries":[{{"data_source":"metrics","query":"avg:x{{*}}"}}]""") with
    | Ok r ->
        Assert.Empty r.Formulas
        Assert.Equal(None, r.Interval)
        Assert.Equal(None, (Assert.Single r.Queries).Name)
    | Error es -> Assert.Fail(String.Join("\n", es))

[<Fact>]
let ``wrong type uses Datadog's message`` () =
    // Recorded 400 from Datadog, for a fixture with the typo "timeseries_rquest".
    let json = (body $"""{window},"queries":[]""").Replace("timeseries_request", "timeseries_rquest")
    Assert.Contains("API input validation failed: Invalid type. Expected \"timeseries_request\".", errorsOf json)

[<Fact>]
let ``missing fields name the JSON location`` () =
    Assert.Equal<string list>([ "missing field 'from' in data.attributes" ], errorsOf (body """ "to":2,"queries":[] """))

    Assert.Equal<string list>(
        [ "missing field 'data_source' in data.attributes.queries[]" ],
        errorsOf (body """ "from":1,"to":2,"queries":[{"query":"x{*}"}] """)
    )

[<Fact>]
let ``explicit nulls read as absent`` () =
    match readJson (body """ "from":1,"to":2,"interval":null,"queries":[{"data_source":"metrics","name":null,"query":"x{*}"}],"formulas":[{"formula":"a","limit":null}] """) with
    | Ok r ->
        Assert.Equal(None, r.Interval)
        Assert.Equal(None, (Assert.Single r.Queries).Name)
        Assert.Equal(None, (Assert.Single r.Formulas).Limit)
    | Error es -> Assert.Fail(String.Join("\n", es))

[<Fact>]
let ``unknown fields are ignored`` () =
    // Dashboards send semantic_mode alongside queries.
    match readJson (body """ "from":1,"to":2,"queries":[{"data_source":"metrics","query":"x{*}","semantic_mode":"native"}] """) with
    | Ok r -> Assert.Single r.Queries |> ignore
    | Error es -> Assert.Fail(String.Join("\n", es))

[<Fact>]
let ``syntax errors name the query and the column`` () =
    let es =
        errorsOf (body $"""{window},"queries":[{{"data_source":"metrics","name":"a","query":"avg:x{{env:prod"}}]""")

    let e = Assert.Single es
    Assert.StartsWith("queries[0] (a): Error in Ln: 1 Col: 15", e)
    Assert.DoesNotContain("\n", e)

[<Fact>]
let ``every bad query and formula is reported`` () =
    let es =
        errorsOf (
            body
                $"""{window},"queries":[{{"data_source":"metrics","query":"x"}},{{"data_source":"metrics","query":"y{{*}}"}}],"formulas":[{{"formula":"a +"}}]"""
        )

    Assert.Equal(2, es.Length)
    Assert.StartsWith("queries[0]:", es[0])
    Assert.StartsWith("formulas[0]:", es[1])

[<Fact>]
let ``other data sources are refused for now`` () =
    let es = errorsOf (body $"""{window},"queries":[{{"data_source":"logs","query":"service:web"}}]""")
    Assert.Equal<string list>([ "queries[0]: data_source 'logs' is not supported yet" ], es)

[<Fact>]
let ``a logs query is refused by its data source, not by its fields`` () =
    // Logs queries carry `search`, not `query` (spec: EventsTimeseriesQuery).
    let es = errorsOf (body $"""{window},"queries":[{{"data_source":"logs","name":"a","search":{{"query":"service:web"}}}}]""")
    Assert.Equal<string list>([ "queries[0] (a): data_source 'logs' is not supported yet" ], es)

[<Fact>]
let ``from must be before to`` () =
    let es = errorsOf (body """ "from":2000,"to":1000,"queries":[{"data_source":"metrics","query":"x{*}"}] """)
    Assert.Contains("data.attributes.from must be earlier than data.attributes.to", es)

[<Fact>]
let ``wrong JSON types are reported in the deserializer's own words`` () =
    let notAList = Assert.Single(errorsOf (body """ "from":1,"to":2,"queries":{} """))
    Assert.StartsWith("invalid request body: ", notAList)
    Assert.Contains("expected JSON array", notAList)

    let notANumber = Assert.Single(errorsOf (body """ "from":"yesterday","to":1000,"queries":[] """))
    Assert.StartsWith("invalid request body: ", notANumber)
    Assert.Contains("could not be converted to System.Int64", notANumber)

[<Fact>]
let ``queries is required`` () =
    Assert.Equal<string list>([ "missing field 'queries' in data.attributes" ], errorsOf (body """ "from":1,"to":2 """))

[<Fact>]
let ``not JSON at all`` () =
    let e = Assert.Single(errorsOf "hello")
    Assert.StartsWith("invalid request body:", e)
