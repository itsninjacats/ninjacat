# Security tables

Six ClickHouse tables fed by the five security intake hosts
(`intake/router_security.go`, migration `schema/migrations/0009_security.sql`):

| host prefix | route | table(s) |
|---|---|---|
| `cws-intake.<site>` | `POST /api/v2/secdump` | `cws_activity_dumps`, `cws_dump_nodes` |
| `runtime-security-http-intake.logs.<site>` | `POST /api/v2/secruntime`, `POST /api/v2/secinfo` | `security_events` |
| `cspm-intake.<site>` | `POST /api/v2/compliance` | `security_events` |
| `sbom-intake.<site>` | `POST /api/v2/sbom` | `sbom_entities`, `sbom_components`, `sbom_vulnerabilities` |
| `sds-intake.<site>` | `POST /api/v2/sdsresult` | `raw_payloads` (see [Sensitive Data Scanner](#sensitive-data-scanner-sdsresult) below) |

All six routes keep their existing response contract: `202 Accepted`, body
`{}`, unconditionally, via a `defer` at the top of every handler. Decoding a
body incorrectly, or receiving one the code has no schema for, never changes
that — it changes only where the bytes end up.

## cws_activity_dumps

One row per `POST /api/v2/secdump` request. `MergeTree`, partitioned by
`toDate(received_at)`, `ORDER BY (tenant_id, received_at)`, 30-day TTL (the
metrics/`k8s_resources` retention class, not logs' 14 days — this is
investigative data, looked at well after arrival).

The request is `gzip(multipart/form-data)` with two independent parts, and a
row is built from whichever decoded — the columns from the missing one are
simply left at their zero value:

| column(s) | source |
|---|---|
| `header_host`, `header_service`, `header_source` | the "event" part's `host`/`service`/`ddsource` keys (generic JSON, `profile.ActivityDumpHeader` — no importable Go type) |
| `header_tags` | the "event" part's `ddtags`, through `tagsToMultiMap` |
| `dns_names` | the "event" part's `dns_names` key, kept as the raw JSON text exactly as received (not decoded further — the shape is undocumented) |
| `header_extra` | every other key on the "event" part, name → raw JSON text |
| `dump_host`, `dump_service`, `dump_source`, `dump_tags` | `SecDump.Host`/`Service`/`Source`/`Tags` (protobuf, `dumpsv1.SecDump`) |
| `agent_version` … `cgroup_manager` | `SecDump.Metadata`, flattened field for field |
| `start_raw`, `end_raw`, `size_raw` | `Metadata.Start`/`End`/`Size` — **raw kernel- or boot-relative counters, not wall-clock timestamps**; `NULL` means `Metadata` itself was absent, a present `0` is a real reading |
| `tree_node_count` | count of every node in `SecDump.Tree`, at every depth |
| `dump` | the "dump" part's raw protobuf bytes, exactly as received |

`dump` is what keeps this lossless: agent-payload v5.0.207 does not expose
field 7 (`mounts`) through any accessor, but `proto.Unmarshal` keeps unknown
fields on the wire, and storing the raw bytes is what keeps them reachable
until a newer `agent-payload` ships an accessor for it.

`dump_id` (`UUID`) is generated per request in Go, never taken from the
wire — it is what `cws_dump_nodes.dump_id` joins against.

Sensitive column: `dump` can carry anything the tree itself carries (see
below) inside its raw bytes, including AWS IMDS credential material.

## cws_dump_nodes

One row per `ProcessActivityNode`, flattened depth-first out of
`SecDump.Tree`. Same engine/partitioning/TTL class as `cws_activity_dumps`.
`ORDER BY (tenant_id, dump_id, node_path)`.

The tree is unbounded and recursive on the wire; rows are not, so:

- `node_path` (`Array(UInt32)`) is the index path from the root — e.g.
  `[2, 0, 1]` means `tree[2].Children[0].Children[1]` — and is on its own
  enough to reconstruct the whole tree.
- `parent_path` and `depth` save a reader from recomputing them from
  `node_path`.
- `node` is `protojson` of the node **with `Children` cleared**: the
  recursive part is exactly what `node_path`/`parent_path`/`depth` already
  reconstruct across rows, so keeping `Children` inline here would store
  every descendant subtree once per ancestor. Everything else on the node —
  process info, file info, matched rules, IMDS events (including any AWS
  `AccessKeyId` the agent captured), network flows — survives in this
  column even though only a few fields below get their own column.
- `pid`, `ppid`, `comm`, `container_id`, `file_path`, `args`, `image_tags`,
  `matched_rule_ids`, `generation_type`, and the four `*_count` columns are
  hot fields pulled out of `Process`/`MatchedRules`/`Files`/`DnsNames`/
  `Sockets`/`Syscalls` for filtering without parsing `node`.

**Sensitive column**: `node` can hold `AWSSecurityCredentials.AccessKeyId`
(nested under `ImdsEvents`) when an agent captured an IMDS credential fetch.
Stored, per the project's default of "store, and document" rather than
"silently drop" — but a reader building anything user-facing on top of this
table should be aware `node` is not a safe column to display verbatim.

## security_events

One row per logs-pipeline **envelope** from `secruntime`, `secinfo` or
`compliance` — `track` tells the three apart, all sharing one handler
(`handleSecLogsTrack`) and one table. `MergeTree`, partitioned by
`toDate(received_at)`, `ORDER BY (tenant_id, track, received_at)`, 30-day
TTL (shaped like `logs` — `ZSTD(3)` message, same engine — but with a longer
TTL than logs' 14 days, since security events are what an incident
investigates well after arrival).

Neither the envelope nor its inner `message` event has a published Go type,
so both are kept generically:

| column(s) | source |
|---|---|
| `seq_in_batch` | position in the request's JSON array — the only ordering signal the wire gives |
| `timestamp` | the envelope's `timestamp` key, parsed from either a millisecond epoch number or an RFC3339 string (both appear on the wire); `NULL` when absent or unparseable, never coerced to `received_at` |
| `hostname`, `service`, `ddsource`, `status`, `ddtags` | the envelope's documented keys; `ddtags` through `tagsToMultiMap` |
| `extra` | every other envelope key, name → raw JSON text |
| `message_raw` | the envelope's `message` key **exactly as it arrived** — the lossless copy |
| `message` | the unwrapped inner JSON object, re-serialized; empty when `message` was missing, not JSON, or not an object |
| `message_decoded` | `1` when `message` unwrapped to a JSON object, `0` otherwise — this is what distinguishes "the inner event legitimately decoded to `{}`" from "it didn't decode at all", which `message` alone cannot |
| `rule_id`, `policy_name` | `agent.rule_id` / `agent.policy_name` inside the decoded inner event, when present |
| `evt_name`, `evt_category` | `evt.name` / `evt.category` inside the decoded inner event, when present |
| `title`, `event_kind` | `title` / `kind` inside the decoded inner event, when present (best-effort — `compliance`'s inner shape is one of two undocumented variants, `compliance.CheckEvent` or `ResourceLog`, and the handler does not distinguish which arrived) |

An envelope whose `message` fails to decode still gets a row — it is never
dropped from the batch, only left with `message_decoded = 0`.

## sbom_entities

One row per `SBOMEntity` (a payload can carry several — one per image
layer, filesystem, etc.). `MergeTree`, partitioned by `toDate(received_at)`,
`ORDER BY (tenant_id, host, received_at)`, 30-day TTL.

| column(s) | source |
|---|---|
| `entity_id` | generated per entity in Go — **not** `SBOMEntity.Id`, which is the sender's own value and not guaranteed unique across payloads. `sbom_components`/`sbom_vulnerabilities` join on `entity_id`. |
| `payload_version`, `host`, `source`, `dd_env` | `SBOMPayload.Version`/`Host`/`Source`/`DdEnv` (`source`/`dd_env` are `Nullable` — both proto3 `optional`) |
| `type`, `id`, `generated_at`, `repo_tags`, `repo_digests`, `in_use`, `generation_duration_ms`, `dd_tags`, `heartbeat`, `hash`, `status`, `kernel_version`, `cpu_architecture` | the matching `SBOMEntity` fields; `generated_at` is `Nullable` (pointer-optional on the wire, never epoch zero when absent) |
| `error` | the oneof's `Error` arm — set only when generation failed |
| `bom` | `protojson` of the whole `cyclonedx_v1_4.Bom` (the oneof's `Cyclonedx` arm) — lossless; empty when `error` is set instead |
| `component_count`, `vulnerability_count` | counts of the rows this entity produced in the two tables below |

**Heartbeat entities are stored as ordinary rows.** `Heartbeat = true` means
"identical to what we sent last time" on the wire, but deduplicating them is
a read-time decision (`WHERE NOT heartbeat`, or collapsing by `hash`), not
something the write path decides for every reader.

Only `services`, `dependencies`, `compositions` and the BOM's own `metadata`
block have no dedicated columns anywhere in these three tables — they exist
only inside `bom`.

## sbom_components

Every `cyclonedx_v1_4.Component` in an entity's BOM, flattened recursively:
a component's own sub-components get rows too, linked by `parent_bom_ref`
and counted up in `depth`. `MergeTree`, `ORDER BY (tenant_id, entity_id,
bom_ref)`, 30-day TTL.

Column-to-field mapping is direct (`bom_ref`, `type`, `name`, `version`,
`purl`, `cpe`, `` `group` ``, `publisher`, `author`, `description`, `scope`),
plus:

- `licenses` — one entry per `LicenseChoice`: the license's `id` if set,
  else its `name`, else the SPDX `expression`.
- `hashes` — `Map(alg name, value)`, keyed by `HashAlg.String()`.
- `properties` — `Map(name, value)` from `Component.Properties`.
- `external_references`, `evidence` — `protojson` **arrays** of the
  corresponding repeated proto fields, kept as JSON text rather than further
  flattened (both are open-ended sub-structures nothing here queries on
  their own yet).

`version` being an **empty string is a valid, meaningful value** per the
CycloneDX spec ("RECOMMENDED to use an empty string") — never treated as
absent.

## sbom_vulnerabilities

Every `cyclonedx_v1_4.Vulnerability` in an entity's BOM — the CycloneDX
VEX-in-BOM shape, findings plus their analysis state. `MergeTree`,
`ORDER BY (tenant_id, entity_id, bom_ref)`, 30-day TTL.

| column(s) | source |
|---|---|
| `bom_ref`, `id`, `cwes`, `description`, `detail`, `recommendation` | direct fields |
| `source_name`, `source_url` | `Vulnerability.Source.Name`/`Url` |
| `ratings`, `advisories`, `affects` | `protojson` arrays of the corresponding repeated proto fields (`VulnerabilityRating`, `Advisory`, `VulnerabilityAffects` — the last including version ranges, which `affects_refs` below does not) |
| `created`, `published`, `updated` | `Nullable` — proto3 `Timestamp` pointers, `NULL` (never epoch zero) when the source did not supply one |
| `analysis_state`, `analysis_justification`, `analysis_response`, `analysis_detail` | `Vulnerability.Analysis`, when present |
| `affects_refs` | every affected component's `bom_ref`, pulled out of `affects` for the join `sbom_components` does not otherwise offer |
| `properties` | `Map(name, value)` from `Vulnerability.Properties` |

## Sensitive Data Scanner (`sdsresult`)

`sdsresult` has **no stored table of its own**. The schema
(`datadog/sds/sds_result.proto`) exists upstream, but the generated Go
package (`pkg/proto/pbgo/sds`) is not published in any `agent-payload`
release through v0.84.0-devel — there is nothing to `proto.Unmarshal` into.

Every request, decodable-looking or not, goes to `raw_payloads`
(`intake/raw.go`'s `storeRaw`) with `intake = "sds"`, `reason = "no_schema"`,
and `note` set to the top-level wire layout read by `secWireFields` (field
numbers, wire types, repeat counts — nested messages are not entered,
since without the schema they cannot be told apart from strings). The
response stays `202 {}` regardless.

Once `pkg/proto/pbgo/sds` is published, `HandleSDSResult` should switch to
`proto.Unmarshal` and gain real tables here — see the `TODO(ninjacat)` in
`intake/router_security.go`.

## dropped_by_decision

Nothing decoded by these handlers is discarded — every field enumerated in
the domain audit (`SecDump`, the logs-pipeline envelope/inner event, and the
whole `cyclonedx_v1_4` message tree) reaches either a typed column, a `Map`,
an `Array`, or a `String` holding raw/`protojson` bytes. The only two
narrowings are:

- **`cws_dump_nodes.node` drops `Children`.** Not lost — it is exactly what
  `node_path`/`parent_path`/`depth` reconstruct across the table's other
  rows, and keeping it would store every descendant subtree once per
  ancestor.
- **`sbom_components`/`sbom_vulnerabilities` keep `external_references`,
  `evidence`, `ratings`, `advisories` and `affects` as `protojson` text
  rather than further-flattened columns.** These are open-ended
  CycloneDX sub-structures nothing here queries on their own today;
  `sbom_entities.bom` also holds the entire BOM losslessly, so nothing here
  is a single point of loss.

`sdsresult` is not a decision to drop anything — there is no Go type to
decode into yet, hence `raw_payloads` rather than a typed table (see above).
