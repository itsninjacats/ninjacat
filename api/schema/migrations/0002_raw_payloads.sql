-- raw_payloads: the fallback that keeps a payload we cannot decode.
--
-- Roughly forty Datadog intakes still have no published schema on our side,
-- and the ones that do occasionally receive a shape the decoder does not
-- recognise. Until this table, both cases ended the same way: the handler
-- logged a line and the bytes were gone. That is the expensive kind of loss —
-- the request that would have told us what the format is arrives once, from a
-- real agent, in someone else's cluster.
--
-- So the rule is: a handler that cannot decode a body, or receives a shape
-- with no published schema, hands the bytes here (intake/raw.go storeRaw).
-- Success paths never do — this is a fallback, not a mirror of the traffic.
--
-- No CREATE DATABASE and no database qualifier, same as 0001: the runner
-- applies every statement over a connection already opened against the target
-- database, and qualifying would break running against a scratch one.

CREATE TABLE IF NOT EXISTS raw_payloads
(
    tenant_id        LowCardinality(String),

    -- Arrival time, not any timestamp from the payload: the payload is by
    -- definition something we could not read.
    received_at      DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Which intake took the request — the router's own label, the same string
    -- the handler logs with ("dbm", "trace", "rum", "lineage", ...). Part of
    -- the sort key because the question this table answers is always "what is
    -- <intake> actually sending?".
    intake           LowCardinality(String),

    -- Why the bytes ended up here: "no_schema" (we do not decode this intake
    -- yet), "decode_error" (we tried and failed), "unexpected_shape" (it
    -- decoded, but into something the converter does not handle). A bounded
    -- vocabulary on purpose — a free-text explanation goes in note.
    reason           LowCardinality(String),

    method           LowCardinality(String),

    -- The Host header, kept verbatim. Host-based routing IS the dispatch here
    -- (intake/routes.go), so a payload without its host is missing the field
    -- that says which product sent it. Not LowCardinality: per-agent
    -- rewrites like 7-58-2-app.agent.<site> make this unbounded.
    host             String,
    path             String,

    -- The query string, parsed. Array values because a parameter may legally
    -- repeat; same reason tags are a multiset, different source.
    query            Map(String, Array(String)) CODEC(ZSTD(3)),

    content_type     LowCardinality(String),

    -- The ORIGINAL Content-Encoding header. THE BODY BELOW IS ALREADY
    -- DECOMPRESSED: the Decompress middleware (intake/body.go) unwraps zstd,
    -- gzip and deflate before any handler sees the request, so this column
    -- records what the sender used, not what the body is. Keeping it matters
    -- because the codec is a fingerprint of the subsystem (zstd for metrics
    -- and /intake/, gzip for traces, deflate for distribution points) and
    -- because a mislabelled encoding is itself a decode failure worth seeing.
    content_encoding LowCardinality(String),

    -- A fixed allowlist of request headers, filled in intake/raw.go. An
    -- allowlist rather than everything, because "everything" includes
    -- Dd-Api-Key and Authorization, and a debugging table is the last place a
    -- credential should be durable. The chosen keys identify the sender and
    -- correlate the request: Via, User-Agent, DD-EVP-ORIGIN(-VERSION),
    -- DD-REQUEST-ID, DD-Agent-Hostname/-Env, Datadog-Container-ID, the
    -- X-Datadog-*-Tags pair, X-Requested-With (the diagnose probe marker),
    -- X-Dd-Hostname, X-Dd-Processagentversion, X-DD-Request-ID.
    headers          Map(String, String) CODEC(ZSTD(3)),

    -- The payload itself, decompressed, exactly as the handler saw it.
    -- ZSTD(3) like every other bulky cold column: nothing filters on this, it
    -- is read whole when somebody sits down to reverse-engineer a format.
    body             String CODEC(ZSTD(3)),
    body_bytes       UInt64 CODEC(T64, ZSTD(1)),

    -- Free text from the handler: the decoder error, the field that was
    -- missing, whatever the person reading this later will want.
    note             String
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, intake, received_at)
-- 30 days. This is a debugging sample, not a record: long enough to cover
-- "it started failing sometime last month", short enough that an intake
-- nobody has got round to cannot fill a disk.
TTL toDateTime(received_at) + INTERVAL 30 DAY;
