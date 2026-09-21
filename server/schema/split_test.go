package schema

import (
	"strings"
	"testing"
)

// The splitter's real workload is the real file, so test it against exactly
// that: the embedded 0001. The count is asserted, not derived — 17 CREATEs is
// what ClickHouse accepts one by one, and a splitter bug shows up here as a
// wrong count before it ever reaches a server.
func TestSplitStatementsRealInitialMigration(t *testing.T) {
	data, err := files.ReadFile("migrations/0001_initial.sql")
	if err != nil {
		t.Fatalf("read embedded migration: %s", err)
	}

	stmts := SplitStatements(string(data))
	if len(stmts) != 17 {
		t.Fatalf("expected 17 statements, got %d", len(stmts))
	}
	for i, s := range stmts {
		if !strings.HasPrefix(s, "CREATE ") {
			t.Errorf("statement %d does not start with CREATE: %.60q", i, s)
		}
		if strings.Contains(s, ";") {
			t.Errorf("statement %d still contains a semicolon: %.60q", i, s)
		}
	}

	// The tricky expressions from the real file must survive comment
	// stripping intact — they look nothing like comments but sit on lines
	// that also carry them.
	joined := strings.Join(stmts, "\n")
	for _, want := range []string{
		"tags['env'][1]",
		"Enum8('OK' = 0, 'WARNING' = 1, 'CRITICAL' = 2, 'UNKNOWN' = 3)",
		"sipHash64(tenant_id, metric, host, tags)",
		"ReplacingMergeTree(collected_at)",
	} {
		if !strings.Contains(joined, want) {
			t.Errorf("expected %q to survive splitting", want)
		}
	}
}

func TestSplitStatementsSemicolonInString(t *testing.T) {
	stmts := SplitStatements(`SELECT 'a;b'; SELECT 2`)
	if len(stmts) != 2 {
		t.Fatalf("expected 2 statements, got %d: %q", len(stmts), stmts)
	}
	if stmts[0] != `SELECT 'a;b'` {
		t.Errorf("semicolon inside string literal split the statement: %q", stmts[0])
	}
}

func TestSplitStatementsCommentWithSemicolon(t *testing.T) {
	in := "-- naglowek; z przecinkiem, i srednikiem\nSELECT 1; -- ogon; tez\nSELECT 2"
	stmts := SplitStatements(in)
	if len(stmts) != 2 || stmts[0] != "SELECT 1" || stmts[1] != "SELECT 2" {
		t.Fatalf("comments mishandled: %q", stmts)
	}
}

func TestSplitStatementsDashesInsideStringAreNotComments(t *testing.T) {
	stmts := SplitStatements(`SELECT 'a--b;c'`)
	if len(stmts) != 1 || stmts[0] != `SELECT 'a--b;c'` {
		t.Fatalf("-- inside a string was treated as a comment: %q", stmts)
	}
}

func TestSplitStatementsEscapedQuotes(t *testing.T) {
	// Backslash escape: the quote does not end the string, the semicolon
	// after it is still inside.
	stmts := SplitStatements(`SELECT 'it\'s;fine'`)
	if len(stmts) != 1 || stmts[0] != `SELECT 'it\'s;fine'` {
		t.Fatalf("backslash-escaped quote mishandled: %q", stmts)
	}

	// SQL-style doubled quote: close-then-reopen, equivalent for splitting.
	stmts = SplitStatements(`SELECT 'it''s;fine'`)
	if len(stmts) != 1 || stmts[0] != `SELECT 'it''s;fine'` {
		t.Fatalf("doubled quote mishandled: %q", stmts)
	}
}

func TestSplitStatementsBlockCommentAndEmptyFragments(t *testing.T) {
	stmts := SplitStatements("/* only; a comment */ ;;\n  ;\nSELECT/*x*/1;")
	if len(stmts) != 1 {
		t.Fatalf("expected 1 statement, got %d: %q", len(stmts), stmts)
	}
	// The block comment must leave a separator behind, not glue tokens.
	if stmts[0] != "SELECT 1" {
		t.Errorf("block comment removal glued tokens: %q", stmts[0])
	}
}
