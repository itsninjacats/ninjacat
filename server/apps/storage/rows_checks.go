package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// check_runs — service check results.

type WriteCheckRuns struct{ Runs []CheckRunRow }

func (m WriteCheckRuns) rows() []Row { return toRows(m.Runs) }

// CheckRunRow is one service check result in ninjacat.check_runs.
// Status is an Enum8 in ClickHouse, so it travels as its name, not its number.
type CheckRunRow struct {
	TenantID  string
	Timestamp time.Time
	CheckName string
	Host      string
	Status    string // OK / WARNING / CRITICAL / UNKNOWN
	Message   string
	Tags      map[string][]string
}

func (r CheckRunRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.CheckName, r.Host,
		r.Status, r.Message, orEmpty(r.Tags))
}

func init() {
	registerWriter(ChecksWriter, WriterConfig{
		Name: "check_runs",
		Insert: `INSERT INTO check_runs
			(tenant_id, timestamp, check_name, host, status, message, tags)`,
		// Check runs trickle in, so a small threshold and a longer timer.
		MaxRows: 500, FlushInterval: 5 * time.Second,
	}, CheckRunRow{})
}
