-- Network Device Monitoring — four hosts, five routes, one product from the
-- operator's point of view (see intake/router_ndm.go's header).
--
-- Three of the five wire shapes (ndm, ndmconfig, ndmtraps) have no importable
-- Go type: the real ones live in pkg/networkdevice/metadata,
-- pkg/networkconfigmanagement/report and comp/snmptraps/formatter, all of
-- which pull the whole datadog-agent module in. router_ndm.go mirrors them
-- with local structs instead (decode-twice, like kubeActionDecode in
-- router_kubeops.go), so the JSON field names below are ninjacat's best
-- reconstruction of the wire, not a copy of a vendored tag. netflow_flows and
-- network_paths ARE typed against the real vendored structs
-- (comp/netflow/payload.FlowPayload, pkg/networkpath/payload.NetworkPath),
-- so those two column lists are exact.
--
-- No CREATE DATABASE and no database qualifiers — same reasoning as 0001.

-- ---------------------------------------------------------------------------
-- Device inventory: /api/v2/ndm, the "devices" list of
-- metadata.NetworkDevicesMetadata.
--
-- One row per device per collection pass — classified with the bursty
-- Kubernetes tables (k8s_resources): a full inventory arrives every pass, not
-- incrementally, so daily partitions and a 30-day TTL rather than a "current"
-- companion table (no ReplacingMergeTree pair here; nothing in the brief for
-- this table asked for one, and namespace+device_id+collect_timestamp already
-- answers "what did we see, and when").
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ndm_devices
(
    tenant_id         LowCardinality(String),

    -- Payload-level fields (metadata.NetworkDevicesMetadata): set once for
    -- the whole collection pass, not per device.
    namespace         LowCardinality(String),
    subnet            String,
    integration       LowCardinality(String),
    collect_timestamp DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Top-level payload keys the local mirror struct does not declare, kept
    -- as raw JSON text per key — the same idea as k8s_actions.extra_keys, but
    -- keeping the values too since there is no shared Go type here to grow a
    -- field for them later.
    extra             Map(String, String) CODEC(ZSTD(3)),

    device_id         String,
    id_tags           Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    tags              Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    ip_address        String,

    -- DeviceStatus travels as an int32 on the wire (1=Reachable,
    -- 2=Unreachable); anything else, including an absent field, is
    -- "unknown". Enum8 travels by name over the driver, never by number.
    status            Enum8('unknown' = 0, 'reachable' = 1, 'unreachable' = 2),
    ping_status       Enum8('unknown' = 0, 'reachable' = 1, 'unreachable' = 2),

    name               String,
    description        String CODEC(ZSTD(3)),
    sys_object_id      String,
    location           String,
    profile            LowCardinality(String),
    profile_version    UInt64 CODEC(T64, ZSTD(1)),
    vendor             LowCardinality(String),

    -- DeviceMetadata carries its own subnet/integration alongside the
    -- payload-level ones above; the profile can genuinely differ per device
    -- inside one collection pass, so both are kept rather than assumed equal.
    device_subnet      String,
    serial_number      String,
    version            String,
    product_name       String,
    model              String,
    os_name            LowCardinality(String),
    os_version         String,
    os_hostname        String,
    device_integration LowCardinality(String),
    device_type        LowCardinality(String),

    INDEX idx_tag_keys      mapKeys(tags)                   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values    arrayFlatten(mapValues(tags))   TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_id_tag_keys   mapKeys(id_tags)                TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_id_tag_values arrayFlatten(mapValues(id_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(collect_timestamp)
ORDER BY (tenant_id, namespace, device_id, collect_timestamp)
TTL collect_timestamp + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Interface inventory: /api/v2/ndm, the "interfaces" list.
-- Same collection-pass shape as ndm_devices, keyed one level deeper by index.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ndm_interfaces
(
    tenant_id         LowCardinality(String),
    namespace         LowCardinality(String),
    subnet            String,
    integration       LowCardinality(String),
    collect_timestamp DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    extra             Map(String, String) CODEC(ZSTD(3)),

    device_id         String,
    id_tags           Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- Named if_index, not index: INDEX is part of CREATE TABLE's own grammar
    -- (the bloom_filter declarations below), and a column literally called
    -- "index" parses as the start of one and fails with a cryptic error.
    if_index          Int32 CODEC(T64, ZSTD(1)),
    raw_id            String,
    raw_id_type       LowCardinality(String),
    name              String,
    alias             String,
    description       String,
    mac_address       String,

    -- IF-MIB's ifAdminStatus/ifOperStatus (RFC 2863) are small numeric enums,
    -- but the vendored InterfaceMetadata struct is not importable here (see
    -- the file header), so the exact value set cannot be confirmed from
    -- source. Kept as the raw wire number rather than guessing a label.
    admin_status      UInt8 CODEC(T64, ZSTD(1)),
    oper_status       UInt8 CODEC(T64, ZSTD(1)),
    -- ifType, from the IANA ifType registry (thousands of values) — same
    -- reasoning, kept numeric.
    type              UInt32 CODEC(T64, ZSTD(1)),

    -- *bool on the Go side: nil/true/false are three different facts, so
    -- Nullable(UInt8) rather than a UInt8 that would make "not reported"
    -- indistinguishable from false.
    is_physical       Nullable(UInt8),
    meraki_enabled    Nullable(UInt8),
    meraki_status     LowCardinality(String),

    INDEX idx_id_tag_keys   mapKeys(id_tags)                TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_id_tag_values arrayFlatten(mapValues(id_tags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(collect_timestamp)
ORDER BY (tenant_id, namespace, device_id, if_index, collect_timestamp)
TTL collect_timestamp + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- IP address inventory: /api/v2/ndm, the "ip_addresses" list.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ndm_ip_addresses
(
    tenant_id         LowCardinality(String),
    namespace         LowCardinality(String),
    subnet            String,
    integration       LowCardinality(String),
    collect_timestamp DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    extra             Map(String, String) CODEC(ZSTD(3)),

    interface_id      String,
    ip_address        String,
    prefixlen         Int32 CODEC(T64, ZSTD(1))
)
ENGINE = MergeTree
PARTITION BY toDate(collect_timestamp)
ORDER BY (tenant_id, namespace, interface_id, collect_timestamp)
TTL collect_timestamp + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Everything else /api/v2/ndm carries: links, vpn_tunnels, netflow_exporters,
-- diagnoses, device_oids and scan_status. Six shapes, small and varied enough
-- (a link has no device_id at all; a diagnosis nests its own list) that
-- typing each into its own table would mean six more tables for a few rows
-- apiece. object keeps the element exactly as it arrived — lossless, and
-- still queryable with JSONExtract — while kind/device_id/interface_id are
-- promoted for the filters that matter (per intake/router_ndm.go's
-- ndmMetadataObjectRows: device_id/interface_id are read off the same
-- element when that shape has one, empty otherwise).
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ndm_metadata_objects
(
    tenant_id         LowCardinality(String),
    namespace         LowCardinality(String),
    subnet            String,
    integration       LowCardinality(String),
    collect_timestamp DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    extra             Map(String, String) CODEC(ZSTD(3)),

    -- link / vpn_tunnel / netflow_exporter / diagnosis / device_oid / scan_status
    kind              LowCardinality(String),
    device_id         String,
    interface_id      String,
    object            String CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(collect_timestamp)
ORDER BY (tenant_id, namespace, kind, device_id, collect_timestamp)
TTL collect_timestamp + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- Device configuration backups: /api/v2/ndmconfig, report.NCMPayload.
--
-- Classified with k8s_manifests: a whole document per row (a full running-
-- or startup-config), bulky rather than frequent, so a short TTL — a week of
-- full configs outweighs a month of every other NDM table here.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS ndm_device_configs
(
    tenant_id         LowCardinality(String),
    namespace         LowCardinality(String),
    collect_timestamp DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),

    -- Neither is in the documented report.NCMPayload, but both are read off
    -- the wire in practice (router_ndm.go's ndmLogConfig predates this
    -- migration and already reads them) — real columns rather than folded
    -- into extra, since they recur on every request rather than being a
    -- one-off addition.
    agent_hostname    LowCardinality(String),
    -- Raw JSON, kept whole rather than typed: the report flags this key as
    -- observed-but-undocumented, so its shape is not confirmed enough to
    -- justify columns yet.
    inventories       String CODEC(ZSTD(3)),
    extra             Map(String, String) CODEC(ZSTD(3)),

    device_id         String,
    device_ip         String,
    config_type       LowCardinality(String),
    config_source     LowCardinality(String),
    -- Backfilled agent-side to the collection time whenever a device's own
    -- config has no timestamp (ToNCMPayload, per the NDM gap report), so this
    -- is never legitimately 0 by the time it reaches us — no Nullable needed.
    timestamp         DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    tags              Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- A whole device config file. Bulky and cold, like k8s_manifests.content —
    -- read whole when someone needs it, never filtered on.
    content           String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)                 TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags))  TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, namespace, device_id, timestamp)
TTL timestamp + INTERVAL 7 DAY;

-- ---------------------------------------------------------------------------
-- SNMP traps: /api/v2/ndmtraps, comp/snmptraps/formatter's {"trap": {...}}.
--
-- Classified with events/container_events (point-in-time facts, not an
-- upsert): a trap is a thing that happened, never replaced.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS snmp_traps
(
    tenant_id       LowCardinality(String),
    timestamp       DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),

    ddsource        LowCardinality(String),
    ddtags          Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),
    -- Pulled out of ddtags (snmp_device:<ip>) because it is the column every
    -- "who sent this trap" query filters on first.
    device          LowCardinality(String),

    uptime          UInt32 CODEC(T64, ZSTD(1)),
    snmp_trap_oid   String,
    snmp_trap_name  LowCardinality(String),
    snmp_trap_mib   LowCardinality(String),

    -- v1-only fields (comp/snmptraps/formatter): absent, not zero, on v2/v3
    -- traps. enterprise_oid stays a plain String (empty = absent is
    -- unambiguous for an OID, unlike the two integers below).
    enterprise_oid  String,
    generic_trap    Nullable(Int32),
    specific_trap   Nullable(Int32),

    -- Parallel arrays: variables[i] = (var_oids[i], var_types[i], var_values[i]).
    -- value is kept as the JSON text of whatever the SNMP type decoded to
    -- (string, number, or something MIB-specific) rather than forced into one
    -- column type it may not fit.
    var_oids        Array(String) CODEC(ZSTD(1)),
    var_types       Array(LowCardinality(String)) CODEC(ZSTD(1)),
    var_values      Array(String) CODEC(ZSTD(3)),

    -- Keys the OID resolver merged in beyond the fixed formatter fields
    -- (MIB/profile-defined, so the name set is not fixed) — raw JSON text per
    -- key, same shape as the other NDM tables' extra.
    enriched        Map(String, String) CODEC(ZSTD(3)),

    -- The trap sub-object exactly as received, for the rare case a field
    -- above was misparsed and the wire needs re-reading.
    raw             String CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(ddtags)                TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(ddtags)) TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toDate(timestamp)
ORDER BY (tenant_id, device, timestamp)
TTL timestamp + INTERVAL 30 DAY;

-- ---------------------------------------------------------------------------
-- NetFlow / sFlow / IPFIX: /api/v2/ndmflow, comp/netflow/payload.FlowPayload.
-- A real vendored type (see the file header), decoded through
-- intake.ndmDecodeFlows rather than plain json.Unmarshal — FlowPayload's own
-- MarshalJSON spreads AdditionalFields over the root object instead of an
-- "additional_fields" key, and a bare Unmarshal would silently drop them.
--
-- Classified with logs: every configured exporter reports on its own timer,
-- independent of every other NDM table here, and can be the highest-volume
-- track by a wide margin.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS netflow_flows
(
    tenant_id         LowCardinality(String),
    -- Assumed milliseconds: undocumented on the wire, unlike flow_start/
    -- flow_end below (FlowPayload comments them "in seconds" explicitly,
    -- which is the exception, not the rule, for this intake — see
    -- network_paths.timestamp for the same reasoning). The intake test
    -- (TestNDMDecodeFlowsRestoresAdditionalFields) pins a captured value at
    -- 1758326400123, a 13-digit millisecond epoch.
    flush_timestamp   DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    flow_type         LowCardinality(String),
    sampling_rate     UInt64 CODEC(T64, ZSTD(1)),
    direction         LowCardinality(String),
    flow_start        DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    flow_end          DateTime('UTC') CODEC(DoubleDelta, ZSTD(1)),
    bytes             UInt64 CODEC(T64, ZSTD(1)),
    packets           UInt64 CODEC(T64, ZSTD(1)),
    ether_type        LowCardinality(String),
    ip_protocol       LowCardinality(String),
    tos               UInt32 CODEC(T64, ZSTD(1)),
    dscp              UInt32 CODEC(T64, ZSTD(1)),
    dscp_name         LowCardinality(String),

    device_namespace  LowCardinality(String),
    exporter_ip       LowCardinality(String),

    source_ip                     String,
    -- A port NUMBER on the wire is a string too (Endpoint.Port in
    -- comp/netflow/payload) — it can also read "*" for an ephemeral port, so
    -- it is never a numeric column here.
    source_port                   LowCardinality(String),
    source_mac                    String,
    source_mask                   String,
    source_reverse_dns_hostname   String,

    destination_ip                    String,
    destination_port                  LowCardinality(String),
    destination_mac                   String,
    destination_mask                  String,
    destination_reverse_dns_hostname  String,

    ingress_interface_index UInt32 CODEC(T64, ZSTD(1)),
    egress_interface_index  UInt32 CODEC(T64, ZSTD(1)),

    host              LowCardinality(String),
    tcp_flags         Array(LowCardinality(String)),
    next_hop_ip       String,

    -- Datadog's own marshaller spreads these over the root object instead of
    -- an "additional_fields" key; intake.ndmDecodeFlows puts them back.
    -- Values are the JSON text of whatever arrived, numbers kept as
    -- json.Number so a large counter never rounds through float64.
    additional_fields Map(String, String) CODEC(ZSTD(3))
)
ENGINE = MergeTree
PARTITION BY toDate(flow_start)
ORDER BY (tenant_id, device_namespace, exporter_ip, flow_start)
TTL toDateTime(flow_start) + INTERVAL 14 DAY;

-- ---------------------------------------------------------------------------
-- Traceroute results: /api/v2/netpath, pkg/networkpath/payload.NetworkPath.
-- A real vendored type, decoded as-is (no MarshalJSON asymmetry to work
-- around here, unlike netflow).
--
-- Classified with check_runs: a scheduled test run, human/config scale
-- rather than per-packet volume — same monthly partitioning AND the same
-- 90-day TTL (conventions.md pairs toYYYYMM with 90d+ retention, toDate with
-- <=30d for cheap DROP PARTITION), not the 30-day TTL every daily-partitioned
-- table in this migration gets.
-- ---------------------------------------------------------------------------
CREATE TABLE IF NOT EXISTS network_paths
(
    tenant_id         LowCardinality(String),
    -- Assumed milliseconds — see netflow_flows.flush_timestamp for why: this
    -- is the pattern for every undocumented Datadog "timestamp" field in this
    -- intake, with seconds called out explicitly where it applies instead.
    timestamp         DateTime64(3, 'UTC') CODEC(DoubleDelta, ZSTD(1)),
    agent_version     LowCardinality(String),
    namespace         LowCardinality(String),
    test_config_id    String,
    test_config_name  String,
    test_result_id    String,
    test_run_id       String,

    origin              LowCardinality(String),
    test_run_type       LowCardinality(String),
    test_config_source  LowCardinality(String),
    source_product      LowCardinality(String),
    collector_type      LowCardinality(String),
    protocol            LowCardinality(String),

    source_name          String,
    source_display_name  String,
    source_hostname      LowCardinality(String),
    -- Source.Via is *payload.Via: nil means the agent could not resolve a
    -- route, not "resolved to nothing" — Nullable keeps that distinct from an
    -- empty string.
    source_via_subnet_alias            Nullable(String),
    source_via_interface_hardware_addr Nullable(String),
    source_network_id    String,
    source_service       LowCardinality(String),
    source_container_id  String,
    source_public_ip     String,

    destination_hostname LowCardinality(String),
    destination_port     UInt16 CODEC(T64, ZSTD(1)),
    destination_service  LowCardinality(String),

    hop_count_avg Float64 CODEC(Gorilla, ZSTD(1)),
    hop_count_min Int32 CODEC(T64, ZSTD(1)),
    hop_count_max Int32 CODEC(T64, ZSTD(1)),

    -- One traceroute can run several times (Traceroute.Runs); these six are
    -- parallel arrays over the runs, outer index = run.
    run_ids                     Array(String),
    run_source_ips               Array(String),
    run_source_ports             Array(UInt16),
    run_destination_ips          Array(String),
    run_destination_ports        Array(UInt16),
    run_destination_reverse_dns  Array(Array(String)),

    -- Hops nest one level deeper than the run arrays above — outer index is
    -- still the run, inner index is the hop within it.
    hop_ttls        Array(Array(Int32)),
    hop_ips         Array(Array(String)),
    -- A hop's own reverse_dns is itself a list of names, hence three levels.
    hop_reverse_dns Array(Array(Array(String))),
    hop_rtts        Array(Array(Float64)),
    hop_reachable   Array(Array(UInt8)),

    -- E2eProbe is one probe run over the whole path, not per traceroute run —
    -- flat arrays of every sample, not nested like the hops above.
    e2e_rtts                   Array(Float64) CODEC(ZSTD(1)),
    e2e_packets_sent           Int32 CODEC(T64, ZSTD(1)),
    e2e_packets_received       Int32 CODEC(T64, ZSTD(1)),
    e2e_packet_loss_percentage Float32 CODEC(Gorilla, ZSTD(1)),
    e2e_jitter                 Float64 CODEC(Gorilla, ZSTD(1)),
    e2e_rtt_avg                Float64 CODEC(Gorilla, ZSTD(1)),
    e2e_rtt_min                Float64 CODEC(Gorilla, ZSTD(1)),
    e2e_rtt_max                Float64 CODEC(Gorilla, ZSTD(1)),

    tags Map(LowCardinality(String), Array(LowCardinality(String))) CODEC(ZSTD(3)),

    INDEX idx_tag_keys   mapKeys(tags)                  TYPE bloom_filter(0.01) GRANULARITY 4,
    INDEX idx_tag_values arrayFlatten(mapValues(tags))  TYPE bloom_filter(0.01) GRANULARITY 4
)
ENGINE = MergeTree
PARTITION BY toYYYYMM(timestamp)
ORDER BY (tenant_id, namespace, test_config_id, timestamp)
TTL toDateTime(timestamp) + INTERVAL 90 DAY;
