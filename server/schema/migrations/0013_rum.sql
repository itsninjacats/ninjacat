-- RUM: what the browser, iOS and Android SDKs send on browser-intake.<site>.
--
-- Six tables, one per payload shape rather than one per event kind, for the
-- reason 0001 gives: a table per kind multiplies directories, merges and
-- per-table overhead, while LowCardinality(String) makes the kind a dictionary
-- id. The split that IS made here is the one the data forces — a view is
-- upserted, everything else is appended — plus the three tracks that arrive on
-- different endpoints entirely (replay, spans) or carry no user-facing event
-- at all (telemetry).
--
-- EVERY table carries an `event` (or `span`/`segment`) column holding the JSON
-- exactly as it came off the wire. That is deliberate and it is not
-- redundancy: the RUM schema has well over a hundred fields per variant, all
-- of them optional, several of them arrays of objects, and new ones land with
-- every SDK release. The typed columns exist so a query can FILTER without
-- parsing JSON; the raw column exists so nothing is lost — including keys the
-- generated types do not declare yet, which rumevents keeps in
-- AdditionalProperties and which would otherwise be visible only as a count.
--
-- THE DATE IS THE SENDER'S DATE, NEVER ARRIVAL. iOS and Android buffer events
-- to disk and upload them up to 18 hours later (offline device, tracking
-- consent pending, a crash reported at the next launch), so an old `date` is
-- correct data, not a defect. Arrival time is kept separately in received_at
-- so the lag is measurable. Partitioning and TTL run off `date` because that
-- is what a query filters on.
--
-- No CREATE DATABASE and no database qualifier, same as 0001 and 0002: the
-- runner applies every statement over a connection already opened against the
-- target database, and qualifying would break running against a scratch one.

-- ---------------------------------------------------------------------------
-- rum_views — one row per view, upserted
-- ---------------------------------------------------------------------------
--
-- A view lives as long as the user stays on the page. The SDK re-sends the
-- SAME view.id over and over with a growing _dd.document_version and updated
-- counters, and all three SDKs keep only the highest version per id inside one
-- batch (browser's in-batch replace, iOS's RUMViewEventsFilter, Android's
-- RumViewEventFilter). So one view.id is a series of updates to ONE logical
-- row, not a series of events — ReplacingMergeTree keyed on the version the
-- SDK itself increments, exactly like hosts is keyed on seen_at.
--
-- `view_update` events merge into the same row as `view` events: they carry
-- the same application/session/view identity and the same
-- _dd.document_version counter, and they are the newer wire form of the same
-- update (RumViewUpdateEventView declares the same field set as
-- RumViewEventView). Keeping them apart would split one view's history across
-- two rows and make the counters disagree with themselves. event_type records
-- which form produced the surviving row.
--
-- Readers must use FINAL (or argMax) for exactness, same caveat as hosts.
CREATE TABLE IF NOT EXISTS rum_views
(
    tenant_id          LowCardinality(String),

    -- Arrival, for measuring how stale a batch was. Never used as `date`.
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Request-level identity, the same block on every RUM table. ddsource is
    -- a query parameter on every SDK; the DD-EVP-ORIGIN pair says which SDK
    -- and which version sent the batch (browser puts them in the query
    -- string, iOS and Android in headers). These are the only fields that
    -- identify the SENDER: a browser event has no hostname and no agent.
    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),

    -- DD-REQUEST-ID is a fresh uuid per attempt; DD-IDEMPOTENCY-KEY is
    -- sha1(body) and stays the same across retries, so the pair is what tells
    -- a retry from a new batch.
    request_id         String,
    idempotency_key    String,

    -- Browser only: ?_dd.api=fetch|beacon. Which transport the SDK used
    -- decides what we can conclude from a missing batch — sendBeacon is
    -- fire-and-forget at page unload and its failures are invisible to the
    -- page, fetch's are not.
    dd_api             LowCardinality(String),

    -- Browser only: ?batch_time= and the _dd.retry_* pair, absent on a first
    -- attempt. Nullable rather than 0, because "sent once" and "retried zero
    -- times" are the same number and different facts. retry_after is stored
    -- as sent: the browser puts a delay in milliseconds there, the mobile
    -- SDKs put the HTTP status that caused the retry.
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),

    -- The browser's only network identity. Kept because it is the one input a
    -- geo lookup would need later (the SDKs send no geo at all) and because
    -- it is how abuse on a public, write-only credential is traced.
    remote_addr        String,
    user_agent         String,

    -- Every query parameter that did not get a column of its own, as JSON.
    -- The SDKs add parameters between releases and a parameter nobody
    -- decoded is still evidence; this is the same bargain the `event`
    -- column strikes, one level up. dd-api-key and api_key are removed
    -- before it is built — a credential must not become durable in a
    -- table, and with query-string auth the credential IS a query
    -- parameter.
    query_extra        String CODEC(ZSTD(3)),

    -- The SDK's own timestamp, ms since epoch on the wire.
    date               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The identity hierarchy: application -> session -> view. application.id
    -- is bounded by how many RUM applications exist, so LowCardinality pays;
    -- session and view ids are uuids per visit and per page, so they do not.
    application_id     LowCardinality(String),
    session_id         String,
    view_id            String,

    -- The ReplacingMergeTree version. _dd.document_version, incremented by the
    -- SDK for every update of this view.
    document_version   UInt64 CODEC(T64, ZSTD(1)),

    -- "view" or "view_update" — which wire form won the merge.
    event_type         LowCardinality(String),

    service            LowCardinality(String),
    version            LowCardinality(String),
    build_version      LowCardinality(String),
    build_id           LowCardinality(String),

    -- android | ios | browser | flutter | react-native | roku | unity |
    -- kotlin-multiplatform | electron | cpp | maui. NOT the same thing as
    -- evp_origin: a WebView event embedded in a mobile batch keeps
    -- source:"browser" while the request says ios/android.
    source             LowCardinality(String),

    session_type               LowCardinality(String),
    session_has_replay         Nullable(Bool),
    session_is_active          Nullable(Bool),
    session_sampled_for_replay Nullable(Bool),

    -- usr identifies the PERSON, session only the visit. Unbounded and
    -- personal, so plain String with heavy compression rather than
    -- LowCardinality: a dictionary of every user id is a dictionary that
    -- never stops growing.
    usr_id             String CODEC(ZSTD(3)),
    usr_name           String CODEC(ZSTD(3)),
    usr_email          String CODEC(ZSTD(3)),
    usr_anonymous_id   String CODEC(ZSTD(3)),
    account_id         String,
    account_name       String,

    view_url           String CODEC(ZSTD(3)),
    view_name          String,
    view_referrer      String CODEC(ZSTD(3)),

    -- initial_load | route_change | session_renewal | bf_cache and the mobile
    -- activity_/fragment_/view_controller_ pairs. Worth its own column
    -- because averaging loading_time across initial_load and route_change
    -- produces a number that means nothing.
    view_loading_type  LowCardinality(String),
    view_loading_time  Nullable(Int64),
    view_time_spent    Int64 CODEC(T64, ZSTD(1)),
    view_is_active     Nullable(Bool),
    view_is_slow_rendered Nullable(Bool),

    -- Per-view rollups the SDK computes. Nullable because the sub-object is
    -- absent on platforms that do not produce that kind at all, and "no crash
    -- object" is not "zero crashes measured".
    action_count       Nullable(Int64),
    error_count        Nullable(Int64),
    crash_count        Nullable(Int64),
    long_task_count    Nullable(Int64),
    frozen_frame_count Nullable(Int64),
    resource_count     Nullable(Int64),
    frustration_count  Nullable(Int64),

    -- Web Vitals. Read from view.performance.* when the SDK sends the modern
    -- block, otherwise from the deprecated flat view.* fields — both forms
    -- are still on the wire, and the raw event keeps whichever arrived.
    -- Timings are nanoseconds as the schema defines them; cls is a score.
    lcp                Nullable(Int64),
    cls                Nullable(Float64),
    inp                Nullable(Int64),
    fcp                Nullable(Int64),
    fid                Nullable(Int64),
    fbc                Nullable(Int64),
    ttfb               Nullable(Int64),

    device_type        LowCardinality(String),
    device_brand       LowCardinality(String),
    device_model       String,
    device_name        String,
    os_name            LowCardinality(String),
    os_version         LowCardinality(String),

    -- connected | not_connected | maybe
    connectivity_status LowCardinality(String),

    -- User-provided context and feature-flag evaluations, both free-form
    -- maps in the schema. Kept as JSON text rather than Map(String, String)
    -- because their values are arbitrary JSON (objects, arrays, numbers) and
    -- a Map would force everything through a string cast.
    context            String CODEC(ZSTD(3)),
    feature_flags      String CODEC(ZSTD(3)),

    -- ddtags travels INSIDE the event for every SDK (unlike the agent, which
    -- puts it in the query string). Multiset shape, as everywhere else — see
    -- docs/decisions/0001-tags-are-a-multiset.md.
    ddtags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- The whole event, verbatim. See the header: the typed columns above are
    -- for filtering, this is what makes the row lossless.
    event              String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = ReplacingMergeTree(document_version)
-- MONTHLY, not daily, and that is a correctness requirement rather than a
-- sizing choice: ReplacingMergeTree only ever collapses rows WITHIN one
-- partition, so a partition boundary that fell between two versions of the
-- same view would leave both alive and make FINAL return the older one half
-- the time. `date` is the view's START and stays constant across its updates,
-- so daily partitions would in fact hold, but the whole guarantee would then
-- rest on an SDK detail nobody here controls. A month costs one coarser
-- pruning step and removes the question.
PARTITION BY toYYYYMM(date)
-- application first, then the visit, then the page: every RUM query starts
-- from an application and narrows to a session.
ORDER BY (tenant_id, application_id, session_id, view_id)
-- 30 days, the metrics class. A view IS a current-state row, but unlike a
-- host it is not a live object — the page closed and the row will never be
-- replaced again, so it ages out like any other telemetry.
TTL toDateTime(date) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- rum_events — action, error, resource, long_task, vital, transition
-- ---------------------------------------------------------------------------
--
-- Append-only: unlike a view, each of these is a fact that happened once. One
-- table for six kinds because they share the entire common block (identity,
-- user, device, os, connectivity, tags) and differ only in one sub-object,
-- which becomes a handful of per-kind columns here and stays whole in `event`.
CREATE TABLE IF NOT EXISTS rum_events
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),
    request_id         String,
    idempotency_key    String,

    -- Browser only: ?_dd.api=fetch|beacon. Which transport the SDK used
    -- decides what we can conclude from a missing batch — sendBeacon is
    -- fire-and-forget at page unload and its failures are invisible to the
    -- page, fetch's are not.
    dd_api             LowCardinality(String),
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),
    remote_addr        String,
    user_agent         String,

    -- Every query parameter that did not get a column of its own, as JSON.
    -- The SDKs add parameters between releases and a parameter nobody
    -- decoded is still evidence; this is the same bargain the `event`
    -- column strikes, one level up. dd-api-key and api_key are removed
    -- before it is built — a credential must not become durable in a
    -- table, and with query-string auth the credential IS a query
    -- parameter.
    query_extra        String CODEC(ZSTD(3)),

    date               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    application_id     LowCardinality(String),
    session_id         String,
    view_id            String,

    -- action | error | resource | long_task | vital | transition. Part of the
    -- sort key: "every error in this application" is the query this table
    -- answers most, and a kind is a dictionary id rather than a table.
    event_type         LowCardinality(String),

    service            LowCardinality(String),
    version            LowCardinality(String),
    build_version      LowCardinality(String),
    build_id           LowCardinality(String),
    source             LowCardinality(String),

    session_type       LowCardinality(String),
    session_has_replay Nullable(Bool),

    usr_id             String CODEC(ZSTD(3)),
    usr_name           String CODEC(ZSTD(3)),
    usr_email          String CODEC(ZSTD(3)),
    usr_anonymous_id   String CODEC(ZSTD(3)),
    account_id         String,
    account_name       String,

    view_url           String CODEC(ZSTD(3)),
    view_name          String,
    view_referrer      String CODEC(ZSTD(3)),

    device_type        LowCardinality(String),
    device_brand       LowCardinality(String),
    device_model       String,
    device_name        String,
    os_name            LowCardinality(String),
    os_version         LowCardinality(String),
    connectivity_status LowCardinality(String),

    -- action.*: the name lives in action.target.name, not on action itself.
    -- frustration.type is an ARRAY (one click can be both a rage click and a
    -- dead click), so it stays an array here.
    action_type        LowCardinality(String),
    action_name        String,
    action_id          String,
    action_frustration_types Array(LowCardinality(String)),

    -- error.*: is_crash marks the crash and ANR reports the mobile SDKs
    -- generate at the NEXT app launch, which is why their date can predate
    -- the session they belong to.
    error_id           String,
    error_message      String CODEC(ZSTD(3)),
    error_type         LowCardinality(String),
    error_source       LowCardinality(String),
    error_stack        String CODEC(ZSTD(3)),
    error_is_crash     Nullable(Bool),
    error_handling     LowCardinality(String),
    error_fingerprint  String,

    -- resource.*
    resource_id        String,
    resource_type      LowCardinality(String),
    resource_url       String CODEC(ZSTD(3)),
    resource_method    LowCardinality(String),
    resource_status_code Nullable(Int64),
    resource_duration  Nullable(Int64),
    resource_size      Nullable(Int64),

    -- long_task.*
    long_task_id       String,
    long_task_duration Nullable(Int64),

    -- vital.*: type is the sub-discriminator (duration | operation_step |
    -- app_launch), so three schema variants share these columns.
    vital_id           String,
    vital_type         LowCardinality(String),
    vital_name         String,
    vital_duration     Nullable(Int64),

    context            String CODEC(ZSTD(3)),
    ddtags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    event              String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(date)
ORDER BY (tenant_id, application_id, event_type, session_id, date)
TTL toDateTime(date) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- rum_telemetry — the SDK talking about itself
-- ---------------------------------------------------------------------------
--
-- type:"telemetry" arrives on the same /api/v2/rum track as user-facing
-- events, from every SDK, including SDKs configured for Logs only. It is not
-- product data — it is how a support question ("why is this app sending
-- nothing?") gets answered: configuration events carry the SDK's whole
-- resolved config, usage events name which API the app actually called, error
-- events are the SDK's own failures.
CREATE TABLE IF NOT EXISTS rum_telemetry
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),
    request_id         String,
    idempotency_key    String,

    -- Browser only: ?_dd.api=fetch|beacon. Which transport the SDK used
    -- decides what we can conclude from a missing batch — sendBeacon is
    -- fire-and-forget at page unload and its failures are invisible to the
    -- page, fetch's are not.
    dd_api             LowCardinality(String),
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),
    remote_addr        String,
    user_agent         String,

    -- Every query parameter that did not get a column of its own, as JSON.
    -- The SDKs add parameters between releases and a parameter nobody
    -- decoded is still evidence; this is the same bargain the `event`
    -- column strikes, one level up. dd-api-key and api_key are removed
    -- before it is built — a credential must not become durable in a
    -- table, and with query-string auth the credential IS a query
    -- parameter.
    query_extra        String CODEC(ZSTD(3)),

    date               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    application_id     LowCardinality(String),
    session_id         String,
    view_id            String,
    action_id          String,

    -- Always "telemetry" today. Kept as a column anyway so a future kind on
    -- this table is a value, not a migration.
    type               LowCardinality(String),

    -- The two discriminators: telemetry.status (error | debug) and
    -- telemetry.type (log | configuration | usage).
    status             LowCardinality(String),
    telemetry_type     LowCardinality(String),

    message            String CODEC(ZSTD(3)),
    error_stack        String CODEC(ZSTD(3)),
    error_kind         LowCardinality(String),

    -- The whole telemetry.configuration object as JSON: ~150 optional keys
    -- that change every SDK release, none of them worth a column.
    configuration      String CODEC(ZSTD(3)),

    -- telemetry.usage.feature — a ~40-value enum of SDK API names.
    usage_feature      LowCardinality(String),

    service            LowCardinality(String),
    version            LowCardinality(String),
    source             LowCardinality(String),

    effective_sample_rate Nullable(Float64),
    experimental_features Array(LowCardinality(String)),

    device_brand       LowCardinality(String),
    device_model       String,
    device_architecture LowCardinality(String),
    os_name            LowCardinality(String),
    os_version         LowCardinality(String),
    os_build           String,

    event              String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(date)
ORDER BY (tenant_id, application_id, status, date)
-- 90 days, the audit class: this is the record that says what an SDK was
-- configured to do three months ago, and that question outlives the data.
TTL toDateTime(date) + INTERVAL 90 DAY;

-- ---------------------------------------------------------------------------
-- rum_timeseries — mobile continuous samples (cpu, memory)
-- ---------------------------------------------------------------------------
--
-- type:"timeseries" is mobile-only and structurally unlike a vital: one event
-- carries a whole run of samples, timestamps in one array and each measured
-- quantity in its own parallel array. Flattening it into one row per sample
-- here would multiply the row count by the batch size for no query we have;
-- the arrays are kept as they arrived, which also keeps the pairing between
-- timestamps[i] and values.<name>[i] intact.
CREATE TABLE IF NOT EXISTS rum_timeseries
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),
    request_id         String,
    idempotency_key    String,

    -- Browser only: ?_dd.api=fetch|beacon. Which transport the SDK used
    -- decides what we can conclude from a missing batch — sendBeacon is
    -- fire-and-forget at page unload and its failures are invisible to the
    -- page, fetch's are not.
    dd_api             LowCardinality(String),
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),
    remote_addr        String,
    user_agent         String,

    -- Every query parameter that did not get a column of its own, as JSON.
    -- The SDKs add parameters between releases and a parameter nobody
    -- decoded is still evidence; this is the same bargain the `event`
    -- column strikes, one level up. dd-api-key and api_key are removed
    -- before it is built — a credential must not become durable in a
    -- table, and with query-string auth the credential IS a query
    -- parameter.
    query_extra        String CODEC(ZSTD(3)),

    date               DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    application_id     LowCardinality(String),
    session_id         String,
    view_id            String,

    -- cpu | memory — the sub-discriminator that picks the variant.
    name               LowCardinality(String),
    timeseries_id      String,
    timeseries_schema  LowCardinality(String),

    -- NANOSECOND precision, because the wire carries nanoseconds here
    -- (timeseries.start/end, unlike `date`, which is milliseconds) and
    -- rounding them to milliseconds would stop them lining up with the
    -- timestamps array below.
    start              DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    end                DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Verbatim, in wire order and wire units. The schema does not say whether
    -- these are absolute or relative, and guessing would be a lossy guess.
    timestamps         Array(Int64) CODEC(T64, ZSTD(1)),

    -- data.values is a different object per variant (memory_footprint and
    -- memory_percent for memory, cpu_ticks_* for cpu) and gains members with
    -- each SDK release, so it is kept as JSON rather than as columns that
    -- would be NULL for the other variant.
    values             String CODEC(ZSTD(3)),

    service            LowCardinality(String),
    version            LowCardinality(String),
    source             LowCardinality(String),
    context            String CODEC(ZSTD(3)),
    ddtags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    event              String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(ddtags)   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(date)
ORDER BY (tenant_id, application_id, name, session_id, date)
TTL toDateTime(date) + INTERVAL 14 DAY;

-- ---------------------------------------------------------------------------
-- rum_replay_segments — Session Replay, stored without processing
-- ---------------------------------------------------------------------------
--
-- Datadog's own intake stores segments "without any processing", and the
-- browser SDK's deflate encoder deliberately emits streams that can be
-- CONCATENATED later. So the segment lands here as the zlib bytes that
-- arrived, byte for byte: inflating and re-deflating would throw that property
-- away for nothing, and the record schema (94 structs, 8 unions) is not
-- something an intake should be parsing under load. rumevents/replay exists
-- for the day a viewer needs typed records; nothing at ingest needs them.
--
-- One row per blob part, not per request: the mobile SDKs send N segments in
-- one multipart body (one per view in the on-disk batch) with a parallel array
-- of metadata objects, and collapsing them would lose the per-segment
-- metadata. part_name/part_filename record where in the envelope the blob sat,
-- which is also how the browser form (one part named "segment") and the mobile
-- form (parts named file0, file1, ...) stay distinguishable after the fact.
CREATE TABLE IF NOT EXISTS rum_replay_segments
(
    tenant_id          LowCardinality(String),

    -- Replay metadata has no `date`: start/end describe the recorded window
    -- and are absent entirely on the resource variant. So this table sorts,
    -- partitions and expires on ARRIVAL, which is the one timestamp every row
    -- has. That is a deliberate exception to the header's rule, not an
    -- oversight; the sender's own window is in start/end when it sent one.
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),
    request_id         String,
    idempotency_key    String,

    -- Browser only: ?_dd.api=fetch|beacon. Which transport the SDK used
    -- decides what we can conclude from a missing batch — sendBeacon is
    -- fire-and-forget at page unload and its failures are invisible to the
    -- page, fetch's are not.
    dd_api             LowCardinality(String),
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),
    remote_addr        String,
    user_agent         String,

    -- Every query parameter that did not get a column of its own, as JSON.
    -- The SDKs add parameters between releases and a parameter nobody
    -- decoded is still evidence; this is the same bargain the `event`
    -- column strikes, one level up. dd-api-key and api_key are removed
    -- before it is built — a credential must not become durable in a
    -- table, and with query-string auth the credential IS a query
    -- parameter.
    query_extra        String CODEC(ZSTD(3)),

    application_id     LowCardinality(String),
    session_id         String,
    view_id            String,

    -- browser | android | ios | flutter | react-native | ... — the same
    -- discriminator rumevents/replay uses to pick BrowserSegment vs
    -- MobileSegment.
    source             LowCardinality(String),

    -- "segment" (a recording) or "resource" (a canvas image referenced by
    -- one), told apart by which part carried the blob.
    variant            LowCardinality(String),
    part_name          LowCardinality(String),
    part_filename      String,

    -- Nullable because the resource variant sends metadata with neither, and
    -- an epoch here would put a canvas image in 1970.
    start              Nullable(DateTime64(3, 'UTC')),
    end                Nullable(DateTime64(3, 'UTC')),

    records_count      Nullable(Int64),
    has_full_snapshot  Nullable(Bool),

    -- Browser sends an index; Android sends null (an upstream TODO) and iOS
    -- omits it. Three states, so Nullable.
    index_in_view      Nullable(Int64),

    -- Browser only: init | segment_duration_limit | ... Absent on mobile.
    creation_reason    LowCardinality(String),

    raw_segment_size        Nullable(Int64),
    compressed_segment_size Nullable(Int64),

    -- The blob. Already zlib-compressed by the SDK, so no codec on top: ZSTD
    -- over a deflate stream costs CPU and saves nothing.
    segment            String CODEC(NONE),

    -- The resource variant's raw image bytes, likewise untouched.
    image              String CODEC(NONE),

    -- This blob's metadata object, as JSON.
    event              String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, application_id, session_id, view_id, received_at)
-- 14 days, the bulky class. Replay is by far the heaviest thing RUM sends and
-- a replay nobody watched in two weeks is a replay nobody will watch.
TTL toDateTime(received_at) + INTERVAL 14 DAY;

-- ---------------------------------------------------------------------------
-- rum_spans — the mobile SDKs' own trace format
-- ---------------------------------------------------------------------------
--
-- /api/v2/spans is NOT the agent's /v0.4/traces: it is NDJSON of
-- {"spans":[...],"env":...} envelopes with a flat, hand-rolled span shape,
-- produced only by dd-sdk-ios and dd-sdk-android and covered by no published
-- JSON Schema. It shares a host, a credential and a sender with mobile RUM and
-- nothing at all with the msgpack tracer payloads, which is why it lives here
-- rather than in the trace tables.
--
-- IDs ARE STRINGS. The SDKs encode trace_id/span_id/parent_id as decimal or
-- hex text of 64- and 128-bit numbers; parsing them into a UInt64 would need a
-- guess about the base, and going anywhere near a float64 would silently drop
-- digits above 2^53. Opaque text preserves whatever arrived.
CREATE TABLE IF NOT EXISTS rum_spans
(
    tenant_id          LowCardinality(String),
    received_at        DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    ddsource           LowCardinality(String),
    evp_origin         LowCardinality(String),
    evp_origin_version LowCardinality(String),
    request_id         String,
    idempotency_key    String,

    -- Browser only: ?_dd.api=fetch|beacon. Which transport the SDK used
    -- decides what we can conclude from a missing batch — sendBeacon is
    -- fire-and-forget at page unload and its failures are invisible to the
    -- page, fetch's are not.
    dd_api             LowCardinality(String),
    batch_time         Nullable(DateTime64(3, 'UTC')),
    retry_count        Nullable(UInt32),
    retry_after        Nullable(Int64),
    remote_addr        String,
    user_agent         String,

    -- Every query parameter that did not get a column of its own, as JSON.
    -- The SDKs add parameters between releases and a parameter nobody
    -- decoded is still evidence; this is the same bargain the `event`
    -- column strikes, one level up. dd-api-key and api_key are removed
    -- before it is built — a credential must not become durable in a
    -- table, and with query-string auth the credential IS a query
    -- parameter.
    query_extra        String CODEC(ZSTD(3)),

    trace_id           String,
    span_id            String,
    parent_id          String,

    name               LowCardinality(String),
    service            LowCardinality(String),
    resource           String CODEC(ZSTD(3)),
    type               LowCardinality(String),

    -- From the ENVELOPE, not the span: {"spans":[...],"env":"prod"}.
    env                LowCardinality(String),

    -- Nanosecond precision, because that is what the wire carries and
    -- rounding a span start to milliseconds makes short spans overlap.
    start              DateTime64(9, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    duration_ns        Int64 CODEC(T64, ZSTD(1)),
    error              Int8,

    -- meta on the wire is an object whose values are sometimes nested objects
    -- (meta.device, meta.os). Those are flattened to dotted keys here rather
    -- than dropped, so meta['device.brand'] is reachable. Plain Map, NOT the
    -- tag multiset shape: this is a JSON object, and a JSON object cannot
    -- repeat a key.
    meta               Map(String, String) CODEC(ZSTD(3)),
    metrics            Map(String, Float64) CODEC(ZSTD(3)),

    -- The span object as it arrived.
    span               String CODEC(ZSTD(3)),

    -- Envelope keys other than "spans" and "env", as JSON. Usually empty; it
    -- exists so a key a future SDK adds to the envelope is kept rather than
    -- counted.
    envelope_extra     String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(start)
ORDER BY (tenant_id, service, name, start)
TTL toDateTime(start) + INTERVAL 14 DAY;
