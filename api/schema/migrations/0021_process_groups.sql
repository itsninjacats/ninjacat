-- 0021: process_groups — the processes of a host grouped by name.
--
-- Every agent posts {"resources": {"processes": {"snaps": [...]}}} to
-- /intake/ with its host metadata: the resources check (pkg/gohai/processes),
-- kept from Agent 5. It is the one view of a host's processes that needs no
-- process-agent: one row per name with the group's memory and how many
-- processes are in it. Until 0021 the body went to raw_payloads.
--
-- A snapshot arrives a few times an hour, not every ten seconds: for the
-- live process list see `processes` (0008).

CREATE TABLE IF NOT EXISTS process_groups
(
    tenant_id     LowCardinality(String),
    -- The agent's clock when it took the snapshot, to the second.
    timestamp     DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    host          LowCardinality(String),
    name          LowCardinality(String),
    usernames     Array(LowCardinality(String)),
    process_count UInt32,
    -- The agent sends a whole number here and, as of 7.84, always 0.
    cpu_pct       Float64,
    mem_pct       Float64 CODEC(Gorilla, ZSTD(1)),
    vms           UInt64 CODEC(T64, ZSTD(1)),
    rss           UInt64 CODEC(T64, ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, name)
TTL timestamp + INTERVAL 30 DAY;
