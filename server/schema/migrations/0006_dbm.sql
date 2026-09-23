-- Database Monitoring: dbm-metrics-intake.<site>, six tracks
-- (dbmmetrics, dbmactivity, databasequery, dbmmetadata, dbmhealth,
-- dbmcolumnstatistics), one table.
--
-- No CREATE DATABASE and no database qualifier, same as 0001/0002: the
-- runner applies every statement over a connection already opened against
-- the target database, and qualifying would break running against a
-- scratch one.
--
-- WHY ONE TABLE: every engine (Oracle, Postgres, MySQL, SQL Server, Mongo)
-- reports its rows under a different, engine-specific key (oracle_rows,
-- postgres_activity, mysql_rows, ...), and NONE of them has a Go type in
-- this module to decode into columns — the single in-repo producer (the
-- oracle corecheck) is //go:build oracle and excluded from the build, and
-- every other engine is emitted by Python integrations this workspace never
-- sees. A table per engine would mean inventing a schema for data we
-- genuinely cannot decode; the header of 0001_initial.sql's "one table per
-- payload shape, not per metric" rule applies here with "engine" standing
-- in for "metric". So the columns below are the envelope fields every track
-- agrees on (intake/router_dbm.go's dbmEnvelope), plus `event`, which keeps
-- the complete raw JSON of the event — the only lossless representation of
-- whatever engine-specific rows it carries, decodable later without a schema
-- migration the moment a Go type exists for one.
CREATE TABLE IF NOT EXISTS dbm_events
(
    tenant_id            LowCardinality(String),

    -- Arrival time. Always present, unlike `timestamp` below — every row
    -- gets one even when the wire sent no timestamp of its own.
    received_at          DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Which of the six routes this event arrived on. Bounded vocabulary,
    -- first after tenant_id in ORDER BY because "what is <track> actually
    -- sending" is the question this table exists to answer (raw_payloads'
    -- `intake` column plays the same role, same reasoning).
    track                LowCardinality(String),

    -- The envelope's own "timestamp" (unix ms on the wire). Nullable: an
    -- event that never sent one must stay absent, not become 1970-01-01 or
    -- get backfilled with received_at, which would silently invent data.
    -- Not DoubleDelta: events from many hosts and tracks interleave in
    -- arrival order, so consecutive rows are not a regular series the way
    -- metrics' per-(metric,host) timestamps are.
    timestamp            Nullable(DateTime64(3, 'UTC')) CODEC(ZSTD(1)),

    -- The DATABASE host/instance the event is ABOUT, not the agent that
    -- forwarded it (that identity is agent_hostname/agent_version below).
    host                 LowCardinality(String),
    database_instance    LowCardinality(String),

    agent_hostname       LowCardinality(String),  -- "ddagenthostname" (metrics only)
    agent_version        LowCardinality(String),  -- "ddagentversion", or "agent_version" on dbmmetadata

    source               LowCardinality(String),  -- "ddsource" (samples, activity)
    dbm_type             LowCardinality(String),  -- "dbm_type": fqt | plan | activity
    kind                 LowCardinality(String),  -- "kind": lock_metrics, database_instance, ...
    dbms                 LowCardinality(String),  -- "dbms" (metadata)
    dbms_version         LowCardinality(String),  -- "dbms_version" (metadata)

    -- Nullable, not defaulted to 0: an integration reporting a genuine 0s
    -- interval and one that never sent the field at all are different facts.
    collection_interval  Nullable(Float64),

    -- The multiset tag map — see the header of 0001_initial.sql and
    -- docs/decisions/0001-tags-are-a-multiset.md. Built from the envelope's
    -- merged ddtags ("env:prod,team:db" string OR ["env:prod","team:db"]
    -- array, depending on track) plus "tags".
    tags                 Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- Sorted names of the keys dbmEnvelope could not place in a declared
    -- column — this is where the engine-specific row arrays (oracle_rows,
    -- postgres_activity, ...) and the databasequery "db" object are named,
    -- without ninjacat needing a Go type for their contents. Same pattern as
    -- k8s_actions.extra_keys: visible without a decoder.
    extra_keys           Array(String),

    -- Envelope keys whose VALUE did not fit its declared field (a smaller,
    -- usually-empty list, distinct from extra_keys above — these are known
    -- fields with an unexpected shape, not unknown fields).
    undecoded_keys       Array(String),

    -- The complete event, byte for byte, exactly as it arrived. The only
    -- lossless representation of the engine-specific rows: nothing above
    -- captures oracle_rows/postgres_activity/the "db" object's full
    -- contents, and nothing ever will until a Go type exists for each
    -- engine — this column is what makes that a future decoding step
    -- instead of a permanent gap.
    event                String CODEC(ZSTD(3)),

    -- Convenience columns for the query-sample fields already pulled out of
    -- the "db" object for the log line (intake/router_dbm.go's
    -- dbmQuerySample/dbmQueryPlan) — promoted to real columns because "which
    -- query is this" and "did it have a plan" are the two questions worth a
    -- column instead of a JSON extract. All Nullable: only databasequery
    -- events carry a "db" object at all, and even those may not decode.
    db_instance           Nullable(String),
    query_signature       Nullable(String),
    statement             Nullable(String) CODEC(ZSTD(3)),  -- full SQL text, can be large
    plan_signature        Nullable(String),
    -- Length of db.plan.definition (the list of plan steps), not the byte
    -- size of the plan — a genuine 0-step plan and "no plan was sent at all"
    -- are different facts, so this stays Nullable rather than defaulting to 0.
    -- A THIRD fact -- "a plan was sent but its definition did not parse as a
    -- step array" -- also leaves this column NULL (it has no way to encode
    -- "malformed" itself) but is not silently the same as "no plan sent":
    -- intake/router_dbm.go's dbmEventRow records "db.plan.definition" in
    -- undecoded_keys for that case, and the raw bytes are always in `event`.
    plan_definition_steps Nullable(UInt32),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    -- mapValues on an array-valued map yields Array(Array(...)), which
    -- bloom_filter rejects — flatten to one level so the index still sees
    -- every tag value.
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
-- Point query shape: "what did this database instance report on this
-- track", so track and the two identity columns sit between tenant_id and
-- the time — the same reasoning as metrics' (tenant_id, metric, host,
-- timestamp), with track standing in for metric (it is the same kind of
-- "which shape of row is this" filter) and database_instance added because
-- host alone is not a whole database identity (one host, several instances).
ORDER BY (tenant_id, track, host, database_instance, received_at)
-- 30 days, matching metrics/sketches/k8s_resources rather than the 90-day
-- events/check_runs class: query samples and activity snapshots are bulky
-- (a row can hold a full raw event with dozens of oracle_activity/query
-- rows plus the complete SQL text), so this table is closer in shape and
-- volume to the high-cardinality tables than to the human-scale ones.
TTL toDateTime(received_at) + INTERVAL 30 DAY;
