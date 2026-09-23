package intake

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"testing"
	"time"

	"github.com/DataDog/agent-payload/v5/process"
	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
)

// The agent feeds this reply into CheckRunner.UpdateRTStatus, which indexes the
// statuses it collected without checking that it got any. A nil Status
// therefore does not degrade gracefully — it segfaults the agent and
// crash-loops its container. Observed for real before this was fixed.
func TestResCollectorCarriesStatus(t *testing.T) {
	raw := resCollectorResponse()
	if len(raw) == 0 {
		t.Fatal("empty response")
	}

	msg, err := process.DecodeMessage(raw)
	if err != nil {
		t.Fatalf("the agent could not decode our own reply: %v", err)
	}

	res, ok := msg.Body.(*process.ResCollector)
	if !ok {
		t.Fatalf("body is %T, want *process.ResCollector", msg.Body)
	}
	if res.GetStatus() == nil {
		t.Fatal("Status is nil — this is the shape that panics the agent")
	}
	if got := res.GetStatus().GetInterval(); got != processCheckInterval {
		t.Errorf("interval: got %d, want %d", got, processCheckInterval)
	}
	if got := res.GetStatus().GetActiveClients(); got != 0 {
		t.Errorf("active clients: got %d, want 0 — nothing is watching a live view", got)
	}
	if res.GetHeader() == nil {
		t.Error("nested Header missing; older agents look for it")
	}
}

// ---------------------------------------------------------------------------
// Test support
// ---------------------------------------------------------------------------

// The headers a real process-agent stamps on a submit. Kept as constants
// because several tests assert they reached a column: before this work they
// existed only inside a log line.
const (
	testProcAgentHost      = "ip-10-0-1-7"
	testProcAgentVersion   = "7.58.2"
	testProcContainerCount = "31"
	testProcRequestID      = "b0a1f2c3-0000-4000-8000-000000000001"
)

// encodeProcessFrame wraps a body in the 16-byte frame the process-agent
// speaks, using the agent's own encoder — hand-packing the header would test
// our idea of the format rather than the format.
func encodeProcessFrame(t *testing.T, typ process.MessageType, body process.MessageBody) []byte {
	t.Helper()
	raw, err := process.EncodeMessage(process.Message{
		Header: process.MessageHeader{
			Version:  process.MessageV3,
			Encoding: process.MessageEncodingProtobuf,
			Type:     typ,
			OrgID:    7,
		},
		Body: body,
	})
	if err != nil {
		t.Fatalf("encode %T: %v", body, err)
	}
	return raw
}

// postProcessFrame posts a frame with the API key and the agent's identity
// headers, and checks the reply contract every process route shares: 200,
// application/x-protobuf, a decodable ResCollector. That contract holds even
// for a frame we could not read, which is why it is asserted here rather than
// in one test.
func postProcessFrame(t *testing.T, e *gin.Engine, path string, frame []byte) *httptest.ResponseRecorder {
	t.Helper()

	req := httptest.NewRequest(http.MethodPost, path, bytes.NewReader(frame))
	req.Header.Set("Dd-Api-Key", testAPIKey)
	req.Header.Set("Content-Type", "application/x-protobuf")
	req.Header.Set("X-Dd-Hostname", testProcAgentHost)
	req.Header.Set("X-Dd-Processagentversion", testProcAgentVersion)
	req.Header.Set("X-Dd-ContainerCount", testProcContainerCount)
	req.Header.Set("X-DD-Request-ID", testProcRequestID)

	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Fatalf("POST %s: got %d, want 200 (%s)", path, w.Code, w.Body.String())
	}
	if ct := w.Header().Get("Content-Type"); ct != "application/x-protobuf" {
		t.Errorf("POST %s content-type: got %q, want application/x-protobuf", path, ct)
	}
	reply, err := process.DecodeMessage(w.Body.Bytes())
	if err != nil {
		t.Fatalf("POST %s: the agent could not decode the reply: %v", path, err)
	}
	if res, ok := reply.Body.(*process.ResCollector); !ok || res.GetStatus() == nil {
		t.Fatalf("POST %s: reply is %T with nil status — the shape that panics the agent", path, reply.Body)
	}
	return w
}

// fullProcess is one process with every sub-message populated, so a converter
// test can assert that nothing on the way to a column was skipped. The values
// are deliberately distinct: a field that silently reads its neighbour would
// still look plausible with zeros everywhere.
func fullProcess() *process.Process {
	return &process.Process{
		Key:   4242,
		Pid:   1234,
		NsPid: 7,
		Command: &process.Command{
			Args:   []string{"/usr/bin/app", "--filter=a b", "-v"},
			Cwd:    "/srv/app",
			Root:   "/",
			OnDisk: true,
			Ppid:   1,
			Pgroup: 1200,
			Exe:    "/usr/bin/app",
			Comm:   "app",
		},
		User: &process.ProcessUser{Name: "svc", Uid: 1000, Gid: 1001, Euid: 0, Egid: 2, Suid: 3, Sgid: 4},
		Memory: &process.MemoryStat{
			Rss: 100, Vms: 200, Swap: 300, Shared: 400,
			Text: 500, Lib: 600, Data: 700, Dirty: 800,
		},
		Cpu: &process.CPUStat{
			LastCpu: "cpu3", TotalPct: 12.5, UserPct: 8.5, SystemPct: 4,
			NumThreads: 9, Nice: -5, UserTime: 111, SystemTime: 222,
			Cpus: []*process.SingleCPUStat{{Name: "cpu0", TotalPct: 1}, {Name: "cpu1", TotalPct: 2}},
		},
		CreateTime:             1_700_000_000_000,
		OpenFdCount:            64,
		State:                  process.ProcessState_R,
		IoStat:                 &process.IOStat{ReadRate: 1, WriteRate: 2, ReadBytesRate: 3, WriteBytesRate: 4},
		ContainerId:            "cid-1",
		ContainerKey:           99,
		VoluntaryCtxSwitches:   11,
		InvoluntaryCtxSwitches: 22,
		ByteKey:                []byte{0x01, 0x02},
		ContainerByteKey:       []byte{0x03},
		Networks:               &process.ProcessNetworks{ConnectionRate: 5, BytesRate: 6},
		ProcessContext:         []string{"ctx-a", "ctx-b"},
		// Two tags sharing a key: a pod behind two services produces exactly
		// this, and a plain map would keep only one of them.
		Tags:     []string{"env:prod", "kube_service:a", "kube_service:b"},
		Language: process.Language_LANGUAGE_GO,
		PortInfo: &process.PortInfo{Tcp: []int32{8080, 8443}, Udp: []int32{53}},
		ServiceDiscovery: &process.ServiceDiscovery{
			GeneratedServiceName: &process.ServiceName{
				Name: "guessed", Source: process.ServiceNameSource_SERVICE_NAME_SOURCE_COMMAND_LINE,
			},
			DdServiceName: &process.ServiceName{
				Name: "declared", Source: process.ServiceNameSource_SERVICE_NAME_SOURCE_DD_SERVICE,
			},
			AdditionalGeneratedNames: []*process.ServiceName{
				{Name: "extra", Source: process.ServiceNameSource_SERVICE_NAME_SOURCE_SPRING},
			},
			TracerMetadata: []*process.TracerMetadata{
				{RuntimeId: "runtime-1", ServiceName: "traced"},
			},
			ApmInstrumentation: true,
			Resources: []*process.Resource{
				{Resource: &process.Resource_Logs{Logs: &process.LogResource{Path: "/var/log/app.log"}}},
			},
		},
		InjectionState:       process.InjectionState_INJECTION_INJECTED,
		ZombieChildrenCount:  3,
		ZombieNetRate:        1.5,
		HasZombieAggregation: true,
	}
}

func fullCollectorProc() *process.CollectorProc {
	return &process.CollectorProc{
		HostName:  "host-a",
		NetworkId: "vpc-123",
		Processes: []*process.Process{fullProcess()},
		Host: &process.Host{
			Id: 5, OrgId: 7, Name: "host-a",
			AllTags: []string{"env:prod", "kube_service:a", "kube_service:b"},
			NumCpus: 8, TotalMemory: 16 << 30, TagIndex: 0, TagsModified: 1_700_000_000,
		},
		Info: &process.SystemInfo{
			Uuid: "uuid-1",
			Os: &process.OSInfo{
				Name: "linux", Platform: "ubuntu", Family: "debian",
				Version: "22.04", KernelVersion: "5.15.0",
			},
			Cpus: []*process.CPUInfo{
				{Number: 0, Vendor: "GenuineIntel", Family: "6", Model: "85", PhysicalId: "0", CoreId: "0", Cores: 4, Mhz: 2500, CacheSize: 33792},
				{Number: 1, Vendor: "GenuineIntel", Family: "6", Model: "85", PhysicalId: "0", CoreId: "1", Cores: 4, Mhz: 2500, CacheSize: 33792},
			},
			TotalMemory: 16 << 30,
		},
		GroupId:           11,
		GroupSize:         2,
		Containers:        []*process.Container{fullContainer()},
		ContainerHostType: process.ContainerHostType_fargateEKS,
		Hints:             &process.CollectorProc_HintMask{HintMask: 3},
	}
}

func fullContainer() *process.Container {
	return &process.Container{
		Type: "containerd", Id: "cid-1", Name: "app", Image: "repo/app:1",
		CpuLimit: 2, MemoryLimit: 1 << 30,
		State: process.ContainerState_running, Health: process.ContainerHealth_healthy,
		Created: 1_700_000_000, Started: 1_700_000_060,
		Rbps: 1, Wbps: 2, Key: 77,
		NetRcvdPs: 3, NetSentPs: 4, NetRcvdBps: 5, NetSentBps: 6,
		UserPct: 7, SystemPct: 8, TotalPct: 15,
		MemRss: 9, MemCache: 10,
		ByteKey: []byte{0xAA},
		Tags:    []string{"env:prod", "kube_service:a", "kube_service:b"},
		Addresses: []*process.ContainerAddr{
			{Ip: "10.0.0.1", Port: 8080, Protocol: process.ConnectionType_tcp},
			{Ip: "10.0.0.1", Port: 8080, Protocol: process.ConnectionType_udp},
		},
		ThreadCount: 12, ThreadLimit: 100, MemUsage: 13, CpuUsageNs: 14,
		MemAccounted: 15, CpuRequest: 0.5, MemoryRequest: 1 << 20,
		RepoDigest: "sha256:abc",
	}
}

// ---------------------------------------------------------------------------
// Converter tests
// ---------------------------------------------------------------------------

// The audit that started this work listed, field by field, what storeProcesses
// dropped: service discovery, tracer runtime ids, language, per-core CPU, the
// io rates, the six uids, the listening ports, argv. This pins every one of
// them against a fully populated Process, because "we added the column" and
// "the value reaches the column" are different claims.
func TestProcessRowKeepsEveryDecodedField(t *testing.T) {
	f := processFrame{Tenant: testTenant, At: time.Unix(1, 0).UTC()}
	f.Agent.Version = testProcAgentVersion
	f.Agent.RequestID = testProcRequestID

	proc := fullCollectorProc()
	row := processRow(f, proc, proc.Processes[0])

	cases := []struct {
		field string
		got   any
		want  any
	}{
		{"ns_pid", row.NsPID, int32(7)},
		{"key", row.Key, uint32(4242)},
		{"cwd", row.Cwd, "/srv/app"},
		{"root", row.Root, "/"},
		{"on_disk", row.OnDisk, uint8(1)},
		{"pgroup", row.Pgroup, int32(1200)},
		{"uid", row.UID, int32(1000)},
		{"gid", row.GID, int32(1001)},
		{"euid", row.EUID, int32(0)},
		{"egid", row.EGID, int32(2)},
		{"suid", row.SUID, int32(3)},
		{"sgid", row.SGID, int32(4)},
		{"mem_swap", row.MemSwap, uint64(300)},
		{"mem_shared", row.MemShared, uint64(400)},
		{"mem_text", row.MemText, uint64(500)},
		{"mem_lib", row.MemLib, uint64(600)},
		{"mem_data", row.MemData, uint64(700)},
		{"mem_dirty", row.MemDirty, uint64(800)},
		{"cpu_last_cpu", row.CPULastCPU, "cpu3"},
		{"cpu_user_pct", row.CPUUserPct, float32(8.5)},
		{"cpu_system_pct", row.CPUSystemPct, float32(4)},
		{"cpu_nice", row.CPUNice, int32(-5)},
		{"cpu_user_time", row.CPUUserTime, int64(111)},
		{"cpu_system_time", row.CPUSystemTime, int64(222)},
		{"io_read_rate", row.IOReadRate, float32(1)},
		{"io_write_bytes_rate", row.IOWriteBytesRate, float32(4)},
		{"voluntary_ctx_switches", row.VoluntaryCtxSwitches, uint64(11)},
		{"involuntary_ctx_switches", row.InvoluntaryCtxSwitches, uint64(22)},
		{"net_connection_rate", row.NetConnectionRate, float32(5)},
		{"net_bytes_rate", row.NetBytesRate, float32(6)},
		// Enum NAMES, never numbers: a renumbering upstream must not rewrite
		// stored history.
		{"language", row.Language, "LANGUAGE_GO"},
		{"injection_state", row.InjectionState, "INJECTION_INJECTED"},
		{"state", row.State, "R"},
		{"generated_service_name", row.GeneratedServiceName, "guessed"},
		{"generated_service_name_source", row.GeneratedServiceNameSource, "SERVICE_NAME_SOURCE_COMMAND_LINE"},
		{"dd_service_name", row.DDServiceName, "declared"},
		{"dd_service_name_source", row.DDServiceNameSource, "SERVICE_NAME_SOURCE_DD_SERVICE"},
		{"apm_instrumentation", row.APMInstrumentation, uint8(1)},
		{"zombie_children_count", row.ZombieChildrenCount, uint32(3)},
		{"zombie_net_rate", row.ZombieNetRate, 1.5},
		{"has_zombie_aggregation", row.HasZombieAggregation, uint8(1)},
		{"container_key", row.ContainerKey, uint32(99)},
		{"byte_key", row.ByteKey, "\x01\x02"},
		{"container_byte_key", row.ContainerByteKey, "\x03"},
		{"network_id", row.NetworkID, "vpc-123"},
		{"group_id", row.GroupID, int32(11)},
		{"group_size", row.GroupSize, int32(2)},
		{"container_host_type", row.ContainerHostType, "fargateEKS"},
		{"agent_version", row.AgentVersion, testProcAgentVersion},
		{"request_id", row.RequestID, testProcRequestID},
		{"host_uuid", row.HostUUID, "uuid-1"},
		{"os_name", row.OSName, "linux"},
		{"os_platform", row.OSPlatform, "ubuntu"},
		{"os_family", row.OSFamily, "debian"},
		{"os_version", row.OSVersion, "22.04"},
		{"kernel_version", row.KernelVersion, "5.15.0"},
	}
	for _, tc := range cases {
		t.Run(tc.field, func(t *testing.T) {
			if !reflect.DeepEqual(tc.got, tc.want) {
				t.Errorf("%s: got %#v, want %#v", tc.field, tc.got, tc.want)
			}
		})
	}

	// Parallel arrays: index i of one pairs with index i of the other, and the
	// order is the wire's.
	if !reflect.DeepEqual(row.CPUCoreNames, []string{"cpu0", "cpu1"}) ||
		!reflect.DeepEqual(row.CPUCorePcts, []float32{1, 2}) {
		t.Errorf("per-core cpu: got %v / %v, want [cpu0 cpu1] / [1 2]", row.CPUCoreNames, row.CPUCorePcts)
	}
	if !reflect.DeepEqual(row.PortTCP, []int32{8080, 8443}) || !reflect.DeepEqual(row.PortUDP, []int32{53}) {
		t.Errorf("port info: got tcp=%v udp=%v", row.PortTCP, row.PortUDP)
	}
	if !reflect.DeepEqual(row.ProcessContext, []string{"ctx-a", "ctx-b"}) {
		t.Errorf("process_context: got %v", row.ProcessContext)
	}
	// The APM join: without these two arrays there is no way to tie a pid on
	// a host to the traces it emitted.
	if !reflect.DeepEqual(row.TracerRuntimeIDs, []string{"runtime-1"}) ||
		!reflect.DeepEqual(row.TracerServiceNames, []string{"traced"}) {
		t.Errorf("tracer metadata: got %v / %v", row.TracerRuntimeIDs, row.TracerServiceNames)
	}
	if !reflect.DeepEqual(row.AdditionalGeneratedNames, []string{"extra"}) ||
		!reflect.DeepEqual(row.AdditionalGeneratedNameSources, []string{"SERVICE_NAME_SOURCE_SPRING"}) {
		t.Errorf("additional names: got %v / %v", row.AdditionalGeneratedNames, row.AdditionalGeneratedNameSources)
	}
	// The oneof list is kept whole, so a member upstream adds later still
	// arrives named instead of being counted.
	if !json.Valid([]byte(row.ServiceResources)) {
		t.Errorf("service_resources is not JSON: %q", row.ServiceResources)
	}
	if row.ServiceResources == "" {
		t.Error("service_resources is empty although the process carried a log resource")
	}
}

// A tag key may legally repeat — a pod behind two services sends
// kube_service:a and kube_service:b — and a plain map would keep one of them.
func TestProcessRowKeepsMultiValuedTags(t *testing.T) {
	proc := fullCollectorProc()
	row := processRow(processFrame{Tenant: testTenant}, proc, proc.Processes[0])

	want := map[string][]string{
		"env":          {"prod"},
		"kube_service": {"a", "b"},
	}
	if !reflect.DeepEqual(row.Tags, want) {
		t.Errorf("tags: got %v, want %v", row.Tags, want)
	}
}

// Joining argv on spaces is irreversible: `--filter=a b` and two separate
// arguments produce the same string. Both forms are stored, and this pins that
// args is the one that survives the round trip.
func TestProcessRowKeepsArgvAndCmdline(t *testing.T) {
	proc := fullCollectorProc()
	row := processRow(processFrame{Tenant: testTenant}, proc, proc.Processes[0])

	want := []string{"/usr/bin/app", "--filter=a b", "-v"}
	if !reflect.DeepEqual(row.Args, want) {
		t.Errorf("args: got %v, want %v", row.Args, want)
	}
	if row.Cmdline != "/usr/bin/app --filter=a b -v" {
		t.Errorf("cmdline: got %q", row.Cmdline)
	}
}

// The agent omits every sub-message it could not read, and each of them is a
// pointer. A snapshot from a host where /proc was partly unreadable must
// produce a row, not a panic.
func TestProcessRowToleratesNilSubMessages(t *testing.T) {
	proc := &process.CollectorProc{HostName: "host-a", Processes: []*process.Process{{Pid: 5}}}
	row := processRow(processFrame{Tenant: testTenant}, proc, proc.Processes[0])

	if row.PID != 5 {
		t.Fatalf("pid: got %d, want 5", row.PID)
	}
	if row.Comm != "" || row.User != "" || row.RSS != 0 || row.CPUPct != 0 {
		t.Errorf("a process with no sub-messages produced values from nowhere: %+v", row)
	}
	// Process.host is a backend-resolved field a real agent leaves nil; empty
	// string rather than the literal "null", so "absent" stays readable.
	if row.ProcessHost != "" {
		t.Errorf("process_host: got %q, want empty for a nil Host", row.ProcessHost)
	}
}

// create_time cannot be Nullable without rewriting the column, so absence is
// carried in has_create_time. The distinction matters: a process that started
// at the epoch and a process whose start time the agent could not read are not
// the same row.
func TestProcessRowCreateTimeAbsentIsNotZero(t *testing.T) {
	cases := []struct {
		name    string
		wire    int64
		wantHas uint8
		wantAt  time.Time
	}{
		{"absent", 0, 0, time.Time{}},
		{"negative is absent too", -1, 0, time.Time{}},
		{"present, milliseconds", 1_700_000_000_000, 1, time.UnixMilli(1_700_000_000_000).UTC()},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			proc := &process.CollectorProc{Processes: []*process.Process{{Pid: 1, CreateTime: tc.wire}}}
			row := processRow(processFrame{Tenant: testTenant}, proc, proc.Processes[0])
			if row.HasCreateTime != tc.wantHas {
				t.Errorf("has_create_time: got %d, want %d", row.HasCreateTime, tc.wantHas)
			}
			if !row.CreateTime.Equal(tc.wantAt) {
				t.Errorf("create_time: got %v, want %v", row.CreateTime, tc.wantAt)
			}
		})
	}
}

// The envelope table has to handle all six message types, and they spread the
// same information differently: CollectorProc carries a *Host and a
// *SystemInfo, the realtime messages inline a host id and a cpu count instead.
func TestProcessSnapshotRowPerMessageType(t *testing.T) {
	f := processFrame{Tenant: testTenant, At: time.Unix(1, 0).UTC(), Path: "/api/v1/collector"}

	cases := []struct {
		name  string
		typ   process.MessageType
		body  process.MessageBody
		check func(*testing.T, storage.ProcessSnapshotRow)
	}{
		{
			name: "CollectorProc", typ: process.TypeCollectorProc, body: fullCollectorProc(),
			check: func(t *testing.T, r storage.ProcessSnapshotRow) {
				if r.MessageTypeName != "CollectorProc" || r.MessageType != 12 {
					t.Errorf("type: got %d/%s", r.MessageType, r.MessageTypeName)
				}
				if r.NetworkID != "vpc-123" || r.HostID != 5 || r.HostNumCPUs != 8 {
					t.Errorf("host envelope lost: %+v", r)
				}
				if got := r.HostAllTags["kube_service"]; !reflect.DeepEqual(got, []string{"a", "b"}) {
					t.Errorf("host all_tags multiset: got %v", got)
				}
				if r.HostTagsModified == nil || r.HostTagsModified.Unix() != 1_700_000_000 {
					t.Errorf("host_tags_modified: got %v", r.HostTagsModified)
				}
				if !reflect.DeepEqual(r.CPUNumbers, []int32{0, 1}) || len(r.CPUMhz) != 2 {
					t.Errorf("cpu inventory: %v / %v", r.CPUNumbers, r.CPUMhz)
				}
				if r.KernelVersion != "5.15.0" || r.SysUUID != "uuid-1" {
					t.Errorf("system info lost: %+v", r)
				}
				if r.HintMask == nil || *r.HintMask != 3 {
					t.Errorf("hint_mask: got %v, want 3", r.HintMask)
				}
				if r.ProcessCount != 1 || r.ContainerCount != 1 {
					t.Errorf("counts: got %d/%d", r.ProcessCount, r.ContainerCount)
				}
			},
		},
		{
			name: "CollectorRealTime", typ: process.TypeCollectorRealTime,
			body: &process.CollectorRealTime{
				HostName: "host-a", HostId: 5, OrgId: 7, NumCpus: 8, TotalMemory: 99,
				Stats:          []*process.ProcessStat{{Pid: 1}, {Pid: 2}},
				ContainerStats: []*process.ContainerStat{{Id: "c"}},
			},
			check: func(t *testing.T, r storage.ProcessSnapshotRow) {
				if r.RTHostID != 5 || r.RTOrgID != 7 || r.RTNumCPUs != 8 || r.RTTotalMemory != 99 {
					t.Errorf("realtime envelope lost: %+v", r)
				}
				if r.ProcessStatCount != 2 || r.ContainerStatCount != 1 {
					t.Errorf("counts: got %d/%d", r.ProcessStatCount, r.ContainerStatCount)
				}
				// A realtime frame has no Host message at all; nothing may be
				// invented for it.
				if r.HintMask != nil || r.HostID != 0 {
					t.Errorf("realtime frame produced host fields from nowhere: %+v", r)
				}
			},
		},
		{
			name: "CollectorContainer", typ: process.TypeCollectorContainer,
			body: &process.CollectorContainer{
				HostName: "host-a", NetworkId: "vpc-9",
				Containers:        []*process.Container{fullContainer()},
				ContainerHostType: process.ContainerHostType_sidecar,
			},
			check: func(t *testing.T, r storage.ProcessSnapshotRow) {
				if r.NetworkID != "vpc-9" || r.ContainerCount != 1 {
					t.Errorf("container envelope lost: %+v", r)
				}
				if r.ContainerHostType != "sidecar" {
					t.Errorf("container_host_type: got %q", r.ContainerHostType)
				}
			},
		},
		{
			name: "CollectorContainerRealTime", typ: process.TypeCollectorContainerRealTime,
			body: &process.CollectorContainerRealTime{
				HostName: "host-a", HostId: 6, NumCpus: 4, TotalMemory: 8,
				Stats: []*process.ContainerStat{{Id: "c"}},
			},
			check: func(t *testing.T, r storage.ProcessSnapshotRow) {
				if r.RTHostID != 6 || r.ContainerStatCount != 1 {
					t.Errorf("container realtime envelope lost: %+v", r)
				}
			},
		},
		{
			name: "CollectorProcDiscovery", typ: process.TypeCollectorProcDiscovery,
			body: &process.CollectorProcDiscovery{
				HostName: "host-a", GroupId: 3, GroupSize: 4,
				ProcessDiscoveries: []*process.ProcessDiscovery{{Pid: 1}},
				Host:               &process.Host{Id: 5, Name: "host-a"},
			},
			check: func(t *testing.T, r storage.ProcessSnapshotRow) {
				if r.DiscoveryCount != 1 || r.GroupID != 3 || r.GroupSize != 4 || r.HostID != 5 {
					t.Errorf("discovery envelope lost: %+v", r)
				}
			},
		},
		{
			name: "CollectorConnections", typ: process.TypeCollectorConnections,
			body: &process.CollectorConnections{
				HostName: "host-a", NetworkId: "vpc-1",
				Connections: []*process.Connection{{Pid: 1}, {Pid: 2}, {Pid: 3}},
			},
			check: func(t *testing.T, r storage.ProcessSnapshotRow) {
				if r.ConnectionCount != 3 {
					t.Errorf("connection_count: got %d, want 3", r.ConnectionCount)
				}
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			row := processSnapshotRow(f, process.Message{
				Header: process.MessageHeader{
					Version: process.MessageV3, Type: tc.typ, OrgID: 7, Timestamp: 4242,
				},
				Body: tc.body,
			})
			if row.TenantID != testTenant {
				t.Errorf("tenant: got %q", row.TenantID)
			}
			if row.HostName != "host-a" {
				t.Errorf("host_name: got %q", row.HostName)
			}
			// The frame header's timestamp is stored raw: the agent leaves it
			// at zero and its unit is documented nowhere, so interpreting it
			// would be inventing a value.
			if row.HeaderTimestamp != 4242 || row.HeaderOrgID != 7 {
				t.Errorf("frame header lost: ts=%d org=%d", row.HeaderTimestamp, row.HeaderOrgID)
			}
			tc.check(t, row)
		})
	}
}

// A container the runtime never gave a creation time for must not read as
// having started on 1 January 1970 — the same rule container_events follows.
func TestContainerRowsKeepAbsentTimestampsNull(t *testing.T) {
	f := processFrame{Tenant: testTenant, At: time.Unix(1, 0).UTC()}

	full := containerRows(f, containerEnv{Source: containerSourceProc}, "host-a",
		[]*process.Container{fullContainer()})
	if len(full) != 1 {
		t.Fatalf("rows: got %d, want 1", len(full))
	}
	if full[0].Created == nil || full[0].Created.Unix() != 1_700_000_000 {
		t.Errorf("created: got %v, want 2023-11-14T22:13:20Z", full[0].Created)
	}
	if full[0].Started == nil || full[0].Started.Unix() != 1_700_000_060 {
		t.Errorf("started: got %v", full[0].Started)
	}
	// Three parallel arrays: the same ip and port on two protocols is a real
	// shape, and a map keyed by ip would collapse it.
	if !reflect.DeepEqual(full[0].AddrProtocols, []string{"tcp", "udp"}) {
		t.Errorf("addresses: got %v", full[0].AddrProtocols)
	}
	if full[0].State != "running" || full[0].Health != "healthy" {
		t.Errorf("enums by name: got %q/%q", full[0].State, full[0].Health)
	}
	if got := full[0].Tags["kube_service"]; !reflect.DeepEqual(got, []string{"a", "b"}) {
		t.Errorf("container tags multiset: got %v", got)
	}

	bare := containerRows(f, containerEnv{Source: containerSourceProc}, "host-a",
		[]*process.Container{{Id: "c"}})
	if bare[0].Created != nil || bare[0].Started != nil {
		t.Errorf("a container with no times got %v / %v, want NULL both",
			bare[0].Created, bare[0].Started)
	}
	// A nil element in a repeated field is skipped, never dereferenced.
	if got := containerRows(f, containerEnv{}, "h", []*process.Container{nil}); len(got) != 0 {
		t.Errorf("nil container produced %d rows, want 0", len(got))
	}
}

// The pre-7.35 telemetry message is flattened into the same name/value shape
// as the open-ended map newer agents send, so both eras read the same way —
// and an agent that sent none must not produce a row full of zeros.
func TestConnTelemetryMapAbsentStaysAbsent(t *testing.T) {
	if got := connTelemetryMap(nil); got != nil {
		t.Errorf("nil telemetry: got %v, want nil", got)
	}
	got := connTelemetryMap(&process.CollectorConnectionsTelemetry{KprobesTriggered: 9, DnsStatsDropped: 4})
	if got["kprobes_triggered"] != 9 || got["dns_stats_dropped"] != 4 {
		t.Errorf("telemetry: got %v", got)
	}
	if len(got) != 11 {
		t.Errorf("telemetry keys: got %d, want 11 — every field of the message", len(got))
	}
}

// route_idx is a POSITION in the routes list, so a nil element has to keep its
// slot; dropping it would shift every index after it onto the wrong route.
func TestRoutesJSONKeepsPositions(t *testing.T) {
	raw := routesJSON([]*process.Route{
		{Subnet: &process.Subnet{Alias: "subnet-a"}},
		nil,
		{Interface: &process.Interface{HardwareAddr: "02:42:ac:11:00:02"}},
	})
	var got []map[string]string
	if err := json.Unmarshal([]byte(raw), &got); err != nil {
		t.Fatalf("routes json: %v (%s)", err, raw)
	}
	if len(got) != 3 {
		t.Fatalf("routes: got %d entries, want 3 — a nil route keeps its index", len(got))
	}
	if got[0]["subnet_alias"] != "subnet-a" || got[2]["hardware_addr"] != "02:42:ac:11:00:02" {
		t.Errorf("routes: %s", raw)
	}
	if routesJSON(nil) != "" {
		t.Error("no routes must store nothing, not the literal null")
	}
}

// A connection that conntrack did not translate — which is most of them —
// must leave the NAT columns NULL rather than claiming a translation to
// 0.0.0.0:0. The tag indices are resolved through the agent-payload reader,
// and both the index and the packed buffer are kept beside the result.
func TestConnectionRowTranslationAndTags(t *testing.T) {
	enc := process.NewV2TagEncoder()
	idx := enc.Encode([]string{"env:prod", "kube_service:a", "kube_service:b"})

	conns := &process.CollectorConnections{
		HostName:               "host-a",
		EncodedConnectionsTags: enc.Buffer(),
		Connections: []*process.Connection{
			{
				Pid:   1,
				Laddr: &process.Addr{Ip: "10.0.0.1", Port: 5000},
				Raddr: &process.Addr{Ip: "10.0.0.2", Port: 443},
				Type:  process.ConnectionType_tcp, Family: process.ConnectionFamily_v4,
				Direction: process.ConnectionDirection_outgoing,
				Protocol:  &process.ProtocolStack{Stack: []process.ProtocolType{process.ProtocolType_protocolTLS, process.ProtocolType_protocolHTTP2}},
				TagsIdx:   int32(idx),
			},
			{
				Pid:           2,
				IpTranslation: &process.IPTranslation{ReplSrcIP: "172.17.0.2", ReplDstIP: "10.0.0.9", ReplSrcPort: 1, ReplDstPort: 2},
			},
		},
	}
	f := processFrame{Tenant: testTenant, At: time.Unix(1, 0).UTC()}

	plain := connectionRow(f, conns, conns.Connections[0])
	if plain.IPTranslationReplSrcIP != nil || plain.IPTranslationReplSrcPort != nil {
		t.Errorf("untranslated connection claimed a NAT entry: %v", plain.IPTranslationReplSrcIP)
	}
	if plain.Type != "tcp" || plain.Family != "v4" || plain.Direction != "outgoing" {
		t.Errorf("enums by name: %+v", plain)
	}
	// The stack is ordered: TLS over HTTP/2 is not HTTP/2 over TLS.
	if !reflect.DeepEqual(plain.ProtocolStack, []string{"protocolTLS", "protocolHTTP2"}) {
		t.Errorf("protocol stack: got %v", plain.ProtocolStack)
	}
	if got := plain.Tags["kube_service"]; !reflect.DeepEqual(got, []string{"a", "b"}) {
		t.Errorf("resolved connection tags: got %v, want [a b]", got)
	}

	nat := connectionRow(f, conns, conns.Connections[1])
	if nat.IPTranslationReplSrcIP == nil || *nat.IPTranslationReplSrcIP != "172.17.0.2" {
		t.Errorf("translated connection lost its NAT entry: %v", nat.IPTranslationReplSrcIP)
	}
	if nat.IPTranslationReplDstPort == nil || *nat.IPTranslationReplDstPort != 2 {
		t.Errorf("repl_dst_port: got %v", nat.IPTranslationReplDstPort)
	}
	// A connection with no addresses at all must not panic or invent one.
	if nat.LaddrIP != "" || nat.RaddrPort != 0 {
		t.Errorf("addresses appeared from nowhere: %+v", nat)
	}
}

// ---------------------------------------------------------------------------
// Handler tests
// ---------------------------------------------------------------------------

// The full process check, end to end: a real frame through the real engine,
// asserting the reply contract and that every table the message feeds got its
// rows with the right tenant and the same snapshot id.
func TestHandleCollectorStoresSnapshotProcessesAndContainers(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeProcess)

	proc := fullCollectorProc()
	// A Container hanging off the process as well as on the snapshot: both are
	// stored, told apart by source.
	proc.Processes[0].Container = fullContainer()

	postProcessFrame(t, e, "/api/v1/collector",
		encodeProcessFrame(t, process.TypeCollectorProc, proc))

	snaps := Rows[storage.ProcessSnapshotRow](node)
	if len(snaps) != 1 {
		t.Fatalf("process_snapshots rows: got %d, want 1 — one row per frame", len(snaps))
	}
	if snaps[0].AgentVersion != testProcAgentVersion || snaps[0].AgentHostname != testProcAgentHost {
		t.Errorf("agent headers did not reach the envelope row: %+v", snaps[0])
	}
	if snaps[0].AgentContainerCount != testProcContainerCount || snaps[0].RequestID != testProcRequestID {
		t.Errorf("agent headers incomplete: %+v", snaps[0])
	}
	if snaps[0].FrameBytes == 0 {
		t.Error("frame_bytes is zero")
	}

	procs := Rows[storage.ProcessRow](node)
	if len(procs) != 1 {
		t.Fatalf("processes rows: got %d, want 1", len(procs))
	}
	if procs[0].TenantID != testTenant {
		t.Errorf("tenant: got %q, want %q", procs[0].TenantID, testTenant)
	}
	// Every row of a frame carries the same id back to its envelope.
	if procs[0].SnapshotID != snaps[0].SnapshotID {
		t.Errorf("snapshot_id: process %v, snapshot %v — the join is broken",
			procs[0].SnapshotID, snaps[0].SnapshotID)
	}
	if procs[0].TracerRuntimeIDs == nil {
		t.Error("service discovery did not survive the handler")
	}

	ctrs := Rows[storage.ContainerRow](node)
	if len(ctrs) != 2 {
		t.Fatalf("containers rows: got %d, want 2 — the inventory one and the per-process one", len(ctrs))
	}
	sources := map[string]int32{}
	for _, c := range ctrs {
		sources[c.Source] = c.PID
	}
	if _, ok := sources[containerSourceProc]; !ok {
		t.Errorf("no row from the snapshot inventory: %v", sources)
	}
	if pid, ok := sources[containerSourceProcess]; !ok || pid != 1234 {
		t.Errorf("per-process container row missing or unattributed: %v", sources)
	}

	if msgs := node.SentTo(storage.ProcessesWriter); len(msgs) != 1 {
		t.Errorf("messages to %s: got %d, want 1 — one batch per request",
			storage.ProcessesWriter, len(msgs))
	}
}

// The 2-second realtime stream, which had no table at all: its per-process and
// per-container stats both land now, including the deprecated container-level
// duplicates older agents send instead of a ContainerStat.
func TestHandleCollectorRealTimeStoresStats(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeProcess)

	rt := &process.CollectorRealTime{
		HostName: "host-a", HostId: 5, OrgId: 7, GroupId: 1, GroupSize: 1,
		NumCpus: 8, TotalMemory: 99,
		Stats: []*process.ProcessStat{{
			Pid: 1234, CreateTime: 1_700_000_000_000, Nice: 3, Threads: 9, OpenFdCount: 64,
			Memory:                 &process.MemoryStat{Rss: 1, Vms: 2, Dirty: 8},
			Cpu:                    &process.CPUStat{TotalPct: 5, UserTime: 7},
			IoStat:                 &process.IOStat{ReadRate: 1},
			Networks:               &process.ProcessNetworks{BytesRate: 2},
			ContainerId:            "cid-1",
			ContainerState:         process.ContainerState_running,
			ProcessState:           process.ProcessState_S,
			ContainerHealth:        process.ContainerHealth_unhealthy,
			ContainerRbps:          3,
			VoluntaryCtxSwitches:   11,
			InvoluntaryCtxSwitches: 22,
			ByteKey:                []byte{0x09},
		}},
		ContainerStats: []*process.ContainerStat{{
			Id: "cid-1", TotalPct: 15, MemLimit: 1 << 30,
			State: process.ContainerState_running, Health: process.ContainerHealth_healthy,
			Started: 1_700_000_060,
		}},
	}

	postProcessFrame(t, e, "/api/v1/collector",
		encodeProcessFrame(t, process.TypeCollectorRealTime, rt))

	stats := Rows[storage.ProcessStatRow](node)
	if len(stats) != 1 {
		t.Fatalf("process_stats rows: got %d, want 1", len(stats))
	}
	s := stats[0]
	if s.TenantID != testTenant || s.Host != "host-a" || s.PID != 1234 {
		t.Errorf("identity: %+v", s)
	}
	if s.HasCreateTime != 1 || s.CreateTime.UnixMilli() != 1_700_000_000_000 {
		t.Errorf("create_time: %v (has=%d)", s.CreateTime, s.HasCreateTime)
	}
	// ProcessStat.Nice and .Threads are the message's own fields, not the
	// nested CPUStat's, and both had to survive separately.
	if s.Nice != 3 || s.Threads != 9 || s.CPUUserTime != 7 {
		t.Errorf("nice/threads/cpu: %+v", s)
	}
	if s.ProcessState != "S" || s.ContainerState != "running" || s.ContainerHealth != "unhealthy" {
		t.Errorf("enums by name: %+v", s)
	}
	if s.MemDirty != 8 || s.IOReadRate != 1 || s.NetBytesRate != 2 {
		t.Errorf("nested stats lost: %+v", s)
	}
	if s.RTNumCPUs != 8 || s.RTTotalMemory != 99 || s.RTOrgID != 7 {
		t.Errorf("realtime envelope not denormalised: %+v", s)
	}

	cs := Rows[storage.ContainerStatRow](node)
	if len(cs) != 1 {
		t.Fatalf("container_stats rows: got %d, want 1", len(cs))
	}
	if cs[0].Source != containerStatSourceRealTime {
		t.Errorf("source: got %q, want %q", cs[0].Source, containerStatSourceRealTime)
	}
	if cs[0].Started == nil || cs[0].Started.Unix() != 1_700_000_060 {
		t.Errorf("started: got %v", cs[0].Started)
	}
}

// The container check, both of its message types, on the path they share.
func TestHandleContainerStoresInventoryAndStats(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeProcess)

	postProcessFrame(t, e, "/api/v1/container",
		encodeProcessFrame(t, process.TypeCollectorContainer, &process.CollectorContainer{
			HostName: "host-a", NetworkId: "vpc-9", GroupId: 1, GroupSize: 1,
			Containers: []*process.Container{fullContainer()},
			Info:       &process.SystemInfo{Uuid: "uuid-1"},
			Host:       &process.Host{Id: 5},
		}))

	ctrs := Rows[storage.ContainerRow](node)
	if len(ctrs) != 1 {
		t.Fatalf("containers rows: got %d, want 1", len(ctrs))
	}
	if ctrs[0].Source != containerSourceContainer || ctrs[0].NetworkID != "vpc-9" {
		t.Errorf("container row: %+v", ctrs[0])
	}
	if ctrs[0].RepoDigest != "sha256:abc" || ctrs[0].MemoryRequest == 0 {
		t.Errorf("container fields lost: %+v", ctrs[0])
	}

	postProcessFrame(t, e, "/api/v1/container",
		encodeProcessFrame(t, process.TypeCollectorContainerRealTime, &process.CollectorContainerRealTime{
			HostName: "host-a", HostId: 6, NumCpus: 4, TotalMemory: 8,
			Stats: []*process.ContainerStat{{Id: "cid-1", TotalPct: 3}},
		}))

	cs := Rows[storage.ContainerStatRow](node)
	if len(cs) != 1 {
		t.Fatalf("container_stats rows: got %d, want 1", len(cs))
	}
	if cs[0].Source != containerStatSourceCtrRT || cs[0].RTHostID != 6 {
		t.Errorf("container stat row: %+v", cs[0])
	}

	if n := len(Rows[storage.ProcessSnapshotRow](node)); n != 2 {
		t.Errorf("process_snapshots rows: got %d, want 2 — one per frame", n)
	}
}

// The discovery check: for hosts that run only this one, these rows are the
// sole process-level record that ever arrives.
func TestHandleProcDiscoveryStoresRows(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeProcess)

	postProcessFrame(t, e, "/api/v1/discovery",
		encodeProcessFrame(t, process.TypeCollectorProcDiscovery, &process.CollectorProcDiscovery{
			HostName: "host-a", GroupId: 3, GroupSize: 4,
			ProcessDiscoveries: []*process.ProcessDiscovery{
				{
					Pid: 42, NsPid: 7, CreateTime: 1_700_000_000_000, ByteKey: []byte{0x01},
					Command: &process.Command{Args: []string{"/bin/sh", "-c", "echo a b"}, Comm: "sh", Ppid: 1, Cwd: "/"},
					User:    &process.ProcessUser{Name: "root", Uid: 0, Gid: 0, Euid: 1},
				},
				{Pid: 43},
			},
			Host: &process.Host{Id: 5},
		}))

	rows := Rows[storage.ProcessDiscoveryRow](node)
	if len(rows) != 2 {
		t.Fatalf("process_discoveries rows: got %d, want 2", len(rows))
	}
	if rows[0].NsPID != 7 || rows[0].User != "root" || rows[0].EUID != 1 || rows[0].Cwd != "/" {
		t.Errorf("discovery row lost fields: %+v", rows[0])
	}
	if !reflect.DeepEqual(rows[0].Args, []string{"/bin/sh", "-c", "echo a b"}) {
		t.Errorf("args: got %v", rows[0].Args)
	}
	if rows[0].GroupID != 3 || rows[0].GroupSize != 4 {
		t.Errorf("group chunking lost: %+v", rows[0])
	}
	// The second discovery has nothing but a pid: no panic, and no invented
	// start time.
	if rows[1].HasCreateTime != 0 || rows[1].Comm != "" {
		t.Errorf("empty discovery produced values: %+v", rows[1])
	}
}

// The network check writes two tables at once: the per-frame payload row that
// holds the kernel identity and the packed buffers, and one row per
// connection, linked by payload_id.
func TestHandleConnectionsStoresPayloadAndConnections(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeProcess)

	enc := process.NewV2TagEncoder()
	hostIdx := enc.Encode([]string{"env:prod", "role:db", "role:cache"})

	postProcessFrame(t, e, "/api/v1/connections",
		encodeProcessFrame(t, process.TypeCollectorConnections, &process.CollectorConnections{
			HostName: "host-a", NetworkId: "vpc-1", GroupId: 1, GroupSize: 1,
			Architecture: "amd64", KernelVersion: "5.15.0", Platform: "ubuntu", PlatformVersion: "22.04",
			EncodedTags: enc.Buffer(), HostTagsIndex: int32(hostIdx),
			ContainerForPid:      map[int32]string{1: "cid-1"},
			ConnTelemetry:        &process.CollectorConnectionsTelemetry{KprobesTriggered: 9},
			ConnTelemetryMap:     map[string]int64{"conns_closed": 4},
			CORETelemetryByAsset: map[string]process.COREResult{"tcp": process.COREResult_SuccessEmbeddedBTF},
			PrebuiltEBPFAssets:   []string{"tracer.o"},
			Routes:               []*process.Route{{Subnet: &process.Subnet{Alias: "subnet-a"}}},
			AgentConfiguration:   &process.AgentConfiguration{NpmEnabled: true, UsmEnabled: true},
			Domains:              []string{"example.com"},
			EcsTask:              "arn:task",
			ResolvConfs:          []string{"nameserver 10.0.0.2"},
			Connections: []*process.Connection{
				{
					Pid:   1,
					Laddr: &process.Addr{Ip: "10.0.0.1", Port: 5000, ContainerId: "cid-1"},
					Raddr: &process.Addr{Ip: "10.0.0.2", Port: 443},
					Type:  process.ConnectionType_tcp,
					// Opaque USM blobs: kept as bytes rather than dropped.
					HttpAggregations:     []byte{0x0a, 0x0b},
					DatabaseAggregations: []byte{0x0c},
					DnsCountByRcode:      map[uint32]uint32{0: 5, 3: 1},
					TcpFailuresByErrCode: map[uint32]uint32{110: 2},
					LastTcpRtoCount:      4,
					TcpEcnNegotiated:     true,
				},
				{Pid: 2},
			},
		}))

	payloads := Rows[storage.ConnectionsPayloadRow](node)
	if len(payloads) != 1 {
		t.Fatalf("connections_payloads rows: got %d, want 1", len(payloads))
	}
	p := payloads[0]
	if p.Architecture != "amd64" || p.KernelVersion != "5.15.0" || p.Platform != "ubuntu" {
		t.Errorf("system-probe identity lost: %+v", p)
	}
	if got := p.HostTags["role"]; !reflect.DeepEqual(got, []string{"db", "cache"}) {
		t.Errorf("host tags resolved out of the packed buffer: got %v, want [db cache]", got)
	}
	if p.EncodedTags == "" {
		t.Error("the packed tag buffer was not kept — a future encoding would be unreadable")
	}
	if p.ConnTelemetry["kprobes_triggered"] != 9 || p.ConnTelemetryMap["conns_closed"] != 4 {
		t.Errorf("telemetry: %v / %v", p.ConnTelemetry, p.ConnTelemetryMap)
	}
	if p.CORETelemetry["tcp"] != "SuccessEmbeddedBTF" {
		t.Errorf("CO-RE result must be stored by name: got %q", p.CORETelemetry["tcp"])
	}
	if p.Routes == "" || p.AgentConfiguration == "" {
		t.Errorf("routes/agent configuration lost: %q / %q", p.Routes, p.AgentConfiguration)
	}
	if p.EcsTask != "arn:task" || len(p.ResolvConfs) != 1 || p.ContainerForPID[1] != "cid-1" {
		t.Errorf("payload tail lost: %+v", p)
	}
	if p.ConnectionCount != 2 {
		t.Errorf("connection_count: got %d, want 2", p.ConnectionCount)
	}

	conns := Rows[storage.ConnectionRow](node)
	if len(conns) != 2 {
		t.Fatalf("connections rows: got %d, want 2", len(conns))
	}
	if conns[0].PayloadID != p.PayloadID {
		t.Errorf("payload_id: connection %v, payload %v — the join is broken",
			conns[0].PayloadID, p.PayloadID)
	}
	if conns[0].LaddrIP != "10.0.0.1" || conns[0].RaddrPort != 443 {
		t.Errorf("addresses: %+v", conns[0])
	}
	if conns[0].HTTPAggregations != "\x0a\x0b" || conns[0].DatabaseAggregations != "\x0c" {
		t.Errorf("USM aggregation blobs lost: %q / %q",
			conns[0].HTTPAggregations, conns[0].DatabaseAggregations)
	}
	if conns[0].DNSCountByRcode[3] != 1 || conns[0].TCPFailuresByErrCode[110] != 2 {
		t.Errorf("per-connection maps: %v / %v", conns[0].DNSCountByRcode, conns[0].TCPFailuresByErrCode)
	}
	if conns[0].LastTCPRtoCount != 4 || conns[0].TCPEcnNegotiated != 1 {
		t.Errorf("extended tcp counters: %+v", conns[0])
	}
}

// A frame we cannot read is the payload that would explain why, and it arrives
// once from somebody's real agent. It must reach raw_payloads — and the agent
// must still get its valid ResCollector, because a nil Status crash-loops it.
func TestProcessDecodeFailureGoesToRawPayloads(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeProcess)

	// Version byte 9: not a frame version DecodeMessage knows.
	postProcessFrame(t, e, "/api/v1/collector", []byte{9, 0, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x41})

	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 {
		t.Fatalf("raw_payloads rows: got %d, want 1", len(raws))
	}
	if raws[0].Intake != "process" || raws[0].Reason != "decode_error" {
		t.Errorf("intake/reason: got %q/%q, want process/decode_error", raws[0].Intake, raws[0].Reason)
	}
	if raws[0].BodyBytes != 17 {
		t.Errorf("body_bytes: got %d, want 17 — the bytes we could not read", raws[0].BodyBytes)
	}
	// Nothing may have been stored as if it had decoded.
	if n := len(Rows[storage.ProcessRow](node)); n != 0 {
		t.Errorf("%d process rows from an undecodable frame, want 0", n)
	}
}

// The frame's type byte decides the body, not the path, so the two can
// disagree — a misconfigured agent, or a type we have not met. The bytes are
// kept rather than logged and dropped.
func TestProcessUnexpectedTypeOnRouteGoesToRawPayloads(t *testing.T) {
	a, node := newTestServer(t)
	e := newTestEngine(t, a, a.routeProcess)

	// A discovery frame posted to the collector route: decodes cleanly, but
	// that route has no table for it.
	postProcessFrame(t, e, "/api/v1/collector",
		encodeProcessFrame(t, process.TypeCollectorProcDiscovery, &process.CollectorProcDiscovery{HostName: "host-a"}))

	raws := Rows[storage.RawPayloadRow](node)
	if len(raws) != 1 {
		t.Fatalf("raw_payloads rows: got %d, want 1", len(raws))
	}
	if raws[0].Reason != "unexpected_shape" {
		t.Errorf("reason: got %q, want unexpected_shape", raws[0].Reason)
	}
	if n := len(Rows[storage.ProcessDiscoveryRow](node)); n != 0 {
		t.Errorf("%d discovery rows stored from the wrong route, want 0", n)
	}
}

// Without a tenant there is nowhere to put a row: tenant_id is the first
// ORDER BY column of every table, so a made-up value writes rows no query
// looks at. The agent still gets its reply.
func TestProcessWithoutTenantStoresNothing(t *testing.T) {
	a, node := newTestServer(t)

	r := gin.New()
	r.POST("/api/v1/collector", a.HandleCollector)

	req := httptest.NewRequest(http.MethodPost, "/api/v1/collector",
		bytes.NewReader(encodeProcessFrame(t, process.TypeCollectorProc, fullCollectorProc())))
	w := httptest.NewRecorder()
	r.ServeHTTP(w, req)

	if w.Code != http.StatusOK {
		t.Errorf("got %d, want 200 — the reply contract holds with or without a tenant", w.Code)
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored without a tenant, want 0", n)
	}
}
