package storagetest_test

import (
	"testing"
	"time"

	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// The arity tests prove AppendTo and the INSERT agree about COUNT. Only
// ClickHouse can prove they agree about TYPES: a string argument landing in a
// UInt64 column, or a map whose value shape does not match the declared
// Map(...), passes every in-process check and fails at insert time.
//
// So this walks the whole path once — scratch database, every migration
// applied from empty, a real batch through the registered INSERT, and the
// rows read back. Skips when ClickHouse is not running (see OpenScratchDB);
// NINJACAT_TEST_REQUIRE_CLICKHOUSE=1 turns that skip into a failure.
func TestInsertRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	// Timestamps are set, not left at Go's zero value. Measured, because it is
	// not obvious: inserting a row with time.Time{} SUCCEEDS and then counts
	// as zero rows — year 1 is outside DateTime64's range, the value clamps,
	// and the table's `TTL toDateTime(timestamp) + INTERVAL 30 DAY` drops the
	// part on the spot. No error anywhere. A test built on zero-value rows
	// would therefore assert on a row that never existed, so any test that
	// touches ClickHouse gives its rows a real timestamp. The zero-VALUE case
	// that matters — nil maps and slices — is covered in-process by the arity
	// tests, which need no database.
	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)

	storagetest.Insert(t, conn, storage.MetricsWriter, []storage.Row{
		storage.MetricPoint{TenantID: "default", Timestamp: now},
	})
	storagetest.Insert(t, conn, storage.LogsWriter, []storage.Row{
		storage.LogRow{TenantID: "default", Timestamp: now, Message: "hello"},
	})

	if got := storagetest.Count(t, conn, "metrics"); got != 1 {
		t.Errorf("metrics: %d rows, want 1", got)
	}
	if got := storagetest.Count(t, conn, "logs"); got != 1 {
		t.Errorf("logs: %d rows, want 1", got)
	}

	// And the values are the ones we sent, not the driver's idea of them.
	row := storagetest.QueryRow(t, conn, "SELECT tenant_id, message FROM logs")
	if len(row) != 2 {
		t.Fatalf("SELECT returned %d columns, want 2", len(row))
	}
	if row[0] != "default" {
		t.Errorf("tenant_id: got %v, want %q", row[0], "default")
	}
	if row[1] != "hello" {
		t.Errorf("message: got %v, want %q", row[1], "hello")
	}
}

// raw_payloads is the newest table and the one added through the per-file
// registry, so it gets its own round trip: this is what proves migration 0002
// applies and that registerWriter really put the writer in Writers().
func TestRawPayloadRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	storagetest.Insert(t, conn, storage.RawPayloadsWriter, []storage.Row{
		storage.RawPayloadRow{
			TenantID:        "default",
			ReceivedAt:      now,
			Intake:          "dbm",
			Reason:          "no_schema",
			Method:          "POST",
			Host:            "dbm-metrics-intake.ninjacat.local",
			Path:            "/api/v2/databasequery",
			Query:           map[string][]string{"api-version": {"2"}},
			ContentType:     "application/json",
			ContentEncoding: "gzip",
			Headers:         map[string]string{"User-Agent": "datadog-agent/7"},
			Body:            `{"unknown":true}`,
			BodyBytes:       16,
			Note:            "no decoder for this track yet",
		},
	})

	if got := storagetest.Count(t, conn, "raw_payloads"); got != 1 {
		t.Fatalf("raw_payloads: %d rows, want 1", got)
	}

	row := storagetest.QueryRow(t, conn,
		"SELECT intake, reason, content_encoding, body, body_bytes FROM raw_payloads")
	if len(row) != 5 {
		t.Fatalf("SELECT returned %d columns, want 5", len(row))
	}
	if row[0] != "dbm" || row[1] != "no_schema" {
		t.Errorf("intake/reason: got %v/%v, want dbm/no_schema", row[0], row[1])
	}
	// The stored body is decompressed while content_encoding names what the
	// sender used — the distinction the column comment exists for.
	if row[2] != "gzip" {
		t.Errorf("content_encoding: got %v, want gzip", row[2])
	}
	if row[3] != `{"unknown":true}` {
		t.Errorf("body: got %v", row[3])
	}
	if row[4] != uint64(16) {
		t.Errorf("body_bytes: got %v (%T), want 16", row[4], row[4])
	}
}
