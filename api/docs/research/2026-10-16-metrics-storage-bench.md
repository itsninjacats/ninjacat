# Metrics storage: what the tables cost and how tags are searched (2026-10-16)

Two hours of real points from ninjapaw (2026-10-16 11:00–13:00 UTC: 20,789,378 points,
48,251 series, 2,539 metric names, 13 tag keys per point on average, 22 at p90), copied into a
local ClickHouse 26.8.6.5 and loaded into variants of `metrics` and `metrics_1m`. Every table
merged to one part (`OPTIMIZE FINAL`). Times are the median of 7 warm runs, from
`system.query_log`. The scripts are not kept; the queries below are enough to repeat it.

## Findings

1. **Searching the `tags` map is the slow part, not storing it.** `tags['k']` decompresses every
   key and value of the row before it finds `k`: the column is 29 MiB on disk and 2.9 GiB
   decompressed. A filter on a tag costs 10–25× the time and 15–80× the memory of the same filter
   on a column of its own.
2. **The lifted columns (`env`, `service`, `kube_namespace`, `kube_deployment`, `pod_name`) are
   never read.** The engine filters and groups through `tags` only (`Engine/MetricQuery/Filter.fs`,
   `Compile.fs`). `pod_name` is a plain `String`: 25% of the table. They also hold only the first
   value of a tag, which contradicts decision 0001 (a tag is a multiset).
3. **The rollup stores its tags as plain strings.** `any()` drops a `LowCardinality` nested in a
   `Map` (still true in 26.8), so `metrics_1m.tags` is a `Map(String, Array(String))`: 57% of the
   rollup. A plain column with `allow_dimensions_outside_sorting_key = 1` keeps `LowCardinality`
   and is safe, because the tags follow from `series_id`.
4. **Three rollup columns are not needed.** `value_avg` is `value_sum / points`; `value_events` is
   `value_sum × interval` per minute; `value_last` is never read.
5. **The sort order scatters a series' points.** `ORDER BY (tenant_id, metric, host, timestamp)`
   interleaves the series of a host, so neither the tags nor Gorilla compress well. Putting
   `series_id` before `timestamp` makes the table 3.6× smaller, but a query for the last minutes
   then reads the whole day of the metric. A 15-minute bucket before the series keeps both — if
   the query states the bucket: ClickHouse does not derive `toStartOfFifteenMinutes(timestamp)`
   from a condition on `timestamp` (checked with `EXPLAIN indexes = 1`).
6. **Map buckets do not help here.** `map_serialization_version = 'with_buckets'` reads one bucket
   instead of the whole map, but with 13 keys per row there are 3–4 buckets: memory halves,
   time barely moves, the column grows.
7. **The `JSON` type gives every tag a column of its own, automatically**, up to
   `max_dynamic_paths`. Any tag is then as fast as a lifted column, with no list of tags chosen
   in advance. The cost is size: 4× the `Map` in the same order. Keys with dots need
   `json_type_escape_dots_in_keys = 1` at insert (29% of rows have such a key).

## Size

`metrics` variants, all columns:

| variant | size | vs today |
|---|---|---|
| today: `ORDER BY (…, host, timestamp)`, lifted columns as deployed | 106 MiB | 1× |
| no lifted columns, `ORDER BY (…, host, series_id, timestamp)` | 29.7 MiB | 3.6× smaller |
| same, `ORDER BY (…, toStartOfHour(timestamp), host, series_id, timestamp)` | 31.7 MiB | 3.3× |
| same, `ORDER BY (…, toStartOfFifteenMinutes(timestamp), host, series_id, timestamp)` | 40.2 MiB | 2.6× |
| `tags` as `JSON`, today's order (only the key columns and value) | 253 MiB | 2.4× larger |
| `tags` as `JSON`, 15-minute order (only the key columns and value) | 72 MiB | 1.5× smaller |

`pod_name` alone, 20.8 M rows:

| type | size |
|---|---|
| `String` (today) | 25.0 MiB |
| `String CODEC(ZSTD(3))` | 12.0 MiB |
| `LowCardinality(String)` | 2.1 MiB |
| `LowCardinality(String) CODEC(ZSTD(3))` | 1.2 MiB |
| `Array(LowCardinality(String)) CODEC(ZSTD(1))` | 2.5 MiB |

`metrics_1m` variants (5,457,272 rows):

| variant | size |
|---|---|
| r1: today | 124.7 MiB |
| r2: without `value_avg`, `value_events`, `value_last` | 94.3 MiB |
| r3: r2 with `SimpleAggregateFunction(min/max/sum)` + Gorilla, `points` as `SimpleAggregateFunction(sum, UInt64)` | 89.7 MiB |
| r3lc: r3 with `tags Map(LowCardinality(String), Array(LowCardinality(String)))` as a plain column | **21.0 MiB** |
| r4: r3 without tags, plus a series table (`ReplacingMergeTree`, one row per series) | 18.6 + 0.8 MiB |

## Search: a tag in the map, in a column, in JSON

`M` = `kubernetes_state.pod.ready` (600 series), namespace `longhorn-system`, one pod;
grouping over `containerd.blkio.serviced_recursive` (836 series, 115 pods).

| query | `has(tags['k'], v)` | lifted column | `Array(LowCardinality)` column | map with buckets | `JSON` |
|---|---|---|---|---|---|
| metric + namespace | 46–66 ms, 85–92 MiB | 5 ms, 3 MiB | 6 ms, 3 MiB | 65 ms, 45 MiB | 8–13 ms, 14–26 MiB |
| namespace, every metric | 1500–1900 ms, 230–270 MiB | 38 ms, 3 MiB | 63 ms, 3 MiB | 1389 ms, 70 MiB | 120 ms, 35–41 MiB |
| pod, every metric | 240–590 ms, 110–155 MiB | 11–22 ms, 3–13 MiB | 17 ms, 3 MiB | 187 ms, 61 MiB | 39–73 ms, 41–57 MiB |
| group by pod | 70–82 ms, 42 MiB | 7 ms, 13 MiB | 7 ms, 7 MiB | 52 ms, 12 MiB | 9–11 ms, 10–19 MiB |

Rollup, the engine's shape (inner per-series aggregate, outer group), 60 s steps over 2 h:

| query | r1 (today) | r3lc | r4 + series table (JOIN) |
|---|---|---|---|
| `avg:M{kube_namespace:longhorn-system} by {pod_name}` | 52 ms, 20 MiB | 80 ms, 8 MiB | 36 ms, 12 MiB |
| `sum:containerd.blkio.serviced_recursive by {kube_namespace}` | 89 ms, 37 MiB | 78 ms, 24 MiB | 174 ms, 56 MiB |

The series table wins a narrow filter and loses a wide grouping, where the JOIN carries every
series.

## Time windows and the sort order

`containerd.blkio.serviced_recursive` grouped by pod, ending at 13:00. The bucketed orders are
queried with the bucket condition added (`toStartOfFifteenMinutes(timestamp) >= …(from) AND … <=
…(to)`); without it they read every granule of the metric.

| window | today's order | 1 h bucket | 15 min bucket | series order, no bucket |
|---|---|---|---|---|
| 5 min | 41 ms, 57 k rows | 58 ms, 205 k rows | 34 ms, 57 k rows | 71 ms, 408 k rows |
| 15 min | 59 ms, 90 k rows | 57 ms, 205 k rows | 32 ms, 57 k rows | 74 ms, 408 k rows |
| 60 min | 60 ms, 238 k rows | 57 ms, 205 k rows | 61 ms, 213 k rows | — |
| 120 min | 78 ms, 410 k rows | 78 ms, 410 k rows | 102 ms, 414 k rows | 75 ms, 408 k rows |

With a full day in a partition the series order without a bucket would read ~12× more than the
2-hour sample shows; the bucketed orders would not.

## Queries

```sql
-- the filter shapes, per variant
SELECT count(), avg(value) FROM t WHERE tenant_id = 'default' AND metric = M AND has(tags['kube_namespace'], 'longhorn-system');
SELECT count(), avg(value) FROM t WHERE tenant_id = 'default' AND has(kube_namespace, 'longhorn-system');      -- Array column
SELECT count(), avg(value) FROM t WHERE tenant_id = 'default' AND has(tagsj.kube_namespace.:`Array(Nullable(String))`, 'longhorn-system');  -- JSON
SELECT g, avg(value) FROM t WHERE tenant_id = 'default' AND metric = B GROUP BY arrayJoin(tags['pod_name']) AS g;

-- JSON from the map
INSERT INTO t SELECT …, CAST(toJSONString(tags), 'JSON') FROM metrics SETTINGS json_type_escape_dots_in_keys = 1;

-- what a column costs
SELECT name, formatReadableSize(data_compressed_bytes), formatReadableSize(data_uncompressed_bytes)
FROM system.columns WHERE database = 'bench' AND table = 't';

-- whether the key prunes
EXPLAIN indexes = 1 SELECT count() FROM t WHERE … AND timestamp >= … AND toStartOfFifteenMinutes(timestamp) >= toStartOfFifteenMinutes(…);
```
