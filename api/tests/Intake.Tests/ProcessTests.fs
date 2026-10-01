/// Tests of Routers/Process.fs beyond the golden fixtures.
module NinjaCat.Api.Intake.Tests.ProcessTests

open System
open System.IO
open System.Text.Json.Nodes
open Google.Protobuf
open Microsoft.AspNetCore.Http
open Microsoft.Extensions.Logging.Abstractions
open Microsoft.Extensions.Primitives
open Xunit
open NinjaCat.Api.Intake
open NinjaCat.Api.Intake.Routers.Process
open NinjaCat.Api.Intake.Routers.ProcessBuffers
open NinjaCat.Api.Intake.Tests.Golden
open NinjaCat.Api.Storage.Rows
// Last, so that Process, Route and Connections are the protobuf messages here.
open Datadog.ProcessAgent

let private agentVersion = "7.58.2"
let private requestId = "b0a1f2c3-0000-4000-8000-000000000001"

let private info: FrameInfo =
    { Tenant = "test"
      SnapshotID = Guid.Empty
      At = DateTime(1970, 1, 1, 0, 0, 1, DateTimeKind.Utc)
      Path = "/api/v1/collector"
      Bytes = 0
      Agent =
        { Hostname = ""
          Version = agentVersion
          ContainerCount = ""
          RequestID = requestId } }

let private bytes (base64: string) : byte[] = Convert.FromBase64String base64

let private sameJson (expected: string) (actual: string) =
    Assert.True(JsonNode.DeepEquals(JsonNode.Parse expected, JsonNode.Parse actual), $"expected {expected}\nactual   {actual}")

/// One container with every field set, each to a different value: a field
/// that silently read its neighbour would still look plausible with zeros.
let private fullContainer () : Container =
    let container =
        Container(
            Type = "containerd",
            Id = "cid-1",
            Name = "app",
            Image = "repo/app:1",
            CpuLimit = 2.0f,
            MemoryLimit = (1UL <<< 30),
            State = ContainerState.Running,
            Health = ContainerHealth.Healthy,
            Created = 1_700_000_000L,
            Started = 1_700_000_060L,
            Rbps = 1.0f,
            Wbps = 2.0f,
            Key = 77u,
            NetRcvdPs = 3.0f,
            NetSentPs = 4.0f,
            NetRcvdBps = 5.0f,
            NetSentBps = 6.0f,
            UserPct = 7.0f,
            SystemPct = 8.0f,
            TotalPct = 15.0f,
            MemRss = 9UL,
            MemCache = 10UL,
            ByteKey = ByteString.CopyFrom [| 0xAAuy |],
            ThreadCount = 12UL,
            ThreadLimit = 100UL,
            MemUsage = 13UL,
            CpuUsageNs = 14.0f,
            MemAccounted = 15UL,
            CpuRequest = 0.5f,
            MemoryRequest = (1UL <<< 20),
            RepoDigest = "sha256:abc"
        )

    container.Tags.AddRange [ "env:prod"; "kube_service:a"; "kube_service:b" ]
    // The same ip and port on two protocols: a real shape.
    container.Addresses.Add(ContainerAddr(Ip = "10.0.0.1", Port = 8080, Protocol = ConnectionType.Tcp))
    container.Addresses.Add(ContainerAddr(Ip = "10.0.0.1", Port = 8080, Protocol = ConnectionType.Udp))
    container

let private fullProcess () : Process =
    let command =
        Command(Cwd = "/srv/app", Root = "/", OnDisk = true, Ppid = 1, Pgroup = 1200, Exe = "/usr/bin/app", Comm = "app")

    command.Args.AddRange [ "/usr/bin/app"; "--filter=a b"; "-v" ]

    let cpu =
        CPUStat(
            LastCpu = "cpu3",
            TotalPct = 12.5f,
            UserPct = 8.5f,
            SystemPct = 4.0f,
            NumThreads = 9,
            Nice = -5,
            UserTime = 111L,
            SystemTime = 222L
        )

    cpu.Cpus.Add(SingleCPUStat(Name = "cpu0", TotalPct = 1.0f))
    cpu.Cpus.Add(SingleCPUStat(Name = "cpu1", TotalPct = 2.0f))

    let ports = PortInfo()
    ports.Tcp.AddRange [ 8080; 8443 ]
    ports.Udp.Add 53

    let discovery =
        ServiceDiscovery(
            GeneratedServiceName = ServiceName(Name = "guessed", Source = ServiceNameSource.CommandLine),
            DdServiceName = ServiceName(Name = "declared", Source = ServiceNameSource.DdService),
            ApmInstrumentation = true
        )

    discovery.AdditionalGeneratedNames.Add(ServiceName(Name = "extra", Source = ServiceNameSource.Spring))
    discovery.TracerMetadata.Add(TracerMetadata(RuntimeId = "runtime-1", ServiceName = "traced"))
    discovery.Resources.Add(Resource(Logs = LogResource(Path = "/var/log/app.log")))

    let p =
        Process(
            Key = 4242u,
            Pid = 1234,
            NsPid = 7,
            Command = command,
            User = ProcessUser(Name = "svc", Uid = 1000, Gid = 1001, Euid = 0, Egid = 2, Suid = 3, Sgid = 4),
            Memory =
                MemoryStat(Rss = 100UL, Vms = 200UL, Swap = 300UL, Shared = 400UL, Text = 500UL, Lib = 600UL, Data = 700UL, Dirty = 800UL),
            Cpu = cpu,
            CreateTime = 1_700_000_000_000L,
            OpenFdCount = 64,
            State = ProcessState.R,
            IoStat = IOStat(ReadRate = 1.0f, WriteRate = 2.0f, ReadBytesRate = 3.0f, WriteBytesRate = 4.0f),
            ContainerId = "cid-1",
            ContainerKey = 99u,
            VoluntaryCtxSwitches = 11UL,
            InvoluntaryCtxSwitches = 22UL,
            ByteKey = ByteString.CopyFrom [| 1uy; 2uy |],
            ContainerByteKey = ByteString.CopyFrom [| 3uy |],
            Networks = ProcessNetworks(ConnectionRate = 5.0f, BytesRate = 6.0f),
            Language = Language.Go,
            PortInfo = ports,
            ServiceDiscovery = discovery,
            InjectionState = InjectionState.InjectionInjected,
            ZombieChildrenCount = 3u,
            ZombieNetRate = 1.5,
            HasZombieAggregation = true
        )

    p.ProcessContext.AddRange [ "ctx-a"; "ctx-b" ]
    // Two tags sharing a key: a pod behind two services sends exactly this.
    p.Tags.AddRange [ "env:prod"; "kube_service:a"; "kube_service:b" ]
    p

let private fullCollectorProc () : CollectorProc =
    let host =
        Host(Id = 5L, OrgId = 7, Name = "host-a", NumCpus = 8, TotalMemory = (16L <<< 30), TagIndex = 0, TagsModified = 1_700_000_000L)

    host.AllTags.AddRange [ "env:prod"; "kube_service:a"; "kube_service:b" ]

    let system =
        SystemInfo(
            Uuid = "uuid-1",
            Os = OSInfo(Name = "linux", Platform = "ubuntu", Family = "debian", Version = "22.04", KernelVersion = "5.15.0"),
            TotalMemory = (16L <<< 30)
        )

    for number in [ 0; 1 ] do
        system.Cpus.Add(
            CPUInfo(
                Number = number,
                Vendor = "GenuineIntel",
                Family = "6",
                Model = "85",
                PhysicalId = "0",
                CoreId = string number,
                Cores = 4,
                Mhz = 2500L,
                CacheSize = 33792
            )
        )

    let proc =
        CollectorProc(
            HostName = "host-a",
            NetworkId = "vpc-123",
            Host = host,
            Info = system,
            GroupId = 11,
            GroupSize = 2,
            ContainerHostType = ContainerHostType.FargateEks,
            HintMask = 3
        )

    proc.Processes.Add(fullProcess ())
    proc.Containers.Add(fullContainer ())
    proc

let private frameOf (messageType: uint8) (body: IMessage) : Frame =
    { Header =
        { Version = 3uy
          Encoding = 0uy
          Type = messageType
          SubscriptionID = 0uy
          OrgID = 7
          Timestamp = 4242L }
      Body = body }

/// Posts a frame to the process intake as the test agent would.
let private post (path: string) (body: byte[]) : Response * CapturingSink =
    let sink = CapturingSink()

    let deps: Deps =
        { Store = Replay.testStore ()
          Sink = sink
          Log = NullLogger.Instance
          AckUnknown = false }

    let http = DefaultHttpContext()
    http.Request.Method <- "POST"
    http.Request.Host <- HostString "example.com"
    http.Request.Path <- PathString path
    http.Request.Headers["Dd-Api-Key"] <- StringValues Replay.testKey
    http.Request.Headers["X-Dd-Processagentversion"] <- StringValues agentVersion
    Routes.byGoNames deps [ "routeProcess" ] http body, sink

[<Fact>]
let ``the reply carries a status, which the agent dereferences without checking`` () =
    match ProcessFrame.decode replyFrame with
    | Error e -> Assert.Fail $"the agent could not decode our own reply: {e}"
    | Ok frame ->
        let reply = frame.Body :?> ResCollector
        Assert.NotNull reply.Status
        Assert.Equal(checkInterval, reply.Status.Interval)
        // Nothing is watching a live view.
        Assert.Equal(0, reply.Status.ActiveClients)
        // Older agents look for the nested header.
        Assert.NotNull reply.Header

[<Fact>]
let ``every field of a process reaches its column`` () =
    let proc = fullCollectorProc ()
    let row = processRow info proc proc.Processes[0]

    Assert.Equal(7, row.NsPID)
    Assert.Equal(4242u, row.Key)
    Assert.Equal("/srv/app", row.Cwd)
    Assert.Equal("/", row.Root)
    Assert.Equal(1uy, row.OnDisk)
    Assert.Equal(1200, row.Pgroup)
    Assert.Equal<int list>([ 1000; 1001; 0; 2; 3; 4 ], [ row.UID; row.GID; row.EUID; row.EGID; row.SUID; row.SGID ])

    Assert.Equal<uint64 list>(
        [ 100UL; 200UL; 300UL; 400UL; 500UL; 600UL; 700UL; 800UL ],
        [ row.RSS; row.VMS; row.MemSwap; row.MemShared; row.MemText; row.MemLib; row.MemData; row.MemDirty ]
    )

    Assert.Equal("cpu3", row.CPULastCPU)
    Assert.Equal<float32 list>([ 12.5f; 8.5f; 4.0f ], [ row.CPUPct; row.CPUUserPct; row.CPUSystemPct ])
    Assert.Equal(9, row.Threads)
    Assert.Equal(-5, row.CPUNice)
    Assert.Equal(111L, row.CPUUserTime)
    Assert.Equal(222L, row.CPUSystemTime)
    Assert.Equal<float32 list>([ 1.0f; 2.0f; 3.0f; 4.0f ], [ row.IOReadRate; row.IOWriteRate; row.IOReadBytesRate; row.IOWriteBytesRate ])
    Assert.Equal(11UL, row.VoluntaryCtxSwitches)
    Assert.Equal(22UL, row.InvoluntaryCtxSwitches)
    Assert.Equal(5.0f, row.NetConnectionRate)
    Assert.Equal(6.0f, row.NetBytesRate)
    // Enum NAMES, never numbers.
    Assert.Equal("LANGUAGE_GO", row.Language)
    Assert.Equal("INJECTION_INJECTED", row.InjectionState)
    Assert.Equal("R", row.State)
    Assert.Equal("guessed", row.GeneratedServiceName)
    Assert.Equal("SERVICE_NAME_SOURCE_COMMAND_LINE", row.GeneratedServiceNameSource)
    Assert.Equal("declared", row.DDServiceName)
    Assert.Equal("SERVICE_NAME_SOURCE_DD_SERVICE", row.DDServiceNameSource)
    Assert.Equal(1uy, row.APMInstrumentation)
    Assert.Equal(3u, row.ZombieChildrenCount)
    Assert.Equal(1.5, row.ZombieNetRate)
    Assert.Equal(1uy, row.HasZombieAggregation)
    Assert.Equal(99u, row.ContainerKey)
    Assert.Equal<byte[]>([| 1uy; 2uy |], row.ByteKey)
    Assert.Equal<byte[]>([| 3uy |], row.ContainerByteKey)
    Assert.Equal("vpc-123", row.NetworkID)
    Assert.Equal(11, row.GroupID)
    Assert.Equal(2, row.GroupSize)
    Assert.Equal("fargateEKS", row.ContainerHostType)
    Assert.Equal(agentVersion, row.AgentVersion)
    Assert.Equal(requestId, row.RequestID)
    Assert.Equal("uuid-1", row.HostUUID)

    Assert.Equal<string list>(
        [ "linux"; "ubuntu"; "debian"; "22.04"; "5.15.0" ],
        [ row.OSName; row.OSPlatform; row.OSFamily; row.OSVersion; row.KernelVersion ]
    )

    // Parallel arrays, in the order of the wire.
    Assert.Equal<string[]>([| "cpu0"; "cpu1" |], row.CPUCoreNames)
    Assert.Equal<float32[]>([| 1.0f; 2.0f |], row.CPUCorePcts)
    Assert.Equal<int[]>([| 8080; 8443 |], row.PortTCP)
    Assert.Equal<int[]>([| 53 |], row.PortUDP)
    Assert.Equal<string[]>([| "ctx-a"; "ctx-b" |], row.ProcessContext)
    // The APM join: these tie a pid on a host to the traces it emitted.
    Assert.Equal<string[]>([| "runtime-1" |], row.TracerRuntimeIDs)
    Assert.Equal<string[]>([| "traced" |], row.TracerServiceNames)
    Assert.Equal<string[]>([| "extra" |], row.AdditionalGeneratedNames)
    Assert.Equal<string[]>([| "SERVICE_NAME_SOURCE_SPRING" |], row.AdditionalGeneratedNameSources)
    sameJson """[{"Resource":{"logs":{"path":"/var/log/app.log"}}}]""" row.ServiceResources

[<Fact>]
let ``tags sharing a key all survive`` () =
    let proc = fullCollectorProc ()
    let row = processRow info proc proc.Processes[0]
    Assert.Equal<Map<string, string[]>>(Map [ "env", [| "prod" |]; "kube_service", [| "a"; "b" |] ], row.Tags)

/// Joining argv on spaces cannot be undone: `--filter=a b` and two separate
/// arguments give the same string.
[<Fact>]
let ``argv is kept beside the joined command line`` () =
    let proc = fullCollectorProc ()
    let row = processRow info proc proc.Processes[0]
    Assert.Equal<string[]>([| "/usr/bin/app"; "--filter=a b"; "-v" |], row.Args)
    Assert.Equal("/usr/bin/app --filter=a b -v", row.Cmdline)

/// A snapshot from a host where /proc was partly unreadable.
[<Fact>]
let ``a process with no sub-messages gives a row of zeros`` () =
    let proc = CollectorProc(HostName = "host-a")
    proc.Processes.Add(Process(Pid = 5))
    let row = processRow info proc proc.Processes[0]

    Assert.Equal(5, row.PID)
    Assert.Equal("", row.Comm)
    Assert.Equal("", row.User)
    Assert.Equal(0UL, row.RSS)
    Assert.Equal(0.0f, row.CPUPct)
    Assert.Equal("", row.GeneratedServiceNameSource)
    Assert.Equal("", row.ServiceResources)
    // "" rather than "null" or "{}": the host was absent.
    Assert.Equal("", row.ProcessHost)

[<Theory>]
[<InlineData(0L, 0uy)>]
[<InlineData(-1L, 0uy)>]
[<InlineData(1_700_000_000_000L, 1uy)>]
let ``an absent start time is flagged, not stored as a date`` (wire: int64) (has: uint8) =
    let proc = CollectorProc()
    proc.Processes.Add(Process(Pid = 1, CreateTime = wire))
    let row = processRow info proc proc.Processes[0]

    Assert.Equal(has, row.HasCreateTime)
    let expected = if has = 1uy then DateTime.UnixEpoch.AddMilliseconds(float wire) else DateTime.MinValue
    Assert.Equal(expected, row.CreateTime)

[<Fact>]
let ``a start time past what the column holds is pinned to its end`` () =
    let proc = CollectorProc()
    proc.Processes.Add(Process(Pid = 1, CreateTime = Int64.MaxValue))
    let row = processRow info proc proc.Processes[0]
    Assert.Equal(1uy, row.HasCreateTime)
    Assert.Equal(DateTime(2106, 2, 7, 6, 28, 15, DateTimeKind.Utc), row.CreateTime)

[<Fact>]
let ``an enum value newer than the definitions is stored as its number`` () =
    let proc = CollectorProc()
    proc.Processes.Add(Process(Pid = 1, State = enum<ProcessState> 42))
    Assert.Equal("42", (processRow info proc proc.Processes[0]).State)

let private snapshotOf (messageType: uint8) (body: IMessage) : ProcessSnapshotRow =
    let row = snapshotRow info (frameOf messageType body)
    Assert.Equal("test", row.TenantID)
    Assert.Equal("host-a", row.HostName)
    // The header's timestamp is stored as the number it is.
    Assert.Equal(4242L, row.HeaderTimestamp)
    Assert.Equal(7, row.HeaderOrgID)
    row

[<Fact>]
let ``the envelope of a CollectorProc carries its host, system and hints`` () =
    let row = snapshotOf 12uy (fullCollectorProc ())
    Assert.Equal(12uy, row.MessageType)
    Assert.Equal("CollectorProc", row.MessageTypeName)
    Assert.Equal("vpc-123", row.NetworkID)
    Assert.Equal(5L, row.HostID)
    Assert.Equal(8, row.HostNumCPUs)
    Assert.Equal<string[]>([| "a"; "b" |], row.HostAllTags["kube_service"])
    Assert.Equal(Some(DateTime.UnixEpoch.AddSeconds 1_700_000_000.0), row.HostTagsModified)
    Assert.Equal<int[]>([| 0; 1 |], row.CPUNumbers)
    Assert.Equal<int64[]>([| 2500L; 2500L |], row.CPUMhz)
    Assert.Equal("5.15.0", row.KernelVersion)
    Assert.Equal("uuid-1", row.SysUUID)
    Assert.Equal(Some 3, row.HintMask)
    Assert.Equal(1u, row.ProcessCount)
    Assert.Equal(1u, row.ContainerCount)

[<Fact>]
let ``a CollectorProc without hints has no hint mask, one with mask 0 has 0`` () =
    Assert.Equal(None, (snapshotOf 12uy (CollectorProc(HostName = "host-a"))).HintMask)
    Assert.Equal(Some 0, (snapshotOf 12uy (CollectorProc(HostName = "host-a", HintMask = 0))).HintMask)

[<Fact>]
let ``the envelope of a CollectorRealTime carries the inlined host numbers and no host`` () =
    let body = CollectorRealTime(HostName = "host-a", HostId = 5L, OrgId = 7, NumCpus = 8, TotalMemory = 99L)
    body.Stats.Add(ProcessStat(Pid = 1))
    body.Stats.Add(ProcessStat(Pid = 2))
    body.ContainerStats.Add(ContainerStat(Id = "c"))

    let row = snapshotOf 27uy body
    Assert.Equal("CollectorRealTime", row.MessageTypeName)
    Assert.Equal(5L, row.RTHostID)
    Assert.Equal(7, row.RTOrgID)
    Assert.Equal(8, row.RTNumCPUs)
    Assert.Equal(99L, row.RTTotalMemory)
    Assert.Equal(2u, row.ProcessStatCount)
    Assert.Equal(1u, row.ContainerStatCount)
    // A realtime frame has no Host message; nothing may be invented for it.
    Assert.Equal(None, row.HintMask)
    Assert.Equal(0L, row.HostID)

[<Fact>]
let ``the envelope of a CollectorContainer`` () =
    let body = CollectorContainer(HostName = "host-a", NetworkId = "vpc-9", ContainerHostType = ContainerHostType.Sidecar)
    body.Containers.Add(fullContainer ())

    let row = snapshotOf 39uy body
    Assert.Equal("CollectorContainer", row.MessageTypeName)
    Assert.Equal("vpc-9", row.NetworkID)
    Assert.Equal(1u, row.ContainerCount)
    Assert.Equal("sidecar", row.ContainerHostType)

[<Fact>]
let ``the envelope of a CollectorContainerRealTime`` () =
    let body = CollectorContainerRealTime(HostName = "host-a", HostId = 6L, NumCpus = 4, TotalMemory = 8L)
    body.Stats.Add(ContainerStat(Id = "c"))

    let row = snapshotOf 40uy body
    Assert.Equal("CollectorContainerRealTime", row.MessageTypeName)
    Assert.Equal(6L, row.RTHostID)
    Assert.Equal(1u, row.ContainerStatCount)

[<Fact>]
let ``the envelope of a CollectorProcDiscovery`` () =
    let body = CollectorProcDiscovery(HostName = "host-a", GroupId = 3, GroupSize = 4, Host = Host(Id = 5L, Name = "host-a"))
    body.ProcessDiscoveries.Add(ProcessDiscovery(Pid = 1))

    let row = snapshotOf 53uy body
    Assert.Equal("CollectorProcDiscovery", row.MessageTypeName)
    Assert.Equal(1u, row.DiscoveryCount)
    Assert.Equal(3, row.GroupID)
    Assert.Equal(4, row.GroupSize)
    Assert.Equal(5L, row.HostID)

[<Fact>]
let ``the envelope of a CollectorConnections`` () =
    let body = CollectorConnections(HostName = "host-a", NetworkId = "vpc-1")

    for pid in [ 1; 2; 3 ] do
        body.Connections.Add(Connection(Pid = pid))

    let row = snapshotOf 22uy body
    Assert.Equal("CollectorConnections", row.MessageTypeName)
    Assert.Equal(3u, row.ConnectionCount)

[<Fact>]
let ``a frame type without a name of its own is named by its number`` () =
    let row = snapshotRow info (frameOf 41uy (CollectorPod(HostName = "host-a")))
    Assert.Equal("type_41", row.MessageTypeName)
    Assert.Equal("", row.HostName)

let private inventory: ContainerEnvelope =
    { Source = "collector_proc"
      PID = 0
      NetworkID = ""
      GroupID = 0
      GroupSize = 0
      ContainerHostType = "" }

[<Fact>]
let ``container times the runtime never reported stay empty`` () =
    let full = containerRow info inventory "host-a" (fullContainer ())
    Assert.Equal(Some(DateTime.UnixEpoch.AddSeconds 1_700_000_000.0), full.Created)
    Assert.Equal(Some(DateTime.UnixEpoch.AddSeconds 1_700_000_060.0), full.Started)
    // The same ip and port on two protocols: a map keyed by ip would collapse it.
    Assert.Equal<string[]>([| "tcp"; "udp" |], full.AddrProtocols)
    Assert.Equal("running", full.State)
    Assert.Equal("healthy", full.Health)
    Assert.Equal<string[]>([| "a"; "b" |], full.Tags["kube_service"])

    let bare = containerRow info inventory "host-a" (Container(Id = "c"))
    Assert.Equal(None, bare.Created)
    Assert.Equal(None, bare.Started)

[<Fact>]
let ``a container on a process is stored beside the inventory, told apart by source`` () =
    let proc = fullCollectorProc ()
    proc.Processes[0].Container <- fullContainer ()
    let rows = procContainerRows info proc

    Assert.Equal<(string * int) list>([ "collector_proc", 0; "process", 1234 ], [ for row in rows -> row.Source, row.PID ])

[<Fact>]
let ``telemetry an agent did not send stays empty`` () =
    Assert.Empty(connTelemetry null)

    let sent = connTelemetry (CollectorConnectionsTelemetry(KprobesTriggered = 9L, DnsStatsDropped = 4L))
    Assert.Equal(9L, sent["kprobes_triggered"])
    Assert.Equal(4L, sent["dns_stats_dropped"])
    // Every field of the message.
    Assert.Equal(11, sent.Count)

/// route_idx is a POSITION in the routes list; dropping an empty route would
/// shift every index after it onto the wrong one.
[<Fact>]
let ``a route without subnet or interface keeps its position`` () =
    let routes =
        [ Route(Subnet = Subnet(Alias = "subnet-a"))
          Route()
          Route(Interface = Interface(HardwareAddr = "02:42:ac:11:00:02")) ]

    sameJson
        """[{"subnet_alias":"subnet-a","hardware_addr":""},
            {"subnet_alias":"","hardware_addr":""},
            {"subnet_alias":"","hardware_addr":"02:42:ac:11:00:02"}]"""
        (routesJson routes)

    Assert.Equal("", routesJson [])

// Buffers made by the encoders of agent-payload v5.0.207, each holding three
// tag sets: [env:prod kube_service:a kube_service:b], [env:prod role:db] and
// [zeta:1 alpha:2 alpha:2].
let private tagsV1 = bytes "AQMACABlbnY6cHJvZA4Aa3ViZV9zZXJ2aWNlOmEOAGt1YmVfc2VydmljZTpiAgAIAGVudjpwcm9kBwByb2xlOmRiAwAGAHpldGE6MQcAYWxwaGE6MgcAYWxwaGE6MgAA"
let private tagsV2 = bytes "AkkAAAAIAGVudjpwcm9kDgBrdWJlX3NlcnZpY2U6YQ4Aa3ViZV9zZXJ2aWNlOmIHAHJvbGU6ZGIGAHpldGE6MQcAYWxwaGE6MgMABQAAAA8AAAAfAAAAAgAFAAAALwAAAAMAOAAAAEAAAABAAAAA"
let private tagsV3 = bytes "A0kAAAAIAGVudjpwcm9kDgBrdWJlX3NlcnZpY2U6YQ4Aa3ViZV9zZXJ2aWNlOmIHAHJvbGU6ZGIHAGFscGhhOjIGAHpldGE6MQMABQAAAA8AAAAfAAAAAgAFAAAALwAAAAIAOAAAAEEAAAA="

[<Fact>]
let ``an untranslated connection has no NAT entry, and its tags come out of the buffer`` () =
    let protocol = ProtocolStack()
    protocol.Stack.AddRange [ ProtocolType.ProtocolTls; ProtocolType.ProtocolHttp2 ]

    let plain =
        Connection(
            Pid = 1,
            Laddr = Addr(Ip = "10.0.0.1", Port = 5000),
            Raddr = Addr(Ip = "10.0.0.2", Port = 443),
            Type = ConnectionType.Tcp,
            Family = ConnectionFamily.V4,
            Direction = ConnectionDirection.Outgoing,
            Protocol = protocol,
            TagsIdx = 0,
            LocalContainerTagsIndex = 14,
            RemoteServiceTagsIdx = -1
        )

    let row = connectionRow info "host-a" tagsV2 plain
    Assert.Equal(None, row.IPTranslationReplSrcIP)
    Assert.Equal(None, row.IPTranslationReplSrcPort)
    Assert.Equal<string list>([ "tcp"; "v4"; "outgoing" ], [ row.Type; row.Family; row.Direction ])
    // The stack is ordered: TLS over HTTP/2 is not HTTP/2 over TLS.
    Assert.Equal<string[]>([| "protocolTLS"; "protocolHTTP2" |], row.ProtocolStack)
    Assert.Equal<string[]>([| "a"; "b" |], row.Tags["kube_service"])
    Assert.Equal<Map<string, string[]>>(Map [ "env", [| "prod" |]; "role", [| "db" |] ], row.LocalContainerTags)
    Assert.Empty(row.RemoteServiceTags)

    let translated =
        Connection(Pid = 2, IpTranslation = IPTranslation(ReplSrcIP = "172.17.0.2", ReplDstIP = "10.0.0.9", ReplSrcPort = 1, ReplDstPort = 2))

    let nat = connectionRow info "host-a" tagsV2 translated
    Assert.Equal(Some "172.17.0.2", nat.IPTranslationReplSrcIP)
    Assert.Equal(Some "10.0.0.9", nat.IPTranslationReplDstIP)
    Assert.Equal(Some 1, nat.IPTranslationReplSrcPort)
    Assert.Equal(Some 2, nat.IPTranslationReplDstPort)
    // A connection with no addresses must not invent one.
    Assert.Equal("", nat.LaddrIP)
    Assert.Equal(0, nat.RaddrPort)

let private firstSet = [ "env:prod"; "kube_service:a"; "kube_service:b" ]
let private secondSet = [ "env:prod"; "role:db" ]

[<Fact>]
let ``tag sets come out of a version 1 buffer`` () =
    Assert.Equal<string list>(firstSet, TagBuffer.tags tagsV1 1)
    Assert.Equal<string list>(secondSet, TagBuffer.tags tagsV1 45)
    Assert.Equal<string list>([ "zeta:1"; "alpha:2"; "alpha:2" ], TagBuffer.tags tagsV1 66)
    // What the encoder returned for an empty set.
    Assert.Empty(TagBuffer.tags tagsV1 94)

[<Fact>]
let ``tag sets come out of a version 2 buffer`` () =
    Assert.Equal<string list>(firstSet, TagBuffer.tags tagsV2 0)
    Assert.Equal<string list>(secondSet, TagBuffer.tags tagsV2 14)
    Assert.Equal<string list>([ "zeta:1"; "alpha:2"; "alpha:2" ], TagBuffer.tags tagsV2 24)

/// Version 3 is version 2's layout with sorted, deduplicated sets.
[<Fact>]
let ``tag sets come out of a version 3 buffer`` () =
    Assert.Equal<string list>(firstSet, TagBuffer.tags tagsV3 0)
    Assert.Equal<string list>(secondSet, TagBuffer.tags tagsV3 14)
    Assert.Equal<string list>([ "alpha:2"; "zeta:1" ], TagBuffer.tags tagsV3 24)

[<Fact>]
let ``no tags for a negative index, one past the end, an empty buffer or an unknown version`` () =
    for buffer in [ tagsV1; tagsV2; tagsV3 ] do
        // -1 is what the encoders return for "no tags".
        Assert.Empty(TagBuffer.tags buffer -1)
        Assert.Empty(TagBuffer.tags buffer 9999)

    Assert.Empty(TagBuffer.tags [||] 0)
    Assert.Empty(TagBuffer.tags (Array.append [| 9uy |] tagsV2[1..]) 0)

[<Fact>]
let ``a tag buffer cut short gives the tags before the break, never an exception`` () =
    for buffer in [ tagsV1; tagsV2; tagsV3 ] do
        for length in 0 .. buffer.Length - 1 do
            for index in [ 0; 1; 14; 24; 45; 66 ] do
                Assert.Null(Record.Exception(fun () -> TagBuffer.tags buffer[.. length - 1] index |> ignore))

    // The second set of the version 2 buffer, cut inside its second tag's position.
    Assert.Equal<string list>([ "env:prod" ], TagBuffer.tags tagsV2[..94] 14)
    // The first set of the version 1 buffer, cut inside its third tag.
    Assert.Equal<string list>([ "env:prod"; "kube_service:a" ], TagBuffer.tags tagsV1[..35] 1)

// Made by V1DNSEncoder.Encode and V2DNSEncoder.EncodeDomainDatabase of
// agent-payload v5.0.207, from the same three names.
let private dnsV1 = bytes "AQIAAjMBAAEAAwgxMC4wLjAuMQIADAgxMC4wLjAuMgEACDEwLjAuMC4zARgLZXhhbXBsZS5jb20LZXhhbXBsZS5vcmcaaW50ZXJuYWwuc3ZjLmNsdXN0ZXIubG9jYWw="
let private domainDatabase = bytes "AwALZXhhbXBsZS5jb20LZXhhbXBsZS5vcmcaaW50ZXJuYWwuc3ZjLmNsdXN0ZXIubG9jYWw="
let private dnsNames = [| "example.com"; "example.org"; "internal.svc.cluster.local" |]
let private noNames: Result<string[], string> = Ok [||]

[<Fact>]
let ``DNS names come out of a version 1 buffer`` () =
    Assert.Equal(Ok dnsNames, DnsBuffer.names dnsV1 [||])

[<Fact>]
let ``DNS names come out of a version 2 domain database`` () =
    Assert.Equal(Ok dnsNames, DnsBuffer.names [||] domainDatabase)

/// As the agent's own reader: it looks at encodedDNS first and does not fall
/// back to the domain database.
[<Fact>]
let ``encodedDNS in an unknown version gives no names, even beside a domain database`` () =
    Assert.Equal(noNames, DnsBuffer.names [| 9uy; 0uy; 0uy |] domainDatabase)

[<Fact>]
let ``a payload with neither DNS buffer has no names`` () =
    Assert.Equal(noNames, DnsBuffer.names [||] [||])

[<Fact>]
let ``a DNS buffer cut short is an error or fewer names, never an exception`` () =
    for length in 1 .. dnsV1.Length - 1 do
        Assert.Null(Record.Exception(fun () -> DnsBuffer.names dnsV1[.. length - 1] [||] |> ignore))

    for length in 1 .. domainDatabase.Length - 1 do
        Assert.Null(Record.Exception(fun () -> DnsBuffer.names [||] domainDatabase[.. length - 1] |> ignore))

    // The domain database cut inside its second name.
    Assert.True(Result.isError (DnsBuffer.names [||] domainDatabase[..19]))

    // A name longer than what is left of the buffer.
    Assert.True(Result.isError (DnsBuffer.names [||] [| 1uy; 0uy; 50uy; 97uy |]))
    // A name block longer than the buffer.
    Assert.True(Result.isError (DnsBuffer.names [| 1uy; 0uy; 0uy; 0uy; 100uy; 0uy |] [||]))

/// tenant_id leads every table's ORDER BY, so a made-up value would write
/// rows no query looks at.
[<Fact>]
let ``without a tenant nothing is stored, and the agent still gets its reply`` () =
    let sink = CapturingSink()
    let http = DefaultHttpContext()
    http.Request.Method <- "POST"
    http.Request.Path <- PathString "/api/v1/collector"

    let request: Request =
        { Http = http
          Body = ProcessFrame.encode 12uy 0L (fullCollectorProc ())
          Params = Map.empty
          Key = None
          Sink = sink
          Log = NullLogger.Instance }

    let response = handleCollector request
    Assert.Equal(200, response.Status)
    Assert.Equal<byte[]>(replyFrame, response.Body)
    Assert.Empty(sink.Writes)

/// Zstandard 0.x is the one encoding the frame codec does not read.
[<Fact>]
let ``a zstd 0.x frame is kept raw, and the agent still gets its reply`` () =
    let frame = File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "Fixtures", "frames", "frame_enc2.bin"))
    let response, sink = post "/api/v1/collector" frame

    Assert.Equal(200, response.Status)
    Assert.Equal<byte[]>(replyFrame, response.Body)
    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("process", raw.Intake)
    Assert.Equal("decode_error", raw.Reason)
    Assert.Equal("collector: zstd 0.x frames (message encoding 2) are not supported", raw.Note)
    Assert.Equal<byte[]>(frame, raw.Body)
    Assert.Single(sink.Writes) |> ignore

[<Fact>]
let ``a frame whose body is not its type's protobuf is kept raw`` () =
    // Type 12 with a body that ends inside a field.
    let frame = Array.append (ProcessFrame.encode 12uy 0L (CollectorProc())) [| 0x12uy; 0x7Fuy; 0x41uy |]
    let _, sink = post "/api/v1/collector" frame

    let raw = Assert.Single(sink.Rows<RawPayloadRow>())
    Assert.Equal("decode_error", raw.Reason)
    Assert.StartsWith("collector: ", raw.Note)

[<Fact>]
let ``every route keeps a frame of a type it has no table for`` () =
    let cases =
        [ "/api/v1/container", 53uy, (CollectorProcDiscovery(HostName = "host-a") :> IMessage), "*process.CollectorProcDiscovery"
          "/api/v1/connections", 12uy, (CollectorProc(HostName = "host-a") :> IMessage), "*process.CollectorProc"
          "/api/v1/discovery", 22uy, (CollectorConnections(HostName = "host-a") :> IMessage), "*process.CollectorConnections"
          // A Kubernetes resource: the same frame, meant for another intake.
          "/api/v1/collector", 41uy, (CollectorPod(HostName = "host-a") :> IMessage), "*process.CollectorPod" ]

    for path, messageType, body, goType in cases do
        let response, sink = post path (ProcessFrame.encode messageType 0L body)

        Assert.Equal(200, response.Status)
        let raw = Assert.Single(sink.Rows<RawPayloadRow>())
        Assert.Equal("unexpected_shape", raw.Reason)
        Assert.Equal($"{path} decoded to {goType} (frame type {messageType})", raw.Note)
        Assert.Single(sink.Writes) |> ignore

// Frames encoded by agent-payload v5.0.207, and beside them what Go's
// encoding/json made of the sub-messages after decoding the same frames.
let private connectionsFrame = bytes "AwAWAAAAAAAAAAAAAAAAABIGaG9zdC1hGoYBCAGSAiUIAhIhCAEQ////////////ARgDIgQIChACIgQIAxABIgQIABAFkgINCP///////////wESANICMwgEEi8KBggcEgIIAgolCAESIQgBEP///////////wEYAyIECAoQAiIECAMQASIECAAQBdICBAgFEgDaAgwIERIICgYIARICGAlCCQoFY2lkLTISAEIeCgVjaWQtMRIVCgVjaWQtMRoD+/8BIgN0OjEoBDAJcl8BAgACMwEAAQADCDEwLjAuMC4xAgAMCDEwLjAuMC4yAQAIMTAuMC4wLjMBGAtleGFtcGxlLmNvbQtleGFtcGxlLm9yZxppbnRlcm5hbC5zdmMuY2x1c3Rlci5sb2NhbKoBDQoJY29ubnRyYWNrEgCqARMKBnRyYWNlchIJCAEQARjSCSAF+gEMCgoKCHN1Ym5ldC1h+gEA+gEVEhMKETAyOjQyOmFjOjExOjAwOjAykgITCghzdWJuZXQtYRABGAIiA3g6eZICAJoCBggBKAE4AcICCgoGaG9zdC1iEgDCAjYKBmhvc3QtYRIsCAUQBxoGaG9zdC1hMghlbnY6cHJvZDIDYTxiOAhAgICAgIAgSANQgOLPqgbyAikKBzEuMi4zLjQSHgoHMS4yLjMuNBIDYXdzGgl1cy1lYXN0LTEiA2E6Yg=="
let private procFrame = bytes "AwAMAAAAAAAAAAAAAAAAABIGaG9zdC1hGk8QARosCAUQBxoGaG9zdC1hMghlbnY6cHJvZDIDYTxiOAhAgICAgIAgSANQgOLPqgbSARwyFAoSChAvdmFyL2xvZy9hcHAubG9nMgIKADIAUgYSAWO6AQA="

[<Fact>]
let ``the whole sub-messages of a connections payload are the JSON Go wrote`` () =
    let _, sink = post "/api/v1/connections" connectionsFrame
    let payload = Assert.Single(sink.Rows<ConnectionsPayloadRow>())

    sameJson
        """{"cid-1":{"id":"cid-1","byteKey":"+/8B","tags":["t:1"],"tagIndex":4,"tagsModified":9},"cid-2":{}}"""
        payload.ResolvedResources

    sameJson """[{"alias":"subnet-a","tagIndex":1,"tagsModified":2,"tags":["x:y"]},{}]""" payload.RouteMetadata
    sameJson """{"npmEnabled":true,"csmEnabled":true,"discoveryServiceMapEnabled":true}""" payload.AgentConfiguration

    sameJson
        """{"host-a":{"id":5,"orgId":7,"name":"host-a","allTags":["env:prod","a<b"],"numCpus":8,"totalMemory":1099511627776,"tagIndex":3,"tagsModified":1700000000},"host-b":{}}"""
        payload.ResolvedHostsByName

    sameJson """{"1.2.3.4":{"ip":"1.2.3.4","cloudProvider":"aws","region":"us-east-1","tags":["a:b"]}}""" payload.ResolvedPublicIPs

    // The handler's own shapes: enums spelled out, a slot per route.
    sameJson
        """{"conntrack":{"runtime_compilation_enabled":false,"runtime_compilation_result":"NotAttempted","runtime_compilation_duration":0,"kernel_header_fetch_result":"FetchNotAttempted"},
            "tracer":{"runtime_compilation_enabled":true,"runtime_compilation_result":"CompilationSuccess","runtime_compilation_duration":1234,"kernel_header_fetch_result":"DownloadSuccess"}}"""
        payload.CompilationTelemetry

    sameJson
        """[{"subnet_alias":"subnet-a","hardware_addr":""},{"subnet_alias":"","hardware_addr":""},{"subnet_alias":"","hardware_addr":"02:42:ac:11:00:02"}]"""
        payload.Routes

    Assert.Equal<string[]>(dnsNames, payload.DNSNames)
    Assert.Equal<byte[]>(dnsV1, payload.EncodedDNS)

    let connection = Assert.Single(sink.Rows<ConnectionRow>())
    Assert.Equal(payload.PayloadID, connection.PayloadID)

    // 18446744073709551615 is the largest uint64: it must not pass through a float.
    sameJson
        """{"-1":{},"2":{"dnsTimeouts":1,"dnsSuccessLatencySum":18446744073709551615,"dnsFailureLatencySum":3,"dnsCountByRcode":{"0":5,"10":2,"3":1}}}"""
        connection.DNSStatsByDomain

    Assert.Contains("18446744073709551615", connection.DNSStatsByDomain)

    sameJson
        """{"4":{"dnsStatsByQueryType":{"1":{"dnsTimeouts":1,"dnsSuccessLatencySum":18446744073709551615,"dnsFailureLatencySum":3,"dnsCountByRcode":{"0":5,"10":2,"3":1}},"28":{"dnsTimeouts":2}}},"5":{}}"""
        connection.DNSStatsByDomainByQueryType

    sameJson """{"17":{"dnsStatsByQueryType":{"1":{"dnsFailureLatencySum":9}}}}""" connection.DNSStatsByDomainOffsetByQueryType

[<Fact>]
let ``a connections payload with nothing in it stores empty strings, not empty JSON`` () =
    let _, sink = post "/api/v1/connections" (ProcessFrame.encode 22uy 0L (CollectorConnections(HostName = "host-a")))
    let payload = Assert.Single(sink.Rows<ConnectionsPayloadRow>())

    Assert.Equal<string list>(
        [ ""; ""; ""; ""; ""; ""; "" ],
        [ payload.ResolvedResources
          payload.RouteMetadata
          payload.AgentConfiguration
          payload.ResolvedHostsByName
          payload.ResolvedPublicIPs
          payload.CompilationTelemetry
          payload.Routes ]
    )

    Assert.Empty(payload.ConnTelemetry)
    Assert.Empty(payload.HostTags)
    Assert.Empty(payload.DNSNames)
    // The payload row is written even when there is no connection to go with it.
    Assert.Empty(sink.Rows<ConnectionRow>())
    Assert.Equal(2, sink.Writes.Length)

[<Fact>]
let ``the whole sub-messages of a process are the JSON Go wrote`` () =
    let _, sink = post "/api/v1/collector" procFrame
    let row = Assert.Single(sink.Rows<ProcessRow>())

    sameJson
        """{"id":5,"orgId":7,"name":"host-a","allTags":["env:prod","a<b"],"numCpus":8,"totalMemory":1099511627776,"tagIndex":3,"tagsModified":1700000000}"""
        row.ProcessHost

    // A log resource with a path, one without, and a resource of a kind this
    // build does not know.
    sameJson """[{"Resource":{"logs":{"path":"/var/log/app.log"}}},{"Resource":{"logs":{}}},{"Resource":null}]""" row.ServiceResources

    // A host that was sent but empty is "{}", not "".
    let container = Assert.Single(sink.Rows<ContainerRow>())
    Assert.Equal("{}", container.HostInfo)

[<Fact>]
let ``a DNS buffer that cannot be read through still stores the payload`` () =
    let conns = CollectorConnections(HostName = "host-a", EncodedDNS = ByteString.CopyFrom [| 1uy; 0uy; 0uy; 0uy; 100uy; 0uy |])
    conns.Connections.Add(Connection(Pid = 1))
    let _, sink = post "/api/v1/connections" (ProcessFrame.encode 22uy 0L conns)

    let payload = Assert.Single(sink.Rows<ConnectionsPayloadRow>())
    Assert.Empty(payload.DNSNames)
    Assert.Equal(6, payload.EncodedDNS.Length)
    Assert.Single(sink.Rows<ConnectionRow>()) |> ignore
