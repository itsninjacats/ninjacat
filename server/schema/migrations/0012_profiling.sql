-- profiles, debugger_logs, debugger_diagnostics, symdb_uploads and
-- symbol_uploads — the tables fed by intake/router_profiling.go's three
-- hosts: intake.profile.<site> (Continuous Profiler), debugger-intake.<site>
-- (Dynamic Instrumentation logs/diagnostics and symbol-database uploads) and
-- sourcemap-intake.<site> (native symbol uploads for the host profiler).
--
-- No CREATE DATABASE and no database qualifier, same as 0001/0002: the
-- migration runner applies every statement over a connection already opened
-- against the target database, and qualifying would break running against a
-- scratch one (schema/schema.go, schema/storagetest).
--
-- Every table here uses received_at — arrival time, always known — as its
-- PARTITION BY / ORDER BY time column, never a wire-supplied timestamp: the
-- wire timestamps these payloads carry (a profile's start/end, a diagnostic
-- message's timestamp) are informational and travel in their own Nullable
-- columns instead, so an absent or malformed one never has to fall back to
-- anything and never has to become 1970-01-01.

-- ---------------------------------------------------------------------------
-- profiles: /api/v2/profile and /v1/input (intake.profile.<site>).
--
-- One row per multipart submission. Samples are NOT exploded into rows: a
-- single CPU profile routinely carries tens of thousands of them, and pprof
-- (github.com/google/pprof/profile) is a perfectly good reader for the bytes
-- kept in attach_bytes — exploding would multiply write volume by the sample
-- count to reproduce a shape an existing tool already parses for free. What
-- IS pulled out per attachment is its pprof envelope (sample types/units,
-- period, counts) as parallel arrays, so a query can filter by shape without
-- re-parsing every attachment.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS profiles
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Which of this router's two paths produced the row ("profile" for
    -- /api/v2/profile, "profile-v1" for /v1/input). Left open-ended (String,
    -- not Enum8) rather than closed to two values, so a future RUM profiler
    -- reuse ("browser", see docs/tables/profiling.md) needs no migration.
    variant                LowCardinality(String),
    dd_evp_origin          LowCardinality(String),
    dd_evp_origin_version  LowCardinality(String),

    -- The "event" part, byte for byte, before any decode: the lossless copy.
    -- Every column from start_raw to tags_profiler below is a convenience
    -- extract of it, read from a generic JSON decode, never the source of
    -- truth.
    event  String CODEC(ZSTD(3)),

    -- Datadog's profiler sends start/end as RFC3339 strings; some producers
    -- send epoch numbers instead. *_raw keeps the wire value verbatim
    -- (json.Number's own text, never rounded through float64) so nothing is
    -- lost even when *_parsed could not make sense of it.
    start_raw     String,
    start_parsed  Nullable(DateTime64(3, 'UTC')),
    end_raw       String,
    end_parsed    Nullable(DateTime64(3, 'UTC')),

    family    LowCardinality(String),
    version   LowCardinality(String),
    runtime   LowCardinality(String),
    language  LowCardinality(String),

    -- tags_profiler arrives as a THIRD tag convention on top of the flat
    -- array and the map[string]string the rest of the intake sees: a single
    -- comma-joined string. Split with splitDDTags, then the same multiset
    -- shape as every other Datadog tag column —
    -- docs/decisions/0001-tags-are-a-multiset.md.
    tags_profiler  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- A hot column pulled out of tags_profiler for the point-query pattern
    -- every other tagged table gets ("one service, time range").
    service  LowCardinality(String) MATERIALIZED tags_profiler['service'][1],

    -- Attachments: parallel arrays, one entry per multipart part other than
    -- "event", ordered by part name (router_profiling.go's profileRow walks
    -- parts via sortedKeys) — NOT wire order, which a map[string][]byte never
    -- preserved to begin with; see docs/tables/profiling.md.
    attach_name             Array(String),
    -- Raw bytes exactly as the part arrived — gzip if the attachment was,
    -- since profile.ParseData undoes that internally and this column is the
    -- undecoded original. Light codec: these are frequently pre-compressed,
    -- so a heavier one buys little.
    attach_bytes            Array(String) CODEC(ZSTD(1)),
    attach_size             Array(UInt64),
    -- 1 when profile.ParseData accepted the part as pprof; every other
    -- attach_* field at that index is zero-valued (not NULL — there is no
    -- per-attachment Nullable here, see docs/tables/profiling.md) when 0.
    attach_parsed           Array(UInt8),
    attach_sample_types     Array(Array(String)),
    attach_sample_units     Array(Array(String)),
    attach_sample_count     Array(UInt64),
    attach_time_nanos       Array(Int64),
    attach_duration_nanos   Array(Int64),
    attach_period_type      Array(String),
    attach_period           Array(Int64),
    attach_mapping_count    Array(UInt32),
    attach_location_count   Array(UInt32),
    attach_function_count   Array(UInt32),

    INDEX idx_tag_keys   mapKeys(tags_profiler)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags_profiler)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, service, received_at)
-- Bulky-document class, same as k8s_manifests: 7 days is enough to catch "a
-- deploy regressed CPU usage sometime last week" without a multi-megabyte
-- attachment per row filling a disk.
TTL toDateTime(received_at) + INTERVAL 7 DAY
SETTINGS index_granularity = 8192;

-- ---------------------------------------------------------------------------
-- debugger_logs: the "json" variant of /api/v2/debugger — a JSON array or
-- NDJSON batch of Dynamic Instrumentation log/snapshot entries.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS debugger_logs
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    service   LowCardinality(String),
    ddsource  LowCardinality(String),

    -- The ?ddtags= query string, split and parsed as a multiset — it travels
    -- alongside every entry in the batch, not per entry.
    ddtags  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- The array element (or NDJSON line), byte for byte.
    entry  String CODEC(ZSTD(3)),

    -- Top-level keys of entry other than service/ddsource: there is no fixed
    -- schema here to decode against (the upstream dyninst logSender type is
    -- not published), so "extra" is everything else — same pattern as
    -- k8s_actions.extra_keys.
    extra_keys  Array(String),

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, service, received_at)
-- Trickle-in class, same as check_runs: DI log entries only exist while a
-- probe is attached to a running service, not continuously like the main
-- logs table, so its 14-day TTL is copied rather than invented.
TTL toDateTime(received_at) + INTERVAL 14 DAY
SETTINGS index_granularity = 8192;

-- ---------------------------------------------------------------------------
-- debugger_diagnostics: the "diagnostics" variant of /api/v2/debugger — probe
-- status messages (uploader.DiagnosticMessage, decoded generically since the
-- upstream type is unexported).
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS debugger_diagnostics
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The message's own claimed timestamp, parsed best-effort. Nullable: a
    -- missing or unparseable value stays missing, it never becomes
    -- received_at or the epoch.
    timestamp  Nullable(DateTime64(3, 'UTC')),

    service        LowCardinality(String),
    ddsource       LowCardinality(String),

    -- The ?ddtags= query string, split and parsed as a multiset — same
    -- convention as debugger_logs.ddtags. It travels alongside every message
    -- in the diagnostics batch, not per message.
    ddtags  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    runtime_id     String,
    probe_id       String,
    status         LowCardinality(String),
    probe_version  String,

    -- debugger.diagnostics.exception is optional on the wire; both columns
    -- are Nullable together so "no exception" and "an exception with an
    -- empty message" stay distinguishable.
    exception_type     Nullable(String),
    exception_message  Nullable(String),

    -- The array element, byte for byte.
    message  String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, service, received_at)
-- Human-scale class, same as events/k8s_actions: a probe's status changes a
-- handful of times per install, and this is the audit trail of that, worth
-- keeping as long as those tables' 90 days.
TTL toDateTime(received_at) + INTERVAL 90 DAY
SETTINGS index_granularity = 8192;

-- ---------------------------------------------------------------------------
-- symdb_uploads: the "symdb" variant of /api/v2/debugger — multipart with
-- both a file (gzip'd JSON) and an event part.
--
-- The event part and the envelope inside the gzip'd file describe the same
-- upload but are two different producers/serializers: the event spells its
-- keys camelCase (uploadId), the file's envelope spells them snake_case
-- (upload_id). Both are kept, under env_-prefixed names for the file's
-- spelling, rather than merged into one guess — the drift itself is worth
-- being able to see.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS symdb_uploads
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The "event" part, byte for byte, before any decode — the lossless
    -- copy every column from service to attachment_size below is a
    -- convenience extract of, same pattern as profiles.event /
    -- symbol_uploads.meta / debugger_logs.entry.
    event  String CODEC(ZSTD(3)),

    -- The ?ddtags= query string, split and parsed as a multiset — same
    -- convention as debugger_logs.ddtags. It travels alongside the whole
    -- upload, not per scope.
    ddtags  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    service      LowCardinality(String),
    version      LowCardinality(String),
    language     LowCardinality(String),
    runtime_id   String,

    -- Identifiers, not quantities — kept as the wire's literal text rather
    -- than parsed to an integer, since nothing guarantees they stay numeric
    -- across producers.
    upload_id  String,
    batch_num  String,

    -- Nullable: the event may not carry these at all, which is a different
    -- state from "false" / "0 bytes".
    final            Nullable(UInt8),
    attachment_size  Nullable(UInt64),

    -- The gzip'd file part exactly as received — the lossless copy.
    -- inflated_size/scope_count/scopes_ok and the env_* columns below are
    -- convenience extracts of what is already fully present here.
    file           String CODEC(ZSTD(1)),
    inflated_size  UInt64,
    -- Top-level scopes only; nested child scopes are not counted here — see
    -- docs/tables/profiling.md.
    scope_count    UInt32,
    -- 1 when the envelope's "scopes" key was present and decoded; 0 when it
    -- was missing or malformed. The envelope itself may still have decoded
    -- fine when this is 0 — env_* stay populated independently.
    scopes_ok      UInt8,

    env_service     String,
    env_version     String,
    env_language    String,
    env_upload_id   String,
    env_batch_num   String,
    env_final       String,

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, service, received_at)
-- Bulky-document class, same as k8s_manifests: file carries an entire gzip'd
-- symbol-database batch.
TTL toDateTime(received_at) + INTERVAL 7 DAY
SETTINGS index_granularity = 8192;

-- ---------------------------------------------------------------------------
-- symbol_uploads: sourcemap-intake.<site>'s /api/v2/srcmap — native symbol
-- uploads from the host profiler (elf_symbol_file + event).
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS symbol_uploads
(
    tenant_id    LowCardinality(String),
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    type            LowCardinality(String),
    arch            LowCardinality(String),
    gnu_build_id    String,
    go_build_id     String,
    file_hash       String,
    symbol_source   LowCardinality(String),
    origin          LowCardinality(String),
    origin_version  String,
    filename        String,

    -- The "event" part, byte for byte — the lossless copy of which the nine
    -- columns above are convenience extracts (symbolUploadRequestMetadata
    -- from comp/host-profiler/symboluploader is unexported upstream, hence
    -- the generic decode on the intake side).
    meta  String CODEC(ZSTD(3)),

    has_elf         UInt8,
    -- "ELF32"/"ELF64"/"ELF?"/"" and "LE"/"BE"/"?"/"" — "" together with
    -- has_elf = 0 when no elf_symbol_file part was sent at all.
    elf_class       LowCardinality(String),
    elf_endianness  LowCardinality(String),
    -- The elf_symbol_file part, raw bytes, unparsed — no ELF section/symbol
    -- parsing is attempted anywhere in this router.
    elf       String CODEC(ZSTD(1)),
    elf_size  UInt64,

    -- Every multipart part besides "event" and "elf_symbol_file" — nothing
    -- this handler names, but nothing a future producer sends is thrown away
    -- either. Not a Datadog tags map, so it stays a plain Map.
    other_parts  Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
-- symbol_source (e.g. the uploader identifying itself) is the closest thing
-- this payload has to a bounded "which producer" column — there is no
-- service field on a symbol upload, and a build id is exactly the wrong
-- shape for a sort prefix (unique per build, not a grouping key).
ORDER BY (tenant_id, symbol_source, received_at)
-- Bulky-document class, same as k8s_manifests: elf carries a whole ELF
-- symbol file.
TTL toDateTime(received_at) + INTERVAL 7 DAY
SETTINGS index_granularity = 8192;
