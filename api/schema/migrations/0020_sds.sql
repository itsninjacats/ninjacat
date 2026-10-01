-- 0020: Sensitive Data Scanner results (sds-intake.<site>, /api/v2/sdsresult).
--
-- One payload is one scan of one resource: SdsResultPayload, in the agent's
-- repository as pkg/proto/datadog/sds/sds_result.proto. Until 0020 these went
-- to raw_payloads whole. Three tables, the payload's own three levels:
--
--   sds_scans     the payload: who scanned what, with which rules
--   sds_results   one location that was scanned, with or without matches
--   sds_matches   one rule that matched in one location
--
-- scan_id is minted on arrival and ties the three together. A result with
-- no match still gets its row: "scanned and clean" is what coverage is
-- measured by.

CREATE TABLE IF NOT EXISTS sds_scans
(
    tenant_id            LowCardinality(String),
    received_at          DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    scan_id              UUID,
    -- The scanner's clock, Unix milliseconds on the wire. NULL when it sent 0.
    timestamp            Nullable(DateTime64(3, 'UTC')),
    -- The agent's scanner sends 'postgres_table' and a name built from the
    -- cluster, database, schema and table.
    resource_type        LowCardinality(String),
    resource_name        String CODEC(ZSTD(1)),

    -- 'agent', 'agentless', 'datadog_crawler', or '' when not sent. The
    -- columns after it are that source's own fields.
    scanning_source      LowCardinality(String),
    source_version       LowCardinality(String),
    source_region        LowCardinality(String),
    source_hostname      String,
    source_service_name  String,
    scanner_version      LowCardinality(String),
    scanner_region       LowCardinality(String),

    rule_ids             Array(String),
    -- The deprecated map of rule id to name, priority, tags and labels, as
    -- JSON; '' when empty.
    rules                String CODEC(ZSTD(3)),

    -- ScanStats. NULL when the payload carried none: a file count of 0 is a
    -- scan that found no files.
    scan_duration_ms                   Nullable(Int64),
    total_files_found                  Nullable(Int64),
    files_scanned                      Nullable(Int64),
    files_skipped_unsupported_type     Nullable(Int64),
    files_partially_scanned_size_limit Nullable(Int64),
    total_data_scanned_bytes           Nullable(Int64),
    skipped_files_by_type              Map(String, Int64),

    result_count         UInt32
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, resource_type, resource_name, received_at)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

CREATE TABLE IF NOT EXISTS sds_results
(
    tenant_id            LowCardinality(String),
    received_at          DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    scan_id              UUID,
    result_index         UInt32,
    resource_type        LowCardinality(String),
    resource_name        String CODEC(ZSTD(1)),

    -- 'postgres_table', 'rds_table', 'snowflake_table', 's3_file', or 'path'
    -- and 'database' from the deprecated fields; '' when not sent.
    location_kind        LowCardinality(String),
    database_name        String,
    schema_name          String,
    table_name           String,
    -- A file's path.
    path                 String CODEC(ZSTD(1)),
    -- NULL where the location's kind has no such field.
    table_row_count      Nullable(Int64),
    scanned_row_count    Nullable(Int64),
    scanned_column_names Array(String),
    scanned_column_types Array(String),
    -- The whole location as protobuf's JSON: each kind has fields of its own
    -- (ARNs, snapshot times, sizes) that have no column.
    location             String CODEC(ZSTD(3)),

    duration             Int64,
    task_id              String,
    sub_task_id          String,
    task_started_at      Nullable(DateTime64(3, 'UTC')),
    task_ended_at        Nullable(DateTime64(3, 'UTC')),
    -- STATUS_UNKNOWN, SUCCESS or ERROR; '' when the result carried no task
    -- metadata.
    task_status          LowCardinality(String),
    failure_reason       String CODEC(ZSTD(1)),

    match_count          UInt32,
    table_match_count    UInt32
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, resource_type, resource_name, received_at, scan_id, result_index)
TTL toDateTime(received_at) + INTERVAL 30 DAY;

CREATE TABLE IF NOT EXISTS sds_matches
(
    tenant_id            LowCardinality(String),
    received_at          DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    scan_id              UUID,
    result_index         UInt32,
    resource_type        LowCardinality(String),
    resource_name        String CODEC(ZSTD(1)),
    location_kind        LowCardinality(String),
    database_name        String,
    schema_name          String,
    table_name           String,
    path                 String CODEC(ZSTD(1)),

    -- 'match': a rule matched in text (a file). 'table_match': a rule
    -- matched in a column, counted over rows. The columns of the other kind
    -- stay at their defaults.
    kind                 LowCardinality(String),
    rule_id              LowCardinality(String),

    -- What the scanner chose to show of the match. Stored as sent: whether
    -- it is redacted is the scanner's decision, not this table's.
    sample               String CODEC(ZSTD(3)),
    start_index          Int64,
    end_index            Int64,
    match_path           Nullable(String),
    line                 Nullable(Int64),
    column               Nullable(Int64),
    start_line           Nullable(Int64),
    end_line             Nullable(Int64),
    row                  Nullable(Int64),
    start_index_in_line  Nullable(Int64),
    end_index_in_line    Nullable(Int64),
    match_status         Nullable(String),

    column_name          String,
    count_matched_rows   Int64,
    count_total_rows     Int64,
    count_matches        Int64
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, rule_id, received_at, scan_id, result_index)
TTL toDateTime(received_at) + INTERVAL 30 DAY;
