package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/google/uuid"
)

// processes — process-agent snapshots.
//
// The other six tables of this intake (process_snapshots, process_stats,
// containers, container_stats, process_discoveries, connections,
// connections_payloads) live in rows_process_extra.go; this file stays the
// home of the table that shipped first.

type WriteProcesses struct{ Processes []ProcessRow }

func (m WriteProcesses) rows() []Row { return toRows(m.Processes) }

// ProcessRow is one process from a process-agent snapshot, stored in
// ninjacat.processes.
//
// Snapshots are BULKY: a few hundred processes per host every ~10s. The table
// has a shorter TTL than metrics for that reason.
//
// THE FIELD ORDER BELOW IS THE COLUMN ORDER, and the first eighteen fields are
// the ones the table shipped with. New fields are APPENDED, never inserted:
// migration 0008 adds its columns with ALTER TABLE ... ADD COLUMN, which puts
// them after the existing ones, and AppendTo has to agree.
type ProcessRow struct {
	TenantID    string
	Timestamp   time.Time
	Host        string
	PID         int32
	PPID        int32
	User        string
	Comm        string
	Exe         string
	Cmdline     string
	RSS         uint64
	VMS         uint64
	CPUPct      float32
	Threads     int32
	OpenFDs     int32
	State       string
	CreateTime  time.Time
	ContainerID string
	Tags        map[string][]string

	// SnapshotID is the frame this process arrived in; it joins to
	// process_snapshots and groups the chunks of one snapshot.
	SnapshotID uuid.UUID

	NsPID int32
	Key   uint32

	Cwd    string
	Root   string
	OnDisk uint8
	Pgroup int32

	// Args is argv as sent. Cmdline above is the same thing joined on spaces
	// and cannot be split back — an argument containing a space is
	// indistinguishable from two arguments once joined.
	Args []string

	UID  int32
	GID  int32
	EUID int32
	EGID int32
	SUID int32
	SGID int32

	MemSwap   uint64
	MemShared uint64
	MemText   uint64
	MemLib    uint64
	MemData   uint64
	MemDirty  uint64

	CPULastCPU    string
	CPUUserPct    float32
	CPUSystemPct  float32
	CPUNice       int32
	CPUUserTime   int64
	CPUSystemTime int64

	// CPUCoreNames and CPUCorePcts are PARALLEL: index i of one pairs with
	// index i of the other. A map would lose the order of a repeated field
	// and collapse two cores reporting the same name.
	CPUCoreNames []string
	CPUCorePcts  []float32

	IOReadRate       float32
	IOWriteRate      float32
	IOReadBytesRate  float32
	IOWriteBytesRate float32

	VoluntaryCtxSwitches   uint64
	InvoluntaryCtxSwitches uint64

	NetConnectionRate float32
	NetBytesRate      float32

	ProcessContext []string

	// Language and InjectionState are enum NAMES (LANGUAGE_GO,
	// INJECTION_INJECTED), never the numbers — a renumbering upstream would
	// otherwise rewrite stored history.
	Language string

	PortTCP []int32
	PortUDP []int32

	GeneratedServiceName           string
	GeneratedServiceNameSource     string
	DDServiceName                  string
	DDServiceNameSource            string
	AdditionalGeneratedNames       []string
	AdditionalGeneratedNameSources []string
	TracerRuntimeIDs               []string
	TracerServiceNames             []string
	APMInstrumentation             uint8
	// ServiceResources is the repeated oneof rendered as JSON; see the column
	// comment in 0008_process.sql.
	ServiceResources string

	InjectionState string

	ZombieChildrenCount  uint32
	ZombieNetRate        float64
	HasZombieAggregation uint8

	ContainerKey uint32

	// ByteKey and ContainerByteKey are opaque `bytes` from the wire. Stored as
	// String, which in ClickHouse is a byte string, so they survive verbatim.
	ByteKey          string
	ContainerByteKey string

	// HasCreateTime carries the absent/zero distinction CreateTime cannot:
	// the column is a plain DateTime and making it Nullable would rewrite
	// every existing part. 0 means the agent sent no start time, and
	// CreateTime is meaningless rather than 1970.
	HasCreateTime uint8

	// ProcessHost is Process.host as JSON — a backend-resolved field a real
	// agent leaves nil, so eight columns would be empty in every row.
	ProcessHost string

	NetworkID         string
	GroupID           int32
	GroupSize         int32
	ContainerHostType string
	AgentVersion      string
	RequestID         string
	HostUUID          string
	OSName            string
	OSPlatform        string
	OSFamily          string
	OSVersion         string
	KernelVersion     string
}

func (r ProcessRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host,
		r.PID, r.PPID, r.User, r.Comm, r.Exe, r.Cmdline,
		r.RSS, r.VMS, r.CPUPct, r.Threads, r.OpenFDs,
		r.State, r.CreateTime, r.ContainerID, orEmpty(r.Tags),

		r.SnapshotID, r.NsPID, r.Key,
		r.Cwd, r.Root, r.OnDisk, r.Pgroup, orEmptySlice(r.Args),
		r.UID, r.GID, r.EUID, r.EGID, r.SUID, r.SGID,
		r.MemSwap, r.MemShared, r.MemText, r.MemLib, r.MemData, r.MemDirty,
		r.CPULastCPU, r.CPUUserPct, r.CPUSystemPct, r.CPUNice,
		r.CPUUserTime, r.CPUSystemTime,
		orEmptySlice(r.CPUCoreNames), orEmptySlice(r.CPUCorePcts),
		r.IOReadRate, r.IOWriteRate, r.IOReadBytesRate, r.IOWriteBytesRate,
		r.VoluntaryCtxSwitches, r.InvoluntaryCtxSwitches,
		r.NetConnectionRate, r.NetBytesRate,
		orEmptySlice(r.ProcessContext), r.Language,
		orEmptySlice(r.PortTCP), orEmptySlice(r.PortUDP),
		r.GeneratedServiceName, r.GeneratedServiceNameSource,
		r.DDServiceName, r.DDServiceNameSource,
		orEmptySlice(r.AdditionalGeneratedNames), orEmptySlice(r.AdditionalGeneratedNameSources),
		orEmptySlice(r.TracerRuntimeIDs), orEmptySlice(r.TracerServiceNames),
		r.APMInstrumentation, r.ServiceResources,
		r.InjectionState,
		r.ZombieChildrenCount, r.ZombieNetRate, r.HasZombieAggregation,
		r.ContainerKey, r.ByteKey, r.ContainerByteKey,
		r.HasCreateTime, r.ProcessHost,
		r.NetworkID, r.GroupID, r.GroupSize, r.ContainerHostType,
		r.AgentVersion, r.RequestID,
		r.HostUUID, r.OSName, r.OSPlatform, r.OSFamily, r.OSVersion, r.KernelVersion)
}

func init() {
	registerWriter(ProcessesWriter, WriterConfig{
		Name: "processes",
		Insert: `INSERT INTO processes
			(tenant_id, timestamp, host, pid, ppid, user, comm, exe, cmdline,
			 rss, vms, cpu_pct, threads, open_fds, state, create_time, container_id, tags,
			 snapshot_id, ns_pid, key,
			 cwd, root, on_disk, pgroup, args,
			 uid, gid, euid, egid, suid, sgid,
			 mem_swap, mem_shared, mem_text, mem_lib, mem_data, mem_dirty,
			 cpu_last_cpu, cpu_user_pct, cpu_system_pct, cpu_nice,
			 cpu_user_time, cpu_system_time, cpu_core_names, cpu_core_pcts,
			 io_read_rate, io_write_rate, io_read_bytes_rate, io_write_bytes_rate,
			 voluntary_ctx_switches, involuntary_ctx_switches,
			 net_connection_rate, net_bytes_rate,
			 process_context, language, port_tcp, port_udp,
			 generated_service_name, generated_service_name_source,
			 dd_service_name, dd_service_name_source,
			 additional_generated_names, additional_generated_name_sources,
			 tracer_runtime_ids, tracer_service_names,
			 apm_instrumentation, service_resources, injection_state,
			 zombie_children_count, zombie_net_rate, has_zombie_aggregation,
			 container_key, byte_key, container_byte_key,
			 has_create_time, process_host,
			 network_id, group_id, group_size, container_host_type,
			 agent_version, request_id,
			 host_uuid, os_name, os_platform, os_family, os_version, kernel_version)`,
		// A few hundred rows per host per snapshot, every ~10s. Large batches,
		// moderate interval.
		MaxRows: 10000, FlushInterval: 3 * time.Second,
	}, ProcessRow{})
}
