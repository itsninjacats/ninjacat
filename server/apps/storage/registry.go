package storage

import (
	"fmt"

	"ergo.services/ergo/gen"
)

// Per-file registration of writers and message types.
//
// WHY this exists: adding a table used to mean editing three shared lists —
// the writers slice in application.go, the types slice in register.go and the
// message/row declarations in messages.go. Three files everyone touches is
// three merge conflicts every time two tables are added in parallel, and the
// conflicts land in exactly the lists where a botched resolution is worst: a
// dropped writer silently stops a table, a dropped EDF type breaks remote
// delivery only once nodes are split.
//
// So a table owns ONE file and registers itself from init():
//
//	// apps/storage/rows_foo.go
//	const FooWriter = gen.Atom("storage_foo")
//
//	type WriteFoo struct{ Rows []FooRow }
//	func (m WriteFoo) rows() []Row { return toRows(m.Rows) }
//
//	type FooRow struct{ ... }
//	func (r FooRow) AppendTo(b driver.Batch) error { return b.Append(...) }
//
//	func init() {
//	    registerWriter(FooWriter, WriterConfig{
//	        Name:   "foo",
//	        Insert: `INSERT INTO foo (tenant_id, ...)`,
//	        MaxRows: 500, FlushInterval: 10 * time.Second,
//	    }, FooRow{})
//	    registerTypes(WriteFoo{}, FooRow{})
//	}
//
// Every table follows it, the ones that predate the registry included —
// rows_metrics.go through rows_raw.go. rows_raw.go is the newest and the
// cleanest to copy.
//
// The third argument is a ZERO-VALUE row of the table's row type, and it is
// there so the arity test needs no shared list either. storagetest walks
// Writers() and checks every Zero against its writer's INSERT — column count
// and nil maps/slices — so a table gets that test by registering, not by
// someone remembering to add a line to a file thirteen branches share. Leaving
// it out is a compile error in the file that owns the table, which is the
// point: the failure lands where the fix does.
//
// ORDERING, and why it is worth a paragraph. Go initialises package-level
// variables first, then runs every init() in the package — in the order the
// compiler receives the files, which the go tool sorts by filename. So the
// lists below are built in filename order: deterministic, and stable across
// machines and builds as long as nobody RENAMES a file. Writer order is only
// cosmetic (the supervisor is one_for_one; children do not depend on each
// other), but EDF TYPE order is not: Ergo assigns wire identities in
// registration order, so two nodes that registered the same types in a
// different order do not understand each other, and CLAUDE.md's rule ("never
// reorder or remove registered EDF types") lands here. That is exactly why
// the rows_*.go files carved out of messages.go do NOT call registerTypes:
// their types stay in register.go's original list, in their original order,
// and only tables added after the registry append through extraTypes.
// Renaming a rows_*.go file after it ships reorders that tail — do not, or
// accept that every node must be redeployed together.

// WriterSpec is one writer: the process name handlers send to, the config that
// names its table and its batching, and a zero-value row of the type that
// table takes. Exported for tests and tooling (storagetest asserts every row's
// arity against its writer's INSERT).
type WriterSpec struct {
	Name   gen.Atom
	Config WriterConfig

	// Zero is a zero-value row, carried so a caller can check a table without
	// knowing its row type. An empty row is deliberately the sample: it is the
	// shape that exposes both mistakes worth catching — an argument count that
	// drifted from the INSERT shows up whatever the values are, and a map or
	// slice field that skipped orEmpty is nil ONLY when nobody set it, which
	// is exactly what a real payload with no tags produces.
	Zero Row
}

// registeredWriters holds every writer, in the order the rows_*.go files
// registered them. There is no second list: a writer that is not here does not
// exist, which is what makes "add a table" a one-file change.
var registeredWriters []WriterSpec

// extraTypes holds the EDF types registered from files added AFTER the
// registry, appended after the original list in register.go. The types that
// list already names are not registered again here — see the ordering note
// above.
var extraTypes []any

// registerWriter adds one writer to the supervision tree. Call it from init()
// in the file that owns the table, passing a zero-value row of the type that
// table takes (see WriterSpec.Zero).
//
// A duplicate name panics. That is a programming error — two files claiming
// one process name — and it can only happen at init, before the node starts,
// so a panic here is a failed build run, never a production incident. The
// alternative (last registration wins) would silently route a table's rows
// into another table's writer.
func registerWriter(name gen.Atom, cfg WriterConfig, zero Row) {
	for _, w := range registeredWriters {
		if w.Name == name {
			panic(fmt.Sprintf("storage: writer %s registered twice", name))
		}
	}
	registeredWriters = append(registeredWriters, WriterSpec{Name: name, Config: cfg, Zero: zero})
}

// registerTypes adds message and row types to what RegisterTypes hands Ergo.
// Call it from init() with every Write* message AND every row type the file
// declares — the encoder needs the concrete element type of the slice, not
// just the message around it.
//
// Only for tables added after the registry: the original types are listed in
// register.go and must keep their place there.
func registerTypes(types ...any) {
	extraTypes = append(extraTypes, types...)
}

// Writers returns every registered writer, in registration (filename) order.
// It is what the supervisor builds its children from, and what tests and
// tooling walk to find every table.
func Writers() []WriterSpec {
	out := make([]WriterSpec, len(registeredWriters))
	copy(out, registeredWriters)
	return out
}
