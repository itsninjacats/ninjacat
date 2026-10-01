module NinjaCat.Api.Engine.Tests.SeriesTests

open System
open System.Threading.Tasks
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Api.V2.Timeseries
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Query.Compile

let private row groups bucket value = { Groups = List.map Some groups; BucketMs = bucket; Value = value; SeriesId = None }

// --- series from rows ---------------------------------------------------------

[<Fact>]
let ``rows group into one series per group`` () =
    let series =
        Series.fromRows [ "host" ] 0L [ row [ "web-1" ] 0L 1.0; row [ "web-1" ] 20L 3.0; row [ "web-2" ] 20L 7.0 ]

    Assert.Equal<string list list>([ [ "host:web-1" ]; [ "host:web-2" ] ], series |> List.map _.GroupTags)
    Assert.Equal<Map<int64, float>>(Map [ 0L, 1.0; 20L, 3.0 ], series[0].Points)

[<Fact>]
let ``a series without the tag is the N/A group`` () =
    let rows = [ { Groups = [ Some "web-1"; None ]; BucketMs = 0L; Value = 1.0; SeriesId = None } ]
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
        Task.FromResult [ { Groups = []; BucketMs = from; Value = (if from = 0L then 1.0 else 2.0); SeriesId = None } ]

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

// --- filling and combining ---------------------------------------------------------------

let private seriesRow id bucket value = { Groups = [ Some "prod" ]; BucketMs = bucket; Value = value; SeriesId = Some id }

/// Two hosts in one group, every 20 s. Host 1 misses the bucket at 40 s.
///   host 1:  10   20   --   40
///   host 2:   1    2    3    4
let private twoHosts =
    [ seriesRow 1UL 0L 10.0; seriesRow 1UL 20000L 20.0; seriesRow 1UL 60000L 40.0
      seriesRow 2UL 0L 1.0; seriesRow 2UL 20000L 2.0; seriesRow 2UL 40000L 3.0; seriesRow 2UL 60000L 4.0 ]

let private summed fill rows =
    Series.combine [ "env" ] Plan.Sum fill 0L rows |> List.exactlyOne |> _.Points |> Map.toList

[<Fact>]
let ``without a fill the sum dips where a host missed a bucket`` () =
    Assert.Equal<(int64 * float) list>([ 0L, 11.0; 20000L, 22.0; 40000L, 3.0; 60000L, 44.0 ], summed Plan.NoFill twoHosts)

[<Fact>]
let ``linear fill aligns the host first`` () =
    // Host 1 at 40 s: halfway between 20 and 40 → 30; sum 30 + 3.
    Assert.Equal<(int64 * float) list>([ 0L, 11.0; 20000L, 22.0; 40000L, 33.0; 60000L, 44.0 ], summed (Plan.FillWithin(Plan.Linear, 300)) twoHosts)

[<Fact>]
let ``last and zero fills`` () =
    Assert.Equal<(int64 * float) list>([ 0L, 11.0; 20000L, 22.0; 40000L, 23.0; 60000L, 44.0 ], summed (Plan.FillWithin(Plan.Last, 300)) twoHosts)
    Assert.Equal<(int64 * float) list>([ 0L, 11.0; 20000L, 22.0; 40000L, 3.0; 60000L, 44.0 ], summed (Plan.FillWithin(Plan.Zero, 300)) twoHosts)

[<Fact>]
let ``a gap longer than the limit stays a gap`` () =
    // 20 s after host 1's last point, with a 10 s limit: not filled.
    Assert.Equal<(int64 * float) list>([ 0L, 11.0; 20000L, 22.0; 40000L, 3.0; 60000L, 44.0 ], summed (Plan.FillWithin(Plan.Linear, 10)) twoHosts)

[<Fact>]
let ``a series alone in its group is not filled`` () =
    // Nothing to align with: interpolation is for combining series.
    let alone = [ seriesRow 1UL 0L 10.0; seriesRow 1UL 40000L 30.0 ]
    Assert.Equal<(int64 * float) list>([ 0L, 10.0; 40000L, 30.0 ], summed (Plan.FillWithin(Plan.Linear, 300)) alone)

[<Fact>]
let ``the window grid and zero alignment`` () =
    Assert.Equal<int64 list>([ 20000L; 40000L ], Series.grid 20000L 1L 60000L |> List.ofSeq)
    let s = Series.zeroFill [ 0L; 20000L ] { GroupTags = []; Points = Map [ 20000L, 5.0 ] }
    Assert.Equal<Map<int64, float>>(Map [ 0L, 0.0; 20000L, 5.0 ], s.Points)

// --- extras in the response ------------------------------------------------------------------

[<Fact>]
let ``extras appear only when asked, as ninjacat_ fields`` () =
    let s = { Series.Series.GroupTags = [ "host:a" ]; Series.Series.Points = Map [ 0L, 1.0; 20000L, 2.0 ] }
    let plain = NinjaCat.Api.Engine.Json.options |> fun o -> System.Text.Json.JsonSerializer.Serialize(Series.response [ 0, [ s ] ], o)
    Assert.DoesNotContain("ninjacat_", plain)

    let extra: Series.SeriesExtra =
        { Band = Some(Map [ 20000L, ({ Lower = 1.5; Upper = 2.5 }: Series.Range) ])
          Forecast = Some [ { TimeMs = 40000L; Value = 3.0; Range = { Lower = 2.0; Upper = 4.0 } } ] }

    let outputs: Series.OutputResult list = [ { QueryIndex = 0; Lines = [ { Data = s; Extra = extra } ] } ]
    let json = System.Text.Json.JsonSerializer.Serialize(Series.responseWith outputs, NinjaCat.Api.Engine.Json.options)
    Assert.Contains("\"times\":[0,20000]", json)
    Assert.Contains("\"ninjacat_bounds\":[{\"upper\":[null,2.5],\"lower\":[null,1.5]}]", json)
    Assert.Contains("\"ninjacat_forecast\":[{\"times\":[40000],\"values\":[3],\"upper\":[4],\"lower\":[2]}]", json)

[<Fact>]
let ``a forecast runs end to end, past the window`` () =
    let body =
        """{"data":{"type":"timeseries_request","attributes":{"from":0,"to":100000,"interval":20000,
            "queries":[{"data_source":"metrics","name":"a","query":"avg:cpu{*}"}],
            "formulas":[{"formula":"forecast(a, 'linear', 1, horizon='40s')"}]}}}"""

    // A line: value = bucket seconds.
    let execute (_: Sql) = Task.FromResult [ for t in 0L .. 20000L .. 80000L -> { Groups = []; BucketMs = t; Value = float t / 1000.0; SeriesId = None } ]

    let plan =
        match read body |> Result.bind Plan.plan with
        | Ok p -> p
        | Error es -> failwith (String.Join("\n", es))

    let a = (Execute.run execute (TenantId "default") plan).Result.Data.Attributes
    Assert.Equal<int64 list>([ 0L; 20000L; 40000L; 60000L; 80000L ], a.Times) // the window only
    let f = (Option.get a.NinjacatForecast).Head |> Option.get
    Assert.Equal<int64 list>([ 100000L; 120000L ], f.Times)
    Assert.Equal(100.0, f.Values[0], 6)
    Assert.Equal(120.0, f.Values[1], 6)

[<Fact>]
let ``a seasonal forecast fits the history and continues the season`` () =
    // Hourly seasonality: every hour the same shape, minute by minute. The
    // airline model on a perfectly periodic history predicts the shape again.
    let body =
        """{"data":{"type":"timeseries_request","attributes":{"from":36000000,"to":39600000,"interval":60000,
            "queries":[{"data_source":"metrics","name":"a","query":"avg:cpu{*}.fill(null)"}],
            "formulas":[{"formula":"forecast(a, 'seasonal', 1, seasonality='hourly', horizon='10m')"}]}}}"""

    let shape (t: int64) = 50.0 + 10.0 * sin (2.0 * Math.PI * float ((t / 60000L) % 60L) / 60.0)

    let execute (sql: Sql) =
        let int64Param (prefix: string) = sql.Parameters |> List.pick (fun (n, v) -> match v with Int64 f when n.StartsWith prefix -> Some f | _ -> None)
        let from, until, step = int64Param "from", int64Param "to", int64Param "step" * 1000L
        let first = (from + step - 1L) / step * step
        Task.FromResult [ for t in first .. step .. until - 1L -> { Groups = []; BucketMs = t; Value = shape t; SeriesId = None } ]

    let plan =
        match read body |> Result.bind Plan.plan with
        | Ok p -> p
        | Error es -> failwith (String.Join("\n", es))

    let a = (Execute.run execute (TenantId "default") plan).Result.Data.Attributes
    let f = (Option.get a.NinjacatForecast).Head |> Option.get
    Assert.Equal(10, f.Times.Length)
    for t, v in List.zip f.Times f.Values do
        Assert.Equal(shape t, v, 6)

/// Hourly-seasonal data at 1-minute buckets, with a lasting +20 shift from
/// `shiftAt` on; the engine asks for whatever window and step it needs.
let private seasonalWithShift (shiftAt: int64) (sql: Sql) =
    let int64Param (prefix: string) = sql.Parameters |> List.pick (fun (n, v) -> match v with Int64 f when n.StartsWith prefix -> Some f | _ -> None)
    let from, until, step = int64Param "from", int64Param "to", int64Param "step" * 1000L
    let first = (from + step - 1L) / step * step
    let value (t: int64) =
        50.0 + 10.0 * sin (2.0 * Math.PI * float ((t / 60000L) % 60L) / 60.0)
        + 0.3 * sin (float t / 7e4) // a little texture, so residuals are not zero
        + (if t >= shiftAt then 20.0 else 0.0)
    Task.FromResult [ for t in first .. step .. until - 1L -> { Groups = []; BucketMs = t; Value = value t; SeriesId = None } ]

let private bandAfterShift algorithm =
    // Window: hour 10 to hour 11; the shift lands at 10:20.
    let body =
        $"""{{"data":{{"type":"timeseries_request","attributes":{{"from":36000000,"to":39600000,"interval":60000,
            "queries":[{{"data_source":"metrics","name":"a","query":"avg:cpu{{*}}.fill(null)"}}],
            "formulas":[{{"formula":"anomalies(a, '{algorithm}', 3, seasonality='hourly')"}}]}}}}}}"""

    let plan =
        match read body |> Result.bind Plan.plan with
        | Ok p -> p
        | Error es -> failwith (String.Join("\n", es))

    let a = (Execute.run (seasonalWithShift 37200000L) (TenantId "default") plan).Result.Data.Attributes
    let b = (Option.get a.NinjacatBounds).Head |> Option.get
    // Points 30–60 minutes into the window: well after the shift.
    [ for i in 0 .. a.Times.Length - 1 do
          if a.Times[i] >= 37800000L then
              match a.Values[0][i], b.Lower[i], b.Upper[i] with
              | Some v, Some lo, Some hi -> yield v < lo || v > hi
              | _ -> () ]

[<Fact>]
let ``robust keeps expecting the season through a lasting shift`` () =
    let outside = bandAfterShift "robust"
    Assert.NotEmpty outside
    Assert.True(List.forall id outside, "every shifted point should lie outside robust's band")

[<Fact>]
let ``agile follows a lasting shift`` () =
    let outside = bandAfterShift "agile"
    Assert.NotEmpty outside
    Assert.True(outside |> List.filter id |> List.length < outside.Length / 4, "agile should have caught up with the shift")
