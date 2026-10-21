-- 0002: the one-minute rollup gets the tags, so it answers the same queries as
-- `metrics`.
--
-- The rollup of 0001 had no tags (it waited for a series table that was never
-- built), so it could not answer a filtered or grouped query and nothing read
-- it. It is rebuilt here and refilled from the raw points still kept; the old
-- rows could not be given their tags.
--
-- The retention of both tables is configurable, so the TTLs below are only
-- the defaults: the migrating process sets them after the migrations
-- (NINJACAT_METRICS_RETENTION_DAYS, NINJACAT_METRICS_ROLLUP_RETENTION_DAYS).

DROP VIEW IF EXISTS metrics_1m_mv;
DROP TABLE IF EXISTS metrics_1m;

-- The columns a filter or a group reads are named as in `metrics`, so a tag
-- filter compiles the same for both tables.
CREATE TABLE metrics_1m
(
    tenant_id   LowCardinality(String),
    minute      DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    metric      LowCardinality(String),
    host        LowCardinality(String),
    series_id   UInt64,

    -- All three follow from series_id, and a series' rows sit next to each
    -- other, so the repeated map compresses to almost nothing. Its keys and
    -- values are plain strings: any() drops a LowCardinality nested in a map,
    -- and ClickHouse refuses a column type the function does not return.
    metric_type SimpleAggregateFunction(any, LowCardinality(String)),
    interval    SimpleAggregateFunction(max, UInt32),
    tags        SimpleAggregateFunction(any, Map(String, Array(String))) CODEC(ZSTD(3)),

    points      AggregateFunction(count, Float64),
    value_avg   AggregateFunction(avg, Float64),
    value_min   AggregateFunction(min, Float64),
    value_max   AggregateFunction(max, Float64),
    value_sum   AggregateFunction(sum, Float64),
    value_last  AggregateFunction(argMax, Float64, DateTime64(3, 'UTC')),
    -- A RATE's events: value × interval, the interval floored at 1 s as the
    -- engine floors it for as_count() on raw points (Compile.fs).
    value_events AggregateFunction(sum, Float64),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = AggregatingMergeTree
PARTITION BY toYYYYMM(minute)
ORDER BY (tenant_id, metric, host, series_id, minute)
TTL minute + INTERVAL 400 DAY;

-- Filled before the view exists: a point inserted in between is missing from
-- the rollup, where one counted twice would be wrong forever.
INSERT INTO metrics_1m
SELECT
    tenant_id,
    toStartOfMinute(timestamp) AS minute,
    metric,
    host,
    series_id,
    any(metric_type),
    max(interval),
    any(tags),
    countState(value),
    avgState(value),
    minState(value),
    maxState(value),
    sumState(value),
    argMaxState(value, timestamp),
    sumState(value * greatest(interval, 1))
FROM metrics
GROUP BY tenant_id, minute, metric, host, series_id;

CREATE MATERIALIZED VIEW metrics_1m_mv
TO metrics_1m AS
SELECT
    tenant_id,
    toStartOfMinute(timestamp) AS minute,
    metric,
    host,
    series_id,
    any(metric_type)                        AS metric_type,
    max(interval)                           AS interval,
    any(tags)                               AS tags,
    countState(value)                       AS points,
    avgState(value)                         AS value_avg,
    minState(value)                         AS value_min,
    maxState(value)                         AS value_max,
    sumState(value)                         AS value_sum,
    argMaxState(value, timestamp)           AS value_last,
    -- Qualified: a bare `interval` here would be the alias above, max(interval).
    sumState(value * greatest(metrics.interval, 1)) AS value_events
FROM metrics
GROUP BY tenant_id, minute, metric, host, series_id;
