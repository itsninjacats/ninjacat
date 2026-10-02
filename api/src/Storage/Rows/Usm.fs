/// Universal Service Monitoring: what system-probe saw on a connection, per
/// HTTP endpoint, Kafka topic and database operation. The agent ships these
/// inside each Connection as serialized messages; here every entry is a row.
/// Reasoning per column: 0019_usm.sql.
namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// The columns a stat row shares with the `connections` row it came from.
type ConnectionKey =
    { TenantID: string
      Timestamp: DateTime
      Host: string
      PayloadID: Guid
      PID: int32
      LaddrIP: string
      LaddrPort: int32
      RaddrIP: string
      RaddrPort: int32 }

module ConnectionKey =
    let columns =
        [ "tenant_id"; "timestamp"; "host"; "payload_id"; "pid"; "laddr_ip"; "laddr_port"; "raddr_ip"; "raddr_port" ]

    let values (k: ConnectionKey) : obj list =
        [ k.TenantID; k.Timestamp; k.Host; k.PayloadID; k.PID; k.LaddrIP; k.LaddrPort; k.RaddrIP; k.RaddrPort ]

module private UsmSketch =
    let columns =
        [ "latencies"; "latencies_state"; "sketch_count"; "sketch_sum"; "sketch_min"; "sketch_max"
          "sketch_bin_keys"; "sketch_bin_counts" ]

/// One endpoint of one connection, under one response status.
type ConnectionHttpStatRow =
    { Connection: ConnectionKey
      /// "http" or "http2".
      Protocol: string
      Method: string
      Path: string
      FullPath: uint8
      /// 0 when the agent reported only the class.
      StatusCode: int32
      /// "1xx" to "5xx".
      StatusClass: string
      Count: uint32
      /// The one latency when Count is 1: the agent sends no sketch then.
      FirstLatencySample: float
      LatencySum: float
      Latencies: SketchSummary }

module ConnectionHttpStats =
    let table: Table<ConnectionHttpStatRow> =
        { Table.create
              "storage_connection_http_stats"
              "connection_http_stats"
              (ConnectionKey.columns
               @ [ "protocol"; "method"; "path"; "full_path"; "status_code"; "status_class"
                   "count"; "first_latency_sample"; "latency_sum" ]
               @ UsmSketch.columns)
              (fun (r: ConnectionHttpStatRow) ->
                  let own: obj list =
                      [ r.Protocol; r.Method; r.Path; r.FullPath; r.StatusCode; r.StatusClass
                        r.Count; r.FirstLatencySample; r.LatencySum ]

                  Array.ofList (ConnectionKey.values r.Connection @ own @ SketchSummary.values r.Latencies))
          with
              MaxRows = 2_000
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 100_000
              MaxInFlight = 2 }

/// One Kafka request kind and topic of one connection, under one error code.
type ConnectionKafkaStatRow =
    { Connection: ConnectionKey
      /// Kafka's API key: 0 is Produce, 1 is Fetch.
      ApiKey: uint32
      ApiVersion: uint32
      Topic: string
      /// -1 to 119; 0 is success.
      ErrorCode: int32
      Count: uint32
      FirstLatencySample: float
      Latencies: SketchSummary }

module ConnectionKafkaStats =
    let table: Table<ConnectionKafkaStatRow> =
        { Table.create
              "storage_connection_kafka_stats"
              "connection_kafka_stats"
              (ConnectionKey.columns
               @ [ "api_key"; "api_version"; "topic"; "error_code"; "count"; "first_latency_sample" ]
               @ UsmSketch.columns)
              (fun (r: ConnectionKafkaStatRow) ->
                  let own: obj list = [ r.ApiKey; r.ApiVersion; r.Topic; r.ErrorCode; r.Count; r.FirstLatencySample ]
                  Array.ofList (ConnectionKey.values r.Connection @ own @ SketchSummary.values r.Latencies))
          with
              MaxRows = 2_000
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 100_000
              MaxInFlight = 2 }

/// One database operation of one connection: a Postgres operation on a
/// table, or a Redis command on a key under one error type.
type ConnectionDatabaseStatRow =
    { Connection: ConnectionKey
      /// "postgres" or "redis".
      DBMS: string
      /// The enum's name: PostgresSelectOp, RedisGetCommand.
      Operation: string
      /// Postgres only.
      TableName: string
      /// Redis only.
      KeyName: string
      KeyTruncated: uint8
      /// Redis only, the enum's name; RedisNoError for a success.
      ErrorType: string
      Count: uint32
      FirstLatencySample: float
      Latencies: SketchSummary }

module ConnectionDatabaseStats =
    let table: Table<ConnectionDatabaseStatRow> =
        { Table.create
              "storage_connection_database_stats"
              "connection_database_stats"
              (ConnectionKey.columns
               @ [ "dbms"; "operation"; "table_name"; "key_name"; "key_truncated"; "error_type"
                   "count"; "first_latency_sample" ]
               @ UsmSketch.columns)
              (fun (r: ConnectionDatabaseStatRow) ->
                  let own: obj list =
                      [ r.DBMS; r.Operation; r.TableName; r.KeyName; r.KeyTruncated; r.ErrorType
                        r.Count; r.FirstLatencySample ]

                  Array.ofList (ConnectionKey.values r.Connection @ own @ SketchSummary.values r.Latencies))
          with
              MaxRows = 2_000
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 100_000
              MaxInFlight = 2 }
