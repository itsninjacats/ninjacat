package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// agent_flares — POST /support/flare, registered on every intake host (see
// intake/server.go's engineAuth and the routes.go comment on the
// "-flare.agent." rewrite). Schema: schema/migrations/0016_agent_flares.sql.
// Docs: docs/tables/flares.md.
//
// A flare is a support artifact somebody explicitly asked for, not
// telemetry, which is why it gets its own table rather than riding
// raw_payloads: the archive is the point, not a debugging fallback.

// AgentFlaresWriter is the process HandleFlare sends WriteAgentFlares to.
const AgentFlaresWriter = gen.Atom("storage_agent_flares")

// WriteAgentFlares carries a batch of flare uploads. Concrete slice, never
// []Row — see the note in messages.go.
type WriteAgentFlares struct{ Flares []AgentFlareRow }

func (m WriteAgentFlares) rows() []Row { return toRows(m.Flares) }

// AgentFlareRow is one flare upload.
//
// CaseID is kept as the string the agent sent (empty means "new case, no id
// assigned yet" — see the migration's header comment), not parsed to an
// integer: the response body's numeric case_id is a separate, possibly
// generated, value (intake/server.go's flareCaseIDNumber) and belongs to the
// HTTP contract, not to what was stored.
type AgentFlareRow struct {
	TenantID     string
	ReceivedAt   time.Time
	Hostname     string
	CaseID       string
	Email        string
	Source       string
	AgentVersion string
	Filename     string
	SizeBytes    uint64

	// Fields holds every multipart field that is not one of the named
	// columns above — rc_task_uuid today, whatever a future agent version
	// adds tomorrow, kept rather than dropped.
	Fields map[string]string

	// Archive is the flare zip itself, verbatim.
	Archive string
}

// AppendTo must match the INSERT column list registered below, argument for
// argument. Nothing checks that at compile time; storagetest's arity test
// does it at test time, off the zero row handed to registerWriter.
func (r AgentFlareRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.ReceivedAt, r.Hostname, r.CaseID, r.Email,
		r.Source, r.AgentVersion, r.Filename, r.SizeBytes,
		orEmpty(r.Fields), r.Archive)
}

func init() {
	registerWriter(AgentFlaresWriter, WriterConfig{
		Name: "agent_flares",
		Insert: `INSERT INTO agent_flares
			(tenant_id, received_at, hostname, case_id, email, source, agent_version,
			 filename, size_bytes, fields, archive)`,
		// Bulky documents, like k8s_manifests and raw_payloads: a row is a
		// whole flare zip, so these counts stand for megabytes rather than
		// rows. A flare is also rare — someone asked for it — so there is no
		// pressure to batch aggressively; the tight ceiling and single
		// in-flight flush exist purely to bound memory against an
		// unexpectedly large archive.
		MaxRows: 200, FlushInterval: 10 * time.Second,
		BufferLimit: 5_000, MaxInFlight: 1,
	}, AgentFlareRow{})
	registerTypes(WriteAgentFlares{}, AgentFlareRow{})
}
