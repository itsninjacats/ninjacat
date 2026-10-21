//! Everything the process-agent sends: processes, containers, connections.
//!
//! One frame carries one of six message types. Its envelope goes to process_snapshots once; the
//! item tables carry snapshot_id back to it and a copy of the few envelope fields every query
//! needs, so the common question is not a join. Reasoning per column: the process tables in
//! 0001_initial.sql.

use std::collections::BTreeMap;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;
use uuid::Uuid;

use super::Tags;
use crate::column::{self, Blob};
use crate::table::{Limits, Table};

/// One process from a process-agent snapshot.
///
/// The field order is the column order. The first eighteen are the columns the table shipped
/// with; the rest were added with ALTER TABLE ... ADD COLUMN and so come after them. New fields
/// are appended, never inserted.
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct Process {
    pub tenant_id: String,
    #[serde(with = "column::secs")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    pub pid: i32,
    pub ppid: i32,
    pub user: String,
    pub comm: String,
    pub exe: String,
    pub cmdline: String,
    pub rss: u64,
    pub vms: u64,
    pub cpu_pct: f32,
    pub threads: i32,
    pub open_fds: i32,
    pub state: String,
    #[serde(with = "column::secs")]
    pub create_time: DateTime<Utc>,
    pub container_id: String,
    pub tags: Tags,

    /// The frame this process arrived in; joins to process_snapshots.
    #[serde(with = "clickhouse::serde::uuid")]
    pub snapshot_id: Uuid,

    pub ns_pid: i32,
    pub key: u32,

    pub cwd: String,
    pub root: String,
    pub on_disk: u8,
    pub pgroup: i32,

    /// argv as sent. cmdline is the same joined on spaces and cannot be split back: an argument
    /// containing a space looks like two.
    pub args: Vec<String>,

    pub uid: i32,
    pub gid: i32,
    pub euid: i32,
    pub egid: i32,
    pub suid: i32,
    pub sgid: i32,

    pub mem_swap: u64,
    pub mem_shared: u64,
    pub mem_text: u64,
    pub mem_lib: u64,
    pub mem_data: u64,
    pub mem_dirty: u64,

    pub cpu_last_cpu: String,
    pub cpu_user_pct: f32,
    pub cpu_system_pct: f32,
    pub cpu_nice: i32,
    pub cpu_user_time: i64,
    pub cpu_system_time: i64,

    /// Parallel arrays: index i of one pairs with index i of the other.
    pub cpu_core_names: Vec<String>,
    pub cpu_core_pcts: Vec<f32>,

    pub io_read_rate: f32,
    pub io_write_rate: f32,
    pub io_read_bytes_rate: f32,
    pub io_write_bytes_rate: f32,

    pub voluntary_ctx_switches: u64,
    pub involuntary_ctx_switches: u64,

    pub net_connection_rate: f32,
    pub net_bytes_rate: f32,

    pub process_context: Vec<String>,

    /// Enum NAMES (LANGUAGE_GO, INJECTION_INJECTED), never the numbers: a renumbering upstream
    /// would otherwise rewrite stored history.
    pub language: String,

    pub port_tcp: Vec<i32>,
    pub port_udp: Vec<i32>,

    pub generated_service_name: String,
    pub generated_service_name_source: String,
    pub dd_service_name: String,
    pub dd_service_name_source: String,
    pub additional_generated_names: Vec<String>,
    pub additional_generated_name_sources: Vec<String>,
    pub tracer_runtime_ids: Vec<String>,
    pub tracer_service_names: Vec<String>,
    pub apm_instrumentation: u8,
    /// The repeated oneof, as JSON.
    pub service_resources: String,

    pub injection_state: String,

    pub zombie_children_count: u32,
    pub zombie_net_rate: f64,
    pub has_zombie_aggregation: u8,

    pub container_key: u32,

    /// Opaque bytes off the wire, stored verbatim.
    pub byte_key: Blob,
    pub container_byte_key: Blob,

    /// 0 means the agent sent no start time and create_time is meaningless, not 1970. The column
    /// is a plain DateTime and cannot say so itself.
    pub has_create_time: u8,

    /// Process.host as JSON: a field the backend fills in, nil from an agent.
    pub process_host: String,

    pub network_id: String,
    pub group_id: i32,
    pub group_size: i32,
    pub container_host_type: String,
    pub agent_version: String,
    pub request_id: String,
    pub host_uuid: String,
    pub os_name: String,
    pub os_platform: String,
    pub os_family: String,
    pub os_version: String,
    pub kernel_version: String,
}

impl Table for Process {
    const WRITER: &'static str = "storage_processes";
    const TABLE: &'static str = "processes";
    // A few hundred rows per host per snapshot, every ~10 s.
    const LIMITS: Limits = Limits { max_rows: 10_000, flush_interval: Duration::from_secs(5), ..Limits::DEFAULT };
}

/// One received frame, whatever its type.
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct ProcessSnapshot {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub snapshot_id: Uuid,

    pub message_type: u8,
    pub message_type_name: String,
    pub path: String,

    pub header_version: u8,
    pub header_encoding: u8,
    pub header_subscription_id: u8,
    pub header_org_id: i32,
    /// Kept as the number it is: the agent leaves it at zero and its unit is documented nowhere.
    pub header_timestamp: i64,
    pub frame_bytes: u64,

    pub host_name: String,
    pub network_id: String,
    pub group_id: i32,
    pub group_size: i32,
    pub container_host_type: String,

    pub host_id: i64,
    pub host_org_id: i32,
    pub host_display_name: String,
    pub host_all_tags: Tags,
    pub host_num_cpus: i32,
    pub host_total_memory: i64,
    pub host_tag_index: i32,
    /// "Never recomputed" and "recomputed at the epoch" are different statements about the
    /// backend's tag cache.
    #[serde(with = "column::millis::option")]
    pub host_tags_modified: Option<DateTime<Utc>>,

    pub sys_uuid: String,
    pub os_name: String,
    pub os_platform: String,
    pub os_family: String,
    pub os_version: String,
    pub kernel_version: String,
    pub sys_total_memory: i64,

    /// Nine parallel arrays, one entry per CPUInfo, in the order sent.
    pub cpu_numbers: Vec<i32>,
    pub cpu_vendors: Vec<String>,
    pub cpu_families: Vec<String>,
    pub cpu_models: Vec<String>,
    pub cpu_physical_ids: Vec<String>,
    pub cpu_core_ids: Vec<String>,
    pub cpu_cores: Vec<i32>,
    pub cpu_mhz: Vec<i64>,
    pub cpu_cache_sizes: Vec<i32>,

    /// Only CollectorProc has hints, and "no hints" differs from "mask 0".
    pub hint_mask: Option<i32>,

    pub rt_host_id: i64,
    pub rt_org_id: i32,
    pub rt_num_cpus: i32,
    pub rt_total_memory: i64,

    pub process_count: u32,
    pub process_stat_count: u32,
    pub container_count: u32,
    pub container_stat_count: u32,
    pub discovery_count: u32,
    pub connection_count: u32,

    pub agent_hostname: String,
    pub agent_version: String,
    pub agent_container_count: String,
    pub request_id: String,
}

impl Table for ProcessSnapshot {
    const WRITER: &'static str = "storage_process_snapshots";
    const TABLE: &'static str = "process_snapshots";
    // One row per frame, wide but tiny: the timer does the flushing.
    const LIMITS: Limits =
        Limits { max_rows: 500, flush_interval: Duration::from_secs(10), buffer_limit: 20_000, max_in_flight: 2 };
}

/// One ProcessStat from the 2-second realtime stream.
///
/// nice and threads are ProcessStat's own fields, not the nested CPUStat's (cpu_nice,
/// cpu_num_threads); the agent does not always fill both.
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct ProcessStat {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    #[serde(with = "clickhouse::serde::uuid")]
    pub snapshot_id: Uuid,

    pub pid: i32,
    pub key: u32,
    #[serde(with = "column::millis")]
    pub create_time: DateTime<Utc>,
    pub has_create_time: u8,

    pub nice: i32,
    pub threads: i32,
    pub open_fds: i32,

    pub mem_rss: u64,
    pub mem_vms: u64,
    pub mem_swap: u64,
    pub mem_shared: u64,
    pub mem_text: u64,
    pub mem_lib: u64,
    pub mem_data: u64,
    pub mem_dirty: u64,

    pub cpu_last_cpu: String,
    pub cpu_total_pct: f32,
    pub cpu_user_pct: f32,
    pub cpu_system_pct: f32,
    pub cpu_num_threads: i32,
    pub cpu_nice: i32,
    pub cpu_user_time: i64,
    pub cpu_system_time: i64,
    pub cpu_core_names: Vec<String>,
    pub cpu_core_pcts: Vec<f32>,

    pub io_read_rate: f32,
    pub io_write_rate: f32,
    pub io_read_bytes_rate: f32,
    pub io_write_bytes_rate: f32,

    pub net_connection_rate: f32,
    pub net_bytes_rate: f32,

    pub voluntary_ctx_switches: u64,
    pub involuntary_ctx_switches: u64,

    pub container_id: String,
    pub container_state: String,
    pub process_state: String,

    // Deprecated upstream, stored anyway: older agents send only these.
    pub container_health: String,
    pub container_rbps: f32,
    pub container_wbps: f32,
    pub container_key: u32,
    pub container_net_rcvd_ps: f32,
    pub container_net_sent_ps: f32,
    pub container_net_rcvd_bps: f32,
    pub container_net_sent_bps: f32,

    pub byte_key: Blob,
    pub container_byte_key: Blob,

    pub group_id: i32,
    pub group_size: i32,
    pub container_host_type: String,
    pub rt_host_id: i64,
    pub rt_org_id: i32,
    pub rt_num_cpus: i32,
    pub rt_total_memory: i64,
    pub agent_version: String,
    pub request_id: String,
}

impl Table for ProcessStat {
    const WRITER: &'static str = "storage_process_stats";
    const TABLE: &'static str = "process_stats";
    // The shape of processes at five times the cadence (2 s, not 10 s).
    const LIMITS: Limits = Limits { max_rows: 10_000, flush_interval: Duration::from_secs(5), ..Limits::DEFAULT };
}

/// One Container. source says which message it hung off (collector_proc, collector_container or
/// process): the same struct arrives all three ways and what is populated differs.
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct Container {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    #[serde(with = "clickhouse::serde::uuid")]
    pub snapshot_id: Uuid,

    pub source: String,
    /// The owning process, for source = "process". Zero otherwise.
    pub pid: i32,

    pub r#type: String,
    pub id: String,
    pub name: String,
    pub image: String,
    pub repo_digest: String,

    pub cpu_limit: f32,
    pub memory_limit: u64,
    pub cpu_request: f32,
    pub memory_request: u64,

    pub state: String,
    pub health: String,

    /// None when the runtime reported no time, so a container never appears to have started at
    /// the epoch.
    #[serde(with = "column::millis::option")]
    pub created: Option<DateTime<Utc>>,
    #[serde(with = "column::millis::option")]
    pub started: Option<DateTime<Utc>>,

    pub rbps: f32,
    pub wbps: f32,
    pub net_rcvd_ps: f32,
    pub net_sent_ps: f32,
    pub net_rcvd_bps: f32,
    pub net_sent_bps: f32,

    pub user_pct: f32,
    pub system_pct: f32,
    pub total_pct: f32,
    pub cpu_usage_ns: f32,

    pub mem_rss: u64,
    pub mem_cache: u64,
    pub mem_usage: u64,
    pub mem_accounted: u64,

    pub thread_count: u64,
    pub thread_limit: u64,

    pub key: u32,
    pub byte_key: Blob,

    /// Three parallel arrays, one entry per ContainerAddr, order as sent.
    pub addr_ips: Vec<String>,
    pub addr_ports: Vec<i32>,
    pub addr_protocols: Vec<String>,

    pub tags: Tags,

    /// Container.host as JSON: a field the backend fills in, nil from an agent.
    pub host_info: String,

    pub network_id: String,
    pub group_id: i32,
    pub group_size: i32,
    pub container_host_type: String,
    pub agent_version: String,
    pub request_id: String,
}

impl Table for Container {
    const WRITER: &'static str = "storage_containers";
    const TABLE: &'static str = "containers";
    // Tens to hundreds of rows per host per pass, each with tag maps and address arrays: the rows
    // carry real memory.
    const LIMITS: Limits =
        Limits { max_rows: 2_000, flush_interval: Duration::from_secs(5), buffer_limit: 50_000, max_in_flight: 2 };
}

/// One ContainerStat. source is collector_realtime or collector_container_realtime; the two
/// messages carry identical fields.
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct ContainerStat {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    #[serde(with = "clickhouse::serde::uuid")]
    pub snapshot_id: Uuid,
    pub source: String,

    pub id: String,

    pub user_pct: f32,
    pub system_pct: f32,
    pub total_pct: f32,
    pub cpu_limit: f32,
    pub cpu_request: f32,
    pub cpu_usage_ns: f32,

    pub mem_rss: u64,
    pub mem_cache: u64,
    pub mem_limit: u64,
    pub mem_usage: u64,
    pub mem_accounted: u64,
    pub memory_request: u64,

    pub rbps: f32,
    pub wbps: f32,
    pub net_rcvd_ps: f32,
    pub net_sent_ps: f32,
    pub net_rcvd_bps: f32,
    pub net_sent_bps: f32,

    pub state: String,
    pub health: String,

    pub key: u32,
    #[serde(with = "column::millis::option")]
    pub started: Option<DateTime<Utc>>,
    pub byte_key: Blob,
    pub thread_count: u64,
    pub thread_limit: u64,

    pub group_id: i32,
    pub group_size: i32,
    pub container_host_type: String,
    pub rt_host_id: i64,
    pub rt_org_id: i32,
    pub rt_num_cpus: i32,
    pub rt_total_memory: i64,
    pub agent_version: String,
    pub request_id: String,
}

impl Table for ContainerStat {
    const WRITER: &'static str = "storage_container_stats";
    const TABLE: &'static str = "container_stats";
    // Narrow numeric rows on the 2-second stream.
    const LIMITS: Limits = Limits { max_rows: 10_000, flush_interval: Duration::from_secs(5), ..Limits::DEFAULT };
}

/// One ProcessDiscovery: pid, command and user, no resource usage. On hosts that run only the
/// discovery check this is the sole process-level record.
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct ProcessDiscovery {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    #[serde(with = "clickhouse::serde::uuid")]
    pub snapshot_id: Uuid,

    pub pid: i32,
    pub ns_pid: i32,
    #[serde(with = "column::millis")]
    pub create_time: DateTime<Utc>,
    pub has_create_time: u8,
    pub byte_key: Blob,

    pub comm: String,
    pub exe: String,
    pub cmdline: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub root: String,
    pub on_disk: u8,
    pub ppid: i32,
    pub pgroup: i32,

    pub user: String,
    pub uid: i32,
    pub gid: i32,
    pub euid: i32,
    pub egid: i32,
    pub suid: i32,
    pub sgid: i32,

    pub host_info: String,

    pub group_id: i32,
    pub group_size: i32,
    pub agent_version: String,
    pub request_id: String,
}

impl Table for ProcessDiscovery {
    const WRITER: &'static str = "storage_process_discoveries";
    const TABLE: &'static str = "process_discoveries";
    // A full process list per pass, like processes, with smaller rows.
    const LIMITS: Limits = Limits { max_rows: 10_000, flush_interval: Duration::from_secs(5), ..Limits::DEFAULT };
}

/// The per-frame half of CollectorConnections: what describes the sender and its eBPF machinery
/// rather than any one connection.
///
/// payload_id is the frame's [`ProcessSnapshot::snapshot_id`] and every
/// [`Connection::payload_id`].
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct ConnectionsPayload {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub received_at: DateTime<Utc>,
    #[serde(with = "clickhouse::serde::uuid")]
    pub payload_id: Uuid,

    pub host_name: String,
    pub network_id: String,
    pub group_id: i32,
    pub group_size: i32,
    pub container_host_type: String,
    pub connection_count: u32,

    pub architecture: String,
    pub kernel_version: String,
    pub platform: String,
    pub platform_version: String,

    pub resolved_resources: String,
    pub container_for_pid: BTreeMap<i32, String>,

    /// The packed tag buffers, verbatim. Connection rows carry their tags resolved, but the
    /// encoding is versioned and one that cannot be read yet must still land somewhere.
    pub encoded_tags: Blob,
    pub encoded_connections_tags: Blob,

    pub host_tags_index: i32,
    pub host_tags: Tags,

    pub conn_telemetry: BTreeMap<String, i64>,
    pub conn_telemetry_map: BTreeMap<String, i64>,

    pub compilation_telemetry: String,
    pub kernel_header_fetch_result: String,
    pub core_telemetry: BTreeMap<String, String>,
    pub prebuilt_ebpf_assets: Vec<String>,

    pub routes: String,
    pub route_metadata: String,
    pub agent_configuration: String,

    pub encoded_dns: Blob,
    pub domains: Vec<String>,
    pub encoded_domain_database: Blob,
    pub encoded_dns_lookups: Blob,
    /// The names read out of whichever DNS encoding arrived, so the common case needs no decoder
    /// at read time.
    pub dns_names: Vec<String>,

    pub resolved_hosts_by_name: String,
    pub resolved_public_ips: String,

    pub ecs_task: String,
    pub resolv_confs: Vec<String>,

    pub agent_hostname: String,
    pub agent_version: String,
    pub agent_container_count: String,
    pub request_id: String,
}

impl Table for ConnectionsPayload {
    const WRITER: &'static str = "storage_connections_payloads";
    const TABLE: &'static str = "connections_payloads";
    // One row per frame, but each carries the packed tag and DNS buffers, kilobytes to megabytes:
    // tight ceilings, one flush in flight.
    const LIMITS: Limits =
        Limits { max_rows: 200, flush_interval: Duration::from_secs(10), buffer_limit: 5_000, max_in_flight: 1 };
}

/// One Connection from the system-probe's connection table.
///
/// The four *_aggregations fields are separately serialised protobufs of Universal Service
/// Monitoring. They are kept as bytes: a decoder can be written against stored bytes later, not
/// against discarded ones.
#[derive(clickhouse::Row, Serialize, Clone, Debug, Default)]
pub struct Connection {
    pub tenant_id: String,
    #[serde(with = "column::millis")]
    pub timestamp: DateTime<Utc>,
    pub host: String,
    #[serde(with = "clickhouse::serde::uuid")]
    pub payload_id: Uuid,

    pub pid: i32,

    pub laddr_ip: String,
    pub laddr_port: i32,
    pub laddr_container_id: String,
    pub laddr_host_name: String,
    pub raddr_ip: String,
    pub raddr_port: i32,
    pub raddr_container_id: String,
    pub raddr_host_name: String,

    pub family: String,
    pub r#type: String,
    pub direction: String,
    pub is_local_port_ephemeral: String,

    pub last_bytes_sent: u64,
    pub last_bytes_received: u64,
    pub last_packets_sent: u64,
    pub last_packets_received: u64,
    pub last_retransmits: u32,

    /// Order is the stack: [protocolTLS, protocolHTTP2] is not the reverse.
    pub protocol_stack: Vec<String>,

    pub net_ns: u32,
    pub remote_network_id: String,

    /// The conntrack NAT entry: None for every untranslated connection, which is most of them,
    /// rather than 0.0.0.0:0.
    pub ip_translation_repl_src_ip: Option<String>,
    pub ip_translation_repl_dst_ip: Option<String>,
    pub ip_translation_repl_src_port: Option<i32>,
    pub ip_translation_repl_dst_port: Option<i32>,

    pub rtt: u32,
    pub rtt_var: u32,
    pub intra_host: u8,

    pub dns_successful_responses: u32,
    pub dns_failed_responses: u32,
    pub dns_timeouts: u32,
    pub dns_success_latency_sum: u64,
    pub dns_failure_latency_sum: u64,
    pub dns_count_by_rcode: BTreeMap<u32, u32>,

    pub dns_stats_by_domain: String,
    pub dns_stats_by_domain_by_query_type: String,
    pub dns_stats_by_domain_offset_by_query_type: String,

    pub last_tcp_established: u32,
    pub last_tcp_closed: u32,

    pub route_idx: i32,
    pub route_target_idx: i32,
    pub resolv_conf_idx: i32,

    pub http_aggregations: Blob,
    pub http2_aggregations: Blob,
    pub data_streams_aggregations: Blob,
    pub database_aggregations: Blob,

    pub tags_indices: Vec<u32>,
    pub tags_idx: i32,
    pub tags_checksum: u32,
    pub tags: Tags,

    pub local_container_tags_index: i32,
    pub local_container_tags: Tags,
    pub remote_service_tags_idx: i32,
    pub remote_service_tags: Tags,

    pub state_index: u32,
    pub tcp_failures_by_err_code: BTreeMap<u32, u32>,
    pub remote_ecs_task: String,
    pub system_probe_conn: u8,

    pub last_tcp_rto_count: u32,
    pub last_tcp_recovery_count: u32,
    pub last_tcp_reord_seen: u32,
    pub last_tcp_rcv_ooo_pack: u32,
    pub last_tcp_delivered_ce: u32,
    pub last_tcp_probe0_count: u32,
    pub tcp_ecn_negotiated: u8,

    pub agent_version: String,
    pub request_id: String,
}

impl Table for Connection {
    const WRITER: &'static str = "storage_connections";
    const TABLE: &'static str = "connections";
    // The highest-volume table of this intake: a busy host reports thousands of connections per
    // frame.
    const LIMITS: Limits =
        Limits { max_rows: 20_000, flush_interval: Duration::from_secs(5), buffer_limit: 500_000, ..Limits::DEFAULT };
}
