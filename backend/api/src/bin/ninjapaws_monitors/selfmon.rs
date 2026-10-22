//! NinjaCat's own vital signs, stored as ordinary metrics so they can be graphed and alerted on
//! like any other: the process (`ninjacat.node.*`), each table's writer (`ninjacat.storage.*`) and
//! the monitors' readings (`ninjacat.monitor.*`).
//!
//! The process is read from /proc, so on another system its points are left out. A Rust process
//! has no garbage collector, so the .NET points of one (gc.cycles, cpu.gc_time, memory.alloc,
//! heap.live) have no counterpart.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use ninjapaws_storage::rows::metrics::MetricPoint;
use ninjapaws_storage::{Sink, WriterStats};

use crate::telemetry::{self, Counters, Kind};

/// The clock ticks per second of /proc's CPU times, fixed by the kernel's ABI.
const USER_HZ: f64 = 100.0;

pub struct SelfMonitor {
    pub tenant: String,
    pub host: String,
    pub interval: Duration,
    pub counts: Arc<Counters>,
    /// Its writers are sampled, and the points written through it.
    pub sink: Sink,
}

/// What the last sample read, so counters are sent as what was added since.
#[derive(Default)]
pub struct Previous {
    cpu: Option<(f64, f64)>,
    writers: BTreeMap<&'static str, WriterStats>,
    totals: BTreeMap<String, f64>,
}

/// The CPU seconds spent in user and in system mode.
fn cpu_seconds() -> Option<(f64, f64)> {
    let stat = std::fs::read_to_string("/proc/self/stat").ok()?;
    // The command name may hold spaces and parentheses; the fields after it do not.
    let (_, after) = stat.rsplit_once(')')?;
    let fields: Vec<&str> = after.split_whitespace().collect();
    let user: f64 = fields.get(11)?.parse().ok()?;
    let system: f64 = fields.get(12)?.parse().ok()?;
    Some((user / USER_HZ, system / USER_HZ))
}

/// A `Name: value` line of /proc/self/status, its number.
fn status_field(name: &str) -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let line = status.lines().find(|l| l.starts_with(name))?;
    line[name.len()..].split_whitespace().next()?.parse().ok()
}

impl SelfMonitor {
    /// Samples every interval until the task is aborted.
    pub async fn run(self) {
        let started = Instant::now();
        let mut previous = Previous::default();
        let mut every = tokio::time::interval(self.interval);
        every.tick().await;
        loop {
            every.tick().await;
            let points = self.sample(Utc::now(), started.elapsed(), &mut previous);
            self.sink.write(points);
        }
    }

    fn point(
        &self,
        now: DateTime<Utc>,
        metric_type: &str,
        name: &str,
        unit: &str,
        value: f64,
        table: Option<&str>,
    ) -> MetricPoint {
        let mut tags = BTreeMap::from([("runtime".to_string(), vec!["Rust".to_string()])]);
        if let Some(table) = table {
            tags.insert("table".to_string(), vec![table.to_string()]);
        }
        MetricPoint {
            tenant_id: self.tenant.clone(),
            timestamp: now,
            metric: name.to_string(),
            host: self.host.clone(),
            metric_type: metric_type.to_string(),
            source_type: "ninjacat".to_string(),
            unit: unit.to_string(),
            interval: self.interval.as_secs() as u32,
            value,
            tags,
            ..Default::default()
        }
    }

    pub fn sample(&self, now: DateTime<Utc>, uptime: Duration, previous: &mut Previous) -> Vec<MetricPoint> {
        let gauge = |name: &str, unit: &str, value: f64| self.point(now, "GAUGE", name, unit, value, None);
        let count = |name: &str, unit: &str, value: f64| self.point(now, "COUNT", name, unit, value.max(0.0), None);
        let mut points = vec![gauge("ninjacat.node.uptime", "second", uptime.as_secs_f64())];
        if let Some(threads) = status_field("Threads:") {
            points.push(gauge("ninjacat.node.threads", "", threads));
        }
        if let Some(kilobytes) = status_field("VmRSS:") {
            points.push(gauge("ninjacat.node.memory.used", "byte", kilobytes * 1024.0));
        }

        let cpu = cpu_seconds();
        if let (Some((user, system)), Some((last_user, last_system))) = (cpu, previous.cpu) {
            points.push(count("ninjacat.node.cpu.user_time", "second", user - last_user));
            points.push(count("ninjacat.node.cpu.system_time", "second", system - last_system));
        }
        previous.cpu = cpu;

        // A writer is made with its table's first write, so it shows from the sample after.
        for w in self.sink.stats() {
            let table = Some(w.table);
            points.push(self.point(now, "GAUGE", "ninjacat.storage.rows.buffered", "", w.buffered as f64, table));
            if let Some(last) = previous.writers.get(w.table) {
                let added = |now: u64, last: u64| now.saturating_sub(last) as f64;
                points.push(self.point(
                    now,
                    "COUNT",
                    "ninjacat.storage.rows.written",
                    "",
                    added(w.written, last.written),
                    table,
                ));
                points.push(self.point(
                    now,
                    "COUNT",
                    "ninjacat.storage.rows.dropped",
                    "",
                    added(w.dropped, last.dropped),
                    table,
                ));
                points.push(self.point(
                    now,
                    "COUNT",
                    "ninjacat.storage.rows.failed",
                    "",
                    added(w.failed, last.failed),
                    table,
                ));
            }
            previous.writers.insert(w.table, w);
        }

        for r in telemetry::readings(&self.counts) {
            if !r.value.is_finite() {
                continue;
            }
            let mut point = self.point(now, "GAUGE", &r.name, r.unit, r.value, None);
            for (key, value) in &r.tags {
                point.tags.insert(key.to_string(), vec![value.to_string()]);
            }
            match r.kind {
                Kind::Gauge => points.push(point),
                Kind::Count => {
                    let id = format!("{}{:?}", r.name, r.tags);
                    if let Some(last) = previous.totals.insert(id, r.value) {
                        point.metric_type = "COUNT".to_string();
                        point.value = (r.value - last).max(0.0);
                        points.push(point);
                    }
                }
            }
        }
        points
    }
}
