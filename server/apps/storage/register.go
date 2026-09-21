package storage

import (
	"fmt"

	"ergo.services/ergo/gen"
)

// RegisterTypes makes this package's message types serializable across nodes.
//
// Local messages need no registration: they travel as `any` with no encoding.
// REMOTE messages do — the receiving node has to know how to rebuild the value,
// and an unregistered type makes Send fail with an error rather than silently
// corrupting anything.
//
// We register even though everything currently runs in one node, because the
// plan is to split roles across nodes (ingest here, storage there). Registering
// later means a confusing runtime failure the first time the split happens;
// registering now costs nothing.
//
// Note RegisterType rejects pointer types, so all row types are registered by
// value — which is also why Row implementations have value receivers.
func RegisterTypes(node gen.Node) error {
	types := []any{
		WriteMetrics{},
		WriteSketches{},
		WriteCheckRuns{},
		WriteLogs{},
		WriteHosts{},
		WriteProcesses{},
		WriteEvents{},
		MetricPoint{},
		SketchRow{},
		CheckRunRow{},
		LogRow{},
		HostRow{},
		ProcessRow{},
		EventRow{},
	}
	if err := node.Network().RegisterTypes(types); err != nil {
		return fmt.Errorf("storage: register types: %w", err)
	}
	return nil
}
