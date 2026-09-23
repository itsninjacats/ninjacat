# Kubernetes and container tables

What the `contlcycle-intake`, `contimage-intake`, `orchestrator` and
`kubeops-intake` hosts write, and where every field of every payload lands.

Six tables, all widened (or created) by
`schema/migrations/0015_k8s_fidelity.sql`. The rule the migration follows:
**if the bytes arrived, a column holds them.** Where a typed column would be
one of thirty nobody reads, the field goes into a structured column — parallel
arrays for repeated sub-messages, a Map for a protobuf map, a JSON String for
an open-ended shape — and never into a log line alone.

| Table | Engine | ORDER BY | Fed by |
|---|---|---|---|
| `container_events` | MergeTree | `(tenant_id, cluster_id, container_id, timestamp)` | `POST /api/v2/contlcycle` → `lcRows` |
| `container_images` | ReplacingMergeTree(`collected_at`) | `(tenant_id, image_key, host)` | `POST /api/v2/contimage` → `ciRows` |
| `k8s_resources` | MergeTree | `(tenant_id, cluster_id, kind, namespace, name, collected_at)` | `POST /api/v2/orch`, `/api/v1/orchestrator` → `orchResourceRows` |
| `k8s_resources_current` | ReplacingMergeTree(`collected_at`) | `(tenant_id, cluster_id, kind, namespace, name)` | the `k8s_resources_current_mv` materialized view, off `k8s_resources` |
| `k8s_cluster` | MergeTree | `(tenant_id, cluster_id, collected_at)` | `CollectorCluster` (type 46) → `orchClusterRows` |
| `k8s_manifests` | MergeTree | `(tenant_id, cluster_id, kind, uid, collected_at)` | `POST /api/v2/orchmanif` (types 80–82) → `orchManifestRows` |
| `k8s_actions` | MergeTree | `(tenant_id, cluster_id, timestamp)` | `POST /api/v2/kubeactions` → `kubeActionRow` |
| `ecs_tasks` | MergeTree | `(tenant_id, cluster_id, arn, collected_at)` | `CollectorECSTask` (type 200) → `orchECSTaskRows` **(new table)** |

TTL: 30 days for `k8s_resources`, `container_events`, `container_images` and
`ecs_tasks`; 7 days for `k8s_manifests` (whole documents); 90 days for
`k8s_cluster` and `k8s_actions`; none for `k8s_resources_current`, because a
live object must never age out of the "right now" view.

Every response on these four hosts stays **202 with an empty JSON object**,
sent from a `defer` at the top of the handler before the body is even read.
Nothing in this work changes a status code or a body.

---

## container_events — `/api/v2/contlcycle`

One row per `contlcycle.Event`. The envelope repeats on every row so a row is
self-sufficient at query time.

| Column(s) | Source |
|---|---|
| `host`, `cluster_id`, `object_kind` | `EventsPayload.Host` / `.ClusterId` / `.ObjectKind` |
| `payload_version` | `EventsPayload.Version` |
| `event_type` | `Event.EventType` |
| `event_variant` | which arm of the `Event` oneof produced the row |
| `container_id`, `container_name`, `source` | `ContainerEvent` |
| `container_name_present` | 1 when `ContainerEvent.ContainerName` was set at all |
| `pod_uid` / `task_arn` | `PodEvent.PodUID` / `TaskEvent.TaskARN` |
| `exit_code`, `created_at`, `exited_at` | the three proto3 optionals, NULL when unset |
| `owner_type`, `owner_uid` | `ContainerEvent.Owner` |
| `old_state`, `new_state` | the flattened strings the log line prints |
| `container_kind`, `precision`, `missed_intermediate` | `ContainerStateTransition` / `PodStateTransition` enums |
| `old_state_kind`, `old_reason`, `old_exit_code`, `old_signal` (and `new_*`) | `ContainerStateValue`, structurally |
| `pod_status_field` | `PodStateTransition.Field` |
| `old_state_variant`, `old_phase`, `old_condition_*` (and `new_*`) | the `PodStatusValue` oneof, per arm |

**Semantics worth knowing.**

- `event_variant` is `container` / `pod` / `task` / `unknown`. Without it, a
  row from a nil oneof or a variant newer than our schema is indistinguishable
  from a typed event whose fields all happened to be empty.
- `container_name_present` exists because `container_name` **cannot become
  Nullable in place** — that rewrites the column for every stored row, and a
  reader expecting `String` would start getting NULLs. Presence travels beside
  the value instead.
- `old_state` / `new_state` are kept alongside the structured columns
  deliberately: they come from the same wire message through the same
  formatter the log uses, so the log and the row cannot disagree, and a
  dashboard that wants a label does not have to reassemble one.
- Every `Nullable` here encodes absent-vs-zero: exit 0, exit nonzero and "the
  runtime never said" are three different states.

## container_images — `/api/v2/contimage`

One row per image sighting; the table replaces on `(tenant_id, image_key, host)`.

| Column(s) | Source |
|---|---|
| `image_key`, `identity_source` | the registry digest, or the image id when there is none |
| `payload_version`, `source` | `ContainerImagePayload.Version` / `.Source` (Nullable: "the sender did not say") |
| `size_bytes`, `size_negative` | `ContainerImage.Size`, clamped at 0 with the clamp recorded |
| `layer_bytes`, `layer_size_negative` | the sum of `Layers[].Size`, clamped at 0, with the flag raised by ANY negative layer |
| `layer_media_types`, `layer_digests`, `layer_sizes`, `layer_urls` | `ContainerImage.Layers[]`, parallel arrays in wire order |
| `layer_history_created`, `_created_by`, `_author`, `_comment`, `_empty_layer` | `Layers[].History`, the build record |
| `os_name`, `os_version`, `architecture` | `ContainerImage.Os` |
| `built_at`, `published_at` | Nullable; a missing date never becomes 1970 |

**Semantics worth knowing.**

- The layer arrays are **index-aligned and never sorted**: an image IS its
  ordered layer stack. `layer_urls` is `Array(Array(String))` and the intake
  fills an empty inner slice for a layer with no URLs, because the driver
  rejects a nil inside an array and takes the whole batch with it.
- `layer_history_created_by` is the Dockerfile instruction. It was decoded and
  thrown away before this migration, and it arrives free on every inventory pass.
- `size_negative` is a guard, not a metric: casting a negative `int64` to
  `UInt64` produces ~18 exabytes, which then poisons every `SUM` over the table.
- `layer_size_negative` is the same guard one level down. `layer_bytes` is the
  sum of the layers' signed sizes cast to `UInt64`, so a single negative layer
  either wraps the total or quietly shrinks it; the sum is clamped at 0 and the
  flag rises as soon as any one layer reported a negative size, because a bad
  layer inside a still-positive sum is just as wrong and far quieter.
  `layer_sizes` keeps the raw signed values, so the original claim stays readable.
- An image with neither digest nor id is skipped and counted through the
  `SelfImagesSkipped` self-metric — unchanged by this work.

## k8s_resources (+ k8s_resources_current)

One generic table for 27 Kubernetes kinds. Three layers:

1. **columns every kind has** — identity, the metadata dates, finalizers,
   every owner reference, conditions, resource requirements, metrics;
2. **`counts Map(String, Int64)`** for the numbers that differ per kind;
3. **`object` and `envelope`**, the whole decoded message and the frame around
   it as JSON, which is what makes the twenty kinds with no typed case
   lossless without three hundred columns.

| Column(s) | Source |
|---|---|
| `org_id`, `subscription_id`, `header_timestamp`, `encoding` | the 16-byte V3 frame header |
| `creation_timestamp`, `deletion_timestamp`, `deletion_grace_period_seconds`, `finalizers` | `Metadata` |
| `owner_kinds`, `owner_names`, `owner_uids` | ALL of `Metadata.OwnerReferences` (`owner_kind`/`owner_name` stay as the first one) |
| `condition_*` arrays, `condition_message` | each kind's own `Conditions` message, plus the agent's one-line summary |
| `resource_requirements` | `[]*ResourceRequirements` as JSON |
| `metrics` | `ResourceMetrics.MetricValues` |
| `object` | the whole element, `encoding/json` |
| `envelope` | the frame envelope with its object list removed |
| `pod_ip`, `nominated_node_name`, `qos_class`, `priority_class`, `start_time`, `scheduled_time`, `host_name` | Pod (41) |
| `container_*` arrays | Pod container statuses, regular first then init (`container_is_init`) |
| `pod_cidr(s)`, `unschedulable`, `taints`, `provider_id`, `node_roles`, `capacity`, `allocatable`, `node_addresses`, the version columns | Node (45) |
| `service_type`, `cluster_ip`, `service_ports` | Service (44) |
| `rbac_rules`, `rbac_subjects`, `rbac_role_ref` | Role / ClusterRole / RoleBinding / ClusterRoleBinding (54–57) |

**`counts` keys, per kind** (stable, part of the storage contract):

- **Pod**: `restarts`, `ready_containers`, `total_containers`, `init_restarts`,
  `ready_init_containers`, `total_init_containers`, `pod_restart_count`
  (the agent's own aggregate, kept beside the recomputed one).
- **Deployment**: `ready`, `desired`, `updated`, `available`, `unavailable`, `replicas`.
- **ReplicaSet**: `ready`, `desired`, `available`, `replicas`, `fully_labeled`.
- **DaemonSet**: `ready`, `desired`, `current`, `updated`, `available`,
  `misscheduled`, `unavailable`.
- **StatefulSet**: `ready`, `desired`, `current`, `updated`, `replicas`, `partition`.
- **Node**: `taints`, `images`.
- **Service**: `ports`.
- **Role**: `rules`. **ClusterRole**: `rules`, `aggregation_rules`.
  **RoleBinding** / **ClusterRoleBinding**: `subjects`.
- **Job**: `active`, `succeeded`, `failed`, `parallelism`, `completions`,
  `backoff_limit`, `active_deadline_seconds`.
- **CronJob**: `active_jobs`, `suspend` (0/1), `successful_jobs_history_limit`,
  `failed_jobs_history_limit`, `starting_deadline_seconds`.
- **HorizontalPodAutoscaler**: `min_replicas`, `max_replicas`,
  `current_replicas`, `desired_replicas`.
- **PodDisruptionBudget**: `disruptions_allowed`, `current_healthy`,
  `desired_healthy`, `expected_pods`, `disrupted_pods`.

A kind with no entry above is **not** a gap: its identity, metadata,
conditions and metrics are filled generically and its whole message is in
`object`. A `counts` entry is a number somebody charts.

**Semantics worth knowing.**

- `header_timestamp` is `Nullable(Int64)`, **not a date**. The V3 frame
  documents no unit for it and no sender we have seen sets it, so converting
  would be picking a scale at random and writing the guess down as fact. The
  day a sender starts filling it, one `SELECT` says whether it counts seconds,
  milliseconds or nanoseconds.
- Conditions are read **by field name**, because each kind declares its own
  condition message: `NodeCondition` has no `lastUpdateTime`, `PodCondition`
  has `lastProbeTime`, and `HorizontalPodAutoscalerCondition` /`VPACondition`
  call the first two fields `ConditionType` / `ConditionStatus`. Both
  conventions are read; the time arrays are `Array(Nullable(...))` so a
  condition that never probed keeps its slot without a fabricated date.
- Conditions and resource requirements live on the object for some kinds and
  inside `Status` / `Spec` for others; both places are tried, and **presence in
  the outer place does not stop the search**: a declared-but-empty slice looks
  exactly like a present one to reflection. A `VerticalPodAutoscaler` declares
  both — a top-level `Conditions` of `VerticalPodAutoscalerCondition` and
  `Status.Conditions` of `VPACondition`, the latter being the list the upstream
  Kubernetes VPA API defines and therefore the one a real autoscaler fills — so
  both are read and concatenated, the object's first. Reading only the first
  field that existed left `condition_*` empty for every VPA in the fleet.
- `object` is `encoding/json`, not protojson: agent-payload's `process`
  package is gogo-generated, so its messages are not protoreflect messages.
  `encoding/json` reads the same struct tags and keeps int64 ids exact.
- `envelope` repeats per row and costs almost nothing — every row of a pass
  carries identical bytes and ZSTD collapses them inside the part. It is where
  `CollectorPod.Host` / `.Info` / `.IsTerminated` and
  `CollectorNode.HostAliasMapping` live.
- **`k8s_resources_current` is fed only by the materialized view.** Nothing
  writes to it from Go. A column added to `k8s_resources` needs the same
  column on `k8s_resources_current` AND a line in the view's SELECT, or the
  view stops firing and the "right now" table silently freezes.
  `0015` recreates the view with `DROP VIEW IF EXISTS` + `CREATE MATERIALIZED
  VIEW IF NOT EXISTS` rather than `MODIFY QUERY`, because only that pair is
  idempotent in both directions — the runner has no transactions and re-runs a
  half-applied file as-is. Verified on ClickHouse 26.8 by
  `TestK8sFidelityRoundTrip`, which SELECTs from `k8s_resources_current FINAL`.
  Read that table with `FINAL` or `argMax`.

## k8s_cluster — `CollectorCluster` (type 46)

One row per cluster per collection pass. Beyond the nine numbers it always
had: the frame header, `group_id`/`group_size`, `agent_version`, the cluster's
`resource_version` and `creation_timestamp`, its `metrics`, its
`extended_resources_capacity`/`_allocatable` (GPUs), and `nodes_*` — the
per-node breakdown behind `node_count`, as parallel arrays in wire order.

`tags` and `cluster_tags` are **two columns on purpose**: the frame's tags and
the `Cluster` object's own are different fields on the wire, and merging them
would erase which producer attached what.

`nodes_allocatable` / `nodes_capacity` are `Array(Map(String, String))`: this
message sends quantities as their Kubernetes strings (`"16Gi"`, `"4"`), unlike
`NodeStatus`, which sends `int64`. Converting would guess at units. A node that
reported none gets an empty map, never a nil — the driver rejects a nil inside
an array.

## k8s_manifests — `/api/v2/orchmanif` (types 80–82)

`content` is the object's own YAML or JSON, whole. New: the frame header,
`group_id`/`group_size`, `host_name`, `agent_version`, `origin_collector`,
`node_name`, `type`, `version`, `extra_attributes`, `content_is_utf8`, and
tags — which manifest rows carried **none of** before.

- `tags` merges all three sources (frame envelope, CRD/CR wrapper, the
  manifest's own), because a query asking for `env:prod` does not care where
  the tag was attached.
- `tag_sources` keeps the provenance: `envelope` / `wrapper` / `manifest` →
  that source's raw tag strings.
- `content_is_utf8` is 1 when `content` is valid UTF-8. A ClickHouse `String`
  is a byte string, so nothing is corrupted either way — but a reader deserves
  to know before parsing.
- `envelope` is the `CollectorManifest` as JSON **minus its manifest list** (the
  list is the rows). It exists for `CollectorManifest.SystemInfo` — the
  reporting agent's uuid, OS, CPU list and total memory — which no column holds
  and which is the same class of field `k8s_resources.envelope` preserves for
  the object collectors. Whatever the envelope grows next lands here too.
- `manifest_host` is `Manifest.Host`, the machine this one object was observed
  on, in full (id, orgId, allTags, numCpus, totalMemory). JSON rather than five
  columns: the shape belongs to the host inventory, not to a manifest. Empty,
  never `null`, when the sender omitted it.

## k8s_actions — `/api/v2/kubeactions`

Every declared field of `ActionResultEvent`, plus three columns that close the
two documented "by design" drops:

- `payloads Map(String, String)` — whatever the executor attached. Byte
  strings, so a non-text attachment survives unchanged.
- `extra String` — the undeclared JSON keys **with their values**.
  `extra_keys` (names only) stays, because filtering on it needs no parsing.
- `timestamp_raw` — the wire's timestamp string exactly as it arrived.
  `timestamp` still falls back to arrival time when it is empty or
  unparseable; that fallback used to erase its own evidence.

**Sensitive columns.** `payloads` and `extra` carry whatever the cluster agent
attached to a remote action — command output, in practice. They inherit the
table's 90-day TTL. Nothing here is a credential the intake adds; the request
header allowlist in `intake/raw.go` still applies to `raw_payloads`.

## ecs_tasks — `CollectorECSTask` (type 200), new table

ECS tasks arrive on the orchestrator intake and used to be decoded and
discarded with a comment: a task has no Kubernetes `Metadata`, so nothing maps
onto `k8s_resources`' identity columns. True — and the answer is a table of
its own, keyed on the task ARN, rather than a lossy squeeze into someone
else's sort key.

Every envelope and task field is typed. Two shapes are worth naming:

- `containers` is JSON plus a `container_count` column. An `ECSContainer`
  nests ports, networks, volumes, health and log options, each a repeated
  sub-message, so parallel arrays would have to be arrays of arrays of structs.
- **Four tag sets, four columns**: `envelope_tags`, `tags`, `ecs_tags`,
  `container_instance_tags`. ECS keeps them apart and so do we — the container
  instance's tags belong to the machine, not to the workload.
- `envelope` is the `CollectorECSTask` as JSON minus its task list. The
  envelope carries a `Host` and an `Info` (`SystemInfo`) of its **own** — the
  machine and agent that ran the collection pass, distinct from the task's host
  — and no typed column holds either.
- `task_host` is `ECSTask.Host` whole. `task_host_name` is only its `Name`, and
  a name alone cannot tell two identically named container instances in
  different accounts apart.

---

## Undecodable input

Nothing is dropped silently. `a.storeRaw` takes the bytes to `raw_payloads`:

| Situation | intake | reason |
|---|---|---|
| `contlcycle` body fails `proto.Unmarshal` | `contlcycle` | `decode_error` |
| `contlcycle` decodes to zero events | `contlcycle` | `unexpected_shape` |
| `contimage` body fails `proto.Unmarshal` | `contimage` | `decode_error` |
| `contimage` decodes to zero images | `contimage` | `unexpected_shape` |
| orchestrator frame fails `process.DecodeMessage` | `orchestrator` | `decode_error` |
| a non-orchestrator message type on this intake | `orchestrator` | `unexpected_shape` |
| `kubeactions` body is not a JSON array | `kubeactions` | `decode_error` |
| one `kubeactions` batch element fails to decode | `kubeactions` | `decode_error` |

Protobuf fails **open** — unknown fields are skipped, so a payload meant for
another endpoint decodes "successfully" into an empty struct. An empty decode
is the only signal the wire gives, which is why it is treated as a shape worth
keeping rather than a no-op.

Decode success paths never call `storeRaw`: it is a fallback, not a mirror of
the traffic.

---

## dropped_by_decision

Everything below is decoded and deliberately not given a column of its own.
Nothing in this list is lost — each item is either reachable in a JSON column
on the same row, or is a duplicate of something already stored.

| Field | Where it still is | Why no column |
|---|---|---|
| Per-object `Yaml []byte` (deprecated, on almost every kind) | `k8s_resources.object` | Deprecated upstream and superseded by `/api/v2/orchmanif`, which stores the same document in `k8s_manifests.content` with its own TTL. A column would be a second copy of a megabyte-scale field. |
| `NodeStatus.Images` (the node-local image cache) | `k8s_resources.object`, counted in `counts['images']` | The `contimage` intake is the authoritative inventory and has a table with a digest key. A per-node duplicate would not join to it any better than the JSON does. |
| Every `Spec`/`Status` leaf of the twenty kinds with no typed case (StorageClass parameters, NetworkPolicy rules, PVC access modes, Ingress backends, LimitRange limits, VPA targets, ServiceAccount secrets, …) | `k8s_resources.object`, whole | One generic table serves 27 kinds; a typed column per leaf would be several hundred columns, all NULL for 26 kinds out of 27. The JSON is lossless and queryable with ClickHouse's JSON functions, and any field that turns out to be asked for often can be promoted to a column later by a migration plus a line in the intake. |
| `CollectorPod.Host`, `.Info`, `.IsTerminated`; `CollectorNode.HostAliasMapping`; per-frame extras of other collectors | `k8s_resources.envelope` — and, for the two collectors decoded into their concrete types, `k8s_manifests.envelope` and `ecs_tasks.envelope` | Frame-level, identical for every row of a pass. One JSON column per table carries all of them instead of a column per kind-specific envelope field. Each of the three tables fed by a collector envelope has one, so no envelope field can go missing because its collector took a different code path — that is how `CollectorManifest.SystemInfo` and `CollectorECSTask.Host`/`.Info` were lost. |
| `MessageHeader.Version` | — | Redundant: the version determines the header layout, and `encoding`, `org_id`, `subscription_id` and `header_timestamp` (present only in V3) already record what the layout yielded. |
| `MessageHeader.Type` | implied by `kind` and by which writer received the row | The Go type the switch resolved is a strictly finer statement of the same fact. |
| `AgentVersion.Commit` / `.Meta` | — | Pre-existing, documented decision in `agentVersionString`: a version column wants `"7.55.1"`, not a hash. |

`ECSTask.Host` beyond `.Name` was on this list and is not any more: `task_host`
now holds the whole message, because a name cannot tell two identically named
container instances in different AWS accounts apart.

Two further things are explicitly **not** on this list, because they used to
be and the fidelity rule does not accept them: `k8s_actions.payloads` (the
attachments) and the values of undeclared `kubeactions` keys. Both are small,
both arrive once, and both are exactly what somebody debugging a failed remote
action wants.
