-- api / logs fidelity: the columns and tables that close the gap between what
-- the api.<site>, app.<site> and http-intake.logs.<site> handlers DECODE and
-- what they STORE.
--
-- Until now those handlers decoded a payload in full and then projected a
-- fraction of it into a row, logging the rest. A log line is not a sink: it
-- rotates, it is bounded to eight items per batch, and nobody greps a month of
-- them to answer "which agent version sent this". Everything the audit found
-- decoded-but-unstored either gets a column here or is written down in
-- docs/tables/{api,logs}.md as a deliberate drop.
--
-- Two shapes recur and are worth stating once:
--
--   * A Map(String, String) named `extra` holds keys the upstream model does
--     not declare, values JSON-encoded. Encoding the VALUE as JSON is what
--     lets one column take a string, a number, an object and an array without
--     a type per key — and unlike k8s_actions.extra_keys, which keeps only the
--     names, this keeps what arrived. An agent that starts sending a new field
--     is then visible AND readable before anyone adds a column for it.
--   * A String column holding a sub-object verbatim (`meta`, `outputs`,
--     `client`, `payload`). EMPTY STRING MEANS THE KEY WAS ABSENT, which is
--     not the same as "{}" — the distinction is free here and impossible to
--     recover later.
--
-- No CREATE DATABASE and no database qualifier, same as 0001 and 0002. Every
-- statement is IF NOT EXISTS so a failure part-way through re-runs cleanly.

-- ---------------------------------------------------------------------------
-- logs: the attributes the model does not declare, and where the timestamp
-- actually came from.
-- ---------------------------------------------------------------------------
--
-- datadogV2.HTTPLogItem declares five fields. Everything else a sender
-- includes — dd.trace_id, usr.*, http.*, error.*, whole nested objects — was
-- COUNTED and dropped. dd.trace_id is the one key that joins a log line to its
-- trace, so counting it is the worst possible outcome.
--
-- A native JSON column rather than a String: the values are read back by
-- attribute (attributes.usr.id), nested objects keep their shape, and
-- ClickHouse stores each discovered path as its own subcolumn, so a query for
-- one attribute does not read the others. The keys lifted into real columns
-- (status, timestamp, date, host/hostname, service, ddsource, ddtags) are
-- REMOVED before the rest lands here — a value in two places is a value that
-- can disagree with itself.
--
-- timestamp_source records which of the five resolution steps produced the
-- row's timestamp. Without it "the sender did not send one" and "the sender
-- sent one we could not parse" are the same row, and the second is a bug
-- report while the first is normal.
ALTER TABLE logs
    ADD COLUMN IF NOT EXISTS attributes JSON,
    ADD COLUMN IF NOT EXISTS timestamp_source LowCardinality(String);

-- ---------------------------------------------------------------------------
-- metrics and sketches: origin, resources, undeclared keys.
-- ---------------------------------------------------------------------------
--
-- ORIGIN is Datadog's own provenance signal: which product, which category and
-- which integration produced a metric (agent vs serverless vs the OTel
-- exporter; dogstatsd vs an integration check). It rode in as one opaque
-- "10/11/42" log string. Three UInt32 columns instead, because the question is
-- always "show me everything from dogstatsd", which is a filter on one of
-- them. ZERO MEANS NOT SENT — that is also what the wire means, since the
-- proto default is 0 and the enum reserves 0 for "unknown", so no Nullable is
-- needed to keep absent and zero apart here.
--
-- RESOURCES is v2's [type, name] list. The host entry has had a column since
-- 0001; everything else (device, and whatever Datadog adds next) was tallied
-- into a log line. A plain Map, NOT the tag multiset shape: a resource list
-- is a set of typed slots, one name per type, so duplicate keys cannot happen
-- the way they do with tags. v1 has no resource list at all — its `device`
-- arrives as an undeclared series key and is written here as device=<name> so
-- one query answers the question across both wire versions.
--
-- EXTRA takes the rest of v1's AdditionalProperties (anything beyond device,
-- source_type_name and unit) and, on sketches, the undeclared keys of a
-- distribution_points series. The agent's v1 encoder already writes three keys
-- the published model does not declare, so "undeclared" here is routine
-- traffic, not an anomaly.
ALTER TABLE metrics
    ADD COLUMN IF NOT EXISTS origin_product  UInt32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS origin_category UInt32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS origin_service  UInt32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS resources Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS extra     Map(String, String) CODEC(ZSTD(3));

-- sketches takes the same five columns so a metric and its distribution answer
-- the same questions. resources stays empty for the agent's own sketch payload
-- (gogen.SketchPayload_Sketch carries a flat host field, not a resource list);
-- it is here because the column layout of the two tables is deliberately
-- parallel and a third-party or OTLP sketch source has somewhere to put one.
ALTER TABLE sketches
    ADD COLUMN IF NOT EXISTS origin_product  UInt32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS origin_category UInt32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS origin_service  UInt32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS resources Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS extra     Map(String, String) CODEC(ZSTD(3));

-- ---------------------------------------------------------------------------
-- check_runs: undeclared keys.
-- ---------------------------------------------------------------------------
--
-- The agent's own servicecheck.ServiceCheck has exactly the six keys the model
-- declares, so a non-empty extra is itself the signal: either a newer agent
-- grew a field or a third-party client is sending something of its own. Either
-- way the value is worth more than the count of it that used to be logged.
ALTER TABLE check_runs
    ADD COLUMN IF NOT EXISTS extra Map(String, String) CODEC(ZSTD(3));

-- ---------------------------------------------------------------------------
-- events: the parent link, the raw enum words, the agent's event_type.
-- ---------------------------------------------------------------------------
--
-- alert_type and priority are COERCED into Datadog's fixed vocabularies before
-- they reach their columns, because dropping an event over an invented alert
-- type would lose the one thing we were asked to remember. The coercion stays,
-- but what the sender actually wrote now survives next to it: alert_type_raw
-- and priority_raw are exactly the strings off the wire, empty when the sender
-- sent nothing. A dashboard reads the coerced column; a bug report reads these.
--
-- related_event_id is Nullable because Datadog's own id space starts at 1 and
-- "no parent" must not become 0, which is what an Int64 with no null would
-- make it.
--
-- event_type is the agent's pkg/metrics/event.Event.EventType, which arrives
-- on the /intake/ events variant and has no equivalent on the public
-- /api/v1/events request — the two sources now feed one table, so the column
-- is simply empty for the public API's events.
ALTER TABLE events
    ADD COLUMN IF NOT EXISTS event_type LowCardinality(String),
    ADD COLUMN IF NOT EXISTS related_event_id Nullable(Int64) CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS alert_type_raw String CODEC(ZSTD(1)),
    ADD COLUMN IF NOT EXISTS priority_raw   String CODEC(ZSTD(1)),
    ADD COLUMN IF NOT EXISTS extra Map(String, String) CODEC(ZSTD(3));

-- ---------------------------------------------------------------------------
-- hosts: the rest of the /intake/ host envelope.
-- ---------------------------------------------------------------------------
--
-- The host payload had nine columns and roughly twenty keys. What was missing
-- is precisely what people ask a fleet inventory: which agent flavour and
-- Python this host runs, whether FIPS is on, how the agent was installed,
-- whether OTLP is enabled, what the network interfaces and filesystems look
-- like, and which tag SOURCES a host carries.
--
-- Sub-objects whose shape shifts between agent versions (meta, network,
-- filesystem, logs, otlp) are kept as their JSON text rather than flattened:
-- flattening would need a column per key of a structure the agent is free to
-- change, and the empty string already distinguishes "the agent did not send
-- this section" from "{}".
--
-- The flat ones (systemStats, install-method, proxy-info, container-meta) stay
-- Maps because their keys ARE the question ("which install method?"). They are
-- ordinary Maps, not the tag multiset: gohai and the agent's metadata
-- producers emit JSON objects, and a JSON object cannot repeat a key.
--
-- host_tags is the exception and DOES carry arrays, for a different reason
-- than tags do: the key is the tag SOURCE (system, google_tags, a custom
-- provider) and the value is that source's whole tag list. Before this, only
-- the "system" source reached the row and every other source was dropped —
-- on a cloud host that silently discarded the provider's tags.
--
-- fips_mode and fips_proxy_enabled are Nullable(UInt8) rather than UInt8: an
-- older agent omits both keys, and "FIPS is off" is a different statement from
-- "this agent does not know about FIPS".
--
-- gohai_extra and intake_extra are the escape hatches — gohai sections beyond
-- the five with columns, and top-level envelope keys this decoder does not
-- know — values JSON-encoded. `resources` is the legacy V5 process snapshot
-- that rides along on the host payload; the process intake owns that format,
-- so it is kept verbatim rather than parsed twice.
ALTER TABLE hosts
    ADD COLUMN IF NOT EXISTS uuid           String,
    ADD COLUMN IF NOT EXISTS agent_flavor   LowCardinality(String),
    ADD COLUMN IF NOT EXISTS python_version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS meta           String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS system_stats   Map(String, String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS network        String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS filesystem     String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS logs           String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS install_method Map(String, String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS proxy_info     Map(String, String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS otlp           String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS container_meta Map(String, String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS fips_mode          Nullable(UInt8),
    ADD COLUMN IF NOT EXISTS fips_proxy_enabled Nullable(UInt8),
    ADD COLUMN IF NOT EXISTS host_tags    Map(LowCardinality(String), Array(String)) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS gohai_extra  Map(String, String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS intake_extra Map(String, String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS resources    String CODEC(ZSTD(3));

-- ---------------------------------------------------------------------------
-- agent_batch_metadata: who sent a batch, as opposed to what was in it.
-- ---------------------------------------------------------------------------
--
-- gogen.SketchPayload carries a CommonMetadata block describing the SENDER —
-- agent version, timezone, its clock, and both of its IPs. None of it belongs
-- on a sketch row (it would repeat once per bucket), and all of it is what you
-- want when a host's numbers look wrong: a clock that disagrees with ours
-- explains a gap on a chart faster than anything else.
--
-- ApiKey is a field of CommonMetadata and is DELIBERATELY ABSENT from this
-- table. The request already authenticated; storing the credential again would
-- put it in a table with a TTL, which is exactly where a credential should
-- never be.
CREATE TABLE IF NOT EXISTS agent_batch_metadata
(
    tenant_id     LowCardinality(String),
    received_at   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Which intake carried the block, so a second payload type that grows a
    -- CommonMetadata lands here too instead of getting its own table.
    intake        LowCardinality(String),

    agent_version LowCardinality(String),
    timezone      LowCardinality(String),

    -- The agent's own clock at send time, seconds, as a float because that is
    -- what the proto declares. Compared against received_at it is a clock-skew
    -- measurement for free.
    current_epoch Float64 CODEC(Gorilla, ZSTD(1)),

    -- Not LowCardinality: in a large fleet these are per-host.
    internal_ip   String,
    public_ip     String
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, agent_version, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- agent_checks: the V5 collector's check status list from /intake/.
-- ---------------------------------------------------------------------------
--
-- collectorimpl.Payload sends check statuses as POSITIONAL ARRAYS — check
-- name, source type, instance id, status, message — with no published type and
-- no key names anywhere on the wire. The five positions that are documented
-- get columns; anything the agent appends after them is kept as a JSON array
-- in positional_extra, because a sixth element appearing one day is exactly
-- the kind of change this table exists to make visible.
--
-- This is NOT check_runs. check_runs holds /api/v1/check_run, the current
-- service-check path, keyed by check and host over time. These are the V5-era
-- collector's own view, arriving on the same request as the host metadata, and
-- merging the two would mean inventing a mapping between two status
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

    -- Nullable because the positional array may simply be shorter than four
    -- elements, and status 0 is a real value ("OK"). A non-nullable column
    -- would report every truncated entry as healthy.
    status        Nullable(Int64) CODEC(T64, ZSTD(1)),
    message       String CODEC(ZSTD(3)),

    -- Elements beyond the fifth, as a JSON array. Empty when there were none.
    positional_extra String CODEC(ZSTD(3)),

    -- The envelope's `meta` object, verbatim JSON. Empty means absent.
    meta          String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, hostname, check_name, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- external_host_tags: tags an agent reports FOR ANOTHER host.
-- ---------------------------------------------------------------------------
--
-- The same /intake/ request carries external_host_tags: [[hostname, {source:
-- [tags]}], ...] — an agent telling the backend about hosts it is not running
-- on (a vSphere collector tagging its VMs, a cloud integration tagging
-- instances). They cannot go on the hosts row, because the row they describe
-- belongs to a different host than the one that sent them, and they are per
-- SOURCE, so one host has several rows here at once.
--
-- tags is the full multiset shape: these are Datadog tags off the wire and
-- nothing stops two of them sharing a key.
CREATE TABLE IF NOT EXISTS external_host_tags
(
    tenant_id   LowCardinality(String),
    received_at DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The host being DESCRIBED, not the agent that sent the request.
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

-- ---------------------------------------------------------------------------
-- agent_metadata: /api/v1/metadata, all thirteen variants, one table.
-- ---------------------------------------------------------------------------
--
-- Eleven producers share this path and each sends a differently shaped
-- payload; none of the types is importable. One table per variant would be
-- thirteen tables of one column each, and the variants change shape with every
-- agent release — so the shared envelope becomes columns and the variant's own
-- body is kept as JSON text under the key that named it.
--
-- payload is a String rather than a native JSON column on purpose: these
-- bodies are read whole, by a person, when they are read at all, and a JSON
-- column would materialise a subcolumn for every distinct path across thirteen
-- unrelated shapes.
--
-- timestamp is Nullable because the envelope's field is optional and 1970 is
-- not an answer. received_at is always ours.
CREATE TABLE IF NOT EXISTS agent_metadata
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The variant key that selected this row: agent_metadata, host_metadata,
    -- check_metadata, ... A request carrying two variant keys produces two
    -- rows, which is what the wire allows.
    variant      LowCardinality(String),

    hostname     LowCardinality(String),
    -- The cluster-agent variants send these INSTEAD of hostname.
    cluster_name LowCardinality(String),
    cluster_id   String,

    timestamp    Nullable(DateTime64(3, 'UTC')) CODEC(DoubleDelta, ZSTD(1)),
    uuid         String,

    payload      String CODEC(ZSTD(3)),

    -- Envelope keys beyond hostname/clustername/cluster_id/timestamp/uuid and
    -- the variant keys themselves, values JSON-encoded. clustercheck_status
    -- and clustercheck_integration_status arrive here, next to the
    -- clustercheck_metadata row they belong to.
    envelope_extra Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, variant, hostname, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- delegated_auth_requests: who asked /api/v2/intake-key for a key.
-- ---------------------------------------------------------------------------
--
-- The agent posts an empty body with `Authorization: Delegated <proof>` and
-- expects an API key back. The proof IS the credential, so it is never stored:
-- what lands here is the SHA-256 of it, hex. That is enough to tell two
-- delegated identities apart, to see one proof suddenly being used from
-- somewhere new, and to key a real delegated-auth mapping on later — and it is
-- useless to anyone who steals the table.
CREATE TABLE IF NOT EXISTS delegated_auth_requests
(
    tenant_id  LowCardinality(String),
    at         DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- "Delegated", or whatever else a client puts before the first space.
    scheme     LowCardinality(String),

    -- SHA-256 of the proof, lowercase hex. NEVER the proof itself.
    proof_fingerprint String,

    -- The API key the request authenticated with, by its id in Postgres —
    -- again never the key, and never its hash either.
    api_key_id LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(at)
ORDER BY (tenant_id, at)
TTL toDateTime(at) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- symbol_queries: what the profiler's symbol uploader asked for.
-- ---------------------------------------------------------------------------
--
-- The uploader asks which build ids we already hold before uploading symbols,
-- and we answer "none" because nothing stores them yet. The question is still
-- worth keeping: the build ids are the profiled binaries of the fleet, and
-- this table is what a future /api/v2/srcmap handler will diff against.
CREATE TABLE IF NOT EXISTS symbol_queries
(
    tenant_id LowCardinality(String),
    at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    arch      LowCardinality(String),

    -- Order preserved: the uploader's reply is positional against this list.
    build_ids Array(String) CODEC(ZSTD(3)),

    -- The JSON:API document as it arrived (type, id, every attribute),
    -- verbatim — the attribute set here is undocumented and moves.
    resource  String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(at)
ORDER BY (tenant_id, at)
TTL toDateTime(at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Private Action Runner: enrollment, work loop, connections.
-- ---------------------------------------------------------------------------
--
-- PAR is the only agent component whose request loop is driven by what we
-- answer, and every one of its calls after enrollment is signed with the
-- identity WE handed out. Nothing was stored, which means nothing can verify
-- those signatures — runner_enrollments is the table that makes that possible.

-- One row per runner, newest enrollment wins: a runner that re-enrolls (a pod
-- restart, a rotated key) must not leave a second identity behind, because the
-- verifier would then have two public keys and no rule for choosing. No TTL —
-- an enrolled runner is live state, and it leaves this table by being replaced,
-- the same reasoning as `hosts`.
CREATE TABLE IF NOT EXISTS runner_enrollments
(
    tenant_id       LowCardinality(String),

    -- Handed out by us, a UUID. It must never contain a colon: the runner
    -- builds urn:dd:apps:on-prem-runner:<region>:<org_id>:<runner_id> and
    -- splits that on ":".
    runner_id       String,
    enrolled_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    name            String,
    modes           Array(LowCardinality(String)),
    host            String,

    -- The runner's own signing key. Public by construction, so this is the one
    -- key-shaped column in the schema that is safe to keep — and it has to be
    -- kept, or the OPMS JWTs it signs can never be checked.
    public_key_pem  String CODEC(ZSTD(3)),

    agent_hostname  String,
    orch_cluster_id String,
    agent_flavor    LowCardinality(String),

    -- What we answered with, derived from the tenant by FNV-64a. Stored
    -- because the derivation is ours: if the hash ever changes, the runners
    -- enrolled under the old one are only recoverable from here.
    org_id          Int64 CODEC(T64, ZSTD(1)),

    -- The request's attributes as they arrived, native JSON so a new
    -- attribute is queryable without a migration.
    attributes      JSON
)
ENGINE = ReplacingMergeTree(enrolled_at)
ORDER BY (tenant_id, runner_id);

-- Task outcomes. error_code is Nullable and numeric: it is an enum on the
-- runner's side, and "no error code" is what a succeeded task sends.
-- outputs is the action's own result — arbitrary JSON of any size, previously
-- logged only as a byte count, which is the one number about it nobody needs.
CREATE TABLE IF NOT EXISTS runner_task_updates
(
    tenant_id     LowCardinality(String),
    at            DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The JSON:API document id: "succeed_task" or "fail_task".
    outcome       LowCardinality(String),

    task_id       String,
    action_fqn    LowCardinality(String),
    job_id        String,

    -- The runner's self-description, verbatim JSON. Empty means absent.
    client        String CODEC(ZSTD(3)),

    branch        LowCardinality(String),
    outputs       String CODEC(ZSTD(3)),

    error_code    Nullable(Int64) CODEC(T64, ZSTD(1)),
    error_details String CODEC(ZSTD(3)),
    api_error     String CODEC(ZSTD(3)),

    -- Undeclared attributes from either level of the document, JSON-encoded
    -- values, prefixed "payload." when they came from the nested object.
    extra         Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(at)
ORDER BY (tenant_id, job_id, task_id, at)
TTL toDateTime(at) + INTERVAL 90 DAY;

-- Per-task heartbeats: high frequency, low value individually, and exactly
-- what answers "when did this job stop making progress". Short TTL and daily
-- partitions for that reason — this is the chattiest table of the group.
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

-- The idle poll. Every dequeue is a runner saying "I am alive, I started at X
-- and last had work at Y" — which is the only liveness signal a runner with no
-- tasks ever produces. Both timestamps are kept as the STRINGS they arrive as:
-- the format is undocumented and an unparseable value must not become 1970.
CREATE TABLE IF NOT EXISTS runner_dequeues
(
    tenant_id             LowCardinality(String),
    at                    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    runner_started_at     String,
    last_task_received_at String,

    -- X-Datadog-OnPrem-Version / -Modes: the runner's identity on a request
    -- that carries no body worth speaking of.
    version               LowCardinality(String),
    modes                 LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toDate(at)
ORDER BY (tenant_id, at)
TTL toDateTime(at) + INTERVAL 7 DAY;

-- Connections an action runner may use to reach a third-party system.
--
-- SENSITIVE: `credentials` holds the SECRETS of that integration — tokens,
-- passwords, private keys — verbatim, because a connection without its
-- credentials cannot be used and dropping them would make the table a lie.
-- The logging path deliberately prints only the KEY NAMES of this object and
-- must keep doing so. Anything that reads this column (a panel, an export, a
-- support flare) has to treat it as a credential store, not as telemetry.
CREATE TABLE IF NOT EXISTS action_connections
(
    tenant_id        LowCardinality(String),
    at               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    name             String,
    runner_id        String,

    tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    integration_type LowCardinality(String),

    -- SECRETS. See the note above.
    credentials      String CODEC(ZSTD(3)),

    extra            Map(String, String) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(at)
ORDER BY (tenant_id, runner_id, name, at)
TTL toDateTime(at) + INTERVAL 90 DAY;
