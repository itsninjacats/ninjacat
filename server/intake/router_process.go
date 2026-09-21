package intake

import (
	"encoding/binary"
	"log"
	"net/http"
	"strings"
	"time"

	"github.com/DataDog/agent-payload/v5/process"
	"github.com/gin-gonic/gin"
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

	msg, ok := decodeProcessFrame(c, "collector")
	if !ok {
		return
	}

	switch body := msg.Body.(type) {
	case *process.CollectorProc:
		proc := body
		// storeProcesses reads proc and appends rows; it does not modify it.
		// What the row keeps and what it leaves behind is listed there.
		a.storeProcesses(c, proc)
		_ = proc // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case *process.CollectorRealTime:
		rt := body
		log.Printf("[collector] realtime host=%s group=%d/%d stats=%d container_stats=%d cpus=%d mem=%d %s",
			rt.HostName, rt.GroupId, rt.GroupSize, len(rt.Stats), len(rt.ContainerStats),
			rt.NumCpus, rt.TotalMemory, processAgentIdentity(c))
		_ = rt // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	default:
		log.Printf("[collector] type=%d %T - not handled", msg.Header.Type, msg.Body)
	}
}

// HandleContainer accepts the container check. One path, two message types:
// 39 (CollectorContainer, full inventory every 10 s) and 40
// (CollectorContainerRealTime, stats only every 2 s). The frame's type byte
// decides, not the body shape.
func (a *Server) HandleContainer(c *gin.Context) {
	defer c.Data(http.StatusOK, "application/x-protobuf", resCollectorResponse())

	msg, ok := decodeProcessFrame(c, "container")
	if !ok {
		return
	}

	switch msg.Header.Type {
	case process.TypeCollectorContainer:
		containers := msg.Body.(*process.CollectorContainer)
		log.Printf("[container] host=%s network=%s group=%d/%d containers=%d %s",
			containers.HostName, containers.NetworkId, containers.GroupId, containers.GroupSize,
			len(containers.Containers), processAgentIdentity(c))
		_ = containers // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	case process.TypeCollectorContainerRealTime:
		rt := msg.Body.(*process.CollectorContainerRealTime)
		log.Printf("[container] realtime host=%s group=%d/%d stats=%d cpus=%d mem=%d %s",
			rt.HostName, rt.GroupId, rt.GroupSize, len(rt.Stats), rt.NumCpus, rt.TotalMemory,
			processAgentIdentity(c))
		_ = rt // TODO(ninjacat): tables. Complete, unconverted, ready to take.
	default:
		log.Printf("[container] type=%d %T - not handled", msg.Header.Type, msg.Body)
	}
}

// HandleConnections accepts the network check (type 22). The body is the
// system-probe's connection table: endpoints, DNS, routes and eBPF telemetry.
func (a *Server) HandleConnections(c *gin.Context) {
	defer c.Data(http.StatusOK, "application/x-protobuf", resCollectorResponse())

	msg, ok := decodeProcessFrame(c, "connections")
	if !ok {
		return
	}

	conns, isConn := msg.Body.(*process.CollectorConnections)
	if !isConn {
		log.Printf("[connections] type=%d %T - not handled", msg.Header.Type, msg.Body)
		return
	}

	log.Printf("[connections] host=%s network=%s group=%d/%d connections=%d routes=%d domains=%d platform=%s/%s kernel=%s %s",
		conns.HostName, conns.NetworkId, conns.GroupId, conns.GroupSize, len(conns.Connections),
		len(conns.Routes), len(conns.Domains), conns.Platform, conns.PlatformVersion, conns.KernelVersion,
		processAgentIdentity(c))

	_ = conns // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// HandleProcDiscovery accepts the lightweight process discovery check
// (type 53): pid, command and user only, no resource usage.
func (a *Server) HandleProcDiscovery(c *gin.Context) {
	defer c.Data(http.StatusOK, "application/x-protobuf", resCollectorResponse())

	msg, ok := decodeProcessFrame(c, "discovery")
	if !ok {
		return
	}

	disc, isDisc := msg.Body.(*process.CollectorProcDiscovery)
	if !isDisc {
		log.Printf("[discovery] type=%d %T - not handled", msg.Header.Type, msg.Body)
		return
	}

	log.Printf("[discovery] host=%s group=%d/%d processes=%d %s",
		disc.HostName, disc.GroupId, disc.GroupSize, len(disc.ProcessDiscoveries),
		processAgentIdentity(c))

	_ = disc // TODO(ninjacat): tables. Complete, unconverted, ready to take.
}

// decodeProcessFrame reads the body and unwraps the 16-byte frame. The
// encoding byte (raw protobuf or zstd) is handled inside DecodeMessage; the
// HTTP layer never sees Content-Encoding on this intake.
func decodeProcessFrame(c *gin.Context, label string) (process.Message, bool) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[%s] cannot read body: %v", label, err)
		return process.Message{}, false
	}

	msg, err := process.DecodeMessage(body)
	if err != nil {
		log.Printf("[%s] process-agent frame: %v (%d bytes)", label, err, len(body))
		return process.Message{}, false
	}
	return msg, true
}

// processAgentIdentity renders the headers the process-agent stamps on every
// submit. X-DD-Request-ID is only sent by the process and connections checks.
func processAgentIdentity(c *gin.Context) string {
	s := "agent-host=" + c.GetHeader("X-Dd-Hostname") +
		" agent-version=" + c.GetHeader("X-Dd-Processagentversion") +
		" container-count=" + c.GetHeader("X-Dd-ContainerCount")
	if id := c.GetHeader("X-DD-Request-ID"); id != "" {
		s += " request-id=" + id
	}
	return s
}

// storeProcesses turns a process-agent snapshot into rows, one per process.
//
// This is a projection, not the payload. ProcessRow keeps, per process: Pid,
// State, OpenFdCount, ContainerId, Tags, Command.{Ppid,Comm,Exe,Args},
// User.Name, Memory.{Rss,Vms}, Cpu.{TotalPct,NumThreads} and CreateTime.
// Everything else stays only in proc: on the snapshot, Host, Info
// (SystemInfo: uuid, OS, CPUs, memory), Containers, NetworkId, GroupId,
// GroupSize, ContainerHostType and Hints; per process, Key, NsPid, Host,
// Command.{Cwd,Root,OnDisk,Pgroup}, User.{Uid,Gid,Euid,Egid,Suid,Sgid},
// Memory.{Swap,Shared,Text,Lib,Data,Dirty}, Cpu.{LastCpu,UserPct,SystemPct,
// Cpus,Nice,UserTime,SystemTime}, IoStat, Container, ContainerKey,
// VoluntaryCtxSwitches, InvoluntaryCtxSwitches, ByteKey, ContainerByteKey,
// Networks, ProcessContext, Language, PortInfo, ServiceDiscovery,
// InjectionState and the Zombie* fields. Args are also joined into one
// string, which cannot be split back when an argument contains a space.
func (a *Server) storeProcesses(c *gin.Context, proc *process.CollectorProc) {
	tenant := TenantFromContext(c)
	if tenant == "" || len(proc.Processes) == 0 {
		return
	}

	// The body has no sample timestamp (CreateTime is the process start,
	// not the sample), so we use arrival time. Snapshots travel within a
	// second of being taken.
	now := time.Now()

	rows := make([]storage.ProcessRow, 0, len(proc.Processes))
	for _, p := range proc.Processes {
		row := storage.ProcessRow{
			TenantID: tenant, Timestamp: now, Host: proc.HostName,
			PID: p.Pid, State: p.State.String(), OpenFDs: p.OpenFdCount,
			ContainerID: p.ContainerId, Tags: tagsToMap(p.Tags),
		}
		// Every nested struct is a pointer and may be nil — the agent omits
		// what it could not read.
		if p.Command != nil {
			row.PPID = p.Command.Ppid
			row.Comm = p.Command.Comm
			row.Exe = p.Command.Exe
			row.Cmdline = strings.Join(p.Command.Args, " ")
		}
		if p.User != nil {
			row.User = p.User.Name
		}
		if p.Memory != nil {
			row.RSS = p.Memory.Rss
			row.VMS = p.Memory.Vms
		}
		if p.Cpu != nil {
			row.CPUPct = p.Cpu.TotalPct
			row.Threads = p.Cpu.NumThreads
		}
		if p.CreateTime > 0 {
			row.CreateTime = time.UnixMilli(p.CreateTime) // milliseconds
		}
		rows = append(rows, row)
	}

	a.store(storage.ProcessesWriter, storage.WriteProcesses{Processes: rows}, len(rows))
}

// resCollectorResponse assembles the reply the process-agent expects.
//
// A 16-byte header (version, encoding, type, subscriptionID, orgID int32,
// timestamp int64 — all little-endian) followed by protobuf. The body carries
// only the nested Header.type, which older agents insist on finding.
func resCollectorResponse() []byte {
	body := []byte{
		0x0a, 0x02, // field 1 (Header), length-delimited, length 2
		0x08, typeResCollector,
	}

	out := make([]byte, messageHeaderSize+len(body))
	out[0] = messageV3
	out[1] = messageEncodingProtobuf
	out[2] = typeResCollector
	out[3] = 0 // subscriptionID
	binary.LittleEndian.PutUint32(out[4:8], 0)
	binary.LittleEndian.PutUint64(out[8:16], uint64(time.Now().Unix()))
	copy(out[messageHeaderSize:], body)
	return out
}
