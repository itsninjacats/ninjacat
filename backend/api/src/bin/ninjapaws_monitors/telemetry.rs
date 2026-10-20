//! `ninjacat.monitor.*`, written from the entities' tasks and read by the sampler from another.
//! (F#: `Monitors/Telemetry.fs`.)

use std::sync::Mutex;
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Default)]
pub struct Counter(AtomicI64);

impl Counter {
    pub fn add(&self, n: i64) {
        self.0.fetch_add(n, Ordering::Relaxed);
    }

    pub fn set(&self, n: i64) {
        self.0.store(n, Ordering::Relaxed);
    }

    pub fn value(&self) -> f64 {
        self.0.load(Ordering::Relaxed) as f64
    }
}

/// The largest value seen since the last sample. NaN, which self-monitoring sends as no point,
/// when nothing was seen.
pub struct Peak(Mutex<f64>);

impl Default for Peak {
    fn default() -> Self {
        Peak(Mutex::new(f64::NAN))
    }
}

impl Peak {
    pub fn observe(&self, value: f64) {
        let mut peak = self.0.lock().unwrap();
        if peak.is_nan() || value > *peak {
            *peak = value;
        }
    }

    pub fn take(&self) -> f64 {
        std::mem::replace(&mut *self.0.lock().unwrap(), f64::NAN)
    }
}

#[derive(Default)]
pub struct Counters {
    pub evaluated: Counter,
    pub errored: Counter,
    /// The previous evaluation, its insert or a definition read was still running.
    pub skipped_busy: Counter,
    /// No gate slot within one interval: the replica is overloaded.
    pub skipped_gate: Counter,
    /// A tick spent storing the transitions of a failed insert.
    pub skipped_pending: Counter,
    /// The engine judged no group: the window was not full, or had no value.
    pub skipped_window: Counter,
    /// A model or a forecast has too little history.
    pub skipped_learning: Counter,
    /// An outlier monitor has fewer than 3 groups to compare.
    pub skipped_few_groups: Counter,
    /// Nanoseconds, the wait for the gate left out.
    pub evaluation_nanos: Counter,
    /// Seconds from the window's end to the result.
    pub evaluation_lag: Peak,
    pub entities: Counter,
    pub transitions_written: Counter,
    pub transitions_failed: Counter,
    pub transitions_pending: Counter,
    /// Taken at the poll, so a notifier that keeps failing reports a growing lag rather than none.
    pub notifier_lag: Peak,
    pub notifier_failed: Counter,
    /// The replicas whose lock session is up, this one included.
    pub replicas: Counter,
    /// The shards this replica holds.
    pub shards: Counter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Gauge,
    /// The value is a running total; what was added since the last sample is sent.
    Count,
}

/// One value the sampler reads.
pub struct Reading {
    pub name: String,
    pub unit: &'static str,
    pub kind: Kind,
    pub tags: Vec<(&'static str, &'static str)>,
    pub value: f64,
}

fn reading(name: &str, unit: &'static str, kind: Kind, tags: &[(&'static str, &'static str)], value: f64) -> Reading {
    Reading { name: format!("ninjacat.monitor.{name}"), unit, kind, tags: tags.to_vec(), value }
}

/// Every reading as it stands now; the peaks start over.
pub fn readings(c: &Counters) -> Vec<Reading> {
    use Kind::{Count, Gauge};
    let evaluations = |tags: &[(&'static str, &'static str)], counter: &Counter| {
        reading("evaluations", "", Count, tags, counter.value())
    };
    vec![
        evaluations(&[("outcome", "evaluated")], &c.evaluated),
        evaluations(&[("outcome", "error")], &c.errored),
        evaluations(&[("outcome", "skipped"), ("cause", "busy")], &c.skipped_busy),
        evaluations(&[("outcome", "skipped"), ("cause", "gate")], &c.skipped_gate),
        evaluations(&[("outcome", "skipped"), ("cause", "pending")], &c.skipped_pending),
        evaluations(&[("outcome", "skipped"), ("cause", "window")], &c.skipped_window),
        evaluations(&[("outcome", "skipped"), ("cause", "learning")], &c.skipped_learning),
        evaluations(&[("outcome", "skipped"), ("cause", "outlier_groups")], &c.skipped_few_groups),
        reading("evaluation.time", "second", Count, &[], c.evaluation_nanos.value() / 1e9),
        reading("evaluation.lag", "second", Gauge, &[], c.evaluation_lag.take()),
        reading("entities", "", Gauge, &[], c.entities.value()),
        reading("transitions.written", "", Count, &[], c.transitions_written.value()),
        reading("transitions.failed", "", Count, &[], c.transitions_failed.value()),
        reading("transitions.pending", "", Gauge, &[], c.transitions_pending.value()),
        reading("notifier.lag", "second", Gauge, &[], c.notifier_lag.take()),
        reading("notifier.failed", "", Count, &[], c.notifier_failed.value()),
        reading("replicas", "", Gauge, &[], c.replicas.value()),
        reading("shards", "", Gauge, &[], c.shards.value()),
    ]
}
