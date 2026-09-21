package schema

import (
	"strings"
	"testing"
)

func TestParseFilename(t *testing.T) {
	version, name, err := parseFilename("0001_initial.sql")
	if err != nil {
		t.Fatalf("valid name rejected: %s", err)
	}
	if version != 1 || name != "initial" {
		t.Fatalf("got version=%d name=%q", version, name)
	}

	version, name, err = parseFilename("0042_add_traces_table.sql")
	if err != nil || version != 42 || name != "add_traces_table" {
		t.Fatalf("got version=%d name=%q err=%v", version, name, err)
	}

	// A malformed name must be an ERROR, not a skip: a skipped file is a
	// migration that silently never runs.
	for _, bad := range []string{
		"initial.sql",          // no version
		"1_initial.sql",        // not zero-padded to four digits
		"00001_initial.sql",    // five digits
		"0001-initial.sql",     // dash, not underscore
		"0001_Initial.sql",     // not snake_case
		"0001_initial.SQL",     // wrong extension case
		"0001_.sql",            // empty name
		"0000_reserved.sql",    // version zero
		"0001_initial.sql.bak", // trailing junk
	} {
		if _, _, err := parseFilename(bad); err == nil {
			t.Errorf("malformed filename %q was accepted", bad)
		}
	}
}

func TestLoadReturnsSortedVersions(t *testing.T) {
	migs, err := Load()
	if err != nil {
		t.Fatalf("Load: %s", err)
	}
	if len(migs) == 0 {
		t.Fatal("no embedded migrations")
	}
	if migs[0].Version != 1 {
		t.Fatalf("first migration is %04d, want 0001", migs[0].Version)
	}
	for i := 1; i < len(migs); i++ {
		if migs[i].Version <= migs[i-1].Version {
			t.Fatalf("migrations out of order: %04d after %04d", migs[i].Version, migs[i-1].Version)
		}
	}
	for _, m := range migs {
		if m.Checksum == 0 {
			t.Errorf("migration %04d_%s has zero checksum", m.Version, m.Name)
		}
	}
}

func TestPlanOrderingAndPending(t *testing.T) {
	migs := []Migration{
		{Version: 1, Name: "one", Checksum: 11},
		{Version: 2, Name: "two", Checksum: 22},
		{Version: 3, Name: "three", Checksum: 33},
	}

	// Nothing applied: everything pending, in version order — 0002 must
	// never be scheduled before 0001.
	pending, err := plan(migs, map[uint32]uint64{})
	if err != nil {
		t.Fatalf("plan: %s", err)
	}
	if len(pending) != 3 || pending[0].Version != 1 || pending[1].Version != 2 || pending[2].Version != 3 {
		t.Fatalf("wrong plan: %+v", pending)
	}

	// 1 applied with the right checksum: only 2 and 3 remain.
	pending, err = plan(migs, map[uint32]uint64{1: 11})
	if err != nil {
		t.Fatalf("plan: %s", err)
	}
	if len(pending) != 2 || pending[0].Version != 2 {
		t.Fatalf("wrong plan after partial apply: %+v", pending)
	}

	// Everything applied: a no-op.
	pending, err = plan(migs, map[uint32]uint64{1: 11, 2: 22, 3: 33})
	if err != nil {
		t.Fatalf("plan: %s", err)
	}
	if len(pending) != 0 {
		t.Fatalf("expected empty plan, got %+v", pending)
	}

	// A ledger version we have no file for (rolled-back binary against a
	// newer schema) is tolerated, not fatal.
	if _, err := plan(migs, map[uint32]uint64{1: 11, 2: 22, 3: 33, 4: 44}); err != nil {
		t.Fatalf("unknown ledger version should be tolerated: %s", err)
	}
}

func TestPlanChecksumMismatchFailsLoudly(t *testing.T) {
	migs := []Migration{{Version: 1, Name: "initial", Checksum: 11}}

	_, err := plan(migs, map[uint32]uint64{1: 999})
	if err == nil {
		t.Fatal("edited already-applied migration was not detected")
	}
	if !strings.Contains(err.Error(), "0001_initial") {
		t.Errorf("error does not name the migration: %s", err)
	}
	if !strings.Contains(err.Error(), "changed") {
		t.Errorf("error does not explain what happened: %s", err)
	}
}

func TestChecksumChangesWithContent(t *testing.T) {
	a := checksum([]byte("CREATE TABLE t (x UInt8) ENGINE = Memory"))
	b := checksum([]byte("CREATE TABLE t (x UInt16) ENGINE = Memory"))
	if a == b {
		t.Fatal("checksum did not change with content")
	}
	if a != checksum([]byte("CREATE TABLE t (x UInt8) ENGINE = Memory")) {
		t.Fatal("checksum is not stable")
	}
}
