package storagetest_test

import (
	"testing"

	"github.com/itsninjacats/server/apps/storage"
	"github.com/itsninjacats/server/apps/storage/storagetest"
)

// Every registered writer's row must produce exactly as many arguments as its
// INSERT declares columns, with no nil maps or slices among them. This is the
// one invariant of the write layer that the compiler cannot see: AppendTo
// passes a variadic []any, so adding a column to the INSERT and forgetting the
// argument (or the reverse) builds cleanly and fails on the first real
// payload, inside a background flush goroutine, in a log.
//
// Note there is no list of tables here. The sample row travels with the
// registration (WriterSpec.Zero), so this test covers a new table the moment
// it registers itself, and a table is never a reason to edit this file — which
// matters when a dozen of them are being added on a dozen branches at once.
func TestEveryWriterRowMatchesItsInsert(t *testing.T) {
	for _, w := range storage.Writers() {
		if w.Zero == nil {
			continue // reported by TestEveryWriterCarriesAZeroRow
		}
		t.Run(w.Config.Name, func(t *testing.T) {
			storagetest.AssertArity(t, w.Name, w.Zero)
		})
	}
}

// A writer registered with a nil Zero has no arity test at all, and the gap is
// silent: the loop above would simply skip it. Fail loudly instead.
//
// registerWriter takes the zero row as a parameter, so omitting it is a
// compile error in the file that owns the table — this only catches an
// explicit nil, and the core writers in application.go, whose rows are paired
// up in registry.go's coreZeroRows.
func TestEveryWriterCarriesAZeroRow(t *testing.T) {
	for _, w := range storage.Writers() {
		if w.Zero == nil {
			t.Errorf("writer %s (table %s) registered a nil Zero row — pass a zero-value "+
				"row to registerWriter, otherwise its AppendTo is never checked against its INSERT",
				w.Name, w.Config.Name)
		}
	}
}

// The registry is only worth trusting if it is complete, and "complete" has a
// lower bound we can state: the thirteen tables that shipped before the
// registry, plus raw_payloads. A refactor that dropped writers would otherwise
// turn the loop above into a test that passes by iterating nothing.
func TestWritersIsNotEmpty(t *testing.T) {
	const atLeast = 14
	if n := len(storage.Writers()); n < atLeast {
		t.Errorf("Writers() returned %d specs, want at least %d — writers have gone missing", n, atLeast)
	}
}

// ColumnCount is the other half of AssertArity and deserves its own pinning:
// the column lists it reads span several lines and hold comments' worth of
// whitespace, and an off-by-one here would make every arity test agree with
// the wrong number.
func TestColumnCount(t *testing.T) {
	cases := []struct {
		name string
		in   string
		want int
	}{
		{"single column", "INSERT INTO t (a)", 1},
		{"one line", "INSERT INTO t (a, b, c)", 3},
		{
			name: "multi-line, as the configs are written",
			in: `INSERT INTO logs
				(tenant_id, timestamp, host, service, source, status, message, tags)`,
			want: 8,
		},
		{"no column list", "INSERT INTO t", 0},
		{"unbalanced parentheses are not a count", "INSERT INTO t (a, b", 0},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			if got := storagetest.ColumnCount(tc.in); got != tc.want {
				t.Errorf("ColumnCount = %d, want %d", got, tc.want)
			}
		})
	}
}
