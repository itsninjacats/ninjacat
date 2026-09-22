package storage

import (
	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// The shared machinery of the write layer: the Row interface every table
// implements, the plumbing that lets one writer serve them all, and the nil
// guards every AppendTo goes through.
//
// These are deliberately independent of the HTTP layer: storage knows nothing
// about Gin or Datadog wire formats. Handlers translate their own structures
// into these and send them. That way a second source (OTLP) can be plugged in
// without touching the write path.
//
// THE ROW TYPES ARE NOT HERE. Each table family owns a rows_*.go file holding
// its rows, its Write* messages and the init() that registers its writers:
//
//	rows_metrics.go     metrics, sketches
//	rows_checks.go      check_runs
//	rows_logs.go        logs
//	rows_hosts.go       hosts
//	rows_events.go      events
//	rows_process.go     processes
//	rows_k8s.go         k8s_resources, k8s_manifests, k8s_cluster, k8s_actions
//	rows_containers.go  container_events, container_images
//	rows_raw.go         raw_payloads
//
// They used to live in this file, in one list, and that list was a merge
// conflict every time two tables were touched at once — in the worst possible
// place, since a resolution that drops a row type or a writer entry compiles
// fine and simply stops a table. One file per family means work on the
// Kubernetes tables and work on the log tables never meet, and a new table is
// a new file rather than an edit to a shared one. The recipe is in
// registry.go's doc comment.

// Row is anything that knows how to add itself to a prepared ClickHouse batch.
//
// This is what lets a single BatchWriter implementation serve every table:
// the writer owns buffering, flushing and backpressure, while each row type
// owns its own column order.
type Row interface {
	AppendTo(batch driver.Batch) error
}

// Messages sent to writers. Delivered with Send (asynchronous), so the handler
// never waits — the agent gets its 202 immediately.
//
// Why an actor instead of writing straight from the handler:
//
//   - ClickHouse dies from many small INSERTs ("Too many parts"). Something has
//     to buffer, and an actor processes messages one at a time, so the buffer
//     needs no locking.
//   - This is the single point every datapoint passes through — the natural
//     home for cardinality limits and tag stripping later on.
//
// Note this is the OPPOSITE of the API key situation. There the read is
// synchronous and blocks the response, so it goes through an atomic.Pointer.
// Here the write is asynchronous and can lag — an actor is ideal.
//
// IMPORTANT: each message carries a CONCRETE slice, not []Row.
//
// A single generic message with an interface field looks tidier, but Ergo
// cannot serialize it for delivery to another node:
//
//	RegisterTypes: unresolvable types: storage.WriteRows
//
// The encoder has to know what is inside the slice, and an interface does not
// tell it. Since the whole point of splitting storage into its own application
// is to run it on a separate node one day, the wire format must stay concrete.
// The Row interface still does its job — inside the writer's buffer, where
// nothing is serialized.

// rowsProvider lets BatchWriter stay generic despite the concrete messages:
// it asks the message for its rows instead of knowing the types itself.
// This interface is never serialized — it is only used locally, after the
// message has already arrived as a concrete value.
type rowsProvider interface {
	rows() []Row
}

func toRows[T Row](items []T) []Row {
	out := make([]Row, len(items))
	for i, it := range items {
		out[i] = it
	}
	return out
}

// flush tells a writer to drain its buffer regardless of size.
// Sent periodically by a timer.
type flush struct{}

// orEmpty guards against nil maps — the ClickHouse driver rejects them.
// Generic since the Kubernetes tables brought map values beyond string
// (Counts is int64, the cluster version spreads are uint32); the multiset tag
// maps (map[string][]string) and the older string-to-string callers all infer
// their types unchanged.
func orEmpty[K comparable, V any](m map[K]V) map[K]V {
	if m == nil {
		return map[K]V{}
	}
	return m
}

// orEmptySlice is the same guard for slices — the driver rejects nil there
// too, which SketchRow handles inline. Factored out here because the
// container rows carry several slices each and the inline form stops
// paying its way.
func orEmptySlice[T any](s []T) []T {
	if s == nil {
		return []T{}
	}
	return s
}
