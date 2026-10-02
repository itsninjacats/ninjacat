/// Universal Service Monitoring: the four serialized messages inside a
/// Connection (HTTPAggregations, HTTP2Aggregations, DataStreamsAggregations,
/// DatabaseAggregations), one row per entry.
///
/// Bytes that do not parse give no rows and an error for the log. The bytes
/// themselves stay on the `connections` row either way.
module NinjaCat.Api.Intake.Routers.Usm

// KafkaAggregation.count is deprecated in the schema and still read: agents
// that predate per-error-code stats send nothing else.
#nowarn "44"

open Google.Protobuf
open Datadog.ProcessAgent
open NinjaCat.Api.Intake
open NinjaCat.Api.Storage.Rows

/// A latency sketch as its columns. One that does not decode is kept as
/// bytes and marked undecodable; it does not cost the row.
let private sketch (latencies: ByteString) : SketchSummary =
    TraceSketch.summary (latencies.ToByteArray())

let private statusClass (code: int) : string =
    if code >= 100 && code < 600 then $"{code / 100}xx" else ""

/// The older shape: five counters, in the order of HTTPResponseStatus
/// (Info, Success, Redirect, ClientErr, ServerErr).
let private classOfSlot (slot: int) : string =
    if slot >= 0 && slot <= 4 then $"{slot + 1}xx" else ""

let private httpStat
    (connection: ConnectionKey)
    (protocol: string)
    (stats: HTTPStats)
    (statusCode: int)
    (statusClass: string)
    (data: HTTPStats.Types.Data)
    : ConnectionHttpStatRow =
    { Connection = connection
      Protocol = protocol
      Method = ProtoEnum.name stats.Method
      Path = stats.Path
      FullPath = (if stats.FullPath then 1uy else 0uy)
      StatusCode = statusCode
      StatusClass = statusClass
      Count = data.Count
      FirstLatencySample = data.FirstLatencySample
      LatencySum = data.LatencySum
      Latencies = sketch data.Latencies }

let private httpRows (connection: ConnectionKey) (protocol: string) (endpoints: HTTPStats seq) : ConnectionHttpStatRow list =
    [ for stats in endpoints do
          for entry in stats.StatsByStatusCode do
              httpStat connection protocol stats entry.Key (statusClass entry.Key) entry.Value

          // Every agent fills all five slots; the empty ones say nothing.
          for slot in 0 .. stats.StatsByResponseStatus.Count - 1 do
              let data = stats.StatsByResponseStatus[slot]

              if data.Count > 0u then
                  httpStat connection protocol stats 0 (classOfSlot slot) data ]

let http (connection: ConnectionKey) (bytes: ByteString) : Result<ConnectionHttpStatRow list, string> =
    try
        Ok(httpRows connection "http" (HTTPAggregations.Parser.ParseFrom bytes).EndpointAggregations)
    with :? InvalidProtocolBufferException as e ->
        Error $"HTTPAggregations: {e.Message}"

let http2 (connection: ConnectionKey) (bytes: ByteString) : Result<ConnectionHttpStatRow list, string> =
    try
        Ok(httpRows connection "http2" (HTTP2Aggregations.Parser.ParseFrom bytes).EndpointAggregations)
    with :? InvalidProtocolBufferException as e ->
        Error $"HTTP2Aggregations: {e.Message}"

let kafka (connection: ConnectionKey) (bytes: ByteString) : Result<ConnectionKafkaStatRow list, string> =
    try
        let aggregations = (DataStreamsAggregations.Parser.ParseFrom bytes).KafkaAggregations

        Ok
            [ for aggregation in aggregations do
                  let header = if isNull aggregation.Header then KafkaRequestHeader() else aggregation.Header

                  let row (errorCode: int) (count: uint32) (first: float) (latencies: SketchSummary) : ConnectionKafkaStatRow =
                      { Connection = connection
                        ApiKey = header.RequestType
                        ApiVersion = header.RequestVersion
                        Topic = aggregation.Topic
                        ErrorCode = errorCode
                        Count = count
                        FirstLatencySample = first
                        Latencies = latencies }

                  for entry in aggregation.StatsByErrorCode do
                      row entry.Key entry.Value.Count entry.Value.FirstLatencySample (sketch entry.Value.Latencies)

                  // Agents older than error codes sent one count per topic.
                  if aggregation.StatsByErrorCode.Count = 0 && aggregation.Count > 0u then
                      row 0 aggregation.Count 0.0 (sketch ByteString.Empty) ]
    with :? InvalidProtocolBufferException as e ->
        Error $"DataStreamsAggregations: {e.Message}"

let database (connection: ConnectionKey) (bytes: ByteString) : Result<ConnectionDatabaseStatRow list, string> =
    try
        let aggregations = (DatabaseAggregations.Parser.ParseFrom bytes).Aggregations

        Ok
            [ for stats in aggregations do
                  match stats.DbStatsCase with
                  | DatabaseStats.DbStatsOneofCase.Postgres ->
                      let postgres = stats.Postgres

                      { Connection = connection
                        DBMS = "postgres"
                        Operation = ProtoEnum.name postgres.Operation
                        TableName = postgres.TableName
                        KeyName = ""
                        KeyTruncated = 0uy
                        ErrorType = ""
                        Count = postgres.Count
                        FirstLatencySample = postgres.FirstLatencySample
                        Latencies = sketch postgres.Latencies }
                  | DatabaseStats.DbStatsOneofCase.Redis ->
                      let redis = stats.Redis

                      for entry in redis.ErrorToStats do
                          { Connection = connection
                            DBMS = "redis"
                            Operation = ProtoEnum.name redis.Command
                            TableName = ""
                            KeyName = redis.KeyName
                            KeyTruncated = (if redis.Truncated then 1uy else 0uy)
                            ErrorType = ProtoEnum.name (enum<RedisErrorType> entry.Key)
                            Count = entry.Value.Count
                            FirstLatencySample = entry.Value.FirstLatencySample
                            Latencies = sketch entry.Value.Latencies }
                  | _ -> () ]
    with :? InvalidProtocolBufferException as e ->
        Error $"DatabaseAggregations: {e.Message}"
