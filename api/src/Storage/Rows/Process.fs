namespace NinjaCat.Api.Storage.Rows

open System
open NinjaCat.Api.Storage

/// One process from a process-agent snapshot.
///
/// The field order is the column order. The first eighteen are the columns
/// the table shipped with; the rest were added with ALTER TABLE ... ADD COLUMN
/// and so come after them. New fields are appended, never inserted.
type ProcessRow =
    { TenantID: string
      Timestamp: DateTime
      Host: string
      PID: int32
      PPID: int32
      User: string
      Comm: string
      Exe: string
      Cmdline: string
      RSS: uint64
      VMS: uint64
      CPUPct: float32
      Threads: int32
      OpenFDs: int32
      State: string
      CreateTime: DateTime
      ContainerID: string
      Tags: Map<string, string[]>

      /// The frame this process arrived in; joins to process_snapshots.
      SnapshotID: Guid

      NsPID: int32
      Key: uint32

      Cwd: string
      Root: string
      OnDisk: uint8
      Pgroup: int32

      /// argv as sent. Cmdline is the same joined on spaces and cannot be
      /// split back: an argument containing a space looks like two.
      Args: string[]

      UID: int32
      GID: int32
      EUID: int32
      EGID: int32
      SUID: int32
      SGID: int32

      MemSwap: uint64
      MemShared: uint64
      MemText: uint64
      MemLib: uint64
      MemData: uint64
      MemDirty: uint64

      CPULastCPU: string
      CPUUserPct: float32
      CPUSystemPct: float32
      CPUNice: int32
      CPUUserTime: int64
      CPUSystemTime: int64

      /// Parallel arrays: index i of one pairs with index i of the other.
      CPUCoreNames: string[]
      CPUCorePcts: float32[]

      IOReadRate: float32
      IOWriteRate: float32
      IOReadBytesRate: float32
      IOWriteBytesRate: float32

      VoluntaryCtxSwitches: uint64
      InvoluntaryCtxSwitches: uint64

      NetConnectionRate: float32
      NetBytesRate: float32

      ProcessContext: string[]

      /// Enum NAMES (LANGUAGE_GO, INJECTION_INJECTED), never the numbers: a
      /// renumbering upstream would otherwise rewrite stored history.
      Language: string

      PortTCP: int32[]
      PortUDP: int32[]

      GeneratedServiceName: string
      GeneratedServiceNameSource: string
      DDServiceName: string
      DDServiceNameSource: string
      AdditionalGeneratedNames: string[]
      AdditionalGeneratedNameSources: string[]
      TracerRuntimeIDs: string[]
      TracerServiceNames: string[]
      APMInstrumentation: uint8
      /// The repeated oneof, as JSON.
      ServiceResources: string

      InjectionState: string

      ZombieChildrenCount: uint32
      ZombieNetRate: float
      HasZombieAggregation: uint8

      ContainerKey: uint32

      /// Opaque bytes off the wire, stored verbatim.
      ByteKey: byte[]
      ContainerByteKey: byte[]

      /// 0 means the agent sent no start time and CreateTime is meaningless,
      /// not 1970. The column is a plain DateTime and cannot say so itself.
      HasCreateTime: uint8

      /// Process.host as JSON: a field the backend fills in, nil from an agent.
      ProcessHost: string

      NetworkID: string
      GroupID: int32
      GroupSize: int32
      ContainerHostType: string
      AgentVersion: string
      RequestID: string
      HostUUID: string
      OSName: string
      OSPlatform: string
      OSFamily: string
      OSVersion: string
      KernelVersion: string }

module Processes =
    let table: Table<ProcessRow> =
        { Table.create
              "storage_processes"
              "processes"
              [ "tenant_id"; "timestamp"; "host"; "pid"; "ppid"; "user"; "comm"; "exe"; "cmdline"
                "rss"; "vms"; "cpu_pct"; "threads"; "open_fds"; "state"; "create_time"; "container_id"; "tags"
                "snapshot_id"; "ns_pid"; "key"
                "cwd"; "root"; "on_disk"; "pgroup"; "args"
                "uid"; "gid"; "euid"; "egid"; "suid"; "sgid"
                "mem_swap"; "mem_shared"; "mem_text"; "mem_lib"; "mem_data"; "mem_dirty"
                "cpu_last_cpu"; "cpu_user_pct"; "cpu_system_pct"; "cpu_nice"
                "cpu_user_time"; "cpu_system_time"; "cpu_core_names"; "cpu_core_pcts"
                "io_read_rate"; "io_write_rate"; "io_read_bytes_rate"; "io_write_bytes_rate"
                "voluntary_ctx_switches"; "involuntary_ctx_switches"
                "net_connection_rate"; "net_bytes_rate"
                "process_context"; "language"; "port_tcp"; "port_udp"
                "generated_service_name"; "generated_service_name_source"
                "dd_service_name"; "dd_service_name_source"
                "additional_generated_names"; "additional_generated_name_sources"
                "tracer_runtime_ids"; "tracer_service_names"
                "apm_instrumentation"; "service_resources"; "injection_state"
                "zombie_children_count"; "zombie_net_rate"; "has_zombie_aggregation"
                "container_key"; "byte_key"; "container_byte_key"
                "has_create_time"; "process_host"
                "network_id"; "group_id"; "group_size"; "container_host_type"
                "agent_version"; "request_id"
                "host_uuid"; "os_name"; "os_platform"; "os_family"; "os_version"; "kernel_version" ]
              (fun (r: ProcessRow) ->
                  [| r.TenantID; r.Timestamp; r.Host; r.PID; r.PPID; r.User; r.Comm; r.Exe; r.Cmdline
                     r.RSS; r.VMS; r.CPUPct; r.Threads; r.OpenFDs; r.State; r.CreateTime; r.ContainerID; r.Tags
                     r.SnapshotID; r.NsPID; r.Key
                     r.Cwd; r.Root; r.OnDisk; r.Pgroup; r.Args
                     r.UID; r.GID; r.EUID; r.EGID; r.SUID; r.SGID
                     r.MemSwap; r.MemShared; r.MemText; r.MemLib; r.MemData; r.MemDirty
                     r.CPULastCPU; r.CPUUserPct; r.CPUSystemPct; r.CPUNice
                     r.CPUUserTime; r.CPUSystemTime; r.CPUCoreNames; r.CPUCorePcts
                     r.IOReadRate; r.IOWriteRate; r.IOReadBytesRate; r.IOWriteBytesRate
                     r.VoluntaryCtxSwitches; r.InvoluntaryCtxSwitches
                     r.NetConnectionRate; r.NetBytesRate
                     r.ProcessContext; r.Language; r.PortTCP; r.PortUDP
                     r.GeneratedServiceName; r.GeneratedServiceNameSource
                     r.DDServiceName; r.DDServiceNameSource
                     r.AdditionalGeneratedNames; r.AdditionalGeneratedNameSources
                     r.TracerRuntimeIDs; r.TracerServiceNames
                     r.APMInstrumentation; r.ServiceResources; r.InjectionState
                     r.ZombieChildrenCount; r.ZombieNetRate; r.HasZombieAggregation
                     r.ContainerKey; r.ByteKey; r.ContainerByteKey
                     r.HasCreateTime; r.ProcessHost
                     r.NetworkID; r.GroupID; r.GroupSize; r.ContainerHostType
                     r.AgentVersion; r.RequestID
                     r.HostUUID; r.OSName; r.OSPlatform; r.OSFamily; r.OSVersion; r.KernelVersion |])
          with
              // A few hundred rows per host per snapshot, every ~10 s.
              MaxRows = 10000
              FlushInterval = TimeSpan.FromSeconds 3.0 }
