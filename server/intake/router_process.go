package intake

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/DataDog/agent-payload/v5/process"
	"github.com/gin-gonic/gin"
	"github.com/google/uuid"
	"github.com/itsninjacats/server/apps/storage"
)

// process.<site> — the process-agent.
//
//	config: process_config.process_dd_url / DD_PROCESS_CONFIG_PROCESS_DD_URL
//
// The only intake with its own 16-byte frame instead of plain protobuf. All
// four paths share the frame and the ResCollector reply; the message type in
// byte 2 of the frame says what the body is, not the path.
//
// WHAT IS STORED, and where:
//
//	every frame, any type   -> process_snapshots  (the envelope: host, network,
//	                           group chunking, Host, SystemInfo, frame header,
//	                           agent headers, item counts)
//	CollectorProc           -> processes          (one row per Process)
//	                           containers         (source=collector_proc for the
//	                           snapshot inventory, source=process for the
//	                           Container hanging off an individual Process)
//	CollectorRealTime       -> process_stats, container_stats
//	CollectorContainer      -> containers         (source=collector_container)
//	CollectorContainerRealTime -> container_stats
//	CollectorProcDiscovery  -> process_discoveries
//	CollectorConnections    -> connections_payloads + connections
//
// Every item row carries the frame's snapshot_id back to process_snapshots;
// the connections pair uses the same uuid under the name payload_id. Columns
// and the reasoning behind them: schema/migrations/0008_process.sql.
//
// A frame that does not decode goes to raw_payloads (storeRaw, reason
// decode_error); a frame that decodes into a type no route expects goes there
// as unexpected_shape. Nothing is logged-and-dropped any more, though the log
// lines stay — they are how this protocol was read in the first place.
//
// Not handled: kubeops-intake's /api/v2/orch and /api/v2/orchmanif — the same
// frame carrying Kubernetes resources, redirected by its own
// orchestrator_explorer.orchestrator_dd_url.

func (a *Server) routeProcess(g *gin.RouterGroup) {
	g.POST("/api/v1/collector", a.HandleCollector)
	g.POST("/api/v1/container", a.HandleContainer)
	g.POST("/api/v1/connections", a.HandleConnections)
	g.POST("/api/v1/discovery", a.HandleProcDiscovery)
}

// Frame constants, read out of agent-payload v5.0.207 (process/message.go).
const (
	messageV3               = 3
	messageEncodingProtobuf = 0
	typeResCollector        = 23
	messageHeaderSize       = 16
)

// Sources recorded on the shared container tables, so a row says which message
// it came off. The same Container struct arrives three ways and the same
// ContainerStat two, and what is populated differs.
const (
	containerSourceProc         = "collector_proc"
	containerSourceContainer    = "collector_container"
	containerSourceProcess      = "process"
	containerStatSourceRealTime = "collector_realtime"
	containerStatSourceCtrRT    = "collector_container_realtime"
)

// HandleCollector accepts the process check. One path, two message types:
// 12 (CollectorProc, full snapshot every 10 s) and 27 (CollectorRealTime,
// stats only every 2 s). DecodeMessage already picked the body struct from
// the frame's type byte, so the body type is the variant. Replies with a
// ResCollector; it neither accepts nor returns JSON.
//
// The frame carries 37 message types; the other ones are Kubernetes resources
// and arrive on a different intake.
func (a *Server) HandleCollector(c *gin.Context) {
	defer c.Data(http.StatusOK, "application/x-protobuf", resCollectorResponse())

	msg, body, ok := a.decodeProcessFrame(c, "collector")
	if !ok {
		return
	}
	f := newProcessFrame(c, msg, len(body))

	switch b := msg.Body.(type) {
	case *process.CollectorProc:
		a.storeProcessSnapshot(f, msg)
		a.storeProcesses(f, b)
		a.storeProcContainers(f, b)
	case *process.CollectorRealTime:
		log.Printf("[collector] realtime host=%s group=%d/%d stats=%d container_stats=%d cpus=%d mem=%d %s",
			b.HostName, b.GroupId, b.GroupSize, len(b.Stats), len(b.ContainerStats),
			b.NumCpus, b.TotalMemory, processAgentIdentity(c))
		a.storeProcessSnapshot(f, msg)
		a.storeProcessStats(f, b)
		a.storeRealTimeContainerStats(f, b)
	default:
		log.Printf("[collector] type=%d %T - not handled", msg.Header.Type, msg.Body)
		a.storeRaw(c, "process", "unexpected_shape",
			fmt.Sprintf("/api/v1/collector decoded to %T (frame type %d)", msg.Body, msg.Header.Type), body)
	}
}

// HandleContainer accepts the container check. One path, two message types:
// 39 (CollectorContainer, full inventory every 10 s) and 40
// (CollectorContainerRealTime, stats only every 2 s). The frame's type byte
// decides, not the body shape.
func (a *Server) HandleContainer(c *gin.Context) {
	defer c.Data(http.StatusOK, "application/x-protobuf", resCollectorResponse())

	msg, body, ok := a.decodeProcessFrame(c, "container")
	if !ok {
		return
	}
	f := newProcessFrame(c, msg, len(body))

	switch b := msg.Body.(type) {
	case *process.CollectorContainer:
		log.Printf("[container] host=%s network=%s group=%d/%d containers=%d %s",
			b.HostName, b.NetworkId, b.GroupId, b.GroupSize,
			len(b.Containers), processAgentIdentity(c))
		a.storeProcessSnapshot(f, msg)
		a.storeContainerInventory(f, b)
	case *process.CollectorContainerRealTime:
		log.Printf("[container] realtime host=%s group=%d/%d stats=%d cpus=%d mem=%d %s",
			b.HostName, b.GroupId, b.GroupSize, len(b.Stats), b.NumCpus, b.TotalMemory,
			processAgentIdentity(c))
		a.storeProcessSnapshot(f, msg)
		a.storeContainerRealTime(f, b)
	default:
		log.Printf("[container] type=%d %T - not handled", msg.Header.Type, msg.Body)
		a.storeRaw(c, "process", "unexpected_shape",
			fmt.Sprintf("/api/v1/container decoded to %T (frame type %d)", msg.Body, msg.Header.Type), body)
	}
}

// HandleConnections accepts the network check (type 22). The body is the
// system-probe's connection table: endpoints, DNS, routes and eBPF telemetry.
func (a *Server) HandleConnections(c *gin.Context) {
	defer c.Data(http.StatusOK, "application/x-protobuf", resCollectorResponse())

	msg, body, ok := a.decodeProcessFrame(c, "connections")
	if !ok {
		return
	}

	conns, isConn := msg.Body.(*process.CollectorConnections)
	if !isConn {
		log.Printf("[connections] type=%d %T - not handled", msg.Header.Type, msg.Body)
		a.storeRaw(c, "process", "unexpected_shape",
			fmt.Sprintf("/api/v1/connections decoded to %T (frame type %d)", msg.Body, msg.Header.Type), body)
		return
	}

	log.Printf("[connections] host=%s network=%s group=%d/%d connections=%d routes=%d domains=%d platform=%s/%s kernel=%s %s",
		conns.HostName, conns.NetworkId, conns.GroupId, conns.GroupSize, len(conns.Connections),
		len(conns.Routes), len(conns.Domains), conns.Platform, conns.PlatformVersion, conns.KernelVersion,
		processAgentIdentity(c))

	f := newProcessFrame(c, msg, len(body))
	a.storeProcessSnapshot(f, msg)
	a.storeConnections(f, conns)
}

// HandleProcDiscovery accepts the lightweight process discovery check
// (type 53): pid, command and user only, no resource usage.
func (a *Server) HandleProcDiscovery(c *gin.Context) {
	defer c.Data(http.StatusOK, "application/x-protobuf", resCollectorResponse())

	msg, body, ok := a.decodeProcessFrame(c, "discovery")
	if !ok {
		return
	}

	disc, isDisc := msg.Body.(*process.CollectorProcDiscovery)
	if !isDisc {
		log.Printf("[discovery] type=%d %T - not handled", msg.Header.Type, msg.Body)
		a.storeRaw(c, "process", "unexpected_shape",
			fmt.Sprintf("/api/v1/discovery decoded to %T (frame type %d)", msg.Body, msg.Header.Type), body)
		return
	}

	log.Printf("[discovery] host=%s group=%d/%d processes=%d %s",
		disc.HostName, disc.GroupId, disc.GroupSize, len(disc.ProcessDiscoveries),
		processAgentIdentity(c))

	f := newProcessFrame(c, msg, len(body))
	a.storeProcessSnapshot(f, msg)
	a.storeProcessDiscoveries(f, disc)
}

// decodeProcessFrame reads the body and unwraps the 16-byte frame. The
// encoding byte (raw protobuf or zstd) is handled inside DecodeMessage; the
// HTTP layer never sees Content-Encoding on this intake.
//
// It returns the raw body alongside the message because a failure has to keep
// the bytes: a frame we cannot read is exactly the payload that would explain
// why, and it arrives once.
func (a *Server) decodeProcessFrame(c *gin.Context, label string) (process.Message, []byte, bool) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[%s] cannot read body: %v", label, err)
		return process.Message{}, nil, false
	}

	msg, err := process.DecodeMessage(body)
	if err != nil {
		log.Printf("[%s] process-agent frame: %v (%d bytes)", label, err, len(body))
		a.storeRaw(c, "process", "decode_error", label+": "+err.Error(), body)
		return process.Message{}, body, false
	}
	return msg, body, true
}

// processAgentIdentity renders the headers the process-agent stamps on every
// submit. X-DD-Request-ID is only sent by the process and connections checks.
func processAgentIdentity(c *gin.Context) string {
	id := processAgentHeaders(c)
	s := "agent-host=" + id.Hostname +
		" agent-version=" + id.Version +
		" container-count=" + id.ContainerCount
	if id.RequestID != "" {
		s += " request-id=" + id.RequestID
	}
	return s
}

// processAgent is the sender identity the process-agent stamps on every
// submit. It used to exist only inside a log line; every table of this intake
// now keeps at least the version and the request id, because "which agent
// wrote this row" is the first question asked of a payload that looks wrong.
type processAgent struct {
	Hostname       string
	Version        string
	ContainerCount string
	RequestID      string
}

func processAgentHeaders(c *gin.Context) processAgent {
	return processAgent{
		Hostname:       c.GetHeader("X-Dd-Hostname"),
		Version:        c.GetHeader("X-Dd-Processagentversion"),
		ContainerCount: c.GetHeader("X-Dd-ContainerCount"),
		RequestID:      c.GetHeader("X-DD-Request-ID"),
	}
}

// processFrame is everything about a received frame that does not come out of
// its body: who sent it, when it arrived, and the id every row it produces
// carries back to process_snapshots.
type processFrame struct {
	Tenant     string
	SnapshotID uuid.UUID
	At         time.Time
	Path       string
	Bytes      int
	Header     process.MessageHeader
	Agent      processAgent
}

// newProcessFrame stamps a frame on arrival.
//
// At is ARRIVAL time. The body carries no sample timestamp — CreateTime is a
// process start, not a sample — and the frame header's own timestamp field is
// left at zero by the agent, so arrival is the only honest answer. Snapshots
// travel within a second of being taken. The header's raw value is stored
// beside it, uninterpreted, in case a sender ever fills it in.
func newProcessFrame(c *gin.Context, msg process.Message, size int) processFrame {
	return processFrame{
		Tenant:     TenantFromContext(c),
		SnapshotID: uuid.New(),
		At:         time.Now().UTC(),
		Path:       c.Request.URL.Path,
		Bytes:      size,
		Header:     msg.Header,
		Agent:      processAgentHeaders(c),
	}
}

// ---------------------------------------------------------------------------
// process_snapshots
// ---------------------------------------------------------------------------

// storeProcessSnapshot writes the frame envelope, whatever the message type.
func (a *Server) storeProcessSnapshot(f processFrame, msg process.Message) {
	if f.Tenant == "" {
		return
	}
	row := processSnapshotRow(f, msg)
	a.store(storage.ProcessSnapshotsWriter,
		storage.WriteProcessSnapshots{Snapshots: []storage.ProcessSnapshotRow{row}}, 1)
}

// processSnapshotRow projects one frame onto the envelope table. It handles
// all six message types, because the envelope fields are spread differently
// across them: CollectorProc and friends carry a *Host and a *SystemInfo,
// while the realtime messages inline a host id, an org id, a cpu count and a
// memory total instead.
func processSnapshotRow(f processFrame, msg process.Message) storage.ProcessSnapshotRow {
	row := storage.ProcessSnapshotRow{
		TenantID:             f.Tenant,
		ReceivedAt:           f.At,
		SnapshotID:           f.SnapshotID,
		MessageType:          uint8(msg.Header.Type),
		MessageTypeName:      processMessageTypeName(msg.Header.Type),
		Path:                 f.Path,
		HeaderVersion:        uint8(msg.Header.Version),
		HeaderEncoding:       uint8(msg.Header.Encoding),
		HeaderSubscriptionID: msg.Header.SubscriptionID,
		HeaderOrgID:          msg.Header.OrgID,
		HeaderTimestamp:      msg.Header.Timestamp,
		FrameBytes:           uint64(f.Bytes),
		AgentHostname:        f.Agent.Hostname,
		AgentVersion:         f.Agent.Version,
		AgentContainerCount:  f.Agent.ContainerCount,
		RequestID:            f.Agent.RequestID,
	}

	switch b := msg.Body.(type) {
	case *process.CollectorProc:
		row.HostName = b.HostName
		row.NetworkID = b.NetworkId
		row.GroupID, row.GroupSize = b.GroupId, b.GroupSize
		row.ContainerHostType = b.ContainerHostType.String()
		applySnapshotHost(&row, b.Host)
		applySnapshotSystemInfo(&row, b.Info)
		row.ProcessCount = uint32(len(b.Processes))
		row.ContainerCount = uint32(len(b.Containers))
		// Hints is a oneof with one member today. nil stays NULL: "no hints"
		// and "hint mask 0" are different statements.
		if h, ok := b.Hints.(*process.CollectorProc_HintMask); ok && h != nil {
			mask := h.HintMask
			row.HintMask = &mask
		}
	case *process.CollectorRealTime:
		row.HostName = b.HostName
		row.GroupID, row.GroupSize = b.GroupId, b.GroupSize
		row.ContainerHostType = b.ContainerHostType.String()
		row.RTHostID, row.RTOrgID = b.HostId, b.OrgId
		row.RTNumCPUs, row.RTTotalMemory = b.NumCpus, b.TotalMemory
		row.ProcessStatCount = uint32(len(b.Stats))
		row.ContainerStatCount = uint32(len(b.ContainerStats))
	case *process.CollectorContainer:
		row.HostName = b.HostName
		row.NetworkID = b.NetworkId
		row.GroupID, row.GroupSize = b.GroupId, b.GroupSize
		row.ContainerHostType = b.ContainerHostType.String()
		applySnapshotHost(&row, b.Host)
		applySnapshotSystemInfo(&row, b.Info)
		row.ContainerCount = uint32(len(b.Containers))
	case *process.CollectorContainerRealTime:
		row.HostName = b.HostName
		row.GroupID, row.GroupSize = b.GroupId, b.GroupSize
		row.ContainerHostType = b.ContainerHostType.String()
		row.RTHostID = b.HostId
		row.RTNumCPUs, row.RTTotalMemory = b.NumCpus, b.TotalMemory
		row.ContainerStatCount = uint32(len(b.Stats))
	case *process.CollectorProcDiscovery:
		row.HostName = b.HostName
		row.GroupID, row.GroupSize = b.GroupId, b.GroupSize
		applySnapshotHost(&row, b.Host)
		row.DiscoveryCount = uint32(len(b.ProcessDiscoveries))
	case *process.CollectorConnections:
		row.HostName = b.HostName
		row.NetworkID = b.NetworkId
		row.GroupID, row.GroupSize = b.GroupId, b.GroupSize
		row.ContainerHostType = b.ContainerHostType.String()
		row.ConnectionCount = uint32(len(b.Connections))
	}
	return row
}

func applySnapshotHost(row *storage.ProcessSnapshotRow, h *process.Host) {
	if h == nil {
		return
	}
	row.HostID = h.Id
	row.HostOrgID = h.OrgId
	row.HostDisplayName = h.Name
	row.HostAllTags = tagsToMultiMap(h.AllTags)
	row.HostNumCPUs = h.NumCpus
	row.HostTotalMemory = h.TotalMemory
	row.HostTagIndex = h.TagIndex
	row.HostTagsModified = unixSecondsPtr(h.TagsModified)
}

func applySnapshotSystemInfo(row *storage.ProcessSnapshotRow, info *process.SystemInfo) {
	if info == nil {
		return
	}
	row.SysUUID = info.Uuid
	row.SysTotalMemory = info.TotalMemory
	if os := info.Os; os != nil {
		row.OSName = os.Name
		row.OSPlatform = os.Platform
		row.OSFamily = os.Family
		row.OSVersion = os.Version
		row.KernelVersion = os.KernelVersion
	}
	// Nine parallel arrays: index i of each is one CPUInfo, in the order the
	// agent sent them. A map keyed by number would drop a host that reports
	// two sockets with the same numbering.
	for _, cpu := range info.Cpus {
		if cpu == nil {
			continue
		}
		row.CPUNumbers = append(row.CPUNumbers, cpu.Number)
		row.CPUVendors = append(row.CPUVendors, cpu.Vendor)
		row.CPUFamilies = append(row.CPUFamilies, cpu.Family)
		row.CPUModels = append(row.CPUModels, cpu.Model)
		row.CPUPhysicalIDs = append(row.CPUPhysicalIDs, cpu.PhysicalId)
		row.CPUCoreIDs = append(row.CPUCoreIDs, cpu.CoreId)
		row.CPUCores = append(row.CPUCores, cpu.Cores)
		row.CPUMhz = append(row.CPUMhz, cpu.Mhz)
		row.CPUCacheSizes = append(row.CPUCacheSizes, cpu.CacheSize)
	}
}

// processMessageTypeName names the frame's type byte. The number is the
// contract and is stored too; the name is what a human reads.
func processMessageTypeName(t process.MessageType) string {
	switch t {
	case process.TypeCollectorProc:
		return "CollectorProc"
	case process.TypeCollectorRealTime:
		return "CollectorRealTime"
	case process.TypeCollectorContainer:
		return "CollectorContainer"
	case process.TypeCollectorContainerRealTime:
		return "CollectorContainerRealTime"
	case process.TypeCollectorProcDiscovery:
		return "CollectorProcDiscovery"
	case process.TypeCollectorConnections:
		return "CollectorConnections"
	}
	return fmt.Sprintf("type_%d", uint8(t))
}

// ---------------------------------------------------------------------------
// processes
// ---------------------------------------------------------------------------

// storeProcesses turns a process-agent snapshot into rows, one per process.
//
// Every field of process.Process reaches a column. The ones that are whole
// sub-messages the backend fills in post-resolution (Process.Host) go to a
// JSON column rather than eight empty ones; Process.Container goes to the
// containers table with source=process. What is left out, and why, is listed
// in docs/tables/process.md.
func (a *Server) storeProcesses(f processFrame, proc *process.CollectorProc) {
	if f.Tenant == "" || len(proc.Processes) == 0 {
		return
	}

	rows := make([]storage.ProcessRow, 0, len(proc.Processes))
	for _, p := range proc.Processes {
		if p == nil {
			continue
		}
		rows = append(rows, processRow(f, proc, p))
	}
	a.store(storage.ProcessesWriter, storage.WriteProcesses{Processes: rows}, len(rows))
}

func processRow(f processFrame, proc *process.CollectorProc, p *process.Process) storage.ProcessRow {
	row := storage.ProcessRow{
		TenantID: f.Tenant, Timestamp: f.At, Host: proc.HostName,
		PID: p.Pid, State: p.State.String(), OpenFDs: p.OpenFdCount,
		ContainerID: p.ContainerId, Tags: tagsToMultiMap(p.Tags),

		SnapshotID: f.SnapshotID,
		NsPID:      p.NsPid,
		Key:        p.Key,

		VoluntaryCtxSwitches:   p.VoluntaryCtxSwitches,
		InvoluntaryCtxSwitches: p.InvoluntaryCtxSwitches,

		ProcessContext: p.ProcessContext,
		Language:       p.Language.String(),
		InjectionState: p.InjectionState.String(),

		ZombieChildrenCount:  p.ZombieChildrenCount,
		ZombieNetRate:        p.ZombieNetRate,
		HasZombieAggregation: boolToUint8(p.HasZombieAggregation),

		ContainerKey:     p.ContainerKey,
		ByteKey:          string(p.ByteKey),
		ContainerByteKey: string(p.ContainerByteKey),
		ProcessHost:      protoJSON(p.Host),

		NetworkID:         proc.NetworkId,
		GroupID:           proc.GroupId,
		GroupSize:         proc.GroupSize,
		ContainerHostType: proc.ContainerHostType.String(),
		AgentVersion:      f.Agent.Version,
		RequestID:         f.Agent.RequestID,
	}

	// Every nested struct is a pointer and may be nil — the agent omits
	// what it could not read.
	if p.Command != nil {
		row.PPID = p.Command.Ppid
		row.Comm = p.Command.Comm
		row.Exe = p.Command.Exe
		// Both forms. Args is the fidelity copy; Cmdline is the convenience
		// one and cannot be split back when an argument contains a space.
		row.Args = p.Command.Args
		row.Cmdline = strings.Join(p.Command.Args, " ")
		row.Cwd = p.Command.Cwd
		row.Root = p.Command.Root
		row.OnDisk = boolToUint8(p.Command.OnDisk)
		row.Pgroup = p.Command.Pgroup
	}
	if p.User != nil {
		row.User = p.User.Name
		row.UID, row.GID = p.User.Uid, p.User.Gid
		row.EUID, row.EGID = p.User.Euid, p.User.Egid
		row.SUID, row.SGID = p.User.Suid, p.User.Sgid
	}
	if p.Memory != nil {
		row.RSS, row.VMS = p.Memory.Rss, p.Memory.Vms
		row.MemSwap, row.MemShared = p.Memory.Swap, p.Memory.Shared
		row.MemText, row.MemLib = p.Memory.Text, p.Memory.Lib
		row.MemData, row.MemDirty = p.Memory.Data, p.Memory.Dirty
	}
	if p.Cpu != nil {
		row.CPUPct = p.Cpu.TotalPct
		row.Threads = p.Cpu.NumThreads
		row.CPULastCPU = p.Cpu.LastCpu
		row.CPUUserPct, row.CPUSystemPct = p.Cpu.UserPct, p.Cpu.SystemPct
		row.CPUNice = p.Cpu.Nice
		row.CPUUserTime, row.CPUSystemTime = p.Cpu.UserTime, p.Cpu.SystemTime
		row.CPUCoreNames, row.CPUCorePcts = singleCPUStats(p.Cpu.Cpus)
	}
	if p.IoStat != nil {
		row.IOReadRate, row.IOWriteRate = p.IoStat.ReadRate, p.IoStat.WriteRate
		row.IOReadBytesRate, row.IOWriteBytesRate = p.IoStat.ReadBytesRate, p.IoStat.WriteBytesRate
	}
	if p.Networks != nil {
		row.NetConnectionRate = p.Networks.ConnectionRate
		row.NetBytesRate = p.Networks.BytesRate
	}
	if p.PortInfo != nil {
		row.PortTCP, row.PortUDP = p.PortInfo.Tcp, p.PortInfo.Udp
	}
	applyServiceDiscovery(&row, p.ServiceDiscovery)

	// create_time is a plain DateTime column, so absence is carried beside it
	// rather than as NULL: has_create_time = 0 means the agent sent no start
	// time and CreateTime is meaningless, not 1970.
	if p.CreateTime > 0 {
		row.CreateTime = time.UnixMilli(p.CreateTime).UTC() // milliseconds
		row.HasCreateTime = 1
	}

	if proc.Info != nil {
		row.HostUUID = proc.Info.Uuid
		if os := proc.Info.Os; os != nil {
			row.OSName, row.OSPlatform = os.Name, os.Platform
			row.OSFamily, row.OSVersion = os.Family, os.Version
			row.KernelVersion = os.KernelVersion
		}
	}
	return row
}

// applyServiceDiscovery flattens ServiceDiscovery onto the row. This is the
// bridge between a process and APM: tracer_runtime_ids is the only join
// between "this pid on this host" and the traces it emitted.
func applyServiceDiscovery(row *storage.ProcessRow, sd *process.ServiceDiscovery) {
	if sd == nil {
		return
	}
	if n := sd.GeneratedServiceName; n != nil {
		row.GeneratedServiceName = n.Name
		row.GeneratedServiceNameSource = n.Source.String()
	}
	if n := sd.DdServiceName; n != nil {
		row.DDServiceName = n.Name
		row.DDServiceNameSource = n.Source.String()
	}
	// Parallel arrays, in the order sent: name i came from source i.
	for _, n := range sd.AdditionalGeneratedNames {
		if n == nil {
			continue
		}
		row.AdditionalGeneratedNames = append(row.AdditionalGeneratedNames, n.Name)
		row.AdditionalGeneratedNameSources = append(row.AdditionalGeneratedNameSources, n.Source.String())
	}
	for _, t := range sd.TracerMetadata {
		if t == nil {
			continue
		}
		row.TracerRuntimeIDs = append(row.TracerRuntimeIDs, t.RuntimeId)
		row.TracerServiceNames = append(row.TracerServiceNames, t.ServiceName)
	}
	row.APMInstrumentation = boolToUint8(sd.ApmInstrumentation)
	if len(sd.Resources) > 0 {
		row.ServiceResources = protoJSON(sd.Resources)
	}
}

// singleCPUStats splits the per-core breakdown into the two parallel arrays
// the column pair expects.
func singleCPUStats(cpus []*process.SingleCPUStat) ([]string, []float32) {
	if len(cpus) == 0 {
		return nil, nil
	}
	names := make([]string, 0, len(cpus))
	pcts := make([]float32, 0, len(cpus))
	for _, cpu := range cpus {
		if cpu == nil {
			continue
		}
		names = append(names, cpu.Name)
		pcts = append(pcts, cpu.TotalPct)
	}
	return names, pcts
}

// ---------------------------------------------------------------------------
// process_stats
// ---------------------------------------------------------------------------

func (a *Server) storeProcessStats(f processFrame, rt *process.CollectorRealTime) {
	if f.Tenant == "" || len(rt.Stats) == 0 {
		return
	}
	rows := make([]storage.ProcessStatRow, 0, len(rt.Stats))
	for _, s := range rt.Stats {
		if s == nil {
			continue
		}
		rows = append(rows, processStatRow(f, rt, s))
	}
	a.store(storage.ProcessStatsWriter, storage.WriteProcessStats{Stats: rows}, len(rows))
}

func processStatRow(f processFrame, rt *process.CollectorRealTime, s *process.ProcessStat) storage.ProcessStatRow {
	row := storage.ProcessStatRow{
		TenantID: f.Tenant, Timestamp: f.At, Host: rt.HostName,
		SnapshotID: f.SnapshotID,

		PID: s.Pid, Key: s.Key,
		// ProcessStat.Nice and .Threads are the message's own fields and are
		// not the nested CPUStat's; the agent does not always fill both.
		Nice: s.Nice, Threads: s.Threads, OpenFDs: s.OpenFdCount,

		VoluntaryCtxSwitches:   s.VoluntaryCtxSwitches,
		InvoluntaryCtxSwitches: s.InvoluntaryCtxSwitches,

		ContainerID:    s.ContainerId,
		ContainerState: s.ContainerState.String(),
		ProcessState:   s.ProcessState.String(),

		ContainerHealth:     s.ContainerHealth.String(),
		ContainerRbps:       s.ContainerRbps,
		ContainerWbps:       s.ContainerWbps,
		ContainerKey:        s.ContainerKey,
		ContainerNetRcvdPs:  s.ContainerNetRcvdPs,
		ContainerNetSentPs:  s.ContainerNetSentPs,
		ContainerNetRcvdBps: s.ContainerNetRcvdBps,
		ContainerNetSentBps: s.ContainerNetSentBps,

		ByteKey:          string(s.ByteKey),
		ContainerByteKey: string(s.ContainerByteKey),

		GroupID: rt.GroupId, GroupSize: rt.GroupSize,
		ContainerHostType: rt.ContainerHostType.String(),
		RTHostID:          rt.HostId,
		RTOrgID:           rt.OrgId,
		RTNumCPUs:         rt.NumCpus,
		RTTotalMemory:     rt.TotalMemory,
		AgentVersion:      f.Agent.Version,
		RequestID:         f.Agent.RequestID,
	}
	if s.CreateTime > 0 {
		row.CreateTime = time.UnixMilli(s.CreateTime).UTC()
		row.HasCreateTime = 1
	}
	if m := s.Memory; m != nil {
		row.MemRSS, row.MemVMS = m.Rss, m.Vms
		row.MemSwap, row.MemShared = m.Swap, m.Shared
		row.MemText, row.MemLib = m.Text, m.Lib
		row.MemData, row.MemDirty = m.Data, m.Dirty
	}
	if cpu := s.Cpu; cpu != nil {
		row.CPULastCPU = cpu.LastCpu
		row.CPUTotalPct, row.CPUUserPct, row.CPUSystemPct = cpu.TotalPct, cpu.UserPct, cpu.SystemPct
		row.CPUNumThreads = cpu.NumThreads
		row.CPUNice = cpu.Nice
		row.CPUUserTime, row.CPUSystemTime = cpu.UserTime, cpu.SystemTime
		row.CPUCoreNames, row.CPUCorePcts = singleCPUStats(cpu.Cpus)
	}
	if io := s.IoStat; io != nil {
		row.IOReadRate, row.IOWriteRate = io.ReadRate, io.WriteRate
		row.IOReadBytesRate, row.IOWriteBytesRate = io.ReadBytesRate, io.WriteBytesRate
	}
	if n := s.Networks; n != nil {
		row.NetConnectionRate, row.NetBytesRate = n.ConnectionRate, n.BytesRate
	}
	return row
}

// ---------------------------------------------------------------------------
// containers
// ---------------------------------------------------------------------------

// containerEnv is the envelope a Container row inherits. It differs per
// message, so it is passed rather than re-derived.
type containerEnv struct {
	Source            string
	PID               int32
	NetworkID         string
	GroupID           int32
	GroupSize         int32
	ContainerHostType string
}

// storeProcContainers writes both places a Container hides inside a
// CollectorProc: the snapshot-level inventory, and the copy hanging off an
// individual Process. Both are stored, tagged by source — the per-process one
// carries limits and health the inventory sometimes lacks, and losing it
// would mean losing the only container record for a process whose container
// the snapshot inventory missed.
func (a *Server) storeProcContainers(f processFrame, proc *process.CollectorProc) {
	if f.Tenant == "" {
		return
	}
	env := containerEnv{
		Source: containerSourceProc, NetworkID: proc.NetworkId,
		GroupID: proc.GroupId, GroupSize: proc.GroupSize,
		ContainerHostType: proc.ContainerHostType.String(),
	}
	rows := containerRows(f, env, proc.HostName, proc.Containers)

	for _, p := range proc.Processes {
		if p == nil || p.Container == nil {
			continue
		}
		perProcess := env
		perProcess.Source = containerSourceProcess
		perProcess.PID = p.Pid
		rows = append(rows, containerRows(f, perProcess, proc.HostName, []*process.Container{p.Container})...)
	}
	a.store(storage.ContainersWriter, storage.WriteContainers{Containers: rows}, len(rows))
}

func (a *Server) storeContainerInventory(f processFrame, cc *process.CollectorContainer) {
	if f.Tenant == "" || len(cc.Containers) == 0 {
		return
	}
	env := containerEnv{
		Source: containerSourceContainer, NetworkID: cc.NetworkId,
		GroupID: cc.GroupId, GroupSize: cc.GroupSize,
		ContainerHostType: cc.ContainerHostType.String(),
	}
	rows := containerRows(f, env, cc.HostName, cc.Containers)
	a.store(storage.ContainersWriter, storage.WriteContainers{Containers: rows}, len(rows))
}

func containerRows(f processFrame, env containerEnv, host string, cs []*process.Container) []storage.ContainerRow {
	rows := make([]storage.ContainerRow, 0, len(cs))
	for _, ctr := range cs {
		if ctr == nil {
			continue
		}
		row := storage.ContainerRow{
			TenantID: f.Tenant, Timestamp: f.At, Host: host,
			SnapshotID: f.SnapshotID,
			Source:     env.Source, PID: env.PID,

			Type: ctr.Type, ID: ctr.Id, Name: ctr.Name, Image: ctr.Image,
			RepoDigest: ctr.RepoDigest,

			CPULimit: ctr.CpuLimit, MemoryLimit: ctr.MemoryLimit,
			CPURequest: ctr.CpuRequest, MemoryRequest: ctr.MemoryRequest,

			State:  ctr.State.String(),
			Health: ctr.Health.String(),
			// Unix SECONDS on the wire (the agent sends CreatedAt.Unix()).
			// nil, not 1970, when the runtime reported none.
			Created: unixSecondsPtr(ctr.Created),
			Started: unixSecondsPtr(ctr.Started),

			Rbps: ctr.Rbps, Wbps: ctr.Wbps,
			NetRcvdPs: ctr.NetRcvdPs, NetSentPs: ctr.NetSentPs,
			NetRcvdBps: ctr.NetRcvdBps, NetSentBps: ctr.NetSentBps,

			UserPct: ctr.UserPct, SystemPct: ctr.SystemPct,
			TotalPct: ctr.TotalPct, CPUUsageNs: ctr.CpuUsageNs,

			MemRSS: ctr.MemRss, MemCache: ctr.MemCache,
			MemUsage: ctr.MemUsage, MemAccounted: ctr.MemAccounted,

			ThreadCount: ctr.ThreadCount, ThreadLimit: ctr.ThreadLimit,

			Key: ctr.Key, ByteKey: string(ctr.ByteKey),
			Tags:     tagsToMultiMap(ctr.Tags),
			HostInfo: protoJSON(ctr.Host),

			NetworkID: env.NetworkID, GroupID: env.GroupID, GroupSize: env.GroupSize,
			ContainerHostType: env.ContainerHostType,
			AgentVersion:      f.Agent.Version,
			RequestID:         f.Agent.RequestID,
		}
		// Three parallel arrays, order as sent: a container legitimately
		// binds the same port on two addresses.
		for _, addr := range ctr.Addresses {
			if addr == nil {
				continue
			}
			row.AddrIPs = append(row.AddrIPs, addr.Ip)
			row.AddrPorts = append(row.AddrPorts, addr.Port)
			row.AddrProtocols = append(row.AddrProtocols, addr.Protocol.String())
		}
		rows = append(rows, row)
	}
	return rows
}

// ---------------------------------------------------------------------------
// container_stats
// ---------------------------------------------------------------------------

func (a *Server) storeRealTimeContainerStats(f processFrame, rt *process.CollectorRealTime) {
	if f.Tenant == "" || len(rt.ContainerStats) == 0 {
		return
	}
	env := containerStatEnv{
		Source: containerStatSourceRealTime,
		Host:   rt.HostName, GroupID: rt.GroupId, GroupSize: rt.GroupSize,
		ContainerHostType: rt.ContainerHostType.String(),
		RTHostID:          rt.HostId, RTOrgID: rt.OrgId,
		RTNumCPUs: rt.NumCpus, RTTotalMemory: rt.TotalMemory,
	}
	rows := containerStatRows(f, env, rt.ContainerStats)
	a.store(storage.ContainerStatsWriter, storage.WriteContainerStats{Stats: rows}, len(rows))
}

func (a *Server) storeContainerRealTime(f processFrame, rt *process.CollectorContainerRealTime) {
	if f.Tenant == "" || len(rt.Stats) == 0 {
		return
	}
	env := containerStatEnv{
		Source: containerStatSourceCtrRT,
		Host:   rt.HostName, GroupID: rt.GroupId, GroupSize: rt.GroupSize,
		ContainerHostType: rt.ContainerHostType.String(),
		// CollectorContainerRealTime carries no org id.
		RTHostID:  rt.HostId,
		RTNumCPUs: rt.NumCpus, RTTotalMemory: rt.TotalMemory,
	}
	rows := containerStatRows(f, env, rt.Stats)
	a.store(storage.ContainerStatsWriter, storage.WriteContainerStats{Stats: rows}, len(rows))
}

type containerStatEnv struct {
	Source            string
	Host              string
	GroupID           int32
	GroupSize         int32
	ContainerHostType string
	RTHostID          int64
	RTOrgID           int32
	RTNumCPUs         int32
	RTTotalMemory     int64
}

func containerStatRows(f processFrame, env containerStatEnv, stats []*process.ContainerStat) []storage.ContainerStatRow {
	rows := make([]storage.ContainerStatRow, 0, len(stats))
	for _, s := range stats {
		if s == nil {
			continue
		}
		rows = append(rows, storage.ContainerStatRow{
			TenantID: f.Tenant, Timestamp: f.At, Host: env.Host,
			SnapshotID: f.SnapshotID, Source: env.Source,

			ID: s.Id,

			UserPct: s.UserPct, SystemPct: s.SystemPct, TotalPct: s.TotalPct,
			CPULimit: s.CpuLimit, CPURequest: s.CpuRequest, CPUUsageNs: s.CpuUsageNs,

			MemRSS: s.MemRss, MemCache: s.MemCache, MemLimit: s.MemLimit,
			MemUsage: s.MemUsage, MemAccounted: s.MemAccounted,
			MemoryRequest: s.MemoryRequest,

			Rbps: s.Rbps, Wbps: s.Wbps,
			NetRcvdPs: s.NetRcvdPs, NetSentPs: s.NetSentPs,
			NetRcvdBps: s.NetRcvdBps, NetSentBps: s.NetSentBps,

			State: s.State.String(), Health: s.Health.String(),

			Key: s.Key, Started: unixSecondsPtr(s.Started),
			ByteKey:     string(s.ByteKey),
			ThreadCount: s.ThreadCount, ThreadLimit: s.ThreadLimit,

			GroupID: env.GroupID, GroupSize: env.GroupSize,
			ContainerHostType: env.ContainerHostType,
			RTHostID:          env.RTHostID, RTOrgID: env.RTOrgID,
			RTNumCPUs: env.RTNumCPUs, RTTotalMemory: env.RTTotalMemory,
			AgentVersion: f.Agent.Version,
			RequestID:    f.Agent.RequestID,
		})
	}
	return rows
}

// ---------------------------------------------------------------------------
// process_discoveries
// ---------------------------------------------------------------------------

func (a *Server) storeProcessDiscoveries(f processFrame, disc *process.CollectorProcDiscovery) {
	if f.Tenant == "" || len(disc.ProcessDiscoveries) == 0 {
		return
	}
	rows := make([]storage.ProcessDiscoveryRow, 0, len(disc.ProcessDiscoveries))
	for _, d := range disc.ProcessDiscoveries {
		if d == nil {
			continue
		}
		rows = append(rows, processDiscoveryRow(f, disc, d))
	}
	a.store(storage.ProcessDiscoveriesWriter,
		storage.WriteProcessDiscoveries{Discoveries: rows}, len(rows))
}

func processDiscoveryRow(f processFrame, disc *process.CollectorProcDiscovery, d *process.ProcessDiscovery) storage.ProcessDiscoveryRow {
	row := storage.ProcessDiscoveryRow{
		TenantID: f.Tenant, Timestamp: f.At, Host: disc.HostName,
		SnapshotID: f.SnapshotID,

		PID: d.Pid, NsPID: d.NsPid, ByteKey: string(d.ByteKey),
		HostInfo: protoJSON(d.Host),

		GroupID: disc.GroupId, GroupSize: disc.GroupSize,
		AgentVersion: f.Agent.Version,
		RequestID:    f.Agent.RequestID,
	}
	if d.CreateTime > 0 {
		row.CreateTime = time.UnixMilli(d.CreateTime).UTC()
		row.HasCreateTime = 1
	}
	if cmd := d.Command; cmd != nil {
		row.Comm, row.Exe = cmd.Comm, cmd.Exe
		row.Args = cmd.Args
		row.Cmdline = strings.Join(cmd.Args, " ")
		row.Cwd, row.Root = cmd.Cwd, cmd.Root
		row.OnDisk = boolToUint8(cmd.OnDisk)
		row.PPID, row.Pgroup = cmd.Ppid, cmd.Pgroup
	}
	if u := d.User; u != nil {
		row.User = u.Name
		row.UID, row.GID = u.Uid, u.Gid
		row.EUID, row.EGID = u.Euid, u.Egid
		row.SUID, row.SGID = u.Suid, u.Sgid
	}
	return row
}

// ---------------------------------------------------------------------------
// connections + connections_payloads
// ---------------------------------------------------------------------------

// storeConnections writes the payload row first and then the connection rows.
// Both carry the frame's uuid — as payload_id here, as snapshot_id on
// process_snapshots — which is what ties a connection to the kernel version,
// routing table and tag buffer it was collected with.
func (a *Server) storeConnections(f processFrame, conns *process.CollectorConnections) {
	if f.Tenant == "" {
		return
	}
	payload := connectionsPayloadRow(f, conns)
	a.store(storage.ConnectionsPayloadsWriter,
		storage.WriteConnectionsPayloads{Payloads: []storage.ConnectionsPayloadRow{payload}}, 1)

	rows := make([]storage.ConnectionRow, 0, len(conns.Connections))
	for _, conn := range conns.Connections {
		if conn == nil {
			continue
		}
		rows = append(rows, connectionRow(f, conns, conn))
	}
	a.store(storage.ConnectionsWriter, storage.WriteConnections{Connections: rows}, len(rows))
}

func connectionsPayloadRow(f processFrame, conns *process.CollectorConnections) storage.ConnectionsPayloadRow {
	row := storage.ConnectionsPayloadRow{
		TenantID: f.Tenant, ReceivedAt: f.At, PayloadID: f.SnapshotID,

		HostName: conns.HostName, NetworkID: conns.NetworkId,
		GroupID: conns.GroupId, GroupSize: conns.GroupSize,
		ContainerHostType: conns.ContainerHostType.String(),
		ConnectionCount:   uint32(len(conns.Connections)),

		Architecture: conns.Architecture, KernelVersion: conns.KernelVersion,
		Platform: conns.Platform, PlatformVersion: conns.PlatformVersion,

		ResolvedResources: protoJSON(conns.ResolvedResources),
		ContainerForPID:   conns.ContainerForPid,

		// The buffers stay even though the tags are resolved onto the
		// connection rows: the encoding is versioned (v1/v2/v3) and one we
		// cannot read yet must still land somewhere readable.
		EncodedTags:            string(conns.EncodedTags),
		EncodedConnectionsTags: string(conns.EncodedConnectionsTags),
		HostTagsIndex:          conns.HostTagsIndex,
		HostTags:               tagsToMultiMap(conns.GetTags(int(conns.HostTagsIndex))),

		ConnTelemetry:    connTelemetryMap(conns.ConnTelemetry),
		ConnTelemetryMap: conns.ConnTelemetryMap,

		CompilationTelemetry:    compilationTelemetryJSON(conns.CompilationTelemetryByAsset),
		KernelHeaderFetchResult: conns.KernelHeaderFetchResult.String(),
		CORETelemetry:           coreTelemetryNames(conns.CORETelemetryByAsset),
		PrebuiltEBPFAssets:      conns.PrebuiltEBPFAssets,

		Routes:             routesJSON(conns.Routes),
		RouteMetadata:      protoJSON(conns.RouteMetadata),
		AgentConfiguration: protoJSON(conns.AgentConfiguration),

		EncodedDNS:            string(conns.EncodedDNS),
		Domains:               conns.Domains,
		EncodedDomainDatabase: string(conns.EncodedDomainDatabase),
		EncodedDNSLookups:     string(conns.EncodedDnsLookups),

		ResolvedHostsByName: protoJSON(conns.ResolvedHostsByName),
		ResolvedPublicIPs:   protoJSON(conns.ResolvedPublicIps),

		EcsTask:     conns.EcsTask,
		ResolvConfs: conns.ResolvConfs,

		AgentHostname:       f.Agent.Hostname,
		AgentVersion:        f.Agent.Version,
		AgentContainerCount: f.Agent.ContainerCount,
		RequestID:           f.Agent.RequestID,
	}
	// The module's own reader, so a v1 buffer and a v2 domain database both
	// come out as names. An error here only means "no DNS encoding in this
	// payload", which is normal; the buffers are stored either way.
	if names, err := conns.GetDNSNames(); err == nil {
		row.DNSNames = names
	}
	return row
}

func connectionRow(f processFrame, conns *process.CollectorConnections, c *process.Connection) storage.ConnectionRow {
	row := storage.ConnectionRow{
		TenantID: f.Tenant, Timestamp: f.At, Host: conns.HostName,
		PayloadID: f.SnapshotID,

		PID: c.Pid,

		Family:               c.Family.String(),
		Type:                 c.Type.String(),
		Direction:            c.Direction.String(),
		IsLocalPortEphemeral: c.IsLocalPortEphemeral.String(),

		LastBytesSent: c.LastBytesSent, LastBytesReceived: c.LastBytesReceived,
		LastPacketsSent: c.LastPacketsSent, LastPacketsReceived: c.LastPacketsReceived,
		LastRetransmits: c.LastRetransmits,

		NetNS: c.NetNS, RemoteNetworkID: c.RemoteNetworkId,

		RTT: c.Rtt, RTTVar: c.RttVar, IntraHost: boolToUint8(c.IntraHost),

		DNSSuccessfulResponses: c.DnsSuccessfulResponses,
		DNSFailedResponses:     c.DnsFailedResponses,
		DNSTimeouts:            c.DnsTimeouts,
		DNSSuccessLatencySum:   c.DnsSuccessLatencySum,
		DNSFailureLatencySum:   c.DnsFailureLatencySum,
		DNSCountByRcode:        c.DnsCountByRcode,

		DNSStatsByDomain:                  protoJSON(c.DnsStatsByDomain),
		DNSStatsByDomainByQueryType:       protoJSON(c.DnsStatsByDomainByQueryType),
		DNSStatsByDomainOffsetByQueryType: protoJSON(c.DnsStatsByDomainOffsetByQueryType),

		LastTCPEstablished: c.LastTcpEstablished, LastTCPClosed: c.LastTcpClosed,

		RouteIdx: c.RouteIdx, RouteTargetIdx: c.RouteTargetIdx,
		ResolvConfIdx: c.ResolvConfIdx,

		// Opaque protobufs from a package this repo does not vendor. Kept as
		// bytes: a decoder can be written against stored bytes later, but not
		// against bytes we threw away.
		HTTPAggregations:        string(c.HttpAggregations),
		HTTP2Aggregations:       string(c.Http2Aggregations),
		DataStreamsAggregations: string(c.DataStreamsAggregations),
		DatabaseAggregations:    string(c.DatabaseAggregations),

		TagsIndices: c.Tags, TagsIdx: c.TagsIdx, TagsChecksum: c.TagsChecksum,
		// Resolved through the module's own reader against
		// EncodedConnectionsTags; the index and the buffer are both kept too.
		Tags:                    tagsToMultiMap(conns.GetConnectionsTags(c.TagsIdx)),
		LocalContainerTagsIndex: c.LocalContainerTagsIndex,
		LocalContainerTags:      tagsToMultiMap(conns.GetConnectionsTags(c.LocalContainerTagsIndex)),
		RemoteServiceTagsIdx:    c.RemoteServiceTagsIdx,
		RemoteServiceTags:       tagsToMultiMap(conns.GetConnectionsTags(c.RemoteServiceTagsIdx)),

		StateIndex:           c.StateIndex,
		TCPFailuresByErrCode: c.TcpFailuresByErrCode,
		RemoteEcsTask:        c.RemoteEcsTask,
		SystemProbeConn:      boolToUint8(c.SystemProbeConn),

		LastTCPRtoCount:      c.LastTcpRtoCount,
		LastTCPRecoveryCount: c.LastTcpRecoveryCount,
		LastTCPReordSeen:     c.LastTcpReordSeen,
		LastTCPRcvOooPack:    c.LastTcpRcvOooPack,
		LastTCPDeliveredCe:   c.LastTcpDeliveredCe,
		LastTCPProbe0Count:   c.LastTcpProbe0Count,
		TCPEcnNegotiated:     boolToUint8(c.TcpEcnNegotiated),

		AgentVersion: f.Agent.Version,
		RequestID:    f.Agent.RequestID,
	}
	if l := c.Laddr; l != nil {
		row.LaddrIP, row.LaddrPort = l.Ip, l.Port
		row.LaddrContainerID, row.LaddrHostName = l.ContainerId, l.HostName
	}
	if r := c.Raddr; r != nil {
		row.RaddrIP, row.RaddrPort = r.Ip, r.Port
		row.RaddrContainerID, row.RaddrHostName = r.ContainerId, r.HostName
	}
	if p := c.Protocol; p != nil {
		// Order is the stack: [protocolTLS, protocolHTTP2] is not the reverse.
		for _, layer := range p.Stack {
			row.ProtocolStack = append(row.ProtocolStack, layer.String())
		}
	}
	// Absent for every connection conntrack did not translate, which is most
	// of them — so nil, not 0.0.0.0:0.
	if t := c.IpTranslation; t != nil {
		srcIP, dstIP := t.ReplSrcIP, t.ReplDstIP
		srcPort, dstPort := t.ReplSrcPort, t.ReplDstPort
		row.IPTranslationReplSrcIP, row.IPTranslationReplDstIP = &srcIP, &dstIP
		row.IPTranslationReplSrcPort, row.IPTranslationReplDstPort = &srcPort, &dstPort
	}
	return row
}

// connTelemetryMap flattens the pre-7.35 fixed telemetry message into the same
// name/value shape as the open-ended connTelemetryMap newer agents send, so
// both eras read the same way. nil stays nil (and becomes an empty map at
// Append), which is how "this agent sent no telemetry" survives.
func connTelemetryMap(t *process.CollectorConnectionsTelemetry) map[string]int64 {
	if t == nil {
		return nil
	}
	return map[string]int64{
		"kprobes_triggered":           t.KprobesTriggered,
		"kprobes_missed":              t.KprobesMissed,
		"conntrack_registers":         t.ConntrackRegisters,
		"conntrack_registers_dropped": t.ConntrackRegistersDropped,
		"dns_packets_processed":       t.DnsPacketsProcessed,
		"conns_closed":                t.ConnsClosed,
		"conns_bpf_map_size":          t.ConnsBpfMapSize,
		"udp_sends_processed":         t.UdpSendsProcessed,
		"udp_sends_missed":            t.UdpSendsMissed,
		"conntrack_sampling_percent":  t.ConntrackSamplingPercent,
		"dns_stats_dropped":           t.DnsStatsDropped,
	}
}

// coreTelemetryNames stores the CO-RE result per asset BY NAME
// (SuccessCustomBTF, BtfNotFound, VerifierError, ...), never the number.
func coreTelemetryNames(m map[string]process.COREResult) map[string]string {
	if len(m) == 0 {
		return nil
	}
	out := make(map[string]string, len(m))
	for asset, result := range m {
		out[asset] = result.String()
	}
	return out
}

// compilationTelemetryJSON renders the per-asset runtime-compilation results
// with their enums spelled out; json.Marshal of the proto struct would write
// the numbers, and a renumbering upstream would then rewrite stored history.
func compilationTelemetryJSON(m map[string]*process.RuntimeCompilationTelemetry) string {
	if len(m) == 0 {
		return ""
	}
	type entry struct {
		RuntimeCompilationEnabled  bool   `json:"runtime_compilation_enabled"`
		RuntimeCompilationResult   string `json:"runtime_compilation_result"`
		RuntimeCompilationDuration int64  `json:"runtime_compilation_duration"`
		KernelHeaderFetchResult    string `json:"kernel_header_fetch_result"`
	}
	out := make(map[string]entry, len(m))
	for asset, t := range m {
		if t == nil {
			continue
		}
		out[asset] = entry{
			RuntimeCompilationEnabled:  t.RuntimeCompilationEnabled,
			RuntimeCompilationResult:   t.RuntimeCompilationResult.String(),
			RuntimeCompilationDuration: t.RuntimeCompilationDuration,
			KernelHeaderFetchResult:    t.KernelHeaderFetchResult.String(),
		}
	}
	return protoJSON(out)
}

// routesJSON flattens the routing table. Route is two one-field sub-messages
// and json.Marshal of it would nest three levels deep for two strings; the
// INDEX is what matters, because connections.route_idx points into this list.
func routesJSON(routes []*process.Route) string {
	if len(routes) == 0 {
		return ""
	}
	type entry struct {
		SubnetAlias  string `json:"subnet_alias"`
		HardwareAddr string `json:"hardware_addr"`
	}
	out := make([]entry, 0, len(routes))
	for _, r := range routes {
		var e entry
		if r != nil {
			if r.Subnet != nil {
				e.SubnetAlias = r.Subnet.Alias
			}
			if r.Interface != nil {
				e.HardwareAddr = r.Interface.HardwareAddr
			}
		}
		// A nil element keeps its slot: route_idx is a position in this list.
		out = append(out, e)
	}
	return protoJSON(out)
}

// ---------------------------------------------------------------------------
// Small shared conversions
// ---------------------------------------------------------------------------

// protoJSON renders a sub-message we keep whole rather than in columns. Empty
// string for nothing at all, so a reader can tell "absent" from "{}".
//
// A marshal failure is logged and stored as an empty string rather than
// killing the whole batch: one unreadable sub-object must not cost the
// hundreds of rows around it.
func protoJSON(v any) string {
	if v == nil {
		return ""
	}
	b, err := json.Marshal(v)
	if err != nil {
		log.Printf("[process] cannot render %T as JSON: %v", v, err)
		return ""
	}
	// A typed nil pointer or a nil map arrives here as a non-nil `any` and
	// marshals to "null"; store nothing instead, so a reader can tell
	// "absent" from "present but empty".
	if string(b) == "null" {
		return ""
	}
	return string(b)
}

// unixSecondsPtr turns a unix-seconds field into a pointer, so "the runtime
// never reported a time" stays NULL instead of becoming 1970-01-01.
func unixSecondsPtr(sec int64) *time.Time {
	if sec <= 0 {
		return nil
	}
	t := time.Unix(sec, 0).UTC()
	return &t
}

// boolToUint8 is the bridge to ClickHouse's UInt8 booleans.
func boolToUint8(b bool) uint8 {
	if b {
		return 1
	}
	return 0
}

// resCollectorResponse assembles the reply the process-agent expects.
//
// The Status field is NOT optional in practice, however much the schema says
// it is. The agent feeds every collector reply into CheckRunner.UpdateRTStatus,
// which indexes the statuses it gathered without first checking that it got
// any — so a ResCollector with a nil Status makes the agent dereference nil
// and take its whole container down with it.
//
// This was not theory: against an earlier version of this function a real
// datadog-agent DaemonSet crash-looped three times in a local cluster, with
//
//	panic: runtime error: invalid memory address or nil pointer dereference
//	  pkg/process/runner.(*CheckRunner).UpdateRTStatus
//
// ActiveClients 0 means "nobody is watching the live process view", which is
// true — we have no such view — and keeps the agent in its normal collection
// cadence instead of switching to real-time mode. Interval echoes the standard
// process check interval, which is what the agent uses when the backend does
// not ask for something different.
//
// Built through the generated types rather than hand-packed bytes: the frame
// layout is the agent's contract, not ours, and EncodeMessage is the same code
// the agent uses to read it.
func resCollectorResponse() []byte {
	out, err := process.EncodeMessage(process.Message{
		Header: process.MessageHeader{
			Version:  process.MessageV3,
			Encoding: process.MessageEncodingProtobuf,
			Type:     process.TypeResCollector,
		},
		Body: &process.ResCollector{
			Header: &process.ResCollector_Header{Type: int32(process.TypeResCollector)},
			Status: &process.CollectorStatus{ActiveClients: 0, Interval: processCheckInterval},
		},
	})
	if err != nil {
		// Encoding a constant message cannot fail in practice. If it somehow
		// does, an empty body is still better than a panic in the handler —
		// the agent retries, and the log says why.
		log.Printf("[collector] cannot encode ResCollector: %v", err)
		return nil
	}
	return out
}

// processCheckInterval is the cadence, in seconds, the agent is told to keep
// for its process checks. It matches the agent's own default.
const processCheckInterval = 10
