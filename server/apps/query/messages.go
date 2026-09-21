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
}

// Point is one bucket of a series.
type Point struct {
	Timestamp time.Time `json:"t"`
	Value     float64   `json:"v"`
}

// Series is one line on the chart.
type Series struct {
	Metric string  `json:"metric"`
	Host   string  `json:"host"`
	Points []Point `json:"points"`
}

// SeriesResult is the answer to QuerySeries.
type SeriesResult struct {
	Series []Series
	Step   time.Duration
}

// Failed is returned instead of a result when the query could not run.
//
// It is a message rather than a Go error because it has to survive the trip
// back from another node once roles are split, and an error value does not
// serialize.
type Failed struct {
	Reason string
}
