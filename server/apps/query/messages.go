package query

import "time"

// Requests the query pool answers, and the results it sends back.
//
// These are plain values with no ClickHouse types in sight. That is the point:
// the panel asks a question, this package decides how to answer it, and the
// HTTP layer never learns SQL exists.

// ListMetrics asks for metric names, optionally narrowed by a substring.
type ListMetrics struct {
	TenantID string
	Search   string
	Limit    int
}

// MetricsList is the answer to ListMetrics.
type MetricsList struct {
	Names []string
}

// ListTagKeys asks which tag keys appear on a metric's points. An empty
// Metric means across every metric — the explorer asks that before a metric
// is picked.
type ListTagKeys struct {
	TenantID string
	Metric   string
}

// TagKeysList is the answer to ListTagKeys.
type TagKeysList struct {
	Keys []string
}

// ListTagValues asks for the distinct values one tag key holds, optionally
// narrowed by a substring — this is what fills the autocomplete dropdown
// after the user types "env:".
type ListTagValues struct {
	TenantID string
	Metric   string
	Key      string
	Search   string
	Limit    int
}

// TagValuesList is the answer to ListTagValues.
type TagValuesList struct {
	Values []string
}

// TagFilter narrows a query to points carrying a tag.
//
// Values within one filter are OR-ed: {Key: "kube_service", Values: ["a",
// "b"]} matches a point in either service. Separate filters are AND-ed.
// That split is exactly how a Datadog scope composes — repeating a key
// widens it, adding a new key narrows it — and it is also what the schema's
// multiset tags demand: a single point can itself carry kube_service:a AND
// kube_service:b, so matching is membership, never equality.
type TagFilter struct {
	Key    string
	Values []string
}

// ListHosts asks which hosts reported a given metric. An empty Metric means
// every host we have seen.
type ListHosts struct {
	TenantID string
	Metric   string
}

// HostsList is the answer to ListHosts.
type HostsList struct {
	Hosts []string
}

// QuerySeries asks for one metric over a time range, one line per host.
//
// Step is the bucket width. The caller picks it from the width of the chart
// rather than from the range, so a 24h view and a 1h view both come back with
// a sane number of points instead of 5760 and 240.
type QuerySeries struct {
	TenantID string
	Metric   string
	Hosts    []string
	From     time.Time
	To       time.Time
	Step     time.Duration

	// Aggregation applied inside each bucket: avg, min, max, sum or count.
	// Defaults to avg.
	Aggregation string

	// Tags narrows the query to points carrying these tags. See TagFilter for
	// how repeated keys compose.
	Tags []TagFilter

	// GroupBy switches the result from one series per host to one series per
	// combination of these tag keys' values. Empty keeps the host grouping.
	// The worker caps the length — see maxGroupByKeys.
	GroupBy []string
}

// Point is one bucket of a series.
type Point struct {
	Timestamp time.Time `json:"t"`
	Value     float64   `json:"v"`
}

// Series is one line on the chart.
//
// Name and Tags describe what the line represents regardless of how the query
// was grouped: grouped by host they are "<host>" and {"host": ...}, grouped
// by tags they are "key:value, key:value" (sorted by key, so the same group
// always gets the same name) and the matching map. Host stays populated for
// the host grouping so the panel page written against the old shape keeps
// working; omitempty hides it when a tag grouping makes it meaningless.
type Series struct {
	Metric string            `json:"metric"`
	Host   string            `json:"host,omitempty"`
	Name   string            `json:"name"`
	Tags   map[string]string `json:"tags"`
	Points []Point           `json:"points"`
}

// SeriesResult is the answer to QuerySeries.
type SeriesResult struct {
	Series []Series
	Step   time.Duration
}

// SearchLogs asks for raw log lines, newest first.
//
// Query is a free-text phrase matched against message; the scalar fields are
// exact matches on their columns; Tags composes like it does for metrics.
type SearchLogs struct {
	TenantID string
	Query    string
	Service  string
	Host     string
	Status   string
	Tags     []TagFilter
	From     time.Time
	To       time.Time
	Limit    int
}

// LogEntry is one log line.
//
// Tags keeps the array-valued shape the table stores — flattening it to one
// value per key here would re-introduce exactly the loss the schema was
// changed to prevent.
type LogEntry struct {
	Timestamp time.Time           `json:"timestamp"`
	Host      string              `json:"host"`
	Service   string              `json:"service"`
	Source    string              `json:"source"`
	Status    string              `json:"status"`
	Message   string              `json:"message"`
	Tags      map[string][]string `json:"tags"`
}

// LogsResult is the answer to SearchLogs. Count is how many lines matched in
// total, so the explorer can say "showing 200 of 12,431".
type LogsResult struct {
	Logs  []LogEntry
	Count uint64
}

// ListLogFacets asks for the sidebar counts of the log explorer: which
// services, hosts and statuses appear in a window, and how often.
type ListLogFacets struct {
	TenantID string
	From     time.Time
	To       time.Time
}

// FacetCount is one sidebar row: a value and how many lines carry it.
type FacetCount struct {
	Value string `json:"value"`
	Count uint64 `json:"count"`
}

// LogFacets is the answer to ListLogFacets.
type LogFacets struct {
	Services []FacetCount
	Hosts    []FacetCount
	Statuses []FacetCount
}

// Failed is returned instead of a result when the query could not run.
//
// It is a message rather than a Go error because it has to survive the trip
// back from another node once roles are split, and an error value does not
// serialize.
type Failed struct {
	Reason string
}
