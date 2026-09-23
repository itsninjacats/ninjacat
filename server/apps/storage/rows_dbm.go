package storage

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// dbm_events — every Database Monitoring track, one table.
//
// WHY one table for six tracks (dbmmetrics, dbmactivity, databasequery,
// dbmmetadata, dbmhealth, dbmcolumnstatistics): the engine-specific row
// arrays each track carries (oracle_rows, oracle_activity, postgres_rows,
// mysql_activity, ...) have NO Go type anywhere in this module — the only
// in-repo producer (the oracle corecheck) is //go:build oracle and excluded,
// and every other engine is a Python integration this workspace never sees.
// A table per engine-variant would mean inventing a schema for data we
// cannot decode, which is exactly the antipattern the header of
// 0001_initial.sql warns against, just aimed at engines instead of metrics.
//
// So the lossless design is: decode the envelope fields every track agrees
// on into real columns (intake/router_dbm.go's dbmEnvelope), and keep the
// COMPLETE event verbatim in `event` — the only representation of the
// engine-specific rows that cannot go stale as new engines/integrations ship.
// `extra_keys`/`undecoded_keys` make the undeclared and malformed parts of
// an event visible without a SELECT into the JSON blob.
const DBMEventsWriter = gen.Atom("storage_dbm_events")

// WriteDBMEvents carries a batch of decoded DBM events. Concrete slice, never
// []Row — see the note in messages.go.
type WriteDBMEvents struct{ Events []DBMEventRow }

func (m WriteDBMEvents) rows() []Row { return toRows(m.Events) }

// DBMEventRow is one DBM event, envelope fields as columns plus the raw
// event for everything else.
type DBMEventRow struct {
	TenantID   string
	ReceivedAt time.Time // arrival time — always present, unlike Timestamp below

	// Track is the route the event arrived on: dbmmetrics, dbmactivity,
	// databasequery, dbmmetadata, dbmhealth or dbmcolumnstatistics.
	Track string

	// Timestamp is the envelope's own "timestamp" (unix ms), nil when the
	// wire never sent one — the events that do carry it disagree by track on
	// whether it can be 0, so absent stays absent rather than becoming 1970.
	Timestamp *time.Time

	Host             string // DB host, not the agent's
	DatabaseInstance string
	AgentHostname    string // "ddagenthostname" (metrics only)
	AgentVersion     string // merged "ddagentversion" / "agent_version"
	Source           string // "ddsource"
	DBMType          string // "dbm_type": fqt | plan | activity
	Kind             string // "kind": lock_metrics, database_instance, ...
	DBMS             string
	DBMSVersion      string

	// CollectionInterval is nil when neither "collection_interval" nor
	// "min_collection_interval" was sent — 0 is a value an integration could
	// legitimately report and must not be confused with "not reported".
	CollectionInterval *float64

	// Tags is the multiset built from the envelope's merged ddtags+tags —
	// see docs/decisions/0001-tags-are-a-multiset.md.
	Tags map[string][]string

	// ExtraKeys is the sorted list of keys dbmEnvelope could not place in a
	// declared field — the engine-specific row arrays live here by name
	// (oracle_rows, postgres_activity, ...) without needing a Go type for
	// their contents.
	ExtraKeys []string

	// UndecodedKeys is the (smaller, and usually empty) list of keys whose
	// value did not fit its declared field — kept apart from ExtraKeys
	// because these are envelope fields with a shape ninjacat did not
	// expect, not engine payloads ninjacat never tried to type.
	UndecodedKeys []string

	// Event is the complete event exactly as received, byte for byte. This
	// is the only lossless representation of the engine-specific rows —
	// everything a future per-engine decoder would need is here.
	Event string

	// The query-sample convenience columns, pulled from the "db" object on
	// databasequery events (dbmQuerySample/dbmQueryPlan in
	// intake/router_dbm.go's log helper, promoted here to real columns).
	// Nil on every other track, and on a databasequery event whose "db"
	// object did not decode.
	DBInstance          *string
	QuerySignature      *string
	Statement           *string
	PlanSignature       *string
	PlanDefinitionSteps *uint32
}

// AppendTo must match the INSERT column list registered below, argument for
// argument — see storagetest's arity test.
func (r DBMEventRow) AppendTo(b driver.Batch) error {
	return b.Append(
		r.TenantID, r.ReceivedAt, r.Track, r.Timestamp,
		r.Host, r.DatabaseInstance, r.AgentHostname, r.AgentVersion,
		r.Source, r.DBMType, r.Kind, r.DBMS, r.DBMSVersion,
		r.CollectionInterval, orEmpty(r.Tags),
		orEmptySlice(r.ExtraKeys), orEmptySlice(r.UndecodedKeys),
		r.Event,
		r.DBInstance, r.QuerySignature, r.Statement, r.PlanSignature, r.PlanDefinitionSteps,
	)
}

func init() {
	registerWriter(DBMEventsWriter, WriterConfig{
		Name: "dbm_events",
		Insert: `INSERT INTO dbm_events
			(tenant_id, received_at, track, timestamp,
			 host, database_instance, agent_hostname, agent_version,
			 source, dbm_type, kind, dbms, dbms_version,
			 collection_interval, tags,
			 extra_keys, undecoded_keys,
			 event,
			 db_instance, query_signature, statement, plan_signature, plan_definition_steps)`,
		// Bulky-document-like (a row carries the WHOLE raw event, and an
		// activity/query-sample event can itself hold hundreds of
		// oracle_activity/oracle_rows entries) — same caution as
		// k8s_manifests on the buffer — but one POST batches many DBM
		// events at once, unlike one manifest per document, so MaxRows and
		// FlushInterval get more headroom than the manifest table needs.
		MaxRows: 500, FlushInterval: 5 * time.Second,
		BufferLimit: 20_000, MaxInFlight: 2,
	}, DBMEventRow{})
	registerTypes(WriteDBMEvents{}, DBMEventRow{})
}
