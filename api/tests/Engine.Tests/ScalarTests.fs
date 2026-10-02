module NinjaCat.Api.Engine.Tests.ScalarTests

open System
open System.Text.Json
open System.Threading.Tasks
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Api.V2
open NinjaCat.Api.Engine.Api.V2.Scalar
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Query.Compile

let private body (queries: string) (formulas: string) =
    $"""{{"data":{{"type":"scalar_request","attributes":{{"from":0,"to":100000,"queries":[{queries}],"formulas":[{formulas}]}}}}}}"""

let private errorsOf json =
    match read json with
    | Ok r -> failwith $"expected errors, got {r}"
    | Error es -> es

[<Fact>]
let ``the recorded Datadog request`` () =
    // datadog-api-client-go, Scenario_Scalar_cross_product_query_returns_OK_response.
    let json =
        """{"data":{"attributes":{"formulas":[{"formula":"a","limit":{"count":10,"order":"desc"}}],"from":1711391280000,"queries":[{"aggregator":"avg","data_source":"metrics","name":"a","query":"avg:system.cpu.user{*}"}],"to":1711394880000},"type":"scalar_request"}}"""

    match read json with
    | Ok r ->
        Assert.Equal<ScalarAggregator list>([ ScalarAvg ], r.Aggregators)
        Assert.Equal<string list>([ "a" ], r.ColumnNames)
    | Error es -> Assert.Fail(String.Join("\n", es))

[<Fact>]
let ``aggregator is required and checked`` () =
    Assert.Contains("missing field 'aggregator' in data.attributes.queries[]", errorsOf (body """{"data_source":"metrics","query":"x{*}"}""" ""))
    Assert.Contains("queries[0]: aggregator 'percentile' is not supported yet", errorsOf (body """{"data_source":"metrics","query":"x{*}","aggregator":"percentile"}""" ""))

    let e = errorsOf (body """{"data_source":"metrics","query":"x{*}","aggregator":"median"}""" "") |> List.head
    Assert.StartsWith("queries[0]: aggregator must be one of", e)

[<Fact>]
let ``wrong type uses Datadog's wording`` () =
    let json = (body """{"data_source":"metrics","query":"x{*}","aggregator":"avg"}""" "").Replace("scalar_request", "timeseries_request")
    Assert.Contains("API input validation failed: Invalid type. Expected \"scalar_request\".", errorsOf json)

/// Rows every 20 s: host a at 1, 2, 3; host b at 10, 20, 30 — for query x.
/// Query y is ungrouped: 4, 4, 4.
let private execute (sql: Sql) =
    let metric = sql.Parameters |> List.pick (fun (n, v) -> match v with String m when n.StartsWith "metric" -> Some m | _ -> None)
    let row groups t v = { Groups = groups; BucketMs = t; Value = v; SeriesId = None }

    Task.FromResult(
        match metric with
        | "x" -> [ for t, v in [ 0L, 1.0; 20000L, 2.0; 40000L, 3.0 ] do
                       row [ Some "a" ] t v
                       row [ Some "b" ] t (v * 10.0) ]
        | _ -> [ for t in [ 0L; 20000L; 40000L ] -> row [] t 4.0 ]
    )

let private run queries formulas =
    match read (body queries formulas) with
    | Error es -> failwith (String.Join("\n", es))
    | Ok r ->
        match Plan.plan r.Request with
        | Error es -> failwith (String.Join("\n", es))
        | Ok p ->
            match (Execute.runScalar execute (TenantId "t") p r).Result with
            | Ok response -> JsonSerializer.Serialize(response, Serialization.options)
            | Error es -> failwith (String.Join("\n", es))

[<Fact>]
let ``one value per group, a group column per key`` () =
    let json = run """{"data_source":"metrics","name":"x","query":"avg:x{*} by {host}.fill(null)","aggregator":"avg"}""" ""
    Assert.Contains("""{"name":"host","type":"group","values":[["a"],["b"]]}""", json)
    Assert.Contains("""{"name":"x","type":"number","values":[2,20],"meta":{}}""", json)

[<Fact>]
let ``each aggregator`` () =
    let value agg =
        let json = run $"""{{"data_source":"metrics","name":"y","query":"avg:y{{*}}.fill(null)","aggregator":"{agg}"}}""" ""
        let doc = JsonDocument.Parse json
        let columns = doc.RootElement.GetProperty("data").GetProperty("attributes").GetProperty("columns")
        let values = columns.[0].GetProperty("values")
        values.[0].GetDouble()

    Assert.Equal(4.0, value "avg")
    Assert.Equal(4.0, value "mean")
    Assert.Equal(12.0, value "sum")
    Assert.Equal(4.0, value "last")
    Assert.Equal(sqrt 48.0, value "l2norm", 9)
    // A 100 s window takes a 2 s step (Datadog's table): three points of 4, each 2 s wide.
    Assert.Equal(12.0 * 2.0, value "area")

[<Fact>]
let ``formulas run on the reduced values`` () =
    // avg(x) / avg(y): host a 2 / 4, host b 20 / 4.
    let json =
        run
            """{"data_source":"metrics","name":"x","query":"avg:x{*} by {host}.fill(null)","aggregator":"avg"},{"data_source":"metrics","name":"y","query":"avg:y{*}.fill(null)","aggregator":"avg"}"""
            """{"formula":"x / y * 100"}"""

    Assert.Contains("""{"name":"x / y * 100","type":"number","values":[50,500],"meta":{}}""", json)

[<Fact>]
let ``a limit orders and trims the rows`` () =
    let json =
        run
            """{"data_source":"metrics","name":"x","query":"avg:x{*} by {host}.fill(null)","aggregator":"max"}"""
            """{"formula":"x","limit":{"count":1,"order":"desc"}}"""

    Assert.Contains("""{"name":"host","type":"group","values":[["b"]]}""", json)
    Assert.Contains("""{"name":"x","type":"number","values":[30],"meta":{}}""", json)

[<Fact>]
let ``a limit on one formula limits the rows of all`` () =
    let json =
        run
            """{"data_source":"metrics","name":"x","query":"avg:x{*} by {host}.fill(null)","aggregator":"max"}"""
            """{"formula":"x","limit":{"count":1,"order":"desc"}},{"formula":"x * 2"}"""

    Assert.Contains("""{"name":"host","type":"group","values":[["b"]]}""", json)
    Assert.Contains("""{"name":"x * 2","type":"number","values":[60],"meta":{}}""", json)
