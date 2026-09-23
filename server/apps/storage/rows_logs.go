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

	// Attributes is every attribute the sender included that did NOT become a
	// column, as JSON text for a native JSON column. dd.trace_id lives here,
	// and it is the one key that joins a log line to its trace — counting it,
	// which is what this handler used to do, was the worst available outcome.
	//
	// Empty is written as "{}" by AppendTo: a JSON column rejects an empty
	// string, and "no attributes" is an empty object either way.
	Attributes string

	// TimestampSource names which wire field produced Timestamp:
	// "timestamp_ms", "timestamp_string", "date_ms", "date_string", or
	// "arrival" when the sender gave none we could read. Without it, "no
	// timestamp was sent" and "a timestamp was sent in a form we failed to
	// parse" are the same row — and only the second is a bug.
	TimestampSource string
}

func (r LogRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.Service,
		r.Source, r.Status, r.Message, orEmpty(r.Tags),
		orJSONObject(r.Attributes), r.TimestampSource)
}

func init() {
	registerWriter(LogsWriter, WriterConfig{
		Name: "logs",
		Insert: `INSERT INTO logs
			(tenant_id, timestamp, host, service, source, status, message, tags,
			 attributes, timestamp_source)`,
		// Logs are the high-volume table: bigger batches, bigger safety margin.
		MaxRows: 20000, FlushInterval: time.Second, BufferLimit: 500_000,
	}, LogRow{})
}
