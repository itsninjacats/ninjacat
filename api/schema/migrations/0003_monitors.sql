-- 0003: monitors — what each monitor group went through, what was evaluated, what was notified.

CREATE TABLE IF NOT EXISTS monitor_transitions
(
    tenant_id          LowCardinality(String),
    monitor_id         UInt64,
    monitor_version    UInt32,
    query_hash         String,                       -- tells a query change from a threshold change
    monitor_kind       LowCardinality(String),       -- metric; later log, composite, …
    group_key          String,                       -- '*' for a simple alert
    group_tags         Array(String),                -- k:v, for notify_by and {{key.name}}
    at                 DateTime64(3, 'UTC'),         -- the aligned tick, the evaluation's identity
    from_state         LowCardinality(String),       -- none, OK, Warn, Alert, No Data
    to_state           LowCardinality(String),
    reason             LowCardinality(String),       -- threshold, recovery, no_data, timeout, expired,
                                                     -- definition_changed, deleted, new, resolved
    actor              Nullable(String),             -- the user: updated_by / deleted_by, later resolved
    value              Nullable(Float64),
    crossed            LowCardinality(String),       -- critical, warning, critical_recovery, warning_recovery, none
    comparator         LowCardinality(String),
    critical           Nullable(Float64),
    warning            Nullable(Float64),
    critical_recovery  Nullable(Float64),
    warning_recovery   Nullable(Float64),
    window_from        DateTime64(3, 'UTC'),
    window_to          DateTime64(3, 'UTC'),
    first_triggered_at Nullable(DateTime64(3, 'UTC')),
    last_triggered_at  Nullable(DateTime64(3, 'UTC')),
    inserted_at        DateTime64(3, 'UTC') DEFAULT now64(3),
    INDEX inserted_at_idx inserted_at TYPE minmax GRANULARITY 1  -- the notifier's cursor
)
ENGINE = ReplacingMergeTree
PARTITION BY toYYYYMM(at)
ORDER BY (tenant_id, monitor_id, group_key, at)
TTL toDateTime(at) + INTERVAL 400 DAY;

CREATE TABLE IF NOT EXISTS monitor_evaluations   -- one row per group per evaluation
(
    tenant_id LowCardinality(String), monitor_id UInt64, group_key String,
    at DateTime64(3, 'UTC'), value Nullable(Float64), state LowCardinality(String),
    outcome LowCardinality(String),               -- evaluated, skipped, error
    detail String                                 -- why skipped or failed
)
ENGINE = MergeTree
PARTITION BY toDate(at)
ORDER BY (tenant_id, monitor_id, group_key, at)
TTL toDateTime(at) + INTERVAL 7 DAY;

CREATE TABLE IF NOT EXISTS monitor_notifications -- what the notifier did, one row per attempt
(
    tenant_id LowCardinality(String), monitor_id UInt64, group_key String,
    transition_at DateTime64(3, 'UTC'),
    transition_inserted_at DateTime64(3, 'UTC'),  -- the notifier's cursor
    kind LowCardinality(String),                  -- transition, renotify, downtime_end, escalation
    seq UInt32,                                   -- the nth renotify or escalation
    channel LowCardinality(String),
    status LowCardinality(String),                -- stub, sent, muted, failed
    attempt UInt32,
    handled_at DateTime64(3, 'UTC')
)
ENGINE = ReplacingMergeTree(handled_at)
PARTITION BY toYYYYMM(transition_at)
ORDER BY (tenant_id, monitor_id, group_key, transition_at, kind, seq, channel)
TTL toDateTime(transition_at) + INTERVAL 400 DAY;
