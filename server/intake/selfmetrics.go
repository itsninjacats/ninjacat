package intake

import (
	"time"

	"github.com/itsninjacats/server/apps/storage"
)

// Metrics the intake emits about ITSELF, as opposed to the telemetry it
// receives. They travel the same path as everything else — a message to the
// metrics writer — so they are queryable in the panel with no special casing.
//
// The prefix mirrors apps/selfmon's "ninjacat.node.*": Datadog's own agent
// reports under "datadog.agent.*", and the same reasoning applies here. A
// reader should be able to tell at a glance that a metric describes ninjacat
// rather than the infrastructure ninjacat is watching.
//
// Why this exists at all: every intake decision to DISCARD something is
// invisible by construction. A log line is written once and read never; the
// operator finds out the inventory has a hole only by going looking for one.
// A metric turns that into something a dashboard can show and an alert can
// fire on.
const selfPrefix = "ninjacat.intake."

// SelfImagesSkipped counts container images dropped for having no identity to
// key on — neither a registry digest nor an image id. See ciRows.
const SelfImagesSkipped = "images.skipped"

// selfCountPoint builds one COUNT point. Split from the send so the shape can
// be asserted in a test without a running node.
//
// Interval stays 0 deliberately. selfmon samples on a timer and so reports the
// window its counts cover; this is event-driven — the value is what happened
// in one payload, and claiming an interval it does not have would be a lie the
// query layer would later divide by.
func selfCountPoint(tenant, host, name string, value float64, now time.Time, tags map[string][]string) storage.MetricPoint {
	return storage.MetricPoint{
		TenantID:   tenant,
		Timestamp:  now,
		Metric:     selfPrefix + name,
		Host:       host,
		MetricType: "COUNT",
		SourceType: "ninjacat",
		Interval:   0,
		Value:      value,
		Tags:       tags,
	}
}

// countSelf emits one self-metric. Fire-and-forget, like every other write
// from a handler: a self-metric must never be able to delay or fail the 202
// the agent is waiting for.
//
// The tenant is the one whose payload triggered this, not an operator tenant.
// "Which customer is shipping images with no digest" is the useful question,
// and answering it needs the point to sit in that customer's data.
func (a *Server) countSelf(tenant, host, name string, value float64, tags map[string][]string) {
	if value == 0 {
		return
	}
	point := selfCountPoint(tenant, host, name, value, time.Now().UTC(), tags)
	a.store(storage.MetricsWriter, storage.WriteMetrics{Points: []storage.MetricPoint{point}}, 1)
}
