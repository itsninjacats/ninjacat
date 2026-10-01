/// The rest of the process intake: everything the process-agent sends that
/// is not a Process.
///
/// One frame carries one of six message types. Its envelope goes to
/// process_snapshots once; the item tables carry SnapshotID back to it and a
/// copy of the few envelope fields every query needs, so the common question
/// is not a join. Reasoning per column: 0008_process.sql.
namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One received frame, whatever its type.
type ProcessSnapshotRow =
    { TenantID: string
      ReceivedAt: DateTime
      SnapshotID: Guid

      MessageType: uint8
      MessageTypeName: string
      Path: string

      HeaderVersion: uint8
      HeaderEncoding: uint8
      HeaderSubscriptionID: uint8
      HeaderOrgID: int32
      /// Kept as the number it is: the agent leaves it at zero and its unit
      /// is documented nowhere.
      HeaderTimestamp: int64
      FrameBytes: uint64

      HostName: string
      NetworkID: string
      GroupID: int32
      GroupSize: int32
      ContainerHostType: string

      HostID: int64
      HostOrgID: int32
      HostDisplayName: string
      HostAllTags: Map<string, string[]>
      HostNumCPUs: int32
      HostTotalMemory: int64
      HostTagIndex: int32
      /// "Never recomputed" and "recomputed at the epoch" are different
      /// statements about the backend's tag cache.
      HostTagsModified: DateTime option

      SysUUID: string
      OSName: string
      OSPlatform: string
      OSFamily: string
      OSVersion: string
      KernelVersion: string
      SysTotalMemory: int64

      /// Nine parallel arrays, one entry per CPUInfo, in the order sent.
      CPUNumbers: int32[]
      CPUVendors: string[]
      CPUFamilies: string[]
      CPUModels: string[]
      CPUPhysicalIDs: string[]
      CPUCoreIDs: string[]
      CPUCores: int32[]
      CPUMhz: int64[]
      CPUCacheSizes: int32[]

      /// Only CollectorProc has hints, and "no hints" differs from "mask 0".
      HintMask: int32 option

      RTHostID: int64
      RTOrgID: int32
      RTNumCPUs: int32
      RTTotalMemory: int64

      ProcessCount: uint32
      ProcessStatCount: uint32
      ContainerCount: uint32
      ContainerStatCount: uint32
      DiscoveryCount: uint32
      ConnectionCount: uint32

      AgentHostname: string
      AgentVersion: string
      AgentContainerCount: string
      RequestID: string }

/// One ProcessStat from the 2-second realtime stream.
///
/// Nice and Threads are ProcessStat's own fields, not the nested CPUStat's
/// (CPUNice, CPUNumThreads); the agent does not always fill both.
type ProcessStatRow =
    { TenantID: string
      Timestamp: DateTime
      Host: string
      SnapshotID: Guid

      PID: int32
      Key: uint32
      CreateTime: DateTime
      HasCreateTime: uint8

      Nice: int32
      Threads: int32
      OpenFDs: int32

      MemRSS: uint64
      MemVMS: uint64
      MemSwap: uint64
      MemShared: uint64
      MemText: uint64
      MemLib: uint64
      MemData: uint64
      MemDirty: uint64

      CPULastCPU: string
      CPUTotalPct: float32
      CPUUserPct: float32
      CPUSystemPct: float32
      CPUNumThreads: int32
      CPUNice: int32
      CPUUserTime: int64
      CPUSystemTime: int64
      CPUCoreNames: string[]
      CPUCorePcts: float32[]

      IOReadRate: float32
      IOWriteRate: float32
      IOReadBytesRate: float32
      IOWriteBytesRate: float32

      NetConnectionRate: float32
      NetBytesRate: float32

      VoluntaryCtxSwitches: uint64
      InvoluntaryCtxSwitches: uint64

      ContainerID: string
      ContainerState: string
      ProcessState: string

      // Deprecated upstream, stored anyway: older agents send only these.
      ContainerHealth: string
      ContainerRbps: float32
      ContainerWbps: float32
      ContainerKey: uint32
      ContainerNetRcvdPs: float32
      ContainerNetSentPs: float32
      ContainerNetRcvdBps: float32
      ContainerNetSentBps: float32

      ByteKey: byte[]
      ContainerByteKey: byte[]

      GroupID: int32
      GroupSize: int32
      ContainerHostType: string
      RTHostID: int64
      RTOrgID: int32
      RTNumCPUs: int32
      RTTotalMemory: int64
      AgentVersion: string
      RequestID: string }

/// One Container. Source says which message it hung off (collector_proc,
/// collector_container or process): the same struct arrives all three ways
/// and what is populated differs.
type ContainerRow =
    { TenantID: string
      Timestamp: DateTime
      Host: string
      SnapshotID: Guid

      Source: string
      /// The owning process, for Source = "process". Zero otherwise.
      PID: int32

      Type: string
      ID: string
      Name: string
      Image: string
      RepoDigest: string

      CPULimit: float32
      MemoryLimit: uint64
      CPURequest: float32
      MemoryRequest: uint64

      State: string
      Health: string

      /// None when the runtime reported no time, so a container never
      /// appears to have started at the epoch.
      Created: DateTime option
      Started: DateTime option

      Rbps: float32
      Wbps: float32
      NetRcvdPs: float32
      NetSentPs: float32
      NetRcvdBps: float32
      NetSentBps: float32

      UserPct: float32
      SystemPct: float32
      TotalPct: float32
      CPUUsageNs: float32

      MemRSS: uint64
      MemCache: uint64
      MemUsage: uint64
      MemAccounted: uint64

      ThreadCount: uint64
      ThreadLimit: uint64

      Key: uint32
      ByteKey: byte[]

      /// Three parallel arrays, one entry per ContainerAddr, order as sent.
      AddrIPs: string[]
      AddrPorts: int32[]
      AddrProtocols: string[]

      Tags: Map<string, string[]>

      /// Container.host as JSON: a field the backend fills in, nil from an agent.
      HostInfo: string

      NetworkID: string
      GroupID: int32
      GroupSize: int32
      ContainerHostType: string
      AgentVersion: string
      RequestID: string }

/// One ContainerStat. Source is collector_realtime or
/// collector_container_realtime; the two messages carry identical fields.
type ContainerStatRow =
    { TenantID: string
      Timestamp: DateTime
      Host: string
      SnapshotID: Guid
      Source: string

      ID: string

      UserPct: float32
      SystemPct: float32
      TotalPct: float32
      CPULimit: float32
      CPURequest: float32
      CPUUsageNs: float32

      MemRSS: uint64
      MemCache: uint64
      MemLimit: uint64
      MemUsage: uint64
      MemAccounted: uint64
      MemoryRequest: uint64

      Rbps: float32
      Wbps: float32
      NetRcvdPs: float32
      NetSentPs: float32
      NetRcvdBps: float32
      NetSentBps: float32

      State: string
      Health: string

      Key: uint32
      Started: DateTime option
      ByteKey: byte[]
      ThreadCount: uint64
      ThreadLimit: uint64

      GroupID: int32
      GroupSize: int32
      ContainerHostType: string
      RTHostID: int64
      RTOrgID: int32
      RTNumCPUs: int32
      RTTotalMemory: int64
      AgentVersion: string
      RequestID: string }

/// One ProcessDiscovery: pid, command and user, no resource usage. On hosts
/// that run only the discovery check this is the sole process-level record.
type ProcessDiscoveryRow =
    { TenantID: string
      Timestamp: DateTime
      Host: string
      SnapshotID: Guid

      PID: int32
      NsPID: int32
      CreateTime: DateTime
      HasCreateTime: uint8
      ByteKey: byte[]

      Comm: string
      Exe: string
      Cmdline: string
      Args: string[]
      Cwd: string
      Root: string
      OnDisk: uint8
      PPID: int32
      Pgroup: int32

      User: string
      UID: int32
      GID: int32
      EUID: int32
      EGID: int32
      SUID: int32
      SGID: int32

      HostInfo: string

      GroupID: int32
      GroupSize: int32
      AgentVersion: string
      RequestID: string }

/// The per-frame half of CollectorConnections: what describes the sender and
/// its eBPF machinery rather than any one connection.
///
/// PayloadID is the frame's ProcessSnapshotRow.SnapshotID and every
/// ConnectionRow.PayloadID.
type ConnectionsPayloadRow =
    { TenantID: string
      ReceivedAt: DateTime
      PayloadID: Guid

      HostName: string
      NetworkID: string
      GroupID: int32
      GroupSize: int32
      ContainerHostType: string
      ConnectionCount: uint32

      Architecture: string
      KernelVersion: string
      Platform: string
      PlatformVersion: string

      ResolvedResources: string
      ContainerForPID: Map<int32, string>

      /// The packed tag buffers, verbatim. Connection rows carry their tags
      /// resolved, but the encoding is versioned and one that cannot be read
      /// yet must still land somewhere.
      EncodedTags: byte[]
      EncodedConnectionsTags: byte[]

      HostTagsIndex: int32
      HostTags: Map<string, string[]>

      ConnTelemetry: Map<string, int64>
      ConnTelemetryMap: Map<string, int64>

      CompilationTelemetry: string
      KernelHeaderFetchResult: string
      CORETelemetry: Map<string, string>
      PrebuiltEBPFAssets: string[]

      Routes: string
      RouteMetadata: string
      AgentConfiguration: string

      EncodedDNS: byte[]
      Domains: string[]
      EncodedDomainDatabase: byte[]
      EncodedDNSLookups: byte[]
      /// The names read out of whichever DNS encoding arrived, so the common
      /// case needs no decoder at read time.
      DNSNames: string[]

      ResolvedHostsByName: string
      ResolvedPublicIPs: string

      EcsTask: string
      ResolvConfs: string[]

      AgentHostname: string
      AgentVersion: string
      AgentContainerCount: string
      RequestID: string }

/// One Connection from the system-probe's connection table.
///
/// The four *Aggregations fields are separately serialised protobufs of
/// Universal Service Monitoring. They are kept as bytes: a decoder can be
/// written against stored bytes later, not against discarded ones.
type ConnectionRow =
    { TenantID: string
      Timestamp: DateTime
      Host: string
      PayloadID: Guid

      PID: int32

      LaddrIP: string
      LaddrPort: int32
      LaddrContainerID: string
      LaddrHostName: string
      RaddrIP: string
      RaddrPort: int32
      RaddrContainerID: string
      RaddrHostName: string

      Family: string
      Type: string
      Direction: string
      IsLocalPortEphemeral: string

      LastBytesSent: uint64
      LastBytesReceived: uint64
      LastPacketsSent: uint64
      LastPacketsReceived: uint64
      LastRetransmits: uint32

      /// Order is the stack: [protocolTLS, protocolHTTP2] is not the reverse.
      ProtocolStack: string[]

      NetNS: uint32
      RemoteNetworkID: string

      /// The conntrack NAT entry: None for every untranslated connection,
      /// which is most of them, rather than 0.0.0.0:0.
      IPTranslationReplSrcIP: string option
      IPTranslationReplDstIP: string option
      IPTranslationReplSrcPort: int32 option
      IPTranslationReplDstPort: int32 option

      RTT: uint32
      RTTVar: uint32
      IntraHost: uint8

      DNSSuccessfulResponses: uint32
      DNSFailedResponses: uint32
      DNSTimeouts: uint32
      DNSSuccessLatencySum: uint64
      DNSFailureLatencySum: uint64
      DNSCountByRcode: Map<uint32, uint32>

      DNSStatsByDomain: string
      DNSStatsByDomainByQueryType: string
      DNSStatsByDomainOffsetByQueryType: string

      LastTCPEstablished: uint32
      LastTCPClosed: uint32

      RouteIdx: int32
      RouteTargetIdx: int32
      ResolvConfIdx: int32

      HTTPAggregations: byte[]
      HTTP2Aggregations: byte[]
      DataStreamsAggregations: byte[]
      DatabaseAggregations: byte[]

      TagsIndices: uint32[]
      TagsIdx: int32
      TagsChecksum: uint32
      Tags: Map<string, string[]>

      LocalContainerTagsIndex: int32
      LocalContainerTags: Map<string, string[]>
      RemoteServiceTagsIdx: int32
      RemoteServiceTags: Map<string, string[]>

      StateIndex: uint32
      TCPFailuresByErrCode: Map<uint32, uint32>
      RemoteEcsTask: string
      SystemProbeConn: uint8

      LastTCPRtoCount: uint32
      LastTCPRecoveryCount: uint32
      LastTCPReordSeen: uint32
      LastTCPRcvOooPack: uint32
      LastTCPDeliveredCe: uint32
      LastTCPProbe0Count: uint32
      TCPEcnNegotiated: uint8

      AgentVersion: string
      RequestID: string }

module ProcessSnapshots =
    let table: Table<ProcessSnapshotRow> =
        { Table.create
              "storage_process_snapshots"
              "process_snapshots"
              [ "tenant_id"; "received_at"; "snapshot_id"
                "message_type"; "message_type_name"; "path"
                "header_version"; "header_encoding"; "header_subscription_id"
                "header_org_id"; "header_timestamp"; "frame_bytes"
                "host_name"; "network_id"; "group_id"; "group_size"; "container_host_type"
                "host_id"; "host_org_id"; "host_display_name"; "host_all_tags"
                "host_num_cpus"; "host_total_memory"; "host_tag_index"; "host_tags_modified"
                "sys_uuid"; "os_name"; "os_platform"; "os_family"; "os_version"
                "kernel_version"; "sys_total_memory"
                "cpu_numbers"; "cpu_vendors"; "cpu_families"; "cpu_models"
                "cpu_physical_ids"; "cpu_core_ids"; "cpu_cores"; "cpu_mhz"; "cpu_cache_sizes"
                "hint_mask"
                "rt_host_id"; "rt_org_id"; "rt_num_cpus"; "rt_total_memory"
                "process_count"; "process_stat_count"; "container_count"
                "container_stat_count"; "discovery_count"; "connection_count"
                "agent_hostname"; "agent_version"; "agent_container_count"; "request_id" ]
              (fun (r: ProcessSnapshotRow) ->
                  [| r.TenantID; r.ReceivedAt; r.SnapshotID
                     r.MessageType; r.MessageTypeName; r.Path
                     r.HeaderVersion; r.HeaderEncoding; r.HeaderSubscriptionID
                     r.HeaderOrgID; r.HeaderTimestamp; r.FrameBytes
                     r.HostName; r.NetworkID; r.GroupID; r.GroupSize; r.ContainerHostType
                     r.HostID; r.HostOrgID; r.HostDisplayName; r.HostAllTags
                     r.HostNumCPUs; r.HostTotalMemory; r.HostTagIndex; Col.opt r.HostTagsModified
                     r.SysUUID; r.OSName; r.OSPlatform; r.OSFamily; r.OSVersion
                     r.KernelVersion; r.SysTotalMemory
                     r.CPUNumbers; r.CPUVendors; r.CPUFamilies; r.CPUModels
                     r.CPUPhysicalIDs; r.CPUCoreIDs; r.CPUCores; r.CPUMhz; r.CPUCacheSizes
                     Col.opt r.HintMask
                     r.RTHostID; r.RTOrgID; r.RTNumCPUs; r.RTTotalMemory
                     r.ProcessCount; r.ProcessStatCount; r.ContainerCount
                     r.ContainerStatCount; r.DiscoveryCount; r.ConnectionCount
                     r.AgentHostname; r.AgentVersion; r.AgentContainerCount; r.RequestID |])
          with
              // One row per frame, wide but tiny: the timer does the flushing.
              MaxRows = 500
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 20_000
              MaxInFlight = 2 }

module ProcessStats =
    let table: Table<ProcessStatRow> =
        { Table.create
              "storage_process_stats"
              "process_stats"
              [ "tenant_id"; "timestamp"; "host"; "snapshot_id"
                "pid"; "key"; "create_time"; "has_create_time"
                "nice"; "threads"; "open_fds"
                "mem_rss"; "mem_vms"; "mem_swap"; "mem_shared"
                "mem_text"; "mem_lib"; "mem_data"; "mem_dirty"
                "cpu_last_cpu"; "cpu_total_pct"; "cpu_user_pct"; "cpu_system_pct"
                "cpu_num_threads"; "cpu_nice"; "cpu_user_time"; "cpu_system_time"
                "cpu_core_names"; "cpu_core_pcts"
                "io_read_rate"; "io_write_rate"; "io_read_bytes_rate"; "io_write_bytes_rate"
                "net_connection_rate"; "net_bytes_rate"
                "voluntary_ctx_switches"; "involuntary_ctx_switches"
                "container_id"; "container_state"; "process_state"
                "container_health"; "container_rbps"; "container_wbps"; "container_key"
                "container_net_rcvd_ps"; "container_net_sent_ps"
                "container_net_rcvd_bps"; "container_net_sent_bps"
                "byte_key"; "container_byte_key"
                "group_id"; "group_size"; "container_host_type"
                "rt_host_id"; "rt_org_id"; "rt_num_cpus"; "rt_total_memory"
                "agent_version"; "request_id" ]
              (fun (r: ProcessStatRow) ->
                  [| r.TenantID; r.Timestamp; r.Host; r.SnapshotID
                     r.PID; r.Key; r.CreateTime; r.HasCreateTime
                     r.Nice; r.Threads; r.OpenFDs
                     r.MemRSS; r.MemVMS; r.MemSwap; r.MemShared
                     r.MemText; r.MemLib; r.MemData; r.MemDirty
                     r.CPULastCPU; r.CPUTotalPct; r.CPUUserPct; r.CPUSystemPct
                     r.CPUNumThreads; r.CPUNice; r.CPUUserTime; r.CPUSystemTime
                     r.CPUCoreNames; r.CPUCorePcts
                     r.IOReadRate; r.IOWriteRate; r.IOReadBytesRate; r.IOWriteBytesRate
                     r.NetConnectionRate; r.NetBytesRate
                     r.VoluntaryCtxSwitches; r.InvoluntaryCtxSwitches
                     r.ContainerID; r.ContainerState; r.ProcessState
                     r.ContainerHealth; r.ContainerRbps; r.ContainerWbps; r.ContainerKey
                     r.ContainerNetRcvdPs; r.ContainerNetSentPs
                     r.ContainerNetRcvdBps; r.ContainerNetSentBps
                     r.ByteKey; r.ContainerByteKey
                     r.GroupID; r.GroupSize; r.ContainerHostType
                     r.RTHostID; r.RTOrgID; r.RTNumCPUs; r.RTTotalMemory
                     r.AgentVersion; r.RequestID |])
          with
              // The shape of processes at five times the cadence (2 s, not 10 s).
              MaxRows = 10000
              FlushInterval = TimeSpan.FromSeconds 3.0 }

module Containers =
    let table: Table<ContainerRow> =
        { Table.create
              "storage_containers"
              "containers"
              [ "tenant_id"; "timestamp"; "host"; "snapshot_id"; "source"; "pid"
                "type"; "id"; "name"; "image"; "repo_digest"
                "cpu_limit"; "memory_limit"; "cpu_request"; "memory_request"
                "state"; "health"; "created"; "started"
                "rbps"; "wbps"; "net_rcvd_ps"; "net_sent_ps"; "net_rcvd_bps"; "net_sent_bps"
                "user_pct"; "system_pct"; "total_pct"; "cpu_usage_ns"
                "mem_rss"; "mem_cache"; "mem_usage"; "mem_accounted"
                "thread_count"; "thread_limit"; "key"; "byte_key"
                "addr_ips"; "addr_ports"; "addr_protocols"; "tags"; "host_info"
                "network_id"; "group_id"; "group_size"; "container_host_type"
                "agent_version"; "request_id" ]
              (fun (r: ContainerRow) ->
                  [| r.TenantID; r.Timestamp; r.Host; r.SnapshotID; r.Source; r.PID
                     r.Type; r.ID; r.Name; r.Image; r.RepoDigest
                     r.CPULimit; r.MemoryLimit; r.CPURequest; r.MemoryRequest
                     r.State; r.Health; Col.opt r.Created; Col.opt r.Started
                     r.Rbps; r.Wbps; r.NetRcvdPs; r.NetSentPs; r.NetRcvdBps; r.NetSentBps
                     r.UserPct; r.SystemPct; r.TotalPct; r.CPUUsageNs
                     r.MemRSS; r.MemCache; r.MemUsage; r.MemAccounted
                     r.ThreadCount; r.ThreadLimit; r.Key; r.ByteKey
                     r.AddrIPs; r.AddrPorts; r.AddrProtocols; r.Tags; r.HostInfo
                     r.NetworkID; r.GroupID; r.GroupSize; r.ContainerHostType
                     r.AgentVersion; r.RequestID |])
          with
              // Tens to hundreds of rows per host per pass, each with tag maps
              // and address arrays: the rows carry real memory.
              MaxRows = 2000
              FlushInterval = TimeSpan.FromSeconds 5.0
              BufferLimit = 50_000
              MaxInFlight = 2 }

module ContainerStats =
    let table: Table<ContainerStatRow> =
        { Table.create
              "storage_container_stats"
              "container_stats"
              [ "tenant_id"; "timestamp"; "host"; "snapshot_id"; "source"; "id"
                "user_pct"; "system_pct"; "total_pct"; "cpu_limit"; "cpu_request"; "cpu_usage_ns"
                "mem_rss"; "mem_cache"; "mem_limit"; "mem_usage"; "mem_accounted"; "memory_request"
                "rbps"; "wbps"; "net_rcvd_ps"; "net_sent_ps"; "net_rcvd_bps"; "net_sent_bps"
                "state"; "health"; "key"; "started"; "byte_key"; "thread_count"; "thread_limit"
                "group_id"; "group_size"; "container_host_type"
                "rt_host_id"; "rt_org_id"; "rt_num_cpus"; "rt_total_memory"
                "agent_version"; "request_id" ]
              (fun (r: ContainerStatRow) ->
                  [| r.TenantID; r.Timestamp; r.Host; r.SnapshotID; r.Source; r.ID
                     r.UserPct; r.SystemPct; r.TotalPct; r.CPULimit; r.CPURequest; r.CPUUsageNs
                     r.MemRSS; r.MemCache; r.MemLimit; r.MemUsage; r.MemAccounted; r.MemoryRequest
                     r.Rbps; r.Wbps; r.NetRcvdPs; r.NetSentPs; r.NetRcvdBps; r.NetSentBps
                     r.State; r.Health; r.Key; Col.opt r.Started; r.ByteKey; r.ThreadCount; r.ThreadLimit
                     r.GroupID; r.GroupSize; r.ContainerHostType
                     r.RTHostID; r.RTOrgID; r.RTNumCPUs; r.RTTotalMemory
                     r.AgentVersion; r.RequestID |])
          with
              // Narrow numeric rows on the 2-second stream.
              MaxRows = 10000
              FlushInterval = TimeSpan.FromSeconds 3.0 }

module ProcessDiscoveries =
    let table: Table<ProcessDiscoveryRow> =
        { Table.create
              "storage_process_discoveries"
              "process_discoveries"
              [ "tenant_id"; "timestamp"; "host"; "snapshot_id"
                "pid"; "ns_pid"; "create_time"; "has_create_time"; "byte_key"
                "comm"; "exe"; "cmdline"; "args"; "cwd"; "root"; "on_disk"; "ppid"; "pgroup"
                "user"; "uid"; "gid"; "euid"; "egid"; "suid"; "sgid"; "host_info"
                "group_id"; "group_size"; "agent_version"; "request_id" ]
              (fun (r: ProcessDiscoveryRow) ->
                  [| r.TenantID; r.Timestamp; r.Host; r.SnapshotID
                     r.PID; r.NsPID; r.CreateTime; r.HasCreateTime; r.ByteKey
                     r.Comm; r.Exe; r.Cmdline; r.Args; r.Cwd; r.Root; r.OnDisk; r.PPID; r.Pgroup
                     r.User; r.UID; r.GID; r.EUID; r.EGID; r.SUID; r.SGID; r.HostInfo
                     r.GroupID; r.GroupSize; r.AgentVersion; r.RequestID |])
          with
              // A full process list per pass, like processes, with smaller rows.
              MaxRows = 10000
              FlushInterval = TimeSpan.FromSeconds 3.0 }

module Connections =
    let table: Table<ConnectionRow> =
        { Table.create
              "storage_connections"
              "connections"
              [ "tenant_id"; "timestamp"; "host"; "payload_id"; "pid"
                "laddr_ip"; "laddr_port"; "laddr_container_id"; "laddr_host_name"
                "raddr_ip"; "raddr_port"; "raddr_container_id"; "raddr_host_name"
                "family"; "type"; "direction"; "is_local_port_ephemeral"
                "last_bytes_sent"; "last_bytes_received"
                "last_packets_sent"; "last_packets_received"; "last_retransmits"
                "protocol_stack"; "net_ns"; "remote_network_id"
                "ip_translation_repl_src_ip"; "ip_translation_repl_dst_ip"
                "ip_translation_repl_src_port"; "ip_translation_repl_dst_port"
                "rtt"; "rtt_var"; "intra_host"
                "dns_successful_responses"; "dns_failed_responses"; "dns_timeouts"
                "dns_success_latency_sum"; "dns_failure_latency_sum"; "dns_count_by_rcode"
                "dns_stats_by_domain"; "dns_stats_by_domain_by_query_type"
                "dns_stats_by_domain_offset_by_query_type"
                "last_tcp_established"; "last_tcp_closed"
                "route_idx"; "route_target_idx"; "resolv_conf_idx"
                "http_aggregations"; "http2_aggregations"
                "data_streams_aggregations"; "database_aggregations"
                "tags_indices"; "tags_idx"; "tags_checksum"; "tags"
                "local_container_tags_index"; "local_container_tags"
                "remote_service_tags_idx"; "remote_service_tags"
                "state_index"; "tcp_failures_by_err_code"
                "remote_ecs_task"; "system_probe_conn"
                "last_tcp_rto_count"; "last_tcp_recovery_count"; "last_tcp_reord_seen"
                "last_tcp_rcv_ooo_pack"; "last_tcp_delivered_ce"; "last_tcp_probe0_count"
                "tcp_ecn_negotiated"; "agent_version"; "request_id" ]
              (fun (r: ConnectionRow) ->
                  [| r.TenantID; r.Timestamp; r.Host; r.PayloadID; r.PID
                     r.LaddrIP; r.LaddrPort; r.LaddrContainerID; r.LaddrHostName
                     r.RaddrIP; r.RaddrPort; r.RaddrContainerID; r.RaddrHostName
                     r.Family; r.Type; r.Direction; r.IsLocalPortEphemeral
                     r.LastBytesSent; r.LastBytesReceived
                     r.LastPacketsSent; r.LastPacketsReceived; r.LastRetransmits
                     r.ProtocolStack; r.NetNS; r.RemoteNetworkID
                     Col.opt r.IPTranslationReplSrcIP; Col.opt r.IPTranslationReplDstIP
                     Col.opt r.IPTranslationReplSrcPort; Col.opt r.IPTranslationReplDstPort
                     r.RTT; r.RTTVar; r.IntraHost
                     r.DNSSuccessfulResponses; r.DNSFailedResponses; r.DNSTimeouts
                     r.DNSSuccessLatencySum; r.DNSFailureLatencySum; r.DNSCountByRcode
                     r.DNSStatsByDomain; r.DNSStatsByDomainByQueryType
                     r.DNSStatsByDomainOffsetByQueryType
                     r.LastTCPEstablished; r.LastTCPClosed
                     r.RouteIdx; r.RouteTargetIdx; r.ResolvConfIdx
                     r.HTTPAggregations; r.HTTP2Aggregations
                     r.DataStreamsAggregations; r.DatabaseAggregations
                     r.TagsIndices; r.TagsIdx; r.TagsChecksum; r.Tags
                     r.LocalContainerTagsIndex; r.LocalContainerTags
                     r.RemoteServiceTagsIdx; r.RemoteServiceTags
                     r.StateIndex; r.TCPFailuresByErrCode
                     r.RemoteEcsTask; r.SystemProbeConn
                     r.LastTCPRtoCount; r.LastTCPRecoveryCount; r.LastTCPReordSeen
                     r.LastTCPRcvOooPack; r.LastTCPDeliveredCe; r.LastTCPProbe0Count
                     r.TCPEcnNegotiated; r.AgentVersion; r.RequestID |])
          with
              // The highest-volume table of this intake: a busy host reports
              // thousands of connections per frame.
              MaxRows = 20000
              FlushInterval = TimeSpan.FromSeconds 1.0
              BufferLimit = 500_000 }

module ConnectionsPayloads =
    let table: Table<ConnectionsPayloadRow> =
        { Table.create
              "storage_connections_payloads"
              "connections_payloads"
              [ "tenant_id"; "received_at"; "payload_id"
                "host_name"; "network_id"; "group_id"; "group_size"
                "container_host_type"; "connection_count"
                "architecture"; "kernel_version"; "platform"; "platform_version"
                "resolved_resources"; "container_for_pid"
                "encoded_tags"; "encoded_connections_tags"
                "host_tags_index"; "host_tags"
                "conn_telemetry"; "conn_telemetry_map"
                "compilation_telemetry"; "kernel_header_fetch_result"
                "core_telemetry"; "prebuilt_ebpf_assets"
                "routes"; "route_metadata"; "agent_configuration"
                "encoded_dns"; "domains"; "encoded_domain_database"; "encoded_dns_lookups"
                "dns_names"; "resolved_hosts_by_name"; "resolved_public_ips"
                "ecs_task"; "resolv_confs"
                "agent_hostname"; "agent_version"; "agent_container_count"; "request_id" ]
              (fun (r: ConnectionsPayloadRow) ->
                  [| r.TenantID; r.ReceivedAt; r.PayloadID
                     r.HostName; r.NetworkID; r.GroupID; r.GroupSize
                     r.ContainerHostType; r.ConnectionCount
                     r.Architecture; r.KernelVersion; r.Platform; r.PlatformVersion
                     r.ResolvedResources; r.ContainerForPID
                     r.EncodedTags; r.EncodedConnectionsTags
                     r.HostTagsIndex; r.HostTags
                     r.ConnTelemetry; r.ConnTelemetryMap
                     r.CompilationTelemetry; r.KernelHeaderFetchResult
                     r.CORETelemetry; r.PrebuiltEBPFAssets
                     r.Routes; r.RouteMetadata; r.AgentConfiguration
                     r.EncodedDNS; r.Domains; r.EncodedDomainDatabase; r.EncodedDNSLookups
                     r.DNSNames; r.ResolvedHostsByName; r.ResolvedPublicIPs
                     r.EcsTask; r.ResolvConfs
                     r.AgentHostname; r.AgentVersion; r.AgentContainerCount; r.RequestID |])
          with
              // One row per frame, but each carries the packed tag and DNS
              // buffers, kilobytes to megabytes: tight ceilings, one flush in
              // flight.
              MaxRows = 200
              FlushInterval = TimeSpan.FromSeconds 10.0
              BufferLimit = 5_000
              MaxInFlight = 1 }
