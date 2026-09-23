package storage

import (
	"fmt"
	"testing"
	"time"

	"ergo.services/ergo"
	"ergo.services/ergo/gen"
)

// TestRegisterTypesResolvesOnARealNode is the regression test for the startup
// failure CLAUDE.md warns every table's own tests miss: none of them ever
// call RegisterTypes on a real Ergo node, so a nested named struct type used
// in a Write*/Row (SketchSummary and DSMCommon inside rows_traces.go, both
// APMStatRow/DSMPipelineStatRow fields but never themselves passed to
// registerTypes) built and unit-tested clean while the actual binary panicked
// at boot with "storage: register types: RegisterTypes: ENP:R1: unresolvable
// types: ...".
//
// Ergo's EDF encoder (ergo.services/ergo/net/edf) resolves a struct-kind field
// by looking up its exact reflect.Type in a package-level registry populated
// only by RegisterType/RegisterTypes — unlike map, slice, array and pointer
// kinds, getEncoder (net/edf/encode.go) has no ad-hoc case for reflect.Struct,
// so an unregistered nested struct type fails the OUTER type's own
// registration ("(struct field encode) type ... must be registered first",
// net/edf/register.go's reflect.Struct case in RegisterTypeOf around line
// 427-455). RegisterTypesOf (net/edf/register.go ~line 1050) retries the whole
// batch across passes and only gives up once a pass makes no progress, which
// is why the outer type — not the missing nested one — is what the error
// message finally names as "unresolvable".
//
// A node with networking disabled still exercises the real check: Ergo
// registers its default wire protocol (net/proto's enp) in createNetwork
// BEFORE gen.NetworkOptions.Mode is consulted (node/network.go's
// createNetwork calls n.RegisterProto(n.defaultProto) unconditionally, and
// network.start only skips ACTUALLY STARTING that protocol when the mode is
// disabled) — so node.Network().RegisterTypes(...) resolves against the same
// registry the production binary uses, with no port bound and nothing that
// can collide with another test or a locally running ninjacat.
func TestRegisterTypesResolvesOnARealNode(t *testing.T) {
	name := gen.Atom(fmt.Sprintf("storage_edf_test_%d@localhost", time.Now().UnixNano()))

	node, err := ergo.StartNode(name, gen.NodeOptions{
		Network: gen.NetworkOptions{Mode: gen.NetworkModeDisabled},
		// Quiet: this test asserts a return value, not log output, and the
		// default logger's startup banner and per-process noise would
		// otherwise dominate `go test -v` for every other test in the package.
		Log: gen.LogOptions{DefaultLogger: gen.DefaultLoggerOptions{Disable: true}},
	})
	if err != nil {
		t.Fatalf("StartNode: %v", err)
	}
	defer node.StopWithTimeout(2 * time.Second)

	if err := RegisterTypes(node); err != nil {
		t.Fatalf("RegisterTypes: %v\n\nEvery Write* message type AND every row type it carries a slice of "+
			"must be listed in registerTypes(); a nested named struct field (or embed) used by one of "+
			"those row types — like rows_traces.go's SketchSummary/DSMCommon, or rows_rum.go's RumRequest "+
			"— must ALSO be listed, or Ergo's EDF encoder cannot resolve the outer type's fields.", err)
	}
}
