# Process intake tables

Everything the process-agent sends on `process.<site>`, and where it lands.

Four HTTP routes share one 16-byte frame and one reply; the frame's type byte,
not the path, says what the body is. Six message types arrive, and each has a
table. Schema: [`schema/migrations/0008_process.sql`](../../schema/migrations/0008_process.sql).
Converters: [`intake/router_process.go`](../../intake/router_process.go).
Rows: [`apps/storage/rows_process.go`](../../apps/storage/rows_process.go) and
[`rows_process_extra.go`](../../apps/storage/rows_process_extra.go).

## The tables

| table | engine | ORDER BY | TTL | fed by |
|---|---|---|---|---|
| `process_snapshots` | MergeTree | `(tenant_id, host_name, received_at, message_type)` | 30 d | every frame, all six message types |
| `processes` | MergeTree | `(tenant_id, host, timestamp, pid)` | 7 d | `CollectorProc.processes[]` |
| `process_stats` | MergeTree | `(tenant_id, host, timestamp, pid)` | 7 d | `CollectorRealTime.stats[]` |
| `containers` | MergeTree | `(tenant_id, host, timestamp, id)` | 7 d | `CollectorProc.containers[]`, `CollectorContainer.containers[]`, `Process.container` |
| `container_stats` | MergeTree | `(tenant_id, host, timestamp, id)` | 7 d | `CollectorRealTime.containerStats[]`, `CollectorContainerRealTime.stats[]` |
| `process_discoveries` | MergeTree | `(tenant_id, host, timestamp, pid)` | 7 d | `CollectorProcDiscovery.processDiscoveries[]` |
| `connections` | MergeTree | `(tenant_id, host, timestamp, pid)` | 7 d | `CollectorConnections.connections[]` |
| `connections_payloads` | MergeTree | `(tenant_id, host_name, received_at)` | 7 d | `CollectorConnections`, one row per frame |

All eight partition by day (`toDate`), which is what makes retention a
`DROP PARTITION` rather than a mutation. `process_snapshots` keeps rows four
times longer than the item tables on purpose: a row is tiny, and "when did that
host stop reporting" is asked long after the processes themselves aged out.

Nothing here is a `ReplacingMergeTree`. Every one of these is an append-only
observation at a point in time — a process list, a stats sample, a connection
table — not a current-state object with an identity to upsert on.

## Column → source field

The migration carries a comment per column; this is the map at message level.

### `process_snapshots`
One row per received frame. `snapshot_id` is a UUID minted on arrival and is
what every item row of that frame carries back.

| columns | source |
|---|---|
| `message_type`, `message_type_name`, `header_*`, `frame_bytes`, `path` | the 16-byte frame header plus the route it arrived on |
| `host_name`, `network_id`, `group_id`, `group_size`, `container_host_type` | the message's own envelope fields |
| `host_id`, `host_org_id`, `host_display_name`, `host_all_tags`, `host_num_cpus`, `host_total_memory`, `host_tag_index`, `host_tags_modified` | `*Host` (CollectorProc / CollectorContainer / CollectorProcDiscovery) |
| `sys_uuid`, `os_*`, `kernel_version`, `sys_total_memory`, `cpu_*` (nine parallel arrays) | `*SystemInfo` and its `[]*CPUInfo` |
| `hint_mask` | `CollectorProc.hints` oneof, `Nullable` |
| `rt_host_id`, `rt_org_id`, `rt_num_cpus`, `rt_total_memory` | the realtime messages, which inline these instead of a `Host` |
| `*_count` | `len()` of the message's item list, per kind |
| `agent_hostname`, `agent_version`, `agent_container_count`, `request_id` | `X-Dd-Hostname`, `X-Dd-Processagentversion`, `X-Dd-ContainerCount`, `X-DD-Request-ID` |

### `processes`
The first eighteen columns are the ones the table shipped with and are
unchanged. Migration 0008 appends the rest of `process.Process`:

| columns | source |
|---|---|
| `ns_pid`, `key` | `Process.nsPid`, `Process.key` |
| `args`, `cwd`, `root`, `on_disk`, `pgroup` (plus the existing `ppid`/`comm`/`exe`/`cmdline`) | `Command` |
| `uid`, `gid`, `euid`, `egid`, `suid`, `sgid` | `ProcessUser` |
| `mem_swap`, `mem_shared`, `mem_text`, `mem_lib`, `mem_data`, `mem_dirty` | `MemoryStat` |
| `cpu_last_cpu`, `cpu_user_pct`, `cpu_system_pct`, `cpu_nice`, `cpu_user_time`, `cpu_system_time`, `cpu_core_names`, `cpu_core_pcts` | `CPUStat`, the last two from `CPUStat.cpus[]` |
| `io_*` | `IOStat` |
| `voluntary_ctx_switches`, `involuntary_ctx_switches` | `Process` |
| `net_connection_rate`, `net_bytes_rate` | `ProcessNetworks` |
| `process_context`, `language`, `injection_state` | `Process` |
| `port_tcp`, `port_udp` | `PortInfo` |
| `generated_service_name(_source)`, `dd_service_name(_source)`, `additional_generated_names(_sources)`, `tracer_runtime_ids`, `tracer_service_names`, `apm_instrumentation`, `service_resources` | `ServiceDiscovery` |
| `zombie_children_count`, `zombie_net_rate`, `has_zombie_aggregation` | `Process` |
| `container_key`, `byte_key`, `container_byte_key` | `Process` |
| `has_create_time` | derived, see below |
| `process_host` | `Process.host`, as JSON |
| `network_id`, `group_id`, `group_size`, `container_host_type` | the `CollectorProc` envelope |
| `agent_version`, `request_id` | request headers |
| `host_uuid`, `os_*`, `kernel_version` | `CollectorProc.info` |

`Process.container` does not become a column here — it becomes a row in
`containers` with `source = 'process'` and `pid` set.

### `process_stats`
Every field of `ProcessStat`, including the eight deprecated container-level
duplicates (`container_health`, `container_rbps`, `container_wbps`,
`container_key`, `container_net_*`), plus the flattened `MemoryStat`, `CPUStat`,
`IOStat` and `ProcessNetworks`, plus the `CollectorRealTime` envelope.

### `containers` and `container_stats`
Every field of `Container` and `ContainerStat` respectively. `source` says which
message the struct hung off (`collector_proc`, `collector_container`,
`process`; `collector_realtime`, `collector_container_realtime`).
`ContainerAddr[]` becomes the three parallel arrays `addr_ips` / `addr_ports` /
`addr_protocols`.

### `process_discoveries`
Every field of `ProcessDiscovery`, with `Command` and `ProcessUser` flattened
the same way `processes` flattens them.

### `connections` and `connections_payloads`
`CollectorConnections` is split in two: the ~40 payload-level fields (kernel
identity, telemetry, routing table, packed buffers) go to
`connections_payloads` once per frame, and each `Connection` becomes a row in
`connections`. They share `payload_id`, which is also the frame's
`process_snapshots.snapshot_id`.

## Semantics worth knowing

**`snapshot_id` / `payload_id` are the same uuid.** One is minted per received
frame. `process_snapshots.snapshot_id` = `processes.snapshot_id` =
`connections.payload_id` = `connections_payloads.payload_id`. It also groups the
chunks of one snapshot: the agent splits a big host into `group_size` frames
sharing a `group_id`, and each chunk gets its own uuid, so "one snapshot" is
`(host, group_id)` and "one frame" is the uuid.

**Every proto enum is stored by name.** `state = 'R'`, `container_state =
'running'`, `language = 'LANGUAGE_GO'`, `protocol_stack = ['protocolTLS',
'protocolHTTP2']`, `core_telemetry['tcp'] = 'SuccessEmbeddedBTF'`. Never the
number: the `.proto` is not ours, and a renumbering upstream would rewrite
stored history.

**`has_create_time` is the absent/zero flag.** `processes.create_time`,
`process_stats.create_time` and `process_discoveries.create_time` are plain
`DateTime` columns; making `processes.create_time` `Nullable` would rewrite
every existing part. So absence rides beside the value: **filter on
`has_create_time = 1` before reading `create_time`**. Everywhere else, absence
is a real `NULL` — `containers.created`/`started`, `container_stats.started`,
`process_snapshots.host_tags_modified`, `connections.ip_translation_*`,
`process_snapshots.hint_mask`.

**Timestamps.** `timestamp` / `received_at` is ARRIVAL time: the body carries no
sample time, and the frame header's own timestamp field is left at zero by the
agent, so it is stored raw and uninterpreted in
`process_snapshots.header_timestamp`. `createTime` is milliseconds on the wire;
`Container.created`/`started` and `Host.tagsModified` are unix seconds.

**`args` vs `cmdline`.** Both are stored. `cmdline` is argv joined on spaces and
is irreversible — `--filter=a b` and two separate arguments produce the same
string. `args` is the fidelity copy; prefer it.

**Raw columns.** `byte_key` / `container_byte_key`, `encoded_tags`,
`encoded_connections_tags`, `encoded_dns`, `encoded_domain_database`,
`encoded_dns_lookups`, and `connections.http_aggregations` /
`http2_aggregations` / `data_streams_aggregations` / `database_aggregations` are
`String` columns holding the bytes verbatim (ClickHouse `String` is a byte
string, not UTF-8). The four aggregation blobs are separately serialised
protobufs from a package this repo does not vendor: no decoder exists yet, and
one can be written against stored bytes but not against bytes we discarded.

**JSON columns.** `processes.process_host`, `containers.host_info`,
`process_discoveries.host_info`, `processes.service_resources`, and on
`connections_payloads`: `resolved_resources`, `compilation_telemetry`, `routes`,
`route_metadata`, `agent_configuration`, `resolved_hosts_by_name`,
`resolved_public_ips`; on `connections`: the three `dns_stats_by_domain*` maps.
Empty string means the field was absent — never the literal `null`.
`routes` keeps a slot for a nil element, because `connections.route_idx` is a
position in that list.

**Resolved tags.** `connections.tags`, `.local_container_tags`,
`.remote_service_tags` and `connections_payloads.host_tags` are decoded out of
the packed buffers with the agent-payload module's own reader
(`CollectorConnections.GetTags` / `GetConnectionsTags`, `process/tags.go`,
v1/v2/v3). The raw index and the raw buffer are stored beside the result.
Caveat: the wire has no sentinel for "no tag index", so an unset `tagsIdx`
reads as `0`, which is a valid index; when the buffer is empty nothing is
resolved, and the raw index is always there to check against.

**Tag maps are multisets** — `Map(LowCardinality(String), Array(LowCardinality(String)))`,
queried with `has(tags['k'], 'v')`. Every tag column on these tables uses it,
including `process_snapshots.host_all_tags`. `connections_payloads.container_for_pid`
and `connections.dns_count_by_rcode` / `tcp_failures_by_err_code` are protobuf
maps, which cannot repeat a key, so they stay plain `Map`.

**Sensitive columns.** `cmdline` and `args` hold whole command lines, which is
where secrets passed as flags end up; `cwd`, `root` and `exe` expose filesystem
layout; `laddr_ip` / `raddr_ip` and the `resolv_confs` / `dns_names` columns are
network topology. Nothing here is a credential of ours — request headers are
never stored on these tables, only the agent version, hostname, container count
and request id — but treat `cmdline`/`args` as the same class of data as a log
line.

**Undecodable input.** A frame `DecodeMessage` rejects goes to `raw_payloads`
with `intake = 'process'`, `reason = 'decode_error'`. A frame that decodes into
a type the route has no table for goes there with `reason =
'unexpected_shape'`. Either way the agent still receives its `ResCollector`:
that reply is byte-identical to what it was before this work, because a nil
`Status` in it dereferences nil inside the agent's `CheckRunner.UpdateRTStatus`
and crash-loops the container.

## dropped_by_decision

Every field of every one of the six message types reaches a column. What
follows are placement and indexing decisions, not lost data:

- **`X-Dd-Hostname` and `X-Dd-ContainerCount` are kept once per frame**, on
  `process_snapshots` (and `connections_payloads`), not denormalised onto the
  item tables. `agent_version` and `request_id` are on the item tables because
  they are what a "which agent wrote this row" query filters on; the other two
  are one join on `snapshot_id` away and would otherwise be repeated on every
  one of a few hundred rows.
- **Nested `*Host` sub-messages are JSON, not columns.** `Process.host`,
  `Container.host` and `ProcessDiscovery.host` are backend-resolution fields a
  real agent leaves nil, so eight columns each would be empty in every row we
  will ever receive. They go to `process_host` / `host_info` as JSON —
  lossless, and free when absent. The `Host` that IS populated, the snapshot's
  own, gets real columns on `process_snapshots`.
- **Per-connection DNS names are not materialised.** The encoded buffers
  (`encoded_dns`, `encoded_domain_database`, `encoded_dns_lookups`) and the
  decoded name list (`dns_names`) are all on `connections_payloads`; resolving
  one `Addr` against them is a read-time operation on data we already hold, and
  doing it per connection would multiply the work by thousands per frame.
- **Only the primary tag map of each table gets the bloom-filter index pair.**
  `connections.local_container_tags` and `.remote_service_tags` are stored but
  unindexed: three index pairs on the highest-volume table of this intake is a
  write cost the queries do not ask for, and they can be added later without a
  rewrite.
