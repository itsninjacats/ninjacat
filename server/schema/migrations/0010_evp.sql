-- EVP misc intakes: six hosts, seven routes, none stored before this file.
--
-- Nothing here shares a wire shape with anything else in the schema, so each
-- table follows the payload it is named for rather than a shared pattern.
-- Two general choices repeat throughout, though:
--
--   - Most of these payloads carry their own timestamp only as a string
--     (EmittedAt, DetectedAt, testStartedAt, eventTime, ...), in a format the
--     producer defines and the agent does not validate. Every such field gets
--     TWO columns: the string exactly as sent, and a Nullable(DateTime64) that
--     holds the parsed value when it happens to be RFC3339 and NULL otherwise
--     — never a fallback to now(), which would silently invent a timestamp
--     the sender never gave us. received_at (arrival time, always present) is
--     what PARTITION BY and the TTL key off, since it is the one time value
--     every row actually has.
--
--   - Several payloads are explicitly schemaless at some level (structpb
--     Struct, OpenLineage facets, Python integration results, undeclared
--     JSON keys). Those stay String columns holding the sub-object's JSON
--     verbatim rather than being flattened into columns that would need a
--     schema change every time a producer adds a field.
--
-- See server/docs/tables/evp.md for the field-by-field source mapping and the
-- dropped_by_decision list.

-- ---------------------------------------------------------------------------
-- Agent discovery: /api/v2/agentdiscovery
--
-- One row per AgentDiscoveryPayload (a batch covers one host_id, several
-- integration/runtime payloads). config_files travel as four parallel arrays
-- rather than a Nested column — ClickHouse's Nested is sugar over the same
-- parallel-array storage with none of the flexibility, and this table
-- has no second nested structure competing for the name.
--
-- env_vars keeps VALUES, not just names: HandleAgentDiscovery's own log line
-- redacts them (names only) precisely because they are credential-shaped
-- (API keys, DB passwords an integration config embeds), but the row is the
-- one thing this endpoint exists to capture — see server/intake/router_evp.go.
-- This is an explicit product decision, not an oversight: the operator who
-- runs this table owns what reads it. Flagged sensitive below and in the
-- docs table.
--
-- What is NOT recoverable: proto.Unmarshal discards fields the linked
-- agent-payload version does not declare, and there is no unknown-field
-- sink at the proto layer the way there is for JSON — a newer agent sending
-- a field this schema predates is silently dropped by the decode itself,
-- before this table ever sees it.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS agent_discovery
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    host_id            LowCardinality(String),
    integration        LowCardinality(String),
    runtime            LowCardinality(String),
    runtime_id         String,
    ingestion_timestamp Nullable(DateTime64(3, 'UTC')),

    -- Parallel arrays, index-aligned: config_paths[i] describes
    -- config_contents[i]. PayloadFormat travels by its enum name, matching
    -- the Enum8 convention used for check_runs.status.
    config_paths       Array(String),
    config_contents    Array(String) CODEC(ZSTD(3)),
    config_truncated   Array(UInt8),
    config_formats     Array(LowCardinality(String)),

    -- SENSITIVE: values, not just names — see the header comment above.
    env_vars           Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, host_id, integration, runtime, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Agent health: /api/v2/agenthealth
--
-- One HealthReport per request, split into a report row and one issue row
-- per entry of its Issues map — the map key is itself data (see issue_key
-- below), and Go map iteration order is random, so the conversion sorts
-- issue keys before building rows: two runs over the same report must
-- produce the same row order, or a diff between two ingests of "the same"
-- payload would be meaningless.
--
-- report_id is generated at ingest (the wire has no report-level id, only a
-- map of per-issue ids) and is the join key between the two tables.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS agent_health_reports
(
    tenant_id         LowCardinality(String),
    received_at       DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    report_id         UUID,
    schema_version    LowCardinality(String),
    event_type        LowCardinality(String),
    emitted_at        String,
    emitted_at_parsed Nullable(DateTime64(3, 'UTC')),
    service           LowCardinality(String),
    host              LowCardinality(String),
    -- ptr-optional on the wire (HostInfo.AgentVersion *string): nil must
    -- stay NULL, not "".
    agent_version     Nullable(String),
    par_ids           Array(String),
    issue_count       UInt32 CODEC(T64, ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, host, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

CREATE TABLE IF NOT EXISTS agent_health_issues
(
    tenant_id                LowCardinality(String),
    received_at              DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    report_id                UUID,
    -- The Issues map key — not the same as Issue.Id below, though producers
    -- usually set them equal. Kept as its own column because the map key is
    -- what the wire actually uses to address one issue.
    issue_key                String,
    id                       String,
    issue_name               LowCardinality(String),
    title                    String CODEC(ZSTD(1)),
    description              String CODEC(ZSTD(3)),
    category                 LowCardinality(String),
    location                 LowCardinality(String),
    severity                 LowCardinality(String),   -- Issue.Severity enum name
    detected_at               String,
    detected_at_parsed        Nullable(DateTime64(3, 'UTC')),
    source                   LowCardinality(String),
    -- structpb.Struct, genuinely schemaless per issue — JSON, not columns.
    extra                    String CODEC(ZSTD(3)),

    remediation_summary      String CODEC(ZSTD(1)),
    -- Parallel arrays, index-aligned: remediation_step_order[i] pairs with
    -- remediation_step_text[i].
    remediation_step_order   Array(Int32),
    remediation_step_text    Array(String) CODEC(ZSTD(3)),
    script_language          LowCardinality(String),
    script_language_version  LowCardinality(String),
    script_filename          String,
    script_requires_root     UInt8,
    script_content           String CODEC(ZSTD(3)),

    tags                     Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    persisted_state          LowCardinality(String),   -- PersistedIssue.State enum name
    first_seen               String,
    last_seen                String,
    -- ptr-optional on the wire (PersistedIssue.ResolvedAt *string): nil
    -- means "not resolved", which is not the same as an empty string.
    resolved_at              Nullable(String),
    issue_type               LowCardinality(String),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, report_id, issue_key)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- Event management: /api/v2/events
--
-- One JSON:API-shaped envelope per request, no Go type — each producer
-- builds its own map[string]any. attributes below is the INNER
-- data.attributes.attributes object, kept nested and NOT merged with the
-- outer columns: the two share the key name "attributes" by the wire's own
-- design, and flattening them would let one clobber the other.
--
-- outer_extra catches keys at the data.attributes level this table does not
-- name a column for — every producer-specific key that is not one of the
-- ones the intake documents (host, title, category, integration_id, message,
-- timestamp, tags, aggregation_key, attributes, "system-notable-events").
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS event_management_events
(
    tenant_id           LowCardinality(String),
    received_at         DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    data_type           LowCardinality(String),   -- data.type, e.g. "event"
    host                LowCardinality(String),
    title               String CODEC(ZSTD(1)),
    category            LowCardinality(String),
    integration_id      LowCardinality(String),
    message             String CODEC(ZSTD(3)),
    timestamp           String,
    timestamp_parsed    Nullable(DateTime64(3, 'UTC')),

    tags                Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    aggregation_key     String CODEC(ZSTD(1)),
    -- From the hyphenated "system-notable-events".event_type key — the
    -- Go map key travels as-is, never guessed at as a struct tag.
    notable_event_type  LowCardinality(String),
    -- The INNER data.attributes.attributes object, JSON, kept nested.
    attributes          String CODEC(ZSTD(3)),
    outer_extra         Map(String, String) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_aggkey     aggregation_key TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(received_at)
ORDER BY (tenant_id, received_at)
TTL toDateTime(received_at) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- Software inventory: /api/v2/softinv
--
-- One row per software entry, not per payload — "a full host inventory runs
-- to hundreds of lines" per the intake audit, so a payload-per-row table
-- would need an Array(Tuple(...)) or Nested column doing the same job with
-- worse ergonomics for a query like "every host running curl < 8.0".
--
-- extra holds undeclared keys on the SOFTWARE ENTRY (per pkg/inventory/
-- software, beyond software_type/name/version/publisher/deployment_status/
-- deployment_time/product_code/is_64_bit/install_paths); payload_extra holds
-- undeclared keys at the PAYLOAD level (beyond hostname/host_software). Both
-- are JSON-per-key maps, repeated onto every row of the payload they came
-- from — see docs/tables/evp.md for why that repetition is the right
-- trade-off here.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS host_software
(
    tenant_id                LowCardinality(String),
    received_at               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    hostname                 LowCardinality(String),
    software_type            LowCardinality(String),
    name                     String,
    version                  String,
    publisher                LowCardinality(String),
    deployment_status        LowCardinality(String),
    deployment_time          String,
    deployment_time_parsed   Nullable(DateTime64(3, 'UTC')),
    product_code             String,
    -- Absent (unknown bitness) is a real, common case — Nullable rather than
    -- defaulting to false, which would claim 32-bit for software the agent
    -- never actually determined the architecture of.
    is_64_bit                Nullable(UInt8),
    install_paths            Array(String),
    extra                    Map(String, String) CODEC(ZSTD(3)),
    payload_extra            Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, hostname, name, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Synthetics results: /api/v2/synthetics
--
-- One row per test result. assertions travel as five parallel arrays rather
-- than a JSON blob because "how many assertions failed, and which" is the
-- question this table exists to answer quickly; expected/actual stay JSON
-- text within the array since common.TestResult does not say what type they
-- are (the intake audit found no Go type for this track at all).
--
-- config.request has fields beyond host/port that the source comment for
-- this track leaves as "..." — genuinely unenumerated upstream, so the whole
-- result.config object is kept as JSON rather than guessing a partial shape.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS synthetics_results
(
    tenant_id                        LowCardinality(String),
    received_at                      DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    test_id                          String,
    test_name                        String,
    test_type                        LowCardinality(String),
    test_subtype                     LowCardinality(String),
    test_version                     String,

    location_id                      LowCardinality(String),
    location_name                    LowCardinality(String),
    location_display_name            String,

    result_id                        String,
    result_initial_id                String,
    status                           LowCardinality(String),
    run_type                         LowCardinality(String),
    -- Wire type is undocumented (no Go type exists for this track); kept as
    -- the value exactly as rendered rather than guessing numeric vs string.
    duration                         String,

    test_started_at                  String,
    test_started_at_parsed           Nullable(DateTime64(3, 'UTC')),
    test_finished_at                 String,
    test_finished_at_parsed          Nullable(DateTime64(3, 'UTC')),
    test_triggered_at                String,
    test_triggered_at_parsed         Nullable(DateTime64(3, 'UTC')),

    -- Parallel arrays, index-aligned: assertion_type[i]/operator[i]/
    -- expected[i]/actual[i]/valid[i] all describe assertion i. Order and
    -- count both matter — the handler used to tally only valid==true, which
    -- is exactly the fidelity loss this table exists to close.
    assertion_type                   Array(String),
    assertion_operator               Array(String),
    assertion_expected                Array(String) CODEC(ZSTD(3)),
    assertion_actual                 Array(String) CODEC(ZSTD(3)),
    assertion_valid                  Array(UInt8),

    -- failure's presence is itself a signal (a result can have no failure
    -- object at all), so both columns are Nullable rather than "".
    failure_code                     Nullable(String),
    failure_message                  Nullable(String),

    config                           String CODEC(ZSTD(3)),

    netstats_packets_sent            Nullable(Int64),
    netstats_packets_received        Nullable(Int64),
    netstats_packet_loss_percentage  Nullable(Float64),
    netstats_jitter                  Nullable(Float64),
    netstats_latency                 Nullable(Float64),
    netstats_hops                    Nullable(Int64),

    -- Same type netpath-intake carries; kept as JSON here rather than
    -- duplicating that table's column design for a field this track only
    -- forwards.
    netpath                          String CODEC(ZSTD(3)),
    dd                               String CODEC(ZSTD(3)),
    enrichment                       String CODEC(ZSTD(3)),
    v                                String,
    extra                            Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, test_id, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- OpenLineage events: /api/v1/lineage
--
-- One row per RunEvent (a request is usually one event; an array is
-- tolerated and unrolled here the same way). run_facets/job_facets/dataset
-- facets are OpenLineage's extension mechanism — genuinely open-ended by
-- spec, not merely undocumented — so they stay JSON text, never columns.
--
-- run_id is the natural correlation key across a run's START/RUNNING/
-- COMPLETE/FAIL/ABORT lifecycle, hence first after tenant_id in ORDER BY.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS openlineage_events
(
    tenant_id        LowCardinality(String),
    received_at      DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    event_type       LowCardinality(String),
    event_time       String,
    event_time_parsed Nullable(DateTime64(3, 'UTC')),
    producer         String,
    schema_url       String,

    run_id           String,
    run_facets       String CODEC(ZSTD(3)),
    job_namespace    LowCardinality(String),
    job_name         String,
    job_facets       String CODEC(ZSTD(3)),

    -- Parallel arrays per dataset list, index-aligned within each list;
    -- inputs and outputs are two independent lists, not one shared index.
    input_namespace  Array(String),
    input_name       Array(String),
    input_facets     Array(String) CODEC(ZSTD(3)),
    output_namespace Array(String),
    output_name      Array(String),
    output_facets    Array(String) CODEC(ZSTD(3)),

    -- Context, not payload: the ?api-version query param and the Via
    -- header the trace-agent's reverse proxy adds.
    api_version      String,
    via              String,
    extra            Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, run_id, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Query action results: /api/v2/query-actions
--
-- The one track in the whole EVP group with zero field names asserted
-- anywhere in the Go code — comp/dataobs/queryactions only schedules the
-- checks, and the results are produced by Python integrations this repo
-- never sees the source of. result is the entry verbatim as JSON; keys
-- records its top-level field names so "what shapes have we actually seen"
-- is a query instead of a SELECT * scan.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS query_action_results
(
    tenant_id              LowCardinality(String),
    received_at            DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    result                 String CODEC(ZSTD(3)),
    keys                   Array(String),
    dd_evp_origin          LowCardinality(String),
    dd_evp_origin_version  LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;
