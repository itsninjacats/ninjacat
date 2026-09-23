package storage_test

import (
	"time"

	"testing"

	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// This proves migration 0006_dbm.sql, DBMEventRow.AppendTo and the
// registered INSERT all agree with ClickHouse's own types — the arity tests
// in storagetest catch a column COUNT drift, only a real connection catches
// a Nullable/Map/Array TYPE drift (a string landing in a Nullable(Float64)
// column, for instance).
//
// A databasequery-shaped row is used because it is the one track that
// exercises every optional column at once: a real Timestamp, a real
// CollectionInterval, tags, extra/undecoded keys and every db.*/plan.*
// convenience column.
func TestDBMEventRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	receivedAt := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	ts := time.Date(2026, 9, 23, 10, 29, 58, 123000000, time.UTC)
	interval := 10.0
	steps := uint32(3)
	instance := "orcl"
	querySig := "abc123"
	statement := "select 1 from dual"
	planSig := "plan-xyz"

	storagetest.Insert(t, conn, storage.DBMEventsWriter, []storage.Row{
		storage.DBMEventRow{
			TenantID:            "default",
			ReceivedAt:          receivedAt,
			Track:               "databasequery",
			Timestamp:           &ts,
			Host:                "db1",
			DatabaseInstance:    "db1/orcl",
			AgentHostname:       "agent1",
			AgentVersion:        "7.60.0",
			Source:              "oracle",
			DBMType:             "fqt",
			Kind:                "",
			DBMS:                "oracle",
			DBMSVersion:         "19c",
			CollectionInterval:  &interval,
			Tags:                map[string][]string{"env": {"prod"}, "kube_service": {"a", "b"}},
			ExtraKeys:           []string{"db"},
			UndecodedKeys:       nil,
			Event:               `{"host":"db1","db":{"instance":"orcl"}}`,
			DBInstance:          &instance,
			QuerySignature:      &querySig,
			Statement:           &statement,
			PlanSignature:       &planSig,
			PlanDefinitionSteps: &steps,
		},
		// A minimal row with every optional field left absent, to prove the
		// Nullable columns and empty containers round-trip too — not just
		// the fully populated case above.
		storage.DBMEventRow{
			TenantID:   "default",
			ReceivedAt: receivedAt,
			Track:      "dbmhealth",
			Host:       "db2",
			Event:      `{"host":"db2"}`,
		},
	})

	if got := storagetest.Count(t, conn, "dbm_events"); got != 2 {
		t.Fatalf("dbm_events: %d rows, want 2", got)
	}

	row := storagetest.QueryRow(t, conn,
		`SELECT track, timestamp, collection_interval, db_instance, plan_definition_steps, event
		 FROM dbm_events WHERE track = 'databasequery'`)
	if len(row) != 6 {
		t.Fatalf("SELECT returned %d columns, want 6", len(row))
	}
	if row[0] != "databasequery" {
		t.Errorf("track: got %v", row[0])
	}
	// Nullable columns scan as pointers (nil means NULL); a populated one
	// scans as a non-nil pointer to the value, never the bare value.
	if gotTS, ok := row[1].(*time.Time); !ok || gotTS == nil || !gotTS.Equal(ts) {
		t.Errorf("timestamp: got %v, want %v", row[1], ts)
	}
	if gotInterval, ok := row[2].(*float64); !ok || gotInterval == nil || *gotInterval != 10 {
		t.Errorf("collection_interval: got %v (%T), want 10", row[2], row[2])
	}
	if gotInstance, ok := row[3].(*string); !ok || gotInstance == nil || *gotInstance != "orcl" {
		t.Errorf("db_instance: got %v, want orcl", row[3])
	}
	if gotSteps, ok := row[4].(*uint32); !ok || gotSteps == nil || *gotSteps != 3 {
		t.Errorf("plan_definition_steps: got %v (%T), want 3", row[4], row[4])
	}
	if row[5] != `{"host":"db1","db":{"instance":"orcl"}}` {
		t.Errorf("event: got %v, want the raw JSON verbatim", row[5])
	}

	// The absent-timestamp row must read back NULL, not a zero time — this is
	// the fidelity rule the Nullable column exists to enforce. QueryRow scans
	// a Nullable column as a typed pointer, so NULL comes back as a
	// (*T)(nil) inside the interface, not the untyped nil a bare `!= nil`
	// would compare against — check the pointer through its concrete type.
	healthRow := storagetest.QueryRow(t, conn,
		`SELECT timestamp, collection_interval, db_instance FROM dbm_events WHERE track = 'dbmhealth'`)
	if v, ok := healthRow[0].(*time.Time); !ok || v != nil {
		t.Errorf("dbmhealth timestamp: got %v, want NULL (absent, not 1970)", healthRow[0])
	}
	if v, ok := healthRow[1].(*float64); !ok || v != nil {
		t.Errorf("dbmhealth collection_interval: got %v, want NULL", healthRow[1])
	}
	if v, ok := healthRow[2].(*string); !ok || v != nil {
		t.Errorf("dbmhealth db_instance: got %v, want NULL", healthRow[2])
	}
}
