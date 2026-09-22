package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// hosts — host metadata, the one "current state" table fed directly by a
// writer rather than by a materialized view.

type WriteHosts struct{ Hosts []HostRow }

func (m WriteHosts) rows() []Row { return toRows(m.Hosts) }

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
	Tags         map[string][]string
}

func (r HostRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Host, r.SeenAt, r.AgentVersion, r.OS,
		orEmpty(r.Platform), orEmpty(r.CPU), orEmpty(r.Memory), orEmpty(r.Tags))
}

func init() {
	registerWriter(HostsWriter, WriterConfig{
		Name: "hosts",
		Insert: `INSERT INTO hosts
			(tenant_id, host, seen_at, agent_version, os, platform, cpu, memory, tags)`,
		// One row per host every ~20s. ReplacingMergeTree collapses duplicates.
		MaxRows: 100, FlushInterval: 10 * time.Second,
	}, HostRow{})
}
