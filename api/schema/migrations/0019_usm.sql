-- 0019: Universal Service Monitoring — what system-probe saw on each
-- connection, per HTTP endpoint, Kafka topic and database operation.
--
-- The agent ships these inside every Connection as four serialized messages
-- (HTTPAggregations, HTTP2Aggregations, DataStreamsAggregations,
-- DatabaseAggregations; agent-payload, process/agent.proto). `connections`
-- keeps the bytes. Here each entry is a row, so "which endpoints answer 5xx"
-- and "which topic is slow" are column filters.
--
-- Every table starts with the columns that find the connection in
-- `connections`: payload_id, pid and both addresses. Same partitioning, order
-- and TTL as `connections`, because they are read together.
--
-- Latencies are nanoseconds. A stat with one sample carries it in
-- first_latency_sample and has no sketch; with more, the sketch is a DDSketch
-- kept as bytes with its decoded numbers beside it, as in apm_stats (0003).
-- Enum columns hold the .proto's names (Get, PostgresSelectOp, RedisErrOom).

CREATE TABLE IF NOT EXISTS connection_http_stats
(
    tenant_id            LowCardinality(String),
    timestamp            DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                 LowCardinality(String),
    payload_id           UUID,
    pid                  Int32 CODEC(T64, ZSTD(1)),
    laddr_ip             String,
    laddr_port           Int32 CODEC(T64, ZSTD(1)),
    raddr_ip             String,
    raddr_port           Int32 CODEC(T64, ZSTD(1)),

    -- 'http' or 'http2': the two aggregations have one shape.
    protocol             LowCardinality(String),
    method               LowCardinality(String),
    -- The agent reads a bounded part of the request line; full_path = 0
    -- means the path may be cut short.
    path                 String CODEC(ZSTD(1)),
    full_path            UInt8,
    -- Newer agents report each status code; older ones only the class, and
    -- then status_code is 0.
    status_code          Int32,
    status_class         LowCardinality(String),
    count                UInt32,
    first_latency_sample Float64,
    -- Sent in place of a sketch by the discovery service map mode.
    latency_sum          Float64,

    latencies            String CODEC(ZSTD(3)),
    latencies_state      Enum8('absent' = 0, 'undecodable' = 1, 'empty' = 2, 'ok' = 3),
    sketch_count         Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_sum           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_min           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_max           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_bin_keys      Array(Int32) CODEC(ZSTD(1)),
    sketch_bin_counts    Array(Float64) CODEC(ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, pid)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

CREATE TABLE IF NOT EXISTS connection_kafka_stats
(
    tenant_id            LowCardinality(String),
    timestamp            DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                 LowCardinality(String),
    payload_id           UUID,
    pid                  Int32 CODEC(T64, ZSTD(1)),
    laddr_ip             String,
    laddr_port           Int32 CODEC(T64, ZSTD(1)),
    raddr_ip             String,
    raddr_port           Int32 CODEC(T64, ZSTD(1)),

    -- Kafka's own API key and version: 0 is Produce, 1 is Fetch.
    api_key              UInt32,
    api_version          UInt32,
    topic                String CODEC(ZSTD(1)),
    -- Kafka's error code, -1 to 119; 0 is success. Agents older than error
    -- codes send one count per topic, stored under 0.
    error_code           Int32,
    count                UInt32,
    first_latency_sample Float64,

    latencies            String CODEC(ZSTD(3)),
    latencies_state      Enum8('absent' = 0, 'undecodable' = 1, 'empty' = 2, 'ok' = 3),
    sketch_count         Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_sum           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_min           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_max           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_bin_keys      Array(Int32) CODEC(ZSTD(1)),
    sketch_bin_counts    Array(Float64) CODEC(ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, pid)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

CREATE TABLE IF NOT EXISTS connection_database_stats
(
    tenant_id            LowCardinality(String),
    timestamp            DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                 LowCardinality(String),
    payload_id           UUID,
    pid                  Int32 CODEC(T64, ZSTD(1)),
    laddr_ip             String,
    laddr_port           Int32 CODEC(T64, ZSTD(1)),
    raddr_ip             String,
    raddr_port           Int32 CODEC(T64, ZSTD(1)),

    -- 'postgres' or 'redis': the two databases system-probe decodes.
    dbms                 LowCardinality(String),
    operation            LowCardinality(String),
    -- Postgres: the table the operation ran on.
    table_name           String CODEC(ZSTD(1)),
    -- Redis: the key, and whether the agent cut it short.
    key_name             String CODEC(ZSTD(1)),
    key_truncated        UInt8,
    -- Redis: one row per error type; RedisNoError for the successes.
    error_type           LowCardinality(String),
    count                UInt32,
    first_latency_sample Float64,

    latencies            String CODEC(ZSTD(3)),
    latencies_state      Enum8('absent' = 0, 'undecodable' = 1, 'empty' = 2, 'ok' = 3),
    sketch_count         Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_sum           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_min           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_max           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    sketch_bin_keys      Array(Int32) CODEC(ZSTD(1)),
    sketch_bin_counts    Array(Float64) CODEC(ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, pid)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;
