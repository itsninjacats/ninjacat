package storage_test

import (
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// The arity tests prove AppendTo and each INSERT agree about COUNT. Only
// ClickHouse can prove they agree about TYPES, and the process tables are
// where that is least obvious: a UUID column fed a string, a
// Map(UInt32, UInt32) fed a map[string]string, parallel arrays declared
// Array(Int32) but filled with int64 — every one of those passes the
// in-process checks and fails on the first real payload, inside a background
// flush, in a log nobody reads.
//
// So this walks the whole path once per table: scratch database, migrations
// applied from empty (which is also the only test that 0008 applies at all),
// a batch through the registered INSERT, and the values read back.
func TestProcessTablesRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	// A real timestamp, never time.Time{}: year 1 is outside DateTime64's
	// range, clamps on insert and is then dropped on the spot by the table's
	// TTL — silently, so a test built on zero-value rows asserts on rows that
	// never existed.
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	started := time.Date(2026, 9, 23, 10, 0, 0, 0, time.UTC)
	frame := uuid.New()

	storagetest.Insert(t, conn, storage.ProcessSnapshotsWriter, []storage.Row{
		storage.ProcessSnapshotRow{
			TenantID: "default", ReceivedAt: now, SnapshotID: frame,
			MessageType: 12, MessageTypeName: "CollectorProc", Path: "/api/v1/collector",
			HeaderVersion: 3, HeaderOrgID: 7, HeaderTimestamp: 4242, FrameBytes: 1024,
			HostName: "host-a", NetworkID: "vpc-1", GroupID: 1, GroupSize: 2,
			ContainerHostType: "fargateEKS",
			HostID:            5,
			HostAllTags:       map[string][]string{"kube_service": {"a", "b"}},
			HostTagsModified:  &started,
			OSName:            "linux", KernelVersion: "5.15.0",
			CPUNumbers: []int32{0, 1}, CPUMhz: []int64{2500, 2500},
			CPUVendors:   []string{"GenuineIntel", "GenuineIntel"},
			ProcessCount: 2, ContainerCount: 1,
			AgentVersion: "7.58.2", AgentHostname: "ip-10-0-1-7",
		},
	})

	storagetest.Insert(t, conn, storage.ProcessesWriter, []storage.Row{
		storage.ProcessRow{
			TenantID: "default", Timestamp: now, Host: "host-a",
			PID: 1234, Comm: "app", CreateTime: started, HasCreateTime: 1,
			Tags:       map[string][]string{"kube_service": {"a", "b"}},
			SnapshotID: frame, NsPID: 7, Key: 4242,
			Args:         []string{"/usr/bin/app", "--filter=a b"},
			CPUCoreNames: []string{"cpu0"}, CPUCorePcts: []float32{1.5},
			PortTCP: []int32{8080}, Language: "LANGUAGE_GO",
			TracerRuntimeIDs: []string{"runtime-1"}, TracerServiceNames: []string{"traced"},
			ServiceResources: `[{"Resource":{"logs":{"path":"/var/log/app.log"}}}]`,
			ByteKey:          "\x01\x02",
			NetworkID:        "vpc-1", AgentVersion: "7.58.2",
		},
	})

	storagetest.Insert(t, conn, storage.ProcessStatsWriter, []storage.Row{
		storage.ProcessStatRow{
			TenantID: "default", Timestamp: now, Host: "host-a", SnapshotID: frame,
			PID: 1234, CreateTime: started, HasCreateTime: 1,
			CPUCoreNames: []string{"cpu0"}, CPUCorePcts: []float32{2},
			ProcessState: "S", ContainerState: "running", ContainerHealth: "healthy",
			MemDirty: 8, RTNumCPUs: 8,
		},
	})

	storagetest.Insert(t, conn, storage.ContainersWriter, []storage.Row{
		storage.ContainerRow{
			TenantID: "default", Timestamp: now, Host: "host-a", SnapshotID: frame,
			Source: "collector_proc", ID: "cid-1", Name: "app", Image: "repo/app:1",
			State: "running", Health: "healthy",
			Created: &started, Started: &started,
			AddrIPs: []string{"10.0.0.1"}, AddrPorts: []int32{8080},
			AddrProtocols: []string{"tcp"},
			Tags:          map[string][]string{"kube_service": {"a", "b"}},
			ThreadCount:   12, MemoryRequest: 1 << 20,
		},
		// The same struct arriving off a Process instead of the inventory.
		storage.ContainerRow{
			TenantID: "default", Timestamp: now, Host: "host-a", SnapshotID: frame,
			Source: "process", PID: 1234, ID: "cid-1",
			// No created/started reported: NULL, not 1970.
			State: "running",
		},
	})

	storagetest.Insert(t, conn, storage.ContainerStatsWriter, []storage.Row{
		storage.ContainerStatRow{
			TenantID: "default", Timestamp: now, Host: "host-a", SnapshotID: frame,
			Source: "collector_realtime", ID: "cid-1",
			TotalPct: 15, MemLimit: 1 << 30, State: "running", Health: "healthy",
			Started: &started,
		},
	})

	storagetest.Insert(t, conn, storage.ProcessDiscoveriesWriter, []storage.Row{
		storage.ProcessDiscoveryRow{
			TenantID: "default", Timestamp: now, Host: "host-a", SnapshotID: frame,
			PID: 42, NsPID: 7, CreateTime: started, HasCreateTime: 1,
			Comm: "sh", Args: []string{"/bin/sh", "-c", "echo a b"},
			Cmdline: "/bin/sh -c echo a b", User: "root", EUID: 1,
		},
	})

	storagetest.Insert(t, conn, storage.ConnectionsPayloadsWriter, []storage.Row{
		storage.ConnectionsPayloadRow{
			TenantID: "default", ReceivedAt: now, PayloadID: frame,
			HostName: "host-a", Architecture: "amd64", KernelVersion: "5.15.0",
			ContainerForPID: map[int32]string{1: "cid-1"},
			HostTags:        map[string][]string{"role": {"db", "cache"}},
			ConnTelemetry:   map[string]int64{"kprobes_triggered": 9},
			CORETelemetry:   map[string]string{"tcp": "SuccessEmbeddedBTF"},
			Domains:         []string{"example.com"},
			DNSNames:        []string{"example.com"},
			Routes:          `[{"subnet_alias":"subnet-a","hardware_addr":""}]`,
			EncodedTags:     "\x02\x00\x00",
			ConnectionCount: 2,
		},
	})

	storagetest.Insert(t, conn, storage.ConnectionsWriter, []storage.Row{
		storage.ConnectionRow{
			TenantID: "default", Timestamp: now, Host: "host-a", PayloadID: frame,
			PID: 1, LaddrIP: "10.0.0.1", LaddrPort: 5000,
			RaddrIP: "10.0.0.2", RaddrPort: 443,
			Family: "v4", Type: "tcp", Direction: "outgoing",
			ProtocolStack:        []string{"protocolTLS", "protocolHTTP2"},
			DNSCountByRcode:      map[uint32]uint32{0: 5, 3: 1},
			TCPFailuresByErrCode: map[uint32]uint32{110: 2},
			TagsIndices:          []uint32{1, 2},
			Tags:                 map[string][]string{"kube_service": {"a", "b"}},
			HTTPAggregations:     "\x0a\x0b",
			LastTCPRtoCount:      4, TCPEcnNegotiated: 1,
		},
	})

	for table, want := range map[string]uint64{
		"process_snapshots":    1,
		"processes":            1,
		"process_stats":        1,
		"containers":           2,
		"container_stats":      1,
		"process_discoveries":  1,
		"connections_payloads": 1,
		"connections":          1,
	} {
		if got := storagetest.Count(t, conn, table); got != want {
			t.Errorf("%s: %d rows, want %d", table, got, want)
		}
	}

	// The values, not just the counts. Parallel arrays, a multiset tag map and
	// a UUID all read back as sent.
	row := storagetest.QueryRow(t, conn,
		"SELECT snapshot_id, args, cpu_core_names, tags['kube_service'], language FROM processes")
	if len(row) != 5 {
		t.Fatalf("SELECT returned %d columns, want 5", len(row))
	}
	if got, ok := row[0].(uuid.UUID); !ok || got != frame {
		t.Errorf("snapshot_id: got %v (%T), want %v", row[0], row[0], frame)
	}
	if got, ok := row[1].([]string); !ok || len(got) != 2 || got[1] != "--filter=a b" {
		t.Errorf("args: got %v — the argument with a space must survive as one element", row[1])
	}
	if got, ok := row[2].([]string); !ok || len(got) != 1 || got[0] != "cpu0" {
		t.Errorf("cpu_core_names: got %v", row[2])
	}
	if got, ok := row[3].([]string); !ok || len(got) != 2 {
		t.Errorf("tags['kube_service']: got %v, want two values — the multiset shape", row[3])
	}
	if row[4] != "LANGUAGE_GO" {
		t.Errorf("language: got %v, want the enum NAME", row[4])
	}

	// Absent is not zero: the container with no reported times reads NULL,
	// not 1970-01-01.
	nul := storagetest.QueryRow(t, conn,
		"SELECT created, started FROM containers WHERE source = 'process'")
	for i, v := range nul {
		p, ok := v.(*time.Time)
		if !ok {
			t.Fatalf("column %d scanned as %T, want *time.Time (Nullable)", i, v)
		}
		if p != nil {
			t.Errorf("column %d: got %v, want NULL — an unreported time must not become a date", i, *p)
		}
	}

	// The per-connection maps keep their integer key types end to end.
	cm := storagetest.QueryRow(t, conn,
		"SELECT dns_count_by_rcode[3], tcp_failures_by_err_code[110], protocol_stack FROM connections")
	if cm[0] != uint32(1) || cm[1] != uint32(2) {
		t.Errorf("connection maps: got %v / %v, want 1 / 2", cm[0], cm[1])
	}
	if got, ok := cm[2].([]string); !ok || len(got) != 2 || got[0] != "protocolTLS" {
		t.Errorf("protocol_stack: got %v — order is the stack", cm[2])
	}

	// The two halves of a connections frame must find each other.
	joined := storagetest.QueryRow(t, conn,
		"SELECT count() FROM connections c INNER JOIN connections_payloads p ON c.payload_id = p.payload_id")
	if joined[0] != uint64(1) {
		t.Errorf("connections joined to their payload: got %v, want 1", joined[0])
	}
}
