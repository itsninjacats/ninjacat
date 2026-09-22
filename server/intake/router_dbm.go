package intake

import (
	"encoding/json"
	"fmt"
	"log"
	"math"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
)

// dbm-metrics-intake.<site> — Database Monitoring.
//
//	config: none — event platform, see router_containers.go.
//
// Six tracks on one host, all JSON. They were separate intakes once; the
// agent's own comment (comp/forwarder/eventplatform/impl/pipelines_dbm.go)
// says "all of our data now flows through the same intake".
//
// The body is a JSON array (logs-library arraySerializer) of events the agent
// never parses: Python integrations hand bytes to SubmitEventPlatformEvent and
// the forwarder batches them. Each event is decoded into dbmEnvelope below;
// the engine-specific remainder stays raw.
//
// Storage: every track lands in one table, storage.DBMEventsWriter /
// dbm_events (schema/migrations/0006_dbm.sql) — see that migration's header
// for why one table serves six tracks and every database engine. A body
// that is not a JSON array/object, or an element that fails to decode, goes
// to storeRaw instead (intake/raw.go) rather than being dropped.
func (a *Server) routeDBM(g *gin.RouterGroup) {
	g.POST("/api/v2/dbmmetrics", a.handleDBMMetrics)             // database metrics
	g.POST("/api/v2/dbmactivity", a.handleDBMActivity)           // active sessions
	g.POST("/api/v2/databasequery", a.handleDBMQuery)            // query samples
	g.POST("/api/v2/dbmmetadata", a.handleDBMMetadata)           // schemas
	g.POST("/api/v2/dbmhealth", a.handleDBMHealth)               // integration health
	g.POST("/api/v2/dbmcolumnstatistics", a.handleDBMColumnStat) // column statistics
}

// dbmEnvelope is the part of a DBM event that is common to every track.
//
// Why our own type: the rule "use Datadog's types, never invent a schema"
// holds where the agent publishes one. For DBM it does not — the producers are
// Python integrations (integrations-core), and the single Go producer, the
// oracle corecheck in pkg/collector/corechecks/oracle, is behind
// //go:build oracle and cannot be imported. This struct therefore transcribes
// the fields those oracle structs agree on, nothing more:
//
//   - statements.go  FQTPayload, PlanPayload, MetricsPayload (databasequery, dbmmetrics)
//   - activity_structs.go  ActivitySnapshot + Metadata     (dbmactivity)
//   - locks.go  metricsPayload                             (dbmmetrics, kind=lock_metrics)
//   - metadata.go  dbInstanceEvent                         (dbmmetadata, kind=database_instance)
//
// The wire is not consistent across tracks, and the envelope absorbs that:
//
//   - tags come as "ddtags" (comma-joined string on samples, string array on
//     activity) or as "tags" (string array on metrics/metadata) — see dbmTags;
//   - the agent version is "ddagentversion" everywhere except dbmmetadata,
//     which says "agent_version";
//   - "ddsource" and "dbm_type" exist on samples/activity only; metrics and
//     metadata use "kind" instead.
//
// Every field is optional. Keys not listed here, and keys whose value does not
// fit the field, are kept verbatim in Extra so no payload is ever lost.
//
// dbmhealth and dbmcolumnstatistics have no producer in the agent clone (only
// the forwarder pipeline and two release notes mention them), so nothing
// beyond this envelope can be claimed about them; they get the generic read.
type dbmEnvelope struct {
	// Timestamp is "timestamp", Unix milliseconds, kept as the number on the
	// wire: the oracle corecheck sends float64(UnixMilli()), the Python
	// integrations time.time()*1000 with a fraction. Empty means not sent.
	Timestamp        json.Number
	Host             string // "host": the database host, not the agent's
	DatabaseInstance string // "database_instance"
	AgentHostname    string // "ddagenthostname" (metrics only)
	AgentVersion     string // "ddagentversion", or "agent_version" on dbmmetadata
	Source           string // "ddsource" (samples, activity)
	DBMType          string // "dbm_type": fqt | plan (samples), activity
	Kind             string // "kind": lock_metrics, database_instance, ...
	DBMS             string // "dbms" (metadata)
	DBMSVersion      string // "dbms_version" (metadata)

	// CollectionInterval is "collection_interval" (activity, metadata) or
	// "min_collection_interval" (metrics), whichever the track sends. A
	// pointer, not a bare float64: a genuine 0s interval and "neither key was
	// sent" are different facts, and a storage column downstream is Nullable
	// specifically to keep that distinction (see 0006_dbm.sql).
	CollectionInterval *float64

	// Tags merges "ddtags" and "tags", normalized to a list.
	Tags dbmTags

	// Extra holds everything the envelope does not describe: engine-specific
	// row arrays (oracle_rows, oracle_activity, postgres_rows, ...), the "db"
	// object on samples, and any key whose value did not decode (listed in
	// Undecoded). Nothing is dropped.
	Extra map[string]json.RawMessage

	// Undecoded lists envelope keys whose value had an unexpected shape; their
	// raw value stays in Extra.
	Undecoded []string
}

// dbmTags accepts both spellings the agent uses for a tag list: a
// comma-joined string ("env:prod,team:db") and a JSON string array.
type dbmTags []string

func (t *dbmTags) UnmarshalJSON(data []byte) error {
	if string(data) == "null" {
		*t = nil
		return nil
	}
	if len(data) > 0 && data[0] == '"' {
		var s string
		if err := json.Unmarshal(data, &s); err != nil {
			return err
		}
		*t = nil
		for _, tag := range strings.Split(s, ",") {
			if tag = strings.TrimSpace(tag); tag != "" {
				*t = append(*t, tag)
			}
		}
		return nil
	}
	var list []string
	if err := json.Unmarshal(data, &list); err != nil {
		return err
	}
	*t = list
	return nil
}

// UnmarshalJSON decodes the envelope field by field so that one malformed
// value cannot take the others down with it: a key that fails to decode is
// recorded in Undecoded and left in Extra together with every unknown key.
func (e *dbmEnvelope) UnmarshalJSON(data []byte) error {
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	take := func(key string, dst any) {
		raw, ok := fields[key]
		if !ok {
			return
		}
		if err := json.Unmarshal(raw, dst); err != nil {
			e.Undecoded = append(e.Undecoded, key)
			return
		}
		delete(fields, key)
	}

	take("timestamp", &e.Timestamp)
	take("host", &e.Host)
	take("database_instance", &e.DatabaseInstance)
	take("ddagenthostname", &e.AgentHostname)
	take("ddsource", &e.Source)
	take("dbm_type", &e.DBMType)
	take("kind", &e.Kind)
	take("dbms", &e.DBMS)
	take("dbms_version", &e.DBMSVersion)

	var altVersion string
	take("ddagentversion", &e.AgentVersion)
	take("agent_version", &altVersion)
	if e.AgentVersion == "" {
		e.AgentVersion = altVersion
	}

	var minInterval *float64
	take("collection_interval", &e.CollectionInterval)
	take("min_collection_interval", &minInterval)
	if e.CollectionInterval == nil {
		e.CollectionInterval = minInterval
	}

	var tags dbmTags
	take("ddtags", &e.Tags)
	take("tags", &tags)
	e.Tags = append(e.Tags, tags...)

	e.Extra = fields
	return nil
}

// dbmQuerySample is the "db" object of a databasequery event, as the oracle
// FQTDB and PlanDB structs (statements.go) agree on it. The statement metadata
// is left raw: FQTDBMetadata says dd_tables/dd_commands, PlanStatementMetadata
// says tables/commands, and the Python integrations are unverified.
type dbmQuerySample struct {
	Instance       string          `json:"instance"`
	QuerySignature string          `json:"query_signature"`
	Statement      string          `json:"statement"`
	Metadata       json.RawMessage `json:"metadata"`
	Plan           *dbmQueryPlan   `json:"plan"`
}

// dbmQueryPlan is db.plan on dbm_type=plan. The definition is a list of steps
// whose columns are engine-specific (oracle PlanDefinition).
type dbmQueryPlan struct {
	Signature  string          `json:"signature"`
	Definition json.RawMessage `json:"definition"`
}

// dbmLogLimit caps per-event log lines; a batch can hold hundreds of samples.
// It limits the log only: dbmBatch decodes every event.
const dbmLogLimit = 3

// handleDBMMetrics — /api/v2/dbmmetrics: query metrics (oracle
// MetricsPayload, kind absent) and lock metrics (kind=lock_metrics). Engine
// rows sit in Extra under oracle_rows, postgres_rows, mysql_rows, ...
func (a *Server) handleDBMMetrics(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	events, ok := a.dbmBatch(c, "dbmmetrics")
	if !ok {
		return
	}
	a.storeDBMEvents(c, "dbmmetrics", events)
}

// handleDBMActivity — /api/v2/dbmactivity: activity snapshots (oracle
// ActivitySnapshot, dbm_type=activity). Sessions sit in Extra under
// oracle_activity, postgres_activity, mysql_activity, ...
func (a *Server) handleDBMActivity(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	events, ok := a.dbmBatch(c, "dbmactivity")
	if !ok {
		return
	}
	a.storeDBMEvents(c, "dbmactivity", events)
}

// handleDBMQuery — /api/v2/databasequery: query samples, dbm_type=fqt (full
// query text) or plan. The "db" object (dbmQuerySample) sits in Extra and
// also feeds the db_instance/query_signature/statement/plan_* convenience
// columns on the stored row.
func (a *Server) handleDBMQuery(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	events, ok := a.dbmBatch(c, "databasequery")
	if !ok {
		return
	}
	a.storeDBMEvents(c, "databasequery", events)
}

// handleDBMMetadata — /api/v2/dbmmetadata: kind=database_instance (oracle
// dbInstanceEvent) and schema collections; agent_version instead of
// ddagentversion. Schema payloads sit in Extra under "metadata".
func (a *Server) handleDBMMetadata(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	events, ok := a.dbmBatch(c, "dbmmetadata")
	if !ok {
		return
	}
	a.storeDBMEvents(c, "dbmmetadata", events)
}

// handleDBMHealth — /api/v2/dbmhealth: integration health. No producer in
// the agent clone; whatever is sent beyond the envelope is in Extra.
func (a *Server) handleDBMHealth(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	events, ok := a.dbmBatch(c, "dbmhealth")
	if !ok {
		return
	}
	a.storeDBMEvents(c, "dbmhealth", events)
}

// handleDBMColumnStat — /api/v2/dbmcolumnstatistics: column statistics. No
// producer in the agent clone; whatever is sent beyond the envelope is in
// Extra.
func (a *Server) handleDBMColumnStat(c *gin.Context) {
	defer c.JSON(http.StatusAccepted, gin.H{})
	// The label used to be "dbmcolstat", which meant every log line for this
	// route was tagged differently from its own path — grepping for
	// dbmcolumnstatistics found nothing. Track column and log label now
	// match the route.
	events, ok := a.dbmBatch(c, "dbmcolumnstatistics")
	if !ok {
		return
	}
	a.storeDBMEvents(c, "dbmcolumnstatistics", events)
}

// dbmEvent pairs a decoded envelope with the exact bytes it was decoded
// from — dbmEnvelope only ever describes the fields it recognizes, and the
// stored row needs the complete event verbatim (see DBMEventRow.Event).
type dbmEvent struct {
	Raw json.RawMessage
	Env dbmEnvelope
}

// dbmBatch reads the body, splits it and decodes every event into a
// dbmEnvelope. The log shows the first dbmLogLimit events; the returned slice
// holds all of them, each with its engine-specific remainder raw in Extra.
//
// A body that does not split into a JSON array or object, and an element
// that is not a JSON object, are handed to storeRaw rather than dropped —
// dbmEnvelope's own UnmarshalJSON already keeps anything IT does not
// recognize (that is what Extra/Undecoded are for), so the only things that
// reach storeRaw here are inputs dbmEnvelope could not even attempt to read.
func (a *Server) dbmBatch(c *gin.Context, label string) ([]dbmEvent, bool) {
	body, err := c.GetRawData()
	if err != nil {
		log.Printf("[%s] cannot read body: %v", label, err)
		return nil, false
	}

	raws, err := dbmEvents(body)
	if err != nil {
		log.Printf("[%s] not a JSON array (%v), raw follows", label, err)
		describe(label, c.GetHeader("Content-Type"), body)
		a.storeRaw(c, "dbm", "decode_error", fmt.Sprintf("[%s] not a JSON array or object: %v", label, err), body)
		return nil, false
	}
	log.Printf("[%s] %d events, %d B", label, len(raws), len(body))

	events := make([]dbmEvent, 0, len(raws))
	for i, raw := range raws {
		var ev dbmEnvelope
		if err := json.Unmarshal(raw, &ev); err != nil {
			log.Printf("[%s] event %d: not an object (%v), dropped", label, i, err)
			a.storeRaw(c, "dbm", "decode_error", fmt.Sprintf("[%s] event %d: not an object: %v", label, i, err), raw)
			continue
		}
		events = append(events, dbmEvent{Raw: raw, Env: ev})
		if i >= dbmLogLimit {
			continue
		}
		log.Printf("[%s] event %d %s", label, i, ev.summary())
		if rows := ev.rowCounts(); rows != "" {
			log.Printf("   rows %s", rows)
		}
		if label == "databasequery" {
			log.Printf("   sample %s", ev.sample())
		}
		if len(ev.Undecoded) > 0 {
			log.Printf("   undecoded %s", strings.Join(ev.Undecoded, ", "))
		}
		if i == 0 {
			if extra := ev.extraKeys(); len(extra) > 0 {
				log.Printf("   extra %s", strings.Join(extra, ", "))
			}
		}
	}
	if len(raws) > dbmLogLimit {
		log.Printf("[%s] ... %d more", label, len(raws)-dbmLogLimit)
	}
	return events, true
}

// storeDBMEvents converts a track's decoded batch into rows and hands them
// to storage.DBMEventsWriter. Tenant comes from the request; with no tenant
// nothing is stored, matching storeRaw's own rule (tenant_id is the first
// ORDER BY column of every table, and a row without one is invisible to
// every query — see intake/raw.go).
func (a *Server) storeDBMEvents(c *gin.Context, track string, events []dbmEvent) {
	if len(events) == 0 {
		return
	}
	tenant := TenantFromContext(c)
	if tenant == "" {
		return
	}

	receivedAt := time.Now().UTC()
	rows := make([]storage.DBMEventRow, 0, len(events))
	for _, ev := range events {
		rows = append(rows, dbmEventRow(tenant, track, receivedAt, ev))
	}
	a.store(storage.DBMEventsWriter, storage.WriteDBMEvents{Events: rows}, len(rows))
}

// dbmEventRow converts one decoded event into the row dbm_events stores.
// Pure and side-effect free so it can be table-tested directly against
// realistic per-track JSON without a server or an HTTP round trip.
func dbmEventRow(tenant, track string, receivedAt time.Time, ev dbmEvent) storage.DBMEventRow {
	env := ev.Env
	row := storage.DBMEventRow{
		TenantID:           tenant,
		ReceivedAt:         receivedAt,
		Track:              track,
		Timestamp:          dbmTimestamp(env.Timestamp),
		Host:               env.Host,
		DatabaseInstance:   env.DatabaseInstance,
		AgentHostname:      env.AgentHostname,
		AgentVersion:       env.AgentVersion,
		Source:             env.Source,
		DBMType:            env.DBMType,
		Kind:               env.Kind,
		DBMS:               env.DBMS,
		DBMSVersion:        env.DBMSVersion,
		CollectionInterval: env.CollectionInterval,
		Tags:               tagsToMultiMap(env.Tags),
		ExtraKeys:          env.extraKeys(),
		UndecodedKeys:      append([]string(nil), env.Undecoded...),
		Event:              string(ev.Raw),
	}

	if db, ok := env.Extra["db"]; ok {
		var sample dbmQuerySample
		if err := json.Unmarshal(db, &sample); err == nil {
			if sample.Instance != "" {
				row.DBInstance = &sample.Instance
			}
			if sample.QuerySignature != "" {
				row.QuerySignature = &sample.QuerySignature
			}
			if sample.Statement != "" {
				row.Statement = &sample.Statement
			}
			if sample.Plan != nil {
				if sample.Plan.Signature != "" {
					row.PlanSignature = &sample.Plan.Signature
				}
				if steps, ok := dbmPlanDefinitionSteps(sample.Plan.Definition); ok {
					row.PlanDefinitionSteps = &steps
				}
			}
		}
	}

	return row
}

// dbmPlanDefinitionSteps counts the steps in db.plan.definition. The second
// return value is false when the definition was never sent at all — a plan
// with zero steps and "no plan was sent" are different facts, and the
// caller keeps that distinction by leaving the row's pointer nil rather than
// storing 0.
func dbmPlanDefinitionSteps(definition json.RawMessage) (uint32, bool) {
	if len(definition) == 0 {
		return 0, false
	}
	var steps []json.RawMessage
	if err := json.Unmarshal(definition, &steps); err != nil {
		return 0, false
	}
	return uint32(len(steps)), true
}

// dbmTimestamp turns the envelope's wire timestamp into the value the
// Nullable(DateTime64(3)) column takes: nil when the field was never sent —
// an absent timestamp must never become 1970 or "now" — a real instant
// otherwise.
//
// The fast path parses the wire token as an integer directly, so the oracle
// corecheck's float64(UnixMilli()) — an exact integer that merely looks like
// a float on the wire, e.g. "1758326400000" — never touches float64 and
// keeps full int64 precision. Some Python integrations send a genuinely
// fractional number of milliseconds (time.time()*1000); DateTime64(3) is
// millisecond precision anyway, so that one case rounds through float64,
// which is safe here because millisecond epoch values sit far below
// float64's 2^53 exact-integer bound.
func dbmTimestamp(n json.Number) *time.Time {
	if n == "" {
		return nil
	}
	if ms, err := n.Int64(); err == nil {
		t := time.UnixMilli(ms).UTC()
		return &t
	}
	f, err := n.Float64()
	if err != nil {
		return nil
	}
	t := time.UnixMilli(int64(math.Round(f))).UTC()
	return &t
}

// dbmEvents splits the batch. A lone object is wrapped so a hand-crafted
// request still decodes the same way.
func dbmEvents(body []byte) ([]json.RawMessage, error) {
	var events []json.RawMessage
	if err := json.Unmarshal(body, &events); err == nil {
		return events, nil
	}
	var one map[string]json.RawMessage
	if err := json.Unmarshal(body, &one); err != nil {
		return nil, err
	}
	return []json.RawMessage{body}, nil
}

// summary renders the envelope fields that are actually set.
func (e *dbmEnvelope) summary() string {
	var parts []string
	add := func(key, val string) {
		if val != "" {
			parts = append(parts, key+"="+val)
		}
	}
	add("host", e.Host)
	add("instance", e.DatabaseInstance)
	add("dbms", e.DBMS)
	add("dbms_version", e.DBMSVersion)
	add("kind", e.Kind)
	add("type", e.DBMType)
	add("source", e.Source)
	add("agent", e.AgentVersion)
	add("agent_host", e.AgentHostname)
	if e.Timestamp != "" {
		add("ts", dbmTime(e.Timestamp))
	}
	if e.CollectionInterval != nil {
		add("interval", fmt.Sprintf("%gs", *e.CollectionInterval))
	}
	if len(e.Tags) > 0 {
		add("tags", fmt.Sprintf("%d %v", len(e.Tags), dbmHead(e.Tags, 3)))
	}
	if len(parts) == 0 {
		return "(no envelope fields)"
	}
	return strings.Join(parts, " ")
}

// rowCounts lists the top-level arrays outside the envelope by length:
// oracle_activity, oracle_rows and whatever the Python integrations call
// their row sets.
func (e *dbmEnvelope) rowCounts() string {
	var parts []string
	for _, k := range e.extraKeys() {
		raw := e.Extra[k]
		if len(raw) == 0 || raw[0] != '[' {
			continue
		}
		var rows []json.RawMessage
		if json.Unmarshal(raw, &rows) == nil {
			parts = append(parts, fmt.Sprintf("%s=%d", k, len(rows)))
		}
	}
	return strings.Join(parts, " ")
}

// sample reads the databasequery specifics from the "db" object.
func (e *dbmEnvelope) sample() string {
	raw, ok := e.Extra["db"]
	if !ok {
		return "(no db object)"
	}
	var db dbmQuerySample
	if err := json.Unmarshal(raw, &db); err != nil {
		return fmt.Sprintf("(db object does not decode: %v)", err)
	}
	var parts []string
	if db.Instance != "" {
		parts = append(parts, "db.instance="+db.Instance)
	}
	if db.QuerySignature != "" {
		parts = append(parts, "db.query_signature="+db.QuerySignature)
	}
	if db.Statement != "" {
		parts = append(parts, fmt.Sprintf("db.statement=%q", dbmTruncate(db.Statement, 80)))
	}
	if db.Plan != nil {
		parts = append(parts, fmt.Sprintf("db.plan.signature=%s definition=%dB", db.Plan.Signature, len(db.Plan.Definition)))
	}
	if len(parts) == 0 {
		return "(empty db object)"
	}
	return strings.Join(parts, " ")
}

func (e *dbmEnvelope) extraKeys() []string {
	keys := make([]string, 0, len(e.Extra))
	for k := range e.Extra {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

// dbmTime renders a Unix-millisecond timestamp for the log. Display only: the
// envelope keeps the number as sent, fraction and all.
func dbmTime(n json.Number) string {
	ms, err := n.Float64()
	if err != nil {
		return n.String()
	}
	return time.UnixMilli(int64(ms)).UTC().Format(time.RFC3339)
}

func dbmHead(s []string, n int) []string {
	if len(s) <= n {
		return s
	}
	return s[:n]
}

func dbmTruncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	return s[:n] + "..."
}
