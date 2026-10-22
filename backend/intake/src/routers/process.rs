//! process.<site> — the process-agent.
//!
//!   agent config: process_config.process_dd_url / DD_PROCESS_CONFIG_PROCESS_DD_URL
//!
//! The only intake with its own 16-byte frame instead of plain protobuf. All four paths share the
//! frame and the ResCollector reply; the message type in the frame says what the body is, not the
//! path.
//!
//!   every frame, any type      -> process_snapshots (the envelope)
//!   CollectorProc              -> processes, containers
//!   CollectorRealTime          -> process_stats, container_stats
//!   CollectorContainer         -> containers
//!   CollectorContainerRealTime -> container_stats
//!   CollectorProcDiscovery     -> process_discoveries
//!   CollectorConnections       -> connections_payloads, connections
//!
//! Every item row carries the frame's snapshot id back to process_snapshots; the connections pair
//! calls the same id payload_id. A frame that does not decode goes to raw_payloads as
//! decode_error, one that decodes into a type its route has no table for as unexpected_shape.

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::sync::LazyLock;

use chrono::{DateTime, TimeZone, Utc};
use http::{HeaderValue, StatusCode};
use ninjapaws_proto::datadog::process_agent::{
    CollectorConnections, CollectorConnectionsTelemetry, CollectorContainer, CollectorContainerRealTime, CollectorProc,
    CollectorProcDiscovery, CollectorRealTime, CollectorStatus, Connection, ConnectionDirection, ConnectionFamily,
    ConnectionType, Container, ContainerHealth, ContainerHostType, ContainerStat, ContainerState, CoreResult,
    EphemeralPortState, Host, InjectionState, KernelHeaderFetchResult, Language, Process, ProcessDiscovery,
    ProcessStat, ProcessState, ProtocolType, ResCollector, ServiceNameSource, SystemInfo, collector_proc,
    res_collector,
};
use ninjapaws_storage::column::Blob;
use ninjapaws_storage::rows::process as rows;
use ninjapaws_storage::rows::usm::ConnectionKey;
use tako::body::TakoBody;
use tako::types::Response;
use uuid::Uuid;

use super::process_buffers::{dns_buffer, tag_buffer};
use super::usm;
use crate::json;
use crate::process_frame::{self, Body, Frame};
use crate::server::{self, Ctx};
use crate::wire::{enum_name, tags, text, time};

/// The agent omits every sub-message it could not read. An empty one in its place leaves the
/// columns it feeds at zero.
fn or_empty<T: Default + Clone>(message: Option<&T>) -> Cow<'_, T> {
    message.map_or_else(|| Cow::Owned(T::default()), Cow::Borrowed)
}

/// What a row holds when the agent sent no start time: the year 1, which the table writes as the
/// epoch. The has_create_time column beside it says so.
fn no_time() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(1, 1, 1, 0, 0, 0).unwrap()
}

// ClickHouse's DateTime ends in 2106, and the driver refuses a whole batch over one value past
// that. A start time beyond the end is pinned to it.
const LAST_CREATE_MILLIS: i64 = u32::MAX as i64 * 1000;

/// A start time in Unix milliseconds, and whether there was one.
fn create_time(millis: i64) -> (DateTime<Utc>, u8) {
    if millis > 0 { (time::from_unix_millis(millis.min(LAST_CREATE_MILLIS)), 1) } else { (no_time(), 0) }
}

fn host_type(value: i32) -> String {
    enum_name(value, ContainerHostType::as_str_name)
}

/// The sender identity the process-agent stamps on every submit. Every table of this intake
/// keeps at least the version and the request id: "which agent wrote this row" is the first
/// question asked of a payload that looks wrong.
#[derive(Clone, Debug, Default)]
pub struct AgentIdentity {
    pub hostname: String,
    pub version: String,
    pub container_count: String,
    /// Only the process and connections checks send one.
    pub request_id: String,
}

/// What is known about a received frame apart from its header and body.
#[derive(Clone, Debug)]
pub struct FrameInfo {
    pub tenant: String,
    /// Minted on arrival; every row the frame produces carries it.
    pub snapshot_id: Uuid,
    /// ARRIVAL time. The body carries no sample time (create_time is a process start) and the
    /// agent leaves the header's timestamp at zero. Snapshots travel within a second of being
    /// taken.
    pub at: DateTime<Utc>,
    pub path: String,
    pub bytes: usize,
    pub agent: AgentIdentity,
}

fn frame_info(body: &[u8], ctx: &Ctx) -> FrameInfo {
    FrameInfo {
        tenant: ctx.tenant().into(),
        snapshot_id: Uuid::new_v4(),
        at: Utc::now(),
        path: ctx.path().into(),
        bytes: body.len(),
        agent: AgentIdentity {
            hostname: ctx.header("x-dd-hostname").into(),
            version: ctx.header("x-dd-processagentversion").into(),
            container_count: ctx.header("x-dd-containercount").into(),
            request_id: ctx.header("x-dd-request-id").into(),
        },
    }
}

/// The number is the contract and is stored too; the name is what a human reads.
fn message_type_name(message_type: u8) -> String {
    match message_type {
        12 => "CollectorProc".into(),
        22 => "CollectorConnections".into(),
        27 => "CollectorRealTime".into(),
        39 => "CollectorContainer".into(),
        40 => "CollectorContainerRealTime".into(),
        53 => "CollectorProcDiscovery".into(),
        other => format!("type_{other}"),
    }
}

fn with_host(host: Option<&Host>, row: &mut rows::ProcessSnapshot) {
    let Some(host) = host else { return };
    row.host_id = host.id;
    row.host_org_id = host.org_id;
    row.host_display_name = host.name.clone();
    row.host_all_tags = tags::to_multi_map(&host.all_tags);
    row.host_num_cpus = host.num_cpus;
    row.host_total_memory = host.total_memory;
    row.host_tag_index = host.tag_index;
    row.host_tags_modified = time::optional_seconds(host.tags_modified);
}

fn with_system_info(info: Option<&SystemInfo>, row: &mut rows::ProcessSnapshot) {
    let Some(info) = info else { return };
    let os = or_empty(info.os.as_ref());
    row.sys_uuid = info.uuid.clone();
    row.sys_total_memory = info.total_memory;
    row.os_name = os.name.clone();
    row.os_platform = os.platform.clone();
    row.os_family = os.family.clone();
    row.os_version = os.version.clone();
    row.kernel_version = os.kernel_version.clone();
    // Parallel arrays in the order sent. A map keyed by number would drop a host that reports two
    // sockets with the same numbering.
    let cpus = &info.cpus;
    row.cpu_numbers = cpus.iter().map(|c| c.number).collect();
    row.cpu_vendors = cpus.iter().map(|c| c.vendor.clone()).collect();
    row.cpu_families = cpus.iter().map(|c| c.family.clone()).collect();
    row.cpu_models = cpus.iter().map(|c| c.model.clone()).collect();
    row.cpu_physical_ids = cpus.iter().map(|c| c.physical_id.clone()).collect();
    row.cpu_core_ids = cpus.iter().map(|c| c.core_id.clone()).collect();
    row.cpu_cores = cpus.iter().map(|c| c.cores).collect();
    row.cpu_mhz = cpus.iter().map(|c| c.mhz).collect();
    row.cpu_cache_sizes = cpus.iter().map(|c| c.cache_size).collect();
}

/// The envelope row of one frame. The six message types spread the same information
/// differently: CollectorProc and its kin carry a Host and a SystemInfo, the realtime ones inline
/// a host id, a cpu count and a memory total instead.
pub fn snapshot_row(info: &FrameInfo, frame: &Frame) -> rows::ProcessSnapshot {
    let header = &frame.header;
    let mut row = rows::ProcessSnapshot {
        tenant_id: info.tenant.clone(),
        received_at: info.at,
        snapshot_id: info.snapshot_id,
        message_type: header.message_type,
        message_type_name: message_type_name(header.message_type),
        path: info.path.clone(),
        header_version: header.version,
        header_encoding: header.encoding,
        header_subscription_id: header.subscription_id,
        header_org_id: header.org_id,
        header_timestamp: header.timestamp,
        frame_bytes: info.bytes as u64,
        agent_hostname: info.agent.hostname.clone(),
        agent_version: info.agent.version.clone(),
        agent_container_count: info.agent.container_count.clone(),
        request_id: info.agent.request_id.clone(),
        ..Default::default()
    };

    match &frame.body {
        Body::Proc(b) => {
            with_host(b.host.as_ref(), &mut row);
            with_system_info(b.info.as_ref(), &mut row);
            row.host_name = b.host_name.clone();
            row.network_id = b.network_id.clone();
            row.group_id = b.group_id;
            row.group_size = b.group_size;
            row.container_host_type = host_type(b.container_host_type);
            row.process_count = b.processes.len() as u32;
            row.container_count = b.containers.len() as u32;
            // "No hints" and "hint mask 0" are different statements.
            row.hint_mask = b.hints.map(|collector_proc::Hints::HintMask(mask)| mask);
        }
        Body::RealTime(b) => {
            row.host_name = b.host_name.clone();
            row.group_id = b.group_id;
            row.group_size = b.group_size;
            row.container_host_type = host_type(b.container_host_type);
            row.rt_host_id = b.host_id;
            row.rt_org_id = b.org_id;
            row.rt_num_cpus = b.num_cpus;
            row.rt_total_memory = b.total_memory;
            row.process_stat_count = b.stats.len() as u32;
            row.container_stat_count = b.container_stats.len() as u32;
        }
        Body::Container(b) => {
            with_host(b.host.as_ref(), &mut row);
            with_system_info(b.info.as_ref(), &mut row);
            row.host_name = b.host_name.clone();
            row.network_id = b.network_id.clone();
            row.group_id = b.group_id;
            row.group_size = b.group_size;
            row.container_host_type = host_type(b.container_host_type);
            row.container_count = b.containers.len() as u32;
        }
        Body::ContainerRealTime(b) => {
            row.host_name = b.host_name.clone();
            row.group_id = b.group_id;
            row.group_size = b.group_size;
            row.container_host_type = host_type(b.container_host_type);
            row.rt_host_id = b.host_id;
            row.rt_num_cpus = b.num_cpus;
            row.rt_total_memory = b.total_memory;
            row.container_stat_count = b.stats.len() as u32;
        }
        Body::ProcDiscovery(b) => {
            with_host(b.host.as_ref(), &mut row);
            row.host_name = b.host_name.clone();
            row.group_id = b.group_id;
            row.group_size = b.group_size;
            row.discovery_count = b.process_discoveries.len() as u32;
        }
        Body::Connections(b) => {
            row.host_name = b.host_name.clone();
            row.network_id = b.network_id.clone();
            row.group_id = b.group_id;
            row.group_size = b.group_size;
            row.container_host_type = host_type(b.container_host_type);
            row.connection_count = b.connections.len() as u32;
        }
        _ => {}
    }
    row
}

/// One process of a snapshot. Every field of Process reaches a column; Process.host goes to a
/// JSON column and Process.container to the containers table.
pub fn process_row(info: &FrameInfo, proc: &CollectorProc, p: &Process) -> rows::Process {
    let command = or_empty(p.command.as_ref());
    let user = or_empty(p.user.as_ref());
    let memory = or_empty(p.memory.as_ref());
    let cpu = or_empty(p.cpu.as_ref());
    let io = or_empty(p.io_stat.as_ref());
    let networks = or_empty(p.networks.as_ref());
    let ports = or_empty(p.port_info.as_ref());
    // The bridge between a process and APM: tracer_runtime_ids is the only join between "this pid
    // on this host" and the traces it emitted.
    let discovery = or_empty(p.service_discovery.as_ref());
    let generated_name = discovery.generated_service_name.as_ref();
    let dd_name = discovery.dd_service_name.as_ref();
    let source_name = |source: i32| enum_name(source, ServiceNameSource::as_str_name);
    let system = or_empty(proc.info.as_ref());
    let os = or_empty(system.os.as_ref());
    let (created, has_create_time) = create_time(p.create_time);

    rows::Process {
        tenant_id: info.tenant.clone(),
        timestamp: info.at,
        host: proc.host_name.clone(),
        pid: p.pid,
        ppid: command.ppid,
        user: user.name.clone(),
        comm: command.comm.clone(),
        exe: command.exe.clone(),
        cmdline: command.args.join(" "),
        rss: memory.rss,
        vms: memory.vms,
        cpu_pct: cpu.total_pct,
        threads: cpu.num_threads,
        open_fds: p.open_fd_count,
        state: enum_name(p.state, ProcessState::as_str_name),
        create_time: created,
        container_id: p.container_id.clone(),
        tags: tags::to_multi_map(&p.tags),
        snapshot_id: info.snapshot_id,
        ns_pid: p.ns_pid,
        key: p.key,
        cwd: command.cwd.clone(),
        root: command.root.clone(),
        on_disk: text::flag(command.on_disk),
        pgroup: command.pgroup,
        args: command.args.clone(),
        uid: user.uid,
        gid: user.gid,
        euid: user.euid,
        egid: user.egid,
        suid: user.suid,
        sgid: user.sgid,
        mem_swap: memory.swap,
        mem_shared: memory.shared,
        mem_text: memory.text,
        mem_lib: memory.lib,
        mem_data: memory.data,
        mem_dirty: memory.dirty,
        cpu_last_cpu: cpu.last_cpu.clone(),
        cpu_user_pct: cpu.user_pct,
        cpu_system_pct: cpu.system_pct,
        cpu_nice: cpu.nice,
        cpu_user_time: cpu.user_time,
        cpu_system_time: cpu.system_time,
        cpu_core_names: cpu.cpus.iter().map(|c| c.name.clone()).collect(),
        cpu_core_pcts: cpu.cpus.iter().map(|c| c.total_pct).collect(),
        io_read_rate: io.read_rate,
        io_write_rate: io.write_rate,
        io_read_bytes_rate: io.read_bytes_rate,
        io_write_bytes_rate: io.write_bytes_rate,
        voluntary_ctx_switches: p.voluntary_ctx_switches,
        involuntary_ctx_switches: p.involuntary_ctx_switches,
        net_connection_rate: networks.connection_rate,
        net_bytes_rate: networks.bytes_rate,
        process_context: p.process_context.clone(),
        language: enum_name(p.language, Language::as_str_name),
        port_tcp: ports.tcp.clone(),
        port_udp: ports.udp.clone(),
        generated_service_name: generated_name.map(|n| n.name.clone()).unwrap_or_default(),
        generated_service_name_source: generated_name.map(|n| source_name(n.source)).unwrap_or_default(),
        dd_service_name: dd_name.map(|n| n.name.clone()).unwrap_or_default(),
        dd_service_name_source: dd_name.map(|n| source_name(n.source)).unwrap_or_default(),
        // Parallel arrays, in the order sent: name i came from source i.
        additional_generated_names: discovery.additional_generated_names.iter().map(|n| n.name.clone()).collect(),
        additional_generated_name_sources: discovery
            .additional_generated_names
            .iter()
            .map(|n| source_name(n.source))
            .collect(),
        tracer_runtime_ids: discovery.tracer_metadata.iter().map(|t| t.runtime_id.clone()).collect(),
        tracer_service_names: discovery.tracer_metadata.iter().map(|t| t.service_name.clone()).collect(),
        apm_instrumentation: text::flag(discovery.apm_instrumentation),
        service_resources: json::proto::messages(&discovery.resources),
        injection_state: enum_name(p.injection_state, InjectionState::as_str_name),
        zombie_children_count: p.zombie_children_count,
        zombie_net_rate: p.zombie_net_rate,
        has_zombie_aggregation: text::flag(p.has_zombie_aggregation),
        container_key: p.container_key,
        byte_key: Blob(p.byte_key.clone()),
        container_byte_key: Blob(p.container_byte_key.clone()),
        has_create_time,
        process_host: json::proto::message(p.host.as_ref()),
        network_id: proc.network_id.clone(),
        group_id: proc.group_id,
        group_size: proc.group_size,
        container_host_type: host_type(proc.container_host_type),
        agent_version: info.agent.version.clone(),
        request_id: info.agent.request_id.clone(),
        host_uuid: system.uuid.clone(),
        os_name: os.name.clone(),
        os_platform: os.platform.clone(),
        os_family: os.family.clone(),
        os_version: os.version.clone(),
        kernel_version: os.kernel_version.clone(),
    }
}

pub fn process_stat_row(info: &FrameInfo, real_time: &CollectorRealTime, s: &ProcessStat) -> rows::ProcessStat {
    let memory = or_empty(s.memory.as_ref());
    let cpu = or_empty(s.cpu.as_ref());
    let io = or_empty(s.io_stat.as_ref());
    let networks = or_empty(s.networks.as_ref());
    let (created, has_create_time) = create_time(s.create_time);

    rows::ProcessStat {
        tenant_id: info.tenant.clone(),
        timestamp: info.at,
        host: real_time.host_name.clone(),
        snapshot_id: info.snapshot_id,
        pid: s.pid,
        key: s.key,
        create_time: created,
        has_create_time,
        // The message's own fields, not the nested CPUStat's.
        nice: s.nice,
        threads: s.threads,
        open_fds: s.open_fd_count,
        mem_rss: memory.rss,
        mem_vms: memory.vms,
        mem_swap: memory.swap,
        mem_shared: memory.shared,
        mem_text: memory.text,
        mem_lib: memory.lib,
        mem_data: memory.data,
        mem_dirty: memory.dirty,
        cpu_last_cpu: cpu.last_cpu.clone(),
        cpu_total_pct: cpu.total_pct,
        cpu_user_pct: cpu.user_pct,
        cpu_system_pct: cpu.system_pct,
        cpu_num_threads: cpu.num_threads,
        cpu_nice: cpu.nice,
        cpu_user_time: cpu.user_time,
        cpu_system_time: cpu.system_time,
        cpu_core_names: cpu.cpus.iter().map(|c| c.name.clone()).collect(),
        cpu_core_pcts: cpu.cpus.iter().map(|c| c.total_pct).collect(),
        io_read_rate: io.read_rate,
        io_write_rate: io.write_rate,
        io_read_bytes_rate: io.read_bytes_rate,
        io_write_bytes_rate: io.write_bytes_rate,
        net_connection_rate: networks.connection_rate,
        net_bytes_rate: networks.bytes_rate,
        voluntary_ctx_switches: s.voluntary_ctx_switches,
        involuntary_ctx_switches: s.involuntary_ctx_switches,
        container_id: s.container_id.clone(),
        container_state: enum_name(s.container_state, ContainerState::as_str_name),
        process_state: enum_name(s.process_state, ProcessState::as_str_name),
        container_health: enum_name(s.container_health, ContainerHealth::as_str_name),
        container_rbps: s.container_rbps,
        container_wbps: s.container_wbps,
        container_key: s.container_key,
        container_net_rcvd_ps: s.container_net_rcvd_ps,
        container_net_sent_ps: s.container_net_sent_ps,
        container_net_rcvd_bps: s.container_net_rcvd_bps,
        container_net_sent_bps: s.container_net_sent_bps,
        byte_key: Blob(s.byte_key.clone()),
        container_byte_key: Blob(s.container_byte_key.clone()),
        group_id: real_time.group_id,
        group_size: real_time.group_size,
        container_host_type: host_type(real_time.container_host_type),
        rt_host_id: real_time.host_id,
        rt_org_id: real_time.org_id,
        rt_num_cpus: real_time.num_cpus,
        rt_total_memory: real_time.total_memory,
        agent_version: info.agent.version.clone(),
        request_id: info.agent.request_id.clone(),
    }
}

/// What a container row inherits from the message it hung off. The same Container arrives three
/// ways, and what is populated differs.
#[derive(Clone, Debug, Default)]
pub struct ContainerEnvelope {
    /// collector_proc (the snapshot's inventory), process (the copy on one Process) or
    /// collector_container.
    pub source: String,
    /// The owning process, for source = "process".
    pub pid: i32,
    pub network_id: String,
    pub group_id: i32,
    pub group_size: i32,
    pub container_host_type: String,
}

pub fn container_row(info: &FrameInfo, envelope: &ContainerEnvelope, host: &str, c: &Container) -> rows::Container {
    rows::Container {
        tenant_id: info.tenant.clone(),
        timestamp: info.at,
        host: host.into(),
        snapshot_id: info.snapshot_id,
        source: envelope.source.clone(),
        pid: envelope.pid,
        r#type: c.r#type.clone(),
        id: c.id.clone(),
        name: c.name.clone(),
        image: c.image.clone(),
        repo_digest: c.repo_digest.clone(),
        cpu_limit: c.cpu_limit,
        memory_limit: c.memory_limit,
        cpu_request: c.cpu_request,
        memory_request: c.memory_request,
        state: enum_name(c.state, ContainerState::as_str_name),
        health: enum_name(c.health, ContainerHealth::as_str_name),
        // Unix SECONDS on the wire: the agent sends CreatedAt.Unix().
        created: time::optional_seconds(c.created),
        started: time::optional_seconds(c.started),
        rbps: c.rbps,
        wbps: c.wbps,
        net_rcvd_ps: c.net_rcvd_ps,
        net_sent_ps: c.net_sent_ps,
        net_rcvd_bps: c.net_rcvd_bps,
        net_sent_bps: c.net_sent_bps,
        user_pct: c.user_pct,
        system_pct: c.system_pct,
        total_pct: c.total_pct,
        cpu_usage_ns: c.cpu_usage_ns,
        mem_rss: c.mem_rss,
        mem_cache: c.mem_cache,
        mem_usage: c.mem_usage,
        mem_accounted: c.mem_accounted,
        thread_count: c.thread_count,
        thread_limit: c.thread_limit,
        key: c.key,
        byte_key: Blob(c.byte_key.clone()),
        // Parallel arrays, order as sent: a container legitimately binds the same port on two
        // addresses.
        addr_ips: c.addresses.iter().map(|a| a.ip.clone()).collect(),
        addr_ports: c.addresses.iter().map(|a| a.port).collect(),
        addr_protocols: c.addresses.iter().map(|a| enum_name(a.protocol, ConnectionType::as_str_name)).collect(),
        tags: tags::to_multi_map(&c.tags),
        host_info: json::proto::message(c.host.as_ref()),
        network_id: envelope.network_id.clone(),
        group_id: envelope.group_id,
        group_size: envelope.group_size,
        container_host_type: envelope.container_host_type.clone(),
        agent_version: info.agent.version.clone(),
        request_id: info.agent.request_id.clone(),
    }
}

/// Both places a Container hides inside a CollectorProc: the snapshot's inventory, and the copy
/// on an individual Process. The per-process one carries limits and health the inventory
/// sometimes lacks, and may be the only record of a container the inventory missed.
pub fn proc_container_rows(info: &FrameInfo, proc: &CollectorProc) -> Vec<rows::Container> {
    let inventory = ContainerEnvelope {
        source: "collector_proc".into(),
        pid: 0,
        network_id: proc.network_id.clone(),
        group_id: proc.group_id,
        group_size: proc.group_size,
        container_host_type: host_type(proc.container_host_type),
    };
    let from_inventory = proc.containers.iter().map(|c| container_row(info, &inventory, &proc.host_name, c));
    let from_processes = proc.processes.iter().filter_map(|p| {
        let envelope = ContainerEnvelope { source: "process".into(), pid: p.pid, ..inventory.clone() };
        p.container.as_ref().map(|c| container_row(info, &envelope, &proc.host_name, c))
    });
    from_inventory.chain(from_processes).collect()
}

fn inventory_container_rows(info: &FrameInfo, inventory: &CollectorContainer) -> Vec<rows::Container> {
    let envelope = ContainerEnvelope {
        source: "collector_container".into(),
        pid: 0,
        network_id: inventory.network_id.clone(),
        group_id: inventory.group_id,
        group_size: inventory.group_size,
        container_host_type: host_type(inventory.container_host_type),
    };
    inventory.containers.iter().map(|c| container_row(info, &envelope, &inventory.host_name, c)).collect()
}

/// What a container stat row inherits from its message.
#[derive(Clone, Debug, Default)]
pub struct ContainerStatEnvelope {
    /// collector_realtime or collector_container_realtime.
    pub source: String,
    pub host: String,
    pub group_id: i32,
    pub group_size: i32,
    pub container_host_type: String,
    pub rt_host_id: i64,
    pub rt_org_id: i32,
    pub rt_num_cpus: i32,
    pub rt_total_memory: i64,
}

pub fn container_stat_row(
    info: &FrameInfo,
    envelope: &ContainerStatEnvelope,
    s: &ContainerStat,
) -> rows::ContainerStat {
    rows::ContainerStat {
        tenant_id: info.tenant.clone(),
        timestamp: info.at,
        host: envelope.host.clone(),
        snapshot_id: info.snapshot_id,
        source: envelope.source.clone(),
        id: s.id.clone(),
        user_pct: s.user_pct,
        system_pct: s.system_pct,
        total_pct: s.total_pct,
        cpu_limit: s.cpu_limit,
        cpu_request: s.cpu_request,
        cpu_usage_ns: s.cpu_usage_ns,
        mem_rss: s.mem_rss,
        mem_cache: s.mem_cache,
        mem_limit: s.mem_limit,
        mem_usage: s.mem_usage,
        mem_accounted: s.mem_accounted,
        memory_request: s.memory_request,
        rbps: s.rbps,
        wbps: s.wbps,
        net_rcvd_ps: s.net_rcvd_ps,
        net_sent_ps: s.net_sent_ps,
        net_rcvd_bps: s.net_rcvd_bps,
        net_sent_bps: s.net_sent_bps,
        state: enum_name(s.state, ContainerState::as_str_name),
        health: enum_name(s.health, ContainerHealth::as_str_name),
        key: s.key,
        started: time::optional_seconds(s.started),
        byte_key: Blob(s.byte_key.clone()),
        thread_count: s.thread_count,
        thread_limit: s.thread_limit,
        group_id: envelope.group_id,
        group_size: envelope.group_size,
        container_host_type: envelope.container_host_type.clone(),
        rt_host_id: envelope.rt_host_id,
        rt_org_id: envelope.rt_org_id,
        rt_num_cpus: envelope.rt_num_cpus,
        rt_total_memory: envelope.rt_total_memory,
        agent_version: info.agent.version.clone(),
        request_id: info.agent.request_id.clone(),
    }
}

fn real_time_container_stat_rows(info: &FrameInfo, real_time: &CollectorRealTime) -> Vec<rows::ContainerStat> {
    let envelope = ContainerStatEnvelope {
        source: "collector_realtime".into(),
        host: real_time.host_name.clone(),
        group_id: real_time.group_id,
        group_size: real_time.group_size,
        container_host_type: host_type(real_time.container_host_type),
        rt_host_id: real_time.host_id,
        rt_org_id: real_time.org_id,
        rt_num_cpus: real_time.num_cpus,
        rt_total_memory: real_time.total_memory,
    };
    real_time.container_stats.iter().map(|s| container_stat_row(info, &envelope, s)).collect()
}

fn container_real_time_stat_rows(info: &FrameInfo, real_time: &CollectorContainerRealTime) -> Vec<rows::ContainerStat> {
    let envelope = ContainerStatEnvelope {
        source: "collector_container_realtime".into(),
        host: real_time.host_name.clone(),
        group_id: real_time.group_id,
        group_size: real_time.group_size,
        container_host_type: host_type(real_time.container_host_type),
        rt_host_id: real_time.host_id,
        // CollectorContainerRealTime carries no org id.
        rt_org_id: 0,
        rt_num_cpus: real_time.num_cpus,
        rt_total_memory: real_time.total_memory,
    };
    real_time.stats.iter().map(|s| container_stat_row(info, &envelope, s)).collect()
}

pub fn discovery_row(
    info: &FrameInfo,
    discoveries: &CollectorProcDiscovery,
    d: &ProcessDiscovery,
) -> rows::ProcessDiscovery {
    let command = or_empty(d.command.as_ref());
    let user = or_empty(d.user.as_ref());
    let (created, has_create_time) = create_time(d.create_time);

    rows::ProcessDiscovery {
        tenant_id: info.tenant.clone(),
        timestamp: info.at,
        host: discoveries.host_name.clone(),
        snapshot_id: info.snapshot_id,
        pid: d.pid,
        ns_pid: d.ns_pid,
        create_time: created,
        has_create_time,
        byte_key: Blob(d.byte_key.clone()),
        comm: command.comm.clone(),
        exe: command.exe.clone(),
        cmdline: command.args.join(" "),
        args: command.args.clone(),
        cwd: command.cwd.clone(),
        root: command.root.clone(),
        on_disk: text::flag(command.on_disk),
        ppid: command.ppid,
        pgroup: command.pgroup,
        user: user.name.clone(),
        uid: user.uid,
        gid: user.gid,
        euid: user.euid,
        egid: user.egid,
        suid: user.suid,
        sgid: user.sgid,
        host_info: json::proto::message(d.host.as_ref()),
        group_id: discoveries.group_id,
        group_size: discoveries.group_size,
        agent_version: info.agent.version.clone(),
        request_id: info.agent.request_id.clone(),
    }
}

/// The fixed telemetry message agents before 7.35 sent, in the name/value shape of the
/// open-ended map newer ones send, so both eras read the same way. Empty when the agent sent
/// none, not eleven zeros.
pub fn conn_telemetry(telemetry: Option<&CollectorConnectionsTelemetry>) -> BTreeMap<String, i64> {
    let Some(t) = telemetry else {
        return BTreeMap::new();
    };
    [
        ("kprobes_triggered", t.kprobes_triggered),
        ("kprobes_missed", t.kprobes_missed),
        ("conntrack_registers", t.conntrack_registers),
        ("conntrack_registers_dropped", t.conntrack_registers_dropped),
        ("dns_packets_processed", t.dns_packets_processed),
        ("conns_closed", t.conns_closed),
        ("conns_bpf_map_size", t.conns_bpf_map_size),
        ("udp_sends_processed", t.udp_sends_processed),
        ("udp_sends_missed", t.udp_sends_missed),
        ("conntrack_sampling_percent", t.conntrack_sampling_percent),
        ("dns_stats_dropped", t.dns_stats_dropped),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_string(), value))
    .collect()
}

/// A tag set of the payload's encodedTags: the host's, a container's, a remote service's.
///
/// The agent puts a set of one tag, "-", first in that buffer, so that no real set has index 0
/// (pkg/process/checks/net.go). An index that was not sent reads as 0 and lands on it: that set
/// means "none".
fn shared_tags(encoded_tags: &[u8], index: i32) -> Vec<String> {
    let tags = tag_buffer::tags(encoded_tags, index);
    if tags == ["-"] { Vec::new() } else { tags }
}

/// A protobuf map as the sorted map its column is.
fn sorted<K: Ord + Clone, V: Clone>(map: &std::collections::HashMap<K, V>) -> BTreeMap<K, V> {
    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// The per-frame half of a connections payload. `dns_names` is what [`dns_buffer::names`] made
/// of the payload's DNS buffers.
pub fn connections_payload_row(
    info: &FrameInfo,
    conns: &CollectorConnections,
    dns_names: Vec<String>,
) -> rows::ConnectionsPayload {
    rows::ConnectionsPayload {
        tenant_id: info.tenant.clone(),
        received_at: info.at,
        payload_id: info.snapshot_id,
        host_name: conns.host_name.clone(),
        network_id: conns.network_id.clone(),
        group_id: conns.group_id,
        group_size: conns.group_size,
        container_host_type: host_type(conns.container_host_type),
        connection_count: conns.connections.len() as u32,
        architecture: conns.architecture.clone(),
        kernel_version: conns.kernel_version.clone(),
        platform: conns.platform.clone(),
        platform_version: conns.platform_version.clone(),
        resolved_resources: json::proto::message_map(&conns.resolved_resources),
        container_for_pid: sorted(&conns.container_for_pid),
        encoded_tags: Blob(conns.encoded_tags.clone()),
        encoded_connections_tags: Blob(conns.encoded_connections_tags.clone()),
        host_tags_index: conns.host_tags_index,
        host_tags: tags::to_multi_map(shared_tags(&conns.encoded_tags, conns.host_tags_index)),
        conn_telemetry: conn_telemetry(conns.conn_telemetry.as_ref()),
        conn_telemetry_map: sorted(&conns.conn_telemetry_map),
        compilation_telemetry: json::proto::message_map(&conns.compilation_telemetry_by_asset),
        kernel_header_fetch_result: enum_name(conns.kernel_header_fetch_result, KernelHeaderFetchResult::as_str_name),
        // The CO-RE result per asset, by name.
        core_telemetry: conns
            .core_telemetry_by_asset
            .iter()
            .map(|(asset, result)| (asset.clone(), enum_name(*result, CoreResult::as_str_name)))
            .collect(),
        prebuilt_ebpf_assets: conns.prebuilt_ebpf_assets.clone(),
        routes: json::proto::messages(&conns.routes),
        route_metadata: json::proto::messages(&conns.route_metadata),
        agent_configuration: json::proto::message(conns.agent_configuration.as_ref()),
        encoded_dns: Blob(conns.encoded_dns.clone()),
        domains: conns.domains.clone(),
        encoded_domain_database: Blob(conns.encoded_domain_database.clone()),
        encoded_dns_lookups: Blob(conns.encoded_dns_lookups.clone()),
        dns_names,
        resolved_hosts_by_name: json::proto::message_map(&conns.resolved_hosts_by_name),
        resolved_public_ips: json::proto::message_map(&conns.resolved_public_ips),
        ecs_task: conns.ecs_task.clone(),
        resolv_confs: conns.resolv_confs.clone(),
        agent_hostname: info.agent.hostname.clone(),
        agent_version: info.agent.version.clone(),
        agent_container_count: info.agent.container_count.clone(),
        request_id: info.agent.request_id.clone(),
    }
}

/// One connection. Its own tags index the payload's encodedConnectionsTags, where 0 is a real set
/// and the agent writes -1 for "no tags"; its container's and the remote service's index
/// encodedTags. The indices are kept beside the result.
pub fn connection_row(
    info: &FrameInfo,
    host: &str,
    encoded_tags: &[u8],
    connection_tags: &[u8],
    c: &Connection,
) -> rows::Connection {
    let local = or_empty(c.laddr.as_ref());
    let remote = or_empty(c.raddr.as_ref());
    let translation = c.ip_translation.as_ref();

    rows::Connection {
        tenant_id: info.tenant.clone(),
        timestamp: info.at,
        host: host.into(),
        payload_id: info.snapshot_id,
        pid: c.pid,
        laddr_ip: local.ip.clone(),
        laddr_port: local.port,
        laddr_container_id: local.container_id.clone(),
        laddr_host_name: local.host_name.clone(),
        raddr_ip: remote.ip.clone(),
        raddr_port: remote.port,
        raddr_container_id: remote.container_id.clone(),
        raddr_host_name: remote.host_name.clone(),
        family: enum_name(c.family, ConnectionFamily::as_str_name),
        r#type: enum_name(c.r#type, ConnectionType::as_str_name),
        direction: enum_name(c.direction, ConnectionDirection::as_str_name),
        is_local_port_ephemeral: enum_name(c.is_local_port_ephemeral, EphemeralPortState::as_str_name),
        last_bytes_sent: c.last_bytes_sent,
        last_bytes_received: c.last_bytes_received,
        last_packets_sent: c.last_packets_sent,
        last_packets_received: c.last_packets_received,
        last_retransmits: c.last_retransmits,
        protocol_stack: c
            .protocol
            .as_ref()
            .map(|p| p.stack.iter().map(|t| enum_name(*t, ProtocolType::as_str_name)).collect())
            .unwrap_or_default(),
        net_ns: c.net_ns,
        remote_network_id: c.remote_network_id.clone(),
        ip_translation_repl_src_ip: translation.map(|t| t.repl_src_ip.clone()),
        ip_translation_repl_dst_ip: translation.map(|t| t.repl_dst_ip.clone()),
        ip_translation_repl_src_port: translation.map(|t| t.repl_src_port),
        ip_translation_repl_dst_port: translation.map(|t| t.repl_dst_port),
        rtt: c.rtt,
        rtt_var: c.rtt_var,
        intra_host: text::flag(c.intra_host),
        dns_successful_responses: c.dns_successful_responses,
        dns_failed_responses: c.dns_failed_responses,
        dns_timeouts: c.dns_timeouts,
        dns_success_latency_sum: c.dns_success_latency_sum,
        dns_failure_latency_sum: c.dns_failure_latency_sum,
        dns_count_by_rcode: sorted(&c.dns_count_by_rcode),
        dns_stats_by_domain: json::proto::message_map(&c.dns_stats_by_domain),
        dns_stats_by_domain_by_query_type: json::proto::message_map(&c.dns_stats_by_domain_by_query_type),
        dns_stats_by_domain_offset_by_query_type: json::proto::message_map(&c.dns_stats_by_domain_offset_by_query_type),
        last_tcp_established: c.last_tcp_established,
        last_tcp_closed: c.last_tcp_closed,
        route_idx: c.route_idx,
        route_target_idx: c.route_target_idx,
        resolv_conf_idx: c.resolv_conf_idx,
        http_aggregations: Blob(c.http_aggregations.clone()),
        http2_aggregations: Blob(c.http2_aggregations.clone()),
        data_streams_aggregations: Blob(c.data_streams_aggregations.clone()),
        database_aggregations: Blob(c.database_aggregations.clone()),
        tags_indices: c.tags.clone(),
        tags_idx: c.tags_idx,
        tags_checksum: c.tags_checksum,
        tags: tags::to_multi_map(tag_buffer::tags(connection_tags, c.tags_idx)),
        local_container_tags_index: c.local_container_tags_index,
        local_container_tags: tags::to_multi_map(shared_tags(encoded_tags, c.local_container_tags_index)),
        remote_service_tags_idx: c.remote_service_tags_idx,
        remote_service_tags: tags::to_multi_map(shared_tags(encoded_tags, c.remote_service_tags_idx)),
        state_index: c.state_index,
        tcp_failures_by_err_code: sorted(&c.tcp_failures_by_err_code),
        remote_ecs_task: c.remote_ecs_task.clone(),
        system_probe_conn: text::flag(c.system_probe_conn),
        last_tcp_rto_count: c.last_tcp_rto_count,
        last_tcp_recovery_count: c.last_tcp_recovery_count,
        last_tcp_reord_seen: c.last_tcp_reord_seen,
        last_tcp_rcv_ooo_pack: c.last_tcp_rcv_ooo_pack,
        last_tcp_delivered_ce: c.last_tcp_delivered_ce,
        last_tcp_probe0_count: c.last_tcp_probe0_count,
        tcp_ecn_negotiated: text::flag(c.tcp_ecn_negotiated),
        agent_version: info.agent.version.clone(),
        request_id: info.agent.request_id.clone(),
    }
}

/// The cadence, in seconds, the agent is told to keep for its process checks. It matches the
/// agent's own default.
pub const CHECK_INTERVAL: i32 = 10;

/// What every route answers, whatever became of the frame.
///
/// Status is NOT optional in practice, whatever the schema says: the agent feeds every reply into
/// CheckRunner.UpdateRTStatus, which indexes the statuses it gathered without checking that it got
/// any, so a reply without one makes the agent dereference nil and crash-loop its container. This
/// was observed against a real DaemonSet.
///
/// active_clients 0 means nobody is watching a live process view, which keeps the agent in its
/// normal cadence instead of real-time mode.
pub static REPLY_FRAME: LazyLock<Vec<u8>> = LazyLock::new(|| {
    let body = ResCollector {
        header: Some(res_collector::Header { r#type: 23 }),
        status: Some(CollectorStatus { active_clients: 0, interval: CHECK_INTERVAL }),
        ..Default::default()
    };
    process_frame::encode(23, 0, &body)
});

fn reply() -> Response {
    let mut response = http::Response::new(TakoBody::from(REPLY_FRAME.clone()));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(http::header::CONTENT_TYPE, HeaderValue::from_static("application/x-protobuf"));
    response
}

/// The frame of a request. One that cannot be read is kept whole: it is exactly the payload that
/// would explain why, and it arrives once.
fn decode(body: &[u8], ctx: &Ctx, label: &str) -> Option<Frame> {
    match process_frame::decode(body) {
        Ok(frame) => Some(frame),
        Err(error) => {
            ctx.raw("process", "decode_error", &format!("{label}: {error}"), body);
            None
        }
    }
}

/// The frame's type byte decides the body, not the path, so the two can disagree: a
/// misconfigured agent, or a type not met before.
fn unexpected(body: &[u8], ctx: &Ctx, label: &str, frame: &Frame) {
    let note = format!("/api/v1/{label} decoded to {} (frame type {})", frame.body.name(), frame.header.message_type);
    ctx.raw("process", "unexpected_shape", &note, body);
}

/// The process check. One path, two message types: 12 (CollectorProc, the full snapshot every
/// 10 s) and 27 (CollectorRealTime, stats only, every 2 s).
pub async fn handle_collector(ctx: Ctx, body: server::Body) -> Response {
    if let Some(frame) = decode(&body, &ctx, "collector") {
        let info = frame_info(&body, &ctx);
        match &frame.body {
            Body::Proc(proc) => {
                ctx.write(vec![snapshot_row(&info, &frame)]);
                ctx.write(proc.processes.iter().map(|p| process_row(&info, proc, p)).collect());
                ctx.write(proc_container_rows(&info, proc));
            }
            Body::RealTime(real_time) => {
                ctx.write(vec![snapshot_row(&info, &frame)]);
                ctx.write(real_time.stats.iter().map(|s| process_stat_row(&info, real_time, s)).collect());
                ctx.write(real_time_container_stat_rows(&info, real_time));
            }
            _ => unexpected(&body, &ctx, "collector", &frame),
        }
    }
    reply()
}

/// The container check. One path, two message types: 39 (CollectorContainer, the full inventory
/// every 10 s) and 40 (CollectorContainerRealTime, stats only, every 2 s).
pub async fn handle_container(ctx: Ctx, body: server::Body) -> Response {
    if let Some(frame) = decode(&body, &ctx, "container") {
        let info = frame_info(&body, &ctx);
        match &frame.body {
            Body::Container(inventory) => {
                ctx.write(vec![snapshot_row(&info, &frame)]);
                ctx.write(inventory_container_rows(&info, inventory));
            }
            Body::ContainerRealTime(real_time) => {
                ctx.write(vec![snapshot_row(&info, &frame)]);
                ctx.write(container_real_time_stat_rows(&info, real_time));
            }
            _ => unexpected(&body, &ctx, "container", &frame),
        }
    }
    reply()
}

/// What system-probe saw inside each connection, per HTTP endpoint, Kafka topic and database
/// operation. A message that does not parse is logged and gives no rows; its bytes are on the
/// connection's own row.
fn store_usm(ctx: &Ctx, info: &FrameInfo, conns: &CollectorConnections) {
    let mut http = Vec::new();
    let mut kafka = Vec::new();
    let mut database = Vec::new();

    fn take<T>(target: &mut Vec<T>, decoded: Result<Vec<T>, String>) {
        match decoded {
            Ok(rows) => target.extend(rows),
            Err(error) => tracing::warn!("[connections] {error}"),
        }
    }

    for c in &conns.connections {
        let local = or_empty(c.laddr.as_ref());
        let remote = or_empty(c.raddr.as_ref());
        let key = ConnectionKey {
            tenant_id: info.tenant.clone(),
            timestamp: info.at,
            host: conns.host_name.clone(),
            payload_id: info.snapshot_id,
            pid: c.pid,
            laddr_ip: local.ip.clone(),
            laddr_port: local.port,
            raddr_ip: remote.ip.clone(),
            raddr_port: remote.port,
        };
        take(&mut http, usm::http(&key, &c.http_aggregations));
        take(&mut http, usm::http2(&key, &c.http2_aggregations));
        take(&mut kafka, usm::kafka(&key, &c.data_streams_aggregations));
        take(&mut database, usm::database(&key, &c.database_aggregations));
    }

    ctx.write(http);
    ctx.write(kafka);
    ctx.write(database);
}

/// The network check (type 22): the system-probe's connection table, with DNS, routes and eBPF
/// telemetry. The payload row goes first, then one row per connection.
pub async fn handle_connections(ctx: Ctx, body: server::Body) -> Response {
    if let Some(frame) = decode(&body, &ctx, "connections") {
        match &frame.body {
            Body::Connections(conns) => {
                let info = frame_info(&body, &ctx);
                let dns_names =
                    dns_buffer::names(&conns.encoded_dns, &conns.encoded_domain_database).unwrap_or_else(|error| {
                        // The buffers are stored either way.
                        tracing::warn!("[connections] DNS names not read: {error}");
                        Vec::new()
                    });

                ctx.write(vec![snapshot_row(&info, &frame)]);
                ctx.write(vec![connections_payload_row(&info, conns, dns_names)]);
                ctx.write(
                    conns
                        .connections
                        .iter()
                        .map(|c| {
                            connection_row(
                                &info,
                                &conns.host_name,
                                &conns.encoded_tags,
                                &conns.encoded_connections_tags,
                                c,
                            )
                        })
                        .collect(),
                );
                store_usm(&ctx, &info, conns);
            }
            _ => unexpected(&body, &ctx, "connections", &frame),
        }
    }
    reply()
}

/// The lightweight process discovery check (type 53): pid, command and user only, no resource
/// usage.
pub async fn handle_discovery(ctx: Ctx, body: server::Body) -> Response {
    if let Some(frame) = decode(&body, &ctx, "discovery") {
        match &frame.body {
            Body::ProcDiscovery(discoveries) => {
                let info = frame_info(&body, &ctx);
                ctx.write(vec![snapshot_row(&info, &frame)]);
                ctx.write(
                    discoveries.process_discoveries.iter().map(|d| discovery_row(&info, discoveries, d)).collect(),
                );
            }
            _ => unexpected(&body, &ctx, "discovery", &frame),
        }
    }
    reply()
}

#[cfg(test)]
mod tests {
    use ninjapaws_proto::datadog::process_agent::{
        Addr, CollectorPod, Command, ContainerAddr, CpuInfo, CpuStat, Interface, IoStat, IpTranslation, LogResource,
        MemoryStat, OsInfo, PortInfo, ProcessNetworks, ProcessUser, ProtocolStack, Resource, Route, ServiceDiscovery,
        ServiceName, SingleCpuStat, Subnet, TracerMetadata, resource,
    };
    use serde_json::Value;

    use super::*;
    use crate::process_frame::Header;
    use crate::routers::process_buffers::tests::{tags_v2, tags_v3};

    const AGENT_VERSION: &str = "7.58.2";
    const REQUEST_ID: &str = "b0a1f2c3-0000-4000-8000-000000000001";

    fn info() -> FrameInfo {
        FrameInfo {
            tenant: "test".into(),
            snapshot_id: Uuid::nil(),
            at: Utc.with_ymd_and_hms(1970, 1, 1, 0, 0, 1).unwrap(),
            path: "/api/v1/collector".into(),
            bytes: 0,
            agent: AgentIdentity { version: AGENT_VERSION.into(), request_id: REQUEST_ID.into(), ..Default::default() },
        }
    }

    fn same_json(expected: &str, actual: &str) {
        let parse = |text: &str| serde_json::from_str::<Value>(text).unwrap();
        assert_eq!(parse(expected), parse(actual), "{actual}");
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| v.to_string()).collect()
    }

    fn three_tags() -> Vec<String> {
        strings(&["env:prod", "kube_service:a", "kube_service:b"])
    }

    fn seconds(s: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(s, 0).unwrap()
    }

    /// One container with every field set, each to a different value: a field that silently read
    /// its neighbour would still look plausible with zeros.
    fn full_container() -> Container {
        Container {
            r#type: "containerd".into(),
            id: "cid-1".into(),
            name: "app".into(),
            image: "repo/app:1".into(),
            cpu_limit: 2.0,
            memory_limit: 1 << 30,
            state: ContainerState::Running as i32,
            health: ContainerHealth::Healthy as i32,
            created: 1_700_000_000,
            started: 1_700_000_060,
            rbps: 1.0,
            wbps: 2.0,
            key: 77,
            net_rcvd_ps: 3.0,
            net_sent_ps: 4.0,
            net_rcvd_bps: 5.0,
            net_sent_bps: 6.0,
            user_pct: 7.0,
            system_pct: 8.0,
            total_pct: 15.0,
            mem_rss: 9,
            mem_cache: 10,
            byte_key: vec![0xAA],
            thread_count: 12,
            thread_limit: 100,
            mem_usage: 13,
            cpu_usage_ns: 14.0,
            mem_accounted: 15,
            cpu_request: 0.5,
            memory_request: 1 << 20,
            repo_digest: "sha256:abc".into(),
            tags: three_tags(),
            // The same ip and port on two protocols: a real shape.
            addresses: vec![
                ContainerAddr { ip: "10.0.0.1".into(), port: 8080, protocol: ConnectionType::Tcp as i32 },
                ContainerAddr { ip: "10.0.0.1".into(), port: 8080, protocol: ConnectionType::Udp as i32 },
            ],
            ..Default::default()
        }
    }

    fn full_process() -> Process {
        let name = |name: &str, source: ServiceNameSource| ServiceName { name: name.into(), source: source as i32 };
        Process {
            key: 4242,
            pid: 1234,
            ns_pid: 7,
            command: Some(Command {
                args: strings(&["/usr/bin/app", "--filter=a b", "-v"]),
                cwd: "/srv/app".into(),
                root: "/".into(),
                on_disk: true,
                ppid: 1,
                pgroup: 1200,
                exe: "/usr/bin/app".into(),
                comm: "app".into(),
            }),
            user: Some(ProcessUser { name: "svc".into(), uid: 1000, gid: 1001, euid: 0, egid: 2, suid: 3, sgid: 4 }),
            memory: Some(MemoryStat {
                rss: 100,
                vms: 200,
                swap: 300,
                shared: 400,
                text: 500,
                lib: 600,
                data: 700,
                dirty: 800,
            }),
            cpu: Some(CpuStat {
                last_cpu: "cpu3".into(),
                total_pct: 12.5,
                user_pct: 8.5,
                system_pct: 4.0,
                num_threads: 9,
                nice: -5,
                user_time: 111,
                system_time: 222,
                cpus: vec![
                    SingleCpuStat { name: "cpu0".into(), total_pct: 1.0 },
                    SingleCpuStat { name: "cpu1".into(), total_pct: 2.0 },
                ],
            }),
            create_time: 1_700_000_000_000,
            open_fd_count: 64,
            state: ProcessState::R as i32,
            io_stat: Some(IoStat { read_rate: 1.0, write_rate: 2.0, read_bytes_rate: 3.0, write_bytes_rate: 4.0 }),
            container_id: "cid-1".into(),
            container_key: 99,
            voluntary_ctx_switches: 11,
            involuntary_ctx_switches: 22,
            byte_key: vec![1, 2],
            container_byte_key: vec![3],
            networks: Some(ProcessNetworks { connection_rate: 5.0, bytes_rate: 6.0 }),
            language: Language::Go as i32,
            port_info: Some(PortInfo { tcp: vec![8080, 8443], udp: vec![53] }),
            service_discovery: Some(ServiceDiscovery {
                generated_service_name: Some(name("guessed", ServiceNameSource::CommandLine)),
                dd_service_name: Some(name("declared", ServiceNameSource::DdService)),
                additional_generated_names: vec![name("extra", ServiceNameSource::Spring)],
                tracer_metadata: vec![TracerMetadata { runtime_id: "runtime-1".into(), service_name: "traced".into() }],
                apm_instrumentation: true,
                resources: vec![Resource {
                    resource: Some(resource::Resource::Logs(LogResource { path: "/var/log/app.log".into() })),
                }],
            }),
            injection_state: InjectionState::InjectionInjected as i32,
            zombie_children_count: 3,
            zombie_net_rate: 1.5,
            has_zombie_aggregation: true,
            process_context: strings(&["ctx-a", "ctx-b"]),
            // Two tags sharing a key: a pod behind two services sends exactly this.
            tags: three_tags(),
            ..Default::default()
        }
    }

    fn full_collector_proc() -> CollectorProc {
        let cpu = |number: i32| CpuInfo {
            number,
            vendor: "GenuineIntel".into(),
            family: "6".into(),
            model: "85".into(),
            physical_id: "0".into(),
            core_id: number.to_string(),
            cores: 4,
            mhz: 2500,
            cache_size: 33792,
        };
        CollectorProc {
            host_name: "host-a".into(),
            network_id: "vpc-123".into(),
            host: Some(Host {
                id: 5,
                org_id: 7,
                name: "host-a".into(),
                num_cpus: 8,
                total_memory: 16 << 30,
                tag_index: 0,
                tags_modified: 1_700_000_000,
                all_tags: three_tags(),
            }),
            info: Some(SystemInfo {
                uuid: "uuid-1".into(),
                os: Some(OsInfo {
                    name: "linux".into(),
                    platform: "ubuntu".into(),
                    family: "debian".into(),
                    version: "22.04".into(),
                    kernel_version: "5.15.0".into(),
                }),
                total_memory: 16 << 30,
                cpus: vec![cpu(0), cpu(1)],
            }),
            group_id: 11,
            group_size: 2,
            container_host_type: ContainerHostType::FargateEks as i32,
            hints: Some(collector_proc::Hints::HintMask(3)),
            processes: vec![full_process()],
            containers: vec![full_container()],
        }
    }

    fn frame_of(message_type: u8, body: Body) -> Frame {
        Frame {
            header: Header { version: 3, encoding: 0, message_type, subscription_id: 0, org_id: 7, timestamp: 4242 },
            body,
        }
    }

    #[test]
    fn the_reply_carries_a_status_which_the_agent_dereferences_without_checking() {
        let frame = process_frame::decode(&REPLY_FRAME).expect("the agent could not decode our own reply");
        let Body::Res(reply) = frame.body else { panic!("not a ResCollector") };
        let status = reply.status.unwrap();
        assert_eq!(status.interval, CHECK_INTERVAL);
        // Nothing is watching a live view.
        assert_eq!(status.active_clients, 0);
        // Older agents look for the nested header.
        assert!(reply.header.is_some());
    }

    #[test]
    fn every_field_of_a_process_reaches_its_column() {
        let proc = full_collector_proc();
        let row = process_row(&info(), &proc, &proc.processes[0]);

        assert_eq!((row.ns_pid, row.key), (7, 4242));
        assert_eq!((row.cwd.as_str(), row.root.as_str(), row.on_disk, row.pgroup), ("/srv/app", "/", 1, 1200));
        assert_eq!([row.uid, row.gid, row.euid, row.egid, row.suid, row.sgid], [1000, 1001, 0, 2, 3, 4]);
        assert_eq!(
            [row.rss, row.vms, row.mem_swap, row.mem_shared, row.mem_text, row.mem_lib, row.mem_data, row.mem_dirty],
            [100, 200, 300, 400, 500, 600, 700, 800]
        );
        assert_eq!(row.cpu_last_cpu, "cpu3");
        assert_eq!([row.cpu_pct, row.cpu_user_pct, row.cpu_system_pct], [12.5, 8.5, 4.0]);
        assert_eq!((row.threads, row.cpu_nice, row.cpu_user_time, row.cpu_system_time), (9, -5, 111, 222));
        assert_eq!(
            [row.io_read_rate, row.io_write_rate, row.io_read_bytes_rate, row.io_write_bytes_rate],
            [1.0, 2.0, 3.0, 4.0]
        );
        assert_eq!((row.voluntary_ctx_switches, row.involuntary_ctx_switches), (11, 22));
        assert_eq!((row.net_connection_rate, row.net_bytes_rate), (5.0, 6.0));
        // Enum NAMES, never numbers.
        assert_eq!(row.language, "LANGUAGE_GO");
        assert_eq!(row.injection_state, "INJECTION_INJECTED");
        assert_eq!(row.state, "R");
        assert_eq!(row.generated_service_name, "guessed");
        assert_eq!(row.generated_service_name_source, "SERVICE_NAME_SOURCE_COMMAND_LINE");
        assert_eq!(row.dd_service_name, "declared");
        assert_eq!(row.dd_service_name_source, "SERVICE_NAME_SOURCE_DD_SERVICE");
        assert_eq!(row.apm_instrumentation, 1);
        assert_eq!((row.zombie_children_count, row.zombie_net_rate, row.has_zombie_aggregation), (3, 1.5, 1));
        assert_eq!(row.container_key, 99);
        assert_eq!(row.byte_key, Blob(vec![1, 2]));
        assert_eq!(row.container_byte_key, Blob(vec![3]));
        assert_eq!((row.network_id.as_str(), row.group_id, row.group_size), ("vpc-123", 11, 2));
        assert_eq!(row.container_host_type, "fargateEKS");
        assert_eq!((row.agent_version.as_str(), row.request_id.as_str()), (AGENT_VERSION, REQUEST_ID));
        assert_eq!(row.host_uuid, "uuid-1");
        assert_eq!(
            [&row.os_name, &row.os_platform, &row.os_family, &row.os_version, &row.kernel_version],
            ["linux", "ubuntu", "debian", "22.04", "5.15.0"]
        );

        // Parallel arrays, in the order of the wire.
        assert_eq!(row.cpu_core_names, ["cpu0", "cpu1"]);
        assert_eq!(row.cpu_core_pcts, [1.0, 2.0]);
        assert_eq!(row.port_tcp, [8080, 8443]);
        assert_eq!(row.port_udp, [53]);
        assert_eq!(row.process_context, ["ctx-a", "ctx-b"]);
        // The APM join: these tie a pid on a host to the traces it emitted.
        assert_eq!(row.tracer_runtime_ids, ["runtime-1"]);
        assert_eq!(row.tracer_service_names, ["traced"]);
        assert_eq!(row.additional_generated_names, ["extra"]);
        assert_eq!(row.additional_generated_name_sources, ["SERVICE_NAME_SOURCE_SPRING"]);
        same_json(r#"[{"logs":{"path":"/var/log/app.log"}}]"#, &row.service_resources);
    }

    #[test]
    fn tags_sharing_a_key_all_survive() {
        let proc = full_collector_proc();
        let row = process_row(&info(), &proc, &proc.processes[0]);
        assert_eq!(row.tags, tags::to_multi_map(three_tags()));
        assert_eq!(row.tags["kube_service"], ["a", "b"]);
    }

    /// Joining argv on spaces cannot be undone: `--filter=a b` and two separate arguments give the
    /// same string.
    #[test]
    fn argv_is_kept_beside_the_joined_command_line() {
        let proc = full_collector_proc();
        let row = process_row(&info(), &proc, &proc.processes[0]);
        assert_eq!(row.args, ["/usr/bin/app", "--filter=a b", "-v"]);
        assert_eq!(row.cmdline, "/usr/bin/app --filter=a b -v");
    }

    /// A snapshot from a host where /proc was partly unreadable.
    #[test]
    fn a_process_with_no_sub_messages_gives_a_row_of_zeros() {
        let proc = CollectorProc { host_name: "host-a".into(), ..Default::default() };
        let row = process_row(&info(), &proc, &Process { pid: 5, ..Default::default() });
        assert_eq!(row.pid, 5);
        assert_eq!((row.comm.as_str(), row.user.as_str(), row.rss, row.cpu_pct), ("", "", 0, 0.0));
        assert_eq!(row.generated_service_name_source, "");
        assert_eq!(row.service_resources, "");
        // "" rather than "null" or "{}": the host was absent.
        assert_eq!(row.process_host, "");
    }

    #[test]
    fn an_absent_start_time_is_flagged_not_stored_as_a_date() {
        for (wire, has) in [(0, 0), (-1, 0), (1_700_000_000_000, 1)] {
            let row = process_row(
                &info(),
                &CollectorProc::default(),
                &Process { pid: 1, create_time: wire, ..Default::default() },
            );
            assert_eq!(row.has_create_time, has);
            let expected = if has == 1 { Utc.timestamp_millis_opt(wire).unwrap() } else { no_time() };
            assert_eq!(row.create_time, expected);
        }
    }

    #[test]
    fn a_start_time_past_what_the_column_holds_is_pinned_to_its_end() {
        let p = Process { pid: 1, create_time: i64::MAX, ..Default::default() };
        let row = process_row(&info(), &CollectorProc::default(), &p);
        assert_eq!(row.has_create_time, 1);
        assert_eq!(row.create_time, Utc.with_ymd_and_hms(2106, 2, 7, 6, 28, 15).unwrap());
    }

    #[test]
    fn an_enum_value_newer_than_the_definitions_is_stored_as_its_number() {
        let p = Process { pid: 1, state: 42, ..Default::default() };
        assert_eq!(process_row(&info(), &CollectorProc::default(), &p).state, "42");
    }

    fn snapshot_of(message_type: u8, body: Body) -> rows::ProcessSnapshot {
        let row = snapshot_row(&info(), &frame_of(message_type, body));
        assert_eq!(row.tenant_id, "test");
        assert_eq!(row.host_name, "host-a");
        // The header's timestamp is stored as the number it is.
        assert_eq!(row.header_timestamp, 4242);
        assert_eq!(row.header_org_id, 7);
        row
    }

    #[test]
    fn the_envelope_of_a_collector_proc_carries_its_host_system_and_hints() {
        let row = snapshot_of(12, Body::Proc(full_collector_proc()));
        assert_eq!((row.message_type, row.message_type_name.as_str()), (12, "CollectorProc"));
        assert_eq!(row.network_id, "vpc-123");
        assert_eq!((row.host_id, row.host_num_cpus), (5, 8));
        assert_eq!(row.host_all_tags["kube_service"], ["a", "b"]);
        assert_eq!(row.host_tags_modified, Some(seconds(1_700_000_000)));
        assert_eq!(row.cpu_numbers, [0, 1]);
        assert_eq!(row.cpu_mhz, [2500, 2500]);
        assert_eq!(row.kernel_version, "5.15.0");
        assert_eq!(row.sys_uuid, "uuid-1");
        assert_eq!(row.hint_mask, Some(3));
        assert_eq!((row.process_count, row.container_count), (1, 1));
    }

    #[test]
    fn a_collector_proc_without_hints_has_no_hint_mask_one_with_mask_0_has_0() {
        let bare = CollectorProc { host_name: "host-a".into(), ..Default::default() };
        assert_eq!(snapshot_of(12, Body::Proc(bare.clone())).hint_mask, None);
        let zero = CollectorProc { hints: Some(collector_proc::Hints::HintMask(0)), ..bare };
        assert_eq!(snapshot_of(12, Body::Proc(zero)).hint_mask, Some(0));
    }

    #[test]
    fn the_envelope_of_a_collector_real_time_carries_the_inlined_host_numbers_and_no_host() {
        let body = CollectorRealTime {
            host_name: "host-a".into(),
            host_id: 5,
            org_id: 7,
            num_cpus: 8,
            total_memory: 99,
            stats: vec![ProcessStat { pid: 1, ..Default::default() }, ProcessStat { pid: 2, ..Default::default() }],
            container_stats: vec![ContainerStat { id: "c".into(), ..Default::default() }],
            ..Default::default()
        };
        let row = snapshot_of(27, Body::RealTime(body));
        assert_eq!(row.message_type_name, "CollectorRealTime");
        assert_eq!((row.rt_host_id, row.rt_org_id, row.rt_num_cpus, row.rt_total_memory), (5, 7, 8, 99));
        assert_eq!((row.process_stat_count, row.container_stat_count), (2, 1));
        // A realtime frame has no Host message; nothing may be invented for it.
        assert_eq!((row.hint_mask, row.host_id), (None, 0));
    }

    #[test]
    fn the_envelope_of_a_collector_container() {
        let body = CollectorContainer {
            host_name: "host-a".into(),
            network_id: "vpc-9".into(),
            container_host_type: ContainerHostType::Sidecar as i32,
            containers: vec![full_container()],
            ..Default::default()
        };
        let row = snapshot_of(39, Body::Container(body));
        assert_eq!(row.message_type_name, "CollectorContainer");
        assert_eq!((row.network_id.as_str(), row.container_count), ("vpc-9", 1));
        assert_eq!(row.container_host_type, "sidecar");
    }

    #[test]
    fn the_envelope_of_a_collector_container_real_time() {
        let body = CollectorContainerRealTime {
            host_name: "host-a".into(),
            host_id: 6,
            num_cpus: 4,
            total_memory: 8,
            stats: vec![ContainerStat { id: "c".into(), ..Default::default() }],
            ..Default::default()
        };
        let row = snapshot_of(40, Body::ContainerRealTime(body));
        assert_eq!(row.message_type_name, "CollectorContainerRealTime");
        assert_eq!((row.rt_host_id, row.container_stat_count), (6, 1));
    }

    #[test]
    fn the_envelope_of_a_collector_proc_discovery() {
        let body = CollectorProcDiscovery {
            host_name: "host-a".into(),
            group_id: 3,
            group_size: 4,
            host: Some(Host { id: 5, name: "host-a".into(), ..Default::default() }),
            process_discoveries: vec![ProcessDiscovery { pid: 1, ..Default::default() }],
        };
        let row = snapshot_of(53, Body::ProcDiscovery(body));
        assert_eq!(row.message_type_name, "CollectorProcDiscovery");
        assert_eq!((row.discovery_count, row.group_id, row.group_size, row.host_id), (1, 3, 4, 5));
    }

    #[test]
    fn the_envelope_of_a_collector_connections() {
        let body = CollectorConnections {
            host_name: "host-a".into(),
            network_id: "vpc-1".into(),
            connections: (1..=3).map(|pid| Connection { pid, ..Default::default() }).collect(),
            ..Default::default()
        };
        let row = snapshot_of(22, Body::Connections(body));
        assert_eq!((row.message_type_name.as_str(), row.connection_count), ("CollectorConnections", 3));
    }

    #[test]
    fn a_frame_type_without_a_name_of_its_own_is_named_by_its_number() {
        let pod = CollectorPod { host_name: "host-a".into(), ..Default::default() };
        let row = snapshot_row(&info(), &frame_of(41, Body::Pod(pod)));
        assert_eq!((row.message_type_name.as_str(), row.host_name.as_str()), ("type_41", ""));
    }

    fn inventory() -> ContainerEnvelope {
        ContainerEnvelope { source: "collector_proc".into(), ..Default::default() }
    }

    #[test]
    fn container_times_the_runtime_never_reported_stay_empty() {
        let full = container_row(&info(), &inventory(), "host-a", &full_container());
        assert_eq!(full.created, Some(seconds(1_700_000_000)));
        assert_eq!(full.started, Some(seconds(1_700_000_060)));
        // The same ip and port on two protocols: a map keyed by ip would collapse it.
        assert_eq!(full.addr_protocols, ["tcp", "udp"]);
        assert_eq!((full.state.as_str(), full.health.as_str()), ("running", "healthy"));
        assert_eq!(full.tags["kube_service"], ["a", "b"]);

        let bare = container_row(&info(), &inventory(), "host-a", &Container { id: "c".into(), ..Default::default() });
        assert_eq!((bare.created, bare.started), (None, None));
    }

    #[test]
    fn a_container_on_a_process_is_stored_beside_the_inventory_told_apart_by_source() {
        let mut proc = full_collector_proc();
        proc.processes[0].container = Some(full_container());
        let rows = proc_container_rows(&info(), &proc);
        let sources: Vec<(&str, i32)> = rows.iter().map(|r| (r.source.as_str(), r.pid)).collect();
        assert_eq!(sources, [("collector_proc", 0), ("process", 1234)]);
    }

    #[test]
    fn telemetry_an_agent_did_not_send_stays_empty() {
        assert!(conn_telemetry(None).is_empty());
        let telemetry =
            CollectorConnectionsTelemetry { kprobes_triggered: 9, dns_stats_dropped: 4, ..Default::default() };
        let sent = conn_telemetry(Some(&telemetry));
        assert_eq!((sent["kprobes_triggered"], sent["dns_stats_dropped"]), (9, 4));
        // Every field of the message.
        assert_eq!(sent.len(), 11);
    }

    /// route_idx is a POSITION in the routes list; dropping an empty route would shift every index
    /// after it onto the wrong one.
    #[test]
    fn a_route_without_subnet_or_interface_keeps_its_position() {
        let routes = [
            Route { subnet: Some(Subnet { alias: "subnet-a".into() }), interface: None },
            Route::default(),
            Route { subnet: None, interface: Some(Interface { hardware_addr: "02:42:ac:11:00:02".into() }) },
        ];
        same_json(
            r#"[{"subnet":{"alias":"subnet-a"}},{},{"interface":{"HardwareAddr":"02:42:ac:11:00:02"}}]"#,
            &json::proto::messages(&routes),
        );
        assert_eq!(json::proto::messages::<Route>(&[]), "");
    }

    #[test]
    fn an_untranslated_connection_has_no_nat_entry_and_its_tags_come_out_of_the_buffer() {
        let addr = |ip: &str, port: i32| Some(Addr { ip: ip.into(), port, ..Default::default() });
        let plain = Connection {
            pid: 1,
            laddr: addr("10.0.0.1", 5000),
            raddr: addr("10.0.0.2", 443),
            r#type: ConnectionType::Tcp as i32,
            family: ConnectionFamily::V4 as i32,
            direction: ConnectionDirection::Outgoing as i32,
            protocol: Some(ProtocolStack {
                stack: vec![ProtocolType::ProtocolTls as i32, ProtocolType::ProtocolHttp2 as i32],
            }),
            tags_idx: 0,
            local_container_tags_index: 14,
            remote_service_tags_idx: -1,
            ..Default::default()
        };
        let row = connection_row(&info(), "host-a", &tags_v3(), &tags_v2(), &plain);
        assert_eq!((row.ip_translation_repl_src_ip, row.ip_translation_repl_src_port), (None, None));
        assert_eq!([&row.r#type, &row.family, &row.direction], ["tcp", "v4", "outgoing"]);
        // The stack is ordered: TLS over HTTP/2 is not HTTP/2 over TLS.
        assert_eq!(row.protocol_stack, ["protocolTLS", "protocolHTTP2"]);
        assert_eq!(row.tags["kube_service"], ["a", "b"]);
        assert_eq!(row.local_container_tags, tags::to_multi_map(["env:prod", "role:db"]));
        assert!(row.remote_service_tags.is_empty());

        // The container's tags index encodedTags, the connection's own index
        // encodedConnectionsTags: without the first buffer only the latter resolve.
        let without_encoded_tags = connection_row(&info(), "host-a", &[], &tags_v2(), &plain);
        assert_eq!(without_encoded_tags.tags["kube_service"], ["a", "b"]);
        assert!(without_encoded_tags.local_container_tags.is_empty());

        let translated = Connection {
            pid: 2,
            ip_translation: Some(IpTranslation {
                repl_src_ip: "172.17.0.2".into(),
                repl_dst_ip: "10.0.0.9".into(),
                repl_src_port: 1,
                repl_dst_port: 2,
            }),
            ..Default::default()
        };
        let nat = connection_row(&info(), "host-a", &tags_v3(), &tags_v2(), &translated);
        assert_eq!(nat.ip_translation_repl_src_ip.as_deref(), Some("172.17.0.2"));
        assert_eq!(nat.ip_translation_repl_dst_ip.as_deref(), Some("10.0.0.9"));
        assert_eq!((nat.ip_translation_repl_src_port, nat.ip_translation_repl_dst_port), (Some(1), Some(2)));
        // A connection with no addresses must not invent one.
        assert_eq!((nat.laddr_ip.as_str(), nat.raddr_port), ("", 0));
    }

    /// A version 2 buffer as the agent builds encodedTags: the set ["-"] at index 0, then
    /// [env:prod] at index 6.
    fn tags_with_dummy() -> Vec<u8> {
        [
            &[2u8, 18, 0, 0, 0][..], // version, where the footer starts
            &[1, 0],
            b"-", // at 5
            &[8, 0],
            b"env:prod",         // at 8
            &[1, 0, 5, 0, 0, 0], // set 0: one tag, at 5
            &[1, 0, 8, 0, 0, 0], // set 6: one tag, at 8
        ]
        .concat()
    }

    #[test]
    fn an_index_into_encoded_tags_that_was_not_sent_lands_on_the_agents_dash_set_and_means_none() {
        let buffer = tags_with_dummy();
        assert_eq!(tag_buffer::tags(&buffer, 0), ["-"]);

        let unset = connection_row(&info(), "host-a", &buffer, &[], &Connection { pid: 1, ..Default::default() });
        assert!(unset.local_container_tags.is_empty());
        assert!(unset.remote_service_tags.is_empty());

        let c = Connection { pid: 1, local_container_tags_index: 6, remote_service_tags_idx: -1, ..Default::default() };
        let sent = connection_row(&info(), "host-a", &buffer, &[], &c);
        assert_eq!(sent.local_container_tags, tags::to_multi_map(["env:prod"]));
        assert!(sent.remote_service_tags.is_empty());

        let conns = CollectorConnections { encoded_tags: buffer, ..Default::default() };
        assert!(connections_payload_row(&info(), &conns, Vec::new()).host_tags.is_empty());
    }
}
