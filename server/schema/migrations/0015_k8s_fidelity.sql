-- Kubernetes / container fidelity: close the gaps between what the
-- orchestrator and container intakes DECODE and what we actually keep.
--
-- The audit behind this migration (docs/tables/kubernetes.md) found the same
-- shape of loss in four places: the handler decodes a complete object, copies
-- half a dozen identity fields into a row, and the rest — conditions, resource
-- requirements, taints, RBAC rules, layer history, autoscaler targets — is
-- reachable at the call site and then garbage collected. "Nie gub tagow, nie
-- gub danych": if the bytes arrived, a column holds them.
--
-- Three mechanisms, in order of preference:
--
--   1. a typed column, when the field is one people filter or chart on;
--   2. parallel ARRAYS, when the field is a repeated sub-message and the
--      columns of one element must stay aligned with the others (container
--      statuses, conditions, image layers, cluster nodes). Parallel arrays
--      rather than Nested: ClickHouse stores Nested AS parallel arrays anyway,
--      and the flat form is what arrayZip/ARRAY JOIN read without ceremony;
--   3. a String column holding that sub-object's JSON, when the shape is
--      genuinely open-ended (RBAC rules, service ports, taints, node
--      addresses) or when there are 27 kinds and only one of them has the
--      field (k8s_resources.object).
--
-- k8s_resources.object deserves its own paragraph, because it is what makes
-- the twenty kinds with no typed coverage lossless in one column instead of
-- three hundred. It is the WHOLE collector object, serialised with
-- encoding/json. Not protojson: agent-payload's process package is generated
-- by protoc-gen-gogo, so its messages are not protoreflect messages and
-- protojson cannot see them; gogo's own jsonpb would promote an indirect
-- dependency to a direct one for a debugging column. encoding/json reads the
-- same generated struct tags, keeps int64 ids as exact digits, and renders
-- []byte (the deprecated per-object Yaml) as base64 — lossless either way.
-- ZSTD(3) because nothing filters on it: it is read whole, by a person, once.
--
-- No CREATE DATABASE and no database qualifier, same as 0001 and 0002: the
-- runner applies every statement over a connection already opened against the
-- target database, and qualifying would break running against a scratch one.
--
-- Every statement is IF NOT EXISTS / ADD COLUMN IF NOT EXISTS, because DDL is
-- not transactional here: a failure halfway through this file leaves the
-- earlier statements applied and the version unrecorded, so the re-run after
-- the fix must skip what already landed. New columns are APPENDED, never
-- inserted between existing ones — Row.AppendTo passes positional arguments
-- against the INSERT column list, and reordering a live table would silently
-- shift every value one column to the left.

-- ---------------------------------------------------------------------------
-- container_events: /api/v2/contlcycle
--
-- The transition sub-message was being flattened into two opaque strings
-- (old_state / new_state) by the same formatter that writes the log line.
-- Good for a human, useless for "every OOM kill with signal 9": the structured
-- reason / exit code / signal, and the three enums around them, are gone. They
-- come back as columns here, and the flattened strings stay — they are what
-- the log prints, and a query that just wants a label should not have to
-- reassemble one.
-- ---------------------------------------------------------------------------
ALTER TABLE container_events
    -- EventsPayload.Version: the schema version of the payload itself. Worth a
    -- column the day a v2 changes the meaning of a field.
    ADD COLUMN IF NOT EXISTS payload_version LowCardinality(String),

    -- Which arm of the Event oneof produced this row: container / pod / task,
    -- or 'unknown' for a nil oneof or a variant newer than our schema. Without
    -- it an unknown-variant row is indistinguishable from a typed event whose
    -- fields all happened to be empty.
    ADD COLUMN IF NOT EXISTS event_variant LowCardinality(String),

    -- ContainerStateTransition.ContainerKind: regular container, init
    -- container, ephemeral container. A restart storm in init containers is a
    -- different incident from one in app containers.
    ADD COLUMN IF NOT EXISTS container_kind LowCardinality(String),

    -- How exact the transition timestamp is, and whether the agent knows it
    -- missed states in between. Both are the agent telling us how much to
    -- trust the row; dropping them made every transition look equally solid.
    ADD COLUMN IF NOT EXISTS precision LowCardinality(String),
    ADD COLUMN IF NOT EXISTS missed_intermediate LowCardinality(String),

    -- ContainerStateValue, structured. Kind is always present (an enum, so ''
    -- only when there is no state at all); reason, exit code and signal are
    -- proto3 optionals, and the three-way distinction they carry is the whole
    -- point — exit 0, exit nonzero, and "the runtime never said".
    ADD COLUMN IF NOT EXISTS old_state_kind LowCardinality(String),
    ADD COLUMN IF NOT EXISTS old_reason Nullable(String),
    ADD COLUMN IF NOT EXISTS old_exit_code Nullable(Int32),
    ADD COLUMN IF NOT EXISTS old_signal Nullable(Int32),
    ADD COLUMN IF NOT EXISTS new_state_kind LowCardinality(String),
    ADD COLUMN IF NOT EXISTS new_reason Nullable(String),
    ADD COLUMN IF NOT EXISTS new_exit_code Nullable(Int32),
    ADD COLUMN IF NOT EXISTS new_signal Nullable(Int32),

    -- PodStateTransition.Field: WHICH pod status field moved (phase, or one of
    -- the conditions). The pod status value is itself a oneof, so the variant
    -- is recorded too, and both arms get their own columns: collapsing them
    -- into one string lost the Message entirely (lcPodStatus never read it).
    ADD COLUMN IF NOT EXISTS pod_status_field LowCardinality(String),
    ADD COLUMN IF NOT EXISTS old_state_variant LowCardinality(String),
    ADD COLUMN IF NOT EXISTS old_phase Nullable(String),
    ADD COLUMN IF NOT EXISTS old_condition_type Nullable(String),
    ADD COLUMN IF NOT EXISTS old_condition_status Nullable(String),
    ADD COLUMN IF NOT EXISTS old_condition_reason Nullable(String),
    ADD COLUMN IF NOT EXISTS old_condition_message Nullable(String),
    ADD COLUMN IF NOT EXISTS new_state_variant LowCardinality(String),
    ADD COLUMN IF NOT EXISTS new_phase Nullable(String),
    ADD COLUMN IF NOT EXISTS new_condition_type Nullable(String),
    ADD COLUMN IF NOT EXISTS new_condition_status Nullable(String),
    ADD COLUMN IF NOT EXISTS new_condition_reason Nullable(String),
    ADD COLUMN IF NOT EXISTS new_condition_message Nullable(String),

    -- ContainerEvent.ContainerName is a proto3 optional, so "never set" and
    -- "set to empty" are different wire states that GetContainerName()
    -- collapses. The existing container_name column CANNOT become Nullable in
    -- place — that rewrites the column for every row already stored, and a
    -- reader that expects String would start getting NULLs — so presence gets
    -- a flag of its own instead. 1 = the sender set the field.
    ADD COLUMN IF NOT EXISTS container_name_present UInt8;

-- ---------------------------------------------------------------------------
-- container_images: /api/v2/contimage
--
-- The layer list was reduced to two numbers (count and total bytes) and the
-- build history thrown away whole. History is the Dockerfile: the instruction
-- that created each layer, who authored it, when. It is also the only place
-- the image says anything about how it was built, and it arrives for free on
-- every inventory pass.
--
-- Layers are ARRAYS in wire order, index-aligned: layer_digests[3] belongs
-- with layer_sizes[3] and layer_history_created_by[3]. Order is part of the
-- data here — a container image IS its ordered layer stack.
-- ---------------------------------------------------------------------------
ALTER TABLE container_images
    ADD COLUMN IF NOT EXISTS payload_version LowCardinality(String),

    -- ContainerImagePayload.Source, a proto3 optional: which pipeline produced
    -- the inventory ('agent', 'other'). Nullable because "the sender did not
    -- say" is a real answer, distinct from an empty string on the wire.
    ADD COLUMN IF NOT EXISTS source Nullable(String),

    ADD COLUMN IF NOT EXISTS layer_media_types Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS layer_digests Array(String),
    ADD COLUMN IF NOT EXISTS layer_sizes Array(Int64),
    -- Array of arrays: a layer may be fetchable from several URLs (the OCI
    -- foreign-layer case). Rare, but dropping it would mean the row could not
    -- reproduce the manifest it came from.
    ADD COLUMN IF NOT EXISTS layer_urls Array(Array(String)),
    ADD COLUMN IF NOT EXISTS layer_history_created Array(Nullable(DateTime64(3, 'UTC'))),
    ADD COLUMN IF NOT EXISTS layer_history_created_by Array(String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS layer_history_author Array(String),
    ADD COLUMN IF NOT EXISTS layer_history_comment Array(String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS layer_history_empty_layer Array(UInt8),

    -- size_bytes is UInt64 and the wire field is a signed int64. A negative
    -- size is impossible from a healthy agent and wraps to ~18 exabytes if
    -- cast blindly, which then poisons every SUM over the table. The intake
    -- clamps to 0 and raises this flag, so the lie is visible rather than
    -- silently averaged in. Same guard orchVersionSpread already applies to
    -- the cluster version counts.
    ADD COLUMN IF NOT EXISTS size_negative UInt8;

-- ---------------------------------------------------------------------------
-- k8s_resources (+ k8s_resources_current, which must stay column-for-column
-- identical because a materialized view feeds it)
--
-- This is the big one: one generic table serves 27 Kubernetes kinds, and only
-- 7 of them had any coverage beyond identity. Everything below is either a
-- field every kind carries (Metadata timestamps, finalizers, owners,
-- conditions, resource requirements, metrics) or a typed extra for the kinds
-- people actually ask about. The rest — StorageClass parameters, NetworkPolicy
-- rules, PVC access modes, Ingress backends — is in `object`, whole.
--
-- The two tables are altered with the SAME clauses in the SAME order. The MV
-- below is recreated against the full list; if the two ever diverge, the MV
-- fails at insert time and k8s_resources_current silently stops updating.
-- ---------------------------------------------------------------------------
ALTER TABLE k8s_resources
    -- The 16-byte frame header, previously decoded and discarded wholesale.
    -- The agent leaves org_id / subscription_id at 0 and does not set the
    -- timestamp today, which is exactly why the columns are worth having: the
    -- day a sender starts filling them, the data is already being kept rather
    -- than needing a migration first.
    --
    -- header_timestamp is the RAW int64 out of the V3 header, not a date: the
    -- frame format documents no unit for it and no sender we have seen fills
    -- it, so converting would be picking a scale at random and writing the
    -- guess down as fact. Nullable because 0 means "not provided" — and that
    -- must not become either 1970 or a real-looking instant. The day a sender
    -- starts setting it, one SELECT says whether it counts seconds,
    -- milliseconds or nanoseconds, and a later migration can add the typed
    -- column beside this one.
    ADD COLUMN IF NOT EXISTS org_id Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS subscription_id UInt8,
    ADD COLUMN IF NOT EXISTS header_timestamp Nullable(Int64),
    ADD COLUMN IF NOT EXISTS encoding LowCardinality(String),

    -- Metadata, in full. creation_timestamp is the one that hurt most: without
    -- it "how old is this pod" was unanswerable for every object of every
    -- kind, because collected_at is only when WE saw it. Second precision,
    -- because that is what the wire carries (int64 Unix seconds).
    ADD COLUMN IF NOT EXISTS creation_timestamp Nullable(DateTime('UTC')),
    ADD COLUMN IF NOT EXISTS deletion_timestamp Nullable(DateTime('UTC')),
    ADD COLUMN IF NOT EXISTS deletion_grace_period_seconds Nullable(Int64),
    ADD COLUMN IF NOT EXISTS finalizers Array(String),

    -- ALL owner references, not just the first. owner_kind / owner_name stay
    -- as they are (the Pod -> ReplicaSet -> Deployment walk is the query that
    -- gets asked), but a second owner is legal and the uid was never kept at
    -- all — and the uid is what a join actually needs.
    ADD COLUMN IF NOT EXISTS owner_kinds Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS owner_names Array(String),
    ADD COLUMN IF NOT EXISTS owner_uids Array(String),

    -- Conditions, parallel arrays, in wire order. Every workload kind has
    -- them and none of them reached a column: "why is this Deployment not
    -- Available" lives in Reason and Message and nowhere else. The three time
    -- fields are Array(Nullable(...)) because a condition that never probed or
    -- never updated leaves them at 0, and 0 is absence, not 1970.
    ADD COLUMN IF NOT EXISTS condition_types Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS condition_statuses Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS condition_reasons Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS condition_messages Array(String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS condition_last_transition Array(Nullable(DateTime('UTC'))),
    ADD COLUMN IF NOT EXISTS condition_last_update Array(Nullable(DateTime('UTC'))),
    ADD COLUMN IF NOT EXISTS condition_last_probe Array(Nullable(DateTime('UTC'))),
    -- The agent's own one-line summary, separate from the condition list.
    ADD COLUMN IF NOT EXISTS condition_message String CODEC(ZSTD(3)),

    -- CPU and memory requests/limits, per container, JSON. Not a Map: one
    -- object carries several ResourceRequirements (one per container, plus
    -- init containers), each with two maps and a type — a shape that flattens
    -- into a map only by losing which container it belonged to.
    ADD COLUMN IF NOT EXISTS resource_requirements String CODEC(ZSTD(3)),

    -- ResourceMetrics.MetricValues: the agent's own computed numbers for the
    -- object. A plain Map, NOT the tag multiset shape — protobuf map keys are
    -- unique by definition, so the array side would cost lookups to solve a
    -- problem that cannot occur.
    ADD COLUMN IF NOT EXISTS metrics Map(String, Float64),

    -- The whole decoded object as JSON. See the header: this is what makes the
    -- twenty kinds with no typed coverage lossless.
    ADD COLUMN IF NOT EXISTS object String CODEC(ZSTD(3)),

    -- Pod (type 41). The scheduling and QoS facts a pod row is asked for.
    ADD COLUMN IF NOT EXISTS pod_ip String,
    ADD COLUMN IF NOT EXISTS nominated_node_name LowCardinality(String),
    ADD COLUMN IF NOT EXISTS qos_class LowCardinality(String),
    ADD COLUMN IF NOT EXISTS priority_class LowCardinality(String),
    ADD COLUMN IF NOT EXISTS start_time Nullable(DateTime('UTC')),
    ADD COLUMN IF NOT EXISTS scheduled_time Nullable(DateTime('UTC')),
    -- Object.Host.Name — the agent host that reported this object, which is
    -- not the same thing as node_name for cluster-scoped kinds.
    ADD COLUMN IF NOT EXISTS host_name LowCardinality(String),

    -- Container statuses, parallel arrays, regular containers first and init
    -- containers appended (container_is_init says which). Previously only the
    -- SUM of ready and restarts survived, so "which container in this
    -- three-container pod is crashlooping, and with what message" could not be
    -- answered at all — and init containers were not counted anywhere.
    ADD COLUMN IF NOT EXISTS container_names Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS container_ids Array(String),
    ADD COLUMN IF NOT EXISTS container_ready Array(UInt8),
    ADD COLUMN IF NOT EXISTS container_restarts Array(Int32),
    ADD COLUMN IF NOT EXISTS container_states Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS container_messages Array(String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS container_images Array(String),
    ADD COLUMN IF NOT EXISTS container_image_ids Array(String),
    ADD COLUMN IF NOT EXISTS container_is_init Array(UInt8),

    -- Node (type 45). unschedulable is the cordon flag — it was logged and
    -- dropped, and it is the first thing anyone asks about a sick node.
    ADD COLUMN IF NOT EXISTS pod_cidr String,
    ADD COLUMN IF NOT EXISTS pod_cidrs Array(String),
    ADD COLUMN IF NOT EXISTS unschedulable UInt8,
    ADD COLUMN IF NOT EXISTS taints String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS provider_id String,
    ADD COLUMN IF NOT EXISTS node_roles Array(LowCardinality(String)),
    -- Per-node capacity, in the wire's own units (millicores, bytes). Only the
    -- cluster-wide SUM was stored before, which cannot answer "which node is
    -- full".
    ADD COLUMN IF NOT EXISTS capacity Map(String, Int64),
    ADD COLUMN IF NOT EXISTS allocatable Map(String, Int64),
    ADD COLUMN IF NOT EXISTS node_addresses String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS kubelet_version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS kube_proxy_version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS operating_system LowCardinality(String),
    ADD COLUMN IF NOT EXISTS architecture LowCardinality(String),
    ADD COLUMN IF NOT EXISTS kernel_version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS os_image LowCardinality(String),
    ADD COLUMN IF NOT EXISTS container_runtime_version LowCardinality(String),

    -- Service (type 44), which had zero coverage: an identity-only Service row
    -- discards the entire reason a Service exists.
    ADD COLUMN IF NOT EXISTS service_type LowCardinality(String),
    ADD COLUMN IF NOT EXISTS cluster_ip String,
    ADD COLUMN IF NOT EXISTS service_ports String CODEC(ZSTD(3)),

    -- RBAC (54-57). Rules are the permissions actually granted — the single
    -- most security-relevant field in the whole orchestrator feed, and it was
    -- decoded and dropped. JSON because a PolicyRule is five string arrays and
    -- a binding is a list of subjects plus a role reference.
    ADD COLUMN IF NOT EXISTS rbac_rules String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS rbac_subjects String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS rbac_role_ref String CODEC(ZSTD(3)),

    -- The frame envelope as JSON, with the object list left out (it is the
    -- rows). Every Collector* message carries a few fields beyond the six the
    -- generic converter reads by name, and they differ per kind:
    -- CollectorPod.Host / .Info / .IsTerminated describe the cluster agent's
    -- own machine and whether the batch is the terminated-pods one;
    -- CollectorNode.HostAliasMapping resolves node aliases. One column keeps
    -- all of them for all 27 kinds, and repeats per row — which costs almost
    -- nothing, because every row of a pass carries the same bytes and ZSTD
    -- collapses them inside the part.
    ADD COLUMN IF NOT EXISTS envelope String CODEC(ZSTD(3));

ALTER TABLE k8s_resources_current
    ADD COLUMN IF NOT EXISTS org_id Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS subscription_id UInt8,
    ADD COLUMN IF NOT EXISTS header_timestamp Nullable(Int64),
    ADD COLUMN IF NOT EXISTS encoding LowCardinality(String),
    ADD COLUMN IF NOT EXISTS creation_timestamp Nullable(DateTime('UTC')),
    ADD COLUMN IF NOT EXISTS deletion_timestamp Nullable(DateTime('UTC')),
    ADD COLUMN IF NOT EXISTS deletion_grace_period_seconds Nullable(Int64),
    ADD COLUMN IF NOT EXISTS finalizers Array(String),
    ADD COLUMN IF NOT EXISTS owner_kinds Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS owner_names Array(String),
    ADD COLUMN IF NOT EXISTS owner_uids Array(String),
    ADD COLUMN IF NOT EXISTS condition_types Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS condition_statuses Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS condition_reasons Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS condition_messages Array(String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS condition_last_transition Array(Nullable(DateTime('UTC'))),
    ADD COLUMN IF NOT EXISTS condition_last_update Array(Nullable(DateTime('UTC'))),
    ADD COLUMN IF NOT EXISTS condition_last_probe Array(Nullable(DateTime('UTC'))),
    ADD COLUMN IF NOT EXISTS condition_message String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS resource_requirements String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS metrics Map(String, Float64),
    ADD COLUMN IF NOT EXISTS object String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS pod_ip String,
    ADD COLUMN IF NOT EXISTS nominated_node_name LowCardinality(String),
    ADD COLUMN IF NOT EXISTS qos_class LowCardinality(String),
    ADD COLUMN IF NOT EXISTS priority_class LowCardinality(String),
    ADD COLUMN IF NOT EXISTS start_time Nullable(DateTime('UTC')),
    ADD COLUMN IF NOT EXISTS scheduled_time Nullable(DateTime('UTC')),
    ADD COLUMN IF NOT EXISTS host_name LowCardinality(String),
    ADD COLUMN IF NOT EXISTS container_names Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS container_ids Array(String),
    ADD COLUMN IF NOT EXISTS container_ready Array(UInt8),
    ADD COLUMN IF NOT EXISTS container_restarts Array(Int32),
    ADD COLUMN IF NOT EXISTS container_states Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS container_messages Array(String) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS container_images Array(String),
    ADD COLUMN IF NOT EXISTS container_image_ids Array(String),
    ADD COLUMN IF NOT EXISTS container_is_init Array(UInt8),
    ADD COLUMN IF NOT EXISTS pod_cidr String,
    ADD COLUMN IF NOT EXISTS pod_cidrs Array(String),
    ADD COLUMN IF NOT EXISTS unschedulable UInt8,
    ADD COLUMN IF NOT EXISTS taints String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS provider_id String,
    ADD COLUMN IF NOT EXISTS node_roles Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS capacity Map(String, Int64),
    ADD COLUMN IF NOT EXISTS allocatable Map(String, Int64),
    ADD COLUMN IF NOT EXISTS node_addresses String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS kubelet_version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS kube_proxy_version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS operating_system LowCardinality(String),
    ADD COLUMN IF NOT EXISTS architecture LowCardinality(String),
    ADD COLUMN IF NOT EXISTS kernel_version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS os_image LowCardinality(String),
    ADD COLUMN IF NOT EXISTS container_runtime_version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS service_type LowCardinality(String),
    ADD COLUMN IF NOT EXISTS cluster_ip String,
    ADD COLUMN IF NOT EXISTS service_ports String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS rbac_rules String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS rbac_subjects String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS rbac_role_ref String CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS envelope String CODEC(ZSTD(3));

-- The materialized view has to learn the new columns or k8s_resources_current
-- freezes at the old 24.
--
-- DROP + CREATE rather than ALTER TABLE ... MODIFY QUERY, for one reason:
-- MODIFY QUERY has no IF EXISTS form, so on a FRESH database — where 0001 has
-- just created the view and this migration runs seconds later — it works, but
-- on a database where a previous attempt at this file died halfway it may be
-- modifying a view that is not there. DROP VIEW IF EXISTS + CREATE ... IF NOT
-- EXISTS is idempotent in both directions, which is the property this runner
-- needs (no transactions, re-run after a fix). Dropping a materialized view
-- destroys no data: k8s_resources_current is a real table and keeps its rows;
-- only inserts arriving in the gap between the two statements miss the view,
-- and the next collection pass (~10s later) replaces those objects anyway.
--
-- The column list is spelled out rather than SELECT *: ClickHouse freezes * at
-- creation time regardless, so the explicit form costs nothing and makes the
-- next person's diff readable.
DROP VIEW IF EXISTS k8s_resources_current_mv;

CREATE MATERIALIZED VIEW IF NOT EXISTS k8s_resources_current_mv
TO k8s_resources_current AS
SELECT
    tenant_id,
    collected_at,
    cluster_id,
    cluster_name,
    kind,
    namespace,
    name,
    uid,
    resource_version,
    owner_kind,
    owner_name,
    node_name,
    phase,
    status,
    ready,
    desired,
    available,
    counts,
    labels,
    annotations,
    tags,
    agent_version,
    group_id,
    group_size,
    org_id,
    subscription_id,
    header_timestamp,
    encoding,
    creation_timestamp,
    deletion_timestamp,
    deletion_grace_period_seconds,
    finalizers,
    owner_kinds,
    owner_names,
    owner_uids,
    condition_types,
    condition_statuses,
    condition_reasons,
    condition_messages,
    condition_last_transition,
    condition_last_update,
    condition_last_probe,
    condition_message,
    resource_requirements,
    metrics,
    object,
    pod_ip,
    nominated_node_name,
    qos_class,
    priority_class,
    start_time,
    scheduled_time,
    host_name,
    container_names,
    container_ids,
    container_ready,
    container_restarts,
    container_states,
    container_messages,
    container_images,
    container_image_ids,
    container_is_init,
    pod_cidr,
    pod_cidrs,
    unschedulable,
    taints,
    provider_id,
    node_roles,
    capacity,
    allocatable,
    node_addresses,
    kubelet_version,
    kube_proxy_version,
    operating_system,
    architecture,
    kernel_version,
    os_image,
    container_runtime_version,
    service_type,
    cluster_ip,
    service_ports,
    rbac_rules,
    rbac_subjects,
    rbac_role_ref,
    envelope
FROM k8s_resources;

-- ---------------------------------------------------------------------------
-- k8s_cluster: CollectorCluster (type 46)
--
-- The envelope was dropped entirely here (tags, agent version, group) while
-- k8s_resources kept it, and the per-node breakdown behind node_count was
-- discarded — so "which of my nodes is on the old kubelet" was answerable only
-- as a histogram with no names on it.
-- ---------------------------------------------------------------------------
ALTER TABLE k8s_cluster
    ADD COLUMN IF NOT EXISTS org_id Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS subscription_id UInt8,
    ADD COLUMN IF NOT EXISTS header_timestamp Nullable(Int64),
    ADD COLUMN IF NOT EXISTS encoding LowCardinality(String),

    ADD COLUMN IF NOT EXISTS group_id Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS group_size Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS agent_version LowCardinality(String),

    -- Two tag sets, deliberately separate columns: the frame envelope's tags
    -- (CollectorCluster.Tags) and the Cluster object's own (Cluster.Tags).
    -- They are different fields on the wire and merging them would make it
    -- impossible to tell which producer attached a tag.
    ADD COLUMN IF NOT EXISTS tags Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS cluster_tags Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    ADD COLUMN IF NOT EXISTS resource_version String,
    ADD COLUMN IF NOT EXISTS creation_timestamp Nullable(DateTime('UTC')),
    ADD COLUMN IF NOT EXISTS metrics Map(String, Float64),

    -- GPUs and other extended resources. Without these there is no visibility
    -- into custom capacity anywhere in the schema.
    ADD COLUMN IF NOT EXISTS extended_resources_capacity Map(String, Int64),
    ADD COLUMN IF NOT EXISTS extended_resources_allocatable Map(String, Int64),

    -- ClusterNodeInfo, parallel arrays in wire order. The two map arrays are
    -- Map(String, String) because the wire sends quantities as their
    -- Kubernetes strings here ("16Gi", "4"), unlike NodeStatus which sends
    -- int64 — converting would guess at units, so they are kept verbatim.
    ADD COLUMN IF NOT EXISTS nodes_name Array(String),
    ADD COLUMN IF NOT EXISTS nodes_region Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS nodes_instance_type Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS nodes_os Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS nodes_os_image Array(String),
    ADD COLUMN IF NOT EXISTS nodes_architecture Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS nodes_kernel_version Array(String),
    ADD COLUMN IF NOT EXISTS nodes_container_runtime_version Array(String),
    ADD COLUMN IF NOT EXISTS nodes_kubelet_version Array(LowCardinality(String)),
    ADD COLUMN IF NOT EXISTS nodes_allocatable Array(Map(String, String)) CODEC(ZSTD(3)),
    ADD COLUMN IF NOT EXISTS nodes_capacity Array(Map(String, String)) CODEC(ZSTD(3));

-- Tags get the same bloom filters they get on every other table that carries
-- them; arrayFlatten because mapValues over an array-valued map is
-- Array(Array(...)), which bloom_filter rejects.
ALTER TABLE k8s_cluster
    ADD INDEX IF NOT EXISTS idx_tag_keys   mapKeys(tags)                  TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS idx_tag_values arrayFlatten(mapValues(tags))  TYPE bloom_filter(0.01) GRANULARITY 4;

-- ---------------------------------------------------------------------------
-- k8s_manifests: /api/v2/orchmanif (types 80-82)
--
-- Manifest rows carried no tags at all — not the envelope's, not the CRD/CR
-- wrapper's, not the manifest's own — and no ExtraAttributes map, which on a
-- custom resource is where the interesting part lives.
-- ---------------------------------------------------------------------------
ALTER TABLE k8s_manifests
    ADD COLUMN IF NOT EXISTS org_id Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS subscription_id UInt8,
    ADD COLUMN IF NOT EXISTS header_timestamp Nullable(Int64),
    ADD COLUMN IF NOT EXISTS encoding LowCardinality(String),

    ADD COLUMN IF NOT EXISTS group_id Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS group_size Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS host_name LowCardinality(String),
    ADD COLUMN IF NOT EXISTS agent_version LowCardinality(String),
    -- CollectorManifest.OriginCollector: datadogAgent, datadogExporter or
    -- unknown. Which pipeline produced the manifest, in an installation that
    -- runs both.
    ADD COLUMN IF NOT EXISTS origin_collector LowCardinality(String),

    -- All three tag sources merged into one multiset, because a query asking
    -- "manifests tagged env:prod" does not care where the tag was attached...
    ADD COLUMN IF NOT EXISTS tags Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- ...and a second map that says where each one came from, keyed by source
    -- ('envelope' | 'wrapper' | 'manifest') holding that source's raw tag
    -- strings. Cheap: the same handful of strings, one extra dictionary.
    ADD COLUMN IF NOT EXISTS tag_sources Map(LowCardinality(String), Array(String)) CODEC(ZSTD(3)),

    ADD COLUMN IF NOT EXISTS node_name LowCardinality(String),
    -- Manifest.Type, the numeric kind marker, and Manifest.Version, the
    -- manifest schema version. Redundant with kind/api_version today; kept
    -- because "redundant today" is how a silent format change hides.
    ADD COLUMN IF NOT EXISTS type Int32 CODEC(T64, ZSTD(1)),
    ADD COLUMN IF NOT EXISTS version LowCardinality(String),
    ADD COLUMN IF NOT EXISTS extra_attributes Map(String, String) CODEC(ZSTD(3)),

    -- content is []byte on the wire and String in ClickHouse, and String here
    -- is a byte string, so nothing is corrupted either way — but a reader
    -- deserves to know before it tries to parse. 1 = the bytes are valid
    -- UTF-8 (every YAML/JSON manifest seen so far).
    ADD COLUMN IF NOT EXISTS content_is_utf8 UInt8;

ALTER TABLE k8s_manifests
    ADD INDEX IF NOT EXISTS idx_tag_keys   mapKeys(tags)                 TYPE bloom_filter(0.01) GRANULARITY 4,
    ADD INDEX IF NOT EXISTS idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4;

-- ---------------------------------------------------------------------------
-- k8s_actions: /api/v2/kubeactions
--
-- Two documented "by design" drops that the fidelity rule does not accept:
-- the attachments, and the VALUES of undeclared JSON keys (only their names
-- were kept). Both are small, both arrive once, and both are exactly what
-- someone debugging a failed remote action wants.
-- ---------------------------------------------------------------------------
ALTER TABLE k8s_actions
    -- Whatever the executor attached, name -> bytes. Map(String, String)
    -- because ClickHouse Strings are byte strings: a payload that is not text
    -- survives unchanged.
    ADD COLUMN IF NOT EXISTS payloads Map(String, String) CODEC(ZSTD(3)),
    -- The undeclared keys with their values, as a JSON object. extra_keys
    -- (names only) stays: it is cheap to filter on and does not need parsing.
    ADD COLUMN IF NOT EXISTS extra String CODEC(ZSTD(3)),
    -- The timestamp exactly as it arrived. The row's own timestamp falls back
    -- to arrival time when the string is empty or unparseable, and that
    -- fallback used to erase the evidence of itself.
    ADD COLUMN IF NOT EXISTS timestamp_raw String;

-- ---------------------------------------------------------------------------
-- ecs_tasks: CollectorECSTask (type 200), NEW TABLE
--
-- ECS tasks arrive on the orchestrator intake and were decoded and discarded,
-- with a comment explaining that a task has no Kubernetes Metadata and so
-- cannot be stored under k8s_resources' identity columns. True, and the answer
-- is a table of its own rather than a lossy squeeze into someone else's.
--
-- The identity is the task ARN. Same data class as k8s_resources — periodic
-- collection passes, bursty, read over days not months — so the same daily
-- partitions and 30-day TTL. Append-only: this is the history of what ran, not
-- a current-state view.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ecs_tasks
(
    tenant_id        LowCardinality(String),
    collected_at     DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- The frame header, as for every other orchestrator row.
    org_id           Int32 CODEC(T64, ZSTD(1)),
    subscription_id  UInt8,
    header_timestamp Nullable(Int64),
    encoding         LowCardinality(String),

    -- Envelope: CollectorECSTask itself.
    aws_account_id   Int64 CODEC(T64, ZSTD(1)),
    cluster_id       LowCardinality(String),
    cluster_name     LowCardinality(String),
    region           LowCardinality(String),
    group_id         Int32 CODEC(T64, ZSTD(1)),
    group_size       Int32 CODEC(T64, ZSTD(1)),
    host_name        LowCardinality(String),
    agent_version    LowCardinality(String),
    -- The envelope's tags, kept apart from the task's own three tag sets for
    -- the same reason k8s_cluster keeps its two apart.
    envelope_tags    Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    -- The task.
    arn              String,
    resource_version String,
    launch_type      LowCardinality(String),  -- EC2 / FARGATE
    desired_status   LowCardinality(String),
    known_status     LowCardinality(String),
    family           LowCardinality(String),
    version          LowCardinality(String),
    availability_zone LowCardinality(String),
    service_name     LowCardinality(String),
    vpc_id           LowCardinality(String),
    container_instance_arn String,
    daemon_name      LowCardinality(String),
    task_host_name   LowCardinality(String),  -- ECSTask.Host.Name

    -- CPU and memory the task was given, and its ephemeral disk counters.
    -- Plain Maps: protobuf map keys are unique, so no multiset shape.
    limits           Map(String, Float64),
    ephemeral_storage_metrics Map(String, Int64),

    -- Three Unix-seconds timestamps, all genuinely optional: a task that never
    -- finished pulling has no pull_stopped_at, and 0 must not become 1970.
    pull_started_at      Nullable(DateTime('UTC')),
    pull_stopped_at      Nullable(DateTime('UTC')),
    execution_stopped_at Nullable(DateTime('UTC')),

    -- The container list as JSON, plus its length as a column so the common
    -- "how many containers" needs no parsing. An ECSContainer carries ports,
    -- networks, volumes, health, log options and limits — a nested shape that
    -- would be twenty parallel arrays of arrays, read by nobody. It is kept
    -- whole and exactly, which is what a per-container question needs anyway.
    containers       String CODEC(ZSTD(3)),
    container_count  UInt32 CODEC(T64, ZSTD(1)),

    -- The task's own three tag sets, separate because ECS keeps them separate:
    -- Datadog tags, the task's ECS tags, and the container instance's.
    tags                   Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    ecs_tags               Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    container_instance_tags Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)                 TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(collected_at)
ORDER BY (tenant_id, cluster_id, arn, collected_at)
TTL toDateTime(collected_at) + INTERVAL 30 DAY;
