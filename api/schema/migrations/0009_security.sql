-- Security: CWS activity dumps, runtime/compliance security events, and SBOMs.
--
-- Five hosts feed these six tables (intake/router_security.go):
--   cws-intake.<site>                     secdump      -> cws_activity_dumps, cws_dump_nodes
--   runtime-security-http-intake.logs.<site>  secruntime, secinfo -> security_events
--   cspm-intake.<site>                    compliance   -> security_events
--   sbom-intake.<site>                    sbom         -> sbom_entities, sbom_components, sbom_vulnerabilities
--   sds-intake.<site>                     sdsresult    -> raw_payloads (no published Go type yet, see
--                                                          intake/router_security.go's HandleSDSResult)
--
-- No CREATE DATABASE and no database qualifiers, same as 0001/0002: the
-- migration runner applies every statement over a connection already opened
-- against the target database.

-- ---------------------------------------------------------------------------
-- CWS activity dumps: POST /api/v2/secdump
--
-- gzip(multipart/form-data) with two independent parts: "event" (a generic
-- JSON header, profile.ActivityDumpHeader — not an importable Go type) and
-- "dump" (dumpsv1.SecDump, protobuf). Either can be present without the
-- other, so every column below is filled from whichever part supplied it —
-- there is no row-level requirement that both arrived.
--
-- One row per request. The process tree itself is NOT flattened into this
-- row — that is cws_dump_nodes below, linked by dump_id — because a tree can
-- run to thousands of nodes and this row is the "what is this dump" summary,
-- read on its own far more often than the whole tree is walked.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS cws_activity_dumps
(
    tenant_id           LowCardinality(String),

    -- Arrival time: neither part of the payload carries a wall-clock
    -- timestamp for the dump as a whole (Metadata.Start/End are kernel- or
    -- boot-relative counters, not calendar time — see start_raw/end_raw
    -- below), so this is what we actually have.
    received_at         DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Generated per request, never taken from the wire (neither part carries
    -- an id) — this is what cws_dump_nodes.dump_id joins against.
    dump_id             UUID,

    -- "event" part: profile.ActivityDumpHeader. Not importable, so only the
    -- documented keys get columns; anything else lands in header_extra.
    header_host         LowCardinality(String),
    header_service      LowCardinality(String),
    header_source       LowCardinality(String),
    header_tags         Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- Kept as the raw JSON string exactly as received (an array or an array
    -- of objects, undocumented which) — not decoded further, since decoding
    -- a shape we are only guessing at risks losing it instead.
    dns_names           String CODEC(ZSTD(3)),
    -- Any header key beyond host/service/ddsource/ddtags/dns_names, keyed by
    -- name, value kept as its raw JSON text. Plain Map, not the tags
    -- multiset: these are arbitrary undeclared JSON keys, not "key:value"
    -- tag pairs.
    header_extra        Map(String, String) CODEC(ZSTD(3)),
    -- The "event" part's raw bytes, exactly as received, regardless of
    -- whether it parsed as a JSON object above. Mirrors `dump` below: a
    -- malformed "event" part paired with a decodable "dump" part must not
    -- silently lose its bytes just because header_host/.../header_extra
    -- stayed empty.
    header_raw          String CODEC(ZSTD(3)),

    -- "dump" part: dumpsv1.SecDump's own envelope. Can legitimately disagree
    -- with the header's host/service/source — the agent builds the two parts
    -- independently — so both get columns rather than one overwriting the
    -- other.
    dump_host           LowCardinality(String),
    dump_service        LowCardinality(String),
    dump_source         LowCardinality(String),
    dump_tags           Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- dumpsv1.Metadata, flattened. start_raw/end_raw/size_raw are Nullable
    -- UInt64 for two reasons at once: NULL means "Metadata itself was
    -- absent" (the dump decoded with no metadata), while a present 0 is a
    -- real reading (kernel-boot-relative counters legitimately start at 0).
    -- These are NOT wall-clock timestamps — do not toDateTime() them.
    agent_version       LowCardinality(String),
    agent_commit        LowCardinality(String),
    kernel_version      LowCardinality(String),
    linux_distribution  LowCardinality(String),
    arch                LowCardinality(String),
    metadata_name       String,
    protobuf_version    LowCardinality(String),
    differentiate_args  UInt8,
    comm                LowCardinality(String),
    container_id        String,
    start_raw           Nullable(UInt64) CODEC(ZSTD(1)),
    end_raw             Nullable(UInt64) CODEC(ZSTD(1)),
    size_raw            Nullable(UInt64) CODEC(ZSTD(1)),
    serialization       LowCardinality(String),
    cgroup_id           String,
    cgroup_manager      LowCardinality(String),

    -- Total node count across the whole tree (every depth, not just roots) —
    -- the cheap "how big was this dump" answer without joining cws_dump_nodes.
    tree_node_count     UInt32 CODEC(T64, ZSTD(1)),

    -- The "dump" part's raw protobuf bytes, exactly as received. Lossless on
    -- purpose: agent-payload v5.0.207 does not expose field 7 ("mounts")
    -- through any accessor, but proto.Unmarshal keeps it as an unknown field
    -- on the wire, and this is what keeps it reachable until a newer
    -- agent-payload ships.
    dump                String CODEC(ZSTD(3)),

    INDEX idx_header_tag_keys   mapKeys(header_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_header_tag_values arrayFlatten(mapValues(header_tags)) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_dump_tag_keys     mapKeys(dump_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_dump_tag_values   arrayFlatten(mapValues(dump_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, received_at)
-- Security-investigative data, so the metrics/k8s_resources class (30 days)
-- rather than logs' 14 — incident response routinely looks back further than
-- two weeks.
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- CWS process activity tree, one row per ProcessActivityNode, flattened
-- depth-first.
--
-- The tree is unbounded and recursive on the wire; ClickHouse rows are not,
-- so node_path/parent_path/depth reconstruct the shape a Nested column or a
-- self-join would otherwise need. node_path is the index path from the root
-- (e.g. [2, 0, 1] = tree[2].Children[0].Children[1]) — sufficient on its own
-- to rebuild the tree, with parent_path and depth kept alongside because
-- "give me this node's parent" and "give me everything at depth 3" are both
-- queries worth not recomputing from the path.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS cws_dump_nodes
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    dump_id            UUID,

    node_path          Array(UInt32) CODEC(ZSTD(1)),
    depth              UInt16 CODEC(T64, ZSTD(1)),
    parent_path        Array(UInt32) CODEC(ZSTD(1)),

    -- protojson of this node WITH Children CLEARED — the recursive part is
    -- exactly what node_path/parent_path/depth already reconstruct across
    -- rows, so keeping Children inline here would store every descendant
    -- subtree once per ancestor. Everything else on the node (process info,
    -- file info, matched rules, IMDS events including any AWS credential
    -- material the agent captured, network flows, ...) survives in this
    -- column even though only a few hot fields get their own column below.
    node               String CODEC(ZSTD(3)),

    pid                UInt32 CODEC(T64, ZSTD(1)),
    ppid               UInt32 CODEC(T64, ZSTD(1)),
    comm               LowCardinality(String),
    container_id       String,
    file_path          String,
    args               Array(String) CODEC(ZSTD(3)),
    image_tags         Array(String) CODEC(ZSTD(3)),
    matched_rule_ids   Array(String) CODEC(ZSTD(3)),
    generation_type    LowCardinality(String),

    files_count        UInt32 CODEC(T64, ZSTD(1)),
    dns_count          UInt32 CODEC(T64, ZSTD(1)),
    sockets_count      UInt32 CODEC(T64, ZSTD(1)),
    syscalls_count     UInt32 CODEC(T64, ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
-- "every node of this dump, in tree order" is the query this table exists
-- for — dump_id then node_path gives that directly.
ORDER BY (tenant_id, dump_id, node_path)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Security events: POST /api/v2/secruntime, /api/v2/secinfo, /api/v2/compliance
--
-- All three share one logs-pipeline envelope shape and one handler
-- (intake/router_security.go's handleSecLogsTrack) — track tells them apart.
-- Neither the envelope nor the inner event has a published Go type, so both
-- are kept as generic JSON: known envelope fields get columns, the inner
-- event's few well-known paths (agent.rule_id, evt.name, ...) get hot
-- columns, and everything else is preserved in extra/message rather than
-- typed.
--
-- Shaped like logs (MergeTree, ZSTD(3) message, tenant+track+time ordering)
-- but with a longer TTL than logs' 14 days: security events are what an
-- incident response investigates, and that happens well after arrival.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS security_events
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    track              LowCardinality(String), -- secruntime | secinfo | compliance

    -- Position in the JSON array this row's envelope arrived in. The array
    -- itself carries no other ordering signal, and arrival order is worth
    -- keeping.
    seq_in_batch       UInt32 CODEC(T64, ZSTD(1)),

    -- The envelope's own "timestamp" field, parsed from either a millisecond
    -- epoch number or an RFC3339 string (the wire uses both across
    -- producers). NULL when absent or unparseable — never coerced to
    -- received_at, which is a different, arrival-side fact.
    timestamp          Nullable(DateTime64(3, 'UTC')),
    hostname           LowCardinality(String),
    service            LowCardinality(String),
    ddsource           LowCardinality(String),
    status             LowCardinality(String),
    ddtags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- Envelope keys beyond the documented ones (hostname, service, ddsource,
    -- status, timestamp, ddtags, message), raw JSON text per key.
    extra              Map(String, String) CODEC(ZSTD(3)),

    -- The envelope's "message" field exactly as it arrived — usually a JSON
    -- string holding escaped JSON, occasionally already an object. This is
    -- the lossless copy; message/message_decoded below are the best-effort
    -- unwrap.
    message_raw        String CODEC(ZSTD(3)),
    -- The unwrapped inner JSON object, re-serialized — empty when "message"
    -- was missing, not JSON, or not an object. message_decoded distinguishes
    -- that failure from a message that legitimately decoded to "{}".
    message            String CODEC(ZSTD(3)),
    message_decoded     UInt8,

    -- Hot paths pulled out of the decoded inner event when present, empty
    -- otherwise: agent.rule_id / agent.policy_name (CWS runtime events),
    -- evt.name / evt.category (CWS), title / kind (compliance). None of
    -- these are guaranteed to exist on any given track — the inner shape is
    -- producer-defined and only "message" is the source of truth.
    rule_id             LowCardinality(String),
    policy_name         LowCardinality(String),
    evt_name            LowCardinality(String),
    evt_category        LowCardinality(String),
    title               String,
    event_kind          LowCardinality(String),

    INDEX idx_tag_keys   mapKeys(ddtags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, track, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- SBOM entities: POST /api/v2/sbom
--
-- One row per SBOMEntity (a payload can carry several — one per image layer,
-- filesystem, etc.). The full CycloneDX BOM is kept verbatim in `bom`
-- (protojson, lossless) since components and vulnerabilities below only
-- flatten the two lists this schema queries most; services, dependencies,
-- compositions and BOM metadata have no dedicated columns and live only in
-- `bom`.
--
-- Heartbeat entities (Heartbeat = true, meaning "identical to what we sent
-- last time") are stored as ordinary rows, same as any other: deduplicating
-- them is a read-time decision (WHERE NOT heartbeat, or argMax-style
-- collapsing by hash), not something the write path should decide for every
-- reader.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sbom_entities
(
    tenant_id              LowCardinality(String),
    received_at            DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Generated per entity, never taken from the wire — sbom_components and
    -- sbom_vulnerabilities join against this, not against `id` below, which
    -- is the sender's own value and not guaranteed unique across payloads.
    entity_id              UUID,

    payload_version        Int32 CODEC(T64, ZSTD(1)),
    host                    LowCardinality(String),
    -- Both proto3 optional (pointer) fields on SBOMPayload: NULL means the
    -- agent did not set them, not "empty string".
    source                  Nullable(String),
    dd_env                  Nullable(String),

    type                LowCardinality(String), -- SBOMSourceType enum name
    id                  String,                 -- the sender's own entity id
    -- Pointer-optional on the wire: NULL, never epoch zero, when the agent
    -- did not set it.
    generated_at        Nullable(DateTime64(3, 'UTC')),
    repo_tags           Array(String) CODEC(ZSTD(3)),
    repo_digests        Array(String) CODEC(ZSTD(3)),
    in_use              UInt8,
    generation_duration_ms Nullable(Int64),
    dd_tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    heartbeat           UInt8,
    hash                String,
    status              LowCardinality(String), -- SBOMStatus enum name
    kernel_version      LowCardinality(String),
    cpu_architecture    LowCardinality(String),

    -- The oneof's other arm: set when generation failed and no BOM exists at
    -- all. Empty when `bom` is set instead — never both.
    error               String CODEC(ZSTD(3)),

    -- The whole CycloneDX BOM, protojson, lossless. Empty when the oneof was
    -- `error` instead.
    bom                 String CODEC(ZSTD(3)),
    -- Backstop for the rare case protojson.Marshal on the BOM fails (it
    -- requires valid UTF-8 in every string field; proto.Marshal does not) —
    -- raw protobuf bytes, base64. Empty whenever `bom` is set, which is the
    -- overwhelming majority of rows.
    bom_raw             String CODEC(ZSTD(3)),

    component_count      UInt32 CODEC(T64, ZSTD(1)),
    vulnerability_count  UInt32 CODEC(T64, ZSTD(1)),

    INDEX idx_tag_keys   mapKeys(dd_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(dd_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, host, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- SBOM components: every cyclonedx_v1_4.Component in an entity's BOM,
-- flattened recursively (a component's own sub-components get rows too,
-- linked by parent_bom_ref and counted up in depth).
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sbom_components
(
    tenant_id             LowCardinality(String),
    received_at           DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    entity_id             UUID,

    bom_ref               String,
    parent_bom_ref        String, -- empty for a top-level component
    depth                 UInt16 CODEC(T64, ZSTD(1)),

    type                  LowCardinality(String), -- Classification enum name
    name                  String,
    -- Empty string is a valid, meaningful value per the CycloneDX spec
    -- ("RECOMMENDED to use an empty string" when a component has none) —
    -- never treated as absent. That's why version stays plain String while
    -- purl/cpe/group/publisher/author/description below are Nullable: those
    -- six are proto3-optional fields on cyclonedx_v1_4.Component, so NULL
    -- ("never set") must stay distinguishable from a present "".
    version               String,
    purl                  Nullable(String),
    cpe                   Nullable(String),
    `group`               Nullable(String),
    publisher             Nullable(String),
    author                Nullable(String),
    description           Nullable(String) CODEC(ZSTD(3)),
    scope                 LowCardinality(String),

    licenses              Array(String) CODEC(ZSTD(3)),
    hashes                Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    -- Multiset, not a plain map: CycloneDX explicitly allows repeated
    -- Property entries sharing a name (real scanners emit them), and a
    -- last-wins map silently dropped every duplicate but one — see
    -- docs/decisions/0001-tags-are-a-multiset.md for the same mistake made
    -- (and fixed) once already, on Datadog tags.
    properties            Map(String, Array(String)) CODEC(ZSTD(3)),
    -- protojson arrays of the corresponding repeated proto fields — kept as
    -- JSON text rather than further flattened, since neither is queried on
    -- its own today and both are open-ended (ExternalReference/Evidence
    -- carry their own nested structures).
    external_references   String CODEC(ZSTD(3)),
    evidence               String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, entity_id, bom_ref)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- SBOM vulnerabilities: every cyclonedx_v1_4.Vulnerability in an entity's
-- BOM (the CycloneDX VEX-in-BOM shape — findings plus their analysis state).
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS sbom_vulnerabilities
(
    tenant_id                LowCardinality(String),
    received_at              DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    entity_id                UUID,

    bom_ref                  String,
    id                       String, -- e.g. a CVE id
    -- cyclonedx_v1_4.Source.Name/.Url are themselves proto3-optional, so
    -- source_name/source_url stay Nullable even though the outer Source
    -- message being absent already reads the same way (both NULL either way).
    source_name              Nullable(String),
    source_url               Nullable(String),

    ratings                  String CODEC(ZSTD(3)), -- protojson array of VulnerabilityRating
    cwes                     Array(Int32) CODEC(ZSTD(1)),
    -- description/detail/recommendation are proto3-optional on
    -- cyclonedx_v1_4.Vulnerability — Nullable so "never set" survives
    -- distinct from a present "".
    description              Nullable(String) CODEC(ZSTD(3)),
    detail                   Nullable(String) CODEC(ZSTD(3)),
    recommendation           Nullable(String) CODEC(ZSTD(3)),
    advisories               String CODEC(ZSTD(3)), -- protojson array of Advisory

    -- All three are proto3 Timestamp pointers: NULL, never epoch zero, when
    -- the source did not supply one.
    created                  Nullable(DateTime64(3, 'UTC')),
    published                Nullable(DateTime64(3, 'UTC')),
    updated                  Nullable(DateTime64(3, 'UTC')),

    analysis_state          LowCardinality(String),
    analysis_justification  LowCardinality(String),
    analysis_response       Array(LowCardinality(String)),
    analysis_detail         String CODEC(ZSTD(3)),

    -- The bom_ref of every affected component, pulled out of `affects` for
    -- the join sbom_components does not otherwise offer; `affects` keeps the
    -- full VulnerabilityAffects (including version ranges) losslessly.
    affects_refs            Array(String) CODEC(ZSTD(3)),
    affects                 String CODEC(ZSTD(3)), -- protojson array of VulnerabilityAffects
    -- Multiset — see sbom_components.properties' comment above.
    properties              Map(String, Array(String)) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, entity_id, bom_ref)
TTL toDateTime(received_at) + INTERVAL 30 DAY;
