package storage

import (
	"time"

	"github.com/ClickHouse/clickhouse-go/v2/lib/driver"
)

// metrics and sketches — the two point-in-time numeric tables. They share a
// file because they are the same signal in two shapes: a value, or the
// distribution the agent computed instead of one.

type WriteMetrics struct{ Points []MetricPoint }
type WriteSketches struct{ Sketches []SketchRow }

func (m WriteMetrics) rows() []Row  { return toRows(m.Points) }
func (m WriteSketches) rows() []Row { return toRows(m.Sketches) }

// MetricPoint is one row in ninjacat.metrics.
type MetricPoint struct {
	TenantID   string
	Timestamp  time.Time
	Metric     string
	Host       string
	MetricType string // GAUGE / COUNT / RATE
	SourceType string
	Unit       string
	Interval   uint32
	Value      float64
	// Tags is key -> every value seen: Datadog tags are a multiset, and two
	// tags may legally share a key (docs/decisions/0001-tags-are-a-multiset.md).
	// Same shape on every row type carrying Datadog tags.
	Tags map[string][]string

	// Origin is Datadog's provenance triple off v2's Metadata.Origin. Zero
	// means the sender did not send it, which is also what the proto default
	// and the enum's "unknown" mean — so no pointer is needed to keep absent
	// and zero apart. v1 carries no origin at all and leaves all three at 0.
	OriginProduct  uint32
	OriginCategory uint32
	OriginService  uint32

	// Resources is v2's [type, name] list minus the host entry, which has its
	// own column. Single-valued: a resource list is typed slots, not tags.
	// v1's undeclared `device` key is written here as device=<name> so one
	// query spans both wire versions.
	Resources map[string]string

	// Extra holds undeclared series keys, values JSON-encoded.
	Extra map[string]string
}

func (r MetricPoint) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Metric, r.Host,
		r.MetricType, r.SourceType, r.Unit, r.Interval, r.Value, orEmpty(r.Tags),
		r.OriginProduct, r.OriginCategory, r.OriginService,
		orEmpty(r.Resources), orEmpty(r.Extra))
}

// SketchRow is one DDSketch in ninjacat.sketches. Distribution metrics arrive
// as bucketed histograms rather than single values — see docs/datadog-agent.md.
type SketchRow struct {
	TenantID     string
	Timestamp    time.Time
	Metric       string
	Host         string
	Tags         map[string][]string
	Count        uint64
	Min          float64
	Max          float64
	Avg          float64
	Sum          float64
	BucketKeys   []int32
	BucketCounts []uint32

	// The same five columns as MetricPoint, for the same reasons. Resources
	// stays empty for the agent's own sketches — SketchPayload_Sketch has a
	// flat Host field, not a resource list — and exists so the two tables
	// answer the same questions from a source that does carry one.
	OriginProduct  uint32
	OriginCategory uint32
	OriginService  uint32
	Resources      map[string]string
	Extra          map[string]string
}

func (r SketchRow) AppendTo(b driver.Batch) error {
	return b.Append(r.TenantID, r.Timestamp, r.Metric, r.Host, orEmpty(r.Tags),
		r.Count, r.Min, r.Max, r.Avg, r.Sum,
		orEmptySlice(r.BucketKeys), orEmptySlice(r.BucketCounts),
		r.OriginProduct, r.OriginCategory, r.OriginService,
		orEmpty(r.Resources), orEmpty(r.Extra))
}

func init() {
	registerWriter(MetricsWriter, WriterConfig{
		Name: "metrics",
		Insert: `INSERT INTO metrics
			(tenant_id, timestamp, metric, host, metric_type, source_type, unit, interval, value, tags,
			 origin_product, origin_category, origin_service, resources, extra)`,
		MaxRows: 5000, FlushInterval: 2 * time.Second,
	}, MetricPoint{})

	registerWriter(SketchesWriter, WriterConfig{
		Name: "sketches",
		Insert: `INSERT INTO sketches
			(tenant_id, timestamp, metric, host, tags, count, min, max, avg, sum, bucket_keys, bucket_counts,
			 origin_product, origin_category, origin_service, resources, extra)`,
		MaxRows: 1000, FlushInterval: 5 * time.Second,
	}, SketchRow{})
}
