/// Seasonal anomaly models fitted apart from the window they judge: what the
/// HTTP path draws is unchanged by the split, and models fitted once judge
/// later windows.
module NinjaCat.Api.Engine.Tests.AnomalyModelTests

open System
open System.Threading.Tasks
open Xunit
open NinjaCat.Api.Engine
open NinjaCat.Api.Engine.Api.V2.Timeseries
open NinjaCat.Api.Engine.Query
open NinjaCat.Api.Engine.Query.Compile

let private planNow = Plan.plan Plan.defaultRetention (DateTimeOffset.UnixEpoch.AddDays 1.0)

let private int64Param (sql: Sql) (prefix: string) =
    sql.Parameters
    |> List.pick (fun (n, v) ->
        match v with
        | Int64 f when n.StartsWith prefix -> Some f
        | _ -> None)

/// Hourly-seasonal data with a little texture, and a lasting +20 shift from
/// `shiftAt` on, in whatever window and step the engine asks for.
let private seasonal (shiftAt: int64) (sql: Sql) =
    let from, until, step = int64Param sql "from", int64Param sql "to", int64Param sql "step" * 1000L
    let first = (from + step - 1L) / step * step

    let value (t: int64) =
        50.0
        + 10.0 * sin (2.0 * Math.PI * float ((t / 60000L) % 60L) / 60.0)
        + 0.3 * sin (float t / 7e4)
        + (if t >= shiftAt then 20.0 else 0.0)

    Task.FromResult [ for t in first..step .. until - 1L -> { Groups = []; BucketMs = t; Value = value t; SeriesId = None } ]

let private tenant = TenantId "default"

let private planAt (interval: int64) (from: int64) (until: int64) (formula: string) =
    let body =
        $"""{{"data":{{"type":"timeseries_request","attributes":{{"from":{from},"to":{until},"interval":{interval},
            "queries":[{{"data_source":"metrics","name":"a","query":"avg:cpu{{*}}.fill(null)"}}],
            "formulas":[{{"formula":"{formula}"}}]}}}}}}"""

    match read body |> Result.bind planNow with
    | Ok p -> p
    | Error es -> failwith (String.Join("\n", es))

let private planOf = planAt 60000L

let private bands (outputs: Series.OutputResult list) =
    let line = Assert.Single(Assert.Single(outputs).Lines)
    line.Data.Points, defaultArg line.Extra.Band Map.empty

/// What the band adds up to, to pin it: the number of points with one, and
/// the sums of its edges.
let private digest (outputs: Series.OutputResult list) =
    let _, band = bands outputs
    band.Count, band.Values |> Seq.sumBy _.Lower, band.Values |> Seq.sumBy _.Upper

[<Theory>]
[<InlineData("basic", 60, 1567.9956695978, 4891.5251186865)>]
[<InlineData("robust", 60, 2977.6326966654, 3019.8799773412)>]
[<InlineData("agile", 60, 3763.8364062449, 3795.5029735759)>]
let ``the HTTP path's bands over a whole season are pinned`` (algorithm: string, count: int, lower: float, upper: float) =
    let plan = planOf 36000000L 39600000L $"anomalies(a, '{algorithm}', 3, seasonality='hourly')"
    let n, l, u = digest (Execute.evaluate (seasonal 37200000L) tenant plan).Result
    Assert.Equal(count, n)
    Assert.Equal(lower, l, 6)
    Assert.Equal(upper, u, 6)

/// How far each point lies from the middle of its band.
let private offCentre (outputs: Series.OutputResult list) =
    let points, band = bands outputs

    [ for KeyValue(t, v) in points do
          match band.TryFind t with
          | Some r -> abs (v - (r.Lower + r.Upper) / 2.0)
          | None -> () ]

[<Fact>]
let ``robust fitted apart draws the band evaluate draws`` () =
    let plan = planOf 36000000L 39600000L "anomalies(a, 'robust', 3, seasonality='hourly')"
    let execute = seasonal 37200000L
    let models = (Execute.fit execute tenant plan).Result
    Assert.True(models[0].ContainsKey [])
    Assert.Equal<Map<int64, Series.Range>>(snd (bands (Execute.evaluate execute tenant plan).Result), snd (bands (Execute.evaluateWith execute tenant plan (Some models)).Result))

[<Theory>]
[<InlineData("robust")>]
[<InlineData("agile")>]
let ``models fitted at one window judge a later one, in phase`` (algorithm: string) =
    let formula = $"anomalies(a, '{algorithm}', 3, seasonality='hourly')"
    let execute = seasonal Int64.MaxValue
    let models = (Execute.fit execute tenant (planOf 36000000L 39600000L formula)).Result
    // Half an hour later, 45 minutes: not a whole season, so a profile out of
    // phase would miss the season of amplitude 10 by up to 14.
    let later = planOf 39600000L 42300000L formula

    for outputs in [ Execute.evaluate execute tenant later; Execute.evaluateWith execute tenant later (Some models) ] do
        let distances = offCentre outputs.Result
        Assert.Equal(45, distances.Length)
        Assert.InRange(List.max distances, 0.0, 1.0)

[<Theory>]
[<InlineData("robust")>]
[<InlineData("agile")>]
let ``a window that ends off the model's step is expected in phase to its last point`` (algorithm: string) =
    let formula = $"anomalies(a, '{algorithm}', 3, seasonality='hourly')"
    let execute = seasonal Int64.MaxValue
    // Points every 15 s, the window ending half a minute into a model bucket.
    let plan = planAt 15000L 36030000L 39630000L formula
    let models = (Execute.fit execute tenant plan).Result

    for outputs in [ Execute.evaluate execute tenant plan; Execute.evaluateWith execute tenant plan (Some models) ] do
        let distances = offCentre outputs.Result
        Assert.Equal(240, distances.Length)
        Assert.InRange(List.max distances, 0.0, 1.0)

[<Fact>]
let ``agile with a model reads a few seasons, not the whole history`` () =
    let formula = "anomalies(a, 'agile', 3, seasonality='hourly')"
    let reads = ResizeArray<int64>()

    let execute (sql: Sql) =
        lock reads (fun () -> reads.Add(int64Param sql "to" - int64Param sql "from"))
        seasonal Int64.MaxValue sql

    let plan = planOf 36000000L 39600000L formula
    let models = (Execute.fit execute tenant plan).Result
    let fitRead = Seq.max reads
    reads.Clear()
    (Execute.evaluateWith execute tenant plan (Some models)).Result |> ignore
    // Six seasons to fit; three and the window to run.
    Assert.Equal(TimeSpan.FromHours 6.0, TimeSpan.FromMilliseconds(float fitRead) - TimeSpan.FromMinutes 61.0)
    Assert.Equal(TimeSpan.FromHours 4.0, TimeSpan.FromMilliseconds(float (Seq.max reads)) - TimeSpan.FromMinutes 61.0)

[<Fact>]
let ``a group without a model has no band`` () =
    let plan = planOf 36000000L 39600000L "anomalies(a, 'robust', 3, seasonality='hourly')"
    let line = Assert.Single(Assert.Single((Execute.evaluateWith (seasonal Int64.MaxValue) tenant plan (Some Map.empty)).Result).Lines)
    Assert.NotEmpty line.Data.Points
    Assert.True(line.Extra.Band.IsNone)
