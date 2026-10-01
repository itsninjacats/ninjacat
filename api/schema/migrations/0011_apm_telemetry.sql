-- apm_telemetry: the tracer/agent/installer telemetry envelope from
-- instrumentation-telemetry-intake.<site> (POST /api/v2/apmtelemetry).
--
-- No CREATE DATABASE and no database qualifier, same as every prior
-- migration: the runner applies every statement over a connection already
-- opened against the target database, and qualifying would break running
-- against a scratch one.
--
-- Five producers share one wire envelope (tracer libraries, the fleet
-- installer, agent-telemetry, the trace-agent's onboarding events and the
-- cluster-agent's remote-config events — see intake/router_misc.go's header
-- comment for the full breakdown of each). None of their Go types is
-- importable from this module (nested in agent packages, or unexported), so
-- application/host are the few fields every producer is documented to send,
-- pulled out as real columns, and payload — the producer-specific part —
-- travels whole as raw JSON text. That is not a downgrade from "real"
-- columns: nobody here can name the payload's fields with any more authority
-- than the JSON already does.
--
-- ONE ROW PER TOP-LEVEL REQUEST, PLUS ONE PER BATCH ENTRY. request_type
-- "message-batch" carries payload = [{request_type, payload}, ...]; the
-- parent row keeps that whole array as its own payload (so "what did this
-- HTTP request contain" is always answerable from one row), and
-- batch_index/parent_request_type mark the child rows this table adds on
-- top of it, one per entry, with that entry's own request_type and payload.
-- A non-batch request has no children: batch_index stays NULL.
CREATE TABLE IF NOT EXISTS apm_telemetry
(
    tenant_id    LowCardinality(String),

    -- Arrival time. Nothing on the wire is trustworthy as the row's own
    -- timestamp: tracer_time/event_time are producer-reported (parsed
    -- separately below, and only some producers send either), so this is the
    -- one column every row can rely on for "everything in a time window",
    -- the query shape this table exists for.
    received_at  DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- request_type is the wire discriminator (app-started, agent-metrics,
    -- message-batch, apm-onboarding-event, ...); producer is derived from it
    -- (intake's telProducer) because request_type alone is ambiguous for
    -- "logs"/"traces"/"message-batch", shared by more than one producer.
    request_type LowCardinality(String),
    producer     LowCardinality(String),  -- tracer / fleet-installer / agent-telemetry / trace-agent / cluster-agent / unknown
    api_version  LowCardinality(String),

    -- runtime_id is a per-process UUID, not a bounded vocabulary — plain
    -- String, unlike the enum-shaped columns above.
    runtime_id   String,

    -- seq_id travels through json.Number end to end, never through float64:
    -- a plain interface{} decode of a large sequence number would round it
    -- past 2^53, which is exactly the failure this column exists to avoid.
    seq_id            Nullable(Int64),

    -- tracer_time/event_time are Unix seconds with an optional fractional
    -- part, sent by only some producers (event_time is in fact the signal
    -- that tells agent-telemetry apart from the others when request_type
    -- itself is ambiguous). Nullable because "not sent" and "sent as zero"
    -- are different producers' behaviour, not the same fact — and the *_raw
    -- twin keeps the literal JSON text next to the parsed value, because a
    -- parse failure must not erase what was actually received.
    tracer_time       Nullable(DateTime64(3, 'UTC')),
    tracer_time_raw   String,
    event_time        Nullable(DateTime64(3, 'UTC')),
    event_time_raw    String,

    -- application block: the six fields every producer that sends one is
    -- documented to include.
    service_name      LowCardinality(String),
    service_version   LowCardinality(String),
    env               LowCardinality(String),
    language_name     LowCardinality(String),
    language_version  LowCardinality(String),
    tracer_version    LowCardinality(String),

    -- host block: hostname/os/architecture are the three fields read by
    -- name; host_extra keeps every other key gohai or the tracer put there
    -- (dd_process_agent metadata, container ids seen from inside, etc) by
    -- name and value, JSON-text-encoded because their own shapes are not
    -- declared anywhere. Plain Map, like the gohai host metadata maps
    -- elsewhere in this schema (0001's header note) — this is metadata, not
    -- a Datadog tag set, so it does not take the tags multiset shape.
    hostname          LowCardinality(String),
    host_os           LowCardinality(String),
    host_architecture LowCardinality(String),
    host_extra        Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- The producer-specific part, exactly as it arrived. For a batch parent
    -- row this is the whole batch array; for every other row (including a
    -- batch child) it is that row's own payload object/array.
    payload String CODEC(ZSTD(3)),

    -- debug and origin are present per spec on some producers (debug on the
    -- tracer/installer envelope, origin as the fleet-installer's own
    -- presence signal) but neither has a fixed type worth a dedicated
    -- column, so debug keeps its raw JSON text and origin its string value.
    debug   String,
    origin  LowCardinality(String),

    -- Added by the trace-agent's HTTP proxy (pkg/trace/api/telemetry.go),
    -- never by the sender itself — identifies the hop and the host it ran
    -- on, which the JSON envelope has no field for at all.
    via                       LowCardinality(String),
    dd_agent_hostname         LowCardinality(String),
    dd_agent_env              LowCardinality(String),
    datadog_container_id      String,
    x_datadog_container_tags  String,

    -- Every envelope key besides the ones above, JSON-text-encoded by name —
    -- the same "keep the names even without the values" rule k8s_actions'
    -- extra_keys follows (0001), except here the values are cheap to keep
    -- too since there is no fixed row shape to blow up.
    extra Map(LowCardinality(String), String) CODEC(ZSTD(3)),

    -- Set only on the rows a message-batch entry produced (see the table
    -- comment above). NULL, not 0, marks "this is the request's own row" —
    -- entry 0 of a real batch must stay distinguishable from "not a batch
    -- entry at all".
    batch_index         Nullable(UInt32),
    parent_request_type LowCardinality(String)
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, received_at)
-- 14 days, the logs class: this is debugging/diagnostic telemetry about the
-- tracers and agents themselves, not a record anything else depends on, and
-- every instrumented process can heartbeat on its own schedule — volume adds
-- up fast enough that a long TTL buys little and costs real disk.
TTL toDateTime(received_at) + INTERVAL 14 DAY;
