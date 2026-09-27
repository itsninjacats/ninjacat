-- series_id: one id per real series, whatever order its tags arrived in.
--
-- THE BUG. series_id was sipHash64(tenant_id, metric, host, tags), and the
-- hash of a Map depends on the order of its keys. The intake builds the tag
-- map from a Go map (intake/server.go, tagsToMultiMap), and Go randomises map
-- iteration on purpose — so the same series was written with its keys in a
-- different order from one payload to the next, and got a different id each
-- time. Measured on a dev database: 2 298 411 distinct ids for 34 420 real
-- series.
--
-- What that broke: anything aggregating per series. A space aggregation such
-- as `sum:` over hosts added up one real series several times (the F# query
-- service showed memory.used jumping between 1x, 2x and 3x its value), and
-- metrics_1m, keyed by series_id, held one row per bogus series per minute.
--
-- THE FIX is in the hash, not in the writer: sort the keys (mapSort) and the
-- values under each key (arraySort) before hashing. It holds for any writer,
-- including a future non-Go one, and needs no change on the Go side. Values
-- are sorted too because a tag is a multiset (docs/decisions/0001) — the same
-- two values in a different order are the same series.
--
-- No database qualifier, as in every migration: the runner connects to the
-- target database already.

ALTER TABLE metrics
    MODIFY COLUMN series_id UInt64
    MATERIALIZED sipHash64(tenant_id, metric, host, mapSort(mapApply((k, v) -> (k, arraySort(v)), tags)));

ALTER TABLE sketches
    MODIFY COLUMN series_id UInt64
    MATERIALIZED sipHash64(tenant_id, metric, host, mapSort(mapApply((k, v) -> (k, arraySort(v)), tags)));

-- Existing rows keep the id they were written with until recomputed. These
-- mutations rewrite parts in the background; nothing below waits on them,
-- because the metrics_1m rebuild computes the id itself.
ALTER TABLE metrics MATERIALIZE COLUMN series_id;

ALTER TABLE sketches MATERIALIZE COLUMN series_id;

-- metrics_1m has no tags, so its ids cannot be recomputed in place. Rebuild
-- it from the raw table for the span the raw table still covers (30 days of
-- TTL); minutes older than that have nothing left to rebuild from and stay
-- as they are.
--
-- mutations_sync = 2: the DELETE must be finished before the INSERT, or it
-- would run afterwards and delete the rebuilt rows too.
ALTER TABLE metrics_1m
    DELETE WHERE bucket >= (SELECT toStartOfMinute(min(timestamp)) FROM metrics)
    SETTINGS mutations_sync = 2;

-- Rows the intake writes between the DELETE and this INSERT reach metrics_1m
-- twice — once through metrics_1m_mv, once here. At startup the storage app
-- applies migrations before its writers flush, so there are none; with
-- `ninjacat migrate` running beside live ingest, a minute or two around the
-- migration may count double.
INSERT INTO metrics_1m
SELECT
    tenant_id,
    toStartOfMinute(timestamp) AS bucket,
    metric,
    host,
    sipHash64(tenant_id, metric, host, mapSort(mapApply((k, v) -> (k, arraySort(v)), tags))) AS series_id,
    countState(value),
    avgState(value),
    minState(value),
    maxState(value),
    sumState(value)
FROM metrics
GROUP BY tenant_id, bucket, metric, host, series_id;
