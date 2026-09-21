package selfmon

import (
	"time"

	"ergo.services/ergo/gen"
	"github.com/itsninjacats/server/apps/storage"
)

// Metric name prefix. Datadog namespaces its own agent metrics under
// "datadog.agent.*"; ours live under "ninjacat.node.*" for the same reason —
// so a user can tell our telemetry from their own at a glance.
const prefix = "ninjacat.node."

// counters is the previous sample of every monotonic counter the node keeps.
//
// The node reports these as lifetime totals, but a total is rarely what you
// want to look at: "3814 processes spawned since boot" says nothing, while
// "12 spawned in the last 15s" says a supervisor is flapping. So we keep the
// previous sample and emit the difference, which is what Datadog's COUNT type
// means — a delta over the interval, not a running sum.
//
// The first tick has nothing to subtract from, so it emits gauges only.
type counters struct {
	processesSpawned     uint64
	processesSpawnFailed uint64
	processesTerminated  uint64
	sendErrorsLocal      uint64
	sendErrorsRemote     uint64
	callErrorsLocal      uint64
	callErrorsRemote     uint64
	gcCycles             uint64
	logMessages          [6]uint64
	cpuTimeGC            float64
	cpuTimeTotal         float64
	userTime             int64
	systemTime           int64
}

// logLevelNames indexes NodeShortInfo.LogMessages.
var logLevelNames = [6]string{"trace", "debug", "info", "warning", "error", "panic"}

// builder accumulates points for one sample so the callers below stay short.
type builder struct {
	points   []storage.MetricPoint
	tenant   string
	host     string
	now      time.Time
	interval uint32
	tags     map[string]string
}

func (b *builder) add(metricType, name, unit string, value float64) {
	b.points = append(b.points, storage.MetricPoint{
		TenantID: b.tenant, Timestamp: b.now,
		Metric: prefix + name, Host: b.host,
		MetricType: metricType, SourceType: "ninjacat",
		Unit: unit, Interval: b.interval, Value: value,
		Tags: b.tags,
	})
}

func (b *builder) gauge(name, unit string, value float64) { b.add("GAUGE", name, unit, value) }
func (b *builder) count(name, unit string, value float64) { b.add("COUNT", name, unit, value) }

// addTagged emits a point carrying one extra tag on top of the shared set.
// Used for per-level log counts, where the level belongs in a tag rather than
// in six separate metric names.
func (b *builder) addTagged(metricType, name, unit string, value float64, key, val string) {
	tags := make(map[string]string, len(b.tags)+1)
	for k, v := range b.tags {
		tags[k] = v
	}
	tags[key] = val
	b.points = append(b.points, storage.MetricPoint{
		TenantID: b.tenant, Timestamp: b.now,
		Metric: prefix + name, Host: b.host,
		MetricType: metricType, SourceType: "ninjacat",
		Unit: unit, Interval: b.interval, Value: value,
		Tags: tags,
	})
}

// noMemoryLimit is what the node reports when GOMEMLIMIT is unset. Emitting it
// would put a 9.2-exabyte spike on the chart, so the metric is skipped instead.
const noMemoryLimit = uint64(1<<63 - 1)

// build turns one node sample into metric points, and returns the counters to
// remember for the next call.
//
// prev is nil on the very first sample; delta metrics are then skipped rather
// than emitted as their absolute totals, which would be a false spike.
func build(info gen.NodeShortInfo, prev *counters, tenant, host string, interval time.Duration, now time.Time) ([]storage.MetricPoint, counters) {
	b := &builder{
		tenant: tenant, host: host, now: now,
		interval: uint32(interval.Seconds()),
		tags:     identityTags(info),
		points:   make([]storage.MetricPoint, 0, 40),
	}

	// --- node -----------------------------------------------------------
	b.gauge("uptime", "second", float64(info.Uptime))

	// Creation is the node incarnation id — a different value means the node
	// was restarted. It is a METRIC and not a tag on purpose: as a tag it
	// would start a brand new time series on every restart, and a year of
	// deploys would leave hundreds of dead series behind every metric. As a
	// value it costs one series forever, and a step on the chart says
	// "restarted here" without any ambiguity.
	//
	// Ergo sets it to the start timestamp, so it is numerically close to
	// (now - uptime). Uptime tells you the same thing most of the time; this
	// one also survives the case where a restart lands between two samples.
	b.gauge("creation", "", float64(info.Creation))
	b.gauge("goroutines", "", float64(info.Goroutines))
	b.gauge("peers", "", float64(len(info.Peers)))

	// --- processes (actors, not OS processes) ---------------------------
	b.gauge("processes.total", "", float64(info.ProcessesTotal))
	b.gauge("processes.running", "", float64(info.ProcessesRunning))
	b.gauge("processes.wait_response", "", float64(info.ProcessesWaitResponse))
	b.gauge("processes.zombee", "", float64(info.ProcessesZombee))

	// --- applications ---------------------------------------------------
	b.gauge("applications.total", "", float64(info.ApplicationsTotal))
	b.gauge("applications.running", "", float64(info.ApplicationsRunning))

	// --- memory ---------------------------------------------------------
	b.gauge("memory.used", "byte", float64(info.MemoryUsed))
	b.gauge("memory.alloc", "byte", float64(info.MemoryAlloc))
	b.gauge("heap.live", "byte", float64(info.HeapLive))
	b.gauge("heap.goal", "byte", float64(info.HeapGoal))
	if info.MemoryLimit != noMemoryLimit {
		b.gauge("memory.limit", "byte", float64(info.MemoryLimit))
	}

	if prev == nil {
		return b.points, snapshot(info)
	}

	// --- deltas ---------------------------------------------------------
	//
	// Counters only grow, but a node restart resets them to zero. Without a
	// guard the next sample would report a huge negative delta, so a counter
	// that went backwards is treated as a restart and skipped for one tick.
	b.count("processes.spawned", "", delta(info.ProcessesSpawned, prev.processesSpawned))
	b.count("processes.spawn_failed", "", delta(info.ProcessesSpawnFailed, prev.processesSpawnFailed))
	b.count("processes.terminated", "", delta(info.ProcessesTerminated, prev.processesTerminated))

	b.count("send_errors.local", "", delta(info.SendErrorsLocal, prev.sendErrorsLocal))
	b.count("send_errors.remote", "", delta(info.SendErrorsRemote, prev.sendErrorsRemote))
	b.count("call_errors.local", "", delta(info.CallErrorsLocal, prev.callErrorsLocal))
	b.count("call_errors.remote", "", delta(info.CallErrorsRemote, prev.callErrorsRemote))

	b.count("gc.cycles", "", delta(info.GCCycles, prev.gcCycles))
	b.count("cpu.gc_time", "second", deltaF(info.CPUTimeGC, prev.cpuTimeGC))
	b.count("cpu.total_time", "second", deltaF(info.CPUTimeTotal, prev.cpuTimeTotal))
	b.count("cpu.user_time", "second", deltaF(nsToSec(info.UserTime), nsToSec(prev.userTime)))
	b.count("cpu.system_time", "second", deltaF(nsToSec(info.SystemTime), nsToSec(prev.systemTime)))

	// Log volume by level. One metric, six series — "how many errors per
	// minute" is then a filter, not a different metric name.
	for i, name := range logLevelNames {
		b.addTagged("COUNT", "log_messages", "", delta(info.LogMessages[i], prev.logMessages[i]), "level", name)
	}

	return b.points, snapshot(info)
}

// identityTags describes WHAT this node is. Everything here must be stable
// across restarts, because a tag that changes creates a new time series.
//
// That is why version and framework belong here but Creation does not:
// a version changes when you deploy, and a new series at a deploy boundary
// is exactly what you want — it lets you put v0.3 and v0.4 side by side and
// see which one leaks. A restart is not a new thing, it is the same thing
// again, so it must not split the series.
func identityTags(info gen.NodeShortInfo) map[string]string {
	tags := map[string]string{
		"node":      string(info.Name),
		"mode":      info.Mode.String(),
		"version":   orUnknown(info.Version.Release),
		"framework": orUnknown(info.Framework.Release),
	}
	// Commit is empty unless the binary was built from a tagged or committed
	// tree. When present it is the most precise answer to "which build is
	// this", which is the question you ask once a chart looks wrong.
	if c := info.Version.Commit; c != "" {
		tags["commit"] = c
	}
	return tags
}

func orUnknown(s string) string {
	if s == "" {
		return "unknown"
	}
	return s
}

func snapshot(info gen.NodeShortInfo) counters {
	return counters{
		processesSpawned:     info.ProcessesSpawned,
		processesSpawnFailed: info.ProcessesSpawnFailed,
		processesTerminated:  info.ProcessesTerminated,
		sendErrorsLocal:      info.SendErrorsLocal,
		sendErrorsRemote:     info.SendErrorsRemote,
		callErrorsLocal:      info.CallErrorsLocal,
		callErrorsRemote:     info.CallErrorsRemote,
		gcCycles:             info.GCCycles,
		logMessages:          info.LogMessages,
		cpuTimeGC:            info.CPUTimeGC,
		cpuTimeTotal:         info.CPUTimeTotal,
		userTime:             info.UserTime,
		systemTime:           info.SystemTime,
	}
}

// delta returns now-before, or 0 when the counter went backwards (restart).
func delta(now, before uint64) float64 {
	if now < before {
		return 0
	}
	return float64(now - before)
}

func deltaF(now, before float64) float64 {
	if now < before {
		return 0
	}
	return now - before
}

func nsToSec(ns int64) float64 { return float64(ns) / 1e9 }
