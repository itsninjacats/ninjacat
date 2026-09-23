-- CI Visibility / Test Optimization, CI provider webhooks, git metadata and
-- the synthetics agent poller's test catalogue.
--
-- No CREATE DATABASE and no database qualifier, same as every other
-- migration: the runner applies every statement over a connection already
-- opened against the target database, and qualifying would break running
-- against a scratch one.
--
-- WHY THIS IS A FAMILY OF TABLES AND NOT ONE. CI Visibility is not one
-- protocol; it is six wire formats that happen to share a product name:
--
--   citestcycle-intake.<site>  msgpack (or JSON) test/suite/module/session
--                              events        -> ci_test_events
--   citestcov-intake.<site>    multipart, msgpack per-test line coverage
--                                            -> ci_coverage
--   webhook-intake.<site>      gzip'd JSON array of Jenkins/GitLab CI
--                              pipeline spans -> ci_webhook_events
--   api.<site> git endpoints   JSON:API commit search + multipart packfile
--                                            -> git_commits, git_packfiles
--   api.<site> tracer config   settings / skippable / known tests / test
--                              management     -> ci_settings_requests
--   api.<site> datadog-ci      tag / measure / custom spans
--                                            -> ci_pipeline_events
--
-- They are joined by ids, not by shape: forcing them into one table would
-- mean a row that is mostly NULL whichever producer wrote it, and the
-- "one table per payload shape" rule from 0001_initial.sql exists for
-- exactly this. What they DO share is the tenant-first ORDER BY and the
-- "declared columns plus a catch-all map plus the raw body" layering used
-- throughout this schema.
--
-- SOURCES. Every field below was read from a client that actually sends it,
-- not from documentation:
--   dd-trace-go@main ddtrace/tracer/civisibility_tslv.go (the event/tslvSpan
--     wire structs and the canonical JSON the msgpack round-trips to),
--     internal/civisibility/utils/net/{settings,skippable,known_tests,
--     test_management_tests,searchcommits,sendpackfiles,coverage}_api.go
--   datadog-agent@v0.0.0-20260521051500-e70f483e68d2
--     comp/syntheticstestscheduler/{impl/testpoller.go,common/data.go}
--   jenkinsci/datadog-plugin DatadogWebhookBuildLogic/PipelineLogic (via
--     the research spec in docs/tables/ci_visibility.md)
-- The backend-side field catalogue (DataDog/datadog-ci-spec) is not public,
-- so "complete" here means "complete with respect to what the shipped
-- clients write" — which is why every table keeps a raw column.

-- ---------------------------------------------------------------------------
-- ci_test_events — POST /api/v2/citestcycle on citestcycle-intake.<site>
--
-- One row per EVENT, not per payload: a payload is an envelope
-- {version, metadata, events[]} and each event is a test, a suite end, a
-- module end, a session end or a plain span (civisibility_tslv.go's doc
-- comment lists all five shapes). The envelope's per-event-type metadata map
-- (language, runtime-id, library_version, env) is merged into each event's
-- own row rather than kept in a parent table, because every query anyone
-- will write starts from a test, and a join to recover "which language ran
-- this" would be paid on every one of them.
--
-- The uint64 ids are UInt64 columns end-to-end and are decoded through
-- json.Number / msgpack integers, never float64: a test_session_id above
-- 2^53 that round-tripped through a float would silently join to nothing.
CREATE TABLE IF NOT EXISTS ci_test_events
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The outer event's "type" and "version". version is 2 for type "test"
    -- and 1 for everything else (civisibility_tslv.go:298 vs :323/:348/:371);
    -- it is stored because it is the only version negotiation this protocol
    -- has, so a payload that starts arriving as version 3 must be visible.
    event_type         LowCardinality(String),
    event_version      Int32 CODEC(T64, ZSTD(1)),
    -- The envelope's own "version" field (1 today).
    payload_version    Int32 CODEC(T64, ZSTD(1)),

    -- Promoted out of content.meta by the tracer into top-level content
    -- fields (getAndRemoveMetaToUInt64). Which of the three is non-zero says
    -- what the event is: a test carries all three, test_suite_end carries
    -- session+module+suite, test_module_end session+module, test_session_end
    -- session only. 0 means "this event type does not carry it", which is
    -- why these are not Nullable — the absence is structural, not unknown.
    session_id         UInt64 CODEC(T64, ZSTD(1)),
    module_id          UInt64 CODEC(T64, ZSTD(1)),
    suite_id           UInt64 CODEC(T64, ZSTD(1)),

    -- Only "test" and "span" events carry these; the *_end events zero them
    -- and msgp omits them (omitempty).
    trace_id           UInt64 CODEC(T64, ZSTD(1)),
    span_id            UInt64 CODEC(T64, ZSTD(1)),
    parent_id          UInt64 CODEC(T64, ZSTD(1)),

    -- itr_correlation_id, echoed back from a skippable-tests response so the
    -- backend can tie a skip decision to the run that honoured it.
    itr_correlation_id String,

    service            LowCardinality(String),
    env                LowCardinality(String),
    name               LowCardinality(String),
    resource           String,
    -- content.type, which repeats the outer type. Kept separately because
    -- "repeats" is an observation about today's clients, not a guarantee.
    span_type          LowCardinality(String),

    -- start is ns since epoch on the wire. DateTime64(9) is the only
    -- precision that keeps it exactly; truncating to ms would make two test
    -- events in the same millisecond unorderable, and ordering tests inside
    -- a suite is the whole point of the column.
    start              DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    duration_ns        Int64 CODEC(T64, ZSTD(1)),
    -- 0 means no error (tslvSpan.Error).
    error              Int32 CODEC(T64, ZSTD(1)),

    -- test.* hot fields, lifted out of meta. They stay in meta as well: this
    -- is duplication for query speed, not extraction, so a producer that
    -- renames one of these tags loses a column and keeps the data.
    test_name              LowCardinality(String),
    test_suite             LowCardinality(String),
    test_module            LowCardinality(String),
    test_framework         LowCardinality(String),
    test_framework_version LowCardinality(String),
    test_status            LowCardinality(String),
    test_type              LowCardinality(String),
    test_source_file       String,
    -- Nullable because "no source location reported" and "line 0" are
    -- different answers and only one of them is possible.
    test_source_start      Nullable(Int64),
    test_source_end        Nullable(Int64),
    test_parameters        String CODEC(ZSTD(3)),
    test_codeowners        String CODEC(ZSTD(3)),
    test_command           String,
    test_session_name      String,

    -- git.*
    git_repository_url        String,
    git_branch                LowCardinality(String),
    git_tag                   LowCardinality(String),
    git_commit_sha            String,
    git_commit_message        String CODEC(ZSTD(3)),
    git_commit_author_name    String,
    git_commit_author_email   String,
    git_commit_author_date    String,
    git_commit_committer_name  String,
    git_commit_committer_email String,
    git_commit_committer_date  String,

    -- ci.*
    ci_provider_name    LowCardinality(String),
    ci_pipeline_id      String,
    ci_pipeline_name    String,
    ci_pipeline_number  String,
    ci_pipeline_url     String,
    ci_job_id           String,
    ci_job_name         String,
    ci_job_url          String,
    ci_stage_name       String,
    ci_workspace_path   String,
    ci_node_name        String,
    -- ci.node.labels arrives as one tag whose value is a JSON array; kept as
    -- the string the tracer sent rather than parsed, so a producer that
    -- changes the encoding does not silently produce an empty list.
    ci_node_labels      String,

    -- os.* / runtime.*, and the three the ENVELOPE metadata carries rather
    -- than the span (language, runtime_id, library_version).
    os_platform         LowCardinality(String),
    os_version          LowCardinality(String),
    os_architecture     LowCardinality(String),
    runtime_name        LowCardinality(String),
    runtime_version     LowCardinality(String),
    language            LowCardinality(String),
    runtime_id          String,
    library_version     LowCardinality(String),

    -- ITR / early-flake-detection / test-management flags. Nullable(UInt8)
    -- rather than UInt8: a tracer with the feature off sends no tag at all,
    -- and "the tracer never said" must not read as "false" when somebody
    -- later counts retried tests.
    test_is_new              Nullable(UInt8),
    test_is_retry            Nullable(UInt8),
    test_is_modified         Nullable(UInt8),
    test_skipped_by_itr      Nullable(UInt8),
    itr_unskippable          Nullable(UInt8),
    itr_forced_run           Nullable(UInt8),
    code_coverage_enabled    Nullable(UInt8),
    test_retry_reason        LowCardinality(String),
    early_flake_abort_reason LowCardinality(String),

    -- The agent's evp_proxy rewrites the request when a tracer tunnels
    -- through it instead of going agentless, and these four are what
    -- survives to say so. Header names are the REAL ones from
    -- datadog-agent/pkg/trace/api/evp_proxy.go: Datadog-Container-ID (no
    -- X- prefix) and X-Datadog-Hostname; the proxy consumes
    -- X-Datadog-EVP-Subdomain itself, so it is only present when something
    -- other than the agent set it, and it announces its version in
    -- "Via: trace-agent <version>" rather than a header of its own.
    evp_subdomain    LowCardinality(String),
    container_id     String,
    agent_hostname   String,
    agent_version    LowCardinality(String),

    -- meta and metrics verbatim, every key. The hot columns above are copies.
    -- meta is Map(k,String) and not the tags multiset shape: this is a
    -- msgpack map[string]string on the wire, so a duplicate key is not
    -- representable and the multiset would cost lookup speed for a collision
    -- that cannot happen (docs/decisions/0001-tags-are-a-multiset.md).
    meta             Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    metrics          Map(LowCardinality(String), Float64) CODEC(ZSTD(3)),
    -- The envelope's metadata for this event type, "*" merged under the
    -- per-type entry.
    metadata         Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- The event's content, rendered as JSON. msgpack in, JSON here: this
    -- column is read by a human debugging a payload, and the tracer's own
    -- doc comment documents the JSON shape, not the msgpack one.
    content          String CODEC(ZSTD(3)),

    -- Two bloom filters for the two lookups that do not start from the
    -- ORDER BY: "everything in this session" (one CI run) and "this test
    -- over time" (is it flaky). Both are point lookups on a high-cardinality
    -- column, which is what a bloom skip index is for. Declared inline
    -- rather than by ALTER so a re-run of a half-applied migration skips
    -- them with the table, per this schema's IF NOT EXISTS convention.
    INDEX idx_ci_session_id session_id TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_ci_test_name  test_name  TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(start)
-- (service, start) after tenant: every CI Visibility question starts "what
-- did this service's tests do lately". Monthly partitions because the TTL is
-- measured in months, per 0001_initial.sql's note that hundreds of daily
-- partitions slow query planning.
ORDER BY (tenant_id, service, start)
TTL toDateTime(start) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- ci_coverage — POST /api/v2/citestcov on citestcov-intake.<site>
--
-- multipart: a dummy JSON "event" part plus a coverage part that is msgpack
-- (or JSON) of {version, coverages: [{test_session_id, test_suite_id,
-- span_id, files: [{filename, bitmap}]}]}. One row per coverages[] entry.
--
-- The bitmap is opaque. dd-trace-go says so explicitly (skippable.go: "Go
-- coverage metadata is stored and returned by the backend as FileBitmap
-- bytes; the backend does not translate bitmap encodings") — the backend's
-- whole job is to hand the same bytes back on the next skippable-tests
-- response, so decoding it here would be work nobody asked for and a format
-- we would then have to track.
CREATE TABLE IF NOT EXISTS ci_coverage
(
    tenant_id       LowCardinality(String),
    received_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    payload_version Int32 CODEC(T64, ZSTD(1)),

    session_id      UInt64 CODEC(T64, ZSTD(1)),
    suite_id        UInt64 CODEC(T64, ZSTD(1)),
    -- Nullable: the encoder OMITS span_id when itr_suite_skipping_mode is on
    -- (dd-trace-py encoder.py:375), because suite-level coverage has no
    -- per-test span to attribute to. A 0 here would be a real span id.
    span_id         Nullable(UInt64),

    -- Parallel arrays rather than a Nested type: ClickHouse's Nested IS two
    -- parallel arrays, and the flat spelling is what every other table in
    -- this schema uses for repeated fields. A file with no bitmap (coverage
    -- known at file granularity only) keeps an empty string at its index, so
    -- the two arrays always have the same length.
    files_filename  Array(String) CODEC(ZSTD(3)),
    files_bitmap    Array(String) CODEC(ZSTD(3)),

    -- This entry's bytes exactly as they arrived, msgpack or JSON, delimited
    -- out of the part without re-encoding. raw_format says which.
    raw             String CODEC(ZSTD(3)),
    raw_format      LowCardinality(String),

    -- The companion "event" part. It is literally {"dummy": true} in both
    -- shipped clients; kept because "literally" is an observation about
    -- today.
    event           String CODEC(ZSTD(3)),

    -- Keys of the coverage entry this decoder does not know.
    extra           Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    evp_subdomain   LowCardinality(String),
    container_id    String,
    agent_hostname  String,
    agent_version   LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
-- Coverage is never queried by time; it is queried by "the session I am
-- deciding skips for", so the session id comes first after the tenant.
ORDER BY (tenant_id, session_id, suite_id, received_at)
-- The same 90 days as ci_test_events on purpose: a coverage row whose test
-- events have aged out cannot be interpreted, so the two must expire together.
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- git_commits — the commit shas a tracer or datadog-ci told us about.
--
-- Fed by POST /api/v2/git/repository/search_commits, whose body is the
-- tracer's LOCAL commit list asking which of them we already have, and by
-- the pushedSha of every packfile upload. So a row means "this sha was
-- mentioned for this repository", which is the only fact either request
-- actually carries.
--
-- ReplacingMergeTree keyed on the identity (repo, sha) with seen_at as the
-- version, like hosts and container_images: the same sha is mentioned once
-- per CI run forever, and what we want is one row per commit with the latest
-- sighting, not a million duplicates. Readers must use FINAL or argMax.
CREATE TABLE IF NOT EXISTS git_commits
(
    tenant_id      LowCardinality(String),
    repository_url String,
    sha            String,
    seen_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Set when the sha arrived as a packfile upload's pushedSha, and it then
    -- points at git_packfiles.packfile_id. NULL when the sha only ever came
    -- from a search_commits candidate list — which is the common case and is
    -- NOT the same as "we have the objects".
    packfile_id    Nullable(String),

    -- Which request mentioned it: "search_commits" or "packfile".
    source         LowCardinality(String)
)
ENGINE = ReplacingMergeTree(seen_at)
ORDER BY (tenant_id, repository_url, sha);
-- No TTL, deliberately. A commit is an identity, not an event: the row is
-- the answer to "do we know this sha", and an answer that expires turns into
-- a wrong answer rather than a missing one. Same reasoning as hosts.

-- ---------------------------------------------------------------------------
-- git_packfiles — POST /api/v2/git/repository/packfile
--
-- multipart with a JSON "pushedSha" part ({data:{id,type:"commit"},
-- meta:{repository_url}}) and a "packfile" part of raw git pack bytes, one
-- request per .pack file (dd-trace-go sendpackfiles_api.go:54-89).
CREATE TABLE IF NOT EXISTS git_packfiles
(
    tenant_id      LowCardinality(String),
    received_at    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    repository_url String,
    pushed_sha     String,

    -- sha256 of the pack bytes, hex. The upload carries no id of its own, so
    -- this is what git_commits.packfile_id refers to, and it makes a
    -- re-uploaded identical pack recognisable.
    packfile_id    String,
    filename       String,

    -- The pack itself. ZSTD(3) like every other bulky cold column here
    -- (raw_payloads.body, agent_flares.archive): nothing filters on it, it is
    -- read whole by whoever wants the objects. Note a git pack is already
    -- compressed, so expect little from ZSTD — the codec is for uniformity,
    -- not for a win.
    packfile       String CODEC(ZSTD(3)),
    size_bytes     UInt64 CODEC(T64, ZSTD(1)),

    -- Any multipart part that is neither pushedSha nor packfile.
    other_parts    Map(LowCardinality(String), String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, repository_url, pushed_sha, received_at)
-- Bulky-document class, like k8s_manifests: 90 days matches ci_test_events,
-- since the reason to keep a pack is to resolve a commit some test event
-- refers to.
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- ci_settings_requests — the four api.<site> endpoints a tracer CONSULTS.
--
--   settings          POST /api/v2/libraries/tests/services/setting
--   skippable         POST /api/v2/ci/tests/skippable
--   known_tests       POST /api/v2/ci/libraries/tests
--   flaky_tests       POST /api/v2/ci/libraries/tests/flaky
--   test_management   POST /api/v2/test/libraries/test-management/tests
--
-- These are READS from the tracer's point of view, so there is no telemetry
-- to keep — what there is instead is the question and our answer, and that
-- pair is worth a table for one reason: the moment any of these features is
-- implemented, "which tracers asked, for which repo, at which granularity,
-- and what did we tell them" is the entire migration plan. It also makes a
-- tracer stuck in a retry loop visible.
CREATE TABLE IF NOT EXISTS ci_settings_requests
(
    tenant_id      LowCardinality(String),
    received_at    DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    endpoint       LowCardinality(String),

    -- data.id and data.type of the JSON:API envelope. The tracer generates
    -- id per process and the backend is told to correlate every transaction
    -- of one client by it (client.go:190-193).
    request_id     String,
    request_type   LowCardinality(String),

    service        LowCardinality(String),
    env            LowCardinality(String),
    repository_url String,
    branch         LowCardinality(String),
    sha            String,
    -- "test" or "suite" — only the skippable endpoint sends it.
    test_level     LowCardinality(String),
    -- test-management only.
    module         String,
    commit_message String CODEC(ZSTD(3)),
    -- known-tests pagination cursor; empty on the first page.
    page_state     String,

    -- attributes.configurations, the os.*/runtime.*/custom bag that decides
    -- which stored results are comparable. JSON rather than a Map because
    -- "custom" is a nested object.
    configurations String CODEC(ZSTD(3)),

    -- Both sides verbatim. The response is stored because it is a contract
    -- we invented (every feature off) and the day it changes, knowing what a
    -- given tracer was told is the difference between a bug and a mystery.
    request_body   String CODEC(ZSTD(3)),
    response_body  String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, service, received_at)
-- 30 days and daily partitions: this is diagnostic traffic, one request per
-- test process, and it is read while somebody is actively wiring a tracer up.
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- ci_webhook_events — POST /api/v2/webhook[/] on webhook-intake.<site>
--
-- Datadog's one generic "CI provider webhook" intake: the Jenkins plugin
-- posts here (DatadogApiClient.java: webhook-intake.<site>/api/v2/webhook/
-- ?service=<ci instance>, headers DD-API-KEY + DD-CI-PROVIDER-NAME, gzip'd
-- JSON ARRAY batched to 5 MB uncompressed) and GitLab's legacy webhook
-- integration is documented against the identical URL.
--
-- The batch mixes levels: each element carries "level": "pipeline" | "stage"
-- | "job" and the backend tells them apart by that field, not by URL. One
-- table with a level column is therefore the shape the producer already
-- assumes, and the same argument 0001_initial.sql makes for k8s_resources
-- (shared columns are real columns; the rest is a map).
--
-- A provider we do not recognise still gets a full row: provider, whatever
-- hot columns matched, the header allowlist, and the element's raw JSON.
CREATE TABLE IF NOT EXISTS ci_webhook_events
(
    tenant_id       LowCardinality(String),
    received_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- DD-CI-PROVIDER-NAME when the sender sets it (Jenkins does), otherwise
    -- sniffed from the body, otherwise "unknown" — never guessed into a real
    -- provider name.
    provider        LowCardinality(String),
    -- "pipeline" | "stage" | "job" for Datadog's schema; GitLab's native
    -- webhook object_kind lands here too when that is what arrived.
    level           LowCardinality(String),
    -- The ?service= query parameter the Jenkins plugin appends: its CI
    -- instance name.
    service         LowCardinality(String),

    payload_version Nullable(Int64),
    partial_retry   Nullable(UInt8),
    is_manual       Nullable(UInt8),

    trace_id        String,
    span_id         String,
    parent_span_id  String,
    id              String,
    unique_id       String,
    pipeline_id     String,
    pipeline_unique_id String,
    pipeline_name   String,
    stage_id        String,
    stage_name      String,
    parent_stage_id String,

    name            String,
    url             String,
    status          LowCardinality(String),

    -- ISO8601 on the wire. The raw string is kept next to the parse so a
    -- format we do not recognise loses the index, never the value, and a
    -- missing timestamp stays NULL instead of becoming 1970.
    start_raw       String,
    start_parsed    Nullable(DateTime64(3, 'UTC')),
    end_raw         String,
    end_parsed      Nullable(DateTime64(3, 'UTC')),
    queue_time_ms   Nullable(Int64),

    node_name       String,
    node_hostname   String,
    node_workspace  String,
    node_labels     Array(String) CODEC(ZSTD(3)),

    git_repository_url  String,
    git_default_branch  LowCardinality(String),
    git_branch          LowCardinality(String),
    git_sha             String,
    git_tag             LowCardinality(String),
    git_message         String CODEC(ZSTD(3)),
    git_author_name     String,
    git_author_email    String,
    git_author_time     String,
    git_committer_name  String,
    git_committer_email String,
    git_commit_time     String,

    user_name       String,
    user_email      String,

    error_message   String CODEC(ZSTD(3)),
    error_type      LowCardinality(String),
    error_domain    LowCardinality(String),
    error_stack     String CODEC(ZSTD(3)),

    parent_pipeline_trace_id String,
    parent_pipeline_url      String,

    -- Build parameters: a JSON object, so keys are unique by construction.
    parameters      Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- tags, on the other hand, is an ARRAY of "key:value" strings and is a
    -- genuine Datadog tag multiset — the Jenkins plugin emits repeated keys
    -- for per-configuration axes. Hence the array-valued map; see
    -- docs/decisions/0001-tags-are-a-multiset.md for what a plain map costs.
    tags            Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- Provider delivery identifiers, for deduplicating a redelivered hook.
    delivery_id     String,

    -- Request headers, from the same kind of allowlist intake/raw.go uses:
    -- identification only, never a credential.
    headers         Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- Top-level keys of the element this decoder does not name.
    extra           Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- The element verbatim. A CI provider adds fields without telling
    -- anybody, and this is the column that makes that discoverable.
    body            String CODEC(ZSTD(3)),

    -- "show me this pipeline's stages and jobs" is the one webhook query
    -- that does not start from (provider, received_at).
    INDEX idx_ci_webhook_pipeline pipeline_unique_id TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, provider, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- ci_pipeline_events — the datadog-ci CLI's own write endpoints on api.<site>
--
--   POST /api/v2/ci/pipeline/tags       datadog-ci tag      ci_custom_tag
--   POST /api/v2/ci/pipeline/metrics    datadog-ci measure  ci_custom_metric
--   POST /api/intake/ci/custom_spans    datadog-ci trace    ci_app_custom_span
--
-- All three are JSON:API {data:{type, attributes}} and all three carry
-- ci_env: the raw provider environment-variable snapshot the backend uses to
-- correlate the call with a pipeline some webhook or tracer already
-- reported. That correlation is the reason ci_env is a real Map and not just
-- part of the raw body — it is the join key.
CREATE TABLE IF NOT EXISTS ci_pipeline_events
(
    tenant_id   LowCardinality(String),
    received_at DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Our label for the route: "tag" | "measure" | "custom_span".
    kind        LowCardinality(String),
    -- data.type as the CLI sent it.
    data_type   LowCardinality(String),

    provider    LowCardinality(String),
    -- 1 = pipeline, 2 = stage, 3 = job (datadog-ci's LEVEL_TO_NUMBER).
    -- Nullable because a custom span sends no level at all.
    ci_level    Nullable(Int64),
    ci_env      Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- tag: a JSON OBJECT of key -> value, so unique keys, so a plain map.
    -- This is not the "key:value" string list that makes a tag multiset.
    tags        Map(LowCardinality(String), String) CODEC(ZSTD(3)),
    -- measure: key -> float.
    metrics     Map(LowCardinality(String), Float64) CODEC(ZSTD(3)),

    -- custom_span hot fields, best effort: the CLI's Payload type was not
    -- readable from a primary source, so these are filled when the obvious
    -- names are present and left empty otherwise. attributes below always
    -- holds the truth.
    span_name       String,
    span_start_raw  String,
    span_end_raw    String,

    -- data.attributes verbatim, and the whole request under it.
    attributes  String CODEC(ZSTD(3)),
    body        String CODEC(ZSTD(3)),

    extra       Map(LowCardinality(String), String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, kind, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- synthetics_test_configs — the catalogue the agent's synthetics poller reads.
--
-- NOT fed by an intake. The agent GETs
-- intake.synthetics.<site>/api/unstable/synthetics/agents/tests every two
-- seconds and runs whatever comes back (datadog-agent
-- comp/syntheticstestscheduler/impl/testpoller.go); the results come back to
-- http-synthetics.<site> and land in synthetics_results, which already
-- exists. This table is the other half: what a future panel writes and the
-- poller reads.
--
-- It is created now, empty, because the poller handler has to answer
-- {"tests": []} from somewhere, and a TODO pointing at a table that exists
-- is a different kind of TODO from one pointing at a table that does not.
--
-- Columns are exactly common.SyntheticsTestConfig (comp/
-- syntheticstestscheduler/common/data.go), including the discriminator the
-- agent's custom UnmarshalJSON reads: an UNKNOWN subtype makes the agent
-- fail the WHOLE poll, not just that test, so `subtype` must only ever hold
-- "UDP", "TCP" or "ICMP".
CREATE TABLE IF NOT EXISTS synthetics_test_configs
(
    tenant_id   LowCardinality(String),
    -- ReplacingMergeTree version column: every edit rewrites the row.
    updated_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    public_id   String,
    version     Int32 CODEC(T64, ZSTD(1)),
    type        LowCardinality(String),
    subtype     LowCardinality(String),

    -- tick_every on the wire, in seconds.
    tick_every  Int32 CODEC(T64, ZSTD(1)),
    org_id      Int64 CODEC(T64, ZSTD(1)),
    main_dc     LowCardinality(String),
    result_id   String,
    run_type    LowCardinality(String),

    -- config.assertions: [{operator, property, target, type}]. JSON rather
    -- than parallel arrays because the poller hands it straight back out as
    -- JSON and nothing here ever filters on an assertion.
    assertions  String CODEC(ZSTD(3)),
    -- config.request: the subtype-specific object (UDP/TCP/ICMP each have
    -- their own shape). JSON for the same reason.
    request     String CODEC(ZSTD(3)),

    -- Whether the poller should hand this test out. A disabled test is kept,
    -- not deleted: "paused" is a state a UI needs.
    enabled     UInt8
)
ENGINE = ReplacingMergeTree(updated_at)
ORDER BY (tenant_id, public_id);
-- No TTL: a test configuration is a live object, like hosts. It leaves only
-- by being replaced.
