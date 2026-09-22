package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// processes — process-agent snapshots.

type WriteProcesses struct{ Processes []ProcessRow }

func (m WriteProcesses) rows() []Row { return toRows(m.Processes) }

// ProcessRow is one process from a process-agent snapshot, stored in
// ninjacat.processes.
//
// Snapshots are BULKY: a few hundred processes per host every ~10s. The table
// has a shorter TTL than metrics for that reason.
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
}

func (r ProcessRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host,
		r.PID, r.PPID, r.User, r.Comm, r.Exe, r.Cmdline,
		r.RSS, r.VMS, r.CPUPct, r.Threads, r.OpenFDs,
		r.State, r.CreateTime, r.ContainerID, orEmpty(r.Tags))
}

func init() {
	registerWriter(ProcessesWriter, WriterConfig{
		Name: "processes",
		Insert: `INSERT INTO processes
			(tenant_id, timestamp, host, pid, ppid, user, comm, exe, cmdline,
			 rss, vms, cpu_pct, threads, open_fds, state, create_time, container_id, tags)`,
		// A few hundred rows per host per snapshot, every ~10s. Large batches,
		// moderate interval.
		MaxRows: 10000, FlushInterval: 3 * time.Second,
	}, ProcessRow{})
}
