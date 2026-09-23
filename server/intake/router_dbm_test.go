package intake

import (
	"bytes"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"reflect"
	"strings"
	"testing"
	"time"

	"github.com/gin-gonic/gin"
	"github.com/itsninjacats/server/apps/storage"
)

// The envelope must accept every spelling the oracle corecheck uses
// (pkg/collector/corechecks/oracle) and never drop what it does not know.

func TestDBMEnvelopeTagsAsString(t *testing.T) {
	// FQTPayload: ddtags is a comma-joined string.
	body := `{"timestamp":1758326400000,"host":"db1","database_instance":"db1/orcl",
		"ddagentversion":"7.60.0","ddsource":"oracle","ddtags":"env:prod, team:db,,",
		"dbm_type":"fqt","db":{"instance":"orcl","query_signature":"abc","statement":"select 1"}}`

	var ev dbmEnvelope
	if err := json.Unmarshal([]byte(body), &ev); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if want := (dbmTags{"env:prod", "team:db"}); !reflect.DeepEqual(ev.Tags, want) {
		t.Errorf("tags = %v, want %v", ev.Tags, want)
	}
	if ev.Host != "db1" || ev.DatabaseInstance != "db1/orcl" || ev.DBMType != "fqt" || ev.Source != "oracle" {
		t.Errorf("identity fields not decoded: %+v", ev)
	}
	if ev.AgentVersion != "7.60.0" {
		t.Errorf("agent version = %q", ev.AgentVersion)
	}
	if _, ok := ev.Extra["db"]; !ok {
		t.Errorf("db object must stay in Extra, got keys %v", ev.extraKeys())
	}
	if _, ok := ev.Extra["ddtags"]; ok {
		t.Errorf("decoded key ddtags must not remain in Extra")
	}
	if len(ev.Undecoded) != 0 {
		t.Errorf("unexpected undecoded keys %v", ev.Undecoded)
	}
}

func TestDBMEnvelopeTagsAsArray(t *testing.T) {
	// ActivitySnapshot: ddtags is an array. metricsPayload/dbInstanceEvent: tags is an array.
	for _, body := range []string{
		`{"ddtags":["env:prod","team:db"],"collection_interval":10,"oracle_activity":[{},{}]}`,
		`{"tags":["env:prod","team:db"],"min_collection_interval":10,"oracle_rows":[{},{}]}`,
	} {
		var ev dbmEnvelope
		if err := json.Unmarshal([]byte(body), &ev); err != nil {
			t.Fatalf("unmarshal %s: %v", body, err)
		}
		if want := (dbmTags{"env:prod", "team:db"}); !reflect.DeepEqual(ev.Tags, want) {
			t.Errorf("%s: tags = %v, want %v", body, ev.Tags, want)
		}
		if ev.CollectionInterval == nil || *ev.CollectionInterval != 10 {
			t.Errorf("%s: interval = %v, want 10", body, ev.CollectionInterval)
		}
		if got := ev.rowCounts(); got != "oracle_activity=2" && got != "oracle_rows=2" {
			t.Errorf("%s: rowCounts = %q", body, got)
		}
	}
}

func TestDBMEnvelopeAgentVersionSpellings(t *testing.T) {
	// dbInstanceEvent (metadata.go) says agent_version, everything else ddagentversion.
	var ev dbmEnvelope
	if err := json.Unmarshal([]byte(`{"agent_version":"7.60.0","kind":"database_instance","dbms":"oracle"}`), &ev); err != nil {
		t.Fatal(err)
	}
	if ev.AgentVersion != "7.60.0" || ev.Kind != "database_instance" || ev.DBMS != "oracle" {
		t.Errorf("metadata envelope not decoded: %+v", ev)
	}
}

func TestDBMEnvelopeKeepsUnknownAndMalformed(t *testing.T) {
	body := `{"host":"db1","timestamp":"not-a-number","postgres_rows":[1,2,3],"custom":{"a":1}}`

	var ev dbmEnvelope
	if err := json.Unmarshal([]byte(body), &ev); err != nil {
		t.Fatalf("a malformed envelope field must not fail the event: %v", err)
	}
	if ev.Host != "db1" {
		t.Errorf("host lost after a sibling field failed to decode")
	}
	if want := []string{"timestamp"}; !reflect.DeepEqual(ev.Undecoded, want) {
		t.Errorf("undecoded = %v, want %v", ev.Undecoded, want)
	}
	if want := []string{"custom", "postgres_rows", "timestamp"}; !reflect.DeepEqual(ev.extraKeys(), want) {
		t.Errorf("extra keys = %v, want %v", ev.extraKeys(), want)
	}
	if string(ev.Extra["timestamp"]) != `"not-a-number"` {
		t.Errorf("malformed value must be kept verbatim, got %s", ev.Extra["timestamp"])
	}
	if got := ev.rowCounts(); got != "postgres_rows=3" {
		t.Errorf("rowCounts = %q", got)
	}
}

func TestDBMEnvelopeTimestampVerbatim(t *testing.T) {
	// The Python integrations send time.time()*1000 with a fraction; the
	// oracle corecheck an integer. Both must survive as sent.
	var ev dbmEnvelope
	if err := json.Unmarshal([]byte(`{"timestamp":1758326400123.456}`), &ev); err != nil {
		t.Fatal(err)
	}
	if ev.Timestamp.String() != "1758326400123.456" {
		t.Errorf("timestamp = %q, want the wire value verbatim", ev.Timestamp)
	}
	if got := dbmTime(ev.Timestamp); got != "2025-09-20T00:00:00Z" {
		t.Errorf("dbmTime = %q", got)
	}

	// Absent means not sent, not 1970.
	ev = dbmEnvelope{}
	if err := json.Unmarshal([]byte(`{"host":"db1"}`), &ev); err != nil {
		t.Fatal(err)
	}
	if ev.Timestamp != "" {
		t.Errorf("absent timestamp = %q, want empty", ev.Timestamp)
	}
	if s := ev.summary(); strings.Contains(s, "ts=") {
		t.Errorf("summary must not invent a timestamp: %s", s)
	}
}

func TestDBMEventsWrapsLoneObject(t *testing.T) {
	if events, err := dbmEvents([]byte(`[{"a":1},{"b":2}]`)); err != nil || len(events) != 2 {
		t.Errorf("array: %d events, err %v", len(events), err)
	}
	if events, err := dbmEvents([]byte(`{"a":1}`)); err != nil || len(events) != 1 {
		t.Errorf("object: %d events, err %v", len(events), err)
	}
	if _, err := dbmEvents([]byte(`not json`)); err == nil {
		t.Errorf("garbage must error")
	}
}

// dbmTimestamp is the one place the wire's unix-ms number becomes a storage
// timestamp, so its three cases each get pinned: an oracle-shaped integer
// (must stay exact, never rounded through float64), a Python-shaped
// fraction of a millisecond (DateTime64(3) is ms precision, so this rounds),
// and an absent field (must stay nil, never become the Unix epoch).
func TestDBMTimestamp(t *testing.T) {
	cases := []struct {
		name string
		in   json.Number
		want *time.Time // nil means "must be nil"
	}{
		{
			name: "oracle integer milliseconds",
			in:   "1758326400000",
			want: timePtr(time.UnixMilli(1758326400000).UTC()),
		},
		{
			name: "python fractional milliseconds rounds to the nearest ms",
			in:   "1758326400123.6",
			want: timePtr(time.UnixMilli(1758326400124).UTC()),
		},
		{"absent", "", nil},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := dbmTimestamp(tc.in)
			if tc.want == nil {
				if got != nil {
					t.Errorf("dbmTimestamp(%q) = %v, want nil (absent must stay absent, not 1970)", tc.in, got)
				}
				return
			}
			if got == nil || !got.Equal(*tc.want) {
				t.Errorf("dbmTimestamp(%q) = %v, want %v", tc.in, got, tc.want)
			}
		})
	}
}

func timePtr(t time.Time) *time.Time { return &t }

// dbmPlanDefinitionSteps must tell three facts apart: "no plan was sent"
// (nil Definition), "a plan was sent with zero steps" (an empty array), and
// "a plan was sent but its definition is not a step array at all" — folding
// the last case into the first would misrepresent what the producer actually
// sent, not just lose precision (see dbmPlanStepsMalformed's doc comment).
func TestDBMPlanDefinitionSteps(t *testing.T) {
	cases := []struct {
		name       string
		definition json.RawMessage
		wantSteps  uint32
		wantState  dbmPlanStepsState
	}{
		{"absent", nil, 0, dbmPlanStepsAbsent},
		{"empty array", json.RawMessage(`[]`), 0, dbmPlanStepsOK},
		{"three steps", json.RawMessage(`[{"id":1},{"id":2},{"id":3}]`), 3, dbmPlanStepsOK},
		{"not an array", json.RawMessage(`"oops"`), 0, dbmPlanStepsMalformed},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			steps, state := dbmPlanDefinitionSteps(tc.definition)
			if state != tc.wantState || steps != tc.wantSteps {
				t.Errorf("dbmPlanDefinitionSteps(%s) = (%d, %v), want (%d, %v)",
					tc.definition, steps, state, tc.wantSteps, tc.wantState)
			}
		})
	}
}

// A plan sent with a malformed (non-array) definition must leave
// plan_definition_steps NULL (same as "no plan sent" — the column has no
// third state) but must NOT be indistinguishable from "no plan sent": it is
// recorded in undecoded_keys instead, and the raw bytes stay in `event`.
func TestDBMEventRowPlanDefinitionMalformed(t *testing.T) {
	body := `{"timestamp":1758326400000,"host":"db1","database_instance":"db1/orcl",
		"dbm_type":"plan",
		"db":{"instance":"orcl","plan":{"signature":"plan-xyz","definition":"oops"}}}`

	var ev dbmEnvelope
	if err := json.Unmarshal([]byte(body), &ev); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	row := dbmEventRow("test-tenant", "databasequery", time.Now(), dbmEvent{Raw: json.RawMessage(body), Env: ev})

	if row.PlanDefinitionSteps != nil {
		t.Errorf("plan_definition_steps = %v, want nil — the column has no way to encode \"malformed\"", row.PlanDefinitionSteps)
	}
	if row.PlanSignature == nil || *row.PlanSignature != "plan-xyz" {
		t.Errorf("plan_signature = %v, want plan-xyz — a malformed definition must not take the rest of the plan down with it", row.PlanSignature)
	}
	if !contains(row.UndecodedKeys, "db.plan.definition") {
		t.Errorf("undecoded_keys = %v, want db.plan.definition so \"malformed\" is not silently the same as \"absent\"", row.UndecodedKeys)
	}
}

// dbmEventRow is the pure converter from a decoded event to the row
// dbm_events stores. One realistic body per track (built from the oracle
// struct field lists — see the gap-dbm-ndm-process report) plus a
// postgres-shaped body carrying unknown keys, since the Python integrations
// have no Go type to build a literal from.
func TestDBMEventRowFromRealisticJSON(t *testing.T) {
	receivedAt := time.Date(2026, 9, 23, 12, 0, 0, 0, time.UTC)

	cases := []struct {
		name  string
		track string
		body  string
		check func(t *testing.T, row storage.DBMEventRow)
	}{
		{
			name:  "dbmmetrics oracle MetricsPayload",
			track: "dbmmetrics",
			body: `{"host":"db1","database_instance":"db1/orcl","timestamp":1758326400000,
				"min_collection_interval":60,"tags":["env:prod","team:db"],
				"ddagentversion":"7.60.0","ddagenthostname":"agent-host-1",
				"oracle_rows":[{"query_signature":"sig1","sql_text":"select 1"}],"oracle_version":"19c"}`,
			check: func(t *testing.T, row storage.DBMEventRow) {
				if row.Host != "db1" || row.DatabaseInstance != "db1/orcl" {
					t.Errorf("identity: host=%q instance=%q", row.Host, row.DatabaseInstance)
				}
				if row.AgentHostname != "agent-host-1" || row.AgentVersion != "7.60.0" {
					t.Errorf("agent identity: hostname=%q version=%q", row.AgentHostname, row.AgentVersion)
				}
				if row.CollectionInterval == nil || *row.CollectionInterval != 60 {
					t.Errorf("collection_interval = %v, want 60 (from min_collection_interval)", row.CollectionInterval)
				}
				if want := map[string][]string{"env": {"prod"}, "team": {"db"}}; !reflect.DeepEqual(row.Tags, want) {
					t.Errorf("tags = %v, want %v", row.Tags, want)
				}
				if !contains(row.ExtraKeys, "oracle_rows") || !contains(row.ExtraKeys, "oracle_version") {
					t.Errorf("extra_keys = %v, want oracle_rows and oracle_version", row.ExtraKeys)
				}
			},
		},
		{
			name:  "dbmactivity oracle ActivitySnapshot",
			track: "dbmactivity",
			body: `{"host":"db1","database_instance":"db1/orcl","timestamp":1758326400000,
				"ddsource":"oracle","dbm_type":"activity","ddagentversion":"7.60.0",
				"ddtags":["env:prod"],"collection_interval":10,
				"oracle_activity":[{"session_id":1},{"session_id":2}]}`,
			check: func(t *testing.T, row storage.DBMEventRow) {
				if row.Source != "oracle" || row.DBMType != "activity" {
					t.Errorf("source=%q dbm_type=%q", row.Source, row.DBMType)
				}
				if row.CollectionInterval == nil || *row.CollectionInterval != 10 {
					t.Errorf("collection_interval = %v, want 10", row.CollectionInterval)
				}
				if !contains(row.ExtraKeys, "oracle_activity") {
					t.Errorf("extra_keys = %v, want oracle_activity", row.ExtraKeys)
				}
			},
		},
		{
			name:  "databasequery fqt with a decodable db object",
			track: "databasequery",
			body: `{"timestamp":1758326400000,"host":"db1","database_instance":"db1/orcl",
				"ddagentversion":"7.60.0","ddsource":"oracle","ddtags":"env:prod,team:db",
				"dbm_type":"fqt",
				"db":{"instance":"orcl","query_signature":"abc123","statement":"select 1 from dual",
				      "metadata":{"dd_tables":["t1"],"dd_commands":["SELECT"]}}}`,
			check: func(t *testing.T, row storage.DBMEventRow) {
				if row.DBInstance == nil || *row.DBInstance != "orcl" {
					t.Errorf("db_instance = %v, want orcl", row.DBInstance)
				}
				if row.QuerySignature == nil || *row.QuerySignature != "abc123" {
					t.Errorf("query_signature = %v, want abc123", row.QuerySignature)
				}
				if row.Statement == nil || *row.Statement != "select 1 from dual" {
					t.Errorf("statement = %v, want %q", row.Statement, "select 1 from dual")
				}
				if row.PlanSignature != nil {
					t.Errorf("plan_signature = %v, want nil — this event has no plan", row.PlanSignature)
				}
				if row.PlanDefinitionSteps != nil {
					t.Errorf("plan_definition_steps = %v, want nil — no plan was sent", row.PlanDefinitionSteps)
				}
				// The db object's metadata (dd_tables/dd_commands here, tables/commands
				// on a plan event) has no dedicated column — it must still be
				// reachable, verbatim, through the event column.
				if !strings.Contains(row.Event, `"dd_tables":["t1"]`) {
					t.Errorf("event does not carry the db.metadata verbatim: %s", row.Event)
				}
			},
		},
		{
			name:  "databasequery plan with a three-step definition",
			track: "databasequery",
			body: `{"timestamp":1758326400000,"host":"db1","database_instance":"db1/orcl",
				"ddagentversion":"7.60.0","ddsource":"oracle","ddtags":"env:prod","dbm_type":"plan",
				"db":{"instance":"orcl","query_signature":"abc123","statement":"select 1 from dual",
				      "plan":{"signature":"plan-xyz",
				              "definition":[{"id":1,"operation":"SELECT STATEMENT"},
				                            {"id":2,"operation":"TABLE ACCESS"},
				                            {"id":3,"operation":"INDEX SCAN"}]},
				      "metadata":{"tables":["t1"],"commands":["SELECT"]}}}`,
			check: func(t *testing.T, row storage.DBMEventRow) {
				if row.PlanSignature == nil || *row.PlanSignature != "plan-xyz" {
					t.Errorf("plan_signature = %v, want plan-xyz", row.PlanSignature)
				}
				if row.PlanDefinitionSteps == nil || *row.PlanDefinitionSteps != 3 {
					t.Errorf("plan_definition_steps = %v, want 3", row.PlanDefinitionSteps)
				}
			},
		},
		{
			name:  "dbmmetadata dbInstanceEvent, agent_version spelling",
			track: "dbmmetadata",
			body: `{"host":"db1","database_instance":"db1/orcl","agent_version":"7.60.0",
				"dbms":"oracle","kind":"database_instance","collection_interval":300,
				"dbms_version":"19c","tags":["env:prod"],"timestamp":1758326400000,
				"metadata":{"dbm":true,"connection_host":"db1"}}`,
			check: func(t *testing.T, row storage.DBMEventRow) {
				if row.AgentVersion != "7.60.0" {
					t.Errorf("agent_version = %q, want 7.60.0 (from the agent_version spelling)", row.AgentVersion)
				}
				if row.DBMS != "oracle" || row.DBMSVersion != "19c" || row.Kind != "database_instance" {
					t.Errorf("dbms=%q dbms_version=%q kind=%q", row.DBMS, row.DBMSVersion, row.Kind)
				}
				if !contains(row.ExtraKeys, "metadata") {
					t.Errorf("extra_keys = %v, want metadata (the schema-collection payload has no Go type at all)", row.ExtraKeys)
				}
			},
		},
		{
			// No producer anywhere in the agent clone — only the generic
			// envelope read applies.
			name:  "dbmhealth generic envelope",
			track: "dbmhealth",
			body:  `{"host":"db1","database_instance":"db1/orcl","timestamp":1758326400000,"ddagentversion":"7.60.0","ddtags":["env:prod"],"status":"ok"}`,
			check: func(t *testing.T, row storage.DBMEventRow) {
				if row.Track != "dbmhealth" {
					t.Errorf("track = %q, want dbmhealth", row.Track)
				}
				if !contains(row.ExtraKeys, "status") {
					t.Errorf("extra_keys = %v, want status (unknown to the envelope, kept anyway)", row.ExtraKeys)
				}
			},
		},
		{
			// A Postgres-shaped body: no Go type exists for this engine
			// anywhere in the module graph, so every engine-specific key
			// must survive as an unknown key, not get silently dropped.
			name:  "dbmcolumnstatistics postgres-shaped, unknown keys",
			track: "dbmcolumnstatistics",
			body: `{"host":"pg1","database_instance":"pg1/mydb","timestamp":1758326400000,"ddagentversion":"7.60.0",
				"ddtags":["env:staging"],
				"postgres_rows":[{"schema_name":"public","table_name":"users","column_name":"id"}]}`,
			check: func(t *testing.T, row storage.DBMEventRow) {
				if row.Track != "dbmcolumnstatistics" {
					t.Errorf("track = %q, want dbmcolumnstatistics (not the old dbmcolstat label)", row.Track)
				}
				if !contains(row.ExtraKeys, "postgres_rows") {
					t.Errorf("extra_keys = %v, want postgres_rows", row.ExtraKeys)
				}
				if !strings.Contains(row.Event, "schema_name") {
					t.Errorf("event does not carry the postgres-shaped rows verbatim: %s", row.Event)
				}
			},
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			var ev dbmEnvelope
			if err := json.Unmarshal([]byte(tc.body), &ev); err != nil {
				t.Fatalf("unmarshal: %v", err)
			}
			row := dbmEventRow("test-tenant", tc.track, receivedAt, dbmEvent{Raw: json.RawMessage(tc.body), Env: ev})

			if row.TenantID != "test-tenant" {
				t.Errorf("tenant = %q, want test-tenant", row.TenantID)
			}
			if row.Track != tc.track {
				t.Errorf("track = %q, want %q", row.Track, tc.track)
			}
			if !row.ReceivedAt.Equal(receivedAt) {
				t.Errorf("received_at = %v, want %v", row.ReceivedAt, receivedAt)
			}
			if row.Timestamp == nil {
				t.Errorf("timestamp = nil, want the envelope's 1758326400000")
			}
			if row.Event != tc.body {
				t.Errorf("event was not stored verbatim")
			}
			tc.check(t, row)
		})
	}
}

// A missing envelope timestamp must produce a NULL row, not the Unix epoch —
// the one case TestDBMEventRowFromRealisticJSON's fixed bodies do not cover.
func TestDBMEventRowMissingTimestamp(t *testing.T) {
	var ev dbmEnvelope
	if err := json.Unmarshal([]byte(`{"host":"db1"}`), &ev); err != nil {
		t.Fatal(err)
	}
	row := dbmEventRow("test-tenant", "dbmhealth", time.Now(), dbmEvent{Raw: json.RawMessage(`{"host":"db1"}`), Env: ev})
	if row.Timestamp != nil {
		t.Errorf("timestamp = %v, want nil for a wire event that never sent one", row.Timestamp)
	}
}

func contains(list []string, s string) bool {
	for _, v := range list {
		if v == s {
			return true
		}
	}
	return false
}

// The worked example from testserver_test.go, run against every DBM route:
// a real request through the real engine, asserting on the rows that reached
// storage.DBMEventsWriter. This is what pins the dbmcolumnstatistics label
// fix end to end (a log-only check could not tell the stored track value
// apart from the log line).
func TestHandleDBMStoresEventsPerTrack(t *testing.T) {
	cases := []struct {
		route string
		track string // must equal what the route is named — pins the dbmcolstat fix
		body  string
	}{
		{"/api/v2/dbmmetrics", "dbmmetrics", `{"host":"db1","database_instance":"db1/orcl","ddagentversion":"7.60.0","tags":["env:prod"]}`},
		{"/api/v2/dbmactivity", "dbmactivity", `{"host":"db1","database_instance":"db1/orcl","ddsource":"oracle","ddtags":["env:prod"]}`},
		{"/api/v2/databasequery", "databasequery", `{"host":"db1","database_instance":"db1/orcl","dbm_type":"fqt","ddtags":"env:prod","db":{"instance":"orcl","query_signature":"sig1"}}`},
		{"/api/v2/dbmmetadata", "dbmmetadata", `{"host":"db1","database_instance":"db1/orcl","agent_version":"7.60.0","kind":"database_instance"}`},
		{"/api/v2/dbmhealth", "dbmhealth", `{"host":"db1","database_instance":"db1/orcl","status":"ok"}`},
		{"/api/v2/dbmcolumnstatistics", "dbmcolumnstatistics", `{"host":"pg1","database_instance":"pg1/mydb","postgres_rows":[{"column_name":"id"}]}`},
	}

	for _, tc := range cases {
		t.Run(tc.track, func(t *testing.T) {
			a, node := newTestServer(t)
			e := newTestEngine(t, a, a.routeDBM)

			w := post(t, e, tc.route, []byte(tc.body))
			if w.Code != http.StatusAccepted {
				t.Fatalf("POST %s: got %d, want 202 (%s)", tc.route, w.Code, w.Body.String())
			}

			rows := Rows[storage.DBMEventRow](node)
			if len(rows) != 1 {
				t.Fatalf("stored rows: got %d, want 1", len(rows))
			}
			if rows[0].TenantID != testTenant {
				t.Errorf("tenant: got %q, want %q", rows[0].TenantID, testTenant)
			}
			if rows[0].Track != tc.track {
				t.Errorf("track: got %q, want %q", rows[0].Track, tc.track)
			}
			if rows[0].Host != "db1" && rows[0].Host != "pg1" {
				t.Errorf("host: got %q", rows[0].Host)
			}
			if len(node.SentTo(storage.RawPayloadsWriter)) != 0 {
				t.Errorf("a decodable event must not also land in raw_payloads")
			}
		})
	}
}

// A body that is not a JSON array or object, and a batch element that fails
// to decode, must both reach storeRaw rather than being dropped — the
// decode-error path is the one that would otherwise leave no trace at all.
func TestHandleDBMStoresRawOnDecodeError(t *testing.T) {
	t.Run("body is not JSON", func(t *testing.T) {
		a, node := newTestServer(t)
		e := newTestEngine(t, a, a.routeDBM)

		w := post(t, e, "/api/v2/dbmmetrics", []byte(`not json at all`))
		if w.Code != http.StatusAccepted {
			t.Fatalf("got %d, want 202 even for a body we could not read", w.Code)
		}

		raw := Rows[storage.RawPayloadRow](node)
		if len(raw) != 1 {
			t.Fatalf("raw_payloads rows: got %d, want 1", len(raw))
		}
		if raw[0].Intake != "dbm" || raw[0].Reason != "decode_error" {
			t.Errorf("intake/reason: got %q/%q, want dbm/decode_error", raw[0].Intake, raw[0].Reason)
		}
		if raw[0].Body != "not json at all" {
			t.Errorf("body: got %q", raw[0].Body)
		}
		if len(Rows[storage.DBMEventRow](node)) != 0 {
			t.Errorf("nothing should reach dbm_events when the batch never split")
		}
	})

	t.Run("one bad element beside one good one", func(t *testing.T) {
		a, node := newTestServer(t)
		e := newTestEngine(t, a, a.routeDBM)

		body := `[{"host":"db1","database_instance":"db1/orcl"}, "not an object", 42]`
		w := post(t, e, "/api/v2/dbmactivity", []byte(body))
		if w.Code != http.StatusAccepted {
			t.Fatalf("got %d, want 202", w.Code)
		}

		events := Rows[storage.DBMEventRow](node)
		if len(events) != 1 {
			t.Fatalf("dbm_events rows: got %d, want 1 (the one decodable element)", len(events))
		}
		if events[0].Host != "db1" {
			t.Errorf("host: got %q", events[0].Host)
		}

		raw := Rows[storage.RawPayloadRow](node)
		if len(raw) != 2 {
			t.Fatalf("raw_payloads rows: got %d, want 2 (one per bad element)", len(raw))
		}
		for _, r := range raw {
			if r.Intake != "dbm" || r.Reason != "decode_error" {
				t.Errorf("intake/reason: got %q/%q, want dbm/decode_error", r.Intake, r.Reason)
			}
		}
	})

	// encoding/json special-cases a literal `null` for a map target: unlike
	// "not an object" or 42 above (both correctly error), null unmarshals
	// into dbmEnvelope's field map with no error at all. Without an explicit
	// check for it, this element would silently become a zero-value row in
	// dbm_events instead of going to storeRaw like every other undecodable
	// element in the same batch.
	t.Run("a literal null element beside one good one", func(t *testing.T) {
		a, node := newTestServer(t)
		e := newTestEngine(t, a, a.routeDBM)

		body := `[{"host":"db1","database_instance":"db1/orcl"}, null]`
		w := post(t, e, "/api/v2/dbmactivity", []byte(body))
		if w.Code != http.StatusAccepted {
			t.Fatalf("got %d, want 202", w.Code)
		}

		events := Rows[storage.DBMEventRow](node)
		if len(events) != 1 {
			t.Fatalf("dbm_events rows: got %d, want 1 (the one decodable element) — a null element must not become a zero-value row", len(events))
		}
		if events[0].Host != "db1" {
			t.Errorf("host: got %q", events[0].Host)
		}

		raw := Rows[storage.RawPayloadRow](node)
		if len(raw) != 1 {
			t.Fatalf("raw_payloads rows: got %d, want 1 (the null element)", len(raw))
		}
		if raw[0].Intake != "dbm" || raw[0].Reason != "decode_error" {
			t.Errorf("intake/reason: got %q/%q, want dbm/decode_error", raw[0].Intake, raw[0].Reason)
		}
		if raw[0].Body != "null" {
			t.Errorf("body: got %q, want the literal null element", raw[0].Body)
		}
	})
}

// Tenant comes from TenantFromContext; with none in context, both storage
// paths must stay silent while the response is unaffected — a request
// without a key never reaches the handler at all (see
// TestEngineRejectsRequestWithoutKey in testserver_test.go), so this
// exercises the handler directly with a context that carries no tenant.
func TestHandleDBMWithoutTenantStoresNothing(t *testing.T) {
	a, node := newTestServer(t)

	// Deliberately a bare router with no key middleware — a.engine() would
	// reject a keyless request with 403 before the handler ever ran, which
	// would test the middleware, not the handler's own tenant guard. This is
	// the same shape as TestStoreRawWithoutTenantStoresNothing
	// (testserver_test.go).
	e := gin.New()
	e.POST("/api/v2/dbmmetrics", a.handleDBMMetrics)

	req := httptest.NewRequest(http.MethodPost, "/api/v2/dbmmetrics",
		bytes.NewReader([]byte(`{"host":"db1","database_instance":"db1/orcl"}`)))
	w := httptest.NewRecorder()
	e.ServeHTTP(w, req)

	if w.Code != http.StatusAccepted {
		t.Fatalf("got %d, want 202 even with no tenant", w.Code)
	}
	if n := len(node.Sends()); n != 0 {
		t.Errorf("%d messages stored without a tenant, want 0", n)
	}
}
