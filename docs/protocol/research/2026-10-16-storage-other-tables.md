# Storage of logs, traces and the other tables: what they cost and what to change (2026-10-16)

The same method as `2026-10-16-metrics-storage-bench.md`, applied to every other non-empty table:
real rows from ninjapaw copied into a local ClickHouse 26.8.6.5 (the version prod runs), loaded
into variants, each merged to one part, times the median of 7 warm runs from `system.query_log`
with `use_query_condition_cache = 0` (the panel's time bounds move with "now", so that cache would
rarely hit; left on, it hides the cost of the filters). Logs: 2 h of `logs` (1.10 M rows). Traces:
**synthetic**, because ninjapaw has no APM sender. Other tables: 2–8 h samples per table plus
per-column sizes from prod. Prod was only read. Under 10 ms on a laptop means "free". The question
was whether the metrics findings (the tag map is slow to search, the sort order scatters a stream,
strings that should be `LowCardinality`) hold elsewhere: the map finding does everywhere, the
sort-order finding only for snapshot tables, not for logs.

## Logs

The panel's only telemetry read besides metrics: `/app/logs` runs the newest 200 rows, a count and
three facets over the last hour (`Engine/Panel.fs:187-250`). Tag filters go through the metrics
`Filter.fs` (`has(tags[k], v)` and friends).

- **91% of the rows are NinjaCat's own ClickHouse at debug level**, all stored as `status:error`
  (the agent tags stderr as error). They are 93% of the table's bytes and 88% of the token index,
  and the hard-to-compress part (ids): message ratio about 10× against 72× for every other service.
  The chart sets no `logger/level`, so the operator's default (debug) applies. Mostly merges and
  `system.*` housekeeping, not a feedback loop through the `logs` table.
- **Every query is cheap except a tag filter**: 160–230 ms and 20–60 MiB per hour, 10–60× the
  same filter on a column (rows: 15 ms; count: 3 ms; facets: 5 ms; text search 8–75 ms). A 24 h
  window would take 4–5 s. Same cause as in metrics: `tags` is 1.5 MiB on disk and 252 MiB
  decompressed for 2 h (`Map` with `Array` values: an 8-byte offset per key per row, 23 keys).
- **Types are already right.** Low-cardinality strings are `LowCardinality`; `message` is unique
  per row. `attributes` and `timestamp_source` are unread and cost nothing.
- **The sort order does not matter here**, unlike metrics: a log stream is almost
  `(service, host)` already (61 pairs, 145 tag sets). Four orders, bucket-first included, changed
  the size by under 2% and none made the newest-200 read in order.

Tags, the whole table in each form (count per hour of logs):

| tags as | on disk | `kube_namespace` exact / `pod_name` / pod wildcard | note |
|---|---|---|---|
| `Map(LC, Array(LC))` (today) | 1.51 MiB | 160–230 / 230 / 222 ms | |
| **`Array(LowCardinality(String))` of `k:v` + `bloom_filter(0.01)` GRANULARITY 1** | **63 KiB** | **7–10 / 18 / 129 ms** | index skips 134–138 of 142 granules; wildcard cannot use it |
| `JSON` | 4.70 MiB | 8–9 / 12 / 11 ms | 3× the map, 75× the list |
| streams table (`stream_id` in the key, tags in a 145-row side table) | 41 + 34 KiB | 4–6 for every shape | a second write path, a join for the 200 rows |

Free text, rare token: today's `tokenbf_v1 GRANULARITY 4` 15–33 ms; at `GRANULARITY 1` 8–11 ms.
An index on `lower(message)` plus `positionCaseInsensitive` gives Datadog's case-insensitive search
at the same speed; today's search is case-sensitive. The index grows from 1.1 to 4.3 MiB per 2 h
(18% of `message`). The `text` index is 2× the column on these logs, 4× on the others, and no faster
for this query shape; `ngrambf` prunes less than `tokenbf`. Mid-frequency tokens cost the same on
every index (the matches are large messages). `message` at `ZSTD(6)` is 13% smaller for 3.6× the
compression CPU; reads do not change.

## Traces

**Every number here is synthetic.** ninjapaw has no parts in `spans`, `apm_stats` or any `dsm_*`
table, and the golden fixtures hold one-or-two-field test spans. The sample is 3.02 M generated
v0.4-shaped spans (110 k traces, 300 services, 4.5 k resources, 15 `meta` keys on average, 1.4%
errors over 6 h). Nothing reads these tables yet, so the DDL was judged against what a trace UI
needs: a trace by id, latest spans of a service, errors by resource, a `meta` value filter, p95 by
resource, a service overview. The OTel ClickHouse exporter, HyperDX and SigNoz schemas were read
for comparison: all three keep attribute keys `LowCardinality`, lift the hot attributes into
columns, set `ttl_only_drop_parts`, and keep a `trace_id → time` table; NinjaCat's `UInt64` ids
(8 B against 32+ B strings) are the one thing ahead of them.

- **Likely correctness bug.** `trace_id_high` is read per span from `meta['_dd.p.tid']`
  (`Intake/Routers/Trace.fs:177`), but dd-trace-go sets that tag only on the first span of a chunk
  and the agent does not copy it. Every other span gets `trace_id_high = 0`, so `trace_id_hex`
  differs within one trace: a lookup by the 32-digit id returned 14 of 33 spans. Read from the
  tracer's source, not yet confirmed with a real payload.
- **Today's DDL costs 88.7 B/span.** `meta` is 30% of it (`Map(String, String)`: plain-string
  keys, `error.stack` beside `http.url`); `trace_id_hex` is a stored 8.6 B/span copy of two
  integers; `T64` makes `trace_id` 2.7× larger than plain `ZSTD(1)` (it hides the repeat between
  neighbouring spans); `container_id`, `runtime_id` and `resource` are plain `String` though one
  value per process or normalized; `idx_span_id` is 77% of skip-index bytes for a query nobody
  makes. The key comment says `trace_id` "keeps one trace's spans together"; it never breaks a tie
  after a nanosecond `start`.
- **Filtering on a `meta` key is 15–40× slower and 50–100× hungrier than a column**, the metrics
  finding again: `http.status_code = 503` over 1 h takes 282 ms / 412 MiB; a value needle 294 ms.
  Unlike metrics, **`JSON` is smaller than the map** here (36 against 77 MiB).
- **Time-first key** `(tenant_id, toStartOfHour(start), service, name, start)` cuts cross-service
  reads 6× in a 6 h partition (about 24× for a day) for +3% size; the query must state the bucket.

| variant | B/span | `status = 503`, 1 h | value needle, 6 h | service overview |
|---|---|---|---|---|
| today | 88.7 | 282 ms, 412 MiB | 294 ms | 80 ms, 2.9 M rows |
| `meta` keys LC, LC strings, `trace_id_hex` ALIAS | 68.4 | 298 ms | 232 ms | 63 ms |
| + lifted columns, map-values bloom index, codecs, time-first key, `ttl_only_drop_parts` (`tP`) | 54.9 | 6 ms | 14 ms (index) | 9 ms, 0.5 M rows |
| same with `meta JSON` (`tPj`) | **47.5** | 7 ms | 10 ms (no index, one path) | 13 ms |

A `trace_id → (min start, max end)` lookup table costs about 1 B/span and limits the bloom search to
the trace's own day: 15× less index work over 14 daily partitions, growing with volume.

`apm_stats` (static review, no data): the right source for hits, errors and latency, since `spans`
is sampled. Its key is sound; per-process strings, `http_endpoint` and the tag arrays should be
`LowCardinality`. Each sketch is stored twice (raw protobuf and decoded bins), and the bins lack
the index mapping needed to turn them into durations in SQL. `dsm_*`: the same `LowCardinality`
fixes, low volume.

## Other tables

**No code reads any of them** (the query host serves `/internal/metrics/*` and `/internal/logs/*`
only), so the reads below are the ones the DDL comments say each table is for. Four tables matter
for size; the rest are under 0.5 MiB a day each. Empty columns cost nothing (sparse serialization).

| table | MiB/day | TTL | at TTL | where the bytes go |
|---|---|---|---|---|
| processes | ~155 | 7 d | ~1.09 GiB | per-frame `request_id` + `snapshot_id` copied onto every row; `args` repeated every 10 s; Gorilla on Float32 |
| k8s_manifests | ~107 | 7 d | ~750 MiB | **49 identical rows for every resource version** stored |
| containers | ~105 | 7 d | ~730 MiB | ten Float32 Gorilla columns larger than uncompressed; string columns with 75–600 values |
| dbm_events | ~50 | 30 d | ~1.46 GiB | the `event` JSON body |
| k8s_resources | ~15 | 30 d | ~450 MiB | `object` changes every cycle only because the agent sends labels and annotations in Go-map order |

- **Snapshot tables are where the metrics sort-order finding holds**, with a condition. An hour
  bucket before the pid or container id makes `processes` 3.6× and `containers` 2× smaller (with
  the type fixes) — but only once the per-frame ids leave the row: in pid order they become 16 B of
  random bytes per row, more than the rest of the table.
- **processes:** `cmdline` is always `arrayStringConcat(args, ' ')`; as `LowCardinality(String)`
  (18 k values a day) a cmdline `ILIKE` over 4 h drops from 51 ms / 95 MiB to 8 ms / 10 MiB, since it
  runs over the dictionary. One process over 4 h: 14 → 5 ms. A host's list at T: unchanged.
- **Gorilla hurts noisy Float32 gauges** by 15–50% against `ZSTD(1)` (`cpu_pct`, container rates).
  `metrics.value` was out of scope and may deserve the same check.
- **Bigger compression blocks** (`min_compress_block_size = 4 MiB`, `max = 8 MiB`) shrink blob
  tables in place, with no code: `k8s_manifests` 3.5×, `dbm_events` 1.6× (1.9× with `event` at
  `ZSTD(6)`). A point read then costs +2–5 ms and +10–18 MiB. Not for numeric tables read by row.
- **Tag maps everywhere** have bloom indexes nobody uses; a `kube_namespace` filter on
  `containers` takes 66–71 ms in every variant. Lift the tags a container view filters on, if one
  ever does.

## What to change

Ranked by gain against effort. Item 5 holds every change that needs a table rebuilt (a new table in
a new migration: `ORDER BY` and engine cannot change in place, and applied migrations are
immutable), so each table is rebuilt once. Everything else is in place.

1. **Stop storing NinjaCat's own ClickHouse debug logs.** −90% of log rows, −93% of their bytes,
   −88% of the token index, and `status:error` stops being mostly noise. No schema change.
   - `helm/ninjacat/templates/clickhouse.yaml:23`: a `configuration.settings` with
     `logger/level: information`, exposed in values. Optionally an agent exclude rule for the
     container.
   - Risk: none to data; debug output is still available by raising the level.
2. **Log tags as a flat `Array(LowCardinality(String))` of `k:v`, with a bloom index.** Tag filters
   15–30× faster (about 200 → 7–18 ms per hour of logs), 5–40× less memory, the column 24× smaller.
   - Migration: `ADD COLUMN tag_list … MATERIALIZED arrayFlatten(arrayMap(…, mapKeys(tags),
     mapValues(tags)))`, `ADD INDEX idx_tags tag_list TYPE bloom_filter(0.01) GRANULARITY 1`,
     `MATERIALIZE COLUMN`, `MATERIALIZE INDEX`. Drop the map later, once the writer writes the list.
   - Code: a logs-only filter beside `Engine/MetricQuery/Filter.fs:20-45` (`has(tag_list, 'k:v')`,
     `hasAny`, wildcard `arrayExists(… LIKE 'k:…')`, bare key `has(tag_list, 'k') OR arrayExists(x
     -> startsWith(x, 'k:'), …)`); `Engine/Panel.fs:187-228` (`logsWhere`, the rows SELECT);
     `Server/PanelHandlers.fs:138` (`tagMap` splits each entry at the first colon); later
     `Storage/Rows/Logs.fs:14` and `Intake/Routers/Logs.fs:182` (keep the `splitDDTags` list);
     three logs fixtures edited with an `edited` key.
   - Risks: `Filter.fs` also serves `metrics` and must not change. A filter that switches before
     the backfill silently misses older rows. Wildcards cannot use the index (still 2× faster).
     The streams table is faster for every shape but adds a write path; keep it in reserve.
3. **One in-place storage migration**, no code:
   - `k8s_manifests`, `dbm_events`: `MODIFY SETTING min_compress_block_size = 4194304,
     max_compress_block_size = 8388608`; `dbm_events.event CODEC(ZSTD(6))`. About −540 and −720 MiB
     at TTL. (If item 5b goes in first, put the block settings in the new manifests table instead.)
   - `ttl_only_drop_parts = 1` on every daily-partitioned table with a TTL (`logs`, `spans`,
     `apm_stats`, `dsm_*`, the snapshot tables): whole days dropped instead of rewritten by TTL
     merges. Gain in merge I/O, not measured.
   - Risks: a point read decompresses up to 8 MiB per column (+2–5 ms). `MODIFY COLUMN` rewrites
     the column; run one table at a time.
4. **Fix `trace_id_high` for v0.4.** Correctness.
   - `Intake/Routers/Trace.fs:198-227` (`v04TracerRows`): find `_dd.p.tid` within each chunk and
     give it to every span, instead of reading it per span at `:177`. `TestHandleTracesStoresSpansAndKeepsResponse`
     and `TestPortTraces` edited if their rows change.
   - Risk: the finding is from dd-trace-go's source. Record a real v0.4 payload first (the .NET
     tracer in `compose.self.yaml`) and check the other tracers.
5. **Rebuild four tables, once each** (`INSERT … SELECT` and `EXCHANGE TABLES`, or recreate and
   lose up to 7 days of unread snapshots on ninjapaw). Every future reader must state the time
   bucket: ClickHouse does not derive `toStartOfHour(t)` from a range on `t`.
   - a. **processes** (−110 MiB/day, −770 MiB at TTL; history 3× and cmdline search 6× faster):
     drop `request_id`, `snapshot_id`; `LowCardinality` for `exe`, `cwd`, `container_id`,
     `host_uuid`, `generated_service_name`, `dd_service_name`, `cmdline`; `Array(LowCardinality)`
     for `args`, `process_context`; Float32 `CODEC(ZSTD(1))`; `ORDER BY (tenant_id, host,
     toStartOfHour(timestamp), pid, timestamp)`. Code: `Storage/Rows/Process.fs:141-162` and the
     row type; six fixtures with `snapshot_id` edited. Risk: the join to `process_snapshots`
     becomes `(host, timestamp = toDateTime(received_at), group_id)`, unique only while a host
     sends at most one frame per second per group; check before relying on it. Dictionaries grow
     on large or churny fleets.
   - b. **k8s_manifests** (−103 MiB/day, −720 MiB at TTL; kind-wide "latest" 3× faster):
     `ReplacingMergeTree(collected_at)` keyed `(tenant_id, cluster_id, kind, uid,
     resource_version)`, big blocks. Writer unchanged. Risks: deduplication is eventual and per
     day, so readers use `argMax` or `LIMIT 1 BY`; `collected_at` becomes "last seen", and only
     `k8s_resources` still says what existed at T.
   - c. **containers** (−52 MiB/day, −370 MiB at TTL; history 2× faster, host list at T 9 → 15 ms):
     the processes recipe, `ORDER BY (tenant_id, host, toStartOfHour(timestamp), id, timestamp)`.
     Code: `Storage/Rows/Process.fs:813-820`. A 15-minute bucket keeps the host list at 8 ms for
     +2% size. The types alone, in place, give −21 MiB/day.
   - d. **spans**, before an APM sender is pointed at it (nothing to lose today; 1.6–1.9× smaller,
     `meta` filters 15–40× faster): `meta` as `JSON` (smallest; dotted keys need
     `json_type_escape_dots_in_keys = 1` and read back escaped) or `Map(LowCardinality(String),
     String)` with lifted columns and a `mapValues` bloom index (the only form where a value needle
     skips granules); lift `top_level` and one `env`/`version`/`component` for v0.4 and idx alike;
     `trace_id_hex` as ALIAS; `ZSTD(1)` for the ids, `Delta` for `start` and `received_at`;
     `LowCardinality` for `container_id`, `runtime_id`, `resource`; drop `idx_span_id`;
     `ORDER BY (tenant_id, toStartOfHour(start), service, name, start)`. Code:
     `Storage/Rows/Traces.fs:59-190`, `Intake/Routers/Trace.fs:165-245` (`v04SpanRow`,
     `idxSpanRow`). Fold `apm_stats` and `dsm_*`'s `LowCardinality` fixes into the same migration.
     Risks: synthetic data; a tracer that does not normalize resources erases the `LowCardinality`
     gain; heavy traffic interleaves a trace's spans and shrinks the `trace_id` codec gain.
6. **Case-insensitive log search** (Datadog's behaviour; rare tokens 2–3× faster):
   `ADD INDEX idx_message_lc lower(message) TYPE tokenbf_v1(32768, 3, 0) GRANULARITY 1`,
   `MATERIALIZE INDEX`, `DROP INDEX idx_message`; `Engine/Panel.fs:199-205` lowercases the tokens
   and emits `hasToken(lower(message), …)` and `positionCaseInsensitive`. Risk: the index is used
   only when the query repeats `lower(message)` exactly; 4× the index size (less after item 1).
7. **k8s_resources: sort labels, annotations and selectors before storing `object`** (−120 MiB at
   TTL, with big blocks). `Intake/Routers/Kubeops.fs:532`; fixtures edited, since what the route
   stores changes (the order has no meaning in Kubernetes).
8. **Later, with the first reader that needs them:** a trace lookup table (an
   `AggregatingMergeTree` filled by an MV, backfillable); drop `process_snapshots.request_id`
   (`Storage/Rows/Process.fs:683-698`, −25 MiB at TTL); decide whether `apm_stats` keeps the raw
   sketch or the bins plus their mapping.

Not recommended, from the measurements: changing the `logs` key (±2%), `logs.tags` as `JSON`
(3× the map), the `text` or `ngrambf` index on `message`, a minmax index on `logs.timestamp`
(revisit for day-long windows), `message` at `ZSTD(6)` (small, and smaller still after item 1),
`LowCardinality` for `http.url`-like values (too many distinct).

Unmeasured, existing: `logs` is `PARTITION BY toDate(timestamp)` on the sender's clock; a batch
with stray old or future timestamps can create many partitions or hit
`max_partitions_per_insert_block`. Prod has two partitions, so it has not happened yet.
