// Package storagetest is the shared test harness for the write layer.
//
// It exists because every new table repeats the same two checks, and both of
// them catch mistakes the compiler cannot see:
//
//   - the argument order and count in Row.AppendTo must match the column list
//     in the writer's INSERT, and nothing but a test ties the two together;
//   - the ClickHouse driver rejects nil maps and nil slices at Append time, so
//     a row built with no tags at all has to hand it empty containers.
//
// A table's own test file should be three lines:
//
//	func TestFooRowArity(t *testing.T) {
//	    storagetest.AssertArity(t, storage.FooWriter, storage.FooRow{})
//	}
//
// plus, when ClickHouse is available, a round trip through OpenScratchDB.
//
// This is a normal (non-_test) package so tests in ANY package can import it —
// intake's handler tests want the same assertions as storage's. It deliberately
// pulls in nothing that changes the shipping binary: it imports storage and
// schema, which the binary already contains, and testing, which the linker
// drops from any build that does not reference this package.
package storagetest

import (
	"fmt"
	"reflect"
	"strings"
	"testing"

	"ergo.services/ergo/gen"
	"github.com/ClickHouse/clickhouse-go/v2/lib/column"
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
	"github.com/itsninjacats/server/apps/storage"
)

// CaptureBatch is a driver.Batch that records what AppendTo hands it instead
// of talking to ClickHouse. Every other method is a no-op stub.
//
// Args holds the most recent Append (the common case: one row per batch);
// Calls holds every Append in order, for a test that appends several rows.
type CaptureBatch struct {
	Args  []any
	Calls [][]any
}

func (c *CaptureBatch) Append(v ...any) error {
	c.Args = v
	c.Calls = append(c.Calls, v)
	return nil
}

func (c *CaptureBatch) Abort() error                  { return nil }
func (c *CaptureBatch) AppendStruct(any) error        { return nil }
func (c *CaptureBatch) Column(int) driver.BatchColumn { return nil }
func (c *CaptureBatch) Flush() error                  { return nil }
func (c *CaptureBatch) Send() error                   { return nil }
func (c *CaptureBatch) IsSent() bool                  { return false }
func (c *CaptureBatch) Rows() int                     { return len(c.Calls) }
func (c *CaptureBatch) Columns() []column.Interface   { return nil }
func (c *CaptureBatch) Close() error                  { return nil }

// ColumnCount counts the columns in an INSERT statement's column list.
//
// It reads the first parenthesised group and counts the commas at depth one,
// which is all the column list of an INSERT can be — no nested calls, no
// types, no defaults. Deliberately not a SQL parser: the input is a constant
// in a WriterConfig, written by hand five lines above the AppendTo it has to
// agree with.
func ColumnCount(insert string) int {
	open := strings.IndexByte(insert, '(')
	if open < 0 {
		return 0
	}
	depth := 0
	cols := 1
	for i := open; i < len(insert); i++ {
		switch insert[i] {
		case '(':
			depth++
		case ')':
			depth--
			if depth == 0 {
				return cols
			}
		case ',':
			if depth == 1 {
				cols++
			}
		}
	}
	// Unbalanced parentheses: the caller's INSERT is malformed, and returning
	// a count would let an arity assertion "pass" against nonsense.
	return 0
}

// AssertArity checks one row type against its writer's INSERT: the number of
// arguments AppendTo produces must equal the number of columns declared, and
// none of them may be a nil map or slice.
//
// This is THE test to add when adding a table. A mismatch here is the failure
// mode that otherwise shows up as a ClickHouse error at runtime, on the first
// real payload, in a log nobody is reading.
func AssertArity(t testing.TB, writer gen.Atom, row storage.Row) {
	t.Helper()

	spec, ok := writerSpec(writer)
	if !ok {
		t.Fatalf("writer %s is not registered — add registerWriter to its rows_*.go init()", writer)
	}

	b := &CaptureBatch{}
	if err := row.AppendTo(b); err != nil {
		t.Fatalf("%s: AppendTo: %v", writer, err)
	}

	want := ColumnCount(spec.Config.Insert)
	if want == 0 {
		t.Fatalf("%s: cannot read a column list out of its INSERT:\n%s", writer, spec.Config.Insert)
	}
	if len(b.Args) != want {
		t.Fatalf("%s (%s): AppendTo produced %d arguments, the INSERT declares %d columns — "+
			"the two have drifted apart", writer, spec.Config.Name, len(b.Args), want)
	}

	checkNilContainers(t, fmt.Sprintf("%s (%s)", writer, spec.Config.Name), b.Args)
}

// AssertNoNilContainers appends a row and fails if any argument is a nil map
// or a nil slice — the driver rejects both, and a zero-value row is the
// cheapest way to catch a field that skipped orEmpty/orEmptySlice.
//
// Nil POINTERS are fine and deliberately allowed: they are how a Nullable
// column receives NULL, which several rows rely on to keep "not reported"
// distinct from zero.
func AssertNoNilContainers(t testing.TB, row storage.Row) {
	t.Helper()

	b := &CaptureBatch{}
	if err := row.AppendTo(b); err != nil {
		t.Fatalf("AppendTo: %v", err)
	}
	checkNilContainers(t, fmt.Sprintf("%T", row), b.Args)
}

func checkNilContainers(t testing.TB, what string, args []any) {
	t.Helper()
	for i, a := range args {
		if a == nil {
			continue // an untyped nil is not a container; the driver decides
		}
		v := reflect.ValueOf(a)
		switch v.Kind() {
		case reflect.Map, reflect.Slice:
			if v.IsNil() {
				t.Errorf("%s: argument %d is a nil %s — the driver rejects it; "+
					"pass the field through orEmpty/orEmptySlice", what, i, v.Kind())
			}
		}
	}
}

func writerSpec(name gen.Atom) (storage.WriterSpec, bool) {
	for _, w := range storage.Writers() {
		if w.Name == name {
			return w, true
		}
	}
	return storage.WriterSpec{}, false
}
