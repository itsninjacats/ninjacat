/// Tests of Routers/Trace.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.TraceTests

open System
open System.IO
open System.Text
open System.Text.Json.Nodes
open Google.Protobuf
open Microsoft.Extensions.Logging.Abstractions
open Xunit
open Datadog.Trace
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows
open NinjaCat.Api.Intake.Tests.Golden

let private receivedAt = DateTime(2026, 3, 4, 5, 6, 7, DateTimeKind.Utc)

// --- requests recorded from the Go server ---

/// Fixtures/trace: what the Go server answered and stored for payloads its
/// own tests never sent through HTTP — the idx wire format, real sketches,
/// every msgp error, unknown Data Streams fields. Recorded the same way as
/// Fixtures/go, from the requests in Fixtures/trace/recorded_requests_test.go.txt.
let private recorded: Replay.Fixture list =
    Directory.GetFiles(Path.Combine(AppContext.BaseDirectory, "Fixtures", "trace"), "*.json", SearchOption.AllDirectories)
    |> Array.sort
    |> Array.map (fun path ->
        let json = JsonNode.Parse(File.ReadAllText path)

        ({ Id = Path.GetFileName(Path.GetDirectoryName path) + "/" + Path.GetFileNameWithoutExtension path
           Json = json
           RouteSets = [ "routeTrace" ] }
        : Replay.Fixture))
    |> List.ofArray

let recordedIds: obj[] seq = recorded |> Seq.map (fun f -> [| box f.Id |])

[<Theory>]
[<MemberData(nameof recordedIds)>]
let ``a recorded trace request is answered and stored as its fixture says`` (id: string) =
    let fixture = recorded |> List.find (fun f -> f.Id = id)

    match Replay.run fixture with
    | [] -> ()
    | differences -> Assert.Fail(String.concat "\n" (id :: differences))

// --- /api/v0.2/traces, v0.4 ---

/// The value of attribute `key` under its type tag, as JSON text.
let private tagged (json: string) (key: string) (tag: string) : string =
    let attribute = (JsonNode.Parse json)[key]
    let value = attribute[tag]
    value.ToJsonString()

let private v04Payload (tracer: TracerPayload) : AgentPayload =
    let payload = AgentPayload()
    payload.TracerPayloads.Add tracer
    payload

let private tracerOf (spans: Span list) : TracerPayload =
    let chunk = TraceChunk()
    chunk.Spans.AddRange spans
    let tracer = TracerPayload()
    tracer.Chunks.Add chunk
    tracer

[<Fact>]
let ``every span row carries its agent, tracer and chunk`` () =
    let root =
        Span(
            Service = "checkout",
            Name = "flask.request",
            Resource = "GET /pay",
            Type = "web",
            TraceID = 111UL,
            SpanID = 222UL,
            Start = 1_700_000_000_000_000_123L,
            Duration = 5_000_000L
        )

    root.Meta["span.kind"] <- "server"
    root.Meta["_dd.p.tid"] <- "67890abcdef01234"
    root.Metrics["_sampling_priority_v1"] <- 1.0

    let child =
        Span(
            Service = "checkout",
            Name = "postgres.query",
            Resource = "SELECT 1",
            Type = "db",
            TraceID = 111UL,
            SpanID = 333UL,
            ParentID = 222UL,
            Start = 1_700_000_000_001_000_000L,
            Duration = 900_000L,
            Error = 1
        )

    let chunk = TraceChunk(Priority = 1, Origin = "lambda")
    chunk.Tags["_dd.p.dm"] <- "-1"
    chunk.Spans.Add root
    chunk.Spans.Add child

    let tracer =
        TracerPayload(
            ContainerID = "abc123",
            LanguageName = "python",
            LanguageVersion = "3.12.1",
            TracerVersion = "2.9.0",
            RuntimeID = "0f4c-runtime",
            Env = "prod",
            Hostname = "pod-1",
            AppVersion = "1.4.0"
        )

    tracer.Tags["team"] <- "payments"
    tracer.Chunks.Add chunk

    let payload =
        AgentPayload(HostName = "ip-10-0-0-1", Env = "prod", AgentVersion = "7.58.2", TargetTPS = 10.0, ErrorTPS = 10.0, RareSamplerEnabled = true)

    payload.Tags["cluster"] <- "eu-1"
    payload.TracerPayloads.Add tracer

    let rows = Trace.spanRows "t1" receivedAt payload
    Assert.Equal(2, rows.Length)

    for row in rows do
        Assert.Equal("t1", row.TenantID)
        Assert.Equal("v04", row.WireFormat)
        Assert.Equal("ip-10-0-0-1", row.AgentHostname)
        Assert.Equal("7.58.2", row.AgentVersion)
        Assert.Equal(1uy, row.RareSamplerEnabled)
        Assert.Equal(10.0, row.TargetTPS)
        Assert.Equal("python", row.Language)
        Assert.Equal("0f4c-runtime", row.RuntimeID)
        Assert.Equal("abc123", row.ContainerID)
        Assert.Equal(1, row.Priority)
        Assert.Equal("lambda", row.Origin)
        Assert.Equal("-1", row.ChunkTags["_dd.p.dm"])
        // ContainerDebug was absent: NULL, not a zero that reads as "instant".
        Assert.Equal(None, row.ContainerDebugError)
        Assert.Equal(None, row.ContainerDebugLatencyMs)
        Assert.Equal(None, row.ContainerDebugWasBuffered)

    Assert.Equal(0UL, rows[0].ParentID)
    Assert.Equal(222UL, rows[1].ParentID)
    // The upper half of the trace id comes out of meta, and stays in meta.
    Assert.Equal(0x67890abcdef01234UL, rows[0].TraceIDHigh)
    Assert.Equal("67890abcdef01234", rows[0].Meta["_dd.p.tid"])
    Assert.Equal(0UL, rows[1].TraceIDHigh)
    Assert.Equal("server", rows[0].Kind)
    Assert.Equal(UnixNanos 1_700_000_000_000_000_123L, rows[0].Start)
    Assert.Equal(1, rows[1].Error)

[<Fact>]
let ``a payload without spans gives no rows`` () =
    // Go's version of this test put nil entries in the lists; the C# lists
    // cannot hold one, so what is left is the empty levels.
    let empty = TracerPayload()
    empty.Chunks.Add(TraceChunk())
    let payload = v04Payload empty
    payload.TracerPayloads.Add(tracerOf [ Span(Service = "s", SpanID = 1UL) ])

    let rows = Trace.spanRows "t1" receivedAt payload
    Assert.Equal(1, rows.Length)
    Assert.Equal("s", rows[0].Service)
    Assert.Empty(Trace.spanRows "t1" receivedAt (AgentPayload()))

[<Fact>]
let ``container debug is stored when present, a zero latency included`` () =
    let tracer = tracerOf [ Span(SpanID = 1UL) ]

    tracer.ContainerDebug <-
        ContainerDebug(Error = "context deadline exceeded", LatencyMs = 0L, WasBuffered = true, BufferMs = 250L, BufferEvictionReason = "timeout")

    let row = (Trace.spanRows "t1" receivedAt (v04Payload tracer))[0]
    Assert.Equal(Some "context deadline exceeded", row.ContainerDebugError)
    Assert.Equal(Some 0L, row.ContainerDebugLatencyMs)
    Assert.Equal(Some 1uy, row.ContainerDebugWasBuffered)
    Assert.Equal(Some 250L, row.ContainerDebugBufferMs)
    Assert.Equal(Some "timeout", row.ContainerDebugBufferEvictionReason)

[<Fact>]
let ``links and events keep their wire order and their types`` () =
    let span = Span(SpanID = 1UL)
    let first = SpanLink(TraceID = 10UL, TraceIDHigh = 99UL, SpanID = 11UL, Tracestate = "dd=s:1", Flags = 0x80000001u)
    first.Attributes["rel"] <- "follows"
    span.SpanLinks.Add first
    span.SpanLinks.Add(SpanLink(TraceID = 20UL, SpanID = 21UL))

    let frames = AttributeArray()
    frames.Values.Add(AttributeArrayValue(Type = AttributeArrayValue.Types.AttributeArrayValueType.StringValue, StringValue = "a"))
    frames.Values.Add(AttributeArrayValue(Type = AttributeArrayValue.Types.AttributeArrayValueType.IntValue, IntValue = 7L))

    let exception' = SpanEvent(TimeUnixNano = 1_700_000_000_000_000_000UL, Name = "exception")
    exception'.Attributes["code"] <- AttributeAnyValue(Type = AttributeAnyValue.Types.AttributeAnyValueType.IntValue, IntValue = 42L)
    exception'.Attributes["message"] <- AttributeAnyValue(Type = AttributeAnyValue.Types.AttributeAnyValueType.StringValue, StringValue = "boom")
    exception'.Attributes["ratio"] <- AttributeAnyValue(Type = AttributeAnyValue.Types.AttributeAnyValueType.DoubleValue, DoubleValue = 0.5)
    exception'.Attributes["retry"] <- AttributeAnyValue(Type = AttributeAnyValue.Types.AttributeAnyValueType.BoolValue, BoolValue = true)
    exception'.Attributes["frames"] <- AttributeAnyValue(Type = AttributeAnyValue.Types.AttributeAnyValueType.ArrayValue, ArrayValue = frames)
    span.SpanEvents.Add exception'
    span.SpanEvents.Add(SpanEvent(TimeUnixNano = 1_700_000_000_000_000_001UL, Name = "log"))

    let row = (Trace.spanRows "t1" receivedAt (v04Payload (tracerOf [ span ])))[0]

    Assert.Equal<uint64[]>([| 10UL; 20UL |], row.LinkTraceID)
    Assert.Equal<uint64[]>([| 99UL; 0UL |], row.LinkTraceIDHigh)
    Assert.Equal<uint32[]>([| 0x80000001u; 0u |], row.LinkFlags)
    Assert.Equal("""{"rel":{"string":"follows"}}""", row.LinkAttributes[0])
    Assert.Equal("", row.LinkAttributes[1])

    Assert.Equal<string[]>([| "exception"; "log" |], row.EventName)
    Assert.Equal(UnixNanos 1_700_000_000_000_000_001L, row.EventTime[1])

    // The tag names the wire type: 42 as {"int":42} and 0.5 as {"double":0.5}
    // stay apart, which bare JSON numbers would not.
    let attributes = row.EventAttributes[0]
    Assert.Equal("42", tagged attributes "code" "int")
    Assert.Equal("0.5", tagged attributes "ratio" "double")
    Assert.Equal("true", tagged attributes "retry" "bool")
    Assert.Equal("\"boom\"", tagged attributes "message" "string")
    Assert.Equal("""[{"string":"a"},{"int":7}]""", tagged attributes "frames" "array")
    Assert.Equal("", row.EventAttributes[1])

[<Fact>]
let ``meta_struct bytes arrive untouched`` () =
    let raw = [| 0x81uy; 0xa3uy; byte 'k'; byte 'e'; byte 'y'; 0x00uy; 0xffuy |]
    let span = Span(SpanID = 1UL)
    span.MetaStruct["appsec"] <- ByteString.CopyFrom raw

    let row = (Trace.spanRows "t1" receivedAt (v04Payload (tracerOf [ span ])))[0]
    Assert.Equal<byte[]>(raw, row.MetaStruct["appsec"])

[<Theory>]
[<InlineData("67890abcdef01234", 0x67890abcdef01234UL)>]
[<InlineData("FFFFFFFFFFFFFFFF", 0xFFFFFFFFFFFFFFFFUL)>]
[<InlineData("00000000000000000000000000000001F", 31UL)>]
[<InlineData("", 0UL)>]
[<InlineData("not hex", 0UL)>]
[<InlineData("+1", 0UL)>]
[<InlineData("1ffffffffffffffff", 0UL)>]
let ``the upper half of a trace id is read as hex, and is 0 when it is not`` (text: string, expected: uint64) =
    Assert.Equal(expected, Trace.parseHex64 text)

// --- /api/v0.2/traces, idx (v1.0) ---

let private idxString (index: uint32) = Idx.AnyValue(StringValueRef = index)

[<Fact>]
let ``an idx payload is resolved against its string table`` () =
    // Index 0 is the empty string; a reference of 0 means "unset".
    let strings =
        [ ""; "checkout"; "flask.request"; "GET /pay"; "web"; "python"; "3.12.1"; "2.9.0"; "runtime-1"; "prod"; "pod-1"
          "1.4.0"; "http.method"; "GET"; "retries"; "payload"; "tags"; "a"; "dd=s:1"; "exception" ]

    let traceId =
        ByteString.CopyFrom [| 0x67uy; 0x89uy; 0x0auy; 0xbcuy; 0xdeuy; 0xf0uy; 0x12uy; 0x34uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0x6fuy |]

    let span =
        Idx.Span(
            ServiceRef = 1u,
            NameRef = 2u,
            ResourceRef = 3u,
            TypeRef = 4u,
            SpanID = 333UL,
            ParentID = 222UL,
            Start = 1_700_000_000_000_000_123UL,
            Duration = 5_000_000UL,
            Error = true,
            Kind = Idx.SpanKind.Server,
            EnvRef = 9u,
            VersionRef = 11u
        )

    let tags = Idx.ArrayValue()
    tags.Values.Add(idxString 17u)
    span.Attributes[12u] <- idxString 13u
    span.Attributes[14u] <- Idx.AnyValue(IntValue = 3L)
    span.Attributes[15u] <- Idx.AnyValue(BytesValue = ByteString.CopyFrom [| 0x00uy; 0x01uy |])
    span.Attributes[16u] <- Idx.AnyValue(ArrayValue = tags)
    span.Links.Add(Idx.SpanLink(TraceID = traceId, SpanID = 11UL, TracestateRef = 18u, Flags = 1u))
    span.Events.Add(Idx.SpanEvent(Time = 1_700_000_000_000_000_000UL, NameRef = 19u))

    let chunk = Idx.TraceChunk(Priority = 2, TraceID = traceId, SamplingMechanism = 4u)
    chunk.Spans.Add span

    let tracer =
        Idx.TracerPayload(
            LanguageNameRef = 5u,
            LanguageVersionRef = 6u,
            TracerVersionRef = 7u,
            RuntimeIDRef = 8u,
            EnvRef = 9u,
            HostnameRef = 10u,
            AppVersionRef = 11u
        )

    tracer.Strings.AddRange strings
    let list = Idx.KeyValueList()
    list.KeyValues.Add(Idx.KeyValue(Key = 12u, Value = idxString 13u))
    tracer.Attributes[16u] <- Idx.AnyValue(KeyValueList = list)
    tracer.Chunks.Add chunk

    let payload = AgentPayload(HostName = "agent-host")
    payload.IdxTracerPayloads.Add tracer

    let rows = Trace.spanRows "t1" receivedAt payload
    Assert.Equal(1, rows.Length)
    let row = rows[0]

    Assert.Equal("idx", row.WireFormat)
    Assert.Equal("agent-host", row.AgentHostname)
    Assert.Equal("checkout", row.Service)
    Assert.Equal("flask.request", row.Name)
    Assert.Equal("GET /pay", row.Resource)
    Assert.Equal("web", row.SpanType)
    Assert.Equal("", row.ContainerID)
    Assert.Equal("", row.Component)
    Assert.Equal(0x67890abcdef01234UL, row.TraceIDHigh)
    Assert.Equal(0x6fUL, row.TraceID)
    Assert.Equal(4u, row.SamplingMechanism)
    Assert.Equal("server", row.Kind)
    // idx ships a bool; the column keeps v0.4's width.
    Assert.Equal(1, row.Error)
    Assert.Equal("prod", row.SpanEnv)
    Assert.Equal("1.4.0", row.SpanVersion)

    // Scalars are projected into the v0.4 maps…
    Assert.Equal("GET", row.Meta["http.method"])
    Assert.Equal(3.0, row.Metrics["retries"])
    Assert.Equal<byte[]>([| 0x00uy; 0x01uy |], row.MetaStruct["payload"])
    // …and the typed form is kept whole: the only place the array survives.
    Assert.Equal("""[{"string":"a"}]""", tagged row.AttributesJSON "tags" "array")
    Assert.Equal("\"AAE=\"", tagged row.AttributesJSON "payload" "bytes")
    Assert.Equal("""{"tags":{"kvlist":[{"key":"http.method","value":{"string":"GET"}}]}}""", row.TracerAttributesJSON)
    // A non-scalar attribute is not flattened into tracer_tags.
    Assert.False(row.TracerTags.ContainsKey "tags")

    Assert.Equal<uint64[]>([| 0x6fUL |], row.LinkTraceID)
    Assert.Equal<uint64[]>([| 0x67890abcdef01234UL |], row.LinkTraceIDHigh)
    Assert.Equal<string[]>([| "dd=s:1" |], row.LinkTracestate)
    Assert.Equal<string[]>([| "exception" |], row.EventName)

[<Theory>]
[<InlineData(0u, "")>]
[<InlineData(1u, "a")>]
[<InlineData(99u, "")>]
let ``a string reference that is zero or out of range resolves to empty`` (index: uint32, expected: string) =
    Assert.Equal(expected, Trace.stringAt [| ""; "a" |] index)

[<Fact>]
let ``a trace id of any length splits into high and low`` () =
    Assert.Equal((0UL, 0UL), Trace.splitTraceId [||])
    Assert.Equal((1UL <<< 56, 2UL), Trace.splitTraceId [| 1uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 0uy; 2uy |])
    // A short id is left-padded, a long one keeps its last sixteen bytes.
    Assert.Equal((0UL, 0xffUL), Trace.splitTraceId [| 0xffuy |])
    Assert.Equal((0UL, 0UL), Trace.splitTraceId (Array.append [| 0xaauy; 0xbbuy |] (Array.zeroCreate 16)))

[<Fact>]
let ``a nested attribute is cut at the depth limit, and says so`` () =
    let strings = [| ""; "deep"; "leaf" |]
    let leaf = idxString 2u

    // One array per level, far past the limit.
    let mutable value = leaf

    for _ in 1 .. Trace.attributeMaxDepth * 4 do
        let array = Idx.ArrayValue()
        array.Values.Add value
        value <- Idx.AnyValue(ArrayValue = array)

    let attributes = Google.Protobuf.Collections.MapField<uint32, Idx.AnyValue>()
    attributes[1u] <- value
    let json = Trace.idxAttributesJson strings attributes
    Assert.Contains("\"_depth_exceeded\"", json)
    // The marker replaces the tail, so the leaf never appears.
    Assert.DoesNotContain("\"leaf\"", json)

    // The two or three levels real tracers send still come out whole.
    let array = Idx.ArrayValue()
    array.Values.Add leaf
    let list = Idx.KeyValueList()
    list.KeyValues.Add(Idx.KeyValue(Key = 1u, Value = Idx.AnyValue(ArrayValue = array)))
    let shallow = Google.Protobuf.Collections.MapField<uint32, Idx.AnyValue>()
    shallow[1u] <- Idx.AnyValue(KeyValueList = list)

    Assert.Equal(
        """{"deep":{"kvlist":[{"key":"deep","value":{"array":[{"string":"leaf"}]}}]}}""",
        Trace.idxAttributesJson strings shallow
    )

[<Theory>]
[<InlineData(1e21, "1e+21")>]
[<InlineData(1e-7, "1e-07")>]
[<InlineData(123456789.125, "1.23456789125e+08")>]
[<InlineData(100000.0, "100000")>]
[<InlineData(1000000.0, "1e+06")>]
[<InlineData(0.000123, "0.000123")>]
[<InlineData(-2.5e-5, "-2.5e-05")>]
[<InlineData(-0.0, "-0")>]
[<InlineData(0.5, "0.5")>]
[<InlineData(Double.NaN, "NaN")>]
[<InlineData(Double.NegativeInfinity, "-Inf")>]
let ``a float attribute is written in the layout the columns hold`` (value: float, expected: string) =
    Assert.Equal(expected, Text.ofFloat value)

// --- /api/v0.2/stats ---

/// The sketch Go's ddsketch.NewDefaultDDSketch(0.01) builds from 1e6, 2e6
/// and 3e6: the bins and the numbers below are the Go library's.
let private goSketch () : byte[] =
    let positive = Test.Store()
    positive.BinCounts[690] <- 1.0
    positive.BinCounts[725] <- 1.0
    positive.BinCounts[745] <- 1.0

    Test.DDSketch(Mapping = Test.IndexMapping(Gamma = (1.0 + 0.01) / (1.0 - 0.01)), PositiveValues = positive)
        .ToByteArray()

[<Fact>]
let ``a sketch summary has the numbers sketches-go computes`` () =
    let raw = goSketch ()
    let summary = TraceSketch.summary raw

    Assert.Equal(SketchState.Ok, summary.State)
    Assert.Equal<byte[]>(raw, summary.Raw)
    Assert.Equal(Some 3.0, summary.Count)
    // sketches-go gives 5987460.634366452, 994912.7844253893 and
    // 2988992.7847295585: the same to twelve digits, where the sketch
    // itself promises two.
    Assert.Equal(5987460.634366452, summary.Sum.Value, 6)
    Assert.Equal(994912.7844253893, summary.Min.Value, 6)
    Assert.Equal(2988992.7847295585, summary.Max.Value, 6)
    Assert.Equal<int32[]>([| 690; 725; 745 |], summary.BinKeys)
    Assert.Equal<float[]>([| 1.0; 1.0; 1.0 |], summary.BinCounts)

[<Fact>]
let ``no sketch, an empty sketch and bytes that are not one are three different states`` () =
    let absent = TraceSketch.summary [||]
    Assert.Equal(SketchState.Absent, absent.State)

    // A valid sketch that measured nothing; its bytes are kept.
    let raw = Test.DDSketch(Mapping = Test.IndexMapping(Gamma = 1.02), PositiveValues = Test.Store()).ToByteArray()
    let empty = TraceSketch.summary raw
    Assert.Equal(SketchState.Empty, empty.State)
    Assert.Equal(None, empty.Count)
    Assert.Equal<byte[]>(raw, empty.Raw)

    let garbage = TraceSketch.summary (Encoding.UTF8.GetBytes "not a sketch")
    Assert.True garbage.State.IsUndecodable
    Assert.Equal("not a sketch", Encoding.UTF8.GetString garbage.Raw)
    Assert.Equal(None, garbage.Count)

    // A sketch without its mapping cannot be read either.
    let unmapped = TraceSketch.summary (Test.DDSketch(ZeroCount = 4.0).ToByteArray())
    Assert.Equal(SketchState.Undecodable "not a DDSketch: the sketch has no index mapping", unmapped.State)

[<Fact>]
let ``stat rows carry payload, client and bucket, and keep the sketches`` () =
    let okBytes = goSketch ()

    let first =
        ClientGroupedStats(
            Service = "checkout",
            Name = "flask.request",
            Resource = "GET /pay",
            HTTPStatusCode = 200u,
            Type = "web",
            Hits = 120UL,
            Errors = 3UL,
            Duration = 900_000_000UL,
            TopLevelHits = 120UL,
            SpanKind = "server",
            IsTraceRoot = Trilean.True,
            HTTPMethod = "GET",
            HTTPEndpoint = "/pay",
            OkSummary = ByteString.CopyFrom okBytes,
            ErrorSummary = ByteString.CopyFromUtf8 "not a sketch"
        )

    first.PeerTags.Add "db.hostname:pg-1"
    first.AdditionalMetricTags.Add "region:eu"

    let bucket = ClientStatsBucket(Start = 1_700_000_000_000_000_000UL, Duration = 10_000_000_000UL, AgentTimeShift = -500L)
    bucket.Stats.Add first
    bucket.Stats.Add(ClientGroupedStats(Service = "checkout", Name = "postgres.query", IsTraceRoot = Trilean.NotSet))

    let client =
        ClientStatsPayload(
            Hostname = "pod-1",
            Env = "prod",
            Version = "1.4.0",
            Lang = "python",
            TracerVersion = "2.9.0",
            RuntimeID = "runtime-1",
            Sequence = 7UL,
            Service = "checkout",
            ContainerID = "abc123",
            GitCommitSha = "deadbeef",
            ImageTag = "v1.4.0",
            ProcessTags = "entrypoint:gunicorn",
            ProcessTagsHash = UInt64.MaxValue
        )

    // A pod behind two services sends kube_service twice.
    client.Tags.AddRange [ "kube_service:a"; "kube_service:b"; "bare" ]
    client.Stats.Add bucket

    let payload = StatsPayload(AgentHostname = "agent-1", AgentEnv = "prod", AgentVersion = "7.58.2", ClientComputed = true, SplitPayload = true)
    payload.Stats.Add client

    let rows = Trace.statRows NullLogger.Instance "t1" receivedAt payload
    Assert.Equal(2, rows.Length)
    let row = rows[0]

    Assert.Equal("7.58.2", row.AgentVersion)
    Assert.Equal(1uy, row.ClientComputed)
    Assert.Equal(1uy, row.SplitPayload)
    Assert.Equal("runtime-1", row.ClientRuntimeID)
    Assert.Equal(7UL, row.ClientSequence)
    Assert.Equal(UInt64.MaxValue, row.ClientProcessTagsHash)
    Assert.Equal<string[]>([| "a"; "b" |], row.ClientTags["kube_service"])
    Assert.Equal<string[]>([| "" |], row.ClientTags["bare"])
    Assert.Equal(UnixNanos 1_700_000_000_000_000_000L, row.BucketStart)
    Assert.Equal(-500L, row.AgentTimeShiftNs)
    // NOT_SET and FALSE are different answers.
    Assert.Equal("true", row.IsTraceRoot)
    Assert.Equal("not_set", rows[1].IsTraceRoot)

    Assert.Equal(SketchState.Ok, row.OkSummary.State)
    Assert.Equal<byte[]>(okBytes, row.OkSummary.Raw)
    Assert.Equal(Some 3.0, row.OkSummary.Count)
    Assert.Equal(3, row.OkSummary.BinKeys.Length)
    Assert.Equal(3, row.OkSummary.BinCounts.Length)
    Assert.True row.ErrorSummary.State.IsUndecodable
    Assert.Equal("not a sketch", Encoding.UTF8.GetString row.ErrorSummary.Raw)
    Assert.Equal(None, row.ErrorSummary.Count)
    Assert.Equal(SketchState.Absent, rows[1].OkSummary.State)
    Assert.Empty(rows[1].OkSummary.Raw)

// --- /api/v0.1/pipeline_stats ---

/// Just enough MessagePack to write what dd-trace-go sends.
module private Pack =
    let private bigEndian (value: uint64) : byte list = [ for shift in 56..-8..0 -> byte (value >>> shift) ]
    let map (count: int) : byte list = [ 0x80uy ||| byte count ]
    let array (count: int) : byte list = [ 0x90uy ||| byte count ]
    let nil: byte list = [ 0xc0uy ]
    let uint (value: uint64) : byte list = 0xcfuy :: bigEndian value
    let int (value: int64) : byte list = 0xd3uy :: bigEndian (uint64 value)
    let bin (bytes: byte[]) : byte list = 0xc4uy :: byte bytes.Length :: List.ofArray bytes

    let str (text: string) : byte list =
        let bytes = List.ofArray (Encoding.UTF8.GetBytes text)
        if bytes.Length < 32 then (0xa0uy ||| byte bytes.Length) :: bytes else 0xd9uy :: byte bytes.Length :: bytes

[<Fact>]
let ``a pipeline stats payload is decoded by its Go field names, unknown ones kept`` () =
    let pathway = goSketch ()

    let point =
        [ Pack.map 7
          Pack.str "EdgeTags"; Pack.array 2; Pack.str "type:kafka"; Pack.str "topic:orders"
          Pack.str "Hash"; Pack.uint UInt64.MaxValue
          Pack.str "ParentHash"; Pack.uint 42UL
          Pack.str "PathwayLatency"; Pack.bin pathway
          Pack.str "EdgeLatency"; Pack.nil
          Pack.str "PayloadSize"; Pack.bin (Encoding.UTF8.GetBytes "garbage")
          Pack.str "TimestampType"; Pack.str "current" ]

    let backlog =
        [ Pack.map 2
          Pack.str "Tags"; Pack.array 2; Pack.str "type:kafka_commit"; Pack.str "partition:3"
          Pack.str "Value"; Pack.int -17L ]

    let bucket =
        [ Pack.map 5
          Pack.str "Start"; Pack.uint 1_700_000_000_000_000_000UL
          Pack.str "Duration"; Pack.uint 10_000_000_000UL
          Pack.str "Stats"; Pack.array 1 ]
        @ point
        @ [ Pack.str "Backlogs"; Pack.array 1 ]
        @ backlog
        @ [ Pack.str "Transactions"; Pack.bin [| 1uy; 2uy; 3uy |] ]

    let body =
        [ Pack.map 9
          Pack.str "Env"; Pack.str "prod"
          Pack.str "Service"; Pack.str "orders"
          Pack.str "TracerVersion"; Pack.str "2.9.0"
          Pack.str "Lang"; Pack.str "go"
          Pack.str "Version"; Pack.str "1.4.0"
          Pack.str "ProcessTags"; Pack.array 2; Pack.str "entrypoint:worker"; Pack.str "cwd:/srv"
          Pack.str "ProductMask"; Pack.uint 3UL
          // A field this decoder has never heard of.
          Pack.str "SomethingNew"; Pack.str "hello"
          Pack.str "Stats"; Pack.array 1 ]
        @ bucket
        |> List.concat
        |> Array.ofList

    let payload =
        match Trace.decodeDsmPayload body with
        | Ok payload -> payload
        | Error problem -> failwith problem

    Assert.Equal("prod", payload.Env)
    Assert.Equal("orders", payload.Service)
    Assert.Equal("go", payload.Lang)
    Assert.Equal(3UL, payload.ProductMask)
    Assert.Equal<string[]>([| "entrypoint:worker"; "cwd:/srv" |], payload.ProcessTags)
    Assert.Equal(Some(MsgStr "hello"), payload.Unknown.TryFind "SomethingNew")

    let decodedBucket = Assert.Single payload.Buckets
    let decodedPoint = Assert.Single decodedBucket.Points
    let decodedBacklog = Assert.Single decodedBucket.Backlogs
    Assert.Equal(UInt64.MaxValue, decodedPoint.Hash)
    Assert.Equal<string[]>([| "type:kafka"; "topic:orders" |], decodedPoint.EdgeTags)
    Assert.Equal<byte[]>(pathway, decodedPoint.PathwayLatency)
    Assert.Empty decodedPoint.EdgeLatency
    Assert.Equal(-17L, decodedBacklog.Value)
    Assert.Equal<byte[]>([| 1uy; 2uy; 3uy |], decodedBucket.Transactions)

    // …and through to the rows.
    let headers: Trace.DsmHeaders =
        { Via = "trace-agent"
          AdditionalTags = ""
          ContainerTags = ""
          ContentEncoding = "" }

    let points, backlogs, blobs = Trace.dsmRows "t1" receivedAt payload headers
    let pointRow = Assert.Single points
    let backlogRow = Assert.Single backlogs
    let blobRow = Assert.Single blobs
    Assert.Equal("orders", pointRow.DSMCommon.Service)
    Assert.Equal("trace-agent", pointRow.Via)
    Assert.Equal(SketchState.Ok, pointRow.PathwayLatency.State)
    Assert.Equal(SketchState.Absent, pointRow.EdgeLatency.State)
    Assert.True pointRow.PayloadSize.State.IsUndecodable
    Assert.Equal("garbage", Encoding.UTF8.GetString pointRow.PayloadSize.Raw)
    Assert.Equal<string[]>([| "payload.SomethingNew" |], pointRow.UnknownKeys)
    Assert.Equal("""{"payload.SomethingNew":"hello"}""", pointRow.UnknownJSON)
    Assert.Equal(-17L, backlogRow.Value)
    Assert.Equal(2, backlogRow.Tags.Length)
    Assert.Equal<byte[]>([| 1uy; 2uy; 3uy |], blobRow.Transactions)

[<Fact>]
let ``a body that is not a pipeline stats payload is refused`` () =
    Assert.Equal(Error "empty body", Trace.decodeDsmPayload [||])
    Assert.True(Result.isError (Trace.decodeDsmPayload (Encoding.UTF8.GetBytes """{"Env":"prod"}""")))
    // A map that announces one entry and ends.
    Assert.Equal(Error Msgpack.truncated, Trace.decodeDsmPayload [| 0x81uy |])

// --- /api/v2/data_streams_messages ---

[<Fact>]
let ``message rows keep the text, the place in the batch and the key names`` () =
    let messages =
        match Json.tryParseElementBytes (Encoding.UTF8.GetBytes """[{"b":1,"a":{"nested":true}}, "just a string"]""") with
        | Ok messages -> messages
        | Error problem -> failwith problem

    let rows = Trace.dsmMessageRows "t1" receivedAt messages "browser" "5.0.0" "deflate"
    Assert.Equal(2, rows.Length)
    Assert.Equal(0u, rows[0].Position)
    Assert.Equal(1u, rows[1].Position)
    Assert.Equal("""{"b":1,"a":{"nested":true}}""", Encoding.UTF8.GetString rows[0].Message)
    Assert.Equal<string[]>([| "a"; "b" |], rows[0].Keys)
    // A message that is not an object has no keys.
    Assert.Empty rows[1].Keys
    Assert.Equal("browser", rows[0].DDEVPOrigin)
    Assert.Equal("5.0.0", rows[0].DDEVPOriginVersion)
    Assert.Equal("deflate", rows[0].ContentEncoding)

// --- the tables ---

[<Fact>]
let ``every table passes as many values as it has columns`` () =
    let arity (table: Table<'row>) (row: 'row) =
        Assert.Equal(table.Columns.Length, (table.Values row).Length)

    let spans = Trace.spanRows "t1" receivedAt (v04Payload (tracerOf [ Span() ]))
    arity Spans.table spans[0]

    let bucket = ClientStatsBucket()
    bucket.Stats.Add(ClientGroupedStats())
    let client = ClientStatsPayload()
    client.Stats.Add bucket
    let stats = StatsPayload()
    stats.Stats.Add client
    let statRows = Trace.statRows NullLogger.Instance "t1" receivedAt stats
    arity ApmStats.table statRows[0]

    let point: Trace.DsmPoint =
        { EdgeTags = [||]
          Hash = 0UL
          ParentHash = 0UL
          PathwayLatency = [||]
          EdgeLatency = [||]
          PayloadSize = [||]
          TimestampType = ""
          Unknown = Map.empty }

    let payload: Trace.DsmPayload =
        { Env = ""
          Service = ""
          TracerVersion = ""
          Lang = ""
          Version = ""
          ProcessTags = [||]
          ProductMask = 0UL
          Buckets =
            [ { Start = 0UL
                Duration = 0UL
                Points = [ point ]
                Backlogs = [ { Tags = [||]; Value = 0L } ]
                Transactions = [| 1uy |]
                TransactionCheckpointIDs = [||]
                Unknown = Map.empty } ]
          Unknown = Map.empty }

    let headers: Trace.DsmHeaders =
        { Via = ""
          AdditionalTags = ""
          ContainerTags = ""
          ContentEncoding = "" }

    let points, backlogs, blobs = Trace.dsmRows "t1" receivedAt payload headers
    arity DsmPipelineStats.table points[0]
    arity DsmBacklogs.table backlogs[0]
    arity DsmBucketTransactions.table blobs[0]

    let messages = Trace.dsmMessageRows "t1" receivedAt [ Encoding.UTF8.GetBytes "{}" ] "" "" ""
    arity DsmMessages.table messages[0]
