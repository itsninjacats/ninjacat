-- agent_flares: support-flare uploads, POST /support/flare on every intake
-- host (registered once in intake/server.go's engineAuth, so it answers on
-- whichever host the agent's dd_url — or its versioned <maj>-<min>-<patch>-
-- flare.agent.<site> rewrite, see intake/routes.go — happens to point at).
--
-- No CREATE DATABASE and no database qualifier, same as every other
-- migration: the runner applies every statement over a connection already
-- opened against the target database, and qualifying would break running
-- against a scratch one.
--
-- A flare is a support artifact a human explicitly asked for (`datadog-agent
-- flare`, or a remote-config task), not telemetry — case_id/email/hostname/
-- source/agent_version are the named fields the agent's own uploader writes
-- (comp/core/flare/helpers/send_flare.go, getFlareReader: case_id, email,
-- source, rc_task_uuid, then the file part "flare_file", then agent_version
-- and hostname — the last two AFTER the file, so a streaming reader cannot
-- assume metadata precedes the archive). Everything the uploader sends that
-- is not one of those five named columns — rc_task_uuid today, whatever a
-- future agent version adds tomorrow — lands in `fields` instead of being
-- dropped, the same "declared columns plus a catch-all map" shape used
-- throughout this schema for producer-defined extras.
CREATE TABLE IF NOT EXISTS agent_flares
(
    tenant_id     LowCardinality(String),
    received_at   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Not LowCardinality: a real fleet has one hostname per machine, so the
    -- dictionary would grow without bound.
    hostname      String,

    -- Kept as the string the agent sent, not parsed to an integer: an empty
    -- value means "new case, no id assigned yet" (SendTo's getFlareReader
    -- only writes the field when non-empty) and a String preserves that
    -- absence instead of coercing it into 0.
    case_id       String,
    email         String,
    source        LowCardinality(String),
    agent_version LowCardinality(String),

    filename      String,
    size_bytes    UInt64 CODEC(T64, ZSTD(1)),

    -- Every multipart field that is not one of the five named columns above
    -- (rc_task_uuid today; see the header comment).
    fields        Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- The zip archive itself, verbatim. ZSTD(3) like every other bulky cold
    -- column in this schema (raw_payloads.body, k8s_manifests.content):
    -- nothing filters on this, it is read whole when someone opens the case.
    archive       String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, received_at)
-- 90 days: a flare is a support artifact someone asked for, not a debugging
-- sample — the same retention class as check_runs/events/k8s_actions, and
-- monthly partitions because volume here is "a few per incident", not a
-- steady stream.
TTL toDateTime(received_at) + INTERVAL 90 DAY;
