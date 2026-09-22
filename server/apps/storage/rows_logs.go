package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// logs — the highest-volume table in the server.

type WriteLogs struct{ Entries []LogRow }

func (m WriteLogs) rows() []Row { return toRows(m.Entries) }

// LogRow is one log entry in ninjacat.logs.
type LogRow struct {
	TenantID  string
	Timestamp time.Time
	Host      string
	Service   string
	Source    string
	Status    string
	Message   string
	Tags      map[string][]string
}

func (r LogRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.Service,
		r.Source, r.Status, r.Message, orEmpty(r.Tags))
}

func init() {
	registerWriter(LogsWriter, WriterConfig{
		Name: "logs",
		Insert: `INSERT INTO logs
			(tenant_id, timestamp, host, service, source, status, message, tags)`,
		// Logs are the high-volume table: bigger batches, bigger safety margin.
		MaxRows: 20000, FlushInterval: time.Second, BufferLimit: 500_000,
	}, LogRow{})
}
