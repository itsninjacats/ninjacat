-- 0005: the metrics tables in the layout chosen by the 2026-10-16 benchmark
-- (api/docs/research/2026-10-16-metrics-storage-bench.md).
--
-- `metrics` is sorted by a 15-minute bucket before the series, so a series'
-- points sit together (2.6x smaller) while a short window still reads few
-- granules, provided the query states the bucket, which ClickHouse does not
-- derive from a condition on `timestamp` (the engine adds it). The lifted
-- tags hold every value of the tag: a tag is a multiset, and tags['k'][1]
-- kept only the first. A sort order can only change in a new table, so the
-- old one's points are not carried over.
--
-- The rollup keeps sums, minima, maxima, counts and a rate's events as plain
-- values (an average is sum / points, and the last value was never read), and
-- its tags as a plain column, which keeps LowCardinality where any() drops it.
--
-- `metric_tag_keys` lists, per hour, every metric and its tag keys; a metric
-- with no tags is listed under the key ''.
--
-- A view follows table NAMES: after EXCHANGE it fires on the table now called
-- `metrics`. So the old view goes first and the new ones come after both
-- exchanges; a point inserted in between is missing from the rollup, where
-- one counted twice would be wrong forever. No IF NOT EXISTS: a file applied
-- in part must fail rather than exchange the tables back.
--
-- The old tables are kept as metrics_old and metrics_1m_old. Copying the old
-- rollup takes minutes, too long for a start-up; it can be run by hand, once:
--   INSERT INTO metrics_1m (tenant_id, minute, metric, host, series_id, metric_type, interval, tags,
--                           points, value_min, value_max, value_sum, value_events)
--   SELECT tenant_id, minute, metric, host, series_id, metric_type, interval, tags,
--          finalizeAggregation(points), finalizeAggregation(value_min), finalizeAggregation(value_max),
--          finalizeAggregation(value_sum), finalizeAggregation(value_events)
--   FROM metrics_1m_old
--
-- The TTLs below are defaults; the migrating process sets them from
-- NINJACAT_METRICS_RETENTION_DAYS and NINJACAT_METRICS_ROLLUP_RETENTION_DAYS.

CREATE TABLE metrics_next
(
    tenant_id    LowCardinality(String),
    timestamp    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    metric       LowCardinality(String),
    host         LowCardinality(String),
    metric_type  LowCardinality(String),
    source_type  LowCardinality(String),
    unit         LowCardinality(String),
    interval     UInt32 CODEC(DoubleDelta, ZSTD(1)),
    value        Float64 CODEC(Gorilla, ZSTD(1)),
    env             Array(LowCardinality(String)) MATERIALIZED tags['env'] CODEC(ZSTD(1)),
    service         Array(LowCardinality(String)) MATERIALIZED tags['service'] CODEC(ZSTD(1)),
    kube_namespace  Array(LowCardinality(String)) MATERIALIZED tags['kube_namespace'] CODEC(ZSTD(1)),
    kube_deployment Array(LowCardinality(String)) MATERIALIZED tags['kube_deployment'] CODEC(ZSTD(1)),
    pod_name        Array(LowCardinality(String)) MATERIALIZED tags['pod_name'] CODEC(ZSTD(1)),
    tags         Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- Unchanged from 0001: the rollup and every stored series_id depend on it.
    series_id    UInt64 MATERIALIZED sipHash64(tenant_id, metric, host, mapSort(mapApply((k, v) -> (k, arraySort(v)), tags))),
    origin_product  UInt32 CODEC(T64, ZSTD(1)),
    origin_category UInt32 CODEC(T64, ZSTD(1)),
    origin_service  UInt32 CODEC(T64, ZSTD(1)),
    resources    Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    extra        Map(String, String) CODEC(ZSTD(3)),
    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, metric, toStartOfFifteenMinutes(timestamp), host, series_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 30 DAY;

CREATE TABLE metrics_1m_next
(
    tenant_id    LowCardinality(String),
    minute       DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    metric       LowCardinality(String),
    host         LowCardinality(String),
    series_id    UInt64,
    metric_type  SimpleAggregateFunction(any, LowCardinality(String)),
    interval     SimpleAggregateFunction(max, UInt32),
    -- Follows from series_id, so whichever row a merge keeps is right.
    tags         Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    points       SimpleAggregateFunction(sum, UInt64) CODEC(T64, ZSTD(1)),
    value_min    SimpleAggregateFunction(min, Float64) CODEC(Gorilla, ZSTD(1)),
    value_max    SimpleAggregateFunction(max, Float64) CODEC(Gorilla, ZSTD(1)),
    value_sum    SimpleAggregateFunction(sum, Float64) CODEC(Gorilla, ZSTD(1)),
    -- A RATE's events: value × interval, the interval floored at 1 s as the
    -- engine floors it for as_count() on raw points (Compile.fs).
    value_events SimpleAggregateFunction(sum, Float64) CODEC(Gorilla, ZSTD(1)),
    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(minute)
ORDER BY (tenant_id, metric, host, series_id, minute)
TTL minute + INTERVAL 400 DAY
SETTINGS allow_dimensions_outside_sorting_key = 1;

CREATE TABLE metric_tag_keys
(
    tenant_id LowCardinality(String),
    metric    LowCardinality(String),
    hour      DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    key       LowCardinality(String),
    bare      SimpleAggregateFunction(max, UInt8),
    points    SimpleAggregateFunction(sum, UInt64)
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(hour)
ORDER BY (tenant_id, metric, hour, key)
TTL hour + INTERVAL 400 DAY;

DROP VIEW IF EXISTS metrics_1m_mv;

EXCHANGE TABLES metrics AND metrics_next;

EXCHANGE TABLES metrics_1m AND metrics_1m_next;

CREATE MATERIALIZED VIEW metrics_1m_mv
TO metrics_1m AS
SELECT
    tenant_id,
    toStartOfMinute(timestamp) AS minute,
    metric,
    host,
    series_id,
    any(metric_type) AS metric_type,
    max(interval)    AS interval,
    any(tags)        AS tags,
    count()          AS points,
    min(value)       AS value_min,
    max(value)       AS value_max,
    sum(value)       AS value_sum,
    -- Qualified: a bare `interval` here would be the alias above, max(interval).
    sum(value * greatest(metrics.interval, 1)) AS value_events
FROM metrics
GROUP BY tenant_id, minute, metric, host, series_id;

CREATE MATERIALIZED VIEW metric_tag_keys_mv
TO metric_tag_keys AS
SELECT
    tenant_id,
    metric,
    toStartOfHour(timestamp) AS hour,
    k                        AS key,
    max(toUInt8(has(v, ''))) AS bare,
    count()                  AS points
FROM metrics
ARRAY JOIN arrayPushFront(mapKeys(tags), '') AS k, arrayPushFront(mapValues(tags), []) AS v
GROUP BY tenant_id, metric, hour, key;

RENAME TABLE metrics_next TO metrics_old;

RENAME TABLE metrics_1m_next TO metrics_1m_old;
