package storage_test

import (
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// The arity tests (storagetest/arity_test.go) prove every AppendTo agrees
// with its INSERT on argument COUNT. Only ClickHouse can prove they agree on
// TYPES — a UUID landing in a String-typed slot, or a Nullable(UInt64)
// receiving a plain uint64, passes every in-process check and fails only at
// insert time. This is the six security tables' round trip: migration 0009
// applied from empty, one real batch through each registered INSERT, values
// read back.
func TestSecurityTablesRoundTrip(t *testing.T) {
	conn := storagetest.OpenScratchDB(t)

	now := time.Date(2026, 9, 23, 10, 30, 0, 0, time.UTC)
	dumpID := uuid.New()

	start, end, size := uint64(0), uint64(12345), uint64(4096)
	storagetest.Insert(t, conn, storage.CWSActivityDumpsWriter, []storage.Row{
		storage.CWSActivityDumpRow{
			TenantID:      "default",
			ReceivedAt:    now,
			DumpID:        dumpID,
			HeaderHost:    "h1",
			HeaderService: "cws-agent",
			HeaderTags:    map[string][]string{"env": {"prod"}},
			DumpHost:      "h1",
			DumpTags:      map[string][]string{"env": {"prod"}, "kube_service": {"a", "b"}},
			AgentVersion:  "7.58.2",
			// Start=0 is a real kernel-boot-relative reading, not "absent" —
			// Metadata itself was present here, so all three are non-nil.
			Start: &start,
			End:   &end,
			Size:  &size,
			Dump:  "\x08\x01", // a couple of raw protobuf bytes; content is opaque here
		},
	})
	if got := storagetest.Count(t, conn, "cws_activity_dumps"); got != 1 {
		t.Fatalf("cws_activity_dumps: %d rows, want 1", got)
	}
	row := storagetest.QueryRow(t, conn, "SELECT header_host, start_raw, dump FROM cws_activity_dumps")
	if row[0] != "h1" {
		t.Errorf("header_host: got %v, want h1", row[0])
	}
	// Nullable columns scan back as a pointer: nil means NULL, a non-nil
	// pointer to 0 is a present zero — the distinction the column exists for.
	startRaw, ok := row[1].(*uint64)
	if !ok || startRaw == nil || *startRaw != 0 {
		t.Errorf("start_raw: got %v (%T), want a present *uint64(0), not NULL — Metadata was set", row[1], row[1])
	}

	storagetest.Insert(t, conn, storage.CWSDumpNodesWriter, []storage.Row{
		storage.CWSDumpNodeRow{
			TenantID: "default", ReceivedAt: now, DumpID: dumpID,
			NodePath: []uint32{0, 1}, Depth: 2, ParentPath: []uint32{0},
			Node: `{"process":{"pid":2}}`, PID: 2, PPID: 1, Comm: "bash",
		},
	})
	if got := storagetest.Count(t, conn, "cws_dump_nodes"); got != 1 {
		t.Fatalf("cws_dump_nodes: %d rows, want 1", got)
	}

	storagetest.Insert(t, conn, storage.SecurityEventsWriter, []storage.Row{
		storage.SecurityEventRow{
			TenantID: "default", ReceivedAt: now, Track: "secruntime",
			DDTags: map[string][]string{"env": {"prod"}}, MessageDecoded: 1,
			RuleID: "r1",
		},
	})
	if got := storagetest.Count(t, conn, "security_events"); got != 1 {
		t.Fatalf("security_events: %d rows, want 1", got)
	}
	row = storagetest.QueryRow(t, conn, "SELECT track, message_decoded, rule_id FROM security_events")
	if row[0] != "secruntime" || row[2] != "r1" {
		t.Errorf("track/rule_id: got %v/%v", row[0], row[2])
	}

	entityID := uuid.New()
	generatedAt := now.Add(-time.Hour)
	storagetest.Insert(t, conn, storage.SBOMEntitiesWriter, []storage.Row{
		storage.SBOMEntityRow{
			TenantID: "default", ReceivedAt: now, EntityID: entityID,
			Type: "CONTAINER_IMAGE_LAYERS", ID: "e1", GeneratedAt: &generatedAt,
			DDTags: map[string][]string{"image": {"nginx"}}, Bom: `{"specVersion":"1.4"}`,
			ComponentCount: 1, VulnerabilityCount: 1,
		},
	})
	if got := storagetest.Count(t, conn, "sbom_entities"); got != 1 {
		t.Fatalf("sbom_entities: %d rows, want 1", got)
	}

	storagetest.Insert(t, conn, storage.SBOMComponentsWriter, []storage.Row{
		storage.SBOMComponentRow{
			TenantID: "default", ReceivedAt: now, EntityID: entityID,
			BomRef: "pkg:c1", Name: "libfoo", Version: "1.0",
			Licenses: []string{"MIT"}, Hashes: map[string]string{"SHA-256": "abcd"},
		},
	})
	if got := storagetest.Count(t, conn, "sbom_components"); got != 1 {
		t.Fatalf("sbom_components: %d rows, want 1", got)
	}
	row = storagetest.QueryRow(t, conn, "SELECT name, version FROM sbom_components")
	if row[0] != "libfoo" || row[1] != "1.0" {
		t.Errorf("name/version: got %v/%v", row[0], row[1])
	}

	storagetest.Insert(t, conn, storage.SBOMVulnerabilitiesWriter, []storage.Row{
		storage.SBOMVulnerabilityRow{
			TenantID: "default", ReceivedAt: now, EntityID: entityID,
			BomRef: "pkg:c1", ID: "CVE-2024-1234", Cwes: []int32{79},
			AffectsRefs: []string{"pkg:c1"},
		},
	})
	if got := storagetest.Count(t, conn, "sbom_vulnerabilities"); got != 1 {
		t.Fatalf("sbom_vulnerabilities: %d rows, want 1", got)
	}
	row = storagetest.QueryRow(t, conn, "SELECT id, cwes FROM sbom_vulnerabilities")
	if row[0] != "CVE-2024-1234" {
		t.Errorf("id: got %v, want CVE-2024-1234", row[0])
	}
}
