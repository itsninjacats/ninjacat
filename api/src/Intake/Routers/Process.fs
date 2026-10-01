/// process.<site> — the process-agent.
///
///   agent config: process_config.process_dd_url / DD_PROCESS_CONFIG_PROCESS_DD_URL
///
/// The only intake with its own 16-byte frame instead of plain protobuf. All
/// four paths share the frame and the ResCollector reply; the message type in
/// the frame says what the body is, not the path.
///
///   every frame, any type      -> process_snapshots (the envelope)
///   CollectorProc              -> processes, containers
///   CollectorRealTime          -> process_stats, container_stats
///   CollectorContainer         -> containers
///   CollectorContainerRealTime -> container_stats
///   CollectorProcDiscovery     -> process_discoveries
///   CollectorConnections       -> connections_payloads, connections
///
/// Every item row carries the frame's snapshot id back to process_snapshots;
/// the connections pair calls the same id payload_id. A frame that does not
/// decode goes to raw_payloads as decode_error, one that decodes into a type
/// its route has no table for as unexpected_shape.
module NinjaCat.Api.Intake.Routers.Process

open System
open System.Collections.Generic
open System.IO
open System.Text
open System.Text.Encodings.Web
open System.Text.Json
open Google.Protobuf.Collections
open Google.Protobuf.Reflection
// Before the intake's own namespaces: its Route and the Connections table
// must win over the protobuf classes of the same names.
open Datadog.ProcessAgent
open Microsoft.Extensions.Logging
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers.ProcessBuffers
open NinjaCat.Api.Storage
open NinjaCat.Api.Storage.Rows

/// The agent omits every sub-message it could not read. An empty one in its
/// place leaves the columns it feeds at zero.
let private orEmpty (message: 'T) : 'T when 'T: null and 'T: (new: unit -> 'T) =
    if isNull message then new 'T() else message

/// What a row holds when the agent sent no start time: Go's zero time. The
/// has_create_time column beside it says so.
let private noTime = DateTime(0L, DateTimeKind.Utc)

// Go's time holds any int64. ClickHouse's DateTime ends in 2106, and the
// driver refuses a whole batch over one value past that; .NET's DateTime ends
// with the year 9999. A time beyond the end is pinned to it.
let private lastCreateMillis = int64 UInt32.MaxValue * 1000L
let private lastSeconds = DateTimeOffset.MaxValue.ToUnixTimeSeconds()

/// A start time in Unix milliseconds, and whether there was one.
let private createTime (millis: int64) : DateTime * uint8 =
    if millis > 0L then
        Time.fromUnixMillis (min millis lastCreateMillis), 1uy
    else
        noTime, 0uy

/// Unix seconds, or None when the runtime never reported a time: it must not
/// become 1970-01-01.
let private secondsOrNone (seconds: int64) : DateTime option =
    Time.optionalSeconds true (min seconds lastSeconds)

/// The sender identity the process-agent stamps on every submit. Every table
/// of this intake keeps at least the version and the request id: "which agent
/// wrote this row" is the first question asked of a payload that looks wrong.
type AgentIdentity =
    { Hostname: string
      Version: string
      ContainerCount: string
      /// Only the process and connections checks send one.
      RequestID: string }

/// What is known about a received frame apart from its header and body.
type FrameInfo =
    { Tenant: string
      /// Minted on arrival; every row the frame produces carries it.
      SnapshotID: Guid
      /// ARRIVAL time. The body carries no sample time (CreateTime is a
      /// process start) and the agent leaves the header's timestamp at zero.
      /// Snapshots travel within a second of being taken.
      At: DateTime
      Path: string
      Bytes: int
      Agent: AgentIdentity }

let private frameInfo (r: Request) : FrameInfo =
    { Tenant = r.Tenant
      SnapshotID = Guid.NewGuid()
      At = DateTime.UtcNow
      Path = r.Path
      Bytes = r.Body.Length
      Agent =
        { Hostname = r.Header "X-Dd-Hostname"
          Version = r.Header "X-Dd-Processagentversion"
          ContainerCount = r.Header "X-Dd-ContainerCount"
          RequestID = r.Header "X-DD-Request-ID" } }

/// The number is the contract and is stored too; the name is what a human reads.
let private messageTypeName (messageType: uint8) : string =
    match messageType with
    | 12uy -> "CollectorProc"
    | 22uy -> "CollectorConnections"
    | 27uy -> "CollectorRealTime"
    | 39uy -> "CollectorContainer"
    | 40uy -> "CollectorContainerRealTime"
    | 53uy -> "CollectorProcDiscovery"
    | other -> $"type_{other}"

let private withHost (host: Host) (row: ProcessSnapshotRow) : ProcessSnapshotRow =
    if isNull host then
        row
    else
        { row with
            HostID = host.Id
            HostOrgID = host.OrgId
            HostDisplayName = host.Name
            HostAllTags = Tags.toMultiMap host.AllTags
            HostNumCPUs = host.NumCpus
            HostTotalMemory = host.TotalMemory
            HostTagIndex = host.TagIndex
            HostTagsModified = secondsOrNone host.TagsModified }

let private withSystemInfo (info: SystemInfo) (row: ProcessSnapshotRow) : ProcessSnapshotRow =
    if isNull info then
        row
    else
        let os = orEmpty info.Os

        { row with
            SysUUID = info.Uuid
            SysTotalMemory = info.TotalMemory
            OSName = os.Name
            OSPlatform = os.Platform
            OSFamily = os.Family
            OSVersion = os.Version
            KernelVersion = os.KernelVersion
            // Parallel arrays in the order sent. A map keyed by number would
            // drop a host that reports two sockets with the same numbering.
            CPUNumbers = info.Cpus |> Seq.map _.Number |> Array.ofSeq
            CPUVendors = info.Cpus |> Seq.map _.Vendor |> Array.ofSeq
            CPUFamilies = info.Cpus |> Seq.map _.Family |> Array.ofSeq
            CPUModels = info.Cpus |> Seq.map _.Model |> Array.ofSeq
            CPUPhysicalIDs = info.Cpus |> Seq.map _.PhysicalId |> Array.ofSeq
            CPUCoreIDs = info.Cpus |> Seq.map _.CoreId |> Array.ofSeq
            CPUCores = info.Cpus |> Seq.map _.Cores |> Array.ofSeq
            CPUMhz = info.Cpus |> Seq.map _.Mhz |> Array.ofSeq
            CPUCacheSizes = info.Cpus |> Seq.map _.CacheSize |> Array.ofSeq }

/// The envelope row of one frame. The six message types spread the same
/// information differently: CollectorProc and its kin carry a Host and a
/// SystemInfo, the realtime ones inline a host id, a cpu count and a memory
/// total instead.
let snapshotRow (info: FrameInfo) (frame: Frame) : ProcessSnapshotRow =
    let row: ProcessSnapshotRow =
        { TenantID = info.Tenant
          ReceivedAt = info.At
          SnapshotID = info.SnapshotID
          MessageType = frame.Header.Type
          MessageTypeName = messageTypeName frame.Header.Type
          Path = info.Path
          HeaderVersion = frame.Header.Version
          HeaderEncoding = frame.Header.Encoding
          HeaderSubscriptionID = frame.Header.SubscriptionID
          HeaderOrgID = frame.Header.OrgID
          HeaderTimestamp = frame.Header.Timestamp
          FrameBytes = uint64 info.Bytes
          HostName = ""
          NetworkID = ""
          GroupID = 0
          GroupSize = 0
          ContainerHostType = ""
          HostID = 0L
          HostOrgID = 0
          HostDisplayName = ""
          HostAllTags = Map.empty
          HostNumCPUs = 0
          HostTotalMemory = 0L
          HostTagIndex = 0
          HostTagsModified = None
          SysUUID = ""
          OSName = ""
          OSPlatform = ""
          OSFamily = ""
          OSVersion = ""
          KernelVersion = ""
          SysTotalMemory = 0L
          CPUNumbers = [||]
          CPUVendors = [||]
          CPUFamilies = [||]
          CPUModels = [||]
          CPUPhysicalIDs = [||]
          CPUCoreIDs = [||]
          CPUCores = [||]
          CPUMhz = [||]
          CPUCacheSizes = [||]
          HintMask = None
          RTHostID = 0L
          RTOrgID = 0
          RTNumCPUs = 0
          RTTotalMemory = 0L
          ProcessCount = 0u
          ProcessStatCount = 0u
          ContainerCount = 0u
          ContainerStatCount = 0u
          DiscoveryCount = 0u
          ConnectionCount = 0u
          AgentHostname = info.Agent.Hostname
          AgentVersion = info.Agent.Version
          AgentContainerCount = info.Agent.ContainerCount
          RequestID = info.Agent.RequestID }

    match frame.Body with
    | :? CollectorProc as b ->
        { (row |> withHost b.Host |> withSystemInfo b.Info) with
            HostName = b.HostName
            NetworkID = b.NetworkId
            GroupID = b.GroupId
            GroupSize = b.GroupSize
            ContainerHostType = ProcessEnum.name b.ContainerHostType
            ProcessCount = uint32 b.Processes.Count
            ContainerCount = uint32 b.Containers.Count
            // "No hints" and "hint mask 0" are different statements.
            HintMask = (if b.HintsCase = CollectorProc.HintsOneofCase.HintMask then Some b.HintMask else None) }
    | :? CollectorRealTime as b ->
        { row with
            HostName = b.HostName
            GroupID = b.GroupId
            GroupSize = b.GroupSize
            ContainerHostType = ProcessEnum.name b.ContainerHostType
            RTHostID = b.HostId
            RTOrgID = b.OrgId
            RTNumCPUs = b.NumCpus
            RTTotalMemory = b.TotalMemory
            ProcessStatCount = uint32 b.Stats.Count
            ContainerStatCount = uint32 b.ContainerStats.Count }
    | :? CollectorContainer as b ->
        { (row |> withHost b.Host |> withSystemInfo b.Info) with
            HostName = b.HostName
            NetworkID = b.NetworkId
            GroupID = b.GroupId
            GroupSize = b.GroupSize
            ContainerHostType = ProcessEnum.name b.ContainerHostType
            ContainerCount = uint32 b.Containers.Count }
    | :? CollectorContainerRealTime as b ->
        { row with
            HostName = b.HostName
            GroupID = b.GroupId
            GroupSize = b.GroupSize
            ContainerHostType = ProcessEnum.name b.ContainerHostType
            RTHostID = b.HostId
            RTNumCPUs = b.NumCpus
            RTTotalMemory = b.TotalMemory
            ContainerStatCount = uint32 b.Stats.Count }
    | :? CollectorProcDiscovery as b ->
        { (row |> withHost b.Host) with
            HostName = b.HostName
            GroupID = b.GroupId
            GroupSize = b.GroupSize
            DiscoveryCount = uint32 b.ProcessDiscoveries.Count }
    | :? CollectorConnections as b ->
        { row with
            HostName = b.HostName
            NetworkID = b.NetworkId
            GroupID = b.GroupId
            GroupSize = b.GroupSize
            ContainerHostType = ProcessEnum.name b.ContainerHostType
            ConnectionCount = uint32 b.Connections.Count }
    | _ -> row

/// One process of a snapshot. Every field of Process reaches a column;
/// Process.host goes to a JSON column and Process.container to the
/// containers table.
let processRow (info: FrameInfo) (proc: CollectorProc) (p: Process) : ProcessRow =
    let command = orEmpty p.Command
    let user = orEmpty p.User
    let memory = orEmpty p.Memory
    let cpu = orEmpty p.Cpu
    let io = orEmpty p.IoStat
    let networks = orEmpty p.Networks
    let ports = orEmpty p.PortInfo
    // The bridge between a process and APM: tracer_runtime_ids is the only
    // join between "this pid on this host" and the traces it emitted.
    let discovery = orEmpty p.ServiceDiscovery
    let generatedName = discovery.GeneratedServiceName
    let ddName = discovery.DdServiceName
    let system = orEmpty proc.Info
    let os = orEmpty system.Os
    let created, hasCreateTime = createTime p.CreateTime

    { TenantID = info.Tenant
      Timestamp = info.At
      Host = proc.HostName
      PID = p.Pid
      PPID = command.Ppid
      User = user.Name
      Comm = command.Comm
      Exe = command.Exe
      Cmdline = String.Join(" ", command.Args)
      RSS = memory.Rss
      VMS = memory.Vms
      CPUPct = cpu.TotalPct
      Threads = cpu.NumThreads
      OpenFDs = p.OpenFdCount
      State = ProcessEnum.name p.State
      CreateTime = created
      ContainerID = p.ContainerId
      Tags = Tags.toMultiMap p.Tags
      SnapshotID = info.SnapshotID
      NsPID = p.NsPid
      Key = p.Key
      Cwd = command.Cwd
      Root = command.Root
      OnDisk = Text.flag command.OnDisk
      Pgroup = command.Pgroup
      Args = Array.ofSeq command.Args
      UID = user.Uid
      GID = user.Gid
      EUID = user.Euid
      EGID = user.Egid
      SUID = user.Suid
      SGID = user.Sgid
      MemSwap = memory.Swap
      MemShared = memory.Shared
      MemText = memory.Text
      MemLib = memory.Lib
      MemData = memory.Data
      MemDirty = memory.Dirty
      CPULastCPU = cpu.LastCpu
      CPUUserPct = cpu.UserPct
      CPUSystemPct = cpu.SystemPct
      CPUNice = cpu.Nice
      CPUUserTime = cpu.UserTime
      CPUSystemTime = cpu.SystemTime
      CPUCoreNames = cpu.Cpus |> Seq.map _.Name |> Array.ofSeq
      CPUCorePcts = cpu.Cpus |> Seq.map _.TotalPct |> Array.ofSeq
      IOReadRate = io.ReadRate
      IOWriteRate = io.WriteRate
      IOReadBytesRate = io.ReadBytesRate
      IOWriteBytesRate = io.WriteBytesRate
      VoluntaryCtxSwitches = p.VoluntaryCtxSwitches
      InvoluntaryCtxSwitches = p.InvoluntaryCtxSwitches
      NetConnectionRate = networks.ConnectionRate
      NetBytesRate = networks.BytesRate
      ProcessContext = Array.ofSeq p.ProcessContext
      Language = ProcessEnum.name p.Language
      PortTCP = Array.ofSeq ports.Tcp
      PortUDP = Array.ofSeq ports.Udp
      GeneratedServiceName = (if isNull generatedName then "" else generatedName.Name)
      GeneratedServiceNameSource = (if isNull generatedName then "" else ProcessEnum.name generatedName.Source)
      DDServiceName = (if isNull ddName then "" else ddName.Name)
      DDServiceNameSource = (if isNull ddName then "" else ProcessEnum.name ddName.Source)
      // Parallel arrays, in the order sent: name i came from source i.
      AdditionalGeneratedNames = discovery.AdditionalGeneratedNames |> Seq.map _.Name |> Array.ofSeq
      AdditionalGeneratedNameSources = discovery.AdditionalGeneratedNames |> Seq.map (fun n -> ProcessEnum.name n.Source) |> Array.ofSeq
      TracerRuntimeIDs = discovery.TracerMetadata |> Seq.map _.RuntimeId |> Array.ofSeq
      TracerServiceNames = discovery.TracerMetadata |> Seq.map _.ServiceName |> Array.ofSeq
      APMInstrumentation = Text.flag discovery.ApmInstrumentation
      ServiceResources = ProtoJson.messages discovery.Resources
      InjectionState = ProcessEnum.name p.InjectionState
      ZombieChildrenCount = p.ZombieChildrenCount
      ZombieNetRate = p.ZombieNetRate
      HasZombieAggregation = Text.flag p.HasZombieAggregation
      ContainerKey = p.ContainerKey
      ByteKey = p.ByteKey.ToByteArray()
      ContainerByteKey = p.ContainerByteKey.ToByteArray()
      HasCreateTime = hasCreateTime
      ProcessHost = ProtoJson.message p.Host
      NetworkID = proc.NetworkId
      GroupID = proc.GroupId
      GroupSize = proc.GroupSize
      ContainerHostType = ProcessEnum.name proc.ContainerHostType
      AgentVersion = info.Agent.Version
      RequestID = info.Agent.RequestID
      HostUUID = system.Uuid
      OSName = os.Name
      OSPlatform = os.Platform
      OSFamily = os.Family
      OSVersion = os.Version
      KernelVersion = os.KernelVersion }

let processStatRow (info: FrameInfo) (realTime: CollectorRealTime) (s: ProcessStat) : ProcessStatRow =
    let memory = orEmpty s.Memory
    let cpu = orEmpty s.Cpu
    let io = orEmpty s.IoStat
    let networks = orEmpty s.Networks
    let created, hasCreateTime = createTime s.CreateTime

    { TenantID = info.Tenant
      Timestamp = info.At
      Host = realTime.HostName
      SnapshotID = info.SnapshotID
      PID = s.Pid
      Key = s.Key
      CreateTime = created
      HasCreateTime = hasCreateTime
      // The message's own fields, not the nested CPUStat's.
      Nice = s.Nice
      Threads = s.Threads
      OpenFDs = s.OpenFdCount
      MemRSS = memory.Rss
      MemVMS = memory.Vms
      MemSwap = memory.Swap
      MemShared = memory.Shared
      MemText = memory.Text
      MemLib = memory.Lib
      MemData = memory.Data
      MemDirty = memory.Dirty
      CPULastCPU = cpu.LastCpu
      CPUTotalPct = cpu.TotalPct
      CPUUserPct = cpu.UserPct
      CPUSystemPct = cpu.SystemPct
      CPUNumThreads = cpu.NumThreads
      CPUNice = cpu.Nice
      CPUUserTime = cpu.UserTime
      CPUSystemTime = cpu.SystemTime
      CPUCoreNames = cpu.Cpus |> Seq.map _.Name |> Array.ofSeq
      CPUCorePcts = cpu.Cpus |> Seq.map _.TotalPct |> Array.ofSeq
      IOReadRate = io.ReadRate
      IOWriteRate = io.WriteRate
      IOReadBytesRate = io.ReadBytesRate
      IOWriteBytesRate = io.WriteBytesRate
      NetConnectionRate = networks.ConnectionRate
      NetBytesRate = networks.BytesRate
      VoluntaryCtxSwitches = s.VoluntaryCtxSwitches
      InvoluntaryCtxSwitches = s.InvoluntaryCtxSwitches
      ContainerID = s.ContainerId
      ContainerState = ProcessEnum.name s.ContainerState
      ProcessState = ProcessEnum.name s.ProcessState
      ContainerHealth = ProcessEnum.name s.ContainerHealth
      ContainerRbps = s.ContainerRbps
      ContainerWbps = s.ContainerWbps
      ContainerKey = s.ContainerKey
      ContainerNetRcvdPs = s.ContainerNetRcvdPs
      ContainerNetSentPs = s.ContainerNetSentPs
      ContainerNetRcvdBps = s.ContainerNetRcvdBps
      ContainerNetSentBps = s.ContainerNetSentBps
      ByteKey = s.ByteKey.ToByteArray()
      ContainerByteKey = s.ContainerByteKey.ToByteArray()
      GroupID = realTime.GroupId
      GroupSize = realTime.GroupSize
      ContainerHostType = ProcessEnum.name realTime.ContainerHostType
      RTHostID = realTime.HostId
      RTOrgID = realTime.OrgId
      RTNumCPUs = realTime.NumCpus
      RTTotalMemory = realTime.TotalMemory
      AgentVersion = info.Agent.Version
      RequestID = info.Agent.RequestID }

/// What a container row inherits from the message it hung off. The same
/// Container arrives three ways, and what is populated differs.
type ContainerEnvelope =
    { /// collector_proc (the snapshot's inventory), process (the copy on one
      /// Process) or collector_container.
      Source: string
      /// The owning process, for Source = "process".
      PID: int32
      NetworkID: string
      GroupID: int32
      GroupSize: int32
      ContainerHostType: string }

let containerRow (info: FrameInfo) (envelope: ContainerEnvelope) (host: string) (c: Container) : ContainerRow =
    { TenantID = info.Tenant
      Timestamp = info.At
      Host = host
      SnapshotID = info.SnapshotID
      Source = envelope.Source
      PID = envelope.PID
      Type = c.Type
      ID = c.Id
      Name = c.Name
      Image = c.Image
      RepoDigest = c.RepoDigest
      CPULimit = c.CpuLimit
      MemoryLimit = c.MemoryLimit
      CPURequest = c.CpuRequest
      MemoryRequest = c.MemoryRequest
      State = ProcessEnum.name c.State
      Health = ProcessEnum.name c.Health
      // Unix SECONDS on the wire: the agent sends CreatedAt.Unix().
      Created = secondsOrNone c.Created
      Started = secondsOrNone c.Started
      Rbps = c.Rbps
      Wbps = c.Wbps
      NetRcvdPs = c.NetRcvdPs
      NetSentPs = c.NetSentPs
      NetRcvdBps = c.NetRcvdBps
      NetSentBps = c.NetSentBps
      UserPct = c.UserPct
      SystemPct = c.SystemPct
      TotalPct = c.TotalPct
      CPUUsageNs = c.CpuUsageNs
      MemRSS = c.MemRss
      MemCache = c.MemCache
      MemUsage = c.MemUsage
      MemAccounted = c.MemAccounted
      ThreadCount = c.ThreadCount
      ThreadLimit = c.ThreadLimit
      Key = c.Key
      ByteKey = c.ByteKey.ToByteArray()
      // Parallel arrays, order as sent: a container legitimately binds the
      // same port on two addresses.
      AddrIPs = c.Addresses |> Seq.map _.Ip |> Array.ofSeq
      AddrPorts = c.Addresses |> Seq.map _.Port |> Array.ofSeq
      AddrProtocols = c.Addresses |> Seq.map (fun a -> ProcessEnum.name a.Protocol) |> Array.ofSeq
      Tags = Tags.toMultiMap c.Tags
      HostInfo = ProtoJson.message c.Host
      NetworkID = envelope.NetworkID
      GroupID = envelope.GroupID
      GroupSize = envelope.GroupSize
      ContainerHostType = envelope.ContainerHostType
      AgentVersion = info.Agent.Version
      RequestID = info.Agent.RequestID }

/// Both places a Container hides inside a CollectorProc: the snapshot's
/// inventory, and the copy on an individual Process. The per-process one
/// carries limits and health the inventory sometimes lacks, and may be the
/// only record of a container the inventory missed.
let procContainerRows (info: FrameInfo) (proc: CollectorProc) : ContainerRow[] =
    let inventory: ContainerEnvelope =
        { Source = "collector_proc"
          PID = 0
          NetworkID = proc.NetworkId
          GroupID = proc.GroupId
          GroupSize = proc.GroupSize
          ContainerHostType = ProcessEnum.name proc.ContainerHostType }

    let fromInventory =
        proc.Containers |> Seq.map (containerRow info inventory proc.HostName) |> Array.ofSeq

    let fromProcesses =
        proc.Processes
        |> Seq.filter (fun p -> not (isNull p.Container))
        |> Seq.map (fun p -> containerRow info { inventory with Source = "process"; PID = p.Pid } proc.HostName p.Container)
        |> Array.ofSeq

    Array.append fromInventory fromProcesses

let private inventoryContainerRows (info: FrameInfo) (inventory: CollectorContainer) : ContainerRow[] =
    let envelope: ContainerEnvelope =
        { Source = "collector_container"
          PID = 0
          NetworkID = inventory.NetworkId
          GroupID = inventory.GroupId
          GroupSize = inventory.GroupSize
          ContainerHostType = ProcessEnum.name inventory.ContainerHostType }

    inventory.Containers |> Seq.map (containerRow info envelope inventory.HostName) |> Array.ofSeq

/// What a container stat row inherits from its message.
type ContainerStatEnvelope =
    { /// collector_realtime or collector_container_realtime.
      Source: string
      Host: string
      GroupID: int32
      GroupSize: int32
      ContainerHostType: string
      RTHostID: int64
      RTOrgID: int32
      RTNumCPUs: int32
      RTTotalMemory: int64 }

let containerStatRow (info: FrameInfo) (envelope: ContainerStatEnvelope) (s: ContainerStat) : ContainerStatRow =
    { TenantID = info.Tenant
      Timestamp = info.At
      Host = envelope.Host
      SnapshotID = info.SnapshotID
      Source = envelope.Source
      ID = s.Id
      UserPct = s.UserPct
      SystemPct = s.SystemPct
      TotalPct = s.TotalPct
      CPULimit = s.CpuLimit
      CPURequest = s.CpuRequest
      CPUUsageNs = s.CpuUsageNs
      MemRSS = s.MemRss
      MemCache = s.MemCache
      MemLimit = s.MemLimit
      MemUsage = s.MemUsage
      MemAccounted = s.MemAccounted
      MemoryRequest = s.MemoryRequest
      Rbps = s.Rbps
      Wbps = s.Wbps
      NetRcvdPs = s.NetRcvdPs
      NetSentPs = s.NetSentPs
      NetRcvdBps = s.NetRcvdBps
      NetSentBps = s.NetSentBps
      State = ProcessEnum.name s.State
      Health = ProcessEnum.name s.Health
      Key = s.Key
      Started = secondsOrNone s.Started
      ByteKey = s.ByteKey.ToByteArray()
      ThreadCount = s.ThreadCount
      ThreadLimit = s.ThreadLimit
      GroupID = envelope.GroupID
      GroupSize = envelope.GroupSize
      ContainerHostType = envelope.ContainerHostType
      RTHostID = envelope.RTHostID
      RTOrgID = envelope.RTOrgID
      RTNumCPUs = envelope.RTNumCPUs
      RTTotalMemory = envelope.RTTotalMemory
      AgentVersion = info.Agent.Version
      RequestID = info.Agent.RequestID }

let private realTimeContainerStatRows (info: FrameInfo) (realTime: CollectorRealTime) : ContainerStatRow[] =
    let envelope: ContainerStatEnvelope =
        { Source = "collector_realtime"
          Host = realTime.HostName
          GroupID = realTime.GroupId
          GroupSize = realTime.GroupSize
          ContainerHostType = ProcessEnum.name realTime.ContainerHostType
          RTHostID = realTime.HostId
          RTOrgID = realTime.OrgId
          RTNumCPUs = realTime.NumCpus
          RTTotalMemory = realTime.TotalMemory }

    realTime.ContainerStats |> Seq.map (containerStatRow info envelope) |> Array.ofSeq

let private containerRealTimeStatRows (info: FrameInfo) (realTime: CollectorContainerRealTime) : ContainerStatRow[] =
    let envelope: ContainerStatEnvelope =
        { Source = "collector_container_realtime"
          Host = realTime.HostName
          GroupID = realTime.GroupId
          GroupSize = realTime.GroupSize
          ContainerHostType = ProcessEnum.name realTime.ContainerHostType
          RTHostID = realTime.HostId
          // CollectorContainerRealTime carries no org id.
          RTOrgID = 0
          RTNumCPUs = realTime.NumCpus
          RTTotalMemory = realTime.TotalMemory }

    realTime.Stats |> Seq.map (containerStatRow info envelope) |> Array.ofSeq

let discoveryRow (info: FrameInfo) (discoveries: CollectorProcDiscovery) (d: ProcessDiscovery) : ProcessDiscoveryRow =
    let command = orEmpty d.Command
    let user = orEmpty d.User
    let created, hasCreateTime = createTime d.CreateTime

    { TenantID = info.Tenant
      Timestamp = info.At
      Host = discoveries.HostName
      SnapshotID = info.SnapshotID
      PID = d.Pid
      NsPID = d.NsPid
      CreateTime = created
      HasCreateTime = hasCreateTime
      ByteKey = d.ByteKey.ToByteArray()
      Comm = command.Comm
      Exe = command.Exe
      Cmdline = String.Join(" ", command.Args)
      Args = Array.ofSeq command.Args
      Cwd = command.Cwd
      Root = command.Root
      OnDisk = Text.flag command.OnDisk
      PPID = command.Ppid
      Pgroup = command.Pgroup
      User = user.Name
      UID = user.Uid
      GID = user.Gid
      EUID = user.Euid
      EGID = user.Egid
      SUID = user.Suid
      SGID = user.Sgid
      HostInfo = ProtoJson.message d.Host
      GroupID = discoveries.GroupId
      GroupSize = discoveries.GroupSize
      AgentVersion = info.Agent.Version
      RequestID = info.Agent.RequestID }

/// The fixed telemetry message agents before 7.35 sent, in the name/value
/// shape of the open-ended map newer ones send, so both eras read the same
/// way. Empty when the agent sent none, not eleven zeros.
let connTelemetry (telemetry: CollectorConnectionsTelemetry) : Map<string, int64> =
    if isNull telemetry then
        Map.empty
    else
        Map
            [ "kprobes_triggered", telemetry.KprobesTriggered
              "kprobes_missed", telemetry.KprobesMissed
              "conntrack_registers", telemetry.ConntrackRegisters
              "conntrack_registers_dropped", telemetry.ConntrackRegistersDropped
              "dns_packets_processed", telemetry.DnsPacketsProcessed
              "conns_closed", telemetry.ConnsClosed
              "conns_bpf_map_size", telemetry.ConnsBpfMapSize
              "udp_sends_processed", telemetry.UdpSendsProcessed
              "udp_sends_missed", telemetry.UdpSendsMissed
              "conntrack_sampling_percent", telemetry.ConntrackSamplingPercent
              "dns_stats_dropped", telemetry.DnsStatsDropped ]

/// A tag set of the payload's encodedTags: the host's, a container's, a
/// remote service's.
///
/// The agent puts a set of one tag, "-", first in that buffer, so that no
/// real set has index 0 (pkg/process/checks/net.go). An index that was not
/// sent reads as 0 and lands on it: that set means "none".
let private sharedTags (encodedTags: byte[]) (index: int) : string list =
    match TagBuffer.tags encodedTags index with
    | [ "-" ] -> []
    | tags -> tags

/// The per-frame half of a connections payload. `dnsNames` is what
/// DnsBuffer.names made of the payload's DNS buffers.
let connectionsPayloadRow (info: FrameInfo) (conns: CollectorConnections) (dnsNames: string[]) : ConnectionsPayloadRow =
    let encodedTags = conns.EncodedTags.ToByteArray()

    { TenantID = info.Tenant
      ReceivedAt = info.At
      PayloadID = info.SnapshotID
      HostName = conns.HostName
      NetworkID = conns.NetworkId
      GroupID = conns.GroupId
      GroupSize = conns.GroupSize
      ContainerHostType = ProcessEnum.name conns.ContainerHostType
      ConnectionCount = uint32 conns.Connections.Count
      Architecture = conns.Architecture
      KernelVersion = conns.KernelVersion
      Platform = conns.Platform
      PlatformVersion = conns.PlatformVersion
      ResolvedResources = ProtoJson.messageMap conns.ResolvedResources
      ContainerForPID = conns.ContainerForPid |> Seq.map (fun e -> e.Key, e.Value) |> Map.ofSeq
      EncodedTags = encodedTags
      EncodedConnectionsTags = conns.EncodedConnectionsTags.ToByteArray()
      HostTagsIndex = conns.HostTagsIndex
      HostTags = Tags.toMultiMap (sharedTags encodedTags conns.HostTagsIndex)
      ConnTelemetry = connTelemetry conns.ConnTelemetry
      ConnTelemetryMap = conns.ConnTelemetryMap |> Seq.map (fun e -> e.Key, e.Value) |> Map.ofSeq
      CompilationTelemetry = ProtoJson.messageMap conns.CompilationTelemetryByAsset
      KernelHeaderFetchResult = ProcessEnum.name conns.KernelHeaderFetchResult
      // The CO-RE result per asset, by name.
      CORETelemetry = conns.CORETelemetryByAsset |> Seq.map (fun e -> e.Key, ProcessEnum.name e.Value) |> Map.ofSeq
      PrebuiltEBPFAssets = Array.ofSeq conns.PrebuiltEBPFAssets
      Routes = ProtoJson.messages conns.Routes
      RouteMetadata = ProtoJson.messages conns.RouteMetadata
      AgentConfiguration = ProtoJson.message conns.AgentConfiguration
      EncodedDNS = conns.EncodedDNS.ToByteArray()
      Domains = Array.ofSeq conns.Domains
      EncodedDomainDatabase = conns.EncodedDomainDatabase.ToByteArray()
      EncodedDNSLookups = conns.EncodedDnsLookups.ToByteArray()
      DNSNames = dnsNames
      ResolvedHostsByName = ProtoJson.messageMap conns.ResolvedHostsByName
      ResolvedPublicIPs = ProtoJson.messageMap conns.ResolvedPublicIps
      EcsTask = conns.EcsTask
      ResolvConfs = Array.ofSeq conns.ResolvConfs
      AgentHostname = info.Agent.Hostname
      AgentVersion = info.Agent.Version
      AgentContainerCount = info.Agent.ContainerCount
      RequestID = info.Agent.RequestID }

/// One connection. Its own tags index the payload's encodedConnectionsTags,
/// where 0 is a real set and the agent writes -1 for "no tags"; its
/// container's and the remote service's index encodedTags. The indices are
/// kept beside the result.
let connectionRow (info: FrameInfo) (host: string) (encodedTags: byte[]) (connectionTags: byte[]) (c: Connection) : ConnectionRow =
    let local = orEmpty c.Laddr
    let remote = orEmpty c.Raddr
    let translation = c.IpTranslation
    let translated = not (isNull translation)

    { TenantID = info.Tenant
      Timestamp = info.At
      Host = host
      PayloadID = info.SnapshotID
      PID = c.Pid
      LaddrIP = local.Ip
      LaddrPort = local.Port
      LaddrContainerID = local.ContainerId
      LaddrHostName = local.HostName
      RaddrIP = remote.Ip
      RaddrPort = remote.Port
      RaddrContainerID = remote.ContainerId
      RaddrHostName = remote.HostName
      Family = ProcessEnum.name c.Family
      Type = ProcessEnum.name c.Type
      Direction = ProcessEnum.name c.Direction
      IsLocalPortEphemeral = ProcessEnum.name c.IsLocalPortEphemeral
      LastBytesSent = c.LastBytesSent
      LastBytesReceived = c.LastBytesReceived
      LastPacketsSent = c.LastPacketsSent
      LastPacketsReceived = c.LastPacketsReceived
      LastRetransmits = c.LastRetransmits
      ProtocolStack =
        (if isNull c.Protocol then
             [||]
         else
             c.Protocol.Stack |> Seq.map ProcessEnum.name |> Array.ofSeq)
      NetNS = c.NetNS
      RemoteNetworkID = c.RemoteNetworkId
      IPTranslationReplSrcIP = (if translated then Some translation.ReplSrcIP else None)
      IPTranslationReplDstIP = (if translated then Some translation.ReplDstIP else None)
      IPTranslationReplSrcPort = (if translated then Some translation.ReplSrcPort else None)
      IPTranslationReplDstPort = (if translated then Some translation.ReplDstPort else None)
      RTT = c.Rtt
      RTTVar = c.RttVar
      IntraHost = Text.flag c.IntraHost
      DNSSuccessfulResponses = c.DnsSuccessfulResponses
      DNSFailedResponses = c.DnsFailedResponses
      DNSTimeouts = c.DnsTimeouts
      DNSSuccessLatencySum = c.DnsSuccessLatencySum
      DNSFailureLatencySum = c.DnsFailureLatencySum
      DNSCountByRcode = c.DnsCountByRcode |> Seq.map (fun e -> e.Key, e.Value) |> Map.ofSeq
      DNSStatsByDomain = ProtoJson.messageMap c.DnsStatsByDomain
      DNSStatsByDomainByQueryType = ProtoJson.messageMap c.DnsStatsByDomainByQueryType
      DNSStatsByDomainOffsetByQueryType = ProtoJson.messageMap c.DnsStatsByDomainOffsetByQueryType
      LastTCPEstablished = c.LastTcpEstablished
      LastTCPClosed = c.LastTcpClosed
      RouteIdx = c.RouteIdx
      RouteTargetIdx = c.RouteTargetIdx
      ResolvConfIdx = c.ResolvConfIdx
      HTTPAggregations = c.HttpAggregations.ToByteArray()
      HTTP2Aggregations = c.Http2Aggregations.ToByteArray()
      DataStreamsAggregations = c.DataStreamsAggregations.ToByteArray()
      DatabaseAggregations = c.DatabaseAggregations.ToByteArray()
      TagsIndices = Array.ofSeq c.Tags
      TagsIdx = c.TagsIdx
      TagsChecksum = c.TagsChecksum
      Tags = Tags.toMultiMap (TagBuffer.tags connectionTags c.TagsIdx)
      LocalContainerTagsIndex = c.LocalContainerTagsIndex
      LocalContainerTags = Tags.toMultiMap (sharedTags encodedTags c.LocalContainerTagsIndex)
      RemoteServiceTagsIdx = c.RemoteServiceTagsIdx
      RemoteServiceTags = Tags.toMultiMap (sharedTags encodedTags c.RemoteServiceTagsIdx)
      StateIndex = c.StateIndex
      TCPFailuresByErrCode = c.TcpFailuresByErrCode |> Seq.map (fun e -> e.Key, e.Value) |> Map.ofSeq
      RemoteEcsTask = c.RemoteEcsTask
      SystemProbeConn = Text.flag c.SystemProbeConn
      LastTCPRtoCount = c.LastTcpRtoCount
      LastTCPRecoveryCount = c.LastTcpRecoveryCount
      LastTCPReordSeen = c.LastTcpReordSeen
      LastTCPRcvOooPack = c.LastTcpRcvOooPack
      LastTCPDeliveredCe = c.LastTcpDeliveredCe
      LastTCPProbe0Count = c.LastTcpProbe0Count
      TCPEcnNegotiated = Text.flag c.TcpEcnNegotiated
      AgentVersion = info.Agent.Version
      RequestID = info.Agent.RequestID }

/// The cadence, in seconds, the agent is told to keep for its process
/// checks. It matches the agent's own default.
let checkInterval = 10

/// What every route answers, whatever became of the frame.
///
/// Status is NOT optional in practice, whatever the schema says: the agent
/// feeds every reply into CheckRunner.UpdateRTStatus, which indexes the
/// statuses it gathered without checking that it got any, so a reply without
/// one makes the agent dereference nil and crash-loop its container. This
/// was observed against a real DaemonSet.
///
/// ActiveClients 0 means nobody is watching a live process view, which keeps
/// the agent in its normal cadence instead of real-time mode.
let replyFrame: byte[] =
    let header = ResCollector.Types.Header()
    header.Type <- 23

    let status = CollectorStatus()
    status.ActiveClients <- 0
    status.Interval <- checkInterval

    let body = ResCollector()
    body.Header <- header
    body.Status <- status
    ProcessFrame.encode 23uy 0L body

let private reply: Response = Response.bytes 200 "application/x-protobuf" replyFrame

/// The frame of a request. One that cannot be read is kept whole: it is
/// exactly the payload that would explain why, and it arrives once.
let private decode (r: Request) (label: string) : Frame option =
    match ProcessFrame.decode r.Body with
    | Ok frame -> Some frame
    | Error error ->
        r.Log.LogWarning("[{Label}] process-agent frame: {Error} ({Bytes} bytes)", label, error, r.Body.Length)
        Raw.store r "process" "decode_error" (label + ": " + error) r.Body
        None

/// The frame's type byte decides the body, not the path, so the two can
/// disagree: a misconfigured agent, or a type not met before.
let private unexpected (r: Request) (label: string) (frame: Frame) : unit =
    let goType = "*process." + frame.Body.Descriptor.Name
    r.Log.LogWarning("[{Label}] type={Type} {GoType} - not handled", label, frame.Header.Type, goType)
    Raw.store r "process" "unexpected_shape" $"/api/v1/{label} decoded to {goType} (frame type {frame.Header.Type})" r.Body

/// Without a tenant there is nowhere to put a row: tenant_id leads every
/// table's ORDER BY. The agent still gets its reply.
let private write (r: Request) (table: Table<'row>) (rows: 'row[]) : unit =
    if r.Tenant <> "" then
        Sink.write r.Sink table rows

/// The process check. One path, two message types: 12 (CollectorProc, the
/// full snapshot every 10 s) and 27 (CollectorRealTime, stats only, every
/// 2 s).
let handleCollector (r: Request) : Response =
    match decode r "collector" with
    | None -> ()
    | Some frame ->
        let info = frameInfo r

        match frame.Body with
        | :? CollectorProc as proc ->
            write r ProcessSnapshots.table [| snapshotRow info frame |]
            write r Processes.table (proc.Processes |> Seq.map (processRow info proc) |> Array.ofSeq)
            write r Containers.table (procContainerRows info proc)
        | :? CollectorRealTime as realTime ->
            write r ProcessSnapshots.table [| snapshotRow info frame |]
            write r ProcessStats.table (realTime.Stats |> Seq.map (processStatRow info realTime) |> Array.ofSeq)
            write r ContainerStats.table (realTimeContainerStatRows info realTime)
        | _ -> unexpected r "collector" frame

    reply

/// The container check. One path, two message types: 39 (CollectorContainer,
/// the full inventory every 10 s) and 40 (CollectorContainerRealTime, stats
/// only, every 2 s).
let handleContainer (r: Request) : Response =
    match decode r "container" with
    | None -> ()
    | Some frame ->
        let info = frameInfo r

        match frame.Body with
        | :? CollectorContainer as inventory ->
            write r ProcessSnapshots.table [| snapshotRow info frame |]
            write r Containers.table (inventoryContainerRows info inventory)
        | :? CollectorContainerRealTime as realTime ->
            write r ProcessSnapshots.table [| snapshotRow info frame |]
            write r ContainerStats.table (containerRealTimeStatRows info realTime)
        | _ -> unexpected r "container" frame

    reply

/// What system-probe saw inside each connection, per HTTP endpoint, Kafka
/// topic and database operation. A message that does not parse is logged and
/// gives no rows; its bytes are on the connection's own row.
let private storeUsm (r: Request) (info: FrameInfo) (conns: CollectorConnections) : unit =
    let http = ResizeArray<ConnectionHttpStatRow>()
    let kafka = ResizeArray<ConnectionKafkaStatRow>()
    let database = ResizeArray<ConnectionDatabaseStatRow>()

    let take (target: ResizeArray<'row>) (decoded: Result<'row list, string>) : unit =
        match decoded with
        | Ok rows -> target.AddRange rows
        | Error error -> r.Log.LogWarning("[connections] {Error}", error)

    for c in conns.Connections do
        let local = orEmpty c.Laddr
        let remote = orEmpty c.Raddr

        let key: ConnectionKey =
            { TenantID = info.Tenant
              Timestamp = info.At
              Host = conns.HostName
              PayloadID = info.SnapshotID
              PID = c.Pid
              LaddrIP = local.Ip
              LaddrPort = local.Port
              RaddrIP = remote.Ip
              RaddrPort = remote.Port }

        take http (Usm.http key c.HttpAggregations)
        take http (Usm.http2 key c.Http2Aggregations)
        take kafka (Usm.kafka key c.DataStreamsAggregations)
        take database (Usm.database key c.DatabaseAggregations)

    write r ConnectionHttpStats.table (http.ToArray())
    write r ConnectionKafkaStats.table (kafka.ToArray())
    write r ConnectionDatabaseStats.table (database.ToArray())

/// The network check (type 22): the system-probe's connection table, with
/// DNS, routes and eBPF telemetry. The payload row goes first, then one row
/// per connection.
let handleConnections (r: Request) : Response =
    match decode r "connections" with
    | None -> ()
    | Some frame ->
        match frame.Body with
        | :? CollectorConnections as conns ->
            let info = frameInfo r

            let dnsNames =
                match DnsBuffer.names (conns.EncodedDNS.ToByteArray()) (conns.EncodedDomainDatabase.ToByteArray()) with
                | Ok names -> names
                | Error error ->
                    // The buffers are stored either way.
                    r.Log.LogWarning("[connections] DNS names not read: {Error}", error)
                    [||]

            let encodedTags = conns.EncodedTags.ToByteArray()
            let connectionTags = conns.EncodedConnectionsTags.ToByteArray()

            write r ProcessSnapshots.table [| snapshotRow info frame |]
            write r ConnectionsPayloads.table [| connectionsPayloadRow info conns dnsNames |]

            write
                r
                Connections.table
                (conns.Connections |> Seq.map (connectionRow info conns.HostName encodedTags connectionTags) |> Array.ofSeq)

            storeUsm r info conns
        | _ -> unexpected r "connections" frame

    reply

/// The lightweight process discovery check (type 53): pid, command and user
/// only, no resource usage.
let handleDiscovery (r: Request) : Response =
    match decode r "discovery" with
    | None -> ()
    | Some frame ->
        match frame.Body with
        | :? CollectorProcDiscovery as discoveries ->
            let info = frameInfo r
            write r ProcessSnapshots.table [| snapshotRow info frame |]
            write r ProcessDiscoveries.table (discoveries.ProcessDiscoveries |> Seq.map (discoveryRow info discoveries) |> Array.ofSeq)
        | _ -> unexpected r "discovery" frame

    reply
