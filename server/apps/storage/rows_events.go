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
}

func (r EventRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.EventID, r.EventIDNum,
		r.Title, r.Text, r.Host, r.AlertType, r.Priority,
		r.AggregationKey, r.SourceTypeName, r.DeviceName, orEmpty(r.Tags))
}

func init() {
	registerWriter(EventsWriter, WriterConfig{
		Name: "events",
		Insert: `INSERT INTO events
			(tenant_id, timestamp, event_id, event_id_num, title, text, host,
			 alert_type, priority, aggregation_key, source_type_name, device_name, tags)`,
		// Human and system scale, not machine scale: tens or hundreds a day.
		// Small batches, and a timer short enough that a deploy marker shows
		// up on the chart while someone is still looking at it.
		MaxRows: 200, FlushInterval: 2 * time.Second,
	}, EventRow{})
}
