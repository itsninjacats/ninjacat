# Network Device Monitoring tables

Four hosts, five routes, one product (Network Device Monitoring), decoded and
stored by `intake/router_ndm.go` into eight ClickHouse tables defined in
`schema/migrations/0007_ndm.sql`:

| Route | Host | Feeds |
|---|---|---|
| `POST /api/v2/ndm` | `ndm-intake.<site>` | `ndm_devices`, `ndm_interfaces`, `ndm_ip_addresses`, `ndm_metadata_objects` |
| `POST /api/v2/ndmconfig` | `ndm-intake.<site>` | `ndm_device_configs` |
| `POST /api/v2/ndmtraps` | `snmp-traps-intake.<site>` | `snmp_traps` |
| `POST /api/v2/ndmflow` | `ndmflow-intake.<site>` | `netflow_flows` |
| `POST /api/v2/netpath` | `netpath-intake.<site>` | `network_paths` |

All five routes reply `202 {}` unconditionally (the response is `defer`red at
function entry, matching every other event-platform intake) and none has a
`*_dd_url` config knob to redirect. A tenant-less request (`TenantFromContext`
returns `""`) is still answered the same way but stores nothing, per the
project rule.

## Two kinds of source type

`netflow_flows` and `network_paths` decode into the REAL vendored Go types —
`comp/netflow/payload.FlowPayload` and `pkg/networkpath/payload.NetworkPath` —
both small, dependency-free modules already in `go.mod`. Their column lists
below are exact.

`ndm_devices`/`ndm_interfaces`/`ndm_ip_addresses`/`ndm_metadata_objects`
(from `/api/v2/ndm`), `ndm_device_configs` (from `/api/v2/ndmconfig`) and
`snmp_traps` (from `/api/v2/ndmtraps`) have no importable Go type: their real
structs live in `pkg/networkdevice/metadata`,
`pkg/networkconfigmanagement/report` and `comp/snmptraps/formatter`, all of
which pull in the whole `datadog-agent` module. `intake/router_ndm.go` mirrors
them with local structs reconstructed from the wire (docs + captures), using
the same decode-twice pattern as `kubeActionDecode` in `router_kubeops.go`:
decode once into the typed mirror, decode again into a raw map, and whatever
key the mirror does not declare survives as JSON text rather than being
dropped. The JSON field names on these three tracks are ninjacat's best
reconstruction, not a copy of a vendored struct tag — the intake audit that
inventoried what these routes decode today (section 2, NDM) has the full
field-by-field breakdown of what was directly observed on the wire vs.
inferred from the vendored types that exist but cannot be imported.

## Payload-level columns, shared by the four `/api/v2/ndm` tables

`ndm_devices`, `ndm_interfaces`, `ndm_ip_addresses` and `ndm_metadata_objects`
all carry the SAME first block of columns, copied onto every row from the
enclosing `NetworkDevicesMetadata` envelope rather than normalized into a
separate table (one collection pass is small enough that the repetition costs
nothing and keeps every row self-contained for a query):

- `namespace`, `subnet`, `integration`, `collect_timestamp` — the payload's own fields.
- `extra Map(String, String)` — every top-level payload key the local mirror
  struct (`ndmMetadataPayload`) does not declare, raw JSON text per key. A
  newer agent's additions show up here instead of vanishing.

## `ndm_devices`

**Engine** `MergeTree` · **Partition** `toDate(collect_timestamp)` ·
**Order by** `(tenant_id, namespace, device_id, collect_timestamp)` ·
**TTL** `collect_timestamp + INTERVAL 30 DAY`

One row per `DeviceMetadata` object per collection pass — append-only
history, not an upsert, so devices that stop reporting simply age out rather
than being deleted. Classified with the bursty-per-pass Kubernetes tables
(`k8s_resources`): a full inventory arrives every pass, not incrementally.

Fed by `handleNDMMetadata` → `ndmDeviceRow`.

| Column | Source |
|---|---|
| `device_id` | `DeviceMetadata.id` |
| `id_tags`, `tags` | `id_tags`/`tags` (`[]string`), through `tagsToMultiMap` — genuine Datadog tag multisets, bloom-filtered like every other tags column |
| `ip_address`, `name`, `description`, `sys_object_id`, `location`, `profile`, `profile_version`, `vendor`, `serial_number`, `version`, `product_name`, `model`, `os_name`, `os_version`, `os_hostname` | same-named `DeviceMetadata` fields |
| `status`, `ping_status` | `status`/`ping_status`, numeric wire enum (1=Reachable, 2=Unreachable) mapped to the `Enum8('unknown'=0,'reachable'=1,'unreachable'=2)` name by `ndmDeviceStatusName` — absent or any other number reads as `unknown`, never a guess |
| `device_subnet`, `device_integration`, `device_type` | `DeviceMetadata`'s OWN `subnet`/`integration`/`device_type` — kept distinct from the payload-level `subnet`/`integration` columns above because the profile genuinely can differ per device inside one pass |

## `ndm_interfaces`

**Engine** `MergeTree` · **Partition** `toDate(collect_timestamp)` ·
**Order by** `(tenant_id, namespace, device_id, if_index, collect_timestamp)` ·
**TTL** `collect_timestamp + INTERVAL 30 DAY`

One row per `InterfaceMetadata` object per pass. Fed by `handleNDMMetadata` →
`ndmInterfaceRow`.

| Column | Source |
|---|---|
| `device_id`, `id_tags`, `if_index`, `raw_id`, `raw_id_type`, `name`, `alias`, `description`, `mac_address`, `meraki_status` | same-named `InterfaceMetadata` fields (`if_index` is `index` on the wire — see below) |
| `admin_status`, `oper_status`, `type` | the wire's numeric SNMP enums (`IfAdminStatus`, `IfOperStatus`, ifType), kept as the RAW NUMBER — the vendored struct is not importable, so the exact value set could not be confirmed from source, and a wrong guessed label would be worse than a number |
| `is_physical`, `meraki_enabled` | `*bool` on the wire → `Nullable(UInt8)`, via `ndmBoolPtr`: nil/true/false are three distinct facts and a plain `UInt8` would make "not reported" indistinguishable from `false` |

**Column named `if_index`, not `index`**: `INDEX` is part of `CREATE TABLE`'s
own grammar (the bloom_filter declarations on this and other tables), and a
column literally called `index` parses as the start of one and fails with a
confusing syntax error — hit and fixed while writing this migration.

## `ndm_ip_addresses`

**Engine** `MergeTree` · **Partition** `toDate(collect_timestamp)` ·
**Order by** `(tenant_id, namespace, interface_id, collect_timestamp)` ·
**TTL** `collect_timestamp + INTERVAL 30 DAY`

One row per `IPAddressMetadata` object per pass: `interface_id`, `ip_address`,
`prefixlen`. Fed by `handleNDMMetadata` → `ndmIPAddressRow`.

## `ndm_metadata_objects`

**Engine** `MergeTree` · **Partition** `toDate(collect_timestamp)` ·
**Order by** `(tenant_id, namespace, kind, device_id, collect_timestamp)` ·
**TTL** `collect_timestamp + INTERVAL 30 DAY`

Everything else `/api/v2/ndm` carries: `links`, `vpn_tunnels`,
`netflow_exporters`, `diagnoses`, `device_oids` and `scan_status` (a single
optional object, not a list — present iff `NetworkDevicesMetadata.scan_status`
was sent). Six small, varied shapes — a link has no `device_id` at all, a
diagnosis nests its own list — kept whole as JSON (`object`) rather than typed
into six more tables for a handful of rows each. `kind`, `device_id` and
`interface_id` are promoted for the filters that matter; `ndmObjectIDs`
extracts whichever of `device_id`/`interface_id` the element actually
declares (empty string for the shapes that carry neither). A malformed
element (JSON that is not even an object) still keeps its raw bytes rather
than aborting the batch — `ndmMetadataObjectRows`' `_ = json.Unmarshal(...)`
error is intentionally ignored for exactly that reason.

Fed by `handleNDMMetadata` → `ndmMetadataObjectRows`.

## `ndm_device_configs`

**Engine** `MergeTree` · **Partition** `toDate(timestamp)` ·
**Order by** `(tenant_id, namespace, device_id, timestamp)` ·
**TTL** `timestamp + INTERVAL 7 DAY`

One row per `NetworkDeviceConfig` (`report.NCMPayload.configs[]`). Classified
with `k8s_manifests`: a whole document per row (a full running/startup
config), bulky rather than frequent, hence the short TTL relative to the
other NDM tables. Fed by `handleNDMConfig`.

| Column | Source |
|---|---|
| `namespace`, `collect_timestamp` | the enclosing `NCMPayload`'s own fields |
| `agent_hostname`, `inventories` | observed on the wire but **not** in the documented `report.NCMPayload` struct (flagged in the gap report) — real columns rather than folded into `extra`, since they recur on every request rather than being a one-off addition. `inventories` is kept as raw JSON text (its shape is not confirmed enough to type yet) |
| `extra` | any OTHER top-level `NCMPayload` key beyond the ones above, raw JSON text |
| `device_id`, `device_ip`, `config_type`, `config_source`, `content` | same-named `NetworkDeviceConfig` fields |
| `tags` | `tags` (`[]string`), through `tagsToMultiMap` |
| `timestamp` | `NetworkDeviceConfig.timestamp` — backfilled agent-side to the collection time whenever a device's own config has none (`ToNCMPayload`, per the gap report), so this is never legitimately 0 by the time it reaches us. No `Nullable` needed here, unlike most other timestamps in this schema |

## `snmp_traps`

**Engine** `MergeTree` · **Partition** `toDate(timestamp)` ·
**Order by** `(tenant_id, device, timestamp)` ·
**TTL** `timestamp + INTERVAL 30 DAY`

One row per `{"trap": {...}}` envelope (`comp/snmptraps/formatter`).
Classified with `events`/`container_events`: a trap is a thing that
happened, never replaced. Fed by `handleNDMTraps` → `ndmTrapRow`.

| Column | Source |
|---|---|
| `ddsource`, `uptime`, `snmp_trap_oid`, `snmp_trap_name`, `snmp_trap_mib` | same-named trap fields |
| `ddtags` | `ddtags` (comma-joined string), split by `ndmSplitTags` (an empty string yields no tags, unlike a bare `strings.Split` which yields one bogus empty-key tag) and multiset-mapped |
| `device` | pulled out of `ddtags`' `snmp_device:<ip>` — the column every "who sent this trap" query filters on first |
| `enterprise_oid`, `generic_trap`, `specific_trap` | **v1-only** fields. `generic_trap`/`specific_trap` are `Nullable(Int32)` via `*int32`, so a v1 trap reporting `0` stays distinct from a v2/v3 trap where the field was never sent at all |
| `var_oids`, `var_types`, `var_values` | parallel arrays over `variables[]`; `value` is kept as the JSON text of whatever the SNMP type decoded to (string, number, or something MIB-specific) — a genuine property of SNMP data, not a decode gap |
| `enriched` | every trap-object key beyond the fixed formatter fields (MIB/profile-defined by the OID resolver, so the name set is not fixed), raw JSON text per key |
| `raw` | the trap sub-object exactly as received |

## `netflow_flows`

**Engine** `MergeTree` · **Partition** `toDate(flow_start)` ·
**Order by** `(tenant_id, device_namespace, exporter_ip, flow_start)` ·
**TTL** `flow_start + INTERVAL 14 DAY`

One row per `comp/netflow/payload.FlowPayload`, decoded through
`intake.ndmDecodeFlows` — **never** a bare `json.Unmarshal`.
`FlowPayload.MarshalJSON` spreads `AdditionalFields` over the root object
instead of writing an `"additional_fields"` key, and the type has no
`UnmarshalJSON` to undo that; a plain decode silently drops every configured
extra field. `ndmDecodeFlows` decodes twice (once into the typed struct, once
into a raw map) and reconstructs `AdditionalFields` from whatever root key the
struct does not own, numbers kept as `json.Number`. Classified with `logs`:
every configured exporter reports on its own timer, independent of every
other NDM table, and can be the highest-volume track by a wide margin.

Fed by `handleNDMFlow` → `ndmFlowRow`.

| Column | Source |
|---|---|
| `flow_type`, `sampling_rate`, `direction`, `bytes`, `packets`, `ether_type`, `ip_protocol`, `tos`, `dscp`, `dscp_name`, `host`, `tcp_flags` | same-named `FlowPayload` fields |
| `flow_start`, `flow_end` | `start`/`end` — documented "in seconds" on the Go struct, unlike `flush_timestamp` below. Named `flow_start`/`flow_end`, not `start`/`end` (`end` risks the same reserved-word trap as `ndm_interfaces.index`, worth avoiding even where it happens to parse) |
| `flush_timestamp` | `flush_timestamp` — **assumed milliseconds**: undocumented on the wire, but `TestNDMDecodeFlowsRestoresAdditionalFields` pins a captured value at `1758326400123`, a 13-digit millisecond epoch. `start`/`end` explicitly say "in seconds" in the source, which reads as the exception, not the rule, for this intake |
| `device_namespace`, `exporter_ip` | `device.namespace`, `exporter.ip` |
| `source_ip`, `source_port`, `source_mac`, `source_mask`, `source_reverse_dns_hostname` and the `destination_*` equivalents | `source`/`destination` (`Endpoint`). `*_port` is `LowCardinality(String)`, not numeric — the wire's own `Endpoint.Port` is a string and can read `"*"` for an ephemeral port |
| `ingress_interface_index`, `egress_interface_index` | `ingress.interface.index`, `egress.interface.index` |
| `next_hop_ip` | `next_hop.ip` |
| `additional_fields` | `AdditionalFields`, restored by `ndmDecodeFlows`, re-marshalled per value to JSON text so a `json.Number` never rounds through `float64` |

## `network_paths`

**Engine** `MergeTree` · **Partition** `toYYYYMM(timestamp)` ·
**Order by** `(tenant_id, namespace, test_config_id, timestamp)` ·
**TTL** `timestamp + INTERVAL 30 DAY`

One row per `pkg/networkpath/payload.NetworkPath`, decoded as-is (no
`MarshalJSON` asymmetry to work around, unlike netflow). Classified with
`check_runs`: a scheduled test run, human/config scale rather than
per-packet volume. Fed by `handleNetpath` → `ndmNetworkPathRow`.

| Column | Source |
|---|---|
| `agent_version`, `namespace`, `test_config_id`, `test_config_name`, `test_result_id`, `test_run_id`, `origin`, `test_run_type`, `test_config_source`, `source_product`, `collector_type`, `protocol` | same-named `NetworkPath` fields; the five enum-typed ones travel as their string values |
| `timestamp` | `timestamp` — **assumed milliseconds**, same reasoning as `netflow_flows.flush_timestamp`: no unit comment on the Go struct, and every other undocumented "timestamp" field in this intake turned out to be milliseconds, with seconds called out explicitly where it applies instead |
| `source_name`, `source_display_name`, `source_hostname`, `source_network_id`, `source_service`, `source_container_id`, `source_public_ip` | `source.*` |
| `source_via_subnet_alias`, `source_via_interface_hardware_addr` | `source.via.subnet.alias`, `source.via.interface.hardware_addr` — `Nullable(String)`, because `source.via` is `*payload.Via`: `nil` means the agent could not resolve a route, which must stay distinct from an empty string |
| `destination_hostname`, `destination_port`, `destination_service` | `destination.*` |
| `hop_count_avg`, `hop_count_min`, `hop_count_max` | `traceroute.hop_count.*` |
| `run_ids`, `run_source_ips`, `run_source_ports`, `run_destination_ips`, `run_destination_ports`, `run_destination_reverse_dns` | `traceroute.runs[]`, one array element per run (parallel arrays, `Array(...)`; `run_destination_reverse_dns` is `Array(Array(String))` since a destination can resolve to several names) |
| `hop_ttls`, `hop_ips`, `hop_reverse_dns`, `hop_rtts`, `hop_reachable` | `traceroute.runs[].hops[]`, nested one level deeper than the run arrays — outer index is the run, inner index is the hop (`Array(Array(...))`; `hop_reverse_dns` is `Array(Array(Array(String)))` since a hop's own reverse DNS is itself a list) |
| `e2e_rtts`, `e2e_packets_sent`, `e2e_packets_received`, `e2e_packet_loss_percentage`, `e2e_jitter`, `e2e_rtt_avg`, `e2e_rtt_min`, `e2e_rtt_max` | `e2e_probe.*` — ONE probe run over the whole path, not per traceroute run, hence flat arrays rather than nested like the hops above |
| `tags` | `tags` (`[]string`), through `tagsToMultiMap` |

A `net.IP` field that is `nil` (an unresolved hop, a run with no recorded
source) renders as `""`, never Go's `"<nil>"` — see `ndmIPString`.

## Sensitive columns

None of these eight tables store credentials or secrets. `ndm_device_configs.content`
is the closest thing to sensitive data here (a full device configuration,
which can carry SNMP community strings or local passwords depending on the
device) — no redaction is applied; treat the table's TTL and access
boundary as the only protection today.

## `dropped_by_decision`

- **`AdminStatus`/`OperStatus`/interface `Type` (`ndm_interfaces`) are kept as
  raw wire numbers, not decoded into a named `Enum8`.** The vendored
  `InterfaceMetadata` struct (`IfAdminStatus`, `IfOperStatus`) is not
  importable here (see the file header), so the exact value set could not be
  confirmed from source — RFC 2863 documents the IF-MIB numbers, but guessing
  a label wrong would be a worse fidelity loss than a bare integer. `Type`
  (ifType) is the IANA ifType registry, thousands of values, numeric for the
  same reason. *Default is to store, and both ARE stored — this is a
  presentation decision (number vs. name), not a data-loss one.*
- **Query-string parameters and multipart form fields are not applicable to
  any NDM route** — all five are plain JSON bodies, so there is nothing here
  analogous to DBM's `?api-version=` handling.
- **No "current" / `ReplacingMergeTree` companion table for `ndm_devices` or
  `ndm_interfaces`**, unlike `k8s_resources` → `k8s_resources_current`. The
  brief did not ask for one, and `(namespace, device_id, collect_timestamp)`
  already answers "what did we see, and when" for the history table alone;
  revisit if a "device list right now" query becomes common enough to justify
  the extra materialized view.
- **`ndm_metadata_objects` types nothing beyond `kind`/`device_id`/
  `interface_id`** — the other five shapes' own fields (a VPN tunnel's
  `local_outside_ip`, a diagnosis's `severity`/`message`, …) live only in the
  `object` raw-JSON column, queryable with `JSONExtract*` but not indexed as
  columns. Six shapes for a handful of rows each did not seem worth six more
  tables; revisit per-shape if one of them turns out to be high-volume or
  frequently queried on a field other than the two promoted ones.
