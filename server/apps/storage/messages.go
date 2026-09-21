package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/google/uuid"
)

// Message types accepted by the writer actors.
//
// These are deliberately independent of the HTTP layer: storage knows nothing
// about Gin or Datadog wire formats. Handlers translate their own structures
// into these and send them. That way a second source (OTLP) can be plugged in
// without touching the write path.

// Row is anything that knows how to add itself to a prepared ClickHouse batch.
//
// This is what lets a single BatchWriter implementation serve every table:
// the writer owns buffering, flushing and backpressure, while each row type
// owns its own column order.
type Row interface {
	AppendTo(batch driver.Batch) error
}

// Messages sent to writers. Delivered with Send (asynchronous), so the handler
// never waits — the agent gets its 202 immediately.
//
// Why an actor instead of writing straight from the handler:
//
//   - ClickHouse dies from many small INSERTs ("Too many parts"). Something has
//     to buffer, and an actor processes messages one at a time, so the buffer
//     needs no locking.
//   - This is the single point every datapoint passes through — the natural
//     home for cardinality limits and tag stripping later on.
//
// Note this is the OPPOSITE of the API key situation. There the read is
// synchronous and blocks the response, so it goes through an atomic.Pointer.
// Here the write is asynchronous and can lag — an actor is ideal.
//
// IMPORTANT: each message carries a CONCRETE slice, not []Row.
//
// A single generic message with an interface field looks tidier, but Ergo
// cannot serialize it for delivery to another node:
//
//	RegisterTypes: unresolvable types: storage.WriteRows
//
// The encoder has to know what is inside the slice, and an interface does not
// tell it. Since the whole point of splitting storage into its own application
// is to run it on a separate node one day, the wire format must stay concrete.
// The Row interface still does its job — inside the writer's buffer, where
// nothing is serialized.

type WriteMetrics struct{ Points []MetricPoint }
type WriteSketches struct{ Sketches []SketchRow }
type WriteCheckRuns struct{ Runs []CheckRunRow }
type WriteLogs struct{ Entries []LogRow }
type WriteHosts struct{ Hosts []HostRow }
type WriteProcesses struct{ Processes []ProcessRow }
type WriteEvents struct{ Events []EventRow }

// rowsProvider lets BatchWriter stay generic despite the concrete messages:
// it asks the message for its rows instead of knowing the types itself.
// This interface is never serialized — it is only used locally, after the
// message has already arrived as a concrete value.
type rowsProvider interface {
	rows() []Row
}

func (m WriteMetrics) rows() []Row   { return toRows(m.Points) }
func (m WriteSketches) rows() []Row  { return toRows(m.Sketches) }
func (m WriteCheckRuns) rows() []Row { return toRows(m.Runs) }
func (m WriteLogs) rows() []Row      { return toRows(m.Entries) }
func (m WriteHosts) rows() []Row     { return toRows(m.Hosts) }
func (m WriteProcesses) rows() []Row { return toRows(m.Processes) }
func (m WriteEvents) rows() []Row    { return toRows(m.Events) }

func toRows[T Row](items []T) []Row {
	out := make([]Row, len(items))
	for i, it := range items {
		out[i] = it
	}
	return out
}

// flush tells a writer to drain its buffer regardless of size.
// Sent periodically by a timer.
type flush struct{}

// ---------------------------------------------------------------------------
// Row types, one per table
// ---------------------------------------------------------------------------

// MetricPoint is one row in ninjacat.metrics.
type MetricPoint struct {
	TenantID   string
	Timestamp  time.Time
	Metric     string
	Host       string
	MetricType string // GAUGE / COUNT / RATE
	SourceType string
	Unit       string
	Interval   uint32
	Value      float64
	Tags       map[string]string
}

func (r MetricPoint) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Metric, r.Host,
		r.MetricType, r.SourceType, r.Unit, r.Interval, r.Value, orEmpty(r.Tags))
}

// SketchRow is one DDSketch in ninjacat.sketches. Distribution metrics arrive
// as bucketed histograms rather than single values — see docs/datadog-agent.md.
type SketchRow struct {
	TenantID     string
	Timestamp    time.Time
	Metric       string
	Host         string
	Tags         map[string]string
	Count        uint64
	Min          float64
	Max          float64
	Avg          float64
	Sum          float64
	BucketKeys   []int32
	BucketCounts []uint32
}

func (r SketchRow) AppendTo(b driver.Batch) error {
	keys := r.BucketKeys
	if keys == nil {
		keys = []int32{}
	}
	counts := r.BucketCounts
	if counts == nil {
		counts = []uint32{}
	}
	return b.Append(r.TenantID, r.Timestamp, r.Metric, r.Host, orEmpty(r.Tags),
		r.Count, r.Min, r.Max, r.Avg, r.Sum, keys, counts)
}

// CheckRunRow is one service check result in ninjacat.check_runs.
// Status is an Enum8 in ClickHouse, so it travels as its name, not its number.
type CheckRunRow struct {
	TenantID  string
	Timestamp time.Time
	CheckName string
	Host      string
	Status    string // OK / WARNING / CRITICAL / UNKNOWN
	Message   string
	Tags      map[string]string
}

func (r CheckRunRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.CheckName, r.Host,
		r.Status, r.Message, orEmpty(r.Tags))
}

// LogRow is one log entry in ninjacat.logs.
type LogRow struct {
	TenantID  string
	Timestamp time.Time
	Host      string
	Service   string
	Source    string
	Status    string
	Message   string
	Tags      map[string]string
}

func (r LogRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host, r.Service,
		r.Source, r.Status, r.Message, orEmpty(r.Tags))
}

// HostRow is host metadata from /intake/, stored in ninjacat.hosts.
// The table is a ReplacingMergeTree keyed on (tenant_id, host), so repeated
// writes for the same host collapse to the newest one.
type HostRow struct {
	TenantID     string
	Host         string
	SeenAt       time.Time
	AgentVersion string
	OS           string
	Platform     map[string]string
	CPU          map[string]string
	Memory       map[string]string
	Tags         []string
}

func (r HostRow) AppendTo(b driver.Batch) error {
	tags := r.Tags
	if tags == nil {
		tags = []string{}
	}
	return b.Append(r.TenantID, r.Host, r.SeenAt, r.AgentVersion, r.OS,
		orEmpty(r.Platform), orEmpty(r.CPU), orEmpty(r.Memory), tags)
}

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
	Tags        map[string]string
}

func (r ProcessRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Host,
		r.PID, r.PPID, r.User, r.Comm, r.Exe, r.Cmdline,
		r.RSS, r.VMS, r.CPUPct, r.Threads, r.OpenFDs,
		r.State, r.CreateTime, r.ContainerID, orEmpty(r.Tags))
}

// EventRow is one event in ninjacat.events: a deploy, a restart, a config
// change — something you draw on a chart as a vertical line.
//
// EventIDNum exists because Datadog's API hands the caller back a numeric
// event id, and clients store it to build links. We keep both: a UUID for
// ourselves and the numeric form we already returned.
type EventRow struct {
	TenantID       string
	Timestamp      time.Time
	EventID        uuid.UUID
	EventIDNum     uint64
	Title          string
	Text           string
	Host           string
	AlertType      string // error / warning / info / success / user_update / recommendation / snapshot
	Priority       string // normal / low
	AggregationKey string
	SourceTypeName string
	DeviceName     string
	Tags           map[string]string
}

func (r EventRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.EventID, r.EventIDNum,
		r.Title, r.Text, r.Host, r.AlertType, r.Priority,
		r.AggregationKey, r.SourceTypeName, r.DeviceName, orEmpty(r.Tags))
}

// orEmpty guards against nil maps — the ClickHouse driver rejects them.
func orEmpty(m map[string]string) map[string]string {
	if m == nil {
		return map[string]string{}
	}
	return m
}
