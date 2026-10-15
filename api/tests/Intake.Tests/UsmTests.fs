/// Tests of Routers/Usm.fs: the four aggregations inside a Connection.
module NinjaCat.Api.Intake.Tests.UsmTests

// KafkaAggregation.count is deprecated and still sent by older agents.
#nowarn "44"

open System
open System.IO
open Google.Protobuf
open Xunit
open Datadog.ProcessAgent
open NinjaCat.Api.Intake.Routers
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

let private key: ConnectionKey =
    { TenantID = "t"
      Timestamp = DateTime(2026, 10, 1, 12, 0, 0, DateTimeKind.Utc)
      Host = "host-a"
      PayloadID = Guid.Parse "11111111-1111-1111-1111-111111111111"
      PID = 42
      LaddrIP = "10.0.0.1"
      LaddrPort = 5000
      RaddrIP = "10.0.0.2"
      RaddrPort = 443 }

/// The sketch sketches-go builds from 1e6, 2e6 and 3e6 with 1% accuracy.
let private sketch () : ByteString =
    let positive = Test.Store()
    positive.BinCounts[690] <- 1.0
    positive.BinCounts[725] <- 1.0
    positive.BinCounts[745] <- 1.0
    Test.DDSketch(Mapping = Test.IndexMapping(Gamma = (1.0 + 0.01) / (1.0 - 0.01)), PositiveValues = positive).ToByteString()

let private rows (decoded: Result<'row list, string>) : 'row list =
    match decoded with
    | Ok rows -> rows
    | Error e -> failwith e

let httpAggregations () : HTTPAggregations =
    let orders = HTTPStats(Path = "/orders", Method = HTTPMethod.Post, FullPath = true)
    orders.StatsByStatusCode[201] <- HTTPStats.Types.Data(Count = 3u, Latencies = sketch ())
    orders.StatsByStatusCode[503] <- HTTPStats.Types.Data(Count = 1u, FirstLatencySample = 7e6)

    // An older agent: five slots by class, only the 4xx one filled.
    let legacy = HTTPStats(Path = "/old", Method = HTTPMethod.Get)

    for count in [ 0u; 0u; 0u; 2u; 0u ] do
        legacy.StatsByResponseStatus.Add(HTTPStats.Types.Data(Count = count, LatencySum = 9e6))

    let aggregations = HTTPAggregations()
    aggregations.EndpointAggregations.AddRange [ orders; legacy ]
    aggregations

let kafkaAggregations () : DataStreamsAggregations =
    let produce = KafkaAggregation(Header = KafkaRequestHeader(RequestType = 0u, RequestVersion = 9u), Topic = "orders")
    produce.StatsByErrorCode[0] <- KafkaStats(Count = 5u, Latencies = sketch ())
    produce.StatsByErrorCode[-1] <- KafkaStats(Count = 1u, FirstLatencySample = 2e6)
    let old = KafkaAggregation(Header = KafkaRequestHeader(RequestType = 1u, RequestVersion = 4u), Topic = "audit", Count = 8u)
    let aggregations = DataStreamsAggregations()
    aggregations.KafkaAggregations.AddRange [ produce; old ]
    aggregations

let databaseAggregations () : DatabaseAggregations =
    let select =
        PostgresStats(TableName = "orders", Operation = PostgresOperation.PostgresSelectOp, Count = 4u, Latencies = sketch ())

    let get = RedisStats(Command = RedisCommand.RedisGetCommand, KeyName = "session:1", Truncated = true)
    get.ErrorToStats[int RedisErrorType.RedisNoError] <- RedisStatsEntry(Count = 10u, Latencies = sketch ())
    get.ErrorToStats[int RedisErrorType.RedisErrOom] <- RedisStatsEntry(Count = 1u, FirstLatencySample = 3e6)

    let aggregations = DatabaseAggregations()
    aggregations.Aggregations.AddRange [ DatabaseStats(Postgres = select); DatabaseStats(Redis = get) ]
    aggregations

[<Fact>]
let ``an HTTP endpoint gives one row per status code, or per class from an older agent`` () =
    let stats = rows (Usm.http key (httpAggregations().ToByteString()))

    Assert.Equal<(string * string * int * string * uint32) list>(
        [ "/old", "Get", 0, "4xx", 2u; "/orders", "Post", 201, "2xx", 3u; "/orders", "Post", 503, "5xx", 1u ],
        stats |> List.map (fun s -> s.Path, s.Method, s.StatusCode, s.StatusClass, s.Count) |> List.sort
    )

    Assert.All(stats, (fun s -> Assert.Equal(("http", key), (s.Protocol, s.Connection))))

    let created = stats |> List.find (fun s -> s.StatusCode = 201)
    Assert.Equal(1uy, created.FullPath)
    Assert.Equal((SketchState.Ok, Some 3.0), (created.Latencies.State, created.Latencies.Count))

    // One sample: the latency is in the field, and there is no sketch.
    let failed = stats |> List.find (fun s -> s.StatusCode = 503)
    Assert.Equal((7e6, SketchState.Absent), (failed.FirstLatencySample, failed.Latencies.State))

    let old = stats |> List.find (fun s -> s.Path = "/old")
    Assert.Equal(9e6, old.LatencySum)

[<Fact>]
let ``HTTP/2 has the same shape under its own message`` () =
    let aggregations = HTTP2Aggregations()
    aggregations.EndpointAggregations.AddRange (httpAggregations ()).EndpointAggregations
    let stats = rows (Usm.http2 key (aggregations.ToByteString()))
    Assert.Equal(3, stats.Length)
    Assert.All(stats, (fun s -> Assert.Equal("http2", s.Protocol)))

[<Fact>]
let ``a Kafka topic gives one row per error code, and one for an agent that sent only a count`` () =
    let stats = rows (Usm.kafka key (kafkaAggregations().ToByteString()))

    Assert.Equal<(string * uint32 * uint32 * int * uint32) list>(
        [ "audit", 1u, 4u, 0, 8u; "orders", 0u, 9u, -1, 1u; "orders", 0u, 9u, 0, 5u ],
        stats |> List.map (fun s -> s.Topic, s.ApiKey, s.ApiVersion, s.ErrorCode, s.Count) |> List.sort
    )

    let ok = stats |> List.find (fun s -> s.Topic = "orders" && s.ErrorCode = 0)
    Assert.Equal(SketchState.Ok, ok.Latencies.State)

[<Fact>]
let ``a Postgres operation is one row, a Redis command one row per error type`` () =
    let stats = rows (Usm.database key (databaseAggregations().ToByteString()))

    Assert.Equal<(string * string * string * string * string * uint32) list>(
        [ "postgres", "PostgresSelectOp", "orders", "", "", 4u
          "redis", "RedisGetCommand", "", "session:1", "RedisErrOom", 1u
          "redis", "RedisGetCommand", "", "session:1", "RedisNoError", 10u ],
        stats |> List.map (fun s -> s.DBMS, s.Operation, s.TableName, s.KeyName, s.ErrorType, s.Count) |> List.sort
    )

    Assert.Equal<uint8 list>([ 0uy; 1uy; 1uy ], stats |> List.map _.KeyTruncated |> List.sort)

[<Fact>]
let ``no bytes give no rows, bytes that are not the message give an error`` () =
    Assert.Empty(rows (Usm.http key ByteString.Empty))
    Assert.Empty(rows (Usm.kafka key ByteString.Empty))
    Assert.Empty(rows (Usm.database key ByteString.Empty))

    let garbage = ByteString.CopyFrom [| 0xffuy; 0xffuy; 0xffuy |]
    Assert.True((Usm.http key garbage).IsError)
    Assert.True((Usm.http2 key garbage).IsError)
    Assert.True((Usm.kafka key garbage).IsError)
    Assert.True((Usm.database key garbage).IsError)

[<Fact>]
let ``a sketch that is not a sketch is kept as bytes and does not cost the row`` () =
    let stats = HTTPStats(Path = "/x", Method = HTTPMethod.Get)
    stats.StatsByStatusCode[200] <- HTTPStats.Types.Data(Count = 2u, Latencies = ByteString.CopyFrom [| 0xffuy; 0x01uy |])
    let aggregations = HTTPAggregations()
    aggregations.EndpointAggregations.Add stats

    let row = Assert.Single(rows (Usm.http key (aggregations.ToByteString())))
    Assert.True row.Latencies.State.IsUndecodable
    Assert.Equal<byte[]>([| 0xffuy; 0x01uy |], row.Latencies.Raw)

[<Fact>]
let ``the stat rows fit their ClickHouse tables`` () =
    let http = rows (Usm.http key (httpAggregations().ToByteString())) |> Array.ofList
    let kafka = rows (Usm.kafka key (kafkaAggregations().ToByteString())) |> Array.ofList
    let database = rows (Usm.database key (databaseAggregations().ToByteString())) |> Array.ofList

    ClickHouseRoundTripTests.roundTrip
        (Map [ "connection_http_stats", http.Length; "connection_kafka_stats", kafka.Length; "connection_database_stats", database.Length ])
        (fun sink ->
            Sink.write sink ConnectionHttpStats.table http
            Sink.write sink ConnectionKafkaStats.table kafka
            Sink.write sink ConnectionDatabaseStats.table database)

/// Bytes system-probe 7.84.0 really put inside connections between lab
/// containers: the agent posting to the intake, psql against Postgres,
/// redis-cli against Redis.
let private recorded (name: string) : ByteString =
    ByteString.CopyFrom(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "Fixtures", "real", name)))

[<Fact>]
let ``real HTTP aggregations give the endpoints the agent called, by status code`` () =
    let stats = rows (Usm.http key (recorded "system-probe-7.84.0-http-aggregations.bin"))

    Assert.Equal<string list>(
        [ "/api/v1/check_run"; "/api/v1/metadata"; "/api/v2/series"; "/intake/" ],
        stats |> List.map _.Path |> List.distinct |> List.sort
    )

    Assert.All(
        stats,
        (fun s ->
            Assert.Equal(("http", "Post", "2xx"), (s.Protocol, s.Method, s.StatusClass))
            Assert.True(s.StatusCode = 200 || s.StatusCode = 202)
            Assert.True(s.Count > 0u)
            // One sample is sent as a number, more as a sketch that decodes.
            if s.Count = 1u then
                Assert.Equal(SketchState.Absent, s.Latencies.State)
                Assert.True(s.FirstLatencySample > 0.0)
            else
                Assert.Equal((SketchState.Ok, Some(float s.Count)), (s.Latencies.State, s.Latencies.Count)))
    )

[<Fact>]
let ``real Postgres aggregations name the operation and the table`` () =
    let stats = rows (Usm.database key (recorded "system-probe-7.84.0-postgres-aggregations.bin"))

    Assert.Equal<(string * string * string) list>(
        [ "postgres", "PostgresInsertOp", "orders"; "postgres", "PostgresSelectOp", "orders"; "postgres", "PostgresUpdateOp", "orders" ],
        stats |> List.map (fun s -> s.DBMS, s.Operation, s.TableName) |> List.sort
    )

    Assert.All(stats, (fun s -> Assert.True(s.Count = 1u && s.FirstLatencySample > 0.0)))

[<Fact>]
let ``real Redis aggregations name the command; the key is sent only when the agent is told to`` () =
    let stats = rows (Usm.database key (recorded "system-probe-7.84.0-redis-aggregations.bin"))
    Assert.NotEmpty stats

    Assert.All(
        stats,
        (fun s ->
            Assert.Equal(("redis", "", "RedisNoError"), (s.DBMS, s.KeyName, s.ErrorType))
            Assert.Contains(s.Operation, [ "RedisGetCommand"; "RedisSetCommand" ]))
    )
