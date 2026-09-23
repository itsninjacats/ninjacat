package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/google/uuid"
)

// The rest of the process intake: everything the process-agent sends that is
// not a Process.
//
// One HTTP frame carries one of six message types, and each of them has a
// table here. The frame's own envelope (host, network, group chunking, Host,
// SystemInfo, the agent's headers) goes to process_snapshots once per frame;
// the item tables carry SnapshotID back to it AND a denormalised copy of the
// few envelope fields every query needs, so the common question does not
// become a join. Migration 0008_process.sql holds the reasoning per column.
//
// Split from rows_process.go rather than added to it because `processes`
// predates the registry and its row is the one with an original EDF identity:
// the types below are new and register themselves through registerTypes,
// which appends after register.go's frozen list.

const (
	ProcessSnapshotsWriter    = gen.Atom("storage_process_snapshots")
	ProcessStatsWriter        = gen.Atom("storage_process_stats")
	ContainersWriter          = gen.Atom("storage_containers")
	ContainerStatsWriter      = gen.Atom("storage_container_stats")
	ProcessDiscoveriesWriter  = gen.Atom("storage_process_discoveries")
	ConnectionsWriter         = gen.Atom("storage_connections")
	ConnectionsPayloadsWriter = gen.Atom("storage_connections_payloads")
)

type WriteProcessSnapshots struct{ Snapshots []ProcessSnapshotRow }
type WriteProcessStats struct{ Stats []ProcessStatRow }
type WriteContainers struct{ Containers []ContainerRow }
type WriteContainerStats struct{ Stats []ContainerStatRow }
type WriteProcessDiscoveries struct{ Discoveries []ProcessDiscoveryRow }
type WriteConnections struct{ Connections []ConnectionRow }
type WriteConnectionsPayloads struct{ Payloads []ConnectionsPayloadRow }

func (m WriteProcessSnapshots) rows() []Row    { return toRows(m.Snapshots) }
func (m WriteProcessStats) rows() []Row        { return toRows(m.Stats) }
func (m WriteContainers) rows() []Row          { return toRows(m.Containers) }
func (m WriteContainerStats) rows() []Row      { return toRows(m.Stats) }
func (m WriteProcessDiscoveries) rows() []Row  { return toRows(m.Discoveries) }
func (m WriteConnections) rows() []Row         { return toRows(m.Connections) }
func (m WriteConnectionsPayloads) rows() []Row { return toRows(m.Payloads) }

// ---------------------------------------------------------------------------
// process_snapshots
// ---------------------------------------------------------------------------

// ProcessSnapshotRow is one received frame, whatever its type.
//
// HeaderTimestamp stays an int64: the 16-byte frame carries a timestamp field,
// the agent leaves it at zero, and its unit is documented nowhere — turning it
// into a time.Time would be inventing a value.
type ProcessSnapshotRow struct {
	TenantID   string
	ReceivedAt time.Time
	SnapshotID uuid.UUID

	MessageType     uint8
	MessageTypeName string
	Path            string

	HeaderVersion        uint8
	HeaderEncoding       uint8
	HeaderSubscriptionID uint8
	HeaderOrgID          int32
	HeaderTimestamp      int64
	FrameBytes           uint64

	HostName          string
	NetworkID         string
	GroupID           int32
	GroupSize         int32
	ContainerHostType string

	HostID          int64
	HostOrgID       int32
	HostDisplayName string
	HostAllTags     map[string][]string
	HostNumCPUs     int32
	HostTotalMemory int64
	HostTagIndex    int32
	// Nullable: "never recomputed" and "recomputed at the epoch" are
	// different statements about the backend's tag cache.
	HostTagsModified *time.Time

	SysUUID        string
	OSName         string
	OSPlatform     string
	OSFamily       string
	OSVersion      string
	KernelVersion  string
	SysTotalMemory int64

	// Nine parallel arrays, one entry per CPUInfo, in the order sent.
	CPUNumbers     []int32
	CPUVendors     []string
	CPUFamilies    []string
	CPUModels      []string
	CPUPhysicalIDs []string
	CPUCoreIDs     []string
	CPUCores       []int32
	CPUMhz         []int64
	CPUCacheSizes  []int32

	// Only CollectorProc has hints, and "no hints" differs from "mask 0".
	HintMask *int32

	RTHostID      int64
	RTOrgID       int32
	RTNumCPUs     int32
	RTTotalMemory int64

	ProcessCount       uint32
	ProcessStatCount   uint32
	ContainerCount     uint32
	ContainerStatCount uint32
	DiscoveryCount     uint32
	ConnectionCount    uint32

	AgentHostname       string
	AgentVersion        string
	AgentContainerCount string
	RequestID           string
}

func (r ProcessSnapshotRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.SnapshotID,
		r.MessageType, r.MessageTypeName, r.Path,
		r.HeaderVersion, r.HeaderEncoding, r.HeaderSubscriptionID,
		r.HeaderOrgID, r.HeaderTimestamp, r.FrameBytes,
		r.HostName, r.NetworkID, r.GroupID, r.GroupSize, r.ContainerHostType,
		r.HostID, r.HostOrgID, r.HostDisplayName, orEmpty(r.HostAllTags),
		r.HostNumCPUs, r.HostTotalMemory, r.HostTagIndex, r.HostTagsModified,
		r.SysUUID, r.OSName, r.OSPlatform, r.OSFamily, r.OSVersion,
		r.KernelVersion, r.SysTotalMemory,
		orEmptySlice(r.CPUNumbers), orEmptySlice(r.CPUVendors),
		orEmptySlice(r.CPUFamilies), orEmptySlice(r.CPUModels),
		orEmptySlice(r.CPUPhysicalIDs), orEmptySlice(r.CPUCoreIDs),
		orEmptySlice(r.CPUCores), orEmptySlice(r.CPUMhz), orEmptySlice(r.CPUCacheSizes),
		r.HintMask,
		r.RTHostID, r.RTOrgID, r.RTNumCPUs, r.RTTotalMemory,
		r.ProcessCount, r.ProcessStatCount, r.ContainerCount,
		r.ContainerStatCount, r.DiscoveryCount, r.ConnectionCount,
		r.AgentHostname, r.AgentVersion, r.AgentContainerCount, r.RequestID)
}

// ---------------------------------------------------------------------------
// process_stats
// ---------------------------------------------------------------------------

// ProcessStatRow is one ProcessStat from the 2-second realtime stream.
//
// Nice and Threads are ProcessStat's own top-level fields and are NOT the same
// as CPUNice / CPUNumThreads below, which come from the nested CPUStat; the
// agent does not always fill both.
type ProcessStatRow struct {
	TenantID   string
	Timestamp  time.Time
	Host       string
	SnapshotID uuid.UUID

	PID           int32
	Key           uint32
	CreateTime    time.Time
	HasCreateTime uint8

	Nice    int32
	Threads int32
	OpenFDs int32

	MemRSS    uint64
	MemVMS    uint64
	MemSwap   uint64
	MemShared uint64
	MemText   uint64
	MemLib    uint64
	MemData   uint64
	MemDirty  uint64

	CPULastCPU    string
	CPUTotalPct   float32
	CPUUserPct    float32
	CPUSystemPct  float32
	CPUNumThreads int32
	CPUNice       int32
	CPUUserTime   int64
	CPUSystemTime int64
	CPUCoreNames  []string
	CPUCorePcts   []float32

	IOReadRate       float32
	IOWriteRate      float32
	IOReadBytesRate  float32
	IOWriteBytesRate float32

	NetConnectionRate float32
	NetBytesRate      float32

	VoluntaryCtxSwitches   uint64
	InvoluntaryCtxSwitches uint64

	ContainerID    string
	ContainerState string
	ProcessState   string

	// Deprecated upstream, stored anyway: older agents send only these.
	ContainerHealth     string
	ContainerRbps       float32
	ContainerWbps       float32
	ContainerKey        uint32
	ContainerNetRcvdPs  float32
	ContainerNetSentPs  float32
	ContainerNetRcvdBps float32
	ContainerNetSentBps float32

	ByteKey          string
	ContainerByteKey string

	GroupID           int32
	GroupSize         int32
	ContainerHostType string
	RTHostID          int64
	RTOrgID           int32
	RTNumCPUs         int32
	RTTotalMemory     int64
	AgentVersion      string
	RequestID         string
}

func (r ProcessStatRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.SnapshotID,
		r.PID, r.Key, r.CreateTime, r.HasCreateTime,
		r.Nice, r.Threads, r.OpenFDs,
		r.MemRSS, r.MemVMS, r.MemSwap, r.MemShared,
		r.MemText, r.MemLib, r.MemData, r.MemDirty,
		r.CPULastCPU, r.CPUTotalPct, r.CPUUserPct, r.CPUSystemPct,
		r.CPUNumThreads, r.CPUNice, r.CPUUserTime, r.CPUSystemTime,
		orEmptySlice(r.CPUCoreNames), orEmptySlice(r.CPUCorePcts),
		r.IOReadRate, r.IOWriteRate, r.IOReadBytesRate, r.IOWriteBytesRate,
		r.NetConnectionRate, r.NetBytesRate,
		r.VoluntaryCtxSwitches, r.InvoluntaryCtxSwitches,
		r.ContainerID, r.ContainerState, r.ProcessState,
		r.ContainerHealth, r.ContainerRbps, r.ContainerWbps, r.ContainerKey,
		r.ContainerNetRcvdPs, r.ContainerNetSentPs,
		r.ContainerNetRcvdBps, r.ContainerNetSentBps,
		r.ByteKey, r.ContainerByteKey,
		r.GroupID, r.GroupSize, r.ContainerHostType,
		r.RTHostID, r.RTOrgID, r.RTNumCPUs, r.RTTotalMemory,
		r.AgentVersion, r.RequestID)
}

// ---------------------------------------------------------------------------
// containers
// ---------------------------------------------------------------------------

// ContainerRow is one Container struct. Source says which message it hung off
// — collector_proc, collector_container or process — because the same struct
// arrives all three ways and the provenance changes what is populated.
type ContainerRow struct {
	TenantID   string
	Timestamp  time.Time
	Host       string
	SnapshotID uuid.UUID

	Source string
	// PID is the owning process, for Source == "process". Zero otherwise.
	PID int32

	Type       string
	ID         string
	Name       string
	Image      string
	RepoDigest string

	CPULimit      float32
	MemoryLimit   uint64
	CPURequest    float32
	MemoryRequest uint64

	State  string
	Health string

	// Unix seconds on the wire; nil when the runtime reported none, so a
	// container never appears to have started at the epoch.
	Created *time.Time
	Started *time.Time

	Rbps       float32
	Wbps       float32
	NetRcvdPs  float32
	NetSentPs  float32
	NetRcvdBps float32
	NetSentBps float32

	UserPct    float32
	SystemPct  float32
	TotalPct   float32
	CPUUsageNs float32

	MemRSS       uint64
	MemCache     uint64
	MemUsage     uint64
	MemAccounted uint64

	ThreadCount uint64
	ThreadLimit uint64

	Key     uint32
	ByteKey string

	// Three parallel arrays, one entry per ContainerAddr, order as sent.
	AddrIPs       []string
	AddrPorts     []int32
	AddrProtocols []string

	Tags map[string][]string

	// Container.host as JSON; a backend-resolved field, nil from a real agent.
	HostInfo string

	NetworkID         string
	GroupID           int32
	GroupSize         int32
	ContainerHostType string
	AgentVersion      string
	RequestID         string
}

func (r ContainerRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.SnapshotID,
		r.Source, r.PID,
		r.Type, r.ID, r.Name, r.Image, r.RepoDigest,
		r.CPULimit, r.MemoryLimit, r.CPURequest, r.MemoryRequest,
		r.State, r.Health, r.Created, r.Started,
		r.Rbps, r.Wbps, r.NetRcvdPs, r.NetSentPs, r.NetRcvdBps, r.NetSentBps,
		r.UserPct, r.SystemPct, r.TotalPct, r.CPUUsageNs,
		r.MemRSS, r.MemCache, r.MemUsage, r.MemAccounted,
		r.ThreadCount, r.ThreadLimit,
		r.Key, r.ByteKey,
		orEmptySlice(r.AddrIPs), orEmptySlice(r.AddrPorts), orEmptySlice(r.AddrProtocols),
		orEmpty(r.Tags), r.HostInfo,
		r.NetworkID, r.GroupID, r.GroupSize, r.ContainerHostType,
		r.AgentVersion, r.RequestID)
}

// ---------------------------------------------------------------------------
// container_stats
// ---------------------------------------------------------------------------

// ContainerStatRow is one ContainerStat. Source is collector_realtime or
// collector_container_realtime — the two messages carry identical fields.
type ContainerStatRow struct {
	TenantID   string
	Timestamp  time.Time
	Host       string
	SnapshotID uuid.UUID
	Source     string

	ID string

	UserPct    float32
	SystemPct  float32
	TotalPct   float32
	CPULimit   float32
	CPURequest float32
	CPUUsageNs float32

	MemRSS        uint64
	MemCache      uint64
	MemLimit      uint64
	MemUsage      uint64
	MemAccounted  uint64
	MemoryRequest uint64

	Rbps       float32
	Wbps       float32
	NetRcvdPs  float32
	NetSentPs  float32
	NetRcvdBps float32
	NetSentBps float32

	State  string
	Health string

	Key         uint32
	Started     *time.Time
	ByteKey     string
	ThreadCount uint64
	ThreadLimit uint64

	GroupID           int32
	GroupSize         int32
	ContainerHostType string
	RTHostID          int64
	RTOrgID           int32
	RTNumCPUs         int32
	RTTotalMemory     int64
	AgentVersion      string
	RequestID         string
}

func (r ContainerStatRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.SnapshotID, r.Source,
		r.ID,
		r.UserPct, r.SystemPct, r.TotalPct, r.CPULimit, r.CPURequest, r.CPUUsageNs,
		r.MemRSS, r.MemCache, r.MemLimit, r.MemUsage, r.MemAccounted, r.MemoryRequest,
		r.Rbps, r.Wbps, r.NetRcvdPs, r.NetSentPs, r.NetRcvdBps, r.NetSentBps,
		r.State, r.Health,
		r.Key, r.Started, r.ByteKey, r.ThreadCount, r.ThreadLimit,
		r.GroupID, r.GroupSize, r.ContainerHostType,
		r.RTHostID, r.RTOrgID, r.RTNumCPUs, r.RTTotalMemory,
		r.AgentVersion, r.RequestID)
}

// ---------------------------------------------------------------------------
// process_discoveries
// ---------------------------------------------------------------------------

// ProcessDiscoveryRow is one ProcessDiscovery: pid, command and user, with no
// resource usage. For hosts that run only the discovery check this is the sole
// process-level record that ever arrives.
type ProcessDiscoveryRow struct {
	TenantID   string
	Timestamp  time.Time
	Host       string
	SnapshotID uuid.UUID

	PID           int32
	NsPID         int32
	CreateTime    time.Time
	HasCreateTime uint8
	ByteKey       string

	Comm    string
	Exe     string
	Cmdline string
	Args    []string
	Cwd     string
	Root    string
	OnDisk  uint8
	PPID    int32
	Pgroup  int32

	User string
	UID  int32
	GID  int32
	EUID int32
	EGID int32
	SUID int32
	SGID int32

	HostInfo string

	GroupID      int32
	GroupSize    int32
	AgentVersion string
	RequestID    string
}

func (r ProcessDiscoveryRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.SnapshotID,
		r.PID, r.NsPID, r.CreateTime, r.HasCreateTime, r.ByteKey,
		r.Comm, r.Exe, r.Cmdline, orEmptySlice(r.Args),
		r.Cwd, r.Root, r.OnDisk, r.PPID, r.Pgroup,
		r.User, r.UID, r.GID, r.EUID, r.EGID, r.SUID, r.SGID,
		r.HostInfo,
		r.GroupID, r.GroupSize, r.AgentVersion, r.RequestID)
}

// ---------------------------------------------------------------------------
// connections_payloads
// ---------------------------------------------------------------------------

// ConnectionsPayloadRow is the per-frame half of CollectorConnections: the
// forty-odd fields that describe the sender and its eBPF machinery rather than
// any one connection.
//
// PayloadID is the same uuid as the frame's ProcessSnapshotRow.SnapshotID and
// as ConnectionRow.PayloadID.
type ConnectionsPayloadRow struct {
	TenantID   string
	ReceivedAt time.Time
	PayloadID  uuid.UUID

	HostName          string
	NetworkID         string
	GroupID           int32
	GroupSize         int32
	ContainerHostType string
	ConnectionCount   uint32

	Architecture    string
	KernelVersion   string
	Platform        string
	PlatformVersion string

	ResolvedResources string
	ContainerForPID   map[int32]string

	// The packed tag buffers, verbatim. Connection rows carry their tags
	// already resolved, but the encoding is versioned and one we cannot read
	// yet must still land somewhere.
	EncodedTags            string
	EncodedConnectionsTags string

	HostTagsIndex int32
	HostTags      map[string][]string

	ConnTelemetry    map[string]int64
	ConnTelemetryMap map[string]int64

	CompilationTelemetry    string
	KernelHeaderFetchResult string
	CORETelemetry           map[string]string
	PrebuiltEBPFAssets      []string

	Routes             string
	RouteMetadata      string
	AgentConfiguration string

	EncodedDNS            string
	Domains               []string
	EncodedDomainDatabase string
	EncodedDNSLookups     string
	// DNSNames is what the module's own reader made of whichever DNS encoding
	// arrived, so the common case needs no decoder at read time.
	DNSNames []string

	ResolvedHostsByName string
	ResolvedPublicIPs   string

	EcsTask     string
	ResolvConfs []string

	AgentHostname       string
	AgentVersion        string
	AgentContainerCount string
	RequestID           string
}

func (r ConnectionsPayloadRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.PayloadID,
		r.HostName, r.NetworkID, r.GroupID, r.GroupSize,
		r.ContainerHostType, r.ConnectionCount,
		r.Architecture, r.KernelVersion, r.Platform, r.PlatformVersion,
		r.ResolvedResources, orEmpty(r.ContainerForPID),
		r.EncodedTags, r.EncodedConnectionsTags,
		r.HostTagsIndex, orEmpty(r.HostTags),
		orEmpty(r.ConnTelemetry), orEmpty(r.ConnTelemetryMap),
		r.CompilationTelemetry, r.KernelHeaderFetchResult,
		orEmpty(r.CORETelemetry), orEmptySlice(r.PrebuiltEBPFAssets),
		r.Routes, r.RouteMetadata, r.AgentConfiguration,
		r.EncodedDNS, orEmptySlice(r.Domains),
		r.EncodedDomainDatabase, r.EncodedDNSLookups, orEmptySlice(r.DNSNames),
		r.ResolvedHostsByName, r.ResolvedPublicIPs,
		r.EcsTask, orEmptySlice(r.ResolvConfs),
		r.AgentHostname, r.AgentVersion, r.AgentContainerCount, r.RequestID)
}

// ---------------------------------------------------------------------------
// connections
// ---------------------------------------------------------------------------

// ConnectionRow is one Connection from the system-probe's connection table.
//
// The four *Aggregations fields are separately serialised protobufs from a
// package this repo does not vendor. They are kept as the bytes they are:
// dropping them would throw away the whole content of Universal Service
// Monitoring, and a decoder can be written against stored bytes later but not
// against bytes we discarded.
type ConnectionRow struct {
	TenantID  string
	Timestamp time.Time
	Host      string
	PayloadID uuid.UUID

	PID int32

	LaddrIP          string
	LaddrPort        int32
	LaddrContainerID string
	LaddrHostName    string
	RaddrIP          string
	RaddrPort        int32
	RaddrContainerID string
	RaddrHostName    string

	Family               string
	Type                 string
	Direction            string
	IsLocalPortEphemeral string

	LastBytesSent       uint64
	LastBytesReceived   uint64
	LastPacketsSent     uint64
	LastPacketsReceived uint64
	LastRetransmits     uint32

	// Order is the stack: [protocolTLS, protocolHTTP2] is not the reverse.
	ProtocolStack []string

	NetNS           uint32
	RemoteNetworkID string

	// The conntrack NAT entry, absent for every untranslated connection —
	// which is most of them, hence pointers rather than 0.0.0.0:0.
	IPTranslationReplSrcIP   *string
	IPTranslationReplDstIP   *string
	IPTranslationReplSrcPort *int32
	IPTranslationReplDstPort *int32

	RTT       uint32
	RTTVar    uint32
	IntraHost uint8

	DNSSuccessfulResponses uint32
	DNSFailedResponses     uint32
	DNSTimeouts            uint32
	DNSSuccessLatencySum   uint64
	DNSFailureLatencySum   uint64
	DNSCountByRcode        map[uint32]uint32

	DNSStatsByDomain                  string
	DNSStatsByDomainByQueryType       string
	DNSStatsByDomainOffsetByQueryType string

	LastTCPEstablished uint32
	LastTCPClosed      uint32

	RouteIdx       int32
	RouteTargetIdx int32
	ResolvConfIdx  int32

	HTTPAggregations        string
	HTTP2Aggregations       string
	DataStreamsAggregations string
	DatabaseAggregations    string

	TagsIndices  []uint32
	TagsIdx      int32
	TagsChecksum uint32
	Tags         map[string][]string

	LocalContainerTagsIndex int32
	LocalContainerTags      map[string][]string
	RemoteServiceTagsIdx    int32
	RemoteServiceTags       map[string][]string

	StateIndex           uint32
	TCPFailuresByErrCode map[uint32]uint32
	RemoteEcsTask        string
	SystemProbeConn      uint8

	LastTCPRtoCount      uint32
	LastTCPRecoveryCount uint32
	LastTCPReordSeen     uint32
	LastTCPRcvOooPack    uint32
	LastTCPDeliveredCe   uint32
	LastTCPProbe0Count   uint32
	TCPEcnNegotiated     uint8

	AgentVersion string
	RequestID    string
}

func (r ConnectionRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.PayloadID, r.PID,
		r.LaddrIP, r.LaddrPort, r.LaddrContainerID, r.LaddrHostName,
		r.RaddrIP, r.RaddrPort, r.RaddrContainerID, r.RaddrHostName,
		r.Family, r.Type, r.Direction, r.IsLocalPortEphemeral,
		r.LastBytesSent, r.LastBytesReceived,
		r.LastPacketsSent, r.LastPacketsReceived, r.LastRetransmits,
		orEmptySlice(r.ProtocolStack),
		r.NetNS, r.RemoteNetworkID,
		r.IPTranslationReplSrcIP, r.IPTranslationReplDstIP,
		r.IPTranslationReplSrcPort, r.IPTranslationReplDstPort,
		r.RTT, r.RTTVar, r.IntraHost,
		r.DNSSuccessfulResponses, r.DNSFailedResponses, r.DNSTimeouts,
		r.DNSSuccessLatencySum, r.DNSFailureLatencySum, orEmpty(r.DNSCountByRcode),
		r.DNSStatsByDomain, r.DNSStatsByDomainByQueryType,
		r.DNSStatsByDomainOffsetByQueryType,
		r.LastTCPEstablished, r.LastTCPClosed,
		r.RouteIdx, r.RouteTargetIdx, r.ResolvConfIdx,
		r.HTTPAggregations, r.HTTP2Aggregations,
		r.DataStreamsAggregations, r.DatabaseAggregations,
		orEmptySlice(r.TagsIndices), r.TagsIdx, r.TagsChecksum, orEmpty(r.Tags),
		r.LocalContainerTagsIndex, orEmpty(r.LocalContainerTags),
		r.RemoteServiceTagsIdx, orEmpty(r.RemoteServiceTags),
		r.StateIndex, orEmpty(r.TCPFailuresByErrCode),
		r.RemoteEcsTask, r.SystemProbeConn,
		r.LastTCPRtoCount, r.LastTCPRecoveryCount, r.LastTCPReordSeen,
		r.LastTCPRcvOooPack, r.LastTCPDeliveredCe, r.LastTCPProbe0Count,
		r.TCPEcnNegotiated,
		r.AgentVersion, r.RequestID)
}

func init() {
	registerWriter(ProcessSnapshotsWriter, WriterConfig{
		Name: "process_snapshots",
		Insert: `INSERT INTO process_snapshots
			(tenant_id, received_at, snapshot_id,
			 message_type, message_type_name, path,
			 header_version, header_encoding, header_subscription_id,
			 header_org_id, header_timestamp, frame_bytes,
			 host_name, network_id, group_id, group_size, container_host_type,
			 host_id, host_org_id, host_display_name, host_all_tags,
			 host_num_cpus, host_total_memory, host_tag_index, host_tags_modified,
			 sys_uuid, os_name, os_platform, os_family, os_version,
			 kernel_version, sys_total_memory,
			 cpu_numbers, cpu_vendors, cpu_families, cpu_models,
			 cpu_physical_ids, cpu_core_ids, cpu_cores, cpu_mhz, cpu_cache_sizes,
			 hint_mask,
			 rt_host_id, rt_org_id, rt_num_cpus, rt_total_memory,
			 process_count, process_stat_count, container_count,
			 container_stat_count, discovery_count, connection_count,
			 agent_hostname, agent_version, agent_container_count, request_id)`,
		// One row per frame: at most six frames per host per ten seconds, and
		// each row is wide but tiny. Quiet-summary class, like k8s_cluster —
		// the timer does the flushing.
		MaxRows: 500, FlushInterval: 10 * time.Second,
		BufferLimit: 20_000, MaxInFlight: 2,
	}, ProcessSnapshotRow{})

	registerWriter(ProcessStatsWriter, WriterConfig{
		Name: "process_stats",
		Insert: `INSERT INTO process_stats
			(tenant_id, timestamp, host, snapshot_id,
			 pid, key, create_time, has_create_time,
			 nice, threads, open_fds,
			 mem_rss, mem_vms, mem_swap, mem_shared,
			 mem_text, mem_lib, mem_data, mem_dirty,
			 cpu_last_cpu, cpu_total_pct, cpu_user_pct, cpu_system_pct,
			 cpu_num_threads, cpu_nice, cpu_user_time, cpu_system_time,
			 cpu_core_names, cpu_core_pcts,
			 io_read_rate, io_write_rate, io_read_bytes_rate, io_write_bytes_rate,
			 net_connection_rate, net_bytes_rate,
			 voluntary_ctx_switches, involuntary_ctx_switches,
			 container_id, container_state, process_state,
			 container_health, container_rbps, container_wbps, container_key,
			 container_net_rcvd_ps, container_net_sent_ps,
			 container_net_rcvd_bps, container_net_sent_bps,
			 byte_key, container_byte_key,
			 group_id, group_size, container_host_type,
			 rt_host_id, rt_org_id, rt_num_cpus, rt_total_memory,
			 agent_version, request_id)`,
		// Same shape as processes but five times the cadence (2s, not 10s):
		// the bulky-periodic-snapshot numbers, unchanged.
		MaxRows: 10000, FlushInterval: 3 * time.Second,
	}, ProcessStatRow{})

	registerWriter(ContainersWriter, WriterConfig{
		Name: "containers",
		Insert: `INSERT INTO containers
			(tenant_id, timestamp, host, snapshot_id, source, pid,
			 type, id, name, image, repo_digest,
			 cpu_limit, memory_limit, cpu_request, memory_request,
			 state, health, created, started,
			 rbps, wbps, net_rcvd_ps, net_sent_ps, net_rcvd_bps, net_sent_bps,
			 user_pct, system_pct, total_pct, cpu_usage_ns,
			 mem_rss, mem_cache, mem_usage, mem_accounted,
			 thread_count, thread_limit, key, byte_key,
			 addr_ips, addr_ports, addr_protocols, tags, host_info,
			 network_id, group_id, group_size, container_host_type,
			 agent_version, request_id)`,
		// Tens to hundreds of rows per host per pass, with tag maps and
		// address arrays on each: the bursty-per-pass class k8s_resources
		// uses, and for the same reason — the rows carry real memory.
		MaxRows: 2000, FlushInterval: 5 * time.Second,
		BufferLimit: 50_000, MaxInFlight: 2,
	}, ContainerRow{})

	registerWriter(ContainerStatsWriter, WriterConfig{
		Name: "container_stats",
		Insert: `INSERT INTO container_stats
			(tenant_id, timestamp, host, snapshot_id, source, id,
			 user_pct, system_pct, total_pct, cpu_limit, cpu_request, cpu_usage_ns,
			 mem_rss, mem_cache, mem_limit, mem_usage, mem_accounted, memory_request,
			 rbps, wbps, net_rcvd_ps, net_sent_ps, net_rcvd_bps, net_sent_bps,
			 state, health, key, started, byte_key, thread_count, thread_limit,
			 group_id, group_size, container_host_type,
			 rt_host_id, rt_org_id, rt_num_cpus, rt_total_memory,
			 agent_version, request_id)`,
		// Narrow numeric rows on the 2-second stream: metric-like, so the
		// metrics defaults with a slightly larger batch.
		MaxRows: 10000, FlushInterval: 3 * time.Second,
	}, ContainerStatRow{})

	registerWriter(ProcessDiscoveriesWriter, WriterConfig{
		Name: "process_discoveries",
		Insert: `INSERT INTO process_discoveries
			(tenant_id, timestamp, host, snapshot_id,
			 pid, ns_pid, create_time, has_create_time, byte_key,
			 comm, exe, cmdline, args, cwd, root, on_disk, ppid, pgroup,
			 user, uid, gid, euid, egid, suid, sgid, host_info,
			 group_id, group_size, agent_version, request_id)`,
		// A full process list per pass, like processes, but with no stats on
		// the rows — same batching, the rows are simply smaller.
		MaxRows: 10000, FlushInterval: 3 * time.Second,
	}, ProcessDiscoveryRow{})

	registerWriter(ConnectionsWriter, WriterConfig{
		Name: "connections",
		Insert: `INSERT INTO connections
			(tenant_id, timestamp, host, payload_id, pid,
			 laddr_ip, laddr_port, laddr_container_id, laddr_host_name,
			 raddr_ip, raddr_port, raddr_container_id, raddr_host_name,
			 family, type, direction, is_local_port_ephemeral,
			 last_bytes_sent, last_bytes_received,
			 last_packets_sent, last_packets_received, last_retransmits,
			 protocol_stack, net_ns, remote_network_id,
			 ip_translation_repl_src_ip, ip_translation_repl_dst_ip,
			 ip_translation_repl_src_port, ip_translation_repl_dst_port,
			 rtt, rtt_var, intra_host,
			 dns_successful_responses, dns_failed_responses, dns_timeouts,
			 dns_success_latency_sum, dns_failure_latency_sum, dns_count_by_rcode,
			 dns_stats_by_domain, dns_stats_by_domain_by_query_type,
			 dns_stats_by_domain_offset_by_query_type,
			 last_tcp_established, last_tcp_closed,
			 route_idx, route_target_idx, resolv_conf_idx,
			 http_aggregations, http2_aggregations,
			 data_streams_aggregations, database_aggregations,
			 tags_indices, tags_idx, tags_checksum, tags,
			 local_container_tags_index, local_container_tags,
			 remote_service_tags_idx, remote_service_tags,
			 state_index, tcp_failures_by_err_code,
			 remote_ecs_task, system_probe_conn,
			 last_tcp_rto_count, last_tcp_recovery_count, last_tcp_reord_seen,
			 last_tcp_rcv_ooo_pack, last_tcp_delivered_ce, last_tcp_probe0_count,
			 tcp_ecn_negotiated, agent_version, request_id)`,
		// The highest-volume table of this intake: a busy host reports
		// thousands of connections per frame. The logs numbers, for the same
		// reason they were chosen there — bigger batches, bigger margin.
		MaxRows: 20000, FlushInterval: 1 * time.Second,
		BufferLimit: 500_000,
	}, ConnectionRow{})

	registerWriter(ConnectionsPayloadsWriter, WriterConfig{
		Name: "connections_payloads",
		Insert: `INSERT INTO connections_payloads
			(tenant_id, received_at, payload_id,
			 host_name, network_id, group_id, group_size,
			 container_host_type, connection_count,
			 architecture, kernel_version, platform, platform_version,
			 resolved_resources, container_for_pid,
			 encoded_tags, encoded_connections_tags,
			 host_tags_index, host_tags,
			 conn_telemetry, conn_telemetry_map,
			 compilation_telemetry, kernel_header_fetch_result,
			 core_telemetry, prebuilt_ebpf_assets,
			 routes, route_metadata, agent_configuration,
			 encoded_dns, domains, encoded_domain_database, encoded_dns_lookups,
			 dns_names, resolved_hosts_by_name, resolved_public_ips,
			 ecs_task, resolv_confs,
			 agent_hostname, agent_version, agent_container_count, request_id)`,
		// One row per frame, but each one carries the packed tag and DNS
		// buffers — kilobytes to megabytes. The bulky-document class
		// k8s_manifests and raw_payloads use: tight ceilings, one flush in
		// flight.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, ConnectionsPayloadRow{})

	registerTypes(
		WriteProcessSnapshots{}, WriteProcessStats{}, WriteContainers{},
		WriteContainerStats{}, WriteProcessDiscoveries{},
		WriteConnections{}, WriteConnectionsPayloads{},
		ProcessSnapshotRow{}, ProcessStatRow{}, ContainerRow{},
		ContainerStatRow{}, ProcessDiscoveryRow{},
		ConnectionRow{}, ConnectionsPayloadRow{},
	)
}
