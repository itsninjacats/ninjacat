package intake

import (
	"encoding/json"
	"reflect"
	"strings"
	"testing"
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
		if ev.CollectionInterval != 10 {
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
