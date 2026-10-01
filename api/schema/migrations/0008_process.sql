-- The process intake (process.<site>) — the whole of it, not just the slice
-- that fitted in the original `processes` table.
--
-- One HTTP frame from the process-agent carries one of six message types, and
-- until now exactly one of them (CollectorProc, type 12) reached a table, with
-- most of every Process struct left on the floor: no service discovery, no
-- tracer runtime ids, no language, no per-core CPU, no io rates, no uids, no
-- listening ports. The rest of the frames — realtime stats, containers,
-- container stats, process discovery and the system-probe connection table —
-- were decoded and logged and then dropped.
--
-- The shape below follows the wire, not the product:
--
--   process_snapshots     one row per FRAME, whatever its type. The envelope
--                         (host, network, group chunking, Host, SystemInfo,
--                         agent headers, frame header) lives here once; the
--                         per-item tables carry a snapshot_id back to it.
--   processes             one row per Process            (CollectorProc)
--   process_stats         one row per ProcessStat        (CollectorRealTime)
--   containers            one row per Container          (CollectorProc,
--                                                         CollectorContainer,
--                                                         Process.Container)
--   container_stats       one row per ContainerStat      (CollectorRealTime,
--                                                         CollectorContainerRealTime)
--   process_discoveries   one row per ProcessDiscovery   (CollectorProcDiscovery)
--   connections           one row per Connection         (CollectorConnections)
--   connections_payloads  one row per CollectorConnections, for the 40-odd
--                         payload-level fields a connection row would repeat
--
-- WHY the envelope is denormalised onto the per-item tables anyway, despite
-- snapshot_id: every question asked of these tables starts with a host and a
-- time window, and a JOIN to fetch "which VPC was this" or "which agent
-- version" would run on every such query. The columns are LowCardinality and
-- repeat within a batch, so the dictionary swallows them.
--
-- EVERY PROTO ENUM IS STORED BY NAME. ProcessState 'R', ContainerState
-- 'running', Language 'LANGUAGE_GO', ProtocolType 'protocolHTTP' — the
-- generated stringer's name, never its number. The numbers are an internal
-- detail of a .proto we do not own; a renumbering upstream would silently
-- rewrite history stored as ints.
--
-- ABSENT IS NOT ZERO. Where the wire genuinely distinguishes "not reported"
-- from zero and the column can carry it, the column is Nullable and the Go
-- field is a pointer: container created/started, Host.tagsModified,
-- ip_translation. The one place it could not be done is documented on
-- processes.has_create_time below.
--
-- No CREATE DATABASE and no database qualifier, same as 0001 and 0002.

-- ---------------------------------------------------------------------------
-- processes: the columns storeProcesses used to drop
-- ---------------------------------------------------------------------------
--
-- ALTER rather than a new table: the 18 columns that shipped keep their names,
-- their types and their position, so every query written against them still
-- runs. New columns are appended, which is also the order Go appends them in
-- (apps/storage/rows_process.go).
--
-- ADD COLUMN IF NOT EXISTS throughout, for the reason 0001 gives: ClickHouse
-- DDL is not transactional, so a file that fails halfway is re-run as-is.

ALTER TABLE processes
    -- The frame this process arrived in. Joins to process_snapshots and, just
    -- as usefully, groups the chunks of one snapshot: the agent splits a big
    -- host into several frames (group_id/group_size) and every chunk gets its
    -- own id, so "one snapshot" is really (host, group_id).
    ADD COLUMN IF NOT EXISTS snapshot_id UUID,

    -- Identity the agent computes and we used to throw away. ns_pid is the pid
    -- inside the process's own PID namespace — in Kubernetes that is the
    -- number the container sees, and the only one that matches what a user
    -- reads out of `kubectl exec ... ps`. key is the agent's dedup key.
    ADD COLUMN IF NOT EXISTS ns_pid Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS key UInt32 CODEC(T64, ZSTD(1)),

    ADD COLUMN IF NOT EXISTS cwd String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS root String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS on_disk UInt8,
    ADD COLUMN IF NOT EXISTS pgroup Int32 CODEC(T64, ZSTD(1)),

    -- args is the argv ARRAY. cmdline (a join on spaces) stays for the
    -- queries already written against it, but it is lossy and always was:
    -- an argument containing a space cannot be split back out of it, and
    -- `--filter=a b` is indistinguishable from two arguments. args is the
    -- fidelity copy; cmdline is the convenience copy.
    ADD COLUMN IF NOT EXISTS args Array(String) CODEC(ZSTD(3)),

    -- The six numeric ids. Real/effective/saved, for both user and group —
    -- a setuid binary is exactly the case where name alone lies.
    ADD COLUMN IF NOT EXISTS uid Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS gid Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS euid Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS egid Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS suid Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS sgid Int32 CODEC(T64, ZSTD(1)),

    -- The rest of MemoryStat. rss and vms were kept; swap is how you see a
    -- host thrashing, and text/lib/data/dirty are how you tell a leak in the
    -- heap from a mapped file.
    ADD COLUMN IF NOT EXISTS mem_swap UInt64 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS mem_shared UInt64 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS mem_text UInt64 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS mem_lib UInt64 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS mem_data UInt64 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS mem_dirty UInt64 CODEC(T64, ZSTD(1)),

    -- The rest of CPUStat. user/system split is the first question after
    -- "why is this process hot"; user_time/system_time are the cumulative
    -- counters the percentages are derived from, and survive a missed sample.
    ADD COLUMN IF NOT EXISTS cpu_last_cpu LowCardinality(String),
    ADD COLUMN IF NOT EXISTS cpu_user_pct Float32 CODEC(Gorilla, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS cpu_system_pct Float32 CODEC(Gorilla, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS cpu_nice Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS cpu_user_time Int64 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS cpu_system_time Int64 CODEC(T64, ZSTD(1)),

    -- Per-core breakdown, as PARALLEL ARRAYS: cpu_core_names[i] pairs with
    -- cpu_core_pcts[i]. Not a Map, because CPUStat.cpus is a repeated field
    -- and repeated means ordered — a map would lose the order and, on a host
    -- reporting the same core name twice, one of the entries.
    ADD COLUMN IF NOT EXISTS cpu_core_names Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS cpu_core_pcts Array(Float32) CODEC(ZSTD(1)),

    -- IOStat, previously not even nil-checked: the field was never read.
    ADD COLUMN IF NOT EXISTS io_read_rate Float32 CODEC(Gorilla, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS io_write_rate Float32 CODEC(Gorilla, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS io_read_bytes_rate Float32 CODEC(Gorilla, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS io_write_bytes_rate Float32 CODEC(Gorilla, ZSTD(1)),

    ADD COLUMN IF NOT EXISTS voluntary_ctx_switches UInt64 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS involuntary_ctx_switches UInt64 CODEC(T64, ZSTD(1)),

    -- ProcessNetworks: connections opened per second and bytes per second,
    -- attributed to the process rather than the container.
    ADD COLUMN IF NOT EXISTS net_connection_rate Float32 CODEC(Gorilla, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS net_bytes_rate Float32 CODEC(Gorilla, ZSTD(1)),

    -- The agent's own context strings for the process (ordered, hence Array).
    ADD COLUMN IF NOT EXISTS process_context Array(String) CODEC(ZSTD(3)),

    -- Runtime identity. Enum names: LANGUAGE_GO, LANGUAGE_JAVA, ...
    ADD COLUMN IF NOT EXISTS language LowCardinality(String),

    -- PortInfo: what the process listens on. Arrays, order as sent.
    ADD COLUMN IF NOT EXISTS port_tcp Array(Int32) CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS port_udp Array(Int32) CODEC(T64, ZSTD(1)),

    -- ServiceDiscovery. This is the bridge between a process and APM: the
    -- service name the agent guessed, the one DD_SERVICE set, and the tracer
    -- runtime ids that appear in trace payloads. Losing tracer_runtime_ids
    -- means losing the only join between "this pid on this host" and "these
    -- traces", which is exactly the correlation the product exists for.
    ADD COLUMN IF NOT EXISTS generated_service_name String,
    ADD COLUMN IF NOT EXISTS generated_service_name_source LowCardinality(String),
    ADD COLUMN IF NOT EXISTS dd_service_name String,
    ADD COLUMN IF NOT EXISTS dd_service_name_source LowCardinality(String),
    ADD COLUMN IF NOT EXISTS additional_generated_names Array(String),
    ADD COLUMN IF NOT EXISTS additional_generated_name_sources Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS tracer_runtime_ids Array(String),
    ADD COLUMN IF NOT EXISTS tracer_service_names Array(String),
    ADD COLUMN IF NOT EXISTS apm_instrumentation UInt8,

    -- ServiceDiscovery.resources is a repeated ONEOF with a single member
    -- defined today (LogResource{path}). A column per member would have to be
    -- migrated every time upstream adds one, and a count would lose the paths,
    -- so the list is kept as JSON: [{"logs":{"path":"..."}}]. An unknown
    -- future member still arrives here, named.
    ADD COLUMN IF NOT EXISTS service_resources String CODEC(ZSTD(3)),

    -- INJECTION_INJECTED / INJECTION_NOT_INJECTED / INJECTION_UNKNOWN —
    -- whether the tracer was auto-injected into this process.
    ADD COLUMN IF NOT EXISTS injection_state LowCardinality(String),

    ADD COLUMN IF NOT EXISTS zombie_children_count UInt32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS zombie_net_rate Float64 CODEC(Gorilla, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS has_zombie_aggregation UInt8,

    ADD COLUMN IF NOT EXISTS container_key UInt32 CODEC(T64, ZSTD(1)),

    -- byteKey and containerByteKey are opaque `bytes` the backend resolves
    -- keys with. Kept as String (ClickHouse String is a byte string, not
    -- UTF-8) so the bytes survive verbatim.
    ADD COLUMN IF NOT EXISTS byte_key String CODEC(ZSTD(1)),
    ADD COLUMN IF NOT EXISTS container_byte_key String CODEC(ZSTD(1)),

    -- create_time is a plain DateTime and cannot become Nullable without
    -- rewriting the column on every existing part, which on a table this size
    -- is a rewrite nobody asked for. So the distinction is carried beside it:
    -- has_create_time = 0 means the agent sent no start time and create_time
    -- is meaningless, not 1970. Every query that reads create_time must filter
    -- on has_create_time = 1. Should this table ever be rebuilt, fold the two
    -- into a Nullable(DateTime) and drop this column.
    ADD COLUMN IF NOT EXISTS has_create_time UInt8,

    -- Process.host is a *Host the BACKEND fills in post-resolution; a real
    -- agent leaves it nil. Kept as JSON rather than eight columns that are
    -- empty in every row we will ever receive — lossless, and it costs
    -- nothing when absent. Same treatment on containers and
    -- process_discoveries. The Host that IS populated, the snapshot's own,
    -- gets real columns on process_snapshots.
    ADD COLUMN IF NOT EXISTS process_host String CODEC(ZSTD(3)),

    -- The snapshot envelope, denormalised (see the header).
    ADD COLUMN IF NOT EXISTS network_id String,
    ADD COLUMN IF NOT EXISTS group_id Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS group_size Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS container_host_type LowCardinality(String),
    ADD COLUMN IF NOT EXISTS agent_version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS request_id String,
    ADD COLUMN IF NOT EXISTS host_uuid String,
    ADD COLUMN IF NOT EXISTS os_name LowCardinality(String),
    ADD COLUMN IF NOT EXISTS os_platform LowCardinality(String),
    ADD COLUMN IF NOT EXISTS os_family LowCardinality(String),
    ADD COLUMN IF NOT EXISTS os_version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS kernel_version LowCardinality(String);

-- The tags map shipped without the bloom pair every other tagged table has;
-- add it here so "which processes carry env:prod" stops being a full scan.
ALTER TABLE processes
    ADD INDEX IF NOT EXISTS idx_tag_keys mapKeys(tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4;

-- ---------------------------------------------------------------------------
-- process_snapshots: one row per frame, of any of the six types
-- ---------------------------------------------------------------------------
--
-- The frame envelope has no natural home on the item tables: SystemInfo's CPU
-- inventory alone is nine parallel arrays, and repeating it on every one of a
-- few hundred process rows would be absurd. It also answers questions of its
-- own — "did this host stop sending?", "which agent versions are in the
-- fleet?", "how big are the chunks?" — that need no item rows at all.
--
-- Cheap table: six frames per host per ten seconds at most, one row each.

CREATE TABLE IF NOT EXISTS process_snapshots
(
    tenant_id             LowCardinality(String),

    -- Arrival time. The frame header carries a timestamp field, but the agent
    -- leaves it at zero and its unit is undocumented, so it is stored raw
    -- below rather than interpreted into this column.
    received_at           DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The id every item row of this frame carries back. Also the payload_id
    -- of the connections/connections_payloads pair.
    snapshot_id           UUID,

    -- The frame's type byte and the name we know it by: CollectorProc,
    -- CollectorRealTime, CollectorContainer, CollectorContainerRealTime,
    -- CollectorProcDiscovery, CollectorConnections. Both, because the number
    -- is the contract and the name is what a human reads.
    message_type          UInt8,
    message_type_name     LowCardinality(String),

    -- Which of the four routes it arrived on. The type byte decides the body,
    -- not the path, so the two can disagree — and when they do, that is worth
    -- being able to see.
    path                  LowCardinality(String),

    -- The rest of the 16-byte header, raw. header_timestamp stays an Int64:
    -- the agent sends 0 and the unit is not documented anywhere, so turning
    -- it into a DateTime would be inventing a value.
    header_version        UInt8,
    header_encoding       UInt8,
    header_subscription_id UInt8,
    header_org_id         Int32,
    header_timestamp      Int64,
    frame_bytes           UInt64 CODEC(T64, ZSTD(1)),

    host_name             LowCardinality(String),

    -- The VPC id in every major cloud. Two hosts with the same private
    -- address are only distinguishable by this.
    network_id            String,

    -- The agent splits a host too big for one frame into group_size chunks
    -- sharing a group_id. A snapshot is complete when all of them arrived.
    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),

    -- notSpecified / fargateECS / fargateEKS / sidecar.
    container_host_type   LowCardinality(String),

    -- CollectorProc.host / CollectorContainer.host / CollectorProcDiscovery.host
    -- — the resolved host identity. all_tags is a genuine Datadog tag list, so
    -- it is a multiset like every other.
    host_id               Int64 CODEC(T64, ZSTD(1)),
    host_org_id           Int32 CODEC(T64, ZSTD(1)),
    host_display_name     String,
    host_all_tags         Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    host_num_cpus         Int32 CODEC(T64, ZSTD(1)),
    host_total_memory     Int64 CODEC(T64, ZSTD(1)),
    host_tag_index        Int32 CODEC(T64, ZSTD(1)),
    -- Nullable: "the backend never recomputed these tags" and "it recomputed
    -- them at the epoch" are different statements.
    host_tags_modified    Nullable(DateTime64(3, 'UTC')),

    -- SystemInfo. The CPU inventory is nine PARALLEL ARRAYS indexed together,
    -- for the same reason as processes.cpu_core_*: repeated is ordered, and
    -- two sockets legitimately report the same model string.
    sys_uuid              String,
    os_name               LowCardinality(String),
    os_platform           LowCardinality(String),
    os_family             LowCardinality(String),
    os_version            LowCardinality(String),
    kernel_version        LowCardinality(String),
    sys_total_memory      Int64 CODEC(T64, ZSTD(1)),
    cpu_numbers           Array(Int32),
    cpu_vendors           Array(LowCardinality(String)),
    cpu_families          Array(LowCardinality(String)),
    cpu_models            Array(String),
    cpu_physical_ids      Array(String),
    cpu_core_ids          Array(String),
    cpu_cores             Array(Int32),
    cpu_mhz               Array(Int64),
    cpu_cache_sizes       Array(Int32),

    -- CollectorProc.hints is a oneof with one member (hintMask). Nullable
    -- because "no hints" and "hint mask 0" are different, and only
    -- CollectorProc has the field at all.
    hint_mask             Nullable(Int32),

    -- The realtime envelope: CollectorRealTime and CollectorContainerRealTime
    -- carry host id / org / cpu count / memory directly instead of a Host.
    -- Kept apart from host_* so a reader never has to wonder which message
    -- shape a value came from.
    rt_host_id            Int64 CODEC(T64, ZSTD(1)),
    rt_org_id             Int32 CODEC(T64, ZSTD(1)),
    rt_num_cpus           Int32 CODEC(T64, ZSTD(1)),
    rt_total_memory       Int64 CODEC(T64, ZSTD(1)),

    -- How many items this frame carried, per kind. Lets a gap be spotted
    -- ("the frame said 400 processes, the table has 380") without counting
    -- rows in another table.
    process_count         UInt32 CODEC(T64, ZSTD(1)),
    process_stat_count    UInt32 CODEC(T64, ZSTD(1)),
    container_count       UInt32 CODEC(T64, ZSTD(1)),
    container_stat_count  UInt32 CODEC(T64, ZSTD(1)),
    discovery_count       UInt32 CODEC(T64, ZSTD(1)),
    connection_count      UInt32 CODEC(T64, ZSTD(1)),

    -- The headers the process-agent stamps on every submit. This is the only
    -- place the agent's own hostname and container count are kept; the item
    -- tables carry agent_version and request_id and join here for the rest.
    agent_hostname        LowCardinality(String),
    agent_version         LowCardinality(String),
    agent_container_count String,
    request_id            String,

    INDEX idx_tag_keys   mapKeys(host_all_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(host_all_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
-- "what did this host send, and when" — host first, then time, then the type
-- that tells the six streams apart.
ORDER BY (tenant_id, host_name, received_at, message_type)
-- 30 days, four times the item tables. A row is tiny and this is the table
-- that answers "when did that host stop reporting", which is a question asked
-- long after the processes themselves have aged out.
TTL toDateTime(received_at) + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- process_stats: CollectorRealTime.stats, the 2-second stream
-- ---------------------------------------------------------------------------
--
-- Deliberately NOT merged into `processes`. The realtime frame is a different
-- cadence (2s vs 10s), a different shape (no command, no user, no tags) and a
-- different volume, and a shared table would be four fifths NULL whichever
-- frame wrote the row.

CREATE TABLE IF NOT EXISTS process_stats
(
    tenant_id             LowCardinality(String),
    timestamp             DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                  LowCardinality(String),
    snapshot_id           UUID,

    pid                   Int32 CODEC(T64, ZSTD(1)),
    key                   UInt32 CODEC(T64, ZSTD(1)),
    create_time           DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    -- Same has_* convention as processes, kept identical on purpose so the
    -- two tables read the same way.
    has_create_time       UInt8,

    -- ProcessStat.nice and .threads are top-level fields, SEPARATE from
    -- Cpu.nice and Cpu.numThreads, and the agent does not always fill both.
    nice                  Int32 CODEC(T64, ZSTD(1)),
    threads               Int32 CODEC(T64, ZSTD(1)),
    open_fds              Int32 CODEC(T64, ZSTD(1)),

    mem_rss               UInt64 CODEC(T64, ZSTD(1)),
    mem_vms               UInt64 CODEC(T64, ZSTD(1)),
    mem_swap              UInt64 CODEC(T64, ZSTD(1)),
    mem_shared            UInt64 CODEC(T64, ZSTD(1)),
    mem_text              UInt64 CODEC(T64, ZSTD(1)),
    mem_lib               UInt64 CODEC(T64, ZSTD(1)),
    mem_data              UInt64 CODEC(T64, ZSTD(1)),
    mem_dirty             UInt64 CODEC(T64, ZSTD(1)),

    cpu_last_cpu          LowCardinality(String),
    cpu_total_pct         Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_user_pct          Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_system_pct        Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_num_threads       Int32 CODEC(T64, ZSTD(1)),
    cpu_nice              Int32 CODEC(T64, ZSTD(1)),
    cpu_user_time         Int64 CODEC(T64, ZSTD(1)),
    cpu_system_time       Int64 CODEC(T64, ZSTD(1)),
    cpu_core_names        Array(LowCardinality(String)),
    cpu_core_pcts         Array(Float32) CODEC(ZSTD(1)),

    io_read_rate          Float32 CODEC(Gorilla, ZSTD(1)),
    io_write_rate         Float32 CODEC(Gorilla, ZSTD(1)),
    io_read_bytes_rate    Float32 CODEC(Gorilla, ZSTD(1)),
    io_write_bytes_rate   Float32 CODEC(Gorilla, ZSTD(1)),

    net_connection_rate   Float32 CODEC(Gorilla, ZSTD(1)),
    net_bytes_rate        Float32 CODEC(Gorilla, ZSTD(1)),

    voluntary_ctx_switches   UInt64 CODEC(T64, ZSTD(1)),
    involuntary_ctx_switches UInt64 CODEC(T64, ZSTD(1)),

    container_id          String CODEC(ZSTD(1)),
    container_state       LowCardinality(String),
    process_state         LowCardinality(String),

    -- DEPRECATED UPSTREAM, stored anyway. These duplicate container-level
    -- numbers onto every process of that container; the agent still sends
    -- them, older agents send ONLY them, and a column that is redundant when
    -- container_stats exists is the difference between reading and not
    -- reading an old payload.
    container_health         LowCardinality(String),
    container_rbps           Float32 CODEC(Gorilla, ZSTD(1)),
    container_wbps           Float32 CODEC(Gorilla, ZSTD(1)),
    container_key            UInt32 CODEC(T64, ZSTD(1)),
    container_net_rcvd_ps    Float32 CODEC(Gorilla, ZSTD(1)),
    container_net_sent_ps    Float32 CODEC(Gorilla, ZSTD(1)),
    container_net_rcvd_bps   Float32 CODEC(Gorilla, ZSTD(1)),
    container_net_sent_bps   Float32 CODEC(Gorilla, ZSTD(1)),

    byte_key              String CODEC(ZSTD(1)),
    container_byte_key    String CODEC(ZSTD(1)),

    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),
    container_host_type   LowCardinality(String),
    rt_host_id            Int64 CODEC(T64, ZSTD(1)),
    rt_org_id             Int32 CODEC(T64, ZSTD(1)),
    rt_num_cpus           Int32 CODEC(T64, ZSTD(1)),
    rt_total_memory       Int64 CODEC(T64, ZSTD(1)),
    agent_version         LowCardinality(String),
    request_id            String
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, pid)
-- Same class as processes, and five times the rate: 7 days.
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

-- ---------------------------------------------------------------------------
-- containers: every Container struct, wherever it came from
-- ---------------------------------------------------------------------------
--
-- The same Container message reaches us three ways — as the snapshot-level
-- inventory on CollectorProc, as the whole point of CollectorContainer, and
-- hanging off an individual Process. One table with a `source` column rather
-- than three, for the reason 0001 gives about k8s_resources: the fields are
-- identical, and splitting by provenance would mean every query UNIONing
-- three tables to answer one question.

CREATE TABLE IF NOT EXISTS containers
(
    tenant_id             LowCardinality(String),
    timestamp             DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                  LowCardinality(String),
    snapshot_id           UUID,

    -- Which message the struct hung off: collector_proc, collector_container,
    -- process. The third one is per-process and carries pid below; the first
    -- two are host-wide inventories.
    source                LowCardinality(String),
    -- The owning process, for source = 'process'. Zero otherwise.
    pid                   Int32 CODEC(T64, ZSTD(1)),

    -- type is the runtime: docker, containerd, podman, ...
    type                  LowCardinality(String),
    id                    String CODEC(ZSTD(1)),
    -- name and image are marked DEPRECATED upstream and are still the only
    -- human-readable identity a container has in this message. Stored.
    name                  String,
    image                 String,
    repo_digest           String CODEC(ZSTD(1)),

    cpu_limit             Float32 CODEC(Gorilla, ZSTD(1)),
    memory_limit          UInt64 CODEC(T64, ZSTD(1)),
    cpu_request           Float32 CODEC(Gorilla, ZSTD(1)),
    memory_request        UInt64 CODEC(T64, ZSTD(1)),

    -- Enum names: unknown/created/restarting/running/paused/exited/dead and
    -- unknownHealth/starting/healthy/unhealthy.
    state                 LowCardinality(String),
    health                LowCardinality(String),

    -- Unix SECONDS on the wire (the agent sends CreatedAt.Unix()). Nullable
    -- because a container the runtime never reported a creation time for must
    -- not appear to have started at the epoch — the same rule
    -- container_events follows.
    created               Nullable(DateTime64(3, 'UTC')),
    started               Nullable(DateTime64(3, 'UTC')),

    rbps                  Float32 CODEC(Gorilla, ZSTD(1)),
    wbps                  Float32 CODEC(Gorilla, ZSTD(1)),
    net_rcvd_ps           Float32 CODEC(Gorilla, ZSTD(1)),
    net_sent_ps           Float32 CODEC(Gorilla, ZSTD(1)),
    net_rcvd_bps          Float32 CODEC(Gorilla, ZSTD(1)),
    net_sent_bps          Float32 CODEC(Gorilla, ZSTD(1)),

    user_pct              Float32 CODEC(Gorilla, ZSTD(1)),
    system_pct            Float32 CODEC(Gorilla, ZSTD(1)),
    total_pct             Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_usage_ns          Float32 CODEC(Gorilla, ZSTD(1)),

    mem_rss               UInt64 CODEC(T64, ZSTD(1)),
    mem_cache             UInt64 CODEC(T64, ZSTD(1)),
    mem_usage             UInt64 CODEC(T64, ZSTD(1)),
    mem_accounted         UInt64 CODEC(T64, ZSTD(1)),

    thread_count          UInt64 CODEC(T64, ZSTD(1)),
    thread_limit          UInt64 CODEC(T64, ZSTD(1)),

    key                   UInt32 CODEC(T64, ZSTD(1)),
    byte_key              String CODEC(ZSTD(1)),

    -- ContainerAddr is a repeated message; three parallel arrays keep the
    -- order and the triples together. protocol is the ConnectionType enum by
    -- name (tcp/udp).
    addr_ips              Array(String),
    addr_ports            Array(Int32),
    addr_protocols        Array(LowCardinality(String)),

    tags                  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- Container.host, backend-resolved and nil from a real agent. JSON, for
    -- the reason processes.process_host gives.
    host_info             String CODEC(ZSTD(3)),

    network_id            String,
    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),
    container_host_type   LowCardinality(String),
    agent_version         LowCardinality(String),
    request_id            String,

    INDEX idx_tag_keys   mapKeys(tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, id)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

-- ---------------------------------------------------------------------------
-- container_stats: the 2-second container stream
-- ---------------------------------------------------------------------------
--
-- ContainerStat arrives on two messages (CollectorRealTime.containerStats and
-- CollectorContainerRealTime.stats) with identical fields, so again one table
-- plus a source column.

CREATE TABLE IF NOT EXISTS container_stats
(
    tenant_id             LowCardinality(String),
    timestamp             DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                  LowCardinality(String),
    snapshot_id           UUID,
    source                LowCardinality(String),

    id                    String CODEC(ZSTD(1)),

    user_pct              Float32 CODEC(Gorilla, ZSTD(1)),
    system_pct            Float32 CODEC(Gorilla, ZSTD(1)),
    total_pct             Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_limit             Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_request           Float32 CODEC(Gorilla, ZSTD(1)),
    cpu_usage_ns          Float32 CODEC(Gorilla, ZSTD(1)),

    mem_rss               UInt64 CODEC(T64, ZSTD(1)),
    mem_cache             UInt64 CODEC(T64, ZSTD(1)),
    mem_limit             UInt64 CODEC(T64, ZSTD(1)),
    mem_usage             UInt64 CODEC(T64, ZSTD(1)),
    mem_accounted         UInt64 CODEC(T64, ZSTD(1)),
    memory_request        UInt64 CODEC(T64, ZSTD(1)),

    rbps                  Float32 CODEC(Gorilla, ZSTD(1)),
    wbps                  Float32 CODEC(Gorilla, ZSTD(1)),
    net_rcvd_ps           Float32 CODEC(Gorilla, ZSTD(1)),
    net_sent_ps           Float32 CODEC(Gorilla, ZSTD(1)),
    net_rcvd_bps          Float32 CODEC(Gorilla, ZSTD(1)),
    net_sent_bps          Float32 CODEC(Gorilla, ZSTD(1)),

    state                 LowCardinality(String),
    health                LowCardinality(String),

    key                   UInt32 CODEC(T64, ZSTD(1)),
    -- Unix seconds, Nullable for the same reason as containers.started.
    started               Nullable(DateTime64(3, 'UTC')),
    byte_key              String CODEC(ZSTD(1)),
    thread_count          UInt64 CODEC(T64, ZSTD(1)),
    thread_limit          UInt64 CODEC(T64, ZSTD(1)),

    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),
    container_host_type   LowCardinality(String),
    rt_host_id            Int64 CODEC(T64, ZSTD(1)),
    rt_org_id             Int32 CODEC(T64, ZSTD(1)),
    rt_num_cpus           Int32 CODEC(T64, ZSTD(1)),
    rt_total_memory       Int64 CODEC(T64, ZSTD(1)),
    agent_version         LowCardinality(String),
    request_id            String
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, id)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

-- ---------------------------------------------------------------------------
-- process_discoveries: the lightweight check
-- ---------------------------------------------------------------------------
--
-- Pid, command and user, no resource usage. Cheap enough that clusters which
-- turn the full process check off still run this one, so for a lot of hosts
-- this is the ONLY process-level data that ever arrives — which is why it
-- gets a table rather than being folded into `processes` with two thirds of
-- the columns empty.

CREATE TABLE IF NOT EXISTS process_discoveries
(
    tenant_id             LowCardinality(String),
    timestamp             DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                  LowCardinality(String),
    snapshot_id           UUID,

    pid                   Int32 CODEC(T64, ZSTD(1)),
    ns_pid                Int32 CODEC(T64, ZSTD(1)),
    create_time           DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    has_create_time       UInt8,
    byte_key              String CODEC(ZSTD(1)),

    comm                  LowCardinality(String),
    exe                   String CODEC(ZSTD(1)),
    -- Both forms, same trade-off as processes: args is the fidelity copy.
    cmdline               String CODEC(ZSTD(3)),
    args                  Array(String) CODEC(ZSTD(3)),
    cwd                   String CODEC(ZSTD(3)),
    root                  String CODEC(ZSTD(3)),
    on_disk               UInt8,
    ppid                  Int32 CODEC(T64, ZSTD(1)),
    pgroup                Int32 CODEC(T64, ZSTD(1)),

    user                  LowCardinality(String),
    uid                   Int32 CODEC(T64, ZSTD(1)),
    gid                   Int32 CODEC(T64, ZSTD(1)),
    euid                  Int32 CODEC(T64, ZSTD(1)),
    egid                  Int32 CODEC(T64, ZSTD(1)),
    suid                  Int32 CODEC(T64, ZSTD(1)),
    sgid                  Int32 CODEC(T64, ZSTD(1)),

    host_info             String CODEC(ZSTD(3)),

    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),
    agent_version         LowCardinality(String),
    request_id            String
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, host, timestamp, pid)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;

-- ---------------------------------------------------------------------------
-- connections_payloads: the payload-level half of CollectorConnections
-- ---------------------------------------------------------------------------
--
-- CollectorConnections has forty-odd fields that describe the SENDER and its
-- eBPF machinery rather than any one connection: kernel version, compilation
-- telemetry per asset, the routing table, the packed tag and DNS buffers. A
-- host sends thousands of connections per frame, so repeating those on every
-- connection row would multiply a kilobyte by a thousand for nothing.
--
-- payload_id ties the two together, and is the SAME uuid as the frame's
-- process_snapshots.snapshot_id.

CREATE TABLE IF NOT EXISTS connections_payloads
(
    tenant_id             LowCardinality(String),
    received_at           DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    payload_id            UUID,

    host_name             LowCardinality(String),
    network_id            String,
    group_id              Int32 CODEC(T64, ZSTD(1)),
    group_size            Int32 CODEC(T64, ZSTD(1)),
    container_host_type   LowCardinality(String),
    connection_count      UInt32 CODEC(T64, ZSTD(1)),

    -- System-probe host identity. These four are how you tell "eBPF is broken
    -- on the 5.4 kernels" from "eBPF is broken", and they were logged and
    -- never stored.
    architecture          LowCardinality(String),
    kernel_version        LowCardinality(String),
    platform              LowCardinality(String),
    platform_version      LowCardinality(String),

    -- resolvedResources: map<string, ResourceMetadata>. Kept as JSON because
    -- the value is a message with its own tag list and modification time, and
    -- because the map is empty in every payload a real agent sends (the
    -- backend fills it post-resolution).
    resolved_resources    String CODEC(ZSTD(3)),

    -- map<int32,string> pid -> container id. A plain Map: protobuf maps
    -- cannot repeat a key, so the multiset shape would buy nothing.
    container_for_pid     Map(Int32, String) CODEC(ZSTD(3)),

    -- The packed tag buffers, verbatim. The rows in `connections` carry their
    -- tags already resolved through the module's own decoder (process/tags.go
    -- getTags, reached via CollectorConnections.GetTags /
    -- GetConnectionsTags), but the buffers stay: the decoder is versioned
    -- (v1/v2/v3) and a future encoding we cannot read yet must still land
    -- somewhere readable.
    encoded_tags              String CODEC(ZSTD(3)),
    encoded_connections_tags  String CODEC(ZSTD(3)),

    -- The host's own tags, resolved out of encoded_tags at host_tags_index.
    host_tags_index       Int32 CODEC(T64, ZSTD(1)),
    host_tags             Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- Two telemetry sources, both fixed-key counter sets: the pre-7.35
    -- CollectorConnectionsTelemetry message (flattened to its field names)
    -- and the open-ended connTelemetryMap newer agents send instead. Maps
    -- rather than columns because the newer one has no schema at all — a
    -- counter added upstream lands here named, instead of being dropped.
    conn_telemetry        Map(LowCardinality(String), Int64) CODEC(ZSTD(3)),
    conn_telemetry_map    Map(LowCardinality(String), Int64) CODEC(ZSTD(3)),

    -- compilationTelemetryByAsset: per-asset runtime-compilation results.
    -- JSON, because the value is a four-field message with two enums in it.
    compilation_telemetry String CODEC(ZSTD(3)),
    kernel_header_fetch_result LowCardinality(String),
    -- COREResult by name: SuccessCustomBTF, BtfNotFound, VerifierError, ...
    core_telemetry        Map(String, LowCardinality(String)) CODEC(ZSTD(3)),
    prebuilt_ebpf_assets  Array(String),

    -- routes and routeMetadata are parallel lists indexed by
    -- connections.route_idx. JSON preserves both the order and the nested
    -- shape (subnet alias, interface hardware address, per-route tags).
    routes                String CODEC(ZSTD(3)),
    route_metadata        String CODEC(ZSTD(3)),

    -- AgentConfiguration: seven feature flags (npm/usm/dsm/ccm/csm/eudm/
    -- discoveryServiceMap). JSON so an eighth flag needs no migration.
    agent_configuration   String CODEC(ZSTD(3)),

    -- The DNS side. The v1 buffer, the v2 pair, and the domain list; plus the
    -- names decoded out of whichever encoding arrived, so the common case
    -- needs no decoder at read time.
    encoded_dns             String CODEC(ZSTD(3)),
    domains                 Array(String) CODEC(ZSTD(3)),
    encoded_domain_database String CODEC(ZSTD(3)),
    encoded_dns_lookups     String CODEC(ZSTD(3)),
    dns_names               Array(String) CODEC(ZSTD(3)),

    -- resolvedHostsByName and resolvedPublicIps: backend-resolution maps of
    -- messages. JSON, same reasoning as resolved_resources.
    resolved_hosts_by_name String CODEC(ZSTD(3)),
    resolved_public_ips    String CODEC(ZSTD(3)),

    ecs_task              String,
    -- The /etc/resolv.conf variants seen on the host, indexed by
    -- connections.resolv_conf_idx.
    resolv_confs          Array(String) CODEC(ZSTD(3)),

    agent_hostname        LowCardinality(String),
    agent_version         LowCardinality(String),
    agent_container_count String,
    request_id            String,

    INDEX idx_tag_keys   mapKeys(host_tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(host_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(received_at)
ORDER BY (tenant_id, host_name, received_at)
TTL toDateTime(received_at) + INTERVAL 7 DAY;

-- ---------------------------------------------------------------------------
-- connections: one row per Connection
-- ---------------------------------------------------------------------------
--
-- The highest-volume table of this migration by a wide margin: a busy host
-- reports thousands of connections every 30 seconds. Sized and TTL'd like
-- processes, and everything that repeats per frame lives in
-- connections_payloads instead.

CREATE TABLE IF NOT EXISTS connections
(
    tenant_id             LowCardinality(String),
    timestamp             DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    host                  LowCardinality(String),
    -- The frame. Also process_snapshots.snapshot_id and
    -- connections_payloads.payload_id.
    payload_id            UUID,

    pid                   Int32 CODEC(T64, ZSTD(1)),

    -- Addr is {ip, port, containerId, hostName}; both ends expanded, because
    -- "who talked to this ip" must be a column filter, not a JSON extract.
    laddr_ip              String,
    laddr_port            Int32 CODEC(T64, ZSTD(1)),
    laddr_container_id    String CODEC(ZSTD(1)),
    laddr_host_name       String,
    raddr_ip              String,
    raddr_port            Int32 CODEC(T64, ZSTD(1)),
    raddr_container_id    String CODEC(ZSTD(1)),
    raddr_host_name       String,

    -- Enum names throughout: v4/v6, tcp/udp, incoming/outgoing/local/none,
    -- ephemeralTrue/ephemeralFalse/ephemeralUnspecified.
    family                LowCardinality(String),
    type                  LowCardinality(String),
    direction             LowCardinality(String),
    is_local_port_ephemeral LowCardinality(String),

    last_bytes_sent       UInt64 CODEC(T64, ZSTD(1)),
    last_bytes_received   UInt64 CODEC(T64, ZSTD(1)),
    last_packets_sent     UInt64 CODEC(T64, ZSTD(1)),
    last_packets_received UInt64 CODEC(T64, ZSTD(1)),
    last_retransmits      UInt32 CODEC(T64, ZSTD(1)),

    -- ProtocolStack.stack is a repeated enum and the ORDER is the stack:
    -- [protocolTLS, protocolHTTP2] is not the same statement as the reverse.
    protocol_stack        Array(LowCardinality(String)),

    net_ns                UInt32 CODEC(T64, ZSTD(1)),
    remote_network_id     String,

    -- IPTranslation is the conntrack NAT entry and is absent for every
    -- connection that was not translated — which is most of them. Nullable,
    -- because 0.0.0.0:0 would read as a real translation.
    ip_translation_repl_src_ip   Nullable(String),
    ip_translation_repl_dst_ip   Nullable(String),
    ip_translation_repl_src_port Nullable(Int32),
    ip_translation_repl_dst_port Nullable(Int32),

    rtt                   UInt32 CODEC(T64, ZSTD(1)),
    rtt_var               UInt32 CODEC(T64, ZSTD(1)),
    intra_host            UInt8,

    dns_successful_responses UInt32 CODEC(T64, ZSTD(1)),
    dns_failed_responses     UInt32 CODEC(T64, ZSTD(1)),
    dns_timeouts             UInt32 CODEC(T64, ZSTD(1)),
    dns_success_latency_sum  UInt64 CODEC(T64, ZSTD(1)),
    dns_failure_latency_sum  UInt64 CODEC(T64, ZSTD(1)),
    -- rcode -> count. A protobuf map, so a plain Map is right.
    dns_count_by_rcode       Map(UInt32, UInt32) CODEC(ZSTD(3)),

    -- The three per-domain DNS maps: the deprecated flat one older agents
    -- still send, and the two keyed by query type (by domain index and by
    -- domain OFFSET, which are different keyspaces into different DNS
    -- encodings). Nested maps of messages, so JSON rather than four more
    -- Map types nobody would be able to read.
    dns_stats_by_domain                     String CODEC(ZSTD(3)),
    dns_stats_by_domain_by_query_type       String CODEC(ZSTD(3)),
    dns_stats_by_domain_offset_by_query_type String CODEC(ZSTD(3)),

    last_tcp_established  UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_closed       UInt32 CODEC(T64, ZSTD(1)),

    -- Indices into connections_payloads.routes / .route_metadata /
    -- .resolv_confs.
    route_idx             Int32 CODEC(T64, ZSTD(1)),
    route_target_idx      Int32 CODEC(T64, ZSTD(1)),
    resolv_conf_idx       Int32 CODEC(T64, ZSTD(1)),

    -- The four USM aggregation blobs. Each is a separately serialised
    -- protobuf from a package this repo does not vendor, so they are stored
    -- as the bytes they are (ClickHouse String is a byte string). Keeping
    -- them costs disk; dropping them would mean the per-endpoint HTTP,
    -- HTTP/2, Kafka/DSM and database statistics are gone for good, and they
    -- are the entire content of Universal Service Monitoring.
    http_aggregations         String CODEC(ZSTD(3)),
    http2_aggregations        String CODEC(ZSTD(3)),
    data_streams_aggregations String CODEC(ZSTD(3)),
    database_aggregations     String CODEC(ZSTD(3)),

    -- tags_indices is the deprecated repeated-uint32 form; tags_idx points
    -- into connections_payloads.encoded_connections_tags. tags is what that
    -- index resolves to, decoded with the module's own reader — a multiset,
    -- because these are Datadog tags like any other.
    tags_indices          Array(UInt32),
    tags_idx              Int32 CODEC(T64, ZSTD(1)),
    tags_checksum         UInt32 CODEC(T64, ZSTD(1)),
    tags                  Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- Two more tag indices into the same buffer, resolved the same way.
    local_container_tags_index Int32 CODEC(T64, ZSTD(1)),
    local_container_tags       Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    remote_service_tags_idx    Int32 CODEC(T64, ZSTD(1)),
    remote_service_tags        Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    state_index           UInt32 CODEC(T64, ZSTD(1)),
    -- errno -> count for failed TCP handshakes.
    tcp_failures_by_err_code Map(UInt32, UInt32) CODEC(ZSTD(3)),
    remote_ecs_task       String,
    system_probe_conn     UInt8,

    -- The extended TCP counters newer kernels expose.
    last_tcp_rto_count      UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_recovery_count UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_reord_seen     UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_rcv_ooo_pack   UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_delivered_ce   UInt32 CODEC(T64, ZSTD(1)),
    last_tcp_probe0_count   UInt32 CODEC(T64, ZSTD(1)),
    tcp_ecn_negotiated      UInt8,

    agent_version         LowCardinality(String),
    request_id            String,

    INDEX idx_tag_keys   mapKeys(tags) TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
-- "what did this host talk to, in this window" — and pid, because the second
-- question is always which process.
ORDER BY (tenant_id, host, timestamp, pid)
TTL toDateTime(timestamp) + INTERVAL 7 DAY;
