package storage_test

import (
	"testing"
	"time"

	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// This proves migration 0016_agent_flares.sql, AgentFlareRow.AppendTo and the
// registered INSERT all agree with ClickHouse's own types — the arity tests
// in storagetest catch a column COUNT drift, only a real connection catches a
// TYPE drift (a string landing in a UInt64 column, or fields' Map(String,
// String) not matching what was sent).
func TestAgentFlareRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	receivedAt := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)

	storagetest.Insert(t, conn, storage.AgentFlaresWriter, []storage.Row{
		storage.AgentFlareRow{
			TenantID:     "default",
			ReceivedAt:   receivedAt,
			Hostname:     "web-01.example.test",
			CaseID:       "98765",
			Email:        "ops@example.test",
			Source:       "local",
			AgentVersion: "7.60.0",
			Filename:     "datadog-agent-2026-09-23.zip",
			SizeBytes:    17,
			Fields:       map[string]string{"rc_task_uuid": "rc-task-uuid-123"},
			Archive:      "PK\x03\x04 pretend zip",
		},
		// A minimal row with no case_id and no extra fields, to prove the
		// empty-string/empty-map path round-trips too, not just the fully
		// populated case above.
		storage.AgentFlareRow{
			TenantID:   "default",
			ReceivedAt: receivedAt,
			Hostname:   "web-02.example.test",
			Source:     "local",
			Filename:   "flare.zip",
			Archive:    "zip",
		},
	})

	if got := storagetest.Count(t, conn, "agent_flares"); got != 2 {
		t.Fatalf("agent_flares: %d rows, want 2", got)
	}

	row := storagetest.QueryRow(t, conn,
		`SELECT case_id, email, source, agent_version, filename, size_bytes, fields, archive
		 FROM agent_flares WHERE hostname = 'web-01.example.test'`)
	if len(row) != 8 {
		t.Fatalf("SELECT returned %d columns, want 8", len(row))
	}
	if row[0] != "98765" {
		t.Errorf("case_id: got %v, want the string as sent, not a parsed integer", row[0])
	}
	if row[1] != "ops@example.test" || row[2] != "local" || row[3] != "7.60.0" {
		t.Errorf("email/source/agent_version: got %v/%v/%v", row[1], row[2], row[3])
	}
	if row[4] != "datadog-agent-2026-09-23.zip" {
		t.Errorf("filename: got %v", row[4])
	}
	if row[5] != uint64(17) {
		t.Errorf("size_bytes: got %v (%T), want 17", row[5], row[5])
	}
	fields, ok := row[6].(map[string]string)
	if !ok || fields["rc_task_uuid"] != "rc-task-uuid-123" {
		t.Errorf("fields: got %v (%T), want rc_task_uuid=rc-task-uuid-123", row[6], row[6])
	}
	if row[7] != "PK\x03\x04 pretend zip" {
		t.Errorf("archive: got %v, want the bytes kept verbatim", row[7])
	}

	// The absent-case_id row must read back an empty string, not "0" or any
	// other invented sentinel — case_id is a String precisely so "no case
	// yet" stays distinguishable from a real id.
	minimal := storagetest.QueryRow(t, conn,
		`SELECT case_id, fields FROM agent_flares WHERE hostname = 'web-02.example.test'`)
	if minimal[0] != "" {
		t.Errorf("case_id: got %v, want empty string for an absent case_id", minimal[0])
	}
	if fields, ok := minimal[1].(map[string]string); !ok || len(fields) != 0 {
		t.Errorf("fields: got %v (%T), want an empty map, not NULL", minimal[1], minimal[1])
	}
}
