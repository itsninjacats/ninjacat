/// The APM family: everything that arrives on trace.agent.<site>.
///
///   spans                    POST /api/v0.2/traces
///   apm_stats                POST /api/v0.2/stats
///   dsm_pipeline_stats       POST /api/v0.1/pipeline_stats
///   dsm_backlogs             the same request, the bucket's backlog list
///   dsm_bucket_transactions  the same request, the bucket's two packed blobs
///   dsm_messages             POST /api/v2/data_streams_messages
namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One latency distribution: the bytes as the sender shipped them, plus what
/// they decoded to.
///
/// The raw bytes are the record: a DDSketch merges with other sketches and
/// answers any quantile afterwards. The decoded fields only save the common
/// query from unmarshalling a blob.
type SketchSummary =
    { Raw: byte[]
      /// "absent" (no bytes), "undecodable" (bytes that are not a DDSketch),
      /// "empty" (a valid sketch with no values) or "ok".
      State: string
      /// The four numbers exist only in the "ok" state: a zero would read as
      /// a measurement of zero.
      Count: float option
      Sum: float option
      Min: float option
      Max: float option
      /// The positive-value store, index and count in parallel.
      BinKeys: int32[]
      BinCounts: float[] }

module SketchSummary =
    let absent = "absent"
    let undecodable = "undecodable"
    let empty = "empty"
    let ok = "ok"

    /// The eight columns a summary occupies, in schema order.
    let values (s: SketchSummary) : obj list =
        // The state is an Enum8: an empty string would fail the whole batch.
        let state = if s.State = "" then absent else s.State
        [ s.Raw; state; Col.opt s.Count; Col.opt s.Sum; Col.opt s.Min; Col.opt s.Max; s.BinKeys; s.BinCounts ]

/// One span, with the agent, tracer and chunk it arrived under copied onto it.
type SpanRow =
    { TenantID: string
      ReceivedAt: DateTime
      /// "v04" or "idx". Several columns exist in only one of the two wire
      /// formats, so a zero means "no such field" for one and "left unset"
      /// for the other; nothing else tells those apart.
      WireFormat: string

      AgentHostname: string
      AgentEnv: string
      AgentVersion: string
      TargetTPS: float
      ErrorTPS: float
      RareSamplerEnabled: uint8
      AgentTags: Map<string, string>

      ContainerID: string
      Language: string
      LanguageVersion: string
      TracerVersion: string
      RuntimeID: string
      TracerEnv: string
      TracerHostname: string
      AppVersion: string
      TracerTags: Map<string, string>
      /// The idx typed form of the tracer's attributes; empty for v0.4.
      TracerAttributesJSON: string

      /// Present only when the agent had trouble resolving the container.
      ContainerDebugError: string option
      ContainerDebugLatencyMs: int64 option
      ContainerDebugWasBuffered: uint8 option
      ContainerDebugBufferMs: int64 option
      ContainerDebugBufferEvictionReason: string option

      Priority: int32
      Origin: string
      DroppedTrace: uint8
      SamplingMechanism: uint32
      ChunkTags: Map<string, string>
      ChunkAttributesJSON: string

      Service: string
      Name: string
      Resource: string
      SpanType: string

      /// The 128-bit trace id as two integers, whichever wire shape it came in.
      TraceID: uint64
      TraceIDHigh: uint64
      SpanID: uint64
      /// 0 means root.
      ParentID: uint64

      Start: UnixNanos
      DurationNs: int64
      Error: int32

      Kind: string
      SpanEnv: string
      SpanVersion: string
      Component: string

      Meta: Map<string, string>
      Metrics: Map<string, float>
      /// msgpack blobs with no published type per key: bytes, not text.
      MetaStruct: Map<string, byte[]>
      /// The idx typed attribute map in full; meta and metrics carry its
      /// scalar projection.
      AttributesJSON: string

      /// Links and events as parallel arrays: the index is the wire order.
      LinkTraceID: uint64[]
      LinkTraceIDHigh: uint64[]
      LinkSpanID: uint64[]
      LinkAttributes: string[]
      LinkTracestate: string[]
      LinkFlags: uint32[]

      EventTime: UnixNanos[]
      EventName: string[]
      EventAttributes: string[] }

module Spans =
    let table: Table<SpanRow> =
        { Table.create
              "storage_spans"
              "spans"
              [ "tenant_id"; "received_at"; "wire_format"
                "agent_hostname"; "agent_env"; "agent_version"
                "target_tps"; "error_tps"; "rare_sampler_enabled"; "agent_tags"
                "container_id"; "language"; "language_version"; "tracer_version"
                "runtime_id"; "tracer_env"; "tracer_hostname"; "app_version"
                "tracer_tags"; "tracer_attributes_json"
                "container_debug_error"; "container_debug_latency_ms"; "container_debug_was_buffered"
                "container_debug_buffer_ms"; "container_debug_buffer_eviction_reason"
                "priority"; "origin"; "dropped_trace"; "sampling_mechanism"
                "chunk_tags"; "chunk_attributes_json"
                "service"; "name"; "resource"; "span_type"
                "trace_id"; "trace_id_high"; "span_id"; "parent_id"
                "start"; "duration_ns"; "error"
                "kind"; "span_env"; "span_version"; "component"
                "meta"; "metrics"; "meta_struct"; "attributes_json"
                "link_trace_id"; "link_trace_id_high"; "link_span_id"
                "link_attributes"; "link_tracestate"; "link_flags"
                "event_time"; "event_name"; "event_attributes" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.WireFormat
                     r.AgentHostname; r.AgentEnv; r.AgentVersion
                     r.TargetTPS; r.ErrorTPS; r.RareSamplerEnabled; r.AgentTags
                     r.ContainerID; r.Language; r.LanguageVersion; r.TracerVersion
                     r.RuntimeID; r.TracerEnv; r.TracerHostname; r.AppVersion
                     r.TracerTags; r.TracerAttributesJSON
                     Col.opt r.ContainerDebugError; Col.opt r.ContainerDebugLatencyMs; Col.opt r.ContainerDebugWasBuffered
                     Col.opt r.ContainerDebugBufferMs; Col.opt r.ContainerDebugBufferEvictionReason
                     r.Priority; r.Origin; r.DroppedTrace; r.SamplingMechanism
                     r.ChunkTags; r.ChunkAttributesJSON
                     r.Service; r.Name; r.Resource; r.SpanType
                     r.TraceID; r.TraceIDHigh; r.SpanID; r.ParentID
                     r.Start; r.DurationNs; r.Error
                     r.Kind; r.SpanEnv; r.SpanVersion; r.Component
                     r.Meta; r.Metrics; r.MetaStruct; r.AttributesJSON
                     r.LinkTraceID; r.LinkTraceIDHigh; r.LinkSpanID
                     r.LinkAttributes; r.LinkTracestate; r.LinkFlags
                     r.EventTime; r.EventName; r.EventAttributes |])
          with
              MaxRows = 10_000
              FlushInterval = TimeSpan.FromSeconds 2.0
              BufferLimit = 200_000
              MaxInFlight = 4 }

/// One grouped stat with its payload, client and bucket copied onto it.
///
/// These counts are complete: the agent's concentrator sees every trace
/// before the sampler runs, which is why this table is not derived from spans.
type APMStatRow =
    { TenantID: string
      ReceivedAt: DateTime

      AgentHostname: string
      AgentEnv: string
      AgentVersion: string
      ClientComputed: uint8
      SplitPayload: uint8

      ClientHostname: string
      ClientEnv: string
      ClientVersion: string
      ClientLang: string
      ClientTracerVersion: string
      ClientRuntimeID: string
      ClientSequence: uint64
      ClientAgentAggregation: string
      ClientService: string
      ClientContainerID: string
      /// The one flat "key:value" tag list in the trace protocol.
      ClientTags: Map<string, string[]>
      ClientGitCommitSha: string
      ClientImageTag: string
      ClientProcessTags: string
      ClientProcessTagsHash: uint64

      BucketStart: UnixNanos
      BucketDurationNs: uint64
      AgentTimeShiftNs: int64

      Service: string
      Name: string
      Resource: string
      HTTPStatusCode: uint32
      SpanType: string
      DBType: string
      Hits: uint64
      Errors: uint64
      DurationNs: uint64
      Synthetics: uint8
      TopLevelHits: uint64
      SpanKind: string
      PeerTags: string[]
      /// "not_set", "true" or "false": the tracer not saying is an answer too.
      IsTraceRoot: string
      GRPCStatusCode: string
      HTTPMethod: string
      HTTPEndpoint: string
      ServiceSource: string
      SpanDerivedPrimaryTags: string[]
      AdditionalMetricTags: string[]

      OkSummary: SketchSummary
      ErrorSummary: SketchSummary }

module ApmStats =
    let table: Table<APMStatRow> =
        { Table.create
              "storage_apm_stats"
              "apm_stats"
              [ "tenant_id"; "received_at"
                "agent_hostname"; "agent_env"; "agent_version"; "client_computed"; "split_payload"
                "client_hostname"; "client_env"; "client_version"; "client_lang"; "client_tracer_version"
                "client_runtime_id"; "client_sequence"; "client_agent_aggregation"; "client_service"
                "client_container_id"; "client_tags"
                "client_git_commit_sha"; "client_image_tag"; "client_process_tags"; "client_process_tags_hash"
                "bucket_start"; "bucket_duration_ns"; "agent_time_shift_ns"
                "service"; "name"; "resource"; "http_status_code"; "span_type"; "db_type"
                "hits"; "errors"; "duration_ns"; "synthetics"; "top_level_hits"; "span_kind"
                "peer_tags"; "is_trace_root"
                "grpc_status_code"; "http_method"; "http_endpoint"; "service_source"
                "span_derived_primary_tags"; "additional_metric_tags"
                "ok_summary"; "ok_summary_state"; "ok_count"; "ok_sum"; "ok_min"; "ok_max"
                "ok_bin_keys"; "ok_bin_counts"
                "error_summary"; "error_summary_state"; "error_count"; "error_sum"; "error_min"; "error_max"
                "error_bin_keys"; "error_bin_counts" ]
              (fun r ->
                  // An Enum8 column: an empty string would fail the whole batch.
                  let isRoot = if r.IsTraceRoot = "" then "not_set" else r.IsTraceRoot

                  let own: obj list =
                      [ r.TenantID; r.ReceivedAt
                        r.AgentHostname; r.AgentEnv; r.AgentVersion; r.ClientComputed; r.SplitPayload
                        r.ClientHostname; r.ClientEnv; r.ClientVersion; r.ClientLang; r.ClientTracerVersion
                        r.ClientRuntimeID; r.ClientSequence; r.ClientAgentAggregation; r.ClientService
                        r.ClientContainerID; r.ClientTags
                        r.ClientGitCommitSha; r.ClientImageTag; r.ClientProcessTags; r.ClientProcessTagsHash
                        r.BucketStart; r.BucketDurationNs; r.AgentTimeShiftNs
                        r.Service; r.Name; r.Resource; r.HTTPStatusCode; r.SpanType; r.DBType
                        r.Hits; r.Errors; r.DurationNs; r.Synthetics; r.TopLevelHits; r.SpanKind
                        r.PeerTags; isRoot
                        r.GRPCStatusCode; r.HTTPMethod; r.HTTPEndpoint; r.ServiceSource
                        r.SpanDerivedPrimaryTags; r.AdditionalMetricTags ]

                  Array.ofList (own @ SketchSummary.values r.OkSummary @ SketchSummary.values r.ErrorSummary))
          with
              MaxRows = 2_000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

/// The payload and bucket every Data Streams row carries. The three tables
/// below are children of the same bucket and join on these.
type DSMCommon =
    { TenantID: string
      ReceivedAt: DateTime
      Env: string
      Service: string
      TracerVersion: string
      Lang: string
      Version: string
      ProcessTags: string[]
      ProductMask: uint64
      BucketStart: UnixNanos
      BucketDurationNs: uint64 }

module DSMCommon =
    let values (c: DSMCommon) : obj list =
        [ c.TenantID; c.ReceivedAt
          c.Env; c.Service; c.TracerVersion; c.Lang; c.Version; c.ProcessTags; c.ProductMask
          c.BucketStart; c.BucketDurationNs ]

    let columns: string list =
        [ "tenant_id"; "received_at"
          "env"; "service"; "tracer_version"; "lang"; "version"; "process_tags"; "product_mask"
          "bucket_start"; "bucket_duration_ns" ]

/// One checkpoint in a queue pathway. Hash and ParentHash rebuild the pathway
/// graph by self-join.
type DSMPipelineStatRow =
    { DSMCommon: DSMCommon
      EdgeTags: string[]
      Hash: uint64
      ParentHash: uint64
      TimestampType: string
      /// Latencies are in seconds; PayloadSize is a distribution of message
      /// sizes in bytes.
      PathwayLatency: SketchSummary
      EdgeLatency: SketchSummary
      PayloadSize: SketchSummary
      /// The headers the trace-agent adds as it proxies the request: the
      /// only link from a pathway to a workload, the body has none.
      Via: string
      AdditionalTags: string
      ContainerTags: string
      ContentEncoding: string
      /// Keys the decoder did not recognise, and their values as JSON.
      UnknownKeys: string[]
      UnknownJSON: string }

module DsmPipelineStats =
    let table: Table<DSMPipelineStatRow> =
        { Table.create
              "storage_dsm_pipeline_stats"
              "dsm_pipeline_stats"
              (DSMCommon.columns
               @ [ "edge_tags"; "hash"; "parent_hash"; "timestamp_type"
                   "pathway_latency"; "pathway_latency_state"
                   "pathway_count"; "pathway_sum"; "pathway_min"; "pathway_max"
                   "pathway_bin_keys"; "pathway_bin_counts"
                   "edge_latency"; "edge_latency_state"
                   "edge_count"; "edge_sum"; "edge_min"; "edge_max"
                   "edge_bin_keys"; "edge_bin_counts"
                   "payload_size"; "payload_size_state"
                   "payload_size_count"; "payload_size_sum"; "payload_size_min"; "payload_size_max"
                   "payload_size_bin_keys"; "payload_size_bin_counts"
                   "via"; "additional_tags"; "container_tags"; "content_encoding"
                   "unknown_keys"; "unknown_json" ])
              (fun r ->
                  let point: obj list = [ r.EdgeTags; r.Hash; r.ParentHash; r.TimestampType ]

                  let headers: obj list =
                      [ r.Via; r.AdditionalTags; r.ContainerTags; r.ContentEncoding; r.UnknownKeys; r.UnknownJSON ]

                  Array.ofList (
                      DSMCommon.values r.DSMCommon
                      @ point
                      @ SketchSummary.values r.PathwayLatency
                      @ SketchSummary.values r.EdgeLatency
                      @ SketchSummary.values r.PayloadSize
                      @ headers
                  ))
          with
              MaxRows = 2_000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

/// How far behind a consumer is. Tags stay an ordered array: the tracer
/// hashes on the list, order included.
type DSMBacklogRow =
    { DSMCommon: DSMCommon
      Tags: string[]
      Value: int64 }

module DsmBacklogs =
    let table: Table<DSMBacklogRow> =
        { Table.create
              "storage_dsm_backlogs"
              "dsm_backlogs"
              (DSMCommon.columns @ [ "tags"; "value" ])
              (fun r ->
                  let own: obj list = [ r.Tags; r.Value ]
                  Array.ofList (DSMCommon.values r.DSMCommon @ own))
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 20_000
              MaxInFlight = 2 }

/// A bucket's two packed binary blobs, verbatim. The layout is hand-rolled
/// and must match the Java tracer byte for byte, so it is not decoded here.
type DSMBucketTransactionRow =
    { DSMCommon: DSMCommon
      Transactions: byte[]
      TransactionCheckpointIDs: byte[] }

module DsmBucketTransactions =
    let table: Table<DSMBucketTransactionRow> =
        { Table.create
              "storage_dsm_bucket_transactions"
              "dsm_bucket_transactions"
              (DSMCommon.columns @ [ "transactions"; "transaction_checkpoint_ids" ])
              (fun r ->
                  let own: obj list = [ r.Transactions; r.TransactionCheckpointIDs ]
                  Array.ofList (DSMCommon.values r.DSMCommon @ own))
          with
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }

/// One element of the /api/v2/data_streams_messages array, as the JSON text
/// it arrived as: its producer is not public, so no shape is invented.
type DSMMessageRow =
    { TenantID: string
      ReceivedAt: DateTime
      /// The place in the batch, the only ordering these messages have.
      Position: uint32
      /// JSON as it arrived; Go's decoder let invalid UTF-8 through, so bytes.
      Message: byte[]
      /// The element's top-level key names, sorted.
      Keys: string[]
      DDEVPOrigin: string
      DDEVPOriginVersion: string
      ContentEncoding: string }

module DsmMessages =
    let table: Table<DSMMessageRow> =
        { Table.create
              "storage_dsm_messages"
              "dsm_messages"
              [ "tenant_id"; "received_at"; "position"; "message"; "keys"
                "dd_evp_origin"; "dd_evp_origin_version"; "content_encoding" ]
              (fun r ->
                  [| r.TenantID; r.ReceivedAt; r.Position; r.Message; r.Keys
                     r.DDEVPOrigin; r.DDEVPOriginVersion; r.ContentEncoding |])
          with
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 20_000
              MaxInFlight = 2 }
