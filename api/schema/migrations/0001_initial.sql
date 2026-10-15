-- 0001: the whole schema, as of the squash of 2026-10-13.
--
-- Every table starts with tenant_id: it is first in every ORDER BY, so a query
-- for one tenant skips the others' granules before it reads anything. Cheap to
-- carry with one tenant, expensive to add to a sort key later.
--
-- No database qualifier anywhere: the runner connects to the target database.

-- Agent core: metrics and their one-minute rollup, sketches, check runs,
-- logs, hosts, processes, events, Kubernetes resources and container
-- inventory.
--
-- One table per payload type, never one per metric. Every ClickHouse table is
-- its own directories and its own merges, so thousands of metric tables sink
-- the server on file count alone; a LowCardinality metric name costs a
-- dictionary id per row instead.

-- TAG columns are Map(LowCardinality(String), Array(LowCardinality(String))).
--
-- A Datadog tag is a label in a SET, not a key-value property: two tags may
-- share the part before the colon, and real clusters produce them constantly —
-- a pod behind two services carries kube_service:a AND kube_service:b. A plain
-- Map(key, value) would keep whichever won and drop the rest, so the value
-- side is an ARRAY holding every value that arrived, in order. It is free on
-- disk (15.23 MiB vs 15.22 for the plain map over the same 800k-row slice),
-- and the ~60% penalty on map lookups lands on cold paths only — the hot
-- filters go through the materialised columns, which read tags['k'][1].
-- Queries test membership: has(tags['k'], 'v').
-- Measurements and the rejected alternatives:
-- api/docs/decisions/0001-tags-are-a-multiset.md.
--
-- Values are LowCardinality, plus ZSTD(3). On a real Kubernetes cluster's
-- telemetry, 704k metric points, measured with single-valued maps:
--
--   Map(LowCardinality, String) + LZ4   96.5 bytes/point
--   Map(LowCardinality, String) + ZSTD1 43.5 bytes/point
--   Map(LowCardinality, LowCardinality) 17.6 bytes/point   <- this
--
-- 5.5x smaller and not a trade: inserting was marginally FASTER than LZ4
-- (1.34s vs 1.44s for the same batch) and scanning the map twice as fast,
-- because comparisons run against dictionary ids instead of strings.
-- Kubernetes tags are bounded and repetitive — a namespace, a deployment, an
-- image — so the dictionary stays small.
--
-- Kubernetes labels and annotations, and the gohai host maps, are NOT tags
-- and stay ordinary maps: their APIs forbid duplicate keys, so the array
-- shape would pay the lookup penalty for a problem they cannot have.
-- Annotations keep String values: Kubernetes caps a label value at 63
-- characters but not an annotation, and last-applied-configuration routinely
-- holds kilobytes of JSON.
--
-- `extra` columns, Map(String, String), hold the keys the upstream model does
-- not declare, each value JSON-encoded so one column takes a string, a number,
-- an object or an array. A newer sender's field is visible and readable before
-- anyone adds a column for it. A String column holding a sub-object verbatim
-- is empty when the key was absent, which is not the same as "{}".


-- ---------------------------------------------------------------------------
-- Metric points: /api/v1/series and /api/v2/series
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS metrics
(
    -- Cheap to carry, expensive to add later: its place at the head of
    -- ORDER BY could only be changed by rewriting the table. Comes from the
    -- API key the request authenticated with.
    tenant_id    LowCardinality(String),

    -- DoubleDelta suits timestamps rising by a fixed step (the agent sends
    -- every 15 s); ZSTD takes the rest.
    timestamp    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    metric       LowCardinality(String),
    host         LowCardinality(String),
    metric_type  LowCardinality(String),     -- GAUGE / COUNT / RATE
    source_type  LowCardinality(String),
    unit         LowCardinality(String),
    interval     UInt32 CODEC(DoubleDelta, ZSTD(1)),

    -- Gorilla XORs consecutive floats and stores only the differing bits;
    -- for slowly changing metrics it compresses several times better than
    -- ZSTD alone.
    value        Float64 CODEC(Gorilla, ZSTD(1)),

    -- The tags filtered on most, lifted into columns: far faster than a map
    -- lookup.
    env          LowCardinality(String) MATERIALIZED tags['env'][1],
    service      LowCardinality(String) MATERIALIZED tags['service'][1],
    kube_namespace  LowCardinality(String) MATERIALIZED tags['kube_namespace'][1],
    kube_deployment LowCardinality(String) MATERIALIZED tags['kube_deployment'][1],
    pod_name     String MATERIALIZED tags['pod_name'][1],

    tags         Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- One id per real series, whatever order its tags arrived in. The hash of
    -- a Map depends on the order of its keys, and nothing guarantees a writer
    -- sends them in the same order twice, so keys and the values under each
    -- key are sorted before hashing; values too, because a tag is a multiset.
    -- Without it a space aggregation such as `sum:` adds one series up
    -- several times, and metrics_1m holds a row per bogus series per minute.
    series_id    UInt64 MATERIALIZED sipHash64(tenant_id, metric, host, mapSort(mapApply((k, v) -> (k, arraySort(v)), tags))),

    -- Datadog's provenance: which product, category and integration produced
    -- the metric (agent vs serverless vs the OTel exporter; dogstatsd vs a
    -- check). Three columns because the question is "everything from
    -- dogstatsd", a filter on one of them. Zero means not sent, as on the
    -- wire: the proto default is 0 and the enum reserves 0 for unknown.
    origin_product  UInt32 CODEC(T64, ZSTD(1)),
    origin_category UInt32 CODEC(T64, ZSTD(1)),
    origin_service  UInt32 CODEC(T64, ZSTD(1)),

    -- v2's [type, name] resource list beyond the host (device, and whatever
    -- Datadog adds next). A plain Map, not the tag shape: one name per type.
    -- v1's `device` is written here as device=<name>, so one query covers both
    -- wire versions.
    resources    Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    -- v1's undeclared series keys beyond device, source_type_name and unit.
    -- Routine, not an anomaly: the agent's own v1 encoder writes keys the
    -- published model does not declare.
    extra        Map(String, String) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    -- mapValues on an array-valued map yields Array(Array(...)), which
    -- bloom_filter rejects; flattened, the index still sees every tag value.
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
-- Daily: 30 partitions for 30 days, and an expired day leaves as a whole
-- partition rather than row by row. For retention in months or years switch
-- to toYYYYMM(timestamp) — hundreds of partitions slow query planning.
PARTITION BY toDate(timestamp)
-- The sort order decides both compression (neighbouring rows are alike) and
-- speed (whole granules are skipped by key). Metric, host, time fits the most
-- common query: one metric, one host, a time range. tenant_id first, so one
-- tenant's query skips everyone else's granules before reading anything.
ORDER BY (tenant_id, metric, host, timestamp)
TTL toDateTime(timestamp) + INTERVAL 30 DAY
SETTINGS index_granularity = 8192;

-- One-minute rollup, kept far longer than the raw points. It stores aggregate
-- STATES rather than numbers, so minutes merge into hours or days exactly —
-- no averaging of averages.
CREATE TABLE IF NOT EXISTS metrics_1m
(
    tenant_id   LowCardinality(String),
    bucket      DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    metric      LowCardinality(String),
    host        LowCardinality(String),
    series_id   UInt64,
    points      AggregateFunction(count, Float64),
    value_avg   AggregateFunction(avg, Float64),
    value_min   AggregateFunction(min, Float64),
    value_max   AggregateFunction(max, Float64),
    value_sum   AggregateFunction(sum, Float64)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(bucket)
ORDER BY (tenant_id, metric, host, series_id, bucket)
TTL bucket + INTERVAL 400 DAY;

CREATE MATERIALIZED VIEW IF NOT EXISTS metrics_1m_mv
TO metrics_1m AS
SELECT
    tenant_id,
    toStartOfMinute(timestamp) AS bucket,
    metric,
    host,
    series_id,
    countState(value) AS points,
    avgState(value)   AS value_avg,
    minState(value)   AS value_min,
    maxState(value)   AS value_max,
    sumState(value)   AS value_sum
FROM metrics
GROUP BY tenant_id, bucket, metric, host, series_id;

-- ---------------------------------------------------------------------------
-- DDSketch distributions: /api/beta/sketches
--
-- A different shape from a point — a histogram as two parallel arrays — so a
-- table of its own. Its columns otherwise parallel metrics, so a metric and
-- its distribution answer the same questions.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sketches
(
    tenant_id     LowCardinality(String),
    timestamp     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    metric        LowCardinality(String),
    host          LowCardinality(String),
    tags          Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    count         UInt64 CODEC(T64, ZSTD(1)),
    min           Float64 CODEC(Gorilla, ZSTD(1)),
    max           Float64 CODEC(Gorilla, ZSTD(1)),
    avg           Float64 CODEC(Gorilla, ZSTD(1)),
    sum           Float64 CODEC(Gorilla, ZSTD(1)),

    -- The sketch's buckets: k are logarithmic indexes, n their counts. Only
    -- meaningful together, and only together can they be merged across hosts.
    bucket_keys   Array(Int32) CODEC(ZSTD(1)),
    bucket_counts Array(UInt32) CODEC(ZSTD(1)),

    -- Same canonical hash as metrics.series_id.
    series_id     UInt64 MATERIALIZED sipHash64(tenant_id, metric, host, mapSort(mapApply((k, v) -> (k, arraySort(v)), tags))),

    origin_product  UInt32 CODEC(T64, ZSTD(1)),
    origin_category UInt32 CODEC(T64, ZSTD(1)),
    origin_service  UInt32 CODEC(T64, ZSTD(1)),
    -- Empty for the agent's own sketches, which carry a flat host field; here
    -- for a third-party or OTLP source that sends a resource list.
    resources     Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    extra         Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, metric, host, timestamp)
TTL toDateTime(timestamp) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Check statuses: /api/v1/check_run
-- Few rows and a different query pattern (the latest status per check).
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS check_runs
(
    tenant_id  LowCardinality(String),
    timestamp  DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    check_name LowCardinality(String),
    host       LowCardinality(String),
    status     Enum8('OK' = 0, 'WARNING' = 1, 'CRITICAL' = 2, 'UNKNOWN' = 3),
    message    String CODEC(ZSTD(3)),
    tags       Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- The agent's own service check has exactly the declared keys, so a
    -- non-empty extra is itself the signal: a newer agent or a third-party
    -- client sending something of its own.
    extra      Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (tenant_id, check_name, host, timestamp)
TTL timestamp + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- Logs: /api/v2/logs
-- A large text field, filtered by service and time and searched by content:
-- hence its own sort order and the stronger ZSTD.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS logs
(
    tenant_id  LowCardinality(String),
    timestamp  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host       LowCardinality(String),
    service    LowCardinality(String),
    source     LowCardinality(String),
    status     LowCardinality(String),
    message    String CODEC(ZSTD(3)),
    tags       Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- Everything the sender included beyond the declared fields: dd.trace_id,
    -- the one key that joins a log line to its trace, usr.*, http.*, error.*,
    -- whole nested objects. Native JSON, because values are read back by path
    -- (attributes.usr.id) and each path is its own subcolumn. The keys lifted
    -- into real columns are removed first: a value in two places can disagree
    -- with itself.
    attributes JSON,
    -- Which resolution step produced the timestamp. Without it "the sender
    -- sent none" and "the sender sent one we could not parse" are the same
    -- row, and only the second is a bug.
    timestamp_source LowCardinality(String),

    -- A token index drops granules when searching for a phrase, before
    -- message itself is read.
    INDEX idx_message message TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, service, host, timestamp)
TTL toDateTime(timestamp) + INTERVAL 14 DAY;

-- ---------------------------------------------------------------------------
-- Host metadata from /intake/ (gohai): one current row per host, newer
-- versions replace older ones at merge time.
--
-- Sub-objects whose shape shifts between agent versions (meta, network,
-- filesystem, logs, otlp) are kept as JSON text rather than a column per key
-- of a structure the agent is free to change. The flat ones (system_stats,
-- install_method, proxy_info, container_meta) are Maps because their keys are
-- the question, and ordinary Maps because a JSON object cannot repeat a key.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS hosts
(
    tenant_id     LowCardinality(String),
    host          String,
    seen_at       DateTime('UTC'),
    agent_version LowCardinality(String),
    os            LowCardinality(String),
    platform      Map(LowCardinality(String), LowCardinality(String)) CODEC(ZSTD(3)),
    cpu           Map(LowCardinality(String), LowCardinality(String)) CODEC(ZSTD(3)),
    memory        Map(LowCardinality(String), LowCardinality(String)) CODEC(ZSTD(3)),
    tags          Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    uuid           String,
    agent_flavor   LowCardinality(String),
    python_version LowCardinality(String),
    meta           String CODEC(ZSTD(3)),
    system_stats   Map(String, String) CODEC(ZSTD(3)),
    network        String CODEC(ZSTD(3)),
    filesystem     String CODEC(ZSTD(3)),
    logs           String CODEC(ZSTD(3)),
    install_method Map(String, String) CODEC(ZSTD(3)),
    proxy_info     Map(String, String) CODEC(ZSTD(3)),
    otlp           String CODEC(ZSTD(3)),
    container_meta Map(String, String) CODEC(ZSTD(3)),
    -- An older agent omits both keys, and "FIPS is off" is a different
    -- statement from "this agent does not know about FIPS".
    fips_mode          Nullable(UInt8),
    fips_proxy_enabled Nullable(UInt8),
    -- Keyed by tag SOURCE (system, google_tags, a custom provider), each
    -- holding that source's whole tag list — on a cloud host the provider's
    -- tags live here.
    host_tags    Map(LowCardinality(String), Array(String)) CODEC(ZSTD(3)),
    -- gohai sections beyond the five with columns, and top-level envelope
    -- keys the decoder does not know.
    gohai_extra  Map(String, String) CODEC(ZSTD(3)),
    intake_extra Map(String, String) CODEC(ZSTD(3)),
    -- The legacy V5 process snapshot riding along on the host payload. The
    -- process intake owns that format, so it is kept verbatim, not parsed twice.
    resources    String CODEC(ZSTD(3))
)
ENGINE = ReplacingMergeTree(seen_at)
ORDER BY (tenant_id, host);

-- ---------------------------------------------------------------------------
-- Processes: /api/v1/collector (process-agent), one row per Process of a
-- CollectorProc frame.
--
-- Snapshots are heavy — hundreds of processes per host every ~10 s — hence a
-- TTL shorter than metrics: last week's process list interests nobody and
-- outweighs a month of metrics.
--
-- The frame envelope is denormalised onto every row despite snapshot_id:
-- every question starts with a host and a time window, and a JOIN for "which
-- agent version" would run on each one. LowCardinality and repetition within
-- a batch let the dictionary swallow it. Proto enums are stored by name, never
-- by number: the numbers belong to a .proto we do not own.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS processes
(
    tenant_id     LowCardinality(String),
    timestamp     DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    host          LowCardinality(String),

    pid           Int32 CODEC(T64, ZSTD(1)),
    ppid          Int32 CODEC(T64, ZSTD(1)),
    user          LowCardinality(String),

    -- The executable name repeats in every snapshot, so a dictionary; the
    -- command line varies, so a plain String with strong ZSTD.
    comm          LowCardinality(String),
    exe           String CODEC(ZSTD(1)),
    cmdline       String CODEC(ZSTD(3)),

    rss           UInt64 CODEC(T64, ZSTD(1)),
    vms           UInt64 CODEC(T64, ZSTD(1)),
    cpu_pct       Float32 CODEC(Gorilla, ZSTD(1)),
    threads       Int32 CODEC(T64, ZSTD(1)),
    open_fds      Int32 CODEC(T64, ZSTD(1)),

    state         LowCardinality(String),
    -- Meaningless unless has_create_time = 1.
    create_time   DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    container_id  String CODEC(ZSTD(1)),

    tags          Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- The frame this process arrived in; joins to process_snapshots. It also
    -- groups the chunks of one snapshot: the agent splits a big host into
    -- several frames (group_id/group_size), each with its own id, so "one
    -- snapshot" is really (host, group_id).
    snapshot_id UUID,

    -- The pid inside the process's own PID namespace: in Kubernetes, the
    -- number the container sees and the one `kubectl exec ... ps` shows.
    -- key is the agent's dedup key.
    ns_pid Int32 CODEC(T64, ZSTD(1)),
    key UInt32 CODEC(T64, ZSTD(1)),

    cwd String CODEC(ZSTD(3)),
    root String CODEC(ZSTD(3)),
    on_disk UInt8,
    pgroup Int32 CODEC(T64, ZSTD(1)),

    -- The argv array. cmdline joins it on spaces and is lossy: `--filter=a b`
    -- cannot be told from two arguments.
    args Array(String) CODEC(ZSTD(3)),

    -- Real, effective and saved ids — a setuid binary is exactly where the
    -- user name alone lies.
    uid Int32 CODEC(T64, ZSTD(1)),
    gid Int32 CODEC(T64, ZSTD(1)),
    euid Int32 CODEC(T64, ZSTD(1)),
    egid Int32 CODEC(T64, ZSTD(1)),
    suid Int32 CODEC(T64, ZSTD(1)),
    sgid Int32 CODEC(T64, ZSTD(1)),

    -- Swap shows a host thrashing; text/lib/data/dirty tell a heap leak from
    -- a mapped file.
    mem_swap UInt64 CODEC(T64, ZSTD(1)),
    mem_shared UInt64 CODEC(T64, ZSTD(1)),
    mem_text UInt64 CODEC(T64, ZSTD(1)),
    mem_lib UInt64 CODEC(T64, ZSTD(1)),
    mem_data UInt64 CODEC(T64, ZSTD(1)),
    mem_dirty UInt64 CODEC(T64, ZSTD(1)),

    -- user_time/system_time are the cumulative counters the percentages are
    -- derived from, and survive a missed sample.
    cpu_last_cpu LowCardinality(String),
    cpu_user_pct Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_system_pct Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_nice Int32 CODEC(T64, ZSTD(1)),
    cpu_user_time Int64 CODEC(T64, ZSTD(1)),
    cpu_system_time Int64 CODEC(T64, ZSTD(1)),

    -- Per core, as parallel arrays: cpu_core_names[i] pairs with
    -- cpu_core_pcts[i]. Not a Map: the field is repeated, so ordered, and a
    -- host may report the same core name twice.
    cpu_core_names Array(LowCardinality(String)),
    cpu_core_pcts Array(Float32) CODEC(ZSTD(1)),

    io_read_rate Float32 CODEC(Gorilla, ZSTD(1)),
    io_write_rate Float32 CODEC(Gorilla, ZSTD(1)),
    io_read_bytes_rate Float32 CODEC(Gorilla, ZSTD(1)),
    io_write_bytes_rate Float32 CODEC(Gorilla, ZSTD(1)),

    voluntary_ctx_switches UInt64 CODEC(T64, ZSTD(1)),
    involuntary_ctx_switches UInt64 CODEC(T64, ZSTD(1)),

    -- Connections opened and bytes per second, attributed to the process
    -- rather than its container.
    net_connection_rate Float32 CODEC(Gorilla, ZSTD(1)),
    net_bytes_rate Float32 CODEC(Gorilla, ZSTD(1)),

    process_context Array(String) CODEC(ZSTD(3)),

    language LowCardinality(String),

    -- Listening ports, in the order sent.
    port_tcp Array(Int32) CODEC(T64, ZSTD(1)),
    port_udp Array(Int32) CODEC(T64, ZSTD(1)),

    -- Service discovery: the bridge between a process and APM. The tracer
    -- runtime ids also appear in trace payloads, and are the only join
    -- between "this pid on this host" and "these traces".
    generated_service_name String,
    generated_service_name_source LowCardinality(String),
    dd_service_name String,
    dd_service_name_source LowCardinality(String),
    additional_generated_names Array(String),
    additional_generated_name_sources Array(LowCardinality(String)),
    tracer_runtime_ids Array(String),
    tracer_service_names Array(String),
    apm_instrumentation UInt8,

    -- A repeated oneof with one member today (LogResource{path}), kept as JSON
    -- — [{"logs":{"path":"..."}}] — so a member upstream adds later still
    -- arrives, named, with no schema change.
    service_resources String CODEC(ZSTD(3)),

    -- Whether the tracer was auto-injected into this process.
    injection_state LowCardinality(String),

    zombie_children_count UInt32 CODEC(T64, ZSTD(1)),
    zombie_net_rate Float64 CODEC(Gorilla, ZSTD(1)),
    has_zombie_aggregation UInt8,

    container_key UInt32 CODEC(T64, ZSTD(1)),

    -- Opaque bytes the backend resolves keys with; a ClickHouse String is a
    -- byte string, so they survive verbatim.
    byte_key String CODEC(ZSTD(1)),
    container_byte_key String CODEC(ZSTD(1)),

    -- 0 means the agent sent no start time: create_time is then not 1970 but
    -- absent, and every query reading create_time filters on this.
    has_create_time UInt8,

    -- Process.host, which the backend fills after resolution and a real agent
    -- leaves empty. JSON rather than eight columns empty in every row.
    process_host String CODEC(ZSTD(3)),

    network_id String,
    group_id Int32 CODEC(T64, ZSTD(1)),
    group_size Int32 CODEC(T64, ZSTD(1)),
    container_host_type LowCardinality(String),
    agent_version LowCardinality(String),
    request_id String,
    host_uuid String,
    os_name LowCardinality(String),
    os_platform LowCardinality(String),
    os_family LowCardinality(String),
    os_version LowCardinality(String),
    kernel_version LowCardinality(String),

    INDEX idx_tag_keys mapKeys(tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
-- "What ran on this host at that time" is the question that gets asked.
ORDER BY (tenant_id, host, timestamp, pid)
TTL timestamp + INTERVAL 7 DAY;

-- ---------------------------------------------------------------------------
-- Events: /api/v1/events and the agent's /intake/ events
--
-- Deploys, restarts, config changes, monitor alerts: the vertical line on a
-- chart that answers "what happened at 14:30". Events are not filtered by
-- name — the question wants ALL of them in a window to draw on the axis — so
-- time comes right after tenant_id, unlike metrics. Human and system scale,
-- tens or hundreds a day: a longer TTL and nothing to roll up.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS events
(
    tenant_id        LowCardinality(String),
    timestamp        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Ours. Datadog hands the client a numeric event id, hence event_id_num.
    event_id         UUID,
    event_id_num     UInt64 CODEC(T64, ZSTD(1)),

    title            String CODEC(ZSTD(1)),
    text             String CODEC(ZSTD(3)),
    host             LowCardinality(String),

    -- Coerced into Datadog's fixed vocabularies: dropping an event over an
    -- invented alert type would lose what we were asked to remember. What the
    -- sender wrote is in alert_type_raw and priority_raw.
    -- error / warning / info / success / user_update / recommendation / snapshot
    alert_type       LowCardinality(String),
    -- normal / low
    priority         LowCardinality(String),

    -- Datadog folds events with the same key into one thread (successive
    -- restarts of the same service, say).
    aggregation_key  String CODEC(ZSTD(1)),
    source_type_name LowCardinality(String),
    device_name      LowCardinality(String),

    env              LowCardinality(String) MATERIALIZED tags['env'][1],
    service          LowCardinality(String) MATERIALIZED tags['service'][1],
    kube_namespace   LowCardinality(String) MATERIALIZED tags['kube_namespace'][1],

    tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- The agent's own event type, from the /intake/ variant; empty for the
    -- public API's events, which have no such field.
    event_type       LowCardinality(String),
    -- Datadog's id space starts at 1, and "no parent" must not become 0.
    related_event_id Nullable(Int64) CODEC(T64, ZSTD(1)),
    -- Exactly the strings off the wire, empty when the sender sent nothing.
    alert_type_raw   String CODEC(ZSTD(1)),
    priority_raw     String CODEC(ZSTD(1)),
    extra            Map(String, String) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_aggkey     aggregation_key TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (tenant_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- Kubernetes resources: /api/v2/orch (orchestrator collectors, types 41..88)
--
-- One GENERIC table for all ~30 kinds, not one per kind, for the reason at the
-- top of this file. The columns every kind shares are real columns; numbers
-- that differ per kind live in `counts`, the typed extras cover the kinds
-- people ask about (Pod, Node, Service, RBAC), and `object` holds the whole
-- decoded collector object as JSON, which keeps the kinds with no typed
-- coverage lossless in one column. Repeated sub-messages (conditions,
-- container statuses) are parallel arrays in wire order: Nested is stored that
-- way anyway, and the flat form is what arrayZip and ARRAY JOIN read.
--
-- Append-only HISTORY, one row per object per collection pass, because "when
-- did this deployment lose its replicas" needs the passes that are gone.
-- "What is in the cluster right now" is k8s_resources_current below, fed by a
-- materialized view so the writer inserts once and both stay in step.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS k8s_resources
(
    tenant_id        LowCardinality(String),
    collected_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    cluster_id       LowCardinality(String),
    cluster_name     LowCardinality(String),
    kind             LowCardinality(String),   -- Pod / Node / Deployment / ...
    namespace        LowCardinality(String),
    name             String,
    uid              String,
    resource_version String,

    -- The first owner: enough for the Pod -> ReplicaSet -> Deployment walk,
    -- the join that actually gets asked for. All owners are in owner_*s.
    owner_kind       LowCardinality(String),
    owner_name       String,

    node_name        LowCardinality(String),
    phase            LowCardinality(String),
    status           LowCardinality(String),

    -- "Ready vs desired" is THE dashboard query for workloads.
    ready            Int32 CODEC(T64, ZSTD(1)),
    desired          Int32 CODEC(T64, ZSTD(1)),
    available        Int32 CODEC(T64, ZSTD(1)),

    -- Kind-specific numbers (restarts, updated replicas, taints...) in their
    -- own vocabulary, so a new kind needs no schema change.
    counts           Map(String, Int64),

    labels           Map(LowCardinality(String), LowCardinality(String)) CODEC(ZSTD(3)),
    annotations      Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    agent_version    LowCardinality(String),
    -- The agent splits one pass into group_size frames sharing a group_id;
    -- kept so a pass can be reassembled or spotted as partial.
    group_id         Int32 CODEC(T64, ZSTD(1)),
    group_size       Int32 CODEC(T64, ZSTD(1)),

    -- The frame header. The agent leaves org_id and subscription_id at 0 and
    -- the timestamp unset today; the day a sender fills them, they are kept.
    org_id Int32 CODEC(T64, ZSTD(1)),
    subscription_id UInt8,
    -- The raw int64: the frame format documents no unit and no sender seen
    -- fills it, so converting would write a guess down as fact. NULL when 0.
    header_timestamp Nullable(Int64),
    encoding LowCardinality(String),

    -- When the object was created, as opposed to when we saw it. Seconds, as
    -- on the wire.
    creation_timestamp Nullable(DateTime('UTC')),
    deletion_timestamp Nullable(DateTime('UTC')),
    deletion_grace_period_seconds Nullable(Int64),
    finalizers Array(String),

    owner_kinds Array(LowCardinality(String)),
    owner_names Array(String),
    owner_uids Array(String),

    -- "Why is this Deployment not Available" lives in reason and message. The
    -- times are Nullable because a condition that never probed or updated
    -- leaves them at 0, and 0 is absence, not 1970.
    condition_types Array(LowCardinality(String)),
    condition_statuses Array(LowCardinality(String)),
    condition_reasons Array(LowCardinality(String)),
    condition_messages Array(String) CODEC(ZSTD(3)),
    condition_last_transition Array(Nullable(DateTime('UTC'))),
    condition_last_update Array(Nullable(DateTime('UTC'))),
    condition_last_probe Array(Nullable(DateTime('UTC'))),
    -- The agent's one-line summary, separate from the list.
    condition_message String CODEC(ZSTD(3)),

    -- Requests and limits per container, JSON: a Map would lose which
    -- container each belonged to.
    resource_requirements String CODEC(ZSTD(3)),

    -- The agent's own computed numbers. Protobuf map keys are unique, so a
    -- plain Map.
    metrics Map(String, Float64),

    -- In protobuf's proto3 JSON mapping: lowerCamelCase names, 64-bit
    -- integers as strings, enums by name. Read whole, by a person, once:
    -- nothing filters on it.
    object String CODEC(ZSTD(3)),

    -- Pod.
    pod_ip String,
    nominated_node_name LowCardinality(String),
    qos_class LowCardinality(String),
    priority_class LowCardinality(String),
    start_time Nullable(DateTime('UTC')),
    scheduled_time Nullable(DateTime('UTC')),
    -- The agent host that reported the object; not node_name for
    -- cluster-scoped kinds.
    host_name LowCardinality(String),

    -- Container statuses, regular containers first and init containers
    -- appended (container_is_init says which): "which container in this pod
    -- is crashlooping, and with what message".
    container_names Array(LowCardinality(String)),
    container_ids Array(String),
    container_ready Array(UInt8),
    container_restarts Array(Int32),
    container_states Array(LowCardinality(String)),
    container_messages Array(String) CODEC(ZSTD(3)),
    container_images Array(String),
    container_image_ids Array(String),
    container_is_init Array(UInt8),

    -- Node. unschedulable is the cordon flag, the first thing asked about a
    -- sick node.
    pod_cidr String,
    pod_cidrs Array(String),
    unschedulable UInt8,
    taints String CODEC(ZSTD(3)),
    provider_id String,
    node_roles Array(LowCardinality(String)),
    -- In the wire's own units (millicores, bytes): "which node is full".
    capacity Map(String, Int64),
    allocatable Map(String, Int64),
    node_addresses String CODEC(ZSTD(3)),
    kubelet_version LowCardinality(String),
    kube_proxy_version LowCardinality(String),
    operating_system LowCardinality(String),
    architecture LowCardinality(String),
    kernel_version LowCardinality(String),
    os_image LowCardinality(String),
    container_runtime_version LowCardinality(String),

    -- Service.
    service_type LowCardinality(String),
    cluster_ip String,
    service_ports String CODEC(ZSTD(3)),

    -- RBAC: the permissions actually granted, the most security-relevant
    -- field in the orchestrator feed. JSON, because a rule is five string
    -- arrays and a binding a list of subjects plus a role reference.
    rbac_rules String CODEC(ZSTD(3)),
    rbac_subjects String CODEC(ZSTD(3)),
    rbac_role_ref String CODEC(ZSTD(3)),

    -- The frame envelope as JSON, minus the object list (that is the rows):
    -- the fields beyond the generic ones differ per kind. Every row of a pass
    -- carries the same bytes, so ZSTD collapses the repetition.
    envelope String CODEC(ZSTD(3)),

    -- Labels are the tags of the Kubernetes world, the filter reached for
    -- first, so they get the bloom filters tags get elsewhere.
    INDEX idx_label_keys   mapKeys(labels)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_label_values mapValues(labels) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(collected_at)
ORDER BY (tenant_id, cluster_id, kind, namespace, name, collected_at)
TTL toDateTime(collected_at) + INTERVAL 30 DAY;

-- One row per live object, the newest collection winning at merge time
-- (readers still use FINAL or argMax for exactness, as with hosts). Must stay
-- column-for-column identical to k8s_resources: if the two diverge, the view
-- fails at insert time and this table silently stops updating. No TTL: a live
-- object must never age out — rows leave only by being replaced.
CREATE TABLE IF NOT EXISTS k8s_resources_current
(
    tenant_id        LowCardinality(String),
    collected_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    cluster_id       LowCardinality(String),
    cluster_name     LowCardinality(String),
    kind             LowCardinality(String),
    namespace        LowCardinality(String),
    name             String,
    uid              String,
    resource_version String,

    owner_kind       LowCardinality(String),
    owner_name       String,

    node_name        LowCardinality(String),
    phase            LowCardinality(String),
    status           LowCardinality(String),

    ready            Int32 CODEC(T64, ZSTD(1)),
    desired          Int32 CODEC(T64, ZSTD(1)),
    available        Int32 CODEC(T64, ZSTD(1)),

    counts           Map(String, Int64),

    labels           Map(LowCardinality(String), LowCardinality(String)) CODEC(ZSTD(3)),
    annotations      Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    agent_version    LowCardinality(String),
    group_id         Int32 CODEC(T64, ZSTD(1)),
    group_size       Int32 CODEC(T64, ZSTD(1)),

    org_id Int32 CODEC(T64, ZSTD(1)),
    subscription_id UInt8,
    header_timestamp Nullable(Int64),
    encoding LowCardinality(String),
    creation_timestamp Nullable(DateTime('UTC')),
    deletion_timestamp Nullable(DateTime('UTC')),
    deletion_grace_period_seconds Nullable(Int64),
    finalizers Array(String),
    owner_kinds Array(LowCardinality(String)),
    owner_names Array(String),
    owner_uids Array(String),
    condition_types Array(LowCardinality(String)),
    condition_statuses Array(LowCardinality(String)),
    condition_reasons Array(LowCardinality(String)),
    condition_messages Array(String) CODEC(ZSTD(3)),
    condition_last_transition Array(Nullable(DateTime('UTC'))),
    condition_last_update Array(Nullable(DateTime('UTC'))),
    condition_last_probe Array(Nullable(DateTime('UTC'))),
    condition_message String CODEC(ZSTD(3)),
    resource_requirements String CODEC(ZSTD(3)),
    metrics Map(String, Float64),
    object String CODEC(ZSTD(3)),
    pod_ip String,
    nominated_node_name LowCardinality(String),
    qos_class LowCardinality(String),
    priority_class LowCardinality(String),
    start_time Nullable(DateTime('UTC')),
    scheduled_time Nullable(DateTime('UTC')),
    host_name LowCardinality(String),
    container_names Array(LowCardinality(String)),
    container_ids Array(String),
    container_ready Array(UInt8),
    container_restarts Array(Int32),
    container_states Array(LowCardinality(String)),
    container_messages Array(String) CODEC(ZSTD(3)),
    container_images Array(String),
    container_image_ids Array(String),
    container_is_init Array(UInt8),
    pod_cidr String,
    pod_cidrs Array(String),
    unschedulable UInt8,
    taints String CODEC(ZSTD(3)),
    provider_id String,
    node_roles Array(LowCardinality(String)),
    capacity Map(String, Int64),
    allocatable Map(String, Int64),
    node_addresses String CODEC(ZSTD(3)),
    kubelet_version LowCardinality(String),
    kube_proxy_version LowCardinality(String),
    operating_system LowCardinality(String),
    architecture LowCardinality(String),
    kernel_version LowCardinality(String),
    os_image LowCardinality(String),
    container_runtime_version LowCardinality(String),
    service_type LowCardinality(String),
    cluster_ip String,
    service_ports String CODEC(ZSTD(3)),
    rbac_rules String CODEC(ZSTD(3)),
    rbac_subjects String CODEC(ZSTD(3)),
    rbac_role_ref String CODEC(ZSTD(3)),
    envelope String CODEC(ZSTD(3)),

    INDEX idx_label_keys   mapKeys(labels)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_label_values mapValues(labels) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(collected_at)
ORDER BY (tenant_id, cluster_id, kind, namespace, name);

-- The columns are spelled out: ClickHouse freezes SELECT * at creation time
-- anyway, and the explicit list makes the next diff readable.
CREATE MATERIALIZED VIEW IF NOT EXISTS k8s_resources_current_mv
TO k8s_resources_current AS
SELECT
    tenant_id,
    collected_at,
    cluster_id,
    cluster_name,
    kind,
    namespace,
    name,
    uid,
    resource_version,
    owner_kind,
    owner_name,
    node_name,
    phase,
    status,
    ready,
    desired,
    available,
    counts,
    labels,
    annotations,
    tags,
    agent_version,
    group_id,
    group_size,
    org_id,
    subscription_id,
    header_timestamp,
    encoding,
    creation_timestamp,
    deletion_timestamp,
    deletion_grace_period_seconds,
    finalizers,
    owner_kinds,
    owner_names,
    owner_uids,
    condition_types,
    condition_statuses,
    condition_reasons,
    condition_messages,
    condition_last_transition,
    condition_last_update,
    condition_last_probe,
    condition_message,
    resource_requirements,
    metrics,
    object,
    pod_ip,
    nominated_node_name,
    qos_class,
    priority_class,
    start_time,
    scheduled_time,
    host_name,
    container_names,
    container_ids,
    container_ready,
    container_restarts,
    container_states,
    container_messages,
    container_images,
    container_image_ids,
    container_is_init,
    pod_cidr,
    pod_cidrs,
    unschedulable,
    taints,
    provider_id,
    node_roles,
    capacity,
    allocatable,
    node_addresses,
    kubelet_version,
    kube_proxy_version,
    operating_system,
    architecture,
    kernel_version,
    os_image,
    container_runtime_version,
    service_type,
    cluster_ip,
    service_ports,
    rbac_rules,
    rbac_subjects,
    rbac_role_ref,
    envelope
FROM k8s_resources;

-- ---------------------------------------------------------------------------
-- Raw Kubernetes manifests: /api/v2/orchmanif (CollectorManifest, types 80-82)
--
-- The object's own YAML or JSON, "exactly what kubectl would show". Big and
-- rarely read — it is for the moment someone needs the full spec of the pod
-- that died — hence ZSTD(3) on the body and a TTL SHORTER than k8s_resources:
-- a week of full YAML outweighs a month of the structured rows.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS k8s_manifests
(
    tenant_id        LowCardinality(String),
    collected_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    cluster_id       LowCardinality(String),
    cluster_name     LowCardinality(String),
    uid              String,
    kind             LowCardinality(String),
    api_version      LowCardinality(String),
    resource_version String,

    content          String CODEC(ZSTD(3)),
    content_type     LowCardinality(String),
    is_terminated    UInt8,

    org_id Int32 CODEC(T64, ZSTD(1)),
    subscription_id UInt8,
    header_timestamp Nullable(Int64),
    encoding LowCardinality(String),

    group_id Int32 CODEC(T64, ZSTD(1)),
    group_size Int32 CODEC(T64, ZSTD(1)),
    host_name LowCardinality(String),
    agent_version LowCardinality(String),
    -- datadogAgent, datadogExporter or unknown: which pipeline produced the
    -- manifest, in an installation that runs both.
    origin_collector LowCardinality(String),

    -- The envelope's, the CRD/CR wrapper's and the manifest's own tags merged,
    -- because "manifests tagged env:prod" does not care where a tag came
    -- from...
    tags Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- ...and, keyed by source ('envelope' | 'wrapper' | 'manifest'), each
    -- source's raw tag strings.
    tag_sources Map(LowCardinality(String), Array(String)) CODEC(ZSTD(3)),

    node_name LowCardinality(String),
    -- The numeric kind marker and the manifest schema version. Redundant with
    -- kind/api_version today; "redundant today" is how a format change hides.
    type Int32 CODEC(T64, ZSTD(1)),
    version LowCardinality(String),
    -- On a custom resource, where the interesting part lives.
    extra_attributes Map(String, String) CODEC(ZSTD(3)),

    -- content is bytes on the wire and a byte string here, so nothing is
    -- corrupted, but a reader deserves to know before parsing. 1 = valid UTF-8.
    content_is_utf8 UInt8,

    -- The envelope as JSON minus its manifest list (that is the rows): the
    -- reporting agent's SystemInfo, and whatever the envelope grows next.
    envelope String CODEC(ZSTD(3)),
    -- The machine the object was observed on, JSON: its shape belongs to the
    -- host inventory, and nothing joins on it yet.
    manifest_host String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)                 TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(collected_at)
ORDER BY (tenant_id, cluster_id, kind, uid, collected_at)
TTL toDateTime(collected_at) + INTERVAL 7 DAY;

-- ---------------------------------------------------------------------------
-- Cluster summary: CollectorCluster (type 46), one row per collection pass.
--
-- Capacity over time: node count, allocatable vs capacity, and the version
-- spread of kubelets and apiservers (version -> node count, which is how you
-- watch an upgrade roll through). One row per cluster per pass, so monthly
-- partitions and a long TTL cost nothing.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS k8s_cluster
(
    tenant_id          LowCardinality(String),
    collected_at       DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    cluster_id         LowCardinality(String),
    cluster_name       LowCardinality(String),

    node_count         UInt32 CODEC(T64, ZSTD(1)),
    pod_capacity       UInt32 CODEC(T64, ZSTD(1)),
    pod_allocatable    UInt32 CODEC(T64, ZSTD(1)),
    cpu_capacity       UInt64 CODEC(T64, ZSTD(1)),
    cpu_allocatable    UInt64 CODEC(T64, ZSTD(1)),
    memory_capacity    UInt64 CODEC(T64, ZSTD(1)),
    memory_allocatable UInt64 CODEC(T64, ZSTD(1)),

    kubelet_versions   Map(String, UInt32),
    apiserver_versions Map(String, UInt32),

    org_id Int32 CODEC(T64, ZSTD(1)),
    subscription_id UInt8,
    header_timestamp Nullable(Int64),
    encoding LowCardinality(String),

    group_id Int32 CODEC(T64, ZSTD(1)),
    group_size Int32 CODEC(T64, ZSTD(1)),
    agent_version LowCardinality(String),

    -- The frame envelope's tags and the Cluster object's own, apart: merging
    -- them would hide which producer attached a tag.
    tags Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    cluster_tags Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    resource_version String,
    creation_timestamp Nullable(DateTime('UTC')),
    metrics Map(String, Float64),

    -- GPUs and other extended resources.
    extended_resources_capacity Map(String, Int64),
    extended_resources_allocatable Map(String, Int64),

    -- Per node, parallel arrays in wire order: "which of my nodes is on the
    -- old kubelet", with names. The two map arrays keep the Kubernetes
    -- quantity strings ("16Gi", "4") the wire sends here; converting would
    -- guess at units.
    nodes_name Array(String),
    nodes_region Array(LowCardinality(String)),
    nodes_instance_type Array(LowCardinality(String)),
    nodes_os Array(LowCardinality(String)),
    nodes_os_image Array(String),
    nodes_architecture Array(LowCardinality(String)),
    nodes_kernel_version Array(String),
    nodes_container_runtime_version Array(String),
    nodes_kubelet_version Array(LowCardinality(String)),
    nodes_allocatable Array(Map(String, String)) CODEC(ZSTD(3)),
    nodes_capacity Array(Map(String, String)) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)                  TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags))  TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(collected_at)
ORDER BY (tenant_id, cluster_id, collected_at)
TTL toDateTime(collected_at) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- Kubernetes action results: /api/v2/kubeactions
--
-- The audit half of remote configuration: who asked for what, and how it
-- went. Human scale like events — tens a day — and audited like them, so the
-- same monthly partitions and 90-day TTL.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS k8s_actions
(
    tenant_id          LowCardinality(String),
    timestamp          DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    action_id          String,
    org_id             Int64 CODEC(T64, ZSTD(1)),
    event_type         LowCardinality(String),  -- action_received / action_progress / action_executed
    status             LowCardinality(String),  -- success / failed / skipped / expired / claimed / in_progress
    action_type        LowCardinality(String),

    cluster_id         LowCardinality(String),
    cluster_name       LowCardinality(String),
    resource_id        String,
    resource_kind      LowCardinality(String),
    resource_name      String,
    resource_namespace LowCardinality(String),

    requested_by       LowCardinality(String),
    message            String CODEC(ZSTD(3)),
    -- Names of the undeclared JSON keys, cheap to filter on; their values are
    -- in extra.
    extra_keys         Array(String),

    -- Whatever the executor attached, name -> bytes; a ClickHouse String is a
    -- byte string, so a payload that is not text survives.
    payloads Map(String, String) CODEC(ZSTD(3)),
    -- The undeclared keys with their values, as a JSON object.
    extra String CODEC(ZSTD(3)),
    -- The timestamp exactly as it arrived: the row's own falls back to arrival
    -- time when this is empty or unparseable.
    timestamp_raw String
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (tenant_id, cluster_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- Container lifecycle events: /api/v2/contlcycle
--
-- The restart/OOM history, so append-only: a ReplacingMergeTree would eat the
-- very rows the table exists for. Nullable wherever the wire makes a
-- three-way distinction — exited 0, exited nonzero, "the runtime never said" —
-- because collapsing the last into 0 would invent clean exits; absent
-- timestamps stay absent, never 1970. old_state/new_state are the transition
-- as one label; the structured columns after them answer "every OOM kill with
-- signal 9".
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS container_events
(
    tenant_id      LowCardinality(String),
    timestamp      DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    host           LowCardinality(String),
    cluster_id     LowCardinality(String),
    object_kind    LowCardinality(String),  -- container / pod / task
    event_type     LowCardinality(String),  -- start / delete / ...

    container_id   String,
    container_name LowCardinality(String),
    pod_uid        String,
    task_arn       String,
    source         LowCardinality(String),

    exit_code      Nullable(Int32),
    created_at     Nullable(DateTime64(3, 'UTC')),
    exited_at      Nullable(DateTime64(3, 'UTC')),

    owner_type     LowCardinality(String),
    owner_uid      String,

    old_state      LowCardinality(String),
    new_state      LowCardinality(String),
    transition_at  Nullable(DateTime64(3, 'UTC')),

    payload_version LowCardinality(String),

    -- Which arm of the Event oneof produced the row, or 'unknown' for a nil
    -- oneof or a variant newer than the schema — otherwise indistinguishable
    -- from a typed event whose fields happened to be empty.
    event_variant LowCardinality(String),

    -- Regular, init or ephemeral: a restart storm in init containers is a
    -- different incident.
    container_kind LowCardinality(String),

    -- How exact the transition time is, and whether the agent knows it missed
    -- states in between: how much to trust the row.
    precision LowCardinality(String),
    missed_intermediate LowCardinality(String),

    old_state_kind LowCardinality(String),
    old_reason Nullable(String),
    old_exit_code Nullable(Int32),
    old_signal Nullable(Int32),
    new_state_kind LowCardinality(String),
    new_reason Nullable(String),
    new_exit_code Nullable(Int32),
    new_signal Nullable(Int32),

    -- Which pod status field moved (phase, or a condition). The value is a
    -- oneof, so its variant is recorded and each arm has its own columns.
    pod_status_field LowCardinality(String),
    old_state_variant LowCardinality(String),
    old_phase Nullable(String),
    old_condition_type Nullable(String),
    old_condition_status Nullable(String),
    old_condition_reason Nullable(String),
    old_condition_message Nullable(String),
    new_state_variant LowCardinality(String),
    new_phase Nullable(String),
    new_condition_type Nullable(String),
    new_condition_status Nullable(String),
    new_condition_reason Nullable(String),
    new_condition_message Nullable(String),

    -- The container name is a proto3 optional: 1 = the sender set it, so an
    -- empty container_name can be told from a missing one.
    container_name_present UInt8
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, cluster_id, container_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Container image inventory: /api/v2/contimage
--
-- Keyed by (image_key, host). The digest is the immutable identity — tags move
-- between builds, the digest never does, and it is what an SBOM references.
-- host must stay in the key: without it a 50-node rollout collapses into one
-- row holding whichever node reported last.
--
-- The TTL makes the inventory self-healing: while an image is on a node every
-- collection refreshes collected_at; once it is gone nothing does and it ages
-- out. ReplacingMergeTree has no notion of deletion, so without the TTL stale
-- placements would stay forever.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS container_images
(
    tenant_id    LowCardinality(String),
    collected_at DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    host         LowCardinality(String),

    -- The registry digest, or the image id when there is none. A missing
    -- digest is NORMAL — an image built locally, loaded from a tarball, or
    -- pulled before the runtime resolved RepoDigests — and keying on digest
    -- alone would collapse every such image on a host into one row. image_id
    -- is the image CONFIG digest: content-addressed too, but computed locally,
    -- so weaker across heterogeneous runtimes and only the fallback.
    image_key    String,

    -- 'digest' or 'image_id': which fed image_key. "Images with no registry
    -- digest" — the ones an SBOM will never join to — is then a query.
    identity_source LowCardinality(String),

    image_id     String,
    digest       String,
    name         String,
    short_name   LowCardinality(String),
    registry     LowCardinality(String),

    repo_tags    Array(String),
    repo_digests Array(String),

    size_bytes   UInt64 CODEC(T64, ZSTD(1)),
    os_name      LowCardinality(String),
    os_version   LowCardinality(String),
    architecture LowCardinality(String),
    layer_count  UInt32 CODEC(T64, ZSTD(1)),
    layer_bytes  UInt64 CODEC(T64, ZSTD(1)),

    -- Often unknown to the agent, and a missing date must not become 1970.
    built_at     Nullable(DateTime64(3, 'UTC')),
    published_at Nullable(DateTime64(3, 'UTC')),

    dd_tags      Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    payload_version LowCardinality(String),

    -- Which pipeline produced the inventory ('agent', 'other'); NULL when the
    -- sender did not say.
    source Nullable(String),

    -- The layer stack, index-aligned arrays in wire order: an image IS its
    -- ordered layers, and the history is its Dockerfile.
    layer_media_types Array(LowCardinality(String)),
    layer_digests Array(String),
    layer_sizes Array(Int64),
    -- A layer may be fetchable from several URLs (OCI foreign layers).
    layer_urls Array(Array(String)),
    layer_history_created Array(Nullable(DateTime64(3, 'UTC'))),
    layer_history_created_by Array(String) CODEC(ZSTD(3)),
    layer_history_author Array(String),
    layer_history_comment Array(String) CODEC(ZSTD(3)),
    layer_history_empty_layer Array(UInt8),

    -- The wire sizes are signed. A negative one, cast blindly, wraps to ~18
    -- exabytes and poisons every SUM, so size_bytes and layer_bytes are
    -- clamped to 0 and these flags rise instead; layer_size_negative as soon
    -- as ANY layer was negative. layer_sizes keeps the raw signed values.
    size_negative UInt8,
    layer_size_negative UInt8
)
ENGINE = ReplacingMergeTree(collected_at)
PARTITION BY toYYYYMM(collected_at)
ORDER BY (tenant_id, image_key, host)
TTL toDateTime(collected_at) + INTERVAL 30 DAY;

-- Raw payloads: what a handler could not decode, kept instead of dropped.
--
-- The request that would show what an undocumented format looks like
-- arrives once, from a real agent, in someone else's cluster. A handler that
-- cannot decode a body, or gets a shape with no published schema, puts the
-- bytes here. A request that decodes never does: a fallback, not a mirror
-- of the traffic.

CREATE TABLE IF NOT EXISTS raw_payloads
(
    tenant_id        LowCardinality(String),

    -- Arrival time, not a timestamp from the payload: the payload is by
    -- definition something we could not read.
    received_at      DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The router's own label ("dbm", "trace", "rum", ...). In the sort key
    -- because the question asked here is always "what is <intake> sending?".
    intake           LowCardinality(String),

    -- "no_schema" (this intake is not decoded yet), "decode_error" (decoding
    -- failed), "unexpected_shape" (it decoded into something the converter
    -- does not handle). A bounded vocabulary on purpose; free text goes in note.
    reason           LowCardinality(String),

    method           LowCardinality(String),

    -- Routing is by host, so the host is what says which product sent it.
    -- Not LowCardinality: per-agent hosts like 7-58-2-app.agent.<site> make
    -- it unbounded.
    host             String,
    path             String,

    -- Array values because a parameter may legally repeat.
    query            Map(String, Array(String)) CODEC(ZSTD(3)),

    content_type     LowCardinality(String),

    -- What the sender used, not what body is: body is decompressed before any
    -- handler sees it. Kept because the codec fingerprints the subsystem (zstd
    -- for metrics, gzip for traces, deflate for distribution points) and a
    -- mislabelled encoding is itself a decode failure worth seeing.
    content_encoding LowCardinality(String),

    -- An allowlist of request headers that identify and correlate the
    -- sender. Not every header: those include Dd-Api-Key and Authorization,
    -- and a debugging table is the last place a credential should be durable.
    headers          Map(String, String) CODEC(ZSTD(3)),

    -- Nothing filters on it; it is read whole when reverse-engineering a format.
    body             String CODEC(ZSTD(3)),
    body_bytes       UInt64 CODEC(T64, ZSTD(1)),

    -- The decoder error, the missing field: whatever the reader will want.
    note             String
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, intake, received_at)
-- A debugging sample, not a record: long enough to cover "it started
-- failing sometime last month", short enough that an intake nobody has got
-- round to cannot fill a disk.
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- APM and Data Streams: the tables behind trace.agent.<site>.
--
-- spans is a SAMPLE and apm_stats is COMPLETE: the agent feeds every trace to
-- its concentrator before its samplers drop any, so a hit count in apm_stats
-- is right even when 99% of the matching spans were dropped. That is why they
-- are two tables rather than one with a rollup on top.


-- spans: one row per span, from both wire formats in pb.AgentPayload.
--
-- v0.4 carries strings inline; v1.0 ("idx") carries one string table per
-- tracer payload. The two disagree on real things (a v0.4 span has no kind,
-- no per-span env/version/component and no typed attributes), so a zero in
-- those columns means "this format has no such field" for one row and "the
-- tracer did not set it" for another, and only wire_format tells them apart.
--
-- Agent, tracer and chunk fields are copied onto every span: a normalized
-- layout would need two joins to answer "which agent version shipped this
-- span", the question asked when something looks wrong. The copies are
-- LowCardinality, so they cost dictionary ids, not strings.
CREATE TABLE IF NOT EXISTS spans
(
    tenant_id          LowCardinality(String),

    -- Our clock; the span's own is `start`. The two disagreeing is itself a
    -- finding (a skewed container clock puts spans in tomorrow's partition).
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- 'v04' or 'idx'.
    wire_format        LowCardinality(String),

    -- The sampler settings the agent ran with are the difference between "the
    -- service stopped serving" and "the sampler tightened".
    agent_hostname     LowCardinality(String),
    agent_env          LowCardinality(String),
    agent_version      LowCardinality(String),
    target_tps         Float64 CODEC(Gorilla, ZSTD(1)),
    error_tps          Float64 CODEC(Gorilla, ZSTD(1)),
    rare_sampler_enabled UInt8,

    -- A protobuf map<string,string>, not Datadog's flat "key:value" list, so a
    -- key cannot repeat and a plain Map is enough. Same for tracer_tags and
    -- chunk_tags.
    agent_tags         Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- container_id and runtime_id are a UUID per process, so no dictionary.
    container_id       String,
    language           LowCardinality(String),
    language_version   LowCardinality(String),
    tracer_version     LowCardinality(String),
    runtime_id         String,
    tracer_env         LowCardinality(String),
    tracer_hostname    LowCardinality(String),
    app_version        LowCardinality(String),
    tracer_tags        Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- idx only: tracer_tags holds the scalar rendering, this the typed form
    -- (bytes, arrays, nested lists) that a scalar would flatten away.
    tracer_attributes_json String CODEC(ZSTD(3)),

    -- Nullable because the agent sets ContainerDebug only when resolving the
    -- container tags gave it trouble; a zero latency would read as "instant".
    container_debug_error        Nullable(String),
    container_debug_latency_ms   Nullable(Int64),
    container_debug_was_buffered Nullable(UInt8),
    container_debug_buffer_ms    Nullable(Int64),
    container_debug_buffer_eviction_reason Nullable(String),

    -- -1 user_drop / 0 auto_drop / 1 auto_keep / 2 user_keep, kept as the
    -- number so a value we do not know yet still arrives.
    priority           Int32 CODEC(T64, ZSTD(1)),
    origin             LowCardinality(String),
    dropped_trace      UInt8,
    -- idx only; v0.4 carries it in meta['_dd.p.dm'].
    sampling_mechanism UInt32 CODEC(T64, ZSTD(1)),
    chunk_tags         Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    chunk_attributes_json String CODEC(ZSTD(3)),

    service            LowCardinality(String),
    name               LowCardinality(String),
    -- A URL, a SQL statement or a route: unbounded and often long.
    resource           String CODEC(ZSTD(3)),
    -- `type` is a ClickHouse function name.
    span_type          LowCardinality(String),

    -- v0.4 puts the low 64 bits on the span and the high 64 in
    -- meta['_dd.p.tid'] as hex; idx puts all 16 bytes on the chunk. Both land
    -- as this pair, so a query need not know the format, and never through a
    -- float.
    trace_id           UInt64 CODEC(T64, ZSTD(1)),
    trace_id_high      UInt64 CODEC(T64, ZSTD(1)),
    -- The 32-digit form people paste from a Datadog URL. leftPad because hex()
    -- on an integer drops leading zero bytes, and an unpadded id matches nothing.
    trace_id_hex       String MATERIALIZED lower(concat(
                           leftPad(hex(trace_id_high), 16, '0'),
                           leftPad(hex(trace_id), 16, '0'))),

    span_id            UInt64 CODEC(T64, ZSTD(1)),
    -- 0 means root. The wire has no has-parent bit, so NULL would claim a
    -- distinction the tracer never made.
    parent_id          UInt64 CODEC(T64, ZSTD(1)),

    -- Nanoseconds, because a span can be shorter than a millisecond.
    start              DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    -- Signed as in v0.4: a negative duration from a broken clock is a bug to
    -- SELECT, not one to wrap.
    duration_ns        Int64 CODEC(T64, ZSTD(1)),
    -- v0.4 sends an int32 flag, idx a bool; Int32 keeps whatever v0.4 sent.
    error              Int32 CODEC(T64, ZSTD(1)),

    -- The lowercase OTel name, from idx's SpanKind or v0.4's meta['span.kind'].
    kind               LowCardinality(String),
    -- idx only: per-span overrides of the payload-level values.
    span_env           LowCardinality(String),
    span_version       LowCardinality(String),
    component          LowCardinality(String),

    -- The v0.4 shape, and where idx scalars are projected so one query works
    -- across both formats.
    meta               Map(String, String) CODEC(ZSTD(3)),
    metrics            Map(String, Float64) CODEC(ZSTD(3)),
    -- msgpack blobs (AppSec events, process tags) with no published per-key
    -- type, kept as raw bytes; nothing here is validated UTF-8.
    meta_struct        Map(String, String) CODEC(ZSTD(3)),

    -- idx only: every attribute in its declared type. meta/metrics are the
    -- convenience; this is the record.
    attributes_json    String CODEC(ZSTD(3)),

    -- Parallel arrays rather than Nested, so the index is the wire order a
    -- link list needs. Every link adds one element to each link_* array.
    link_trace_id      Array(UInt64) CODEC(T64, ZSTD(1)),
    link_trace_id_high Array(UInt64) CODEC(T64, ZSTD(1)),
    link_span_id       Array(UInt64) CODEC(T64, ZSTD(1)),
    -- One JSON object per link, in the same tagged form as attributes_json, so
    -- a v0.4 string attribute and an idx typed one read the same way.
    link_attributes    Array(String) CODEC(ZSTD(3)),
    link_tracestate    Array(String) CODEC(ZSTD(3)),
    link_flags         Array(UInt32) CODEC(T64, ZSTD(1)),

    event_time         Array(DateTime64(9, 'UTC')) CODEC(ZSTD(1)),
    event_name         Array(String) CODEC(ZSTD(3)),
    event_attributes   Array(String) CODEC(ZSTD(3)),

    -- "Every span of this trace" is the most common trace query, and random
    -- ids spread a trace across every part. 0.001 rather than 0.01 because
    -- each row carries one high-cardinality value and a false positive costs
    -- a whole granule.
    INDEX idx_trace_id trace_id TYPE bloom_filter(0.001) GRANULARITY 1,
    INDEX idx_span_id  span_id  TYPE bloom_filter(0.001) GRANULARITY 1
)
ENGINE = MergeTree
PARTITION BY toDate(start)
-- "One service over a time range" (every panel) and "every span of one trace"
-- pull in opposite directions; a key led by trace_id would lose all time
-- locality. So the key serves the service query and compresses well, the bloom
-- filter serves the trace lookup, and trace_id at the end keeps one trace's
-- spans together once service and time have narrowed the range.
ORDER BY (tenant_id, service, name, start, trace_id)
-- Spans are the highest-volume table and already a sample; apm_stats keeps
-- the complete counts for longer, which is the right way round.
TTL toDateTime(start) + INTERVAL 14 DAY;


-- apm_stats: one row per pb.ClientGroupedStats, the numbers APM pages are
-- built from. The three outer levels of the payload (StatsPayload,
-- ClientStatsPayload, ClientStatsBucket) are copied onto the leaf, as on spans.
CREATE TABLE IF NOT EXISTS apm_stats
(
    tenant_id        LowCardinality(String),
    received_at      DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    agent_hostname   LowCardinality(String),
    agent_env        LowCardinality(String),
    agent_version    LowCardinality(String),
    -- The tracer computed the stats and the agent only forwarded them.
    client_computed  UInt8,
    -- One slice of a payload the agent split: re-aggregate before reading.
    split_payload    UInt8,

    -- The tracer, or the agent's aggregation key when the agent aggregated.
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

    -- The one genuine Datadog "key:value" tag list in the trace protocol, from
    -- orchestrator enrichment, so it gets the multiset shape a repeated key
    -- needs, and the bloom filter pair below.
    client_tags      Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    client_git_commit_sha    String,
    client_image_tag         String,
    client_process_tags      String CODEC(ZSTD(3)),
    client_process_tags_hash UInt64 CODEC(T64, ZSTD(1)),

    bucket_start        DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    bucket_duration_ns  UInt64 CODEC(T64, ZSTD(1)),
    -- How far the agent moved the bucket to line it up with its own window;
    -- usually 0.
    agent_time_shift_ns Int64 CODEC(T64, ZSTD(1)),

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
    -- pb.Trilean, not a bool: collapsing "not set" into "false" would turn
    -- every old tracer's spans into non-roots.
    is_trace_root    Enum8('not_set' = 0, 'true' = 1, 'false' = 2),
    grpc_status_code LowCardinality(String),
    http_method      LowCardinality(String),
    http_endpoint    String CODEC(ZSTD(3)),
    service_source   LowCardinality(String),
    -- Deprecated upstream for additional_metric_tags; tracers still send it.
    span_derived_primary_tags Array(String) CODEC(ZSTD(3)),
    additional_metric_tags    Array(String) CODEC(ZSTD(3)),

    -- Each summary is a DDSketch in protobuf bytes. A sketch merges and answers
    -- any quantile afterwards; a stored p50 answers one and does not combine.
    -- So the bytes are the record and the decoded columns a convenience.
    -- The state keeps apart 'absent' (no such spans), 'undecodable' (bytes that
    -- are not a DDSketch, a bug or a version skew), 'empty' and 'ok'.
    ok_summary       String CODEC(ZSTD(3)),
    ok_summary_state Enum8('absent' = 0, 'undecodable' = 1, 'empty' = 2, 'ok' = 3),
    -- Set only when the state is 'ok'; a zero would read as a measurement.
    ok_count         Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    ok_sum           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    ok_min           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    ok_max           Nullable(Float64) CODEC(Gorilla, ZSTD(1)),
    -- The positive-value store, the same shape as the sketches table.
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

    -- arrayFlatten because mapValues of an array-valued map is
    -- Array(Array(...)), which bloom_filter rejects.
    INDEX idx_client_tag_keys   mapKeys(client_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_client_tag_values arrayFlatten(mapValues(client_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(bucket_start)
-- Every APM question starts with a service and a time range, then narrows to
-- an operation and a resource.
ORDER BY (tenant_id, service, bucket_start, name, resource)
-- Longer than spans: small rows, and the only place the true traffic volume
-- survives once the sampler has thrown the spans away.
TTL toDateTime(bucket_start) + INTERVAL 30 DAY;


-- dsm_pipeline_stats: one row per Data Streams StatsPoint.
--
-- The trace-agent only proxies this route, adding Via and two X-Datadog-*-Tags
-- headers, so the schema is dd-trace-go's: msgpack of
-- datastreams.StatsPayload. A pathway is a chain of queue hops; `hash` is the
-- point and `parent_hash` the one before it, so the graph is a self-join;
-- edge_tags describe the hop that produced the point. Both hashes are 64-bit
-- and must never pass through a float.
CREATE TABLE IF NOT EXISTS dsm_pipeline_stats
(
    tenant_id      LowCardinality(String),
    received_at    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    env            LowCardinality(String),
    service        LowCardinality(String),
    tracer_version LowCardinality(String),
    lang           LowCardinality(String),
    version        LowCardinality(String),
    process_tags   Array(String) CODEC(ZSTD(3)),
    -- Bit 0 APM, bit 1 DSM; kept as the number so an unknown bit survives.
    product_mask   UInt64 CODEC(T64, ZSTD(1)),

    bucket_start       DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    bucket_duration_ns UInt64 CODEC(T64, ZSTD(1)),

    edge_tags      Array(String) CODEC(ZSTD(3)),
    hash           UInt64 CODEC(T64, ZSTD(1)),
    parent_hash    UInt64 CODEC(T64, ZSTD(1)),
    -- 'current' or 'origin': latency measured from this point's own timestamp
    -- or from the first point of the pathway. A string, not an Enum8, because
    -- the vocabulary grows in dd-trace-go, not here.
    timestamp_type LowCardinality(String),

    -- Three DDSketches, raw plus decoded as in apm_stats. The latencies are in
    -- SECONDS, and payload_size is a distribution of message sizes in bytes,
    -- not a single number.
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

    -- The proxy's headers carry container identity found nowhere in the body:
    -- the only link from a pathway to a workload.
    via              String,
    additional_tags  String CODEC(ZSTD(3)),
    container_tags   String CODEC(ZSTD(3)),
    content_encoding LowCardinality(String),

    -- dd-trace-go adds fields without version negotiation, so an unknown key
    -- is expected. Names to GROUP BY when a new one appears, values to see
    -- what it carries.
    unknown_keys Array(String),
    unknown_json String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(bucket_start)
-- hash last so the self-join that rebuilds the graph stays a range read.
ORDER BY (tenant_id, service, bucket_start, hash)
TTL toDateTime(bucket_start) + INTERVAL 30 DAY;


-- dsm_backlogs: one row per Backlog, the queue depth a consumer has not read.
-- Its own table because backlogs hang off the bucket, not a StatsPoint, and a
-- bucket can carry backlogs and no points.
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

    -- Which backlog this is ("type:produced", "topic:x"). An ordered array,
    -- because the tracer hashes the list in its order.
    tags  Array(String) CODEC(ZSTD(3)),
    value Int64 CODEC(T64, ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(bucket_start)
ORDER BY (tenant_id, service, bucket_start)
TTL toDateTime(bucket_start) + INTERVAL 30 DAY;


-- dsm_bucket_transactions: StatsBucket.Transactions and
-- .TransactionCheckpointIds, bucket-level like dsm_backlogs.
--
-- Hand-rolled binary: a record is [checkpointId u8][timestamp i64 BE][idLen u8]
-- [id], the checkpoint table [id u8][nameLen u8][name]. dd-trace-go says the
-- backend expects this layout exactly, so it is kept byte for byte and parsed
-- by the reader, not decoded into a shape a future version would break.
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


-- dsm_messages: /api/v2/data_streams_messages, one row per array element.
--
-- An Event Platform track that happens to live on the trace.agent host. Its
-- producer is not in the open agent repository, so no struct is invented: the
-- element is stored as the JSON it arrived as, and `keys` (its top-level key
-- names) answers "what does this track send?" with a GROUP BY.
CREATE TABLE IF NOT EXISTS dsm_messages
(
    tenant_id   LowCardinality(String),
    received_at DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Batch order is the only ordering these messages are known to have.
    position UInt32 CODEC(T64, ZSTD(1)),

    message String CODEC(ZSTD(3)),
    keys    Array(String) CODEC(ZSTD(3)),

    -- The Event Platform producer, the closest thing to a schema identifier.
    dd_evp_origin         LowCardinality(String),
    dd_evp_origin_version LowCardinality(String),
    content_encoding      LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
-- Arrival time is the only clock this table has.
ORDER BY (tenant_id, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- Database Monitoring: dbm-metrics-intake.<site>, six tracks (dbmmetrics,
-- dbmactivity, databasequery, dbmmetadata, dbmhealth, dbmcolumnstatistics),
-- one table.
--
-- One table, not one per engine: every engine (Oracle, Postgres, MySQL,
-- SQL Server, Mongo) reports its rows under a key of its own (oracle_rows,
-- postgres_activity, mysql_rows, ...) and nothing here knows the shape of
-- any of them. A table per engine would mean inventing a schema for data we
-- cannot decode. So the columns are the envelope fields every track agrees on, and
-- `event` keeps the whole event, from which an engine's rows can be decoded
-- later without a schema change.
CREATE TABLE IF NOT EXISTS dbm_events
(
    tenant_id            LowCardinality(String),

    received_at          DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    track                LowCardinality(String),

    -- Nullable: an event that sent no timestamp stays without one rather than
    -- becoming 1970-01-01 or received_at. Not DoubleDelta: events from many
    -- hosts and tracks interleave in arrival order, so consecutive rows are
    -- not a regular series.
    timestamp            Nullable(DateTime64(3, 'UTC')) CODEC(ZSTD(1)),

    -- The database the event is about, not the agent that forwarded it
    -- (that is agent_hostname/agent_version).
    host                 LowCardinality(String),
    database_instance    LowCardinality(String),

    agent_hostname       LowCardinality(String),
    agent_version        LowCardinality(String),

    source               LowCardinality(String),
    dbm_type             LowCardinality(String),  -- fqt | plan | activity
    kind                 LowCardinality(String),  -- lock_metrics, database_instance, ...
    dbms                 LowCardinality(String),
    dbms_version         LowCardinality(String),

    -- Nullable: a genuine 0s interval and no interval at all are different facts.
    collection_interval  Nullable(Float64),

    -- The multiset tag map (docs/decisions/0001-tags-are-a-multiset.md).
    tags                 Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- Sorted names of the keys with no column of their own: this is where the engine-specific
    -- row arrays and the databasequery "db" object are named, visible
    -- without a decoder.
    extra_keys           Array(String),

    -- Known keys whose value had an unexpected shape.
    undecoded_keys       Array(String),

    -- The whole event as it arrived: the only lossless copy of the
    -- engine-specific rows.
    event                String CODEC(ZSTD(3)),

    -- Pulled out of the "db" object of query samples and plans, because
    -- "which query is this" and "did it have a plan" are worth a column
    -- rather than a JSON extract. Nullable: only databasequery events carry
    -- a "db" object.
    db_instance           Nullable(String),
    query_signature       Nullable(String),
    statement             Nullable(String) CODEC(ZSTD(3)),  -- full SQL text, can be large
    plan_signature        Nullable(String),
    -- The number of steps in db.plan.definition. NULL both when no plan was
    -- sent and when the definition is not a step list; the second case also
    -- puts "db.plan.definition" in undecoded_keys.
    plan_definition_steps Nullable(UInt32),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    -- mapValues of an array-valued map is Array(Array(...)), which
    -- bloom_filter rejects.
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
-- "What did this database instance report on this track": track plays the
-- part metric plays in metrics, and database_instance follows host because
-- one host can run several instances.
ORDER BY (tenant_id, track, host, database_instance, received_at)
-- 30 days like metrics, not 90 like events: query samples and activity
-- snapshots are bulky, a row can hold dozens of activity rows and the
-- complete SQL text.
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- Network Device Monitoring: device, interface and IP inventory, the other
-- metadata objects, config backups, SNMP traps, flows, traceroutes.
--
-- The inventory, config and trap columns were written without Datadog's
-- types for them at hand, so their field names are a reconstruction of the
-- wire. Flows and network paths follow Datadog's own structs, so those
-- columns are exact.

-- Devices. A full inventory arrives every collection pass, never a delta, so
-- each pass is its own rows: daily partitions and a 30-day TTL rather than a
-- "current" table, and (namespace, device_id, collect_timestamp) answers
-- "what did we see, and when".
CREATE TABLE IF NOT EXISTS ndm_devices
(
    tenant_id         LowCardinality(String),

    -- Set once for the whole collection pass, not per device.
    namespace         LowCardinality(String),
    subnet            String,
    integration       LowCardinality(String),
    collect_timestamp DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Top-level payload keys not read into a column, raw JSON text per key,
    -- so nothing is lost before a column exists for them.
    extra             Map(String, String) CODEC(ZSTD(3)),

    device_id         String,
    id_tags           Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    tags              Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    ip_address        String,

    -- An int32 on the wire (1 reachable, 2 unreachable); anything else,
    -- absent included, is unknown.
    status            Enum8('unknown' = 0, 'reachable' = 1, 'unreachable' = 2),
    ping_status       Enum8('unknown' = 0, 'reachable' = 1, 'unreachable' = 2),

    name               String,
    description        String CODEC(ZSTD(3)),
    sys_object_id      String,
    location           String,
    profile            LowCardinality(String),
    profile_version    UInt64 CODEC(T64, ZSTD(1)),
    vendor             LowCardinality(String),

    -- A device carries its own subnet and integration beside the pass's, and
    -- they can differ within one pass, so both are kept.
    device_subnet      String,
    serial_number      String,
    version            String,
    product_name       String,
    model              String,
    os_name            LowCardinality(String),
    os_version         String,
    os_hostname        String,
    device_integration LowCardinality(String),
    device_type        LowCardinality(String),

    INDEX idx_tag_keys      mapKeys(tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values    arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_id_tag_keys   mapKeys(id_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_id_tag_values arrayFlatten(mapValues(id_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(collect_timestamp)
ORDER BY (tenant_id, namespace, device_id, collect_timestamp)
TTL collect_timestamp + INTERVAL 30 DAY;

-- Interfaces: the same collection pass, one level deeper.
CREATE TABLE IF NOT EXISTS ndm_interfaces
(
    tenant_id         LowCardinality(String),
    namespace         LowCardinality(String),
    subnet            String,
    integration       LowCardinality(String),
    collect_timestamp DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    extra             Map(String, String) CODEC(ZSTD(3)),

    device_id         String,
    id_tags           Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- Not "index": that word starts an INDEX declaration in CREATE TABLE and
    -- fails to parse as a column.
    if_index          Int32 CODEC(T64, ZSTD(1)),
    raw_id            String,
    raw_id_type       LowCardinality(String),
    name              String,
    alias             String,
    description       String,
    mac_address       String,

    -- IF-MIB's ifAdminStatus/ifOperStatus and the IANA ifType, kept as the
    -- wire numbers: the value sets cannot be confirmed from source, so no
    -- labels are guessed.
    admin_status      UInt8 CODEC(T64, ZSTD(1)),
    oper_status       UInt8 CODEC(T64, ZSTD(1)),
    type              UInt32 CODEC(T64, ZSTD(1)),

    -- Optional booleans on the wire: "not reported" is not false.
    is_physical       Nullable(UInt8),
    meraki_enabled    Nullable(UInt8),
    meraki_status     LowCardinality(String),

    INDEX idx_id_tag_keys   mapKeys(id_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_id_tag_values arrayFlatten(mapValues(id_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(collect_timestamp)
ORDER BY (tenant_id, namespace, device_id, if_index, collect_timestamp)
TTL collect_timestamp + INTERVAL 30 DAY;

CREATE TABLE IF NOT EXISTS ndm_ip_addresses
(
    tenant_id         LowCardinality(String),
    namespace         LowCardinality(String),
    subnet            String,
    integration       LowCardinality(String),
    collect_timestamp DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    extra             Map(String, String) CODEC(ZSTD(3)),

    interface_id      String,
    ip_address        String,
    prefixlen         Int32 CODEC(T64, ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(collect_timestamp)
ORDER BY (tenant_id, namespace, interface_id, collect_timestamp)
TTL collect_timestamp + INTERVAL 30 DAY;

-- Everything else the inventory carries: links, VPN tunnels, NetFlow
-- exporters, diagnoses, device OIDs, scan status. Six small, varied shapes (a
-- link has no device at all) would be six tables of a few rows each, so the
-- element is kept whole in object, still queryable with JSONExtract, and only
-- the filters that matter are promoted; device_id and interface_id are empty
-- when the shape has none.
CREATE TABLE IF NOT EXISTS ndm_metadata_objects
(
    tenant_id         LowCardinality(String),
    namespace         LowCardinality(String),
    subnet            String,
    integration       LowCardinality(String),
    collect_timestamp DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    extra             Map(String, String) CODEC(ZSTD(3)),

    -- link / vpn_tunnel / netflow_exporter / diagnosis / device_oid / scan_status
    kind              LowCardinality(String),
    device_id         String,
    interface_id      String,
    object            String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(collect_timestamp)
ORDER BY (tenant_id, namespace, kind, device_id, collect_timestamp)
TTL collect_timestamp + INTERVAL 30 DAY;

-- Config backups: a whole running or startup config per row, bulky rather
-- than frequent. A week of them outweighs a month of the inventory, hence
-- the shorter TTL.
CREATE TABLE IF NOT EXISTS ndm_device_configs
(
    tenant_id         LowCardinality(String),
    namespace         LowCardinality(String),
    collect_timestamp DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Neither key is in Datadog's documented payload, but both arrive on
    -- every request, so they are columns rather than entries in extra.
    agent_hostname    LowCardinality(String),
    -- Raw JSON: its shape is not confirmed enough to justify columns.
    inventories       String CODEC(ZSTD(3)),
    extra             Map(String, String) CODEC(ZSTD(3)),

    device_id         String,
    device_ip         String,
    config_type       LowCardinality(String),
    config_source     LowCardinality(String),
    -- The agent fills in the collection time when a config has none, so this
    -- is never legitimately 0 and needs no Nullable.
    timestamp         DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    tags              Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- Read whole when someone needs it, never filtered on.
    content           String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, namespace, device_id, timestamp)
TTL timestamp + INTERVAL 7 DAY;

-- SNMP traps: a thing that happened, never replaced.
CREATE TABLE IF NOT EXISTS snmp_traps
(
    tenant_id       LowCardinality(String),
    timestamp       DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),

    ddsource        LowCardinality(String),
    ddtags          Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- Lifted out of ddtags (snmp_device:<ip>): "who sent this trap" is the
    -- first filter of nearly every query.
    device          LowCardinality(String),

    uptime          UInt32 CODEC(T64, ZSTD(1)),
    snmp_trap_oid   String,
    snmp_trap_name  LowCardinality(String),
    snmp_trap_mib   LowCardinality(String),

    -- SNMPv1 only, absent on v2/v3. An empty OID unambiguously means absent;
    -- for the two integers 0 would not, hence Nullable.
    enterprise_oid  String,
    generic_trap    Nullable(Int32),
    specific_trap   Nullable(Int32),

    -- Parallel arrays, one entry per variable binding. A value is the JSON
    -- text of whatever its SNMP type decoded to, since no one column type
    -- fits them all.
    var_oids        Array(String) CODEC(ZSTD(1)),
    var_types       Array(LowCardinality(String)) CODEC(ZSTD(1)),
    var_values      Array(String) CODEC(ZSTD(3)),

    -- Keys the OID resolver adds from MIBs and profiles, an open set: raw
    -- JSON text per key, like extra in the inventory tables.
    enriched        Map(String, String) CODEC(ZSTD(3)),

    -- The trap as received, for re-reading the wire if a field above was
    -- misparsed.
    raw             String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(ddtags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, device, timestamp)
TTL timestamp + INTERVAL 30 DAY;

-- NetFlow, sFlow and IPFIX. Every exporter reports on its own timer and this
-- can outgrow every other NDM table by far, hence daily partitions and two
-- weeks.
CREATE TABLE IF NOT EXISTS netflow_flows
(
    tenant_id         LowCardinality(String),
    -- Milliseconds, though undocumented: a payload recorded from the agent
    -- carries a 13-digit epoch. flow_start and flow_end are documented as
    -- seconds.
    flush_timestamp   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    flow_type         LowCardinality(String),
    sampling_rate     UInt64 CODEC(T64, ZSTD(1)),
    direction         LowCardinality(String),
    flow_start        DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    flow_end          DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    bytes             UInt64 CODEC(T64, ZSTD(1)),
    packets           UInt64 CODEC(T64, ZSTD(1)),
    ether_type        LowCardinality(String),
    ip_protocol       LowCardinality(String),
    tos               UInt32 CODEC(T64, ZSTD(1)),
    dscp              UInt32 CODEC(T64, ZSTD(1)),
    dscp_name         LowCardinality(String),

    device_namespace  LowCardinality(String),
    exporter_ip       LowCardinality(String),

    source_ip                     String,
    -- A string on the wire, and "*" for an ephemeral port, so never numeric.
    source_port                   LowCardinality(String),
    source_mac                    String,
    source_mask                   String,
    source_reverse_dns_hostname   String,

    destination_ip                    String,
    destination_port                  LowCardinality(String),
    destination_mac                   String,
    destination_mask                  String,
    destination_reverse_dns_hostname  String,

    ingress_interface_index UInt32 CODEC(T64, ZSTD(1)),
    egress_interface_index  UInt32 CODEC(T64, ZSTD(1)),

    host              LowCardinality(String),
    tcp_flags         Array(LowCardinality(String)),
    next_hop_ip       String,

    -- The agent's marshaller spreads these over the root object rather than
    -- under an "additional_fields" key; the intake gathers them back. Values
    -- are the JSON text as received, so a large counter never rounds through
    -- a float.
    additional_fields Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(flow_start)
ORDER BY (tenant_id, device_namespace, exporter_ip, flow_start)
TTL toDateTime(flow_start) + INTERVAL 14 DAY;

-- Traceroute results: scheduled test runs, configuration scale rather than
-- packet scale, so monthly partitions and 90 days, like check runs.
CREATE TABLE IF NOT EXISTS network_paths
(
    tenant_id         LowCardinality(String),
    -- Milliseconds, though undocumented, as with every Datadog "timestamp"
    -- not explicitly called seconds.
    timestamp         DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    agent_version     LowCardinality(String),
    namespace         LowCardinality(String),
    test_config_id    String,
    test_config_name  String,
    test_result_id    String,
    test_run_id       String,

    origin              LowCardinality(String),
    test_run_type       LowCardinality(String),
    test_config_source  LowCardinality(String),
    source_product      LowCardinality(String),
    collector_type      LowCardinality(String),
    protocol            LowCardinality(String),

    source_name          String,
    source_display_name  String,
    source_hostname      LowCardinality(String),
    -- NULL means the agent could not resolve a route, which is not the same
    -- as resolving to an empty string.
    source_via_subnet_alias            Nullable(String),
    source_via_interface_hardware_addr Nullable(String),
    source_network_id    String,
    source_service       LowCardinality(String),
    source_container_id  String,
    source_public_ip     String,

    destination_hostname LowCardinality(String),
    destination_port     UInt16 CODEC(T64, ZSTD(1)),
    destination_service  LowCardinality(String),

    hop_count_avg Float64 CODEC(Gorilla, ZSTD(1)),
    hop_count_min Int32 CODEC(T64, ZSTD(1)),
    hop_count_max Int32 CODEC(T64, ZSTD(1)),

    -- One test can traceroute several times: parallel arrays, one entry per
    -- run.
    run_ids                     Array(String),
    run_source_ips               Array(String),
    run_source_ports             Array(UInt16),
    run_destination_ips          Array(String),
    run_destination_ports        Array(UInt16),
    run_destination_reverse_dns  Array(Array(String)),

    -- Outer index the run, inner index the hop within it; a hop's reverse DNS
    -- is itself a list of names, hence a third level.
    hop_ttls        Array(Array(Int32)),
    hop_ips         Array(Array(String)),
    hop_reverse_dns Array(Array(Array(String))),
    hop_rtts        Array(Array(Float64)),
    hop_reachable   Array(Array(UInt8)),

    -- One end-to-end probe over the whole path, not one per run, so these are
    -- flat.
    e2e_rtts                   Array(Float64) CODEC(ZSTD(1)),
    e2e_packets_sent           Int32 CODEC(T64, ZSTD(1)),
    e2e_packets_received       Int32 CODEC(T64, ZSTD(1)),
    e2e_packet_loss_percentage Float32 CODEC(Gorilla, ZSTD(1)),
    e2e_jitter                 Float64 CODEC(Gorilla, ZSTD(1)),
    e2e_rtt_avg                Float64 CODEC(Gorilla, ZSTD(1)),
    e2e_rtt_min                Float64 CODEC(Gorilla, ZSTD(1)),
    e2e_rtt_max                Float64 CODEC(Gorilla, ZSTD(1)),

    tags Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (tenant_id, namespace, test_config_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 90 DAY;

-- Processes (process.<site>): the frame envelope, realtime stats, containers,
-- discovery and the system-probe connection table.
--
-- The shape follows the wire, not the product. process_snapshots holds one row
-- per frame, whatever its type; the per-item tables carry a snapshot_id back to
-- it. The envelope is denormalised onto the item tables anyway: every question
-- asked of them starts with a host and a time window, and a JOIN for "which VPC"
-- or "which agent version" would run on every one. The columns are
-- LowCardinality and repeat within a batch, so the dictionary swallows them.
--
-- Every proto enum is stored by name (ProcessState 'R', ContainerState
-- 'running', ProtocolType 'protocolHTTP'), never by number: the numbers belong
-- to a .proto we do not own, and a renumbering upstream would silently rewrite
-- history stored as ints.
--
-- Absent is not zero: where the wire distinguishes "not reported" from zero,
-- the column is Nullable. create_time is the exception: has_create_time = 0
-- beside it means the agent sent no start time and create_time is meaningless,
-- not 1970, so every read of create_time filters on has_create_time = 1.

-- The frame envelope has no natural home on the item tables: SystemInfo's CPU
-- inventory alone is nine parallel arrays. It also answers questions of its
-- own — did this host stop sending, which agent versions are in the fleet —
-- that need no item rows.
CREATE TABLE IF NOT EXISTS process_snapshots
(
    tenant_id             LowCardinality(String),
    -- Arrival time, because the header's own timestamp is left at zero by the
    -- agent and its unit is undocumented.
    received_at           DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    -- Also the payload_id of the connections/connections_payloads pair.
    snapshot_id           UUID,

    -- The type byte is the contract, the name is what a human reads.
    message_type          UInt8,
    message_type_name     LowCardinality(String),

    -- The type byte decides the body, not the path, so the two can disagree,
    -- and when they do that is worth seeing.
    path                  LowCardinality(String),

    -- The rest of the 16-byte header, raw. header_timestamp stays an Int64:
    -- turning a zero of unknown unit into a DateTime would invent a value.
    header_version        UInt8,
    header_encoding       UInt8,
    header_subscription_id UInt8,
    header_org_id         Int32,
    header_timestamp      Int64,
    frame_bytes           UInt64 CODEC(T64, ZSTD(1)),

    host_name             LowCardinality(String),
    -- The VPC id. Two hosts with the same private address are told apart only
    -- by this.
    network_id            String,

    -- A host too big for one frame is split into group_size chunks sharing a
    -- group_id; a snapshot is complete when all of them arrived.
    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),

    -- notSpecified / fargateECS / fargateEKS / sidecar.
    container_host_type   LowCardinality(String),

    host_id               Int64 CODEC(T64, ZSTD(1)),
    host_org_id           Int32 CODEC(T64, ZSTD(1)),
    host_display_name     String,
    host_all_tags         Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    host_num_cpus         Int32 CODEC(T64, ZSTD(1)),
    host_total_memory     Int64 CODEC(T64, ZSTD(1)),
    host_tag_index        Int32 CODEC(T64, ZSTD(1)),
    -- "Never recomputed" and "recomputed at the epoch" are different statements.
    host_tags_modified    Nullable(DateTime64(3, 'UTC')),

    -- The CPU inventory is parallel arrays indexed together: repeated is
    -- ordered, and two sockets legitimately report the same model string.
    sys_uuid              String,
    os_name               LowCardinality(String),
    os_platform           LowCardinality(String),
    os_family             LowCardinality(String),
    os_version            LowCardinality(String),
    kernel_version        LowCardinality(String),
    sys_total_memory      Int64 CODEC(T64, ZSTD(1)),
    cpu_numbers           Array(Int32),
    cpu_vendors           Array(LowCardinality(String)),
    cpu_families          Array(LowCardinality(String)),
    cpu_models            Array(String),
    cpu_physical_ids      Array(String),
    cpu_core_ids          Array(String),
    cpu_cores             Array(Int32),
    cpu_mhz               Array(Int64),
    cpu_cache_sizes       Array(Int32),

    -- "No hints" and "hint mask 0" differ, and only CollectorProc has the field.
    hint_mask             Nullable(Int32),

    -- The realtime messages carry host id, org, cpu count and memory directly
    -- instead of a Host; kept apart from host_* so a reader never has to wonder
    -- which message shape a value came from.
    rt_host_id            Int64 CODEC(T64, ZSTD(1)),
    rt_org_id             Int32 CODEC(T64, ZSTD(1)),
    rt_num_cpus           Int32 CODEC(T64, ZSTD(1)),
    rt_total_memory       Int64 CODEC(T64, ZSTD(1)),

    -- Lets a gap be spotted without counting rows in another table.
    process_count         UInt32 CODEC(T64, ZSTD(1)),
    process_stat_count    UInt32 CODEC(T64, ZSTD(1)),
    container_count       UInt32 CODEC(T64, ZSTD(1)),
    container_stat_count  UInt32 CODEC(T64, ZSTD(1)),
    discovery_count       UInt32 CODEC(T64, ZSTD(1)),
    connection_count      UInt32 CODEC(T64, ZSTD(1)),

    -- The only place the agent's own hostname and container count are kept;
    -- the item tables carry agent_version and request_id and join here.
    agent_hostname        LowCardinality(String),
    agent_version         LowCardinality(String),
    agent_container_count String,
    request_id            String,

    INDEX idx_tag_keys   mapKeys(host_all_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(host_all_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, host_name, received_at, message_type)
-- Four times the item tables: a row is tiny, and "when did that host stop
-- reporting" is asked long after its processes have aged out.
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- The 2-second stream, not merged into processes: a different cadence, shape
-- and volume, and a shared table would be mostly NULL whichever frame wrote
-- the row.
CREATE TABLE IF NOT EXISTS process_stats
(
    tenant_id             LowCardinality(String),
    timestamp             DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                  LowCardinality(String),
    snapshot_id           UUID,

    pid                   Int32 CODEC(T64, ZSTD(1)),
    key                   UInt32 CODEC(T64, ZSTD(1)),
    create_time           DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    -- A flag rather than a Nullable create_time, so that every process table
    -- reads the same way as processes.
    has_create_time       UInt8,

    -- Separate from cpu_nice and cpu_num_threads on the wire, and the agent
    -- does not always fill both.
    nice                  Int32 CODEC(T64, ZSTD(1)),
    threads               Int32 CODEC(T64, ZSTD(1)),
    open_fds              Int32 CODEC(T64, ZSTD(1)),

    mem_rss               UInt64 CODEC(T64, ZSTD(1)),
    mem_vms               UInt64 CODEC(T64, ZSTD(1)),
    mem_swap              UInt64 CODEC(T64, ZSTD(1)),
    mem_shared            UInt64 CODEC(T64, ZSTD(1)),
    mem_text              UInt64 CODEC(T64, ZSTD(1)),
    mem_lib               UInt64 CODEC(T64, ZSTD(1)),
    mem_data              UInt64 CODEC(T64, ZSTD(1)),
    mem_dirty             UInt64 CODEC(T64, ZSTD(1)),

    cpu_last_cpu          LowCardinality(String),
    cpu_total_pct         Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_user_pct          Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_system_pct        Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_num_threads       Int32 CODEC(T64, ZSTD(1)),
    cpu_nice              Int32 CODEC(T64, ZSTD(1)),
    cpu_user_time         Int64 CODEC(T64, ZSTD(1)),
    cpu_system_time       Int64 CODEC(T64, ZSTD(1)),
    -- Parallel arrays rather than a Map: the repeated field is ordered and may
    -- name a core twice.
    cpu_core_names        Array(LowCardinality(String)),
    cpu_core_pcts         Array(Float32) CODEC(ZSTD(1)),

    io_read_rate          Float32 CODEC(Gorilla, ZSTD(1)),
    io_write_rate         Float32 CODEC(Gorilla, ZSTD(1)),
    io_read_bytes_rate    Float32 CODEC(Gorilla, ZSTD(1)),
    io_write_bytes_rate   Float32 CODEC(Gorilla, ZSTD(1)),

    net_connection_rate   Float32 CODEC(Gorilla, ZSTD(1)),
    net_bytes_rate        Float32 CODEC(Gorilla, ZSTD(1)),

    voluntary_ctx_switches   UInt64 CODEC(T64, ZSTD(1)),
    involuntary_ctx_switches UInt64 CODEC(T64, ZSTD(1)),

    container_id          String CODEC(ZSTD(1)),
    container_state       LowCardinality(String),
    process_state         LowCardinality(String),

    -- Deprecated upstream, stored anyway: older agents send only these
    -- per-process copies of the container numbers.
    container_health         LowCardinality(String),
    container_rbps           Float32 CODEC(Gorilla, ZSTD(1)),
    container_wbps           Float32 CODEC(Gorilla, ZSTD(1)),
    container_key            UInt32 CODEC(T64, ZSTD(1)),
    container_net_rcvd_ps    Float32 CODEC(Gorilla, ZSTD(1)),
    container_net_sent_ps    Float32 CODEC(Gorilla, ZSTD(1)),
    container_net_rcvd_bps   Float32 CODEC(Gorilla, ZSTD(1)),
    container_net_sent_bps   Float32 CODEC(Gorilla, ZSTD(1)),

    -- Opaque bytes; ClickHouse String is a byte string, so they survive verbatim.
    byte_key              String CODEC(ZSTD(1)),
    container_byte_key    String CODEC(ZSTD(1)),

    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),
    container_host_type   LowCardinality(String),
    rt_host_id            Int64 CODEC(T64, ZSTD(1)),
    rt_org_id             Int32 CODEC(T64, ZSTD(1)),
    rt_num_cpus           Int32 CODEC(T64, ZSTD(1)),
    rt_total_memory       Int64 CODEC(T64, ZSTD(1)),
    agent_version         LowCardinality(String),
    request_id            String
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, pid)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

-- The same Container message arrives three ways: the inventory on
-- CollectorProc, the whole of CollectorContainer, and hanging off a single
-- Process. One table with a source column, because the fields are identical
-- and three tables would mean a UNION for every question.
CREATE TABLE IF NOT EXISTS containers
(
    tenant_id             LowCardinality(String),
    timestamp             DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                  LowCardinality(String),
    snapshot_id           UUID,

    -- collector_proc, collector_container or process; only the last is
    -- per-process and sets pid, which is zero otherwise.
    source                LowCardinality(String),
    pid                   Int32 CODEC(T64, ZSTD(1)),

    -- The runtime: docker, containerd, podman, ...
    type                  LowCardinality(String),
    id                    String CODEC(ZSTD(1)),
    -- Deprecated upstream, and still the only human-readable identity a
    -- container has in this message.
    name                  String,
    image                 String,
    repo_digest           String CODEC(ZSTD(1)),

    cpu_limit             Float32 CODEC(Gorilla, ZSTD(1)),
    memory_limit          UInt64 CODEC(T64, ZSTD(1)),
    cpu_request           Float32 CODEC(Gorilla, ZSTD(1)),
    memory_request        UInt64 CODEC(T64, ZSTD(1)),

    state                 LowCardinality(String),
    health                LowCardinality(String),

    -- Unix seconds on the wire. Nullable so a container whose runtime never
    -- reported the time does not appear to have started at the epoch.
    created               Nullable(DateTime64(3, 'UTC')),
    started               Nullable(DateTime64(3, 'UTC')),

    rbps                  Float32 CODEC(Gorilla, ZSTD(1)),
    wbps                  Float32 CODEC(Gorilla, ZSTD(1)),
    net_rcvd_ps           Float32 CODEC(Gorilla, ZSTD(1)),
    net_sent_ps           Float32 CODEC(Gorilla, ZSTD(1)),
    net_rcvd_bps          Float32 CODEC(Gorilla, ZSTD(1)),
    net_sent_bps          Float32 CODEC(Gorilla, ZSTD(1)),

    user_pct              Float32 CODEC(Gorilla, ZSTD(1)),
    system_pct            Float32 CODEC(Gorilla, ZSTD(1)),
    total_pct             Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_usage_ns          Float32 CODEC(Gorilla, ZSTD(1)),

    mem_rss               UInt64 CODEC(T64, ZSTD(1)),
    mem_cache             UInt64 CODEC(T64, ZSTD(1)),
    mem_usage             UInt64 CODEC(T64, ZSTD(1)),
    mem_accounted         UInt64 CODEC(T64, ZSTD(1)),

    thread_count          UInt64 CODEC(T64, ZSTD(1)),
    thread_limit          UInt64 CODEC(T64, ZSTD(1)),

    key                   UInt32 CODEC(T64, ZSTD(1)),
    byte_key              String CODEC(ZSTD(1)),

    -- ContainerAddr is repeated; parallel arrays keep the order and the
    -- triples together.
    addr_ips              Array(String),
    addr_ports            Array(Int32),
    addr_protocols        Array(LowCardinality(String)),

    tags                  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- Container.host is filled by the backend after resolution and is empty
    -- from a real agent: JSON is lossless and costs nothing when absent.
    host_info             String CODEC(ZSTD(3)),

    network_id            String,
    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),
    container_host_type   LowCardinality(String),
    agent_version         LowCardinality(String),
    request_id            String,

    INDEX idx_tag_keys   mapKeys(tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, id)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

-- ContainerStat arrives on CollectorRealTime and CollectorContainerRealTime
-- with identical fields, so again one table and a source column.
CREATE TABLE IF NOT EXISTS container_stats
(
    tenant_id             LowCardinality(String),
    timestamp             DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                  LowCardinality(String),
    snapshot_id           UUID,
    source                LowCardinality(String),

    id                    String CODEC(ZSTD(1)),

    user_pct              Float32 CODEC(Gorilla, ZSTD(1)),
    system_pct            Float32 CODEC(Gorilla, ZSTD(1)),
    total_pct             Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_limit             Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_request           Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_usage_ns          Float32 CODEC(Gorilla, ZSTD(1)),

    mem_rss               UInt64 CODEC(T64, ZSTD(1)),
    mem_cache             UInt64 CODEC(T64, ZSTD(1)),
    mem_limit             UInt64 CODEC(T64, ZSTD(1)),
    mem_usage             UInt64 CODEC(T64, ZSTD(1)),
    mem_accounted         UInt64 CODEC(T64, ZSTD(1)),
    memory_request        UInt64 CODEC(T64, ZSTD(1)),

    rbps                  Float32 CODEC(Gorilla, ZSTD(1)),
    wbps                  Float32 CODEC(Gorilla, ZSTD(1)),
    net_rcvd_ps           Float32 CODEC(Gorilla, ZSTD(1)),
    net_sent_ps           Float32 CODEC(Gorilla, ZSTD(1)),
    net_rcvd_bps          Float32 CODEC(Gorilla, ZSTD(1)),
    net_sent_bps          Float32 CODEC(Gorilla, ZSTD(1)),

    state                 LowCardinality(String),
    health                LowCardinality(String),

    key                   UInt32 CODEC(T64, ZSTD(1)),
    -- Unix seconds, Nullable for the same reason as containers.started.
    started               Nullable(DateTime64(3, 'UTC')),
    byte_key              String CODEC(ZSTD(1)),
    thread_count          UInt64 CODEC(T64, ZSTD(1)),
    thread_limit          UInt64 CODEC(T64, ZSTD(1)),

    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),
    container_host_type   LowCardinality(String),
    rt_host_id            Int64 CODEC(T64, ZSTD(1)),
    rt_org_id             Int32 CODEC(T64, ZSTD(1)),
    rt_num_cpus           Int32 CODEC(T64, ZSTD(1)),
    rt_total_memory       Int64 CODEC(T64, ZSTD(1)),
    agent_version         LowCardinality(String),
    request_id            String
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, id)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

-- The lightweight check: pid, command and user, no resource usage. Clusters
-- that turn the full process check off still run this one, so for many hosts
-- it is the only process data that arrives — hence a table of its own rather
-- than mostly empty rows in processes.
CREATE TABLE IF NOT EXISTS process_discoveries
(
    tenant_id             LowCardinality(String),
    timestamp             DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                  LowCardinality(String),
    snapshot_id           UUID,

    pid                   Int32 CODEC(T64, ZSTD(1)),
    ns_pid                Int32 CODEC(T64, ZSTD(1)),
    create_time           DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    has_create_time       UInt8,
    byte_key              String CODEC(ZSTD(1)),

    comm                  LowCardinality(String),
    exe                   String CODEC(ZSTD(1)),
    -- cmdline joins argv on spaces and cannot be split back; args is the
    -- fidelity copy.
    cmdline               String CODEC(ZSTD(3)),
    args                  Array(String) CODEC(ZSTD(3)),
    cwd                   String CODEC(ZSTD(3)),
    root                  String CODEC(ZSTD(3)),
    on_disk               UInt8,
    ppid                  Int32 CODEC(T64, ZSTD(1)),
    pgroup                Int32 CODEC(T64, ZSTD(1)),

    user                  LowCardinality(String),
    uid                   Int32 CODEC(T64, ZSTD(1)),
    gid                   Int32 CODEC(T64, ZSTD(1)),
    euid                  Int32 CODEC(T64, ZSTD(1)),
    egid                  Int32 CODEC(T64, ZSTD(1)),
    suid                  Int32 CODEC(T64, ZSTD(1)),
    sgid                  Int32 CODEC(T64, ZSTD(1)),

    host_info             String CODEC(ZSTD(3)),

    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),
    agent_version         LowCardinality(String),
    request_id            String
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, pid)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

-- The forty-odd fields of CollectorConnections that describe the sender and
-- its eBPF machinery rather than any one connection. A host sends thousands of
-- connections per frame; repeating these on each would multiply a kilobyte by
-- a thousand. payload_id is the frame's process_snapshots.snapshot_id.
CREATE TABLE IF NOT EXISTS connections_payloads
(
    tenant_id             LowCardinality(String),
    received_at           DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    payload_id            UUID,

    host_name             LowCardinality(String),
    network_id            String,
    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),
    container_host_type   LowCardinality(String),
    connection_count      UInt32 CODEC(T64, ZSTD(1)),

    -- How "eBPF is broken on the 5.4 kernels" is told from "eBPF is broken".
    architecture          LowCardinality(String),
    kernel_version        LowCardinality(String),
    platform              LowCardinality(String),
    platform_version      LowCardinality(String),

    -- A map of messages, and empty from a real agent (the backend fills it
    -- after resolution), so JSON.
    resolved_resources    String CODEC(ZSTD(3)),

    -- pid -> container id. A protobuf map cannot repeat a key, so a plain Map.
    container_for_pid     Map(Int32, String) CODEC(ZSTD(3)),

    -- The packed tag buffers, verbatim. The connection rows carry their tags
    -- already resolved, but the encoding is versioned, and one we cannot read
    -- yet must still land somewhere.
    encoded_tags              String CODEC(ZSTD(3)),
    encoded_connections_tags  String CODEC(ZSTD(3)),

    host_tags_index       Int32 CODEC(T64, ZSTD(1)),
    host_tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- The fixed telemetry message of pre-7.35 agents flattened to its field
    -- names, and the open-ended map newer agents send instead. Maps, so a counter added
    -- upstream lands here named instead of being dropped.
    conn_telemetry        Map(LowCardinality(String), Int64) CODEC(ZSTD(3)),
    conn_telemetry_map    Map(LowCardinality(String), Int64) CODEC(ZSTD(3)),

    -- Per-asset runtime-compilation results, a message with two enums: JSON.
    compilation_telemetry String CODEC(ZSTD(3)),
    kernel_header_fetch_result LowCardinality(String),
    -- COREResult by name: SuccessCustomBTF, BtfNotFound, VerifierError, ...
    core_telemetry        Map(String, LowCardinality(String)) CODEC(ZSTD(3)),
    prebuilt_ebpf_assets  Array(String),

    -- Parallel lists indexed by connections.route_idx; JSON keeps both the
    -- order and the nested shape.
    routes                String CODEC(ZSTD(3)),
    route_metadata        String CODEC(ZSTD(3)),

    -- Feature flags as JSON, so a new flag needs no schema change.
    agent_configuration   String CODEC(ZSTD(3)),

    -- Every DNS encoding as sent, plus the names decoded out of whichever
    -- arrived, so the common case needs no decoder at read time.
    encoded_dns             String CODEC(ZSTD(3)),
    domains                 Array(String) CODEC(ZSTD(3)),
    encoded_domain_database String CODEC(ZSTD(3)),
    encoded_dns_lookups     String CODEC(ZSTD(3)),
    dns_names               Array(String) CODEC(ZSTD(3)),

    -- Backend-resolution maps of messages, JSON like resolved_resources.
    resolved_hosts_by_name String CODEC(ZSTD(3)),
    resolved_public_ips    String CODEC(ZSTD(3)),

    ecs_task              String,
    -- The /etc/resolv.conf variants seen on the host, indexed by
    -- connections.resolv_conf_idx.
    resolv_confs          Array(String) CODEC(ZSTD(3)),

    agent_hostname        LowCardinality(String),
    agent_version         LowCardinality(String),
    agent_container_count String,
    request_id            String,

    INDEX idx_tag_keys   mapKeys(host_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(host_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, host_name, received_at)
TTL toDateTime(received_at) + INTERVAL 7 DAY;

-- The highest-volume table of the process intake: a busy host reports
-- thousands of connections every 30 seconds, so everything that repeats per
-- frame lives in connections_payloads.
CREATE TABLE IF NOT EXISTS connections
(
    tenant_id             LowCardinality(String),
    timestamp             DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                  LowCardinality(String),
    payload_id            UUID,

    pid                   Int32 CODEC(T64, ZSTD(1)),

    -- Both ends expanded, because "who talked to this ip" must be a column
    -- filter, not a JSON extract.
    laddr_ip              String,
    laddr_port            Int32 CODEC(T64, ZSTD(1)),
    laddr_container_id    String CODEC(ZSTD(1)),
    laddr_host_name       String,
    raddr_ip              String,
    raddr_port            Int32 CODEC(T64, ZSTD(1)),
    raddr_container_id    String CODEC(ZSTD(1)),
    raddr_host_name       String,

    family                LowCardinality(String),
    type                  LowCardinality(String),
    direction             LowCardinality(String),
    is_local_port_ephemeral LowCardinality(String),

    last_bytes_sent       UInt64 CODEC(T64, ZSTD(1)),
    last_bytes_received   UInt64 CODEC(T64, ZSTD(1)),
    last_packets_sent     UInt64 CODEC(T64, ZSTD(1)),
    last_packets_received UInt64 CODEC(T64, ZSTD(1)),
    last_retransmits      UInt32 CODEC(T64, ZSTD(1)),

    -- The order is the stack: [protocolTLS, protocolHTTP2] is not the reverse.
    protocol_stack        Array(LowCardinality(String)),

    net_ns                UInt32 CODEC(T64, ZSTD(1)),
    remote_network_id     String,

    -- The conntrack NAT entry, absent for most connections. Nullable, because
    -- 0.0.0.0:0 would read as a real translation.
    ip_translation_repl_src_ip   Nullable(String),
    ip_translation_repl_dst_ip   Nullable(String),
    ip_translation_repl_src_port Nullable(Int32),
    ip_translation_repl_dst_port Nullable(Int32),

    rtt                   UInt32 CODEC(T64, ZSTD(1)),
    rtt_var               UInt32 CODEC(T64, ZSTD(1)),
    intra_host            UInt8,

    dns_successful_responses UInt32 CODEC(T64, ZSTD(1)),
    dns_failed_responses     UInt32 CODEC(T64, ZSTD(1)),
    dns_timeouts             UInt32 CODEC(T64, ZSTD(1)),
    dns_success_latency_sum  UInt64 CODEC(T64, ZSTD(1)),
    dns_failure_latency_sum  UInt64 CODEC(T64, ZSTD(1)),
    dns_count_by_rcode       Map(UInt32, UInt32) CODEC(ZSTD(3)),

    -- The deprecated flat map older agents still send, and the two keyed by
    -- query type (by domain index and by domain offset, different keyspaces
    -- into different DNS encodings). Nested maps of messages, so JSON.
    dns_stats_by_domain                     String CODEC(ZSTD(3)),
    dns_stats_by_domain_by_query_type       String CODEC(ZSTD(3)),
    dns_stats_by_domain_offset_by_query_type String CODEC(ZSTD(3)),

    last_tcp_established  UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_closed       UInt32 CODEC(T64, ZSTD(1)),

    -- Indices into connections_payloads.routes / .route_metadata / .resolv_confs.
    route_idx             Int32 CODEC(T64, ZSTD(1)),
    route_target_idx      Int32 CODEC(T64, ZSTD(1)),
    resolv_conf_idx       Int32 CODEC(T64, ZSTD(1)),

    -- The USM aggregations: separately serialised protobufs we do not decode,
    -- kept as bytes. They are the entire content of Universal Service
    -- Monitoring, and dropping them would lose it for good.
    http_aggregations         String CODEC(ZSTD(3)),
    http2_aggregations        String CODEC(ZSTD(3)),
    data_streams_aggregations String CODEC(ZSTD(3)),
    database_aggregations     String CODEC(ZSTD(3)),

    -- tags_indices is the deprecated form; tags_idx points into
    -- connections_payloads.encoded_connections_tags, and tags is what it
    -- resolves to.
    tags_indices          Array(UInt32),
    tags_idx              Int32 CODEC(T64, ZSTD(1)),
    tags_checksum         UInt32 CODEC(T64, ZSTD(1)),
    tags                  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    local_container_tags_index Int32 CODEC(T64, ZSTD(1)),
    local_container_tags       Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    remote_service_tags_idx    Int32 CODEC(T64, ZSTD(1)),
    remote_service_tags        Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    state_index           UInt32 CODEC(T64, ZSTD(1)),
    -- errno -> count for failed TCP handshakes.
    tcp_failures_by_err_code Map(UInt32, UInt32) CODEC(ZSTD(3)),
    remote_ecs_task       String,
    system_probe_conn     UInt8,

    last_tcp_rto_count      UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_recovery_count UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_reord_seen     UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_rcv_ooo_pack   UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_delivered_ce   UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_probe0_count   UInt32 CODEC(T64, ZSTD(1)),
    tcp_ecn_negotiated      UInt8,

    agent_version         LowCardinality(String),
    request_id            String,

    INDEX idx_tag_keys   mapKeys(tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
-- The second question after "what did this host talk to" is always which process.
ORDER BY (tenant_id, host, timestamp, pid)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

-- Security: CWS activity dumps, runtime and compliance events, SBOMs.
-- Security data is what incident response looks back on, so it is kept 30
-- days rather than logs' 14.

-- One row per secdump request: the "what is this dump" summary, read far more
-- often than the tree is walked, so the tree (often thousands of nodes) lives
-- in cws_dump_nodes. The request has two independent parts, the JSON "event"
-- header and the protobuf "dump", and either can arrive without the other.
CREATE TABLE IF NOT EXISTS cws_activity_dumps
(
    tenant_id           LowCardinality(String),

    -- Arrival time: the payload carries no wall-clock time for the dump.
    received_at         DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Generated per request; the wire carries no id.
    dump_id             UUID,

    header_host         LowCardinality(String),
    header_service      LowCardinality(String),
    header_source       LowCardinality(String),
    header_tags         Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- Raw JSON as received. WARNING(undocumented): an array of strings or of
    -- objects, which is not documented, so it is not decoded further.
    dns_names           String CODEC(ZSTD(3)),
    -- Undeclared header keys, raw JSON per key; not "key:value" tags, hence
    -- a plain Map.
    header_extra        Map(String, String) CODEC(ZSTD(3)),
    -- The "event" part's bytes even when they did not parse as an object.
    header_raw          String CODEC(ZSTD(3)),

    -- The agent builds the two parts independently, so these can disagree
    -- with the header's and both are kept.
    dump_host           LowCardinality(String),
    dump_service        LowCardinality(String),
    dump_source         LowCardinality(String),
    dump_tags           Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    agent_version       LowCardinality(String),
    agent_commit        LowCardinality(String),
    kernel_version      LowCardinality(String),
    linux_distribution  LowCardinality(String),
    arch                LowCardinality(String),
    metadata_name       String,
    protobuf_version    LowCardinality(String),
    differentiate_args  UInt8,
    comm                LowCardinality(String),
    container_id        String,
    -- start_raw and end_raw are kernel- or boot-relative counters, not
    -- wall-clock time. A real 0 is possible, so NULL alone means the dump had
    -- no Metadata.
    start_raw           Nullable(UInt64) CODEC(ZSTD(1)),
    end_raw             Nullable(UInt64) CODEC(ZSTD(1)),
    size_raw            Nullable(UInt64) CODEC(ZSTD(1)),
    serialization       LowCardinality(String),
    cgroup_id           String,
    cgroup_manager      LowCardinality(String),

    -- Nodes at every depth: the size of the dump without the join.
    tree_node_count     UInt32 CODEC(T64, ZSTD(1)),

    -- The "dump" part's protobuf bytes as received. Field 7 ("mounts") is
    -- not in the published schema, and this is where it survives.
    dump                String CODEC(ZSTD(3)),

    INDEX idx_header_tag_keys   mapKeys(header_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_header_tag_values arrayFlatten(mapValues(header_tags)) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_dump_tag_keys     mapKeys(dump_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_dump_tag_values   arrayFlatten(mapValues(dump_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- One row per ProcessActivityNode, depth first. node_path, the index path from
-- the root ([2, 0, 1] = tree[2].Children[0].Children[1]), rebuilds the tree on
-- its own; parent_path and depth are kept because "this node's parent" and
-- "everything at depth 3" are worth not recomputing.
CREATE TABLE IF NOT EXISTS cws_dump_nodes
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    dump_id            UUID,

    node_path          Array(UInt32) CODEC(ZSTD(1)),
    depth              UInt16 CODEC(T64, ZSTD(1)),
    parent_path        Array(UInt32) CODEC(ZSTD(1)),

    -- The node as JSON with Children cleared, which would otherwise store every
    -- subtree once per ancestor. Everything else on the node, IMDS events with
    -- any AWS credentials the agent captured included, is here.
    node               String CODEC(ZSTD(3)),

    pid                UInt32 CODEC(T64, ZSTD(1)),
    ppid               UInt32 CODEC(T64, ZSTD(1)),
    comm               LowCardinality(String),
    container_id       String,
    file_path          String,
    args               Array(String) CODEC(ZSTD(3)),
    image_tags         Array(String) CODEC(ZSTD(3)),
    matched_rule_ids   Array(String) CODEC(ZSTD(3)),
    generation_type    LowCardinality(String),

    files_count        UInt32 CODEC(T64, ZSTD(1)),
    dns_count          UInt32 CODEC(T64, ZSTD(1)),
    sockets_count      UInt32 CODEC(T64, ZSTD(1)),
    syscalls_count     UInt32 CODEC(T64, ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
-- Every node of one dump in tree order is the query this table exists for.
ORDER BY (tenant_id, dump_id, node_path)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- secruntime, secinfo and compliance share one logs-pipeline envelope. Neither
-- it nor the inner event has a published schema, so known envelope fields and a
-- few hot inner paths get columns and the rest is kept as JSON text.
CREATE TABLE IF NOT EXISTS security_events
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    track              LowCardinality(String), -- secruntime | secinfo | compliance

    -- Position in the JSON array: the only ordering the batch carries.
    seq_in_batch       UInt32 CODEC(T64, ZSTD(1)),

    -- Millisecond epoch or RFC3339, both seen on the wire. NULL when absent
    -- or unparseable, never received_at.
    timestamp          Nullable(DateTime64(3, 'UTC')),
    hostname           LowCardinality(String),
    service            LowCardinality(String),
    ddsource           LowCardinality(String),
    status             LowCardinality(String),
    ddtags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- Envelope keys without a column, raw JSON per key.
    extra              Map(String, String) CODEC(ZSTD(3)),

    -- "message" as it arrived, usually a string of escaped JSON: the lossless
    -- copy. message is the unwrapped object, and message_decoded tells a
    -- failed unwrap from one that legitimately gave "{}".
    message_raw        String CODEC(ZSTD(3)),
    message            String CODEC(ZSTD(3)),
    message_decoded     UInt8,

    -- agent.rule_id, agent.policy_name, evt.name, evt.category, title and
    -- kind of the inner event, empty when absent; none is guaranteed on any
    -- track, and message stays the source of truth.
    rule_id             LowCardinality(String),
    policy_name         LowCardinality(String),
    evt_name            LowCardinality(String),
    evt_category        LowCardinality(String),
    title               String,
    event_kind          LowCardinality(String),

    INDEX idx_tag_keys   mapKeys(ddtags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, track, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- One row per SBOMEntity. Heartbeat entities ("same as last time") are stored
-- like any other: collapsing them is a reader's decision.
CREATE TABLE IF NOT EXISTS sbom_entities
(
    tenant_id              LowCardinality(String),
    received_at            DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Generated per entity; the components and vulnerabilities join on this,
    -- because the sender's `id` is not unique across payloads.
    entity_id              UUID,

    payload_version        Int32 CODEC(T64, ZSTD(1)),
    host                    LowCardinality(String),
    -- Optional on the wire: NULL means unset, not "".
    source                  Nullable(String),
    dd_env                  Nullable(String),

    type                LowCardinality(String), -- SBOMSourceType enum name
    id                  String,                 -- the sender's own entity id
    generated_at        Nullable(DateTime64(3, 'UTC')),
    repo_tags           Array(String) CODEC(ZSTD(3)),
    repo_digests        Array(String) CODEC(ZSTD(3)),
    in_use              UInt8,
    generation_duration_ms Nullable(Int64),
    dd_tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    heartbeat           UInt8,
    hash                String,
    status              LowCardinality(String), -- SBOMStatus enum name
    kernel_version      LowCardinality(String),
    cpu_architecture    LowCardinality(String),

    -- Set when generation failed and there is no BOM; never both.
    error               String CODEC(ZSTD(3)),

    -- The whole CycloneDX BOM as JSON: services, dependencies and metadata
    -- have no columns of their own and live only here.
    bom                 String CODEC(ZSTD(3)),
    -- Base64 protobuf, only for a BOM that cannot be written as JSON.
    bom_raw             String CODEC(ZSTD(3)),

    component_count      UInt32 CODEC(T64, ZSTD(1)),
    vulnerability_count  UInt32 CODEC(T64, ZSTD(1)),

    INDEX idx_tag_keys   mapKeys(dd_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(dd_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, host, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- Every component of a BOM, sub-components included, linked by parent_bom_ref.
CREATE TABLE IF NOT EXISTS sbom_components
(
    tenant_id             LowCardinality(String),
    received_at           DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    entity_id             UUID,

    bom_ref               String,
    parent_bom_ref        String, -- empty for a top-level component
    depth                 UInt16 CODEC(T64, ZSTD(1)),

    type                  LowCardinality(String), -- Classification enum name
    name                  String,
    -- CycloneDX recommends "" for a component without a version, so "" is a
    -- value here. The Nullable fields below are optional on the wire, and
    -- unset must stay distinct from "".
    version               String,
    purl                  Nullable(String),
    cpe                   Nullable(String),
    `group`               Nullable(String),
    publisher             Nullable(String),
    author                Nullable(String),
    description           Nullable(String) CODEC(ZSTD(3)),
    scope                 LowCardinality(String),

    licenses              Array(String) CODEC(ZSTD(3)),
    hashes                Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    -- A multiset: CycloneDX allows repeated property names and scanners emit
    -- them (docs/decisions/0001-tags-are-a-multiset.md).
    properties            Map(String, Array(String)) CODEC(ZSTD(3)),
    -- JSON arrays: open-ended nested structures nobody queries on their own.
    external_references   String CODEC(ZSTD(3)),
    evidence               String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, entity_id, bom_ref)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- Every vulnerability of a BOM: findings plus their analysis state.
CREATE TABLE IF NOT EXISTS sbom_vulnerabilities
(
    tenant_id                LowCardinality(String),
    received_at              DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    entity_id                UUID,

    bom_ref                  String,
    id                       String, -- e.g. a CVE id
    source_name              Nullable(String),
    source_url               Nullable(String),

    ratings                  String CODEC(ZSTD(3)), -- JSON array of VulnerabilityRating
    cwes                     Array(Int32) CODEC(ZSTD(1)),
    description              Nullable(String) CODEC(ZSTD(3)),
    detail                   Nullable(String) CODEC(ZSTD(3)),
    recommendation           Nullable(String) CODEC(ZSTD(3)),
    advisories               String CODEC(ZSTD(3)), -- JSON array of Advisory

    -- NULL when the source gave none, never epoch zero.
    created                  Nullable(DateTime64(3, 'UTC')),
    published                Nullable(DateTime64(3, 'UTC')),
    updated                  Nullable(DateTime64(3, 'UTC')),

    analysis_state          LowCardinality(String),
    analysis_justification  LowCardinality(String),
    analysis_response       Array(LowCardinality(String)),
    analysis_detail         String CODEC(ZSTD(3)),

    -- The affected components' bom_refs, for the join to sbom_components;
    -- affects keeps the rest, version ranges included.
    affects_refs            Array(String) CODEC(ZSTD(3)),
    affects                 String CODEC(ZSTD(3)), -- JSON array of VulnerabilityAffects
    -- A multiset, as in sbom_components.
    properties              Map(String, Array(String)) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, entity_id, bom_ref)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- EVP intakes: agent discovery, agent health, event management, software
-- inventory, synthetics, OpenLineage, query actions.
--
-- None of these share a wire shape, so each table follows its own payload.
-- Two choices repeat throughout:
--
--   - A timestamp the payload carries only as a string, in a format the
--     producer defines, gets two columns: the string as sent, and a
--     Nullable(DateTime64) that holds it parsed when it is RFC3339 and NULL
--     otherwise, never now(), which would invent a time the sender never
--     gave. received_at is the one time every row has, so PARTITION BY and
--     the TTL key off it.
--
--   - What is schemaless by design (a protobuf Struct, OpenLineage facets,
--     results of Python integrations, undeclared keys) stays JSON in a
--     String or Map, so a producer adding a field needs no schema change.
--
-- Parallel arrays are index-aligned: element i of each describes item i.

-- One row per AgentDiscoveryPayload. env_var_values is SENSITIVE: it keeps
-- the values (API keys, passwords an integration config embeds), not just
-- the names, because capturing them is what this endpoint is for; the
-- operator owns what reads this table. Arrays rather than a Map, because
-- EnvVars is a repeated field and a Map would collapse a name sent twice
-- and lose the order.
CREATE TABLE IF NOT EXISTS agent_discovery
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    host_id            LowCardinality(String),
    integration        LowCardinality(String),
    runtime            LowCardinality(String),
    runtime_id         String,
    ingestion_timestamp Nullable(DateTime64(3, 'UTC')),

    config_paths       Array(String),
    config_contents    Array(String) CODEC(ZSTD(3)),
    config_truncated   Array(UInt8),
    -- PayloadFormat by its enum name.
    config_formats     Array(LowCardinality(String)),

    env_var_names      Array(String),
    env_var_values     Array(String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, host_id, integration, runtime, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- One HealthReport becomes a report row and one issue row per entry of its
-- Issues map. The wire has no report-level id, so report_id is generated at
-- ingest and joins the two tables. Issue keys are sorted before the rows are
-- built, so two ingests of the same report yield the same rows in the same
-- order and can be compared.
CREATE TABLE IF NOT EXISTS agent_health_reports
(
    tenant_id         LowCardinality(String),
    received_at       DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    report_id         UUID,
    schema_version    LowCardinality(String),
    event_type        LowCardinality(String),
    emitted_at        String,
    emitted_at_parsed Nullable(DateTime64(3, 'UTC')),
    service           LowCardinality(String),
    host              LowCardinality(String),
    -- Optional on the wire: absent stays NULL, not "".
    agent_version     Nullable(String),
    par_ids           Array(String),
    issue_count       UInt32 CODEC(T64, ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, host, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

CREATE TABLE IF NOT EXISTS agent_health_issues
(
    tenant_id                LowCardinality(String),
    received_at              DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    report_id                UUID,
    -- The Issues map key, which is what the wire addresses an issue by;
    -- producers usually, not always, set it equal to Issue.Id.
    issue_key                String,
    id                       String,
    issue_name               LowCardinality(String),
    title                    String CODEC(ZSTD(1)),
    description              String CODEC(ZSTD(3)),
    category                 LowCardinality(String),
    location                 LowCardinality(String),
    severity                 LowCardinality(String),
    detected_at               String,
    detected_at_parsed        Nullable(DateTime64(3, 'UTC')),
    source                   LowCardinality(String),
    -- Issue.Extra, a protobuf Struct, schemaless per issue. Read from the
    -- request's JSON rather than the decoded Struct, which holds every number
    -- as a double and would round a large id.
    extra                    String CODEC(ZSTD(3)),

    remediation_summary      String CODEC(ZSTD(1)),
    remediation_step_order   Array(Int32),
    remediation_step_text    Array(String) CODEC(ZSTD(3)),
    script_language          LowCardinality(String),
    script_language_version  LowCardinality(String),
    script_filename          String,
    -- NULL means no script was reported, which is not requires_root=false.
    script_requires_root     Nullable(UInt8),
    script_content           String CODEC(ZSTD(3)),

    tags                     Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    persisted_state          LowCardinality(String),
    first_seen               String,
    last_seen                String,
    -- NULL means not resolved, which is not the same as "".
    resolved_at              Nullable(String),
    issue_type               LowCardinality(String),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, report_id, issue_key)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- One JSON:API-shaped envelope per request. outer_extra holds undeclared
-- keys from three levels: data.attributes as is, data under "data.", the
-- envelope under "top.", so a key at one level cannot shadow its namesake at
-- another.
CREATE TABLE IF NOT EXISTS event_management_events
(
    tenant_id           LowCardinality(String),
    received_at         DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    data_type           LowCardinality(String),
    host                LowCardinality(String),
    title               String CODEC(ZSTD(1)),
    category            LowCardinality(String),
    integration_id      LowCardinality(String),
    message             String CODEC(ZSTD(3)),
    timestamp           String,
    timestamp_parsed    Nullable(DateTime64(3, 'UTC')),

    tags                Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    aggregation_key     String CODEC(ZSTD(1)),
    -- From the hyphenated "system-notable-events".event_type key.
    notable_event_type  LowCardinality(String),
    -- The inner data.attributes.attributes object, kept apart from the outer
    -- columns: both levels use the name "attributes" and merging them would
    -- let one clobber the other.
    attributes          String CODEC(ZSTD(3)),
    outer_extra         Map(String, String) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_aggkey     aggregation_key TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- One row per software entry, not per payload: a host's inventory runs to
-- hundreds of entries, and "every host running curl < 8.0" should be a plain
-- WHERE. extra holds the entry's undeclared keys; payload_extra the
-- payload's, with those of the host_software object under "host_software.",
-- repeated onto every row of the payload.
CREATE TABLE IF NOT EXISTS host_software
(
    tenant_id                LowCardinality(String),
    received_at               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    hostname                 LowCardinality(String),
    software_type            LowCardinality(String),
    name                     String,
    version                  String,
    publisher                LowCardinality(String),
    deployment_status        LowCardinality(String),
    deployment_time          String,
    deployment_time_parsed   Nullable(DateTime64(3, 'UTC')),
    product_code             String,
    -- Unknown bitness is common; 0 would claim 32-bit.
    is_64_bit                Nullable(UInt8),
    install_paths            Array(String),
    extra                    Map(String, String) CODEC(ZSTD(3)),
    payload_extra            Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, hostname, name, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- One row per test result. Assertions are arrays rather than JSON because
-- "how many failed, and which" is the question this table answers; their
-- expected/actual stay JSON, as nothing says what type they are.
-- result.config has fields upstream leaves unenumerated, so it is kept
-- whole. extra holds undeclared keys of the top level and, under "result.",
-- of the result object.
CREATE TABLE IF NOT EXISTS synthetics_results
(
    tenant_id                        LowCardinality(String),
    received_at                      DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    test_id                          String,
    test_name                        String,
    test_type                        LowCardinality(String),
    test_subtype                     LowCardinality(String),
    test_version                     String,

    location_id                      LowCardinality(String),
    location_name                    LowCardinality(String),
    location_display_name            String,

    result_id                        String,
    result_initial_id                String,
    status                           LowCardinality(String),
    run_type                         LowCardinality(String),
    -- WARNING(undocumented): the wire type is unknown, so the value is kept
    -- as sent rather than guessed numeric.
    duration                         String,

    test_started_at                  String,
    test_started_at_parsed           Nullable(DateTime64(3, 'UTC')),
    test_finished_at                 String,
    test_finished_at_parsed          Nullable(DateTime64(3, 'UTC')),
    test_triggered_at                String,
    test_triggered_at_parsed         Nullable(DateTime64(3, 'UTC')),

    assertion_type                   Array(String),
    assertion_operator               Array(String),
    assertion_expected                Array(String) CODEC(ZSTD(3)),
    assertion_actual                 Array(String) CODEC(ZSTD(3)),
    assertion_valid                  Array(UInt8),

    -- A result may have no failure object at all, hence NULL rather than "".
    failure_code                     Nullable(String),
    failure_message                  Nullable(String),

    config                           String CODEC(ZSTD(3)),

    netstats_packets_sent            Nullable(Int64),
    netstats_packets_received        Nullable(Int64),
    netstats_packet_loss_percentage  Nullable(Float64),
    netstats_jitter                  Nullable(Float64),
    netstats_latency                 Nullable(Float64),
    netstats_hops                    Nullable(Int64),

    -- The shape the netpath table stores in columns; here it is only
    -- forwarded, so it stays JSON.
    netpath                          String CODEC(ZSTD(3)),
    dd                               String CODEC(ZSTD(3)),
    enrichment                       String CODEC(ZSTD(3)),
    v                                String,
    extra                            Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, test_id, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- One row per RunEvent. Facets are OpenLineage's extension mechanism, open
-- by spec, so they stay JSON. run_id ties a run's START/RUNNING/COMPLETE/
-- FAIL/ABORT events together, hence its place in ORDER BY.
CREATE TABLE IF NOT EXISTS openlineage_events
(
    tenant_id        LowCardinality(String),
    received_at      DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    event_type       LowCardinality(String),
    event_time       String,
    event_time_parsed Nullable(DateTime64(3, 'UTC')),
    producer         String,
    schema_url       String,

    run_id           String,
    run_facets       String CODEC(ZSTD(3)),
    job_namespace    LowCardinality(String),
    job_name         String,
    job_facets       String CODEC(ZSTD(3)),

    -- Inputs and outputs are two lists, each aligned within itself.
    input_namespace  Array(String),
    input_name       Array(String),
    input_facets     Array(String) CODEC(ZSTD(3)),
    output_namespace Array(String),
    output_name      Array(String),
    output_facets    Array(String) CODEC(ZSTD(3)),

    -- From the request, not the event: the ?api-version parameter and the
    -- Via header the trace-agent's proxy adds.
    api_version      String,
    via              String,
    extra            Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, run_id, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- WARNING(undocumented): the agent only schedules these checks; Python
-- integrations produce the results, and no field of them is named in the
-- agent's source. result is the entry as JSON and keys its top-level names,
-- so the shapes seen so far are one query away.
CREATE TABLE IF NOT EXISTS query_action_results
(
    tenant_id              LowCardinality(String),
    received_at            DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    result                 String CODEC(ZSTD(3)),
    keys                   Array(String),
    dd_evp_origin          LowCardinality(String),
    dd_evp_origin_version  LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- APM telemetry: the envelope tracers, the fleet installer and the agents send
-- to instrumentation-telemetry-intake.<site> (POST /api/v2/apmtelemetry).
--
-- Five producers share one envelope. application/host are the few fields
-- every producer is documented to send, so they are columns; payload, the
-- producer-specific part, is kept whole as JSON text, because nothing here
-- can name its fields with more authority than the JSON already does.
--
-- One row per request, plus one per entry of a "message-batch": the parent
-- row keeps the whole batch array as its payload, so what one request
-- contained is answerable from one row, and each child row carries that
-- entry's own request_type and payload.
CREATE TABLE IF NOT EXISTS apm_telemetry
(
    tenant_id    LowCardinality(String),

    -- Arrival time: tracer_time/event_time are producer-reported and only
    -- some producers send either, so this is the one timestamp every row has,
    -- and "everything in a time window" is the query this table exists for.
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- producer is derived because request_type alone is ambiguous for
    -- "logs", "traces" and "message-batch", which more than one producer sends;
    -- there the envelope decides (event_time means agent-telemetry, origin the
    -- fleet installer).
    request_type LowCardinality(String),
    producer     LowCardinality(String),  -- tracer / fleet-installer / agent-telemetry / trace-agent / cluster-agent / unknown
    api_version  LowCardinality(String),

    -- A per-process UUID, not a bounded vocabulary.
    runtime_id   String,

    -- Read from its digits, never through a double, which would round a
    -- sequence number past 2^53.
    seq_id            Nullable(Int64),

    -- Unix seconds with an optional fraction. Nullable because "not sent" and
    -- "sent as zero" are different producers' behaviour; the *_raw twin keeps
    -- the literal text, so a parse failure does not erase what arrived.
    tracer_time       Nullable(DateTime64(3, 'UTC')),
    tracer_time_raw   String,
    event_time        Nullable(DateTime64(3, 'UTC')),
    event_time_raw    String,

    service_name      LowCardinality(String),
    service_version   LowCardinality(String),
    env               LowCardinality(String),
    language_name     LowCardinality(String),
    language_version  LowCardinality(String),
    tracer_version    LowCardinality(String),

    -- host_extra keeps every other host key by name, its value as JSON text,
    -- since their shapes are declared nowhere. A plain Map like the other
    -- host metadata: this is not a Datadog tag set.
    hostname          LowCardinality(String),
    host_os           LowCardinality(String),
    host_architecture LowCardinality(String),
    host_extra        Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    payload String CODEC(ZSTD(3)),

    -- Sent only by some producers and with no fixed type worth a column of
    -- its own: debug keeps its JSON text, origin its string value.
    debug   String,
    origin  LowCardinality(String),

    -- Added by the trace-agent's proxy, never by the sender: the hop and the
    -- host it ran on, which the envelope has no field for.
    via                       LowCardinality(String),
    dd_agent_hostname         LowCardinality(String),
    dd_agent_env              LowCardinality(String),
    datadog_container_id      String,
    x_datadog_container_tags  String,

    -- Every other envelope key, its value as JSON text.
    extra Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- NULL, not 0, marks the request's own row: entry 0 of a batch must stay
    -- distinguishable from a row that is no batch entry at all.
    batch_index         Nullable(UInt32),
    parent_request_type LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, received_at)
-- 14 days, like logs: diagnostics about the tracers and agents themselves,
-- which nothing else depends on, and every instrumented process heartbeats on
-- its own schedule, so volume adds up fast.
TTL toDateTime(received_at) + INTERVAL 14 DAY;

-- Profiling and Dynamic Instrumentation: profiles (intake.profile.<site>),
-- debugger logs, diagnostics and symbol-database uploads
-- (debugger-intake.<site>), native symbol uploads (sourcemap-intake.<site>).
--
-- Every table partitions and sorts by received_at, which is always known. The
-- timestamps these payloads carry (a profile's start/end, a diagnostic's
-- timestamp) are informational and live in their own Nullable columns, so a
-- missing or malformed one never becomes 1970-01-01.

-- One row per multipart submission. Samples are not exploded into rows: a CPU
-- profile routinely carries tens of thousands, and pprof already reads the
-- bytes kept in attach_bytes. Each attachment's pprof envelope is pulled out
-- as parallel arrays so a query can filter by shape without re-parsing.
CREATE TABLE IF NOT EXISTS profiles
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The path the row came by: "profile", "profile-v1" (/v1/input) or
    -- "profile-browser" (browser-intake). A String, not an Enum8, so another
    -- producer needs no migration.
    variant                LowCardinality(String),
    dd_evp_origin          LowCardinality(String),
    dd_evp_origin_version  LowCardinality(String),

    -- The "event" part, byte for byte: the lossless copy. The columns from
    -- start_raw to tags_profiler are convenience extracts of it.
    event  String CODEC(ZSTD(3)),

    -- RFC 3339 from Datadog's profiler, epoch numbers from some other
    -- producers. *_raw keeps the wire text, so nothing is lost when *_parsed
    -- cannot make sense of it.
    start_raw     String,
    start_parsed  Nullable(DateTime64(3, 'UTC')),
    end_raw       String,
    end_parsed    Nullable(DateTime64(3, 'UTC')),

    family    LowCardinality(String),
    version   LowCardinality(String),
    runtime   LowCardinality(String),
    language  LowCardinality(String),

    -- tags_profiler arrives as one comma-joined string; split, it takes the
    -- multiset shape of every other Datadog tag column.
    tags_profiler  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- Pulled out of the tags so the sort key can serve "one service, time
    -- range" as on every other tagged table.
    service  LowCardinality(String) MATERIALIZED tags_profiler['service'][1],

    -- One entry per multipart part other than "event", ordered by part name,
    -- not wire order.
    attach_name             Array(String),
    -- As the part arrived, gzip included. Often pre-compressed already, so a
    -- heavier codec buys little.
    attach_bytes            Array(String) CODEC(ZSTD(1)),
    attach_size             Array(UInt64),
    -- 1 when the part parsed as pprof; when 0, every other attach_* at that
    -- index is zero-valued rather than NULL.
    attach_parsed           Array(UInt8),
    attach_sample_types     Array(Array(String)),
    attach_sample_units     Array(Array(String)),
    attach_sample_count     Array(UInt64),
    attach_time_nanos       Array(Int64),
    attach_duration_nanos   Array(Int64),
    attach_period_type      Array(String),
    attach_period           Array(Int64),
    attach_mapping_count    Array(UInt32),
    attach_location_count   Array(UInt32),
    attach_function_count   Array(UInt32),

    INDEX idx_tag_keys   mapKeys(tags_profiler)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags_profiler)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, service, received_at)
-- Bulky documents, like k8s_manifests: a week catches "a deploy regressed CPU
-- last week" without multi-megabyte rows filling a disk.
TTL toDateTime(received_at) + INTERVAL 7 DAY
SETTINGS index_granularity = 8192;

-- The "json" variant of /api/v2/debugger: Dynamic Instrumentation log and
-- snapshot entries, a JSON array or NDJSON.
CREATE TABLE IF NOT EXISTS debugger_logs
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    service   LowCardinality(String),
    ddsource  LowCardinality(String),

    -- From the ?ddtags= query string, so it is the same for the whole batch.
    ddtags  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- One array element or NDJSON line, byte for byte.
    entry  String CODEC(ZSTD(3)),

    -- Top-level keys other than service/ddsource: the upstream type is not
    -- published, so there is no schema to decode against.
    extra_keys  Array(String),

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, service, received_at)
-- Entries exist only while a probe is attached, so they trickle in like
-- check_runs and keep its 14 days.
TTL toDateTime(received_at) + INTERVAL 14 DAY
SETTINGS index_granularity = 8192;

-- The "diagnostics" variant of /api/v2/debugger: probe status messages,
-- decoded generically since the upstream type is unexported.
CREATE TABLE IF NOT EXISTS debugger_diagnostics
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    timestamp  Nullable(DateTime64(3, 'UTC')),

    service        LowCardinality(String),
    ddsource       LowCardinality(String),

    ddtags  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    runtime_id     String,
    probe_id       String,
    status         LowCardinality(String),
    probe_version  String,

    -- Nullable together, so "no exception" and "an exception with an empty
    -- message" stay apart.
    exception_type     Nullable(String),
    exception_message  Nullable(String),

    -- The array element, byte for byte.
    message  String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, service, received_at)
-- An audit trail of a few status changes per install, kept as long as events
-- and k8s_actions.
TTL toDateTime(received_at) + INTERVAL 90 DAY
SETTINGS index_granularity = 8192;

-- The "symdb" variant of /api/v2/debugger: a gzip'd JSON file part and an
-- event part. Both describe the same upload but come from different
-- serializers (uploadId in the event, upload_id in the file), so both are
-- kept, the file's under env_*, rather than merged into one guess.
CREATE TABLE IF NOT EXISTS symdb_uploads
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The event part, byte for byte; service to attachment_size are extracts
    -- of it.
    event  String CODEC(ZSTD(3)),

    ddtags  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    service      LowCardinality(String),
    version      LowCardinality(String),
    language     LowCardinality(String),
    runtime_id   String,

    -- Identifiers, not quantities, and nothing guarantees they stay numeric.
    upload_id  String,
    batch_num  String,

    -- Absent from the event is not the same as false or 0 bytes.
    final            Nullable(UInt8),
    attachment_size  Nullable(UInt64),

    -- The file part as received; the columns below are extracts of it.
    file           String CODEC(ZSTD(1)),
    inflated_size  UInt64,
    -- Top-level scopes only.
    scope_count    UInt32,
    -- 0 when "scopes" was missing or malformed; env_* are filled regardless.
    scopes_ok      UInt8,

    env_service     String,
    env_version     String,
    env_language    String,
    env_upload_id   String,
    env_batch_num   String,
    env_final       String,

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, service, received_at)
-- Bulky documents: file is a whole symbol-database batch.
TTL toDateTime(received_at) + INTERVAL 7 DAY
SETTINGS index_granularity = 8192;

-- /api/v2/srcmap: native symbol uploads from the host profiler.
CREATE TABLE IF NOT EXISTS symbol_uploads
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    type            LowCardinality(String),
    arch            LowCardinality(String),
    gnu_build_id    String,
    go_build_id     String,
    file_hash       String,
    symbol_source   LowCardinality(String),
    origin          LowCardinality(String),
    origin_version  String,
    filename        String,

    -- The "event" part, of which the columns above are extracts; its upstream
    -- type is unexported, so it is decoded generically.
    meta  String CODEC(ZSTD(3)),

    -- From the ELF ident bytes: "ELF32"/"ELF64"/"ELF?" and "LE"/"BE"/"?".
    -- Both "" when the elf_symbol_file part is missing (has_elf = 0) or is not
    -- ELF.
    has_elf         UInt8,
    elf_class       LowCardinality(String),
    elf_endianness  LowCardinality(String),
    -- Unparsed: only the ident bytes are read.
    elf       String CODEC(ZSTD(1)),
    elf_size  UInt64,

    -- Every other part, so nothing a future producer sends is thrown away.
    -- Not tags, so a plain Map.
    other_parts  Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
-- A symbol upload has no service. symbol_source is the closest bounded
-- "which producer" column; a build id is unique per build, the wrong shape
-- for a sort prefix.
ORDER BY (tenant_id, symbol_source, received_at)
-- Bulky documents: elf is a whole ELF symbol file.
TTL toDateTime(received_at) + INTERVAL 7 DAY
SETTINGS index_granularity = 8192;

-- RUM: what the browser, iOS and Android SDKs send on browser-intake.<site>.
--
-- One table per payload shape rather than per event kind: a table per kind
-- multiplies directories, merges and per-table overhead, while
-- LowCardinality(String) makes the kind a dictionary id. The split that is
-- made is the one the data forces (a view is upserted, everything else is
-- appended) plus the tracks that arrive on other endpoints (replay, spans) or
-- carry no user-facing event at all (telemetry).
--
-- Every table keeps the JSON exactly as it came off the wire (`event`, `span`).
-- The RUM schema has well over a hundred optional fields per variant and new
-- ones land with every SDK release: the typed columns are there so a query
-- can filter without parsing JSON, the raw column so nothing is lost,
-- including keys no column declares yet.
--
-- `date` is the sender's date, never arrival. iOS and Android buffer events
-- to disk and upload them up to 18 hours later (offline device, tracking
-- consent pending, a crash reported at the next launch), so an old `date` is
-- correct data. Arrival is kept in received_at so the lag is measurable;
-- partitioning and TTL run off `date` because that is what a query filters on.

-- A view lives as long as the user stays on the page, and the SDK re-sends the
-- same view.id with a growing _dd.document_version and updated counters. One
-- view.id is a series of updates to one logical row, so ReplacingMergeTree is
-- keyed on the version the SDK itself increments. `view_update` events merge
-- into the same row as `view` events: same identity, same counter, the newer
-- wire form of the same update. Apart, one view's history would split across
-- two rows whose counters disagree. Readers use FINAL (or argMax) for
-- exactness.
CREATE TABLE IF NOT EXISTS rum_views
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The request-level block, the same on every RUM table, and the only
    -- fields that identify the sender: a browser event has no hostname and no
    -- agent. ddsource is a query parameter on every SDK; the DD-EVP-ORIGIN
    -- pair names the SDK and its version.
    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),

    -- How the batch arrived compressed, as the sender spelled it
    -- (?dd-evp-encoding from the browser, Content-Encoding from mobile, ""
    -- for none). The payload columns hold decoded bytes, so this is the only
    -- trace of an SDK release that quietly stops compressing.
    evp_encoding       LowCardinality(String),

    -- DD-REQUEST-ID is fresh per attempt; DD-IDEMPOTENCY-KEY is sha1(body) and
    -- survives retries, so the pair tells a retry from a new batch.
    request_id         String,
    idempotency_key    String,

    -- Browser only: fetch or beacon. sendBeacon's failures are invisible to
    -- the page, fetch's are not, which decides what a missing batch means.
    dd_api             LowCardinality(String),

    -- batch_time is browser only; the retry pair is absent on a first
    -- attempt. Nullable because "sent once" and "retried zero times" are
    -- different facts. retry_after is stored as sent: a delay in ms from the
    -- browser, the HTTP status that caused the retry from mobile.
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),

    -- The browser's only network identity: the input a geo lookup would need
    -- (the SDKs send no geo) and how abuse of a public credential is traced.
    remote_addr        String,
    user_agent         String,

    -- Query parameters without a column of their own, as JSON, with
    -- dd-api-key and api_key removed: with query-string auth the credential
    -- is a parameter and must not become durable in a table.
    query_extra        String CODEC(ZSTD(3)),

    date               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- application.id is bounded by how many RUM applications exist; session
    -- and view ids are uuids per visit and per page.
    application_id     LowCardinality(String),
    session_id         String,
    view_id            String,

    document_version   UInt64 CODEC(T64, ZSTD(1)),

    -- "view" or "view_update": which wire form won the merge.
    event_type         LowCardinality(String),

    service            LowCardinality(String),
    version            LowCardinality(String),
    build_version      LowCardinality(String),
    build_id           LowCardinality(String),

    -- Not the same as evp_origin: a WebView event inside a mobile batch keeps
    -- source "browser" while the request says ios or android.
    source             LowCardinality(String),

    session_type               LowCardinality(String),
    session_has_replay         Nullable(Bool),
    session_is_active          Nullable(Bool),
    session_sampled_for_replay Nullable(Bool),

    -- Unbounded and personal, so not LowCardinality: a dictionary of every
    -- user id never stops growing.
    usr_id             String CODEC(ZSTD(3)),
    usr_name           String CODEC(ZSTD(3)),
    usr_email          String CODEC(ZSTD(3)),
    usr_anonymous_id   String CODEC(ZSTD(3)),
    account_id         String,
    account_name       String,

    view_url           String CODEC(ZSTD(3)),
    view_name          String,
    view_referrer      String CODEC(ZSTD(3)),

    -- Averaging loading_time across initial_load and route_change produces a
    -- number that means nothing, so the type gets a column.
    view_loading_type  LowCardinality(String),
    view_loading_time  Nullable(Int64),
    view_time_spent    Int64 CODEC(T64, ZSTD(1)),
    view_is_active     Nullable(Bool),
    view_is_slow_rendered Nullable(Bool),

    -- Nullable because a platform that never produces a kind sends no
    -- sub-object, and "no crash object" is not "zero crashes".
    action_count       Nullable(Int64),
    error_count        Nullable(Int64),
    crash_count        Nullable(Int64),
    long_task_count    Nullable(Int64),
    frozen_frame_count Nullable(Int64),
    resource_count     Nullable(Int64),
    frustration_count  Nullable(Int64),

    -- Web Vitals, from view.performance.* or the deprecated flat view.*
    -- fields, both still on the wire. Timings in nanoseconds; cls is a score.
    lcp                Nullable(Int64),
    cls                Nullable(Float64),
    inp                Nullable(Int64),
    fcp                Nullable(Int64),
    fid                Nullable(Int64),
    fbc                Nullable(Int64),
    ttfb               Nullable(Int64),

    device_type        LowCardinality(String),
    device_brand       LowCardinality(String),
    device_model       String,
    device_name        String,
    os_name            LowCardinality(String),
    os_version         LowCardinality(String),
    connectivity_status LowCardinality(String),

    -- JSON text rather than Map(String, String): the values are arbitrary
    -- JSON and a Map would force them through a string cast.
    context            String CODEC(ZSTD(3)),
    feature_flags      String CODEC(ZSTD(3)),

    -- ddtags travels inside the event for every SDK, not in the query string
    -- as the agent sends it. Multiset shape, see
    -- docs/decisions/0001-tags-are-a-multiset.md.
    ddtags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    event              String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(document_version)
-- Monthly for correctness, not size: ReplacingMergeTree collapses rows only
-- within a partition, so a boundary between two versions of one view would
-- leave both alive. `date` is the view's start and stays constant across
-- updates, but a month keeps the guarantee from resting on an SDK detail.
PARTITION BY toYYYYMM(date)
-- Every RUM query starts from an application and narrows to a session.
ORDER BY (tenant_id, application_id, session_id, view_id)
-- A closed page is never replaced again, so a view ages out like telemetry.
TTL toDateTime(date) + INTERVAL 30 DAY;

-- action, error, resource, long_task, vital, transition: facts that happened
-- once, sharing the whole common block and differing in one sub-object, which
-- becomes a few per-kind columns here and stays whole in `event`.
CREATE TABLE IF NOT EXISTS rum_events
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),
    evp_encoding       LowCardinality(String),
    request_id         String,
    idempotency_key    String,
    dd_api             LowCardinality(String),
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),
    remote_addr        String,
    user_agent         String,
    query_extra        String CODEC(ZSTD(3)),

    date               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    application_id     LowCardinality(String),
    session_id         String,
    view_id            String,

    -- In the sort key: "every error in this application" is the query this
    -- table answers most.
    event_type         LowCardinality(String),

    service            LowCardinality(String),
    version            LowCardinality(String),
    build_version      LowCardinality(String),
    build_id           LowCardinality(String),
    source             LowCardinality(String),

    session_type       LowCardinality(String),
    session_has_replay Nullable(Bool),

    usr_id             String CODEC(ZSTD(3)),
    usr_name           String CODEC(ZSTD(3)),
    usr_email          String CODEC(ZSTD(3)),
    usr_anonymous_id   String CODEC(ZSTD(3)),
    account_id         String,
    account_name       String,

    view_url           String CODEC(ZSTD(3)),
    view_name          String,
    view_referrer      String CODEC(ZSTD(3)),

    device_type        LowCardinality(String),
    device_brand       LowCardinality(String),
    device_model       String,
    device_name        String,
    os_name            LowCardinality(String),
    os_version         LowCardinality(String),
    connectivity_status LowCardinality(String),

    -- The name lives in action.target.name. One click can be both a rage and
    -- a dead click, so the frustration types stay an array.
    action_type        LowCardinality(String),
    action_name        String,
    action_id          String,
    action_frustration_types Array(LowCardinality(String)),

    -- is_crash marks the crash and ANR reports mobile SDKs send at the next
    -- launch, which is why their date can predate their session.
    error_id           String,
    error_message      String CODEC(ZSTD(3)),
    error_type         LowCardinality(String),
    error_source       LowCardinality(String),
    error_stack        String CODEC(ZSTD(3)),
    error_is_crash     Nullable(Bool),
    error_handling     LowCardinality(String),
    error_fingerprint  String,

    resource_id        String,
    resource_type      LowCardinality(String),
    resource_url       String CODEC(ZSTD(3)),
    resource_method    LowCardinality(String),
    resource_status_code Nullable(Int64),
    resource_duration  Nullable(Int64),
    resource_size      Nullable(Int64),

    long_task_id       String,
    long_task_duration Nullable(Int64),

    -- vital.type (duration | operation_step | app_launch) picks the schema
    -- variant; all three share these columns.
    vital_id           String,
    vital_type         LowCardinality(String),
    vital_name         String,
    vital_duration     Nullable(Int64),

    context            String CODEC(ZSTD(3)),
    ddtags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    event              String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(date)
ORDER BY (tenant_id, application_id, event_type, session_id, date)
TTL toDateTime(date) + INTERVAL 30 DAY;

-- The SDK talking about itself, on the same track as user-facing events and
-- from every SDK, including Logs-only ones. Not product data: it answers "why
-- is this app sending nothing?" with the SDK's resolved configuration, the
-- APIs the app called and the SDK's own failures.
CREATE TABLE IF NOT EXISTS rum_telemetry
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),
    evp_encoding       LowCardinality(String),
    request_id         String,
    idempotency_key    String,
    dd_api             LowCardinality(String),
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),
    remote_addr        String,
    user_agent         String,
    query_extra        String CODEC(ZSTD(3)),

    date               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    application_id     LowCardinality(String),
    session_id         String,
    view_id            String,
    action_id          String,

    -- Always "telemetry" today; a column so another kind is a value, not a
    -- migration.
    type               LowCardinality(String),

    -- telemetry.status (error | debug) and telemetry.type (log |
    -- configuration | usage).
    status             LowCardinality(String),
    telemetry_type     LowCardinality(String),

    message            String CODEC(ZSTD(3)),
    error_stack        String CODEC(ZSTD(3)),
    error_kind         LowCardinality(String),

    -- ~150 optional keys that change every SDK release, none worth a column.
    configuration      String CODEC(ZSTD(3)),

    usage_feature      LowCardinality(String),

    service            LowCardinality(String),
    version            LowCardinality(String),
    source             LowCardinality(String),

    effective_sample_rate Nullable(Float64),
    experimental_features Array(LowCardinality(String)),

    device_brand       LowCardinality(String),
    device_model       String,
    device_architecture LowCardinality(String),
    os_name            LowCardinality(String),
    os_version         LowCardinality(String),
    os_build           String,

    event              String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(date)
ORDER BY (tenant_id, application_id, status, date)
-- The audit class: what an SDK was configured to do three months ago is a
-- question that outlives the data.
TTL toDateTime(date) + INTERVAL 90 DAY;

-- Mobile cpu and memory samples. One event carries a run of samples,
-- timestamps in one array and each quantity in a parallel one; the arrays are
-- kept as they arrived, which keeps timestamps[i] paired with its values and
-- avoids multiplying the row count by the batch size for no query we have.
CREATE TABLE IF NOT EXISTS rum_timeseries
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),
    evp_encoding       LowCardinality(String),
    request_id         String,
    idempotency_key    String,
    dd_api             LowCardinality(String),
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),
    remote_addr        String,
    user_agent         String,
    query_extra        String CODEC(ZSTD(3)),

    date               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    application_id     LowCardinality(String),
    session_id         String,
    view_id            String,

    -- cpu | memory, which picks the variant.
    name               LowCardinality(String),
    timeseries_id      String,
    timeseries_schema  LowCardinality(String),

    -- Nanoseconds, as the wire carries them here; milliseconds would stop
    -- them lining up with the timestamps array.
    start              DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    end                DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Wire order and wire units: the schema does not say whether these are
    -- absolute or relative.
    timestamps         Array(Int64) CODEC(T64, ZSTD(1)),

    -- A different object per variant that gains members every SDK release,
    -- so JSON rather than columns NULL for the other variant.
    values             String CODEC(ZSTD(3)),

    service            LowCardinality(String),
    version            LowCardinality(String),
    source             LowCardinality(String),
    context            String CODEC(ZSTD(3)),
    ddtags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    event              String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(date)
ORDER BY (tenant_id, application_id, name, session_id, date)
TTL toDateTime(date) + INTERVAL 14 DAY;

-- Session Replay, stored without processing, as Datadog's own intake does.
-- The browser's deflate encoder emits streams that can be concatenated later,
-- and re-deflating would throw that away; the record schema (94 structs, 8
-- unions) is not something to parse at ingest under load.
--
-- One row per blob part, not per request: mobile SDKs send N segments in one
-- multipart body with a parallel array of metadata, and collapsing them would
-- lose the per-segment metadata. part_name/part_filename keep the browser form
-- (one part named "segment") and the mobile form (file0, file1, ...) apart.
CREATE TABLE IF NOT EXISTS rum_replay_segments
(
    tenant_id          LowCardinality(String),

    -- Replay metadata has no `date`, and start/end are absent on the resource
    -- variant, so this table sorts, partitions and expires on arrival, the one
    -- timestamp every row has.
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),
    evp_encoding       LowCardinality(String),
    request_id         String,
    idempotency_key    String,
    dd_api             LowCardinality(String),
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),
    remote_addr        String,
    user_agent         String,
    query_extra        String CODEC(ZSTD(3)),

    application_id     LowCardinality(String),
    session_id         String,
    view_id            String,

    -- Browser and mobile segments have different record formats; this says
    -- which one the blob is in.
    source             LowCardinality(String),

    -- "segment" (a recording) or "resource" (a canvas image one references),
    -- told apart by which part carried the blob.
    variant            LowCardinality(String),
    part_name          LowCardinality(String),
    part_filename      String,

    -- Nullable because the resource variant sends neither, and an epoch here
    -- would put a canvas image in 1970.
    start              Nullable(DateTime64(3, 'UTC')),
    end                Nullable(DateTime64(3, 'UTC')),

    records_count      Nullable(Int64),
    has_full_snapshot  Nullable(Bool),

    -- Browser sends an index, Android null, iOS nothing: three states.
    index_in_view      Nullable(Int64),

    -- Browser only.
    creation_reason    LowCardinality(String),

    raw_segment_size        Nullable(Int64),
    compressed_segment_size Nullable(Int64),

    -- Already zlib-compressed by the SDK: ZSTD over a deflate stream costs CPU
    -- and saves nothing. The resource variant's image bytes likewise.
    segment            String CODEC(NONE),
    image              String CODEC(NONE),

    -- This blob's metadata object.
    event              String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, application_id, session_id, view_id, received_at)
-- The bulky class: replay is by far the heaviest thing RUM sends, and a replay
-- nobody watched in two weeks is one nobody will watch.
TTL toDateTime(received_at) + INTERVAL 14 DAY;

-- /api/v2/spans from dd-sdk-ios and dd-sdk-android: NDJSON of
-- {"spans":[...],"env":...} envelopes with a flat span shape no published JSON
-- Schema covers. It shares a host, a credential and a sender with mobile RUM
-- and nothing with the tracers' msgpack, hence here and not in the trace tables.
--
-- IDs are strings: the SDKs send 64- and 128-bit ids as decimal or hex text,
-- parsing them needs a guess about the base, and a float64 anywhere on the way
-- drops digits above 2^53.
CREATE TABLE IF NOT EXISTS rum_spans
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),
    evp_encoding       LowCardinality(String),
    request_id         String,
    idempotency_key    String,
    dd_api             LowCardinality(String),
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),
    remote_addr        String,
    user_agent         String,
    query_extra        String CODEC(ZSTD(3)),

    trace_id           String,
    span_id            String,
    parent_id          String,

    name               LowCardinality(String),
    service            LowCardinality(String),
    resource           String CODEC(ZSTD(3)),
    type               LowCardinality(String),

    -- From the envelope, not the span.
    env                LowCardinality(String),

    -- Nanoseconds, as on the wire: rounding a start to milliseconds makes
    -- short spans overlap.
    start              DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    duration_ns        Int64 CODEC(T64, ZSTD(1)),
    error              Int8,

    -- Nested objects (meta.device, meta.os) are flattened to dotted keys, so
    -- meta['device.brand'] is reachable. A plain Map, not the tag multiset: a
    -- JSON object cannot repeat a key.
    meta               Map(String, String) CODEC(ZSTD(3)),
    metrics            Map(String, Float64) CODEC(ZSTD(3)),

    span               String CODEC(ZSTD(3)),

    -- Envelope keys other than "spans" and "env", so a key a future SDK adds
    -- is kept rather than counted.
    envelope_extra     String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(start)
ORDER BY (tenant_id, service, name, start)
TTL toDateTime(start) + INTERVAL 14 DAY;

-- Agent envelopes: who sent a batch, the /intake/ check and external tag
-- lists, /api/v1/metadata, delegated auth, symbol queries, and the Private
-- Action Runner.
--
-- Two shapes recur. A Map(String, String) named `extra` holds the keys the
-- upstream model does not declare, values JSON-encoded, so one column takes a
-- string, a number, an object or an array, and a field a newer agent starts
-- sending is readable before anyone adds a column for it. A String holding a
-- sub-object verbatim (`meta`, `client`, `outputs`, `payload`) is empty when
-- the key was absent, which is not the same as "{}".

-- gogen.SketchPayload's CommonMetadata describes the SENDER: agent version,
-- timezone, its clock, its IPs. On a sketch row it would repeat once per
-- bucket, and it is what explains a host's numbers looking wrong: a clock
-- that disagrees with ours explains a gap on a chart faster than anything.
--
-- CommonMetadata's ApiKey is deliberately absent. The request already
-- authenticated, and a table with a TTL is no place for a credential.
CREATE TABLE IF NOT EXISTS agent_batch_metadata
(
    tenant_id     LowCardinality(String),
    received_at   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Which intake carried the block, so a second payload type carrying
    -- CommonMetadata lands here too instead of getting its own table.
    intake        LowCardinality(String),

    agent_version LowCardinality(String),
    timezone      LowCardinality(String),

    -- The agent's clock at send time, seconds, a float because the proto
    -- says so. Against received_at it measures clock skew.
    current_epoch Float64 CODEC(Gorilla, ZSTD(1)),

    -- Not LowCardinality: in a large fleet these are per-host.
    internal_ip   String,
    public_ip     String
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, agent_version, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- The V5 collector's check statuses from /intake/. collectorimpl.Payload
-- sends them as POSITIONAL ARRAYS (name, source type, instance id, status,
-- message) with no published type and no key names on the wire. Anything
-- after the fifth element goes to positional_extra, so a sixth appearing one
-- day is visible.
--
-- Not check_runs: that is /api/v1/check_run, the current service-check path.
-- Merging the two would mean inventing a mapping between two status
-- vocabularies that no published document relates.
CREATE TABLE IF NOT EXISTS agent_checks
(
    tenant_id     LowCardinality(String),
    received_at   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    hostname      LowCardinality(String),
    agent_version LowCardinality(String),
    uuid          String,

    check_name    LowCardinality(String),
    source_type   LowCardinality(String),
    instance_id   String,

    -- Nullable because the array may be shorter than four elements and 0 is
    -- a real status ("OK"): a truncated entry must not read as healthy.
    status        Nullable(Int64) CODEC(T64, ZSTD(1)),
    message       String CODEC(ZSTD(3)),

    -- A JSON array, empty when there were none.
    positional_extra String CODEC(ZSTD(3)),

    meta          String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, hostname, check_name, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- Tags an agent reports FOR ANOTHER host ([[hostname, {source: [tags]}], ...]
-- on /intake/): a vSphere collector tagging its VMs, a cloud integration
-- tagging instances. They cannot go on the hosts row, which belongs to the
-- sender, and they are per source, so one host has several rows at once.
-- tags is the multiset shape: nothing stops two Datadog tags sharing a key.
CREATE TABLE IF NOT EXISTS external_host_tags
(
    tenant_id   LowCardinality(String),
    received_at DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The host being described, not the agent that sent the request.
    host        String,
    source      LowCardinality(String),

    tags        Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, host, source, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- /api/v1/metadata: thirteen variants from eleven producers, each shaped
-- differently, none importable, and all changing with agent releases. The
-- shared envelope becomes columns; the variant's own body is kept as JSON
-- text. A String, not a JSON column: these bodies are read whole, by a
-- person, and a JSON column would grow a subcolumn for every path across
-- thirteen unrelated shapes.
CREATE TABLE IF NOT EXISTS agent_metadata
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- agent_metadata, host_metadata, check_metadata, ... A request carrying
    -- two variant keys produces two rows, which is what the wire allows.
    variant      LowCardinality(String),

    hostname     LowCardinality(String),
    -- The cluster-agent variants send these instead of hostname.
    cluster_name LowCardinality(String),
    cluster_id   String,

    -- Nullable: the envelope's field is optional and 1970 is not an answer.
    -- received_at is always ours.
    timestamp    Nullable(DateTime64(3, 'UTC')) CODEC(DoubleDelta, ZSTD(1)),
    uuid         String,

    payload      String CODEC(ZSTD(3)),

    -- Envelope keys beyond the named ones, values JSON-encoded:
    -- clustercheck_status and clustercheck_integration_status arrive here,
    -- next to the clustercheck_metadata row they belong to.
    envelope_extra Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, variant, hostname, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- /api/v2/intake-key: the agent posts `Authorization: Delegated <proof>` and
-- expects an API key back. The proof is the credential, so only its SHA-256
-- is kept: enough to tell delegated identities apart, to notice one used from
-- somewhere new, and to key a delegated-auth mapping on later, and useless to
-- anyone who steals the table.
CREATE TABLE IF NOT EXISTS delegated_auth_requests
(
    tenant_id  LowCardinality(String),
    at         DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- "Delegated", or whatever a client puts before the first space.
    scheme     LowCardinality(String),

    -- Lowercase hex. Never the proof itself.
    proof_fingerprint String,

    -- The key's id in Postgres: never the key, nor its hash.
    api_key_id LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(at)
ORDER BY (tenant_id, at)
TTL toDateTime(at) + INTERVAL 90 DAY;

-- The profiler's symbol uploader asks which build ids we hold before
-- uploading, and we answer "none". The question is kept: the build ids are
-- the profiled binaries of the fleet.
CREATE TABLE IF NOT EXISTS symbol_queries
(
    tenant_id LowCardinality(String),
    at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    arch      LowCardinality(String),

    -- Order preserved: the uploader reads the reply positionally against it.
    build_ids Array(String) CODEC(ZSTD(3)),

    -- The JSON:API document verbatim: its attribute set is undocumented and
    -- moves.
    resource  String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(at)
ORDER BY (tenant_id, at)
TTL toDateTime(at) + INTERVAL 30 DAY;

-- Private Action Runner: a request loop driven by what we answer, every call
-- after enrollment signed with the identity we handed out. Without the
-- enrollment kept, nothing could verify those signatures.

-- One row per runner, newest enrollment wins: a runner that re-enrolls (a pod
-- restart, a rotated key) must not leave a second identity behind, or a
-- verifier would hold two public keys and no rule for choosing. No TTL: an
-- enrolled runner is live state and leaves by being replaced, like `hosts`.
CREATE TABLE IF NOT EXISTS runner_enrollments
(
    tenant_id       LowCardinality(String),

    -- A UUID we hand out. Never a colon in it: the runner builds
    -- urn:dd:apps:on-prem-runner:<region>:<org_id>:<runner_id> and splits on ":".
    runner_id       String,
    enrolled_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    name            String,
    modes           Array(LowCardinality(String)),
    host            String,

    -- Public by construction, so safe to keep, and it must be kept or the
    -- OPMS JWTs the runner signs can never be checked.
    public_key_pem  String CODEC(ZSTD(3)),

    agent_hostname  String,
    orch_cluster_id String,
    agent_flavor    LowCardinality(String),

    -- What we answered, derived from the tenant by FNV-64a. Kept because the
    -- derivation is ours: if it ever changes, the runners enrolled under the
    -- old one are recoverable only from here.
    org_id          Int64 CODEC(T64, ZSTD(1)),

    -- Native JSON so a new attribute is queryable without a migration.
    attributes      JSON
)
ENGINE = ReplacingMergeTree(enrolled_at)
ORDER BY (tenant_id, runner_id);

-- Task outcomes. outputs is the action's own result, arbitrary JSON of any
-- size.
CREATE TABLE IF NOT EXISTS runner_task_updates
(
    tenant_id     LowCardinality(String),
    at            DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The JSON:API document id: "succeed_task" or "fail_task".
    outcome       LowCardinality(String),

    task_id       String,
    action_fqn    LowCardinality(String),
    job_id        String,

    -- The runner's self-description.
    client        String CODEC(ZSTD(3)),

    branch        LowCardinality(String),
    outputs       String CODEC(ZSTD(3)),

    -- An enum on the runner's side; a succeeded task sends none.
    error_code    Nullable(Int64) CODEC(T64, ZSTD(1)),
    error_details String CODEC(ZSTD(3)),
    api_error     String CODEC(ZSTD(3)),

    -- From either level of the document, prefixed "payload." when nested.
    extra         Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(at)
ORDER BY (tenant_id, job_id, task_id, at)
TTL toDateTime(at) + INTERVAL 90 DAY;

-- Per-task heartbeats: the chattiest table here, worth little one by one, and
-- exactly what answers "when did this job stop making progress". Hence daily
-- partitions and a short TTL.
CREATE TABLE IF NOT EXISTS runner_heartbeats
(
    tenant_id  LowCardinality(String),
    at         DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    task_id    String,
    action_fqn LowCardinality(String),
    job_id     String,
    client     String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(at)
ORDER BY (tenant_id, job_id, task_id, at)
TTL toDateTime(at) + INTERVAL 7 DAY;

-- The idle poll: the only liveness signal a runner with no tasks produces.
-- Both timestamps stay the strings they arrive as: the format is
-- undocumented and an unparseable value must not become 1970.
CREATE TABLE IF NOT EXISTS runner_dequeues
(
    tenant_id             LowCardinality(String),
    at                    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    runner_started_at     String,
    last_task_received_at String,

    -- X-Datadog-OnPrem-Version / -Modes: the runner's identity on a request
    -- with no body worth speaking of.
    version               LowCardinality(String),
    modes                 LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toDate(at)
ORDER BY (tenant_id, at)
TTL toDateTime(at) + INTERVAL 7 DAY;

-- Connections an action runner may use to reach a third-party system.
--
-- SENSITIVE: `credentials` holds that integration's secrets (tokens,
-- passwords, private keys) verbatim, because a connection without them cannot
-- be used. It must never be logged, and anything that reads it (a panel, an
-- export, a support flare) has to treat it as a credential store, not as
-- telemetry.
CREATE TABLE IF NOT EXISTS action_connections
(
    tenant_id        LowCardinality(String),
    at               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    name             String,
    runner_id        String,

    tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    integration_type LowCardinality(String),

    -- SECRETS. See above.
    credentials      String CODEC(ZSTD(3)),

    extra            Map(String, String) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(at)
ORDER BY (tenant_id, runner_id, name, at)
TTL toDateTime(at) + INTERVAL 90 DAY;

-- ECS tasks: CollectorECSTask (orchestrator type 200).
--
-- A task has no Kubernetes Metadata, so it cannot be stored under
-- k8s_resources' identity columns; it gets a table of its own rather than a
-- lossy squeeze into someone else's. The identity is the task ARN. Same data
-- class as k8s_resources (periodic collection passes, bursty, read over days
-- not months), so the same daily partitions and 30-day TTL. Append-only: the
-- history of what ran, not a current-state view.
CREATE TABLE IF NOT EXISTS ecs_tasks
(
    tenant_id        LowCardinality(String),
    collected_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The frame header, as for every other orchestrator row.
    org_id           Int32 CODEC(T64, ZSTD(1)),
    subscription_id  UInt8,
    header_timestamp Nullable(Int64),
    encoding         LowCardinality(String),

    aws_account_id   Int64 CODEC(T64, ZSTD(1)),
    cluster_id       LowCardinality(String),
    cluster_name     LowCardinality(String),
    region           LowCardinality(String),
    group_id         Int32 CODEC(T64, ZSTD(1)),
    group_size       Int32 CODEC(T64, ZSTD(1)),
    host_name        LowCardinality(String),
    agent_version    LowCardinality(String),
    -- The envelope's tags (CollectorECSTask.Tags), kept apart from the task's
    -- own three tag sets: they are different fields on the wire, and merged
    -- there would be no telling which producer attached a tag.
    envelope_tags    Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    arn              String,
    resource_version String,
    launch_type      LowCardinality(String),  -- EC2 / FARGATE
    desired_status   LowCardinality(String),
    known_status     LowCardinality(String),
    family           LowCardinality(String),
    version          LowCardinality(String),
    availability_zone LowCardinality(String),
    service_name     LowCardinality(String),
    vpc_id           LowCardinality(String),
    container_instance_arn String,
    daemon_name      LowCardinality(String),
    task_host_name   LowCardinality(String),  -- ECSTask.Host.Name

    -- CPU and memory the task was given, and its ephemeral disk counters.
    -- Plain Maps: protobuf map keys are unique, so no multiset shape.
    limits           Map(String, Float64),
    ephemeral_storage_metrics Map(String, Int64),

    -- Unix seconds, all genuinely optional: a task that never finished
    -- pulling has no pull_stopped_at, and 0 must not become 1970.
    pull_started_at      Nullable(DateTime('UTC')),
    pull_stopped_at      Nullable(DateTime('UTC')),
    execution_stopped_at Nullable(DateTime('UTC')),

    -- The container list as JSON, with its length as a column so "how many
    -- containers" needs no parsing. An ECSContainer carries ports, networks,
    -- volumes, health, log options and limits: as columns it would be twenty
    -- parallel arrays of arrays, read by nobody. Kept whole, it answers a
    -- per-container question exactly.
    containers       String CODEC(ZSTD(3)),
    container_count  UInt32 CODEC(T64, ZSTD(1)),

    -- Separate because ECS keeps them separate: Datadog tags, the task's ECS
    -- tags, and the container instance's.
    tags                   Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    ecs_tags               Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    container_instance_tags Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- The CollectorECSTask envelope as JSON, minus its task list. It carries
    -- a Host and an Info (SystemInfo) of its own, describing the machine and
    -- agent that reported the pass: distinct from the task's host, and held
    -- by none of the columns above.
    envelope         String CODEC(ZSTD(3)),
    -- ECSTask.Host in full: task_host_name is only its Name, while the rest
    -- (id, orgId, allTags, numCpus, totalMemory) tells two container
    -- instances of the same name apart across accounts.
    task_host        String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)                 TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(collected_at)
ORDER BY (tenant_id, cluster_id, arn, collected_at)
TTL toDateTime(collected_at) + INTERVAL 30 DAY;

-- Agent flares: support archives uploaded to POST /support/flare.
--
-- A flare is a support artifact a human asked for (`datadog-agent flare`, or a
-- remote-config task), not telemetry. The named columns are the fields the
-- agent's uploader writes; agent_version and hostname arrive after the file
-- part, so a streaming reader cannot assume metadata precedes the archive.
-- Every other field (rc_task_uuid today, whatever a later agent adds) lands in
-- `fields` instead of being dropped.
CREATE TABLE IF NOT EXISTS agent_flares
(
    tenant_id     LowCardinality(String),
    received_at   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Not LowCardinality: one hostname per machine, so the dictionary would
    -- grow without bound.
    hostname      String,

    -- A string, not an integer: the agent omits it for a new case with no id
    -- yet, and an empty string keeps that absence instead of turning it into 0.
    case_id       String,
    email         String,
    source        LowCardinality(String),
    agent_version LowCardinality(String),

    filename      String,
    size_bytes    UInt64 CODEC(T64, ZSTD(1)),

    fields        Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- The zip archive verbatim. Nothing filters on it; it is read whole when
    -- someone opens the case.
    archive       String CODEC(ZSTD(3))
)
ENGINE = MergeTree
-- Monthly partitions: a few flares per incident, not a steady stream. 90 days,
-- like check_runs, events and k8s_actions: a flare is something a person asked
-- for, not a debugging sample.
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- CI Visibility: test events, coverage, git metadata, tracer settings
-- requests, CI provider webhooks, datadog-ci calls; and the synthetics
-- poller's test catalogue.
--
-- CI Visibility is six wire formats sharing a product name, joined by ids
-- rather than by shape, so it is a family of tables: one table would be a row
-- mostly NULL whichever producer wrote it.
--
-- Every field was read from a client that sends it (dd-trace-go's
-- civisibility code, the agent's synthetics scheduler, the Jenkins plugin),
-- not from documentation. The backend's field catalogue is not public, so
-- "complete" means complete for what shipped clients write, which is why
-- every table keeps a raw column.

-- citestcycle-intake: one row per event (test, suite end, module end,
-- session end, plain span), not per payload. The envelope's per-event-type
-- metadata is merged into each row because every query starts from a test and
-- would otherwise pay a join to learn which language ran it.
--
-- The ids are decoded as integers end to end, never through a float: a
-- session id above 2^53 rounded by a float would join to nothing.
CREATE TABLE IF NOT EXISTS ci_test_events
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The only version negotiation this protocol has (2 for "test", 1 for
    -- the rest), so a version 3 arriving must be visible. payload_version is
    -- the envelope's own.
    event_type         LowCardinality(String),
    event_version      Int32 CODEC(T64, ZSTD(1)),
    payload_version    Int32 CODEC(T64, ZSTD(1)),

    -- Which of the three is non-zero says what the event is: a test carries
    -- all three, a suite end session+module+suite, a module end
    -- session+module, a session end session only. Not Nullable: 0 is a
    -- structural absence, not an unknown.
    session_id         UInt64 CODEC(T64, ZSTD(1)),
    module_id          UInt64 CODEC(T64, ZSTD(1)),
    suite_id           UInt64 CODEC(T64, ZSTD(1)),

    -- Only test and span events carry these; the *_end events leave them 0.
    trace_id           UInt64 CODEC(T64, ZSTD(1)),
    span_id            UInt64 CODEC(T64, ZSTD(1)),
    parent_id          UInt64 CODEC(T64, ZSTD(1)),

    -- Echoed from a skippable-tests response, tying a skip decision to the
    -- run that honoured it.
    itr_correlation_id String,

    service            LowCardinality(String),
    env                LowCardinality(String),
    name               LowCardinality(String),
    resource           String,
    -- content.type repeats the outer type today; that is an observation, not
    -- a guarantee.
    span_type          LowCardinality(String),

    -- Nanoseconds on the wire. Millisecond precision would leave two tests in
    -- the same millisecond unorderable, and ordering tests inside a suite is
    -- the point of the column.
    start              DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    duration_ns        Int64 CODEC(T64, ZSTD(1)),
    error              Int32 CODEC(T64, ZSTD(1)),

    -- Copies of meta tags for query speed, not extraction: a producer that
    -- renames a tag loses a column and keeps the data.
    test_name              LowCardinality(String),
    test_suite             LowCardinality(String),
    test_module            LowCardinality(String),
    test_framework         LowCardinality(String),
    test_framework_version LowCardinality(String),
    test_status            LowCardinality(String),
    test_type              LowCardinality(String),
    test_source_file       String,
    -- "No source location reported" is not "line 0".
    test_source_start      Nullable(Int64),
    test_source_end        Nullable(Int64),
    test_parameters        String CODEC(ZSTD(3)),
    test_codeowners        String CODEC(ZSTD(3)),
    test_command           String,
    test_session_name      String,

    git_repository_url        String,
    git_branch                LowCardinality(String),
    git_tag                   LowCardinality(String),
    git_commit_sha            String,
    git_commit_message        String CODEC(ZSTD(3)),
    git_commit_author_name    String,
    git_commit_author_email   String,
    git_commit_author_date    String,
    git_commit_committer_name  String,
    git_commit_committer_email String,
    git_commit_committer_date  String,

    ci_provider_name    LowCardinality(String),
    ci_pipeline_id      String,
    ci_pipeline_name    String,
    ci_pipeline_number  String,
    ci_pipeline_url     String,
    ci_job_id           String,
    ci_job_name         String,
    ci_job_url          String,
    ci_stage_name       String,
    ci_workspace_path   String,
    ci_node_name        String,
    -- A JSON array in one tag, kept as sent: a producer that changes the
    -- encoding must not silently produce an empty list.
    ci_node_labels      String,

    -- language, runtime_id and library_version come from the envelope's
    -- metadata, not the span.
    os_platform         LowCardinality(String),
    os_version          LowCardinality(String),
    os_architecture     LowCardinality(String),
    runtime_name        LowCardinality(String),
    runtime_version     LowCardinality(String),
    language            LowCardinality(String),
    runtime_id          String,
    library_version     LowCardinality(String),

    -- Nullable: a tracer with the feature off sends no tag, and "never said"
    -- must not count as "false".
    test_is_new              Nullable(UInt8),
    test_is_retry            Nullable(UInt8),
    test_is_modified         Nullable(UInt8),
    test_skipped_by_itr      Nullable(UInt8),
    itr_unskippable          Nullable(UInt8),
    itr_forced_run           Nullable(UInt8),
    code_coverage_enabled    Nullable(UInt8),
    test_retry_reason        LowCardinality(String),
    early_flake_abort_reason LowCardinality(String),

    -- What survives of the agent's evp_proxy when a tracer tunnels through it
    -- (Datadog-Container-ID, X-Datadog-Hostname, "Via: trace-agent <v>").
    -- The proxy consumes X-Datadog-EVP-Subdomain itself, so it is only set
    -- when something else sent it.
    evp_subdomain    LowCardinality(String),
    container_id     String,
    agent_hostname   String,
    agent_version    LowCardinality(String),

    -- Every key verbatim; the hot columns above are copies. A plain map, not
    -- the tag multiset: msgpack map[string]string cannot repeat a key, so the
    -- multiset would cost lookup speed for nothing
    -- (api/docs/decisions/0001-tags-are-a-multiset.md).
    meta             Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    metrics          Map(LowCardinality(String), Float64) CODEC(ZSTD(3)),
    -- The envelope's metadata for this event type, "*" merged under it.
    metadata         Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- The event as JSON: read by a human debugging a payload, and the tracer
    -- documents the JSON shape, not the msgpack one.
    content          String CODEC(ZSTD(3)),

    -- The two point lookups that do not start from the ORDER BY: one CI run,
    -- and one test over time (is it flaky).
    INDEX idx_ci_session_id session_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_ci_test_name  test_name  TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(start)
-- Every CI question starts "what did this service's tests do lately".
-- Monthly partitions: the TTL is months long, and hundreds of daily
-- partitions slow query planning.
ORDER BY (tenant_id, service, start)
TTL toDateTime(start) + INTERVAL 90 DAY;

-- citestcov-intake: one row per coverages[] entry of the multipart coverage
-- part. The bitmap is opaque: the backend's job is to hand the same bytes
-- back in a skippable-tests response, so decoding it would only add a format
-- to track.
CREATE TABLE IF NOT EXISTS ci_coverage
(
    tenant_id       LowCardinality(String),
    received_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    payload_version Int32 CODEC(T64, ZSTD(1)),

    session_id      UInt64 CODEC(T64, ZSTD(1)),
    suite_id        UInt64 CODEC(T64, ZSTD(1)),
    -- Omitted in suite-level skipping mode, where there is no per-test span;
    -- 0 would be a real span id.
    span_id         Nullable(UInt64),

    -- Parallel arrays (a Nested would be the same two arrays), always the same
    -- length: a file known only at file granularity keeps an empty bitmap.
    files_filename  Array(String) CODEC(ZSTD(3)),
    files_bitmap    Array(String) CODEC(ZSTD(3)),

    -- The entry's bytes as they arrived, msgpack or JSON, not re-encoded.
    raw             String CODEC(ZSTD(3)),
    raw_format      LowCardinality(String),

    -- The companion "event" part, {"dummy": true} in today's clients.
    event           String CODEC(ZSTD(3)),

    extra           Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    evp_subdomain   LowCardinality(String),
    container_id    String,
    agent_hostname  String,
    agent_version   LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
-- Queried by "the session I am deciding skips for", never by time.
ORDER BY (tenant_id, session_id, suite_id, received_at)
-- Expires with ci_test_events: coverage without its test events cannot be
-- interpreted.
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- Commit shas a tracer or datadog-ci mentioned for a repository, from
-- search_commits candidate lists and packfile uploads; that is all either
-- request says. The same sha is mentioned on every CI run, so one row per
-- (repo, sha) with the latest sighting. Readers use FINAL or argMax.
CREATE TABLE IF NOT EXISTS git_commits
(
    tenant_id      LowCardinality(String),
    repository_url String,
    sha            String,
    seen_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Set only when the sha was a packfile's pushedSha. NULL, the common
    -- case, does not mean "we have the objects".
    packfile_id    Nullable(String),

    -- "search_commits" or "packfile".
    source         LowCardinality(String)
)
ENGINE = ReplacingMergeTree(seen_at)
ORDER BY (tenant_id, repository_url, sha);
-- No TTL: a commit is an identity, and an expired "do we know this sha" is a
-- wrong answer rather than a missing one.

-- One row per uploaded .pack file.
CREATE TABLE IF NOT EXISTS git_packfiles
(
    tenant_id      LowCardinality(String),
    received_at    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    repository_url String,
    pushed_sha     String,

    -- sha256 of the pack, hex. The upload has no id of its own; this is what
    -- git_commits.packfile_id refers to, and it makes a re-upload
    -- recognisable.
    packfile_id    String,
    filename       String,

    -- A git pack is already compressed; ZSTD is for uniformity with the other
    -- bulky cold columns, not for a win.
    packfile       String CODEC(ZSTD(3)),
    size_bytes     UInt64 CODEC(T64, ZSTD(1)),

    -- Multipart parts other than pushedSha and packfile.
    other_parts    Map(LowCardinality(String), String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, repository_url, pushed_sha, received_at)
-- A pack is kept to resolve commits that test events refer to, so it lives
-- as long as they do.
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- The endpoints a tracer consults: settings, skippable, known_tests,
-- flaky_tests, test_management. Reads from the tracer's side, so what is kept
-- is the question and our answer: once any of these features is implemented,
-- which tracers asked what and what they were told is the migration plan. It
-- also makes a tracer stuck in a retry loop visible.
CREATE TABLE IF NOT EXISTS ci_settings_requests
(
    tenant_id      LowCardinality(String),
    received_at    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    endpoint       LowCardinality(String),

    -- JSON:API data.id and data.type. The tracer generates the id per process
    -- and it correlates every request of one client.
    request_id     String,
    request_type   LowCardinality(String),

    service        LowCardinality(String),
    env            LowCardinality(String),
    repository_url String,
    branch         LowCardinality(String),
    sha            String,
    -- "test" or "suite", skippable only.
    test_level     LowCardinality(String),
    -- test_management only.
    module         String,
    commit_message String CODEC(ZSTD(3)),
    -- known_tests pagination cursor; empty on the first page.
    page_state     String,

    -- The os.*, runtime.* and custom bag that decides which stored results
    -- are comparable. JSON, not a Map, because "custom" is a nested object.
    configurations String CODEC(ZSTD(3)),

    -- The response is a contract we invented (every feature off); when it
    -- changes, knowing what a tracer was told separates a bug from a mystery.
    request_body   String CODEC(ZSTD(3)),
    response_body  String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, service, received_at)
-- Diagnostic traffic, read while somebody is wiring a tracer up.
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- webhook-intake: the generic CI provider webhook (the Jenkins plugin, and
-- GitLab's legacy integration on the same URL). A batch mixes pipeline, stage
-- and job elements told apart by "level", so one table with a level column is
-- the shape the producer assumes. An unrecognised provider still gets a full
-- row.
CREATE TABLE IF NOT EXISTS ci_webhook_events
(
    tenant_id       LowCardinality(String),
    received_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- DD-CI-PROVIDER-NAME, else sniffed from the body, else "unknown"; never
    -- guessed into a real provider name.
    provider        LowCardinality(String),
    -- GitLab's native object_kind lands here too.
    level           LowCardinality(String),
    -- The Jenkins plugin's ?service=, its CI instance name.
    service         LowCardinality(String),

    payload_version Nullable(Int64),
    partial_retry   Nullable(UInt8),
    is_manual       Nullable(UInt8),

    trace_id        String,
    span_id         String,
    parent_span_id  String,
    id              String,
    unique_id       String,
    pipeline_id     String,
    pipeline_unique_id String,
    pipeline_name   String,
    stage_id        String,
    stage_name      String,
    parent_stage_id String,

    name            String,
    url             String,
    status          LowCardinality(String),

    -- ISO 8601 on the wire. An unrecognised format loses the parse, never the
    -- value, and a missing timestamp stays NULL instead of becoming 1970.
    start_raw       String,
    start_parsed    Nullable(DateTime64(3, 'UTC')),
    end_raw         String,
    end_parsed      Nullable(DateTime64(3, 'UTC')),
    queue_time_ms   Nullable(Int64),

    node_name       String,
    node_hostname   String,
    node_workspace  String,
    node_labels     Array(String) CODEC(ZSTD(3)),

    git_repository_url  String,
    git_default_branch  LowCardinality(String),
    git_branch          LowCardinality(String),
    git_sha             String,
    git_tag             LowCardinality(String),
    git_message         String CODEC(ZSTD(3)),
    git_author_name     String,
    git_author_email    String,
    git_author_time     String,
    git_committer_name  String,
    git_committer_email String,
    git_commit_time     String,

    user_name       String,
    user_email      String,

    error_message   String CODEC(ZSTD(3)),
    error_type      LowCardinality(String),
    error_domain    LowCardinality(String),
    error_stack     String CODEC(ZSTD(3)),

    parent_pipeline_trace_id String,
    parent_pipeline_url      String,

    -- A JSON object, so keys are unique.
    parameters      Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- "key:value" strings, a real tag multiset: the Jenkins plugin repeats
    -- keys for per-configuration axes
    -- (api/docs/decisions/0001-tags-are-a-multiset.md).
    tags            Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- For deduplicating a redelivered hook.
    delivery_id     String,

    -- Allowlisted identification headers, never a credential.
    headers         Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    extra           Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- Providers add fields without telling anybody; this is where it shows.
    body            String CODEC(ZSTD(3)),

    -- "This pipeline's stages and jobs" does not start from the ORDER BY.
    INDEX idx_ci_webhook_pipeline pipeline_unique_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, provider, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- datadog-ci's write endpoints: tag, measure, custom spans. All carry ci_env,
-- the provider's environment snapshot used to correlate the call with a
-- pipeline a webhook or tracer reported; it is the join key, hence a Map.
CREATE TABLE IF NOT EXISTS ci_pipeline_events
(
    tenant_id   LowCardinality(String),
    received_at DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Our name for the route ("tag", "measure", "custom_span"); data_type is
    -- what the CLI sent.
    kind        LowCardinality(String),
    data_type   LowCardinality(String),

    provider    LowCardinality(String),
    -- 1 pipeline, 2 stage, 3 job; a custom span sends none.
    ci_level    Nullable(Int64),
    ci_env      Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- A JSON object, so unique keys: not a tag multiset.
    tags        Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    metrics     Map(LowCardinality(String), Float64) CODEC(ZSTD(3)),

    -- Best effort: the CLI's custom span type was not readable from a
    -- primary source, so these fill only when the obvious names are present.
    -- attributes always holds the truth.
    span_name       String,
    span_start_raw  String,
    span_end_raw    String,

    attributes  String CODEC(ZSTD(3)),
    body        String CODEC(ZSTD(3)),

    extra       Map(LowCardinality(String), String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, kind, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- What the agent's synthetics poller fetches every two seconds and runs; its
-- results land in synthetics_results. Not fed by an intake: the panel will
-- write it. Columns are exactly the agent's SyntheticsTestConfig. An unknown
-- subtype makes the agent fail the whole poll, not just that test, so subtype
-- holds only "UDP", "TCP" or "ICMP".
CREATE TABLE IF NOT EXISTS synthetics_test_configs
(
    tenant_id   LowCardinality(String),
    updated_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    public_id   String,
    version     Int32 CODEC(T64, ZSTD(1)),
    type        LowCardinality(String),
    subtype     LowCardinality(String),

    -- Seconds.
    tick_every  Int32 CODEC(T64, ZSTD(1)),
    org_id      Int64 CODEC(T64, ZSTD(1)),
    main_dc     LowCardinality(String),
    result_id   String,
    run_type    LowCardinality(String),

    -- JSON: handed back to the agent as JSON, and nothing filters on it.
    assertions  String CODEC(ZSTD(3)),
    -- Shaped per subtype; JSON for the same reason.
    request     String CODEC(ZSTD(3)),

    -- Kept rather than deleted when off: "paused" is a state a UI needs.
    enabled     UInt8
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (tenant_id, public_id);
-- No TTL: a live object that leaves only by being replaced.

-- Universal Service Monitoring: what system-probe saw on each connection,
-- per HTTP endpoint, Kafka topic and database operation.
--
-- The agent ships these inside every Connection as four serialized messages
-- (HTTPAggregations, HTTP2Aggregations, DataStreamsAggregations,
-- DatabaseAggregations; agent-payload, process/agent.proto). `connections`
-- keeps the bytes; here each entry is a row, so "which endpoints answer 5xx"
-- and "which topic is slow" are column filters.
--
-- Every table starts with the columns that find the connection in
-- `connections`: payload_id, pid and both addresses. Same partitioning, order
-- and TTL as `connections`, because they are read together.
--
-- Latencies are nanoseconds. A stat with one sample carries it in
-- first_latency_sample and has no sketch; with more, the sketch is a DDSketch
-- kept as bytes with its decoded numbers beside it, as in apm_stats.
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

-- Sensitive Data Scanner (sds-intake.<site>, /api/v2/sdsresult).
--
-- One payload is one scan of one resource: the agent's SdsResultPayload
-- (pkg/proto/datadog/sds/sds_result.proto). Three tables, its own three levels:
--
--   sds_scans     the payload: who scanned what, with which rules
--   sds_results   one location that was scanned, with or without matches
--   sds_matches   one rule that matched in one location
--
-- scan_id is minted on arrival and ties the three together. A result with
-- no match still gets its row: "scanned and clean" is what coverage is
-- measured by.

CREATE TABLE IF NOT EXISTS sds_scans
(
    tenant_id            LowCardinality(String),
    received_at          DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    scan_id              UUID,
    -- The scanner's clock, Unix milliseconds on the wire. NULL when it sent 0.
    timestamp            Nullable(DateTime64(3, 'UTC')),
    -- The agent's scanner sends 'postgres_table' and a name built from the
    -- cluster, database, schema and table.
    resource_type        LowCardinality(String),
    resource_name        String CODEC(ZSTD(1)),

    -- 'agent', 'agentless', 'datadog_crawler', or '' when not sent. The
    -- columns after it are that source's own fields.
    scanning_source      LowCardinality(String),
    source_version       LowCardinality(String),
    source_region        LowCardinality(String),
    source_hostname      String,
    source_service_name  String,
    scanner_version      LowCardinality(String),
    scanner_region       LowCardinality(String),

    rule_ids             Array(String),
    -- The deprecated map of rule id to name, priority, tags and labels, as
    -- JSON; '' when empty.
    rules                String CODEC(ZSTD(3)),

    -- ScanStats. NULL when the payload carried none: a file count of 0 is a
    -- scan that found no files.
    scan_duration_ms                   Nullable(Int64),
    total_files_found                  Nullable(Int64),
    files_scanned                      Nullable(Int64),
    files_skipped_unsupported_type     Nullable(Int64),
    files_partially_scanned_size_limit Nullable(Int64),
    total_data_scanned_bytes           Nullable(Int64),
    skipped_files_by_type              Map(String, Int64),

    result_count         UInt32
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, resource_type, resource_name, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

CREATE TABLE IF NOT EXISTS sds_results
(
    tenant_id            LowCardinality(String),
    received_at          DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    scan_id              UUID,
    result_index         UInt32,
    resource_type        LowCardinality(String),
    resource_name        String CODEC(ZSTD(1)),

    -- 'postgres_table', 'rds_table', 'snowflake_table', 's3_file', or 'path'
    -- and 'database' from the deprecated fields; '' when not sent.
    location_kind        LowCardinality(String),
    database_name        String,
    schema_name          String,
    table_name           String,
    path                 String CODEC(ZSTD(1)),
    -- NULL where the location's kind has no such field.
    table_row_count      Nullable(Int64),
    scanned_row_count    Nullable(Int64),
    scanned_column_names Array(String),
    scanned_column_types Array(String),
    -- The whole location as protobuf's JSON: each kind has fields of its own
    -- (ARNs, snapshot times, sizes) that have no column.
    location             String CODEC(ZSTD(3)),

    duration             Int64,
    task_id              String,
    sub_task_id          String,
    task_started_at      Nullable(DateTime64(3, 'UTC')),
    task_ended_at        Nullable(DateTime64(3, 'UTC')),
    -- STATUS_UNKNOWN, SUCCESS or ERROR; '' when the result carried no task
    -- metadata.
    task_status          LowCardinality(String),
    failure_reason       String CODEC(ZSTD(1)),

    match_count          UInt32,
    table_match_count    UInt32
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, resource_type, resource_name, received_at, scan_id, result_index)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

CREATE TABLE IF NOT EXISTS sds_matches
(
    tenant_id            LowCardinality(String),
    received_at          DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    scan_id              UUID,
    result_index         UInt32,
    resource_type        LowCardinality(String),
    resource_name        String CODEC(ZSTD(1)),
    location_kind        LowCardinality(String),
    database_name        String,
    schema_name          String,
    table_name           String,
    path                 String CODEC(ZSTD(1)),

    -- 'match': a rule matched in text (a file). 'table_match': a rule
    -- matched in a column, counted over rows. The columns of the other kind
    -- stay at their defaults.
    kind                 LowCardinality(String),
    rule_id              LowCardinality(String),

    -- What the scanner chose to show of the match. Stored as sent: whether
    -- it is redacted is the scanner's decision, not this table's.
    sample               String CODEC(ZSTD(3)),
    start_index          Int64,
    end_index            Int64,
    match_path           Nullable(String),
    line                 Nullable(Int64),
    column               Nullable(Int64),
    start_line           Nullable(Int64),
    end_line             Nullable(Int64),
    row                  Nullable(Int64),
    start_index_in_line  Nullable(Int64),
    end_index_in_line    Nullable(Int64),
    match_status         Nullable(String),

    column_name          String,
    count_matched_rows   Int64,
    count_total_rows     Int64,
    count_matches        Int64
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, rule_id, received_at, scan_id, result_index)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- Process groups: a host's processes grouped by name.
--
-- Every agent posts {"resources": {"processes": {"snaps": [...]}}} to /intake/
-- with its host metadata (the resources check, kept from Agent 5). It is the
-- one view of a host's processes that needs no process-agent: one row per name
-- with the group's memory and how many processes are in it. A snapshot arrives
-- a few times an hour, not every ten seconds; the live process list is
-- `processes`.
CREATE TABLE IF NOT EXISTS process_groups
(
    tenant_id     LowCardinality(String),
    -- The agent's clock when it took the snapshot, to the second.
    timestamp     DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    host          LowCardinality(String),
    name          LowCardinality(String),
    usernames     Array(LowCardinality(String)),
    process_count UInt32,
    -- The agent sends a whole number here and, as of Agent 7.84, always 0.
    cpu_pct       Float64,
    mem_pct       Float64 CODEC(Gorilla, ZSTD(1)),
    vms           UInt64 CODEC(T64, ZSTD(1)),
    rss           UInt64 CODEC(T64, ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, name)
TTL timestamp + INTERVAL 30 DAY;
