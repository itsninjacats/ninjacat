/// A range past what a date can hold finds nothing, as in Datadog: 200 and
/// the empty answer, never an error.
module NinjaCat.Api.Query.Tests.TimeRangeTests

open System.Net
open System.Net.Http
open Xunit
open NinjaCat.Api.Query.Tests.TestServer

let private ask (method: HttpMethod, path: string, body: string) (expected: string) =
    withServer config (fun client _ ->
        task {
            use! response = send client (Some(mint (panelToken ()))) (method, path, body)
            let! answer = response.Content.ReadAsStringAsync()
            Assert.Equal(HttpStatusCode.OK, response.StatusCode)
            Assert.Equal(expected, answer)
        })

[<Theory>]
[<InlineData("253402300800")>] // year 10000
[<InlineData("-62135596801")>] // before year 1
let ``v1 metrics`` (from: string) =
    ask (HttpMethod.Get, $"/api/v1/metrics?from={from}", "") $"""{{"from":"{from}","metrics":[]}}"""

[<Theory>]
[<InlineData(253402300800000L, 253402300800001L)>]
[<InlineData(-62135596800001L, -62135596800000L)>]
let ``v2 timeseries`` (from: int64, to': int64) =
    ask
        (HttpMethod.Post,
         "/api/v2/query/timeseries",
         $"""{{"data":{{"type":"timeseries_request","attributes":{{"from":{from},"to":{to'},"queries":[{{"data_source":"metrics","name":"a","query":"avg:x{{*}}"}}],"formulas":[{{"formula":"timeshift(a, -86400)"}}]}}}}}}""")
        """{"data":{"id":"0","type":"timeseries_response","attributes":{"series":[],"times":[],"values":[]}}}"""

[<Theory>]
[<InlineData("avg:x{*}", "timeshift(a, -100000000000)", "")>]
[<InlineData("avg:x{*}.rollup(avg, 100000000000)", "diff(a)", "")>]
[<InlineData("avg:x{*}", "diff(a)", ",\"interval\":9000000000000000000")>]
let ``v2 timeseries shifted or stepped past what a date can hold`` (query: string, formula: string, interval: string) =
    ask
        (HttpMethod.Post,
         "/api/v2/query/timeseries",
         $"""{{"data":{{"type":"timeseries_request","attributes":{{"from":1760000000000,"to":1760003600000{interval},"queries":[{{"data_source":"metrics","name":"a","query":"{query}"}}],"formulas":[{{"formula":"{formula}"}}]}}}}}}""")
        """{"data":{"id":"0","type":"timeseries_response","attributes":{"series":[],"times":[],"values":[]}}}"""

[<Theory>]
[<InlineData(253402300800000L, 253402300800001L)>]
[<InlineData(-62135596800001L, -62135596800000L)>]
let ``v2 scalar`` (from: int64, to': int64) =
    ask
        (HttpMethod.Post,
         "/api/v2/query/scalar",
         $"""{{"data":{{"type":"scalar_request","attributes":{{"from":{from},"to":{to'},"queries":[{{"data_source":"metrics","name":"a","aggregator":"avg","query":"avg:x{{*}}"}}]}}}}}}""")
        """{"data":{"type":"scalar_response","attributes":{"columns":[{"name":"a","type":"number","values":[],"meta":{}}]}}}"""

[<Theory>]
[<InlineData("-40000d", "-30000d")>] // before 1970
[<InlineData("-6000000d", "-5000000d")>] // before year 1
let ``panel metric query`` (from: string, to': string) =
    ask
        (HttpMethod.Get, $"/internal/metrics/query?metric=x&from={from}&to={to'}", "")
        """{"from":"1970-01-01T00:00:00Z","metric":"x","series":[],"step":60,"to":"1970-01-01T00:00:00Z"}"""
