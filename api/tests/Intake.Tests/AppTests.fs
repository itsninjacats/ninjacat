/// Tests of Routers/App.fs beyond the golden fixtures.
///
/// The v3 columnar intake has no encoder outside the agent, so the tests
/// bring their own: the helpers below build the columns the way the agent's
/// serializer does, because a payload assembled from literal deltas is
/// unreadable.
module NinjaCat.Api.Intake.Tests.AppTests

open System
open System.IO
open System.IO.Compression
open System.Text
open Google.Protobuf
open Google.Protobuf.Collections
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Xunit
open Datadoghq.Api.Metrics.V3
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Intake.Tests.Golden

/// Sends one request to the intake made of the named Go route sets, the way
/// the golden replay does. Returns the answer and what was written.
let send
    (routeSets: string list)
    (method: string)
    (path: string)
    (headers: (string * string) list)
    (body: byte[])
    : Response * CapturingSink =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let http = DefaultHttpContext()
    http.Request.Method <- method
    http.Request.Host <- HostString "example.com"
    http.Request.Path <- PathString path

    for name, value in headers do
        http.Request.Headers[name] <- StringValues value

    Routes.byGoNames deps routeSets http body, sink

let private same (expected: 'a) (actual: 'a) =
    if expected <> actual then
        Assert.Fail $"want %A{expected}\n got %A{actual}"

let private clean (decoded: App.Decoded) =
    match decoded.Problem with
    | Some problem -> Assert.Fail $"the walk reported: {problem}"
    | None -> ()

// --- the encoder ---

/// A "varint length + bytes" dictionary. The empty string at index 0 is
/// implicit and never written, so the first string here is reference 1.
let private dictBytes (strings: string list) : byte[] =
    use buffer = new MemoryStream()

    for text in strings do
        let bytes = Encoding.UTF8.GetBytes text
        // Every string of these tests is shorter than 128 bytes: one varint byte.
        buffer.WriteByte(byte bytes.Length)
        buffer.Write(bytes, 0, bytes.Length)

    buffer.ToArray()

let private dict (strings: string list) : ByteString = ByteString.CopyFrom(dictBytes strings)

/// Absolute references as the delta column the wire carries.
let private delta (refs: int64 list) : int64 list =
    0L :: refs |> List.pairwise |> List.map (fun (previous, next) -> next - previous)

/// Each set is a length followed by its delta-encoded members.
let private tagsetColumn (sets: int64 list list) : int64[] =
    sets |> List.collect (fun set -> int64 set.Length :: delta set) |> Array.ofList

/// Resource sets given as (typeRef, nameRef) pairs: the lengths, and the two
/// parallel delta columns whose accumulators restart at each set.
let private resourceColumns (sets: (int64 * int64) list list) : int64[] * int64[] * int64[] =
    sets |> List.map (fun set -> int64 set.Length) |> Array.ofList,
    sets |> List.collect (fun set -> delta (List.map fst set)) |> Array.ofList,
    sets |> List.collect (fun set -> delta (List.map snd set)) |> Array.ofList

/// One sketch point's bin keys, delta-encoded.
let private binKeyDeltas (keys: int list) : int list =
    0 :: keys |> List.pairwise |> List.map (fun (previous, next) -> next - previous)

let private packType (kind: metricType) (value: valueType) (flags: metricFlags list) : uint64 =
    flags |> List.fold (fun packed flag -> packed ||| uint64 flag) (uint64 kind ||| uint64 value)

/// Supplies the per-series columns a test does not care about. A real
/// payload carries every one of them for every series, so leaving one out
/// would test a broken payload by accident.
let private fill (md: MetricData) : MetricData =
    let series = md.Types_.Count

    let pad (column: RepeatedField<int64>) =
        while column.Count < series do
            column.Add 0L

    let padUnsigned (column: RepeatedField<uint64>) =
        while column.Count < series do
            column.Add 0UL

    pad md.NameRefs
    pad md.TagsetRefs
    pad md.ResourcesRefs
    pad md.SourceTypeNameRefs
    pad md.OriginInfoRefs
    padUnsigned md.Intervals
    padUnsigned md.NumPoints
    md

/// The worked example: three series off one agent.
///
///   series 0  system.cpu.user        Count  / Sint64   two points, host web-01
///   series 1  http.request.duration  Gauge  / Float64  one point, host web-02,
///                                    a unit, a device resource, and a tagset
///                                    that back-references series 0's tagset
///   series 2  queue.depth            Sketch / Float64  one point, three bins
///
/// Metadata.Tags add env:staging to all three, next to the series' own
/// env:prod: tags are a multiset.
let private examplePayload () : Payload =
    let lens, typeCol, nameCol = resourceColumns [ [ 1L, 2L ]; [ 1L, 5L; 3L, 4L ] ]

    let metadata = Metadata()
    metadata.Tags.Add "env:staging"
    metadata.Resources.AddRange [ "host"; "fallback-host" ]

    let md =
        MetricData(
            DictNameStr = dict [ "system.cpu.user"; "http.request.duration"; "queue.depth" ],
            DictTagStr = dict [ "env:prod"; "kube_service:a"; "kube_service:b"; "role:web" ],
            DictResourceStr = dict [ "host"; "web-01"; "device"; "eth0"; "web-02" ],
            DictSourceTypeName = dict [ "system" ],
            DictUnitStr = dict [ "millisecond" ]
        )

    // env:prod, kube_service:a, kube_service:b — then that set again, plus role:web.
    md.DictTagsets.AddRange(tagsetColumn [ [ 1L; 2L; 3L ]; [ -1L; 4L ] ])
    md.DictResourceLen.AddRange lens
    md.DictResourceType.AddRange typeCol
    md.DictResourceName.AddRange nameCol
    md.DictOriginInfo.AddRange [ 10; 11; 42 ]

    md.Types_.AddRange
        [ packType metricType.Count valueType.Sint64 []
          packType metricType.Gauge valueType.Float64 [ metricFlags.FlagHasUnit ]
          packType metricType.Sketch valueType.Float64 [] ]

    md.NameRefs.AddRange(delta [ 1L; 2L; 3L ])
    md.TagsetRefs.AddRange(delta [ 1L; 2L; 1L ])
    md.ResourcesRefs.AddRange(delta [ 1L; 2L; 1L ])
    md.SourceTypeNameRefs.AddRange(delta [ 1L; 0L; 0L ])
    md.OriginInfoRefs.AddRange(delta [ 1L; 1L; 1L ])
    // Compact: one entry, for the one flagged series.
    md.UnitRefs.AddRange(delta [ 1L ])
    md.Intervals.AddRange [ 15UL; 10UL; 0UL ]
    md.NumPoints.AddRange [ 2UL; 1UL; 1UL ]
    md.Timestamps.AddRange(delta [ 1700000000L; 1700000015L; 1700000020L; 1700000030L ])
    // Series 0's two points, then the sketch count.
    md.ValsSint64.AddRange [ 7L; 9L; 6L ]
    // The gauge, then the sketch's sum, min, max.
    md.ValsFloat64.AddRange [ 1.5; 12.0; 1.0; 9.0 ]
    md.SketchNumBins.Add 3UL
    md.SketchBinKeys.AddRange(binKeyDeltas [ 4; 5; 9 ])
    md.SketchBinCnts.AddRange [ 1u; 2u; 3u ]

    Payload(Metadata = metadata, MetricData = md)

let private at (seconds: int64) : DateTime = Time.fromUnixSeconds seconds

/// One gauge series per name, each with one point and no tags or resources.
let private gauges (value: valueType) (names: string list) : MetricData =
    let md = MetricData(DictNameStr = dict names)

    for _ in names do
        md.Types_.Add(packType metricType.Gauge value [])

    md.NameRefs.AddRange(delta (names |> List.mapi (fun i _ -> int64 i + 1L)))
    md.NumPoints.AddRange(names |> List.map (fun _ -> 1UL))
    md.Timestamps.AddRange(delta (names |> List.mapi (fun i _ -> 1700000000L + int64 i)))
    fill md

// --- the walk ---

[<Fact>]
let ``a v3 batch becomes the rows the v2 intake would have produced`` () =
    let decoded = App.decode "acme" (examplePayload ())
    clean decoded

    let cpuTags = Map [ "env", [| "prod"; "staging" |]; "kube_service", [| "a"; "b" |] ]

    let cpu (seconds: int64) (value: float) : MetricPoint =
        { Metrics.point "acme" (at seconds) "system.cpu.user" value with
            Host = "web-01"
            MetricType = "COUNT"
            SourceType = "system"
            Interval = 15u
            Tags = cpuTags }

    let duration: MetricPoint =
        { Metrics.point "acme" (at 1700000020L) "http.request.duration" 1.5 with
            Host = "web-02"
            MetricType = "GAUGE"
            Unit = "millisecond"
            Interval = 10u
            Tags = cpuTags.Add("role", [| "web" |]) }

    same [| cpu 1700000000L 7.0; cpu 1700000015L 9.0; duration |] decoded.Points

    let sketch: SketchRow =
        { TenantID = "acme"
          Timestamp = at 1700000030L
          Metric = "queue.depth"
          Host = "web-01"
          Tags = cpuTags
          Count = 6UL
          Min = 1.0
          Max = 9.0
          Avg = 2.0
          Sum = 12.0
          BucketKeys = [| 4; 5; 9 |]
          BucketCounts = [| 1u; 2u; 3u |]
          OriginProduct = 0u
          OriginCategory = 0u
          OriginService = 0u
          Resources = Map.empty
          Extra = Map.empty }

    same [| sketch |] decoded.Sketches
    Assert.Equal(0, decoded.WideInts)

[<Fact>]
let ``reference 0 is the empty value and the payload's metadata supplies host and tags`` () =
    let metadata = Metadata()
    metadata.Tags.Add "env:prod"
    metadata.Resources.AddRange [ "host"; "fallback-host"; "device"; "sda" ]

    let md = MetricData(DictNameStr = dict [ "only.metric" ])
    md.Types_.Add(packType metricType.Gauge valueType.Zero [])
    md.NumPoints.Add 1UL
    md.Timestamps.Add 1700000000L

    let decoded = App.decode "acme" (Payload(Metadata = metadata, MetricData = fill md))
    clean decoded

    let point = Assert.Single decoded.Points
    Assert.Equal("", point.Metric)
    Assert.Equal("fallback-host", point.Host)
    // ValueType Zero puts nothing in any value column.
    Assert.Equal(0.0, point.Value)
    same (Map [ "env", [| "prod" |] ]) point.Tags

[<Fact>]
let ``a sint64 beyond 2^53 is counted and still stored`` () =
    let md = gauges valueType.Sint64 [ "bytes.total" ]
    md.NumPoints[0] <- 2UL
    md.Timestamps.Add 15L
    md.ValsSint64.AddRange [ (1L <<< 53) + 3L; 42L ]

    let decoded = App.decode "acme" (Payload(MetricData = md))

    Assert.Equal(1, decoded.WideInts)
    Assert.Equal(2, decoded.Points.Length)
    Assert.Equal(42.0, decoded.Points[1].Value)

/// A sketch summary's count is always a sint64 while sum, min and max follow
/// the value type, so the same summary encodes differently per value type.
let private sketchOf (value: valueType) : MetricData =
    let md = MetricData(DictNameStr = dict [ "latency" ])
    md.Types_.Add(packType metricType.Sketch value [])
    md.NameRefs.Add 1L
    md.NumPoints.Add 1UL
    md.Timestamps.Add 1700000000L
    md.SketchNumBins.Add 2UL
    md.SketchBinKeys.AddRange(binKeyDeltas [ -3; 7 ])
    md.SketchBinCnts.AddRange [ 4u; 6u ]
    fill md

let private assertSketch (md: MetricData) (count: uint64) (sum: float) (min: float) (max: float) (avg: float) =
    let decoded = App.decode "acme" (Payload(MetricData = md))
    clean decoded

    let expected: SketchRow =
        { TenantID = "acme"
          Timestamp = at 1700000000L
          Metric = "latency"
          Host = ""
          Tags = Map.empty
          Count = count
          Min = min
          Max = max
          Avg = avg
          Sum = sum
          BucketKeys = [| -3; 7 |]
          BucketCounts = [| 4u; 6u |]
          OriginProduct = 0u
          OriginCategory = 0u
          OriginService = 0u
          Resources = Map.empty
          Extra = Map.empty }

    same [| expected |] decoded.Sketches

[<Fact>]
let ``a float64 sketch summary has its count alongside in the sint64 column`` () =
    let md = sketchOf valueType.Float64
    md.ValsFloat64.AddRange [ 20.0; 1.0; 9.0 ]
    md.ValsSint64.Add 10L
    assertSketch md 10UL 20.0 1.0 9.0 2.0

[<Fact>]
let ``a float32 sketch summary reads three float32 values`` () =
    let md = sketchOf valueType.Float32
    md.ValsFloat32.AddRange [ 20.0f; 1.0f; 9.0f ]
    md.ValsSint64.Add 10L
    assertSketch md 10UL 20.0 1.0 9.0 2.0

[<Fact>]
let ``a sint64 sketch summary spends four consecutive entries: sum, min, max, count`` () =
    let md = sketchOf valueType.Sint64
    md.ValsSint64.AddRange [ 20L; 1L; 9L; 10L ]
    assertSketch md 10UL 20.0 1.0 9.0 2.0

[<Fact>]
let ``a zero sketch summary still spends one sint64 on the count`` () =
    let md = sketchOf valueType.Zero
    md.ValsSint64.Add 5L
    assertSketch md 5UL 0.0 0.0 0.0 0.0

[<Fact>]
let ``the walk stops at a short value column and keeps the rows it read`` () =
    let md = gauges valueType.Float64 [ "a"; "b" ]
    // One value for two points.
    md.ValsFloat64.Add 1.5

    let decoded = App.decode "acme" (Payload(MetricData = md))

    Assert.Equal(Some $"valsFloat64 ran out at series 1 point 0: {App.truncated}", decoded.Problem)
    Assert.Equal(1.5, (Assert.Single decoded.Points).Value)

/// Two gauges with every per-series column present.
let private complete () : MetricData =
    let md = gauges valueType.Float64 [ "a"; "b" ]
    md.DictSourceTypeName <- dict [ "system" ]
    md.DictOriginInfo.AddRange [ 10; 11; 42 ]
    md.SourceTypeNameRefs[0] <- 1L
    md.SourceTypeNameRefs[1] <- -1L
    md.OriginInfoRefs[0] <- 1L
    md.OriginInfoRefs[1] <- -1L
    md.Intervals[0] <- 15UL
    md.Intervals[1] <- 15UL
    md.ValsFloat64.AddRange [ 1.5; 2.5 ]
    md

[<Fact>]
let ``a payload with every per-series column decodes cleanly`` () =
    let decoded = App.decode "acme" (Payload(MetricData = complete ()))
    clean decoded
    Assert.Equal(2, decoded.Points.Length)

[<Theory>]
[<InlineData("nameRefs")>]
[<InlineData("tagsetRefs")>]
[<InlineData("resourcesRefs")>]
[<InlineData("numPoints")>]
[<InlineData("sourceTypeNameRefs")>]
[<InlineData("originInfoRefs")>]
[<InlineData("intervals")>]
let ``an empty per-series column is reported by name`` (column: string) =
    let md = complete ()

    match column with
    | "nameRefs" -> md.NameRefs.Clear()
    | "tagsetRefs" -> md.TagsetRefs.Clear()
    | "resourcesRefs" -> md.ResourcesRefs.Clear()
    | "numPoints" -> md.NumPoints.Clear()
    | "sourceTypeNameRefs" -> md.SourceTypeNameRefs.Clear()
    | "originInfoRefs" -> md.OriginInfoRefs.Clear()
    | _ -> md.Intervals.Clear()

    let decoded = App.decode "acme" (Payload(MetricData = md))
    Assert.Equal(Some $"{column} has 0 entries for 2 series", decoded.Problem)

[<Fact>]
let ``a missing payload, missing metric data and an empty one give no rows`` () =
    let metadataOnly = Payload(Metadata = Metadata())
    metadataOnly.Metadata.Tags.Add "env:prod"

    for payload in [ null; Payload(); Payload(MetricData = MetricData()); metadataOnly ] do
        let decoded = App.decode "acme" payload
        Assert.Empty decoded.Points
        Assert.Empty decoded.Sketches

[<Fact>]
let ``an odd metadata resources list is reported and its complete pairs still read`` () =
    let metadata = Metadata()
    metadata.Resources.AddRange [ "host"; "web-01"; "device" ]
    let md = gauges valueType.Zero [ "m" ]

    let decoded = App.decode "acme" (Payload(Metadata = metadata, MetricData = md))

    Assert.Equal(Some "metadata.resources has 3 elements, want [Type, Name] pairs", decoded.Problem)
    Assert.Equal("web-01", (Assert.Single decoded.Points).Host)

[<Fact>]
let ``a reference outside its dictionary is reported and the point kept`` () =
    let md = gauges valueType.Zero [ "m" ]
    md.NameRefs[0] <- 5L

    let decoded = App.decode "acme" (Payload(MetricData = md))

    Assert.Equal(Some $"series 0 nameRef 5: {App.badReference}", decoded.Problem)
    Assert.Equal("", (Assert.Single decoded.Points).Metric)

[<Fact>]
let ``an unknown value type is reported and the point kept as zero`` () =
    let md = gauges valueType.Zero [ "m" ]
    md.Types_[0] <- uint64 metricType.Gauge ||| 0x40UL

    let decoded = App.decode "acme" (Payload(MetricData = md))

    Assert.Equal(Some "series 0: unknown valueType 0x40", decoded.Problem)
    Assert.Equal(0.0, (Assert.Single decoded.Points).Value)

[<Theory>]
[<InlineData(true)>]
[<InlineData(false)>]
let ``the compact and the parallel unit layouts put the same unit on the same metric`` (compact: bool) =
    let md = MetricData(DictNameStr = dict [ "plain"; "timed" ], DictUnitStr = dict [ "millisecond" ])
    md.Types_.Add(packType metricType.Gauge valueType.Zero [])
    md.Types_.Add(packType metricType.Gauge valueType.Zero [ metricFlags.FlagHasUnit ])
    md.NameRefs.AddRange(delta [ 1L; 2L ])
    md.NumPoints.AddRange [ 1UL; 1UL ]
    md.Timestamps.AddRange(delta [ 1700000000L; 1700000001L ])
    md.UnitRefs.AddRange(if compact then delta [ 1L ] else delta [ 0L; 1L ])

    let decoded = App.decode "acme" (Payload(MetricData = fill md))
    clean decoded

    same [ ""; "millisecond" ] (decoded.Points |> Array.map _.Unit |> List.ofArray)

// --- the column readers ---

[<Fact>]
let ``a string dictionary is base-1 with the empty string implicit at 0`` () =
    same ([| "" |], None) (App.strDict [||])
    same ([| ""; "a"; "bb" |], None) (App.strDict (dictBytes [ "a"; "bb" ]))
    // An empty string is a legal entry.
    same ([| ""; ""; "a" |], None) (App.strDict (dictBytes [ ""; "a" ]))

[<Fact>]
let ``a truncated string dictionary keeps what it read and says so`` () =
    // Cut short, it must not silently shorten the dictionary: every
    // reference past the cut would resolve to the wrong string.
    let cut = Array.append (dictBytes [ "ok" ]) [| 0x05uy; 'a'B; 'b'B |]
    same ([| ""; "ok" |], Some App.truncated) (App.strDict cut)

    // A length that never ends, and one too long for 64 bits.
    same ([| "" |], Some App.truncated) (App.strDict [| 0x80uy |])
    same ([| "" |], Some App.truncated) (App.strDict (Array.append (Array.create 10 0xFFuy) [| 0x01uy |]))

let private tagStr = [| ""; "env:prod"; "kube_service:a"; "kube_service:b" |]

[<Fact>]
let ``no tagsets is the implicit empty set alone`` () =
    same ([| [||] |], None) (App.tagsets [||] tagStr)

[<Fact>]
let ``two tags sharing a key stay two tags`` () =
    same ([| [||]; [| "kube_service:a"; "kube_service:b" |] |], None) (App.tagsets (tagsetColumn [ [ 2L; 3L ] ]) tagStr)

[<Fact>]
let ``a negative tagset index includes an earlier set whole`` () =
    // Why the format is dense at all: every series of a host shares the
    // host's tags, so the second set says "the first set, plus one".
    let expected =
        [| [||]
           [| "env:prod"; "kube_service:a" |]
           [| "env:prod"; "kube_service:a"; "kube_service:b" |] |]

    same (expected, None) (App.tagsets (tagsetColumn [ [ 1L; 2L ]; [ -1L; 3L ] ]) tagStr)

[<Fact>]
let ``a tagset longer than what follows, or pointing outside a dictionary, is reported`` () =
    same ([| [||] |], Some App.truncated) (App.tagsets [| 4L; 1L |] tagStr)
    same ([| [||] |], Some App.badReference) (App.tagsets (tagsetColumn [ [ 9L ] ]) tagStr)
    // A back-reference to a set that does not exist yet.
    same ([| [||] |], Some App.badReference) (App.tagsets (tagsetColumn [ [ -1L ] ]) tagStr)

[<Fact>]
let ``resource deltas restart at every set`` () =
    // Read as one run across the dictionary, like every other delta column,
    // every series after the first would get the wrong host name.
    let strs = [| ""; "host"; "web-01"; "device"; "eth0"; "web-02" |]
    let lens, typeCol, nameCol = resourceColumns [ [ 1L, 2L ]; [ 1L, 5L; 3L, 4L ] ]

    let expected: App.Resource[][] =
        [| [||]
           [| { Type = "host"; Name = "web-01" } |]
           [| { Type = "host"; Name = "web-02" }; { Type = "device"; Name = "eth0" } |] |]

    same (expected, None) (App.resources lens typeCol nameCol strs)

    Assert.Equal(Some App.truncated, snd (App.resources [| 3L |] [| 1L |] [| 1L |] strs))
    Assert.Equal(Some App.badReference, snd (App.resources [| 1L |] [| 99L |] [| 1L |] strs))

[<Fact>]
let ``origins are base-1 triples and a partial triple is reported`` () =
    let expected: App.Origin[] =
        [| { Product = 0u; Category = 0u; Service = 0u }
           { Product = 10u; Category = 11u; Service = 42u }
           { Product = 1u; Category = 0u; Service = 7u } |]

    same (expected, None) (App.origins [| 10; 11; 42; 1; 0; 7 |])
    Assert.Equal(Some App.truncated, snd (App.origins [| 10; 11 |]))

[<Fact>]
let ``the length of unitRefs decides its layout`` () =
    let flagged = packType metricType.Gauge valueType.Float64 [ metricFlags.FlagHasUnit ]
    let plain = packType metricType.Gauge valueType.Float64 []

    same (App.NoUnits, None) (App.unitLayout [| plain; flagged |] 0)
    same (App.Parallel, None) (App.unitLayout [| plain; flagged |] 2)
    same (App.Compact, None) (App.unitLayout [| plain; flagged; plain |] 1)

    // Neither reading fits: the units are dropped rather than guessed.
    let layout, problem = App.unitLayout [| plain; flagged; plain |] 2
    Assert.Equal(App.NoUnits, layout)

    Assert.Equal(
        Some "unitRefs has 2 entries: neither one per series (3) nor one per flagHasUnit series (1), units dropped",
        problem
    )

// --- the handler ---

let private seriesPath = "/api/intake/metrics/v3/series"

[<Fact>]
let ``without a tenant nothing is stored and the answer is the same`` () =
    let sink = CapturingSink()
    let http = DefaultHttpContext()
    http.Request.Method <- "POST"
    http.Request.Path <- PathString seriesPath

    let request: Request =
        { Http = http
          Body = (examplePayload ()).ToByteArray()
          Params = Map.empty
          Key = None
          Sink = sink
          Log = NullLogger.Instance }

    let route = App.routes |> List.find (fun route -> route.Pattern = seriesPath)
    let response = route.Handler request

    Assert.Equal(202, response.Status)
    Assert.Equal("{}", Encoding.UTF8.GetString response.Body)
    Assert.Empty sink.Writes

let private zstdFrame (data: byte[]) : byte[] =
    use compressor = new ZstdSharp.Compressor()
    compressor.Wrap(data).ToArray()

let private gzipMember (data: byte[]) : byte[] =
    use buffer = new MemoryStream()

    do
        use gzip = new GZipStream(buffer, CompressionMode.Compress)
        gzip.Write(data, 0, data.Length)

    buffer.ToArray()

[<Theory>]
[<InlineData("zstd")>]
[<InlineData("gzip")>]
let ``a body of concatenated compressed frames is read as one payload`` (encoding: string) =
    // The agent compresses every column as a frame of its own.
    let body = (examplePayload ()).ToByteArray()
    let frame = if encoding = "zstd" then zstdFrame else gzipMember
    let framed = Array.concat [ frame body[..39]; frame body[40..199]; frame body[200..] ]

    let response, sink =
        send [ "routeApp" ] "POST" seriesPath [ "Dd-Api-Key", Replay.testKey; "Content-Encoding", encoding ] framed

    Assert.Equal(202, response.Status)
    Assert.Equal(3, sink.Rows<MetricPoint>().Length)
    Assert.Equal(1, sink.Rows<SketchRow>().Length)
    Assert.Empty(sink.Rows<RawPayloadRow>())
