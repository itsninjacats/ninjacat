package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/google/uuid"
)

// events — the vertical lines on a chart.

type WriteEvents struct{ Events []EventRow }

func (m WriteEvents) rows() []Row { return toRows(m.Events) }

// EventRow is one event in ninjacat.events: a deploy, a restart, a config
// change — something you draw on a chart as a vertical line.
//
// EventIDNum exists because Datadog's API hands the caller back a numeric
// event id, and clients store it to build links. We keep both: a UUID for
// ourselves and the numeric form we already returned.
//
// Two sources feed this table: the public /api/v1/events request, and the
// agent's own events riding on /intake/. They fill different subsets — the
// agent sends an event_type and no related_event_id, the public API the
// reverse — which is why the columns of both are here rather than in two
// tables that would have to be UNIONed on every chart.
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
	Tags           map[string][]string

	// EventType is the agent's pkg/metrics/event.Event.EventType. Only the
	// /intake/ source sends it.
	EventType string

	// RelatedEventID is a pointer because Datadog's id space starts at 1 and
	// "no parent" must not arrive in the column as 0.
	RelatedEventID *int64

	// The alert type and priority EXACTLY as the sender wrote them, before
	// the coercion that fills AlertType/Priority. Empty when nothing was sent.
	// The coercion is deliberate — an invented alert type must not cost us the
	// event — but it used to be silent, and a coercion nobody can see is
	// indistinguishable from data that was never sent.
	AlertTypeRaw string
	PriorityRaw  string

	// Extra holds undeclared keys, values JSON-encoded.
	Extra map[string]string
}

func (r EventRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.EventID, r.EventIDNum,
		r.Title, r.Text, r.Host, r.AlertType, r.Priority,
		r.AggregationKey, r.SourceTypeName, r.DeviceName, orEmpty(r.Tags),
		r.EventType, r.RelatedEventID, r.AlertTypeRaw, r.PriorityRaw, orEmpty(r.Extra))
}

func init() {
	registerWriter(EventsWriter, WriterConfig{
		Name: "events",
		Insert: `INSERT INTO events
			(tenant_id, timestamp, event_id, event_id_num, title, text, host,
			 alert_type, priority, aggregation_key, source_type_name, device_name, tags,
			 event_type, related_event_id, alert_type_raw, priority_raw, extra)`,
		// Human and system scale, not machine scale: tens or hundreds a day.
		// Small batches, and a timer short enough that a deploy marker shows
		// up on the chart while someone is still looking at it.
		MaxRows: 200, FlushInterval: 2 * time.Second,
	}, EventRow{})
}
