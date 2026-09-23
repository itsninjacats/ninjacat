-- APM: the tables behind trace.agent.<site>.
--
-- Four routes land on that host and until now not one of them stored a byte
-- (intake/router_trace.go). They carry three different kinds of data and the
-- differences matter more than the shared hostname:
--
--   /api/v0.2/traces               spans, a SAMPLE      -> spans
--   /api/v0.2/stats                counts, COMPLETE     -> apm_stats
--   /api/v0.1/pipeline_stats       Data Streams stats   -> dsm_pipeline_stats
--                                                         dsm_backlogs
--                                                         dsm_bucket_transactions
--   /api/v2/data_streams_messages  opaque JSON          -> dsm_messages
--
-- The sample/complete split is why spans and apm_stats are two tables rather
-- than one with a rollup on top: the agent runs its samplers before it ships
-- spans, but feeds EVERY trace to its concentrator before that, so a hit count
-- in apm_stats is right even when 99% of the matching spans were dropped.
-- Recomputing it from the spans table would be wrong by construction.
--
-- No CREATE DATABASE and no database qualifier, same as 0001 and 0002.


-- ---------------------------------------------------------------------------
-- spans: one row per span, both wire shapes.
--
-- Two formats arrive in one envelope (pb.AgentPayload). v0.4 carries strings
-- inline; v1.0 ("idx", pbgo/trace/idx) carries one string table per tracer
-- payload and every string field is an index into it. wire_format records
-- which one produced the row, because the two disagree on real things — a
-- v0.4 span has no kind, no per-span env/version/component and no typed
-- attributes, and an idx span has no separate Meta/Metrics/MetaStruct maps.
-- Reading a zero out of those columns means "this format has no such field"
-- for one row and "the tracer did not set it" for the other, and only
-- wire_format tells them apart.
--
-- DENORMALIZED ON PURPOSE. Agent, tracer and chunk fields are copied onto
-- every span. The envelope is three levels deep and a normalized layout would
-- need two joins to answer "which agent version shipped this span", which is
-- exactly the question asked when something looks wrong. The repeated values
-- are LowCardinality, so the copies cost dictionary ids, not strings.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS spans
(
    tenant_id          LowCardinality(String),

    -- Arrival time. The span's own clock is `start`; this is ours, and the
    -- two disagreeing is itself a finding (a container with a skewed clock
    -- puts spans in tomorrow's partition).
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- 'v04' (pb.TracerPayload) or 'idx' (idx.TracerPayload). See above.
    wire_format        LowCardinality(String),

    -- Agent envelope: which trace-agent forwarded this, and the sampler
    -- settings it was running with when it chose to keep the trace. Those
    -- settings are the difference between "the service stopped serving" and
    -- "the sampler tightened".
    agent_hostname     LowCardinality(String),
    agent_env          LowCardinality(String),
    agent_version      LowCardinality(String),
    target_tps         Float64 CODEC(Gorilla, ZSTD(1)),
    error_tps          Float64 CODEC(Gorilla, ZSTD(1)),
    rare_sampler_enabled UInt8,

    -- AgentPayload.Tags is a protobuf map<string,string>, NOT Datadog's flat
    -- "key:value" tag list, so it stays a plain Map: the multiset shape of
    -- 0001 exists because a flat list can repeat a key, and a protobuf map
    -- cannot. Same for tracer_tags and chunk_tags below.
    agent_tags         Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- Tracer identity: one process, one runtime. container_id and runtime_id
    -- are unbounded (a UUID per process), so no dictionary.
    container_id       String,
    language           LowCardinality(String),
    language_version   LowCardinality(String),
    tracer_version     LowCardinality(String),
    runtime_id         String,
    tracer_env         LowCardinality(String),
    tracer_hostname    LowCardinality(String),
    app_version        LowCardinality(String),
    tracer_tags        Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- idx only: the same payload-level attributes as tracer_tags, but typed
    -- (AnyValue can be bytes, an array or a nested key-value list). tracer_tags
    -- holds the scalar rendering for both formats; this holds the full form so
    -- a non-scalar value is not flattened away. Empty for v0.4.
    tracer_attributes_json String CODEC(ZSTD(3)),

    -- ContainerDebug, and every column NULLABLE because the whole sub-message
    -- is a pointer the agent sets ONLY when resolving the container tags gave
    -- it trouble. A zero latency would read as "the lookup was instant"; NULL
    -- reads as "there was nothing to report", which is the common case.
    container_debug_error        Nullable(String),
    container_debug_latency_ms   Nullable(Int64),
    container_debug_was_buffered Nullable(UInt8),
    container_debug_buffer_ms    Nullable(Int64),
    container_debug_buffer_eviction_reason Nullable(String),

    -- Chunk: the sampling decision that let these spans through.
    -- -1 user_drop / 0 auto_drop / 1 auto_keep / 2 user_keep, and other
    -- values are possible — kept as the number the agent sent, not as a name,
    -- so a value we do not know yet still arrives.
    priority           Int32 CODEC(T64, ZSTD(1)),
    origin             LowCardinality(String),
    dropped_trace      UInt8,
    -- idx only; in v0.4 the same information travels in meta['_dd.p.dm'].
    sampling_mechanism UInt32 CODEC(T64, ZSTD(1)),
    chunk_tags         Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    chunk_attributes_json String CODEC(ZSTD(3)),

    -- The span itself.
    service            LowCardinality(String),
    name               LowCardinality(String),
    -- Resource is a URL, a SQL statement or a route: unbounded and often long.
    resource           String CODEC(ZSTD(3)),
    -- `type` is a ClickHouse function name; span_type keeps the column
    -- unambiguous in a SELECT. Source field is pb.Span.Type / idx TypeRef.
    span_type          LowCardinality(String),

    -- THE 128-BIT TRACE ID, SPLIT. v0.4 puts the low 64 bits on the span as a
    -- uint64 and the high 64, when they exist, in meta['_dd.p.tid'] as hex;
    -- idx moved the whole id to the chunk as 16 raw bytes. Both are normalized
    -- here to the same pair, so a query does not have to know which format
    -- wrote the row. Never through a float: 2^53 is well below a trace id.
    trace_id           UInt64 CODEC(T64, ZSTD(1)),
    trace_id_high      UInt64 CODEC(T64, ZSTD(1)),
    -- The form people paste from a Datadog URL or a log line: 32 lowercase
    -- hex digits, zero padded. MATERIALIZED rather than stored, because it is
    -- a pure function of the two columns above and recomputing it costs less
    -- than writing it. leftPad is not decoration: ClickHouse's hex() on an
    -- integer drops leading zero BYTES, so hex(111) is '6F', and concatenating
    -- the two halves unpadded would silently produce an id of the wrong width
    -- that matches nothing.
    trace_id_hex       String MATERIALIZED lower(concat(
                           leftPad(hex(trace_id_high), 16, '0'),
                           leftPad(hex(trace_id), 16, '0'))),

    span_id            UInt64 CODEC(T64, ZSTD(1)),
    -- 0 means ROOT, not "absent". The wire has no separate has-parent bit, so
    -- the zero is kept as it came and the meaning is documented here rather
    -- than encoded as NULL, which would claim a distinction the tracer never
    -- made.
    parent_id          UInt64 CODEC(T64, ZSTD(1)),

    -- Nanosecond precision, because that is what the wire carries and a span
    -- can be shorter than a millisecond. DateTime64(9) spans 1900-2262, which
    -- covers every value a tracer can produce.
    start              DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    -- Int64, not UInt64: v0.4 declares it signed, and a negative duration from
    -- a broken clock is a bug worth being able to SELECT rather than one the
    -- schema silently wraps.
    duration_ns        Int64 CODEC(T64, ZSTD(1)),
    -- v0.4 ships an int32 flag, idx a bool. Int32 keeps whatever v0.4 sent.
    error              Int32 CODEC(T64, ZSTD(1)),

    -- idx carries a real OTel SpanKind enum; v0.4 only ever had the string in
    -- meta['span.kind']. Both end up here, as the lowercase OTel name.
    kind               LowCardinality(String),
    -- idx only: per-span overrides of the payload-level values.
    span_env           LowCardinality(String),
    span_version       LowCardinality(String),
    component          LowCardinality(String),

    -- The bulk of a span. meta and metrics are the v0.4 shape and the place
    -- idx scalars are projected to, so one query works across both formats.
    meta               Map(String, String) CODEC(ZSTD(3)),
    metrics            Map(String, Float64) CODEC(ZSTD(3)),
    -- MetaStruct values are msgpack blobs (AppSec events, process tags) with
    -- no published per-key type. Kept as raw bytes in a String, which in
    -- ClickHouse is a byte string, not text — nothing here is validated UTF-8.
    meta_struct        Map(String, String) CODEC(ZSTD(3)),

    -- idx only, and the reason meta/metrics are not the whole story: an idx
    -- attribute can be bytes, an array or a nested key-value list, none of
    -- which survives projection into a string or a float. This column holds
    -- every attribute in its declared type, so the projection above is a
    -- convenience and this is the record.
    attributes_json    String CODEC(ZSTD(3)),

    -- Span links and span events as PARALLEL ARRAYS rather than Nested: the
    -- array index is the wire order, which protobuf `repeated` preserves and
    -- which a reader needs (a link list is ordered). Every link contributes
    -- one element to each of the six link_* arrays.
    link_trace_id      Array(UInt64) CODEC(T64, ZSTD(1)),
    link_trace_id_high Array(UInt64) CODEC(T64, ZSTD(1)),
    link_span_id       Array(UInt64) CODEC(T64, ZSTD(1)),
    -- One JSON object per link, in the same tagged form as attributes_json,
    -- so a v0.4 string attribute and an idx typed one read the same way.
    link_attributes    Array(String) CODEC(ZSTD(3)),
    link_tracestate    Array(String) CODEC(ZSTD(3)),
    link_flags         Array(UInt32) CODEC(T64, ZSTD(1)),

    event_time         Array(DateTime64(9, 'UTC')) CODEC(ZSTD(1)),
    event_name         Array(String) CODEC(ZSTD(3)),
    event_attributes   Array(String) CODEC(ZSTD(3)),

    -- "Give me every span of this trace" is the single most common trace
    -- query and the sort key below does not serve it: trace ids are random,
    -- so they spread across every part. A bloom filter on the granule makes
    -- it a skip scan instead of a full one. 0.001 rather than the 0.01 used
    -- for tag maps because this index carries one high-cardinality value per
    -- row, where a false positive costs a whole granule read.
    INDEX idx_trace_id trace_id TYPE bloom_filter(0.001) GRANULARITY 1,
    INDEX idx_span_id  span_id  TYPE bloom_filter(0.001) GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY toDate(start)
-- WHY THIS KEY. Two access patterns have to be served and they pull in
-- opposite directions: "one service over a time range" (the latency and error
-- panels, and every dashboard) and "every span of one trace" (the flame
-- graph). A key led by trace_id would serve the second and destroy the first —
-- random ids mean no time locality, so every service query would read the
-- whole partition. So the key is the service/time one, which also compresses
-- (service, name and resource repeat for millions of consecutive rows), and
-- the trace lookup is handled by the bloom filter above, which is what skip
-- indexes are for. trace_id still ends the key so that the spans of one trace
-- sit together inside a granule once service and time have narrowed it.
ORDER BY (tenant_id, service, name, start, trace_id)
-- 14 days, the logs class. Spans are the highest-volume table here and they
-- are already a sample; two weeks covers "it started last sprint" without
-- letting one chatty service fill a disk. apm_stats below keeps the complete
-- counts for longer, which is the right way round.
TTL toDateTime(start) + INTERVAL 14 DAY;


-- ---------------------------------------------------------------------------
-- apm_stats: one row per pb.ClientGroupedStats.
--
-- These are the numbers Datadog's APM pages are actually built from. They are
-- COMPLETE — the agent's concentrator sees every trace before the sampler
-- does — which is why this table gets a longer TTL than spans despite
-- describing the same traffic.
--
-- The payload is four levels deep (StatsPayload -> ClientStatsPayload ->
-- ClientStatsBucket -> ClientGroupedStats) and the three outer levels are
-- denormalized onto the leaf, for the same reason as spans.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS apm_stats
(
    tenant_id        LowCardinality(String),
    received_at      DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- StatsPayload: the agent that aggregated or forwarded this.
    agent_hostname   LowCardinality(String),
    agent_env        LowCardinality(String),
    agent_version    LowCardinality(String),
    -- The tracer computed the stats itself and the agent only forwarded them.
    client_computed  UInt8,
    -- This payload is one slice of a larger one the agent had to split, which
    -- tells a reader that re-aggregation is needed before the numbers mean
    -- anything on their own.
    split_payload    UInt8,

    -- ClientStatsPayload: the tracer, or the agent's aggregation key when the
    -- agent did the aggregating.
    client_hostname       LowCardinality(String),
    client_env            LowCardinality(String),
    client_version        LowCardinality(String),
    client_lang           LowCardinality(String),
    client_tracer_version LowCardinality(String),
    client_runtime_id     String,
    client_sequence       UInt64 CODEC(T64, ZSTD(1)),
    client_agent_aggregation LowCardinality(String),
    client_service        LowCardinality(String),
    client_container_id   String,

    -- THE ONE GENUINE DATADOG TAG LIST IN THE WHOLE TRACE PROTOCOL: a flat
    -- []string of "key:value", from orchestrator enrichment. So it gets the
    -- multiset shape of 0001 and the bloom filter pair below, unlike the
    -- protobuf maps on spans.
    client_tags      Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    client_git_commit_sha    String,
    client_image_tag         String,
    client_process_tags      String CODEC(ZSTD(3)),
    client_process_tags_hash UInt64 CODEC(T64, ZSTD(1)),

    -- ClientStatsBucket. agent_time_shift_ns is signed and usually 0: it is
    -- how far the agent had to move the bucket when its own aggregation
    -- window did not line up with the one the tracer reported.
    bucket_start        DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    bucket_duration_ns  UInt64 CODEC(T64, ZSTD(1)),
    agent_time_shift_ns Int64 CODEC(T64, ZSTD(1)),

    -- ClientGroupedStats: the aggregation key, then the counts. Field 14 does
    -- not exist in this proto version, which is why 23 field ids give 21
    -- columns.
    service          LowCardinality(String),
    name             LowCardinality(String),
    resource         String CODEC(ZSTD(3)),
    http_status_code UInt32 CODEC(T64, ZSTD(1)),
    span_type        LowCardinality(String),
    db_type          LowCardinality(String),
    hits             UInt64 CODEC(T64, ZSTD(1)),
    errors           UInt64 CODEC(T64, ZSTD(1)),
    duration_ns      UInt64 CODEC(T64, ZSTD(1)),
    synthetics       UInt8,
    top_level_hits   UInt64 CODEC(T64, ZSTD(1)),
    span_kind        LowCardinality(String),
    peer_tags        Array(String) CODEC(ZSTD(3)),
    -- A genuine THREE-state field (pb.Trilean), not a bool: "the tracer did
    -- not say" is different from "this is not a root span", and collapsing
    -- them would silently turn every old tracer's spans into non-roots. Enum8
    -- travels by NAME through the driver, like check_runs.status in 0001.
    is_trace_root    Enum8('not_set' = 0, 'true' = 1, 'false' = 2),
    grpc_status_code LowCardinality(String),
    http_method      LowCardinality(String),
    http_endpoint    String CODEC(ZSTD(3)),
    service_source   LowCardinality(String),
    -- Deprecated upstream in favour of additional_metric_tags, kept because
    -- tracers in the field still send it.
    span_derived_primary_tags Array(String) CODEC(ZSTD(3)),
    additional_metric_tags    Array(String) CODEC(ZSTD(3)),

    -- THE LATENCY SKETCHES, RAW AND DECODED, AND THE RAW IS THE RECORD.
    --
    -- Each summary is a DDSketch the concentrator built with sketches-go and
    -- shipped as protobuf bytes. A sketch merges with other sketches and
    -- answers ANY quantile afterwards; a stored p50 and p99 answer two and
    -- cannot be combined across services, hosts or time. So the bytes are
    -- kept verbatim and the decoded numbers beside them are a convenience for
    -- the common query, never the only copy.
    --
    -- The state column keeps the three outcomes the decoder already tells
    -- apart, because a NULL for all of them would merge two very different
    -- facts: 'absent' (the group had no ok spans), 'undecodable' (bytes
    -- arrived and are not a DDSketch — a bug or a version skew worth finding),
    -- 'empty' (a valid sketch with no values), 'ok'.
    ok_summary       String CODEC(ZSTD(3)),
    ok_summary_state Enum8('absent' = 0, 'undecodable' = 1, 'empty' = 2, 'ok' = 3),
    -- Nullable because they exist only when the state is 'ok'. Zero would
    -- read as a real measurement of zero nanoseconds.
    ok_count         Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    ok_sum           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    ok_min           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    ok_max           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    -- The positive-value store as parallel arrays, index and count, the same
    -- shape the sketches table in 0001 uses for metric distributions.
    ok_bin_keys      Array(Int32) CODEC(ZSTD(1)),
    ok_bin_counts    Array(Float64) CODEC(ZSTD(1)),

    error_summary       String CODEC(ZSTD(3)),
    error_summary_state Enum8('absent' = 0, 'undecodable' = 1, 'empty' = 2, 'ok' = 3),
    error_count         Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    error_sum           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    error_min           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    error_max           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    error_bin_keys      Array(Int32) CODEC(ZSTD(1)),
    error_bin_counts    Array(Float64) CODEC(ZSTD(1)),

    -- The pair from 0001: mapKeys for "which tag keys exist", arrayFlatten
    -- over mapValues because mapValues on an array-valued map yields
    -- Array(Array(...)), which bloom_filter rejects.
    INDEX idx_client_tag_keys   mapKeys(client_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_client_tag_values arrayFlatten(mapValues(client_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(bucket_start)
-- Every APM question starts with a service and a time range, then narrows to
-- an operation and a resource, so the key follows that order exactly.
ORDER BY (tenant_id, service, bucket_start, name, resource)
-- 30 days rather than the 14 spans get: these rows are small, complete and
-- the only place the true traffic volume survives once the sampler has
-- thrown the spans away.
TTL toDateTime(bucket_start) + INTERVAL 30 DAY;


-- ---------------------------------------------------------------------------
-- dsm_pipeline_stats: one row per StatsPoint of Data Streams Monitoring.
--
-- POST /api/v0.1/pipeline_stats. The trace-agent is a pure reverse proxy for
-- this route (pkg/trace/api/pipeline_stats.go): it rewrites the path, adds
-- Via and the two X-Datadog-*-Tags headers, and forwards the tracer's body
-- untouched. So the schema is dd-trace-go's, not the agent's — msgpack of
-- internal/datastreams.StatsPayload, field names as map keys.
--
-- A pathway is a chain of queue hops. `hash` identifies the point in the
-- chain and `parent_hash` the one before it, so the graph is rebuilt by
-- joining the table to itself; edge_tags describe the hop that produced the
-- point. Both hashes are FNV-ish 64-bit values and must never round-trip
-- through a float.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS dsm_pipeline_stats
(
    tenant_id      LowCardinality(String),
    received_at    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- StatsPayload level, denormalized onto every point.
    env            LowCardinality(String),
    service        LowCardinality(String),
    tracer_version LowCardinality(String),
    lang           LowCardinality(String),
    version        LowCardinality(String),
    -- A flat list, not "key:value" tags: kept in wire order as an array.
    process_tags   Array(String) CODEC(ZSTD(3)),
    -- Bitmask of the products that were active in the tracer: bit 0 APM,
    -- bit 1 DSM. Stored as the number, so a bit we do not know yet survives.
    product_mask   UInt64 CODEC(T64, ZSTD(1)),

    bucket_start       DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    bucket_duration_ns UInt64 CODEC(T64, ZSTD(1)),

    -- StatsPoint.
    edge_tags      Array(String) CODEC(ZSTD(3)),
    hash           UInt64 CODEC(T64, ZSTD(1)),
    parent_hash    UInt64 CODEC(T64, ZSTD(1)),
    -- 'current' or 'origin': whether the latency was measured against this
    -- point's own timestamp or the first point in the pathway. Kept as the
    -- string the tracer sent rather than an Enum8, because the vocabulary
    -- lives in dd-trace-go and grows there, not here.
    timestamp_type LowCardinality(String),

    -- THREE DDSketches per point, all of them raw bytes plus a decoded
    -- summary, for the same reason as apm_stats above. Note the units differ
    -- from APM's: these latencies are SECONDS, and payload_size is a
    -- distribution of message sizes in bytes, not a single number — the field
    -- name in dd-trace-go is misleading and the type ([]byte) is the truth.
    pathway_latency       String CODEC(ZSTD(3)),
    pathway_latency_state Enum8('absent' = 0, 'undecodable' = 1, 'empty' = 2, 'ok' = 3),
    pathway_count         Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    pathway_sum           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    pathway_min           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    pathway_max           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    pathway_bin_keys      Array(Int32) CODEC(ZSTD(1)),
    pathway_bin_counts    Array(Float64) CODEC(ZSTD(1)),

    edge_latency       String CODEC(ZSTD(3)),
    edge_latency_state Enum8('absent' = 0, 'undecodable' = 1, 'empty' = 2, 'ok' = 3),
    edge_count         Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    edge_sum           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    edge_min           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    edge_max           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    edge_bin_keys      Array(Int32) CODEC(ZSTD(1)),
    edge_bin_counts    Array(Float64) CODEC(ZSTD(1)),

    payload_size       String CODEC(ZSTD(3)),
    payload_size_state Enum8('absent' = 0, 'undecodable' = 1, 'empty' = 2, 'ok' = 3),
    payload_size_count Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    payload_size_sum   Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    payload_size_min   Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    payload_size_max   Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    payload_size_bin_keys   Array(Int32) CODEC(ZSTD(1)),
    payload_size_bin_counts Array(Float64) CODEC(ZSTD(1)),

    -- The proxy's own headers. The trace-agent adds them on the way through
    -- and they carry container identity that is NOWHERE in the body, so
    -- dropping them would lose the only link from a pathway to a workload.
    via              String,
    additional_tags  String CODEC(ZSTD(3)),
    container_tags   String CODEC(ZSTD(3)),
    content_encoding LowCardinality(String),

    -- dd-trace-go adds fields to this payload without any version negotiation,
    -- so a key we do not know is expected, not exceptional. The names go in
    -- the array (cheap to GROUP BY when a new one appears) and the values in
    -- the JSON, because a name alone would tell us a field exists and nothing
    -- about what it carries. Same idea as k8s_actions.extra_keys in 0001,
    -- one step further.
    unknown_keys Array(String),
    unknown_json String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(bucket_start)
-- The pathway graph is walked per service and per hash, always inside a time
-- range; hash last so the self-join that rebuilds the graph stays a range read.
ORDER BY (tenant_id, service, bucket_start, hash)
TTL toDateTime(bucket_start) + INTERVAL 30 DAY;


-- ---------------------------------------------------------------------------
-- dsm_backlogs: one row per Backlog, the queue depth a consumer has not read.
--
-- A separate table rather than arrays on dsm_pipeline_stats because backlogs
-- hang off the BUCKET, not off a StatsPoint: a bucket can carry backlogs and
-- no points, and folding them into a point's row would either duplicate them
-- or drop them.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS dsm_backlogs
(
    tenant_id      LowCardinality(String),
    received_at    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    env            LowCardinality(String),
    service        LowCardinality(String),
    tracer_version LowCardinality(String),
    lang           LowCardinality(String),
    version        LowCardinality(String),
    process_tags   Array(String) CODEC(ZSTD(3)),
    product_mask   UInt64 CODEC(T64, ZSTD(1)),

    bucket_start       DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    bucket_duration_ns UInt64 CODEC(T64, ZSTD(1)),

    -- Flat "type:produced" / "topic:x" style strings that identify WHICH
    -- backlog this is. An ordered array, not a map: the tracer builds the list
    -- and its order is part of the identity it hashes on.
    tags  Array(String) CODEC(ZSTD(3)),
    value Int64 CODEC(T64, ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(bucket_start)
ORDER BY (tenant_id, service, bucket_start)
TTL toDateTime(bucket_start) + INTERVAL 30 DAY;


-- ---------------------------------------------------------------------------
-- dsm_bucket_transactions: the two packed binary blobs a DSM bucket can carry.
--
-- StatsBucket.Transactions and .TransactionCheckpointIds are hand-rolled
-- binary, not msgpack structures: a record is
-- [checkpointId uint8][timestamp int64 big-endian][idLen uint8][id bytes] and
-- the checkpoint table is [id uint8][nameLen uint8][name bytes]. dd-trace-go
-- says the layout matches the Java tracer and the backend expects it exactly,
-- so it is kept BYTE FOR BYTE and parsed by whoever reads it, rather than
-- decoded now into a shape a future version would break.
--
-- Its own table for the same reason as dsm_backlogs: these are bucket-level,
-- and denormalizing them onto every StatsPoint would copy a blob per point
-- and still lose them for a bucket that has no points.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS dsm_bucket_transactions
(
    tenant_id      LowCardinality(String),
    received_at    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    env            LowCardinality(String),
    service        LowCardinality(String),
    tracer_version LowCardinality(String),
    lang           LowCardinality(String),
    version        LowCardinality(String),
    process_tags   Array(String) CODEC(ZSTD(3)),
    product_mask   UInt64 CODEC(T64, ZSTD(1)),

    bucket_start       DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    bucket_duration_ns UInt64 CODEC(T64, ZSTD(1)),

    transactions               String CODEC(ZSTD(3)),
    transaction_checkpoint_ids String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(bucket_start)
ORDER BY (tenant_id, service, bucket_start)
TTL toDateTime(bucket_start) + INTERVAL 30 DAY;


-- ---------------------------------------------------------------------------
-- dsm_messages: POST /api/v2/data_streams_messages, one row per array element.
--
-- Not the trace-agent at all — an Event Platform track that happens to live on
-- the trace.agent host, so the forwarder batches and the body is one JSON
-- array. The producer is not in the open agent repository, so there is no
-- struct to decode into and NONE IS INVENTED: the element is stored as the
-- JSON text it arrived as.
--
-- What is added is `keys`, the element's top-level key names. That is the one
-- thing that can be extracted without guessing at semantics, and it is enough
-- to answer "what does this track actually send?" with a GROUP BY instead of
-- a reverse-engineering session. Values stay in `message`.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS dsm_messages
(
    tenant_id   LowCardinality(String),
    received_at DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Index of the element in the array it arrived in. Batch order is the
    -- only ordering these messages have — nothing inside them is known to be
    -- a timestamp — so losing it would make the batch unreconstructable.
    position UInt32 CODEC(T64, ZSTD(1)),

    message String CODEC(ZSTD(3)),
    keys    Array(String) CODEC(ZSTD(3)),

    -- DD-EVP-ORIGIN and its version name the Event Platform producer, which
    -- is the closest thing this track has to a schema identifier.
    dd_evp_origin         LowCardinality(String),
    dd_evp_origin_version LowCardinality(String),
    content_encoding      LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
-- Arrival time is the only clock this table has.
ORDER BY (tenant_id, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;
