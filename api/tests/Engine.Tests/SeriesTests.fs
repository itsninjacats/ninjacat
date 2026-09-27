module NinjaCat.Api.Engine.Tests.SeriesTests

open System
open System.Threading.Tasks
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Api.V2.Timeseries
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Query.Compile

let private row groups bucket value = { Groups = List.map Some groups; BucketMs = bucket; Value = value }

// --- series from rows ---------------------------------------------------------

[<Fact>]
let ``rows group into one series per group`` () =
    let series =
        Series.fromRows [ "host" ] 0L [ row [ "web-1" ] 0L 1.0; row [ "web-1" ] 20L 3.0; row [ "web-2" ] 20L 7.0 ]

    Assert.Equal<string list list>([ [ "host:web-1" ]; [ "host:web-2" ] ], series |> List.map _.GroupTags)
    Assert.Equal<Map<int64, float>>(Map [ 0L, 1.0; 20L, 3.0 ], series[0].Points)

[<Fact>]
let ``a series without the tag is the N/A group`` () =
    let rows = [ { Groups = [ Some "web-1"; None ]; BucketMs = 0L; Value = 1.0 } ]
    let s = Assert.Single(Series.fromRows [ "host"; "role" ] 0L rows)
    Assert.Equal<string list>([ "host:web-1"; "role:N/A" ], s.GroupTags)

[<Fact>]
let ``no group by is one series without tags`` () =
    let s = Assert.Single(Series.fromRows [] 0L [ row [] 0L 1.0; row [] 20L 2.0 ])
    Assert.Empty s.GroupTags

[<Fact>]
let ``a bucket starting before from is dropped`` () =
    let s = Assert.Single(Series.fromRows [] 1000L [ row [] 980L 1.0; row [] 1000L 2.0 ])
    Assert.Equal<int64 list>([ 1000L ], s.Points.Keys |> List.ofSeq)

// --- the response ---------------------------------------------------------------

[<Fact>]
let ``gaps are null, not zero, on a shared time axis`` () =
    // The example from the ClickHouse check: web-1 misses 12:00:20, web-2 misses 12:00:00.
    let series = Series.fromRows [ "host" ] 0L [ row [ "web-1" ] 0L 1.0; row [ "web-1" ] 40L 3.0; row [ "web-2" ] 20L 7.0; row [ "web-2" ] 40L 9.0 ]
    let r = (Series.response [ 0, series ]).Data.Attributes
    Assert.Equal<int64 list>([ 0L; 20L; 40L ], r.Times)
    Assert.Equal<float option list list>([ [ Some 1.0; None; Some 3.0 ]; [ None; Some 7.0; Some 9.0 ] ], r.Values)

[<Fact>]
let ``no data is an empty response, not an error`` () =
    let r = (Series.response [ 0, [] ]).Data
    Assert.Equal("timeseries_response", r.Type)
    Assert.Empty r.Attributes.Series
    Assert.Empty r.Attributes.Times

[<Fact>]
let ``limit keeps the top series by mean`` () =
    let s name v = { Series.GroupTags = [ name ]; Series.Points = Map [ 0L, v ] }
    let kept = Series.limit (Some { Count = Some 2; Order = Desc }) [ s "a" 1.0; s "b" 3.0; s "c" 2.0 ]
    Assert.Equal<string list list>([ [ "b" ]; [ "c" ] ], kept |> List.map _.GroupTags)

// --- the whole run, with ClickHouse replaced by a function ------------------------

[<Fact>]
let ``a request runs end to end`` () =
    let body =
        """{"data":{"type":"timeseries_request","attributes":{"from":0,"to":3600000,
            "queries":[{"data_source":"metrics","name":"a","query":"avg:cpu{*} by {host}"},
                       {"data_source":"metrics","name":"unused","query":"avg:other{*}"}],
            "formulas":[{"formula":"a"}]}}}"""

    let asked = ResizeArray<Sql>()

    let execute (sql: Sql) =
        asked.Add sql
        Task.FromResult [ row [ "web-1" ] 0L 41.0; row [ "web-1" ] 20000L 40.5 ]

    let plan =
        match read body |> Result.bind Plan.plan with
        | Ok p -> p
        | Error es -> failwith (String.Join("\n", es))

    let r = (Execute.run execute (TenantId "default") plan).Result.Data.Attributes

    // Only the query a formula uses went to the database.
    Assert.Equal(String "cpu", Assert.Single(asked).Parameters |> List.find (fst >> _.StartsWith("metric")) |> snd)
    Assert.Equal<string list>([ "host:web-1" ], (Assert.Single r.Series).GroupTags)
    Assert.Equal<int64 list>([ 0L; 20000L ], r.Times)
    Assert.Equal<float option list list>([ [ Some 41.0; Some 40.5 ] ], r.Values)

[<Fact>]
let ``a shifted read queries the past and draws it now`` () =
    let body =
        """{"data":{"type":"timeseries_request","attributes":{"from":86400000,"to":90000000,
            "queries":[{"data_source":"metrics","name":"a","query":"avg:cpu{*}"}],
            "formulas":[{"formula":"a"},{"formula":"day_before(a)"}]}}}"""

    let asked = System.Collections.Concurrent.ConcurrentBag<int64>()

    let execute (sql: Sql) =
        let from = sql.Parameters |> List.pick (fun (n, v) -> match v with Int64 f when n.StartsWith "from" -> Some f | _ -> None)
        asked.Add from
        // Whatever window was asked, one point at its start, valued by window.
        Task.FromResult [ { Groups = []; BucketMs = from; Value = (if from = 0L then 1.0 else 2.0) } ]

    let plan =
        match read body |> Result.bind Plan.plan with
        | Ok p -> p
        | Error es -> failwith (String.Join("\n", es))

    let r = (Execute.run execute (TenantId "default") plan).Result.Data.Attributes

    // Two reads: the window itself, and the same window a day earlier.
    Assert.Equal<int64 list>([ 0L; 86400000L ], asked |> Seq.sort |> List.ofSeq)
    // Both drawn at the present window's start.
    Assert.Equal<int64 list>([ 86400000L ], r.Times)
    Assert.Equal<float option list list>([ [ Some 2.0 ]; [ Some 1.0 ] ], r.Values)
