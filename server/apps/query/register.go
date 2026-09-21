package query

import (
	"fmt"

	"ergo.services/ergo/gen"
)

// RegisterTypes makes this package's messages serializable across nodes.
//
// Both directions matter here, unlike in storage: a query travels out and a
// result travels back, so the request types, the result types and the value
// types nested inside them all have to be registered.
func RegisterTypes(node gen.Node) error {
	types := []any{
		ListMetrics{}, MetricsList{},
		ListHosts{}, HostsList{},
		ListTagKeys{}, TagKeysList{},
		ListTagValues{}, TagValuesList{},
		QuerySeries{}, SeriesResult{},
		Series{}, Point{}, TagFilter{},
		SearchLogs{}, LogsResult{}, LogEntry{},
		ListLogFacets{}, LogFacets{}, FacetCount{},
		Failed{},
	}
	if err := node.Network().RegisterTypes(types); err != nil {
		return fmt.Errorf("query: register types: %w", err)
	}
	return nil
}
