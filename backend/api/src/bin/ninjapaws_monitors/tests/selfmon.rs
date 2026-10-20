//! The buffered writer and self-monitoring's points, stored in a scratch database, so a row that
//! does not fit its table fails here.

use std::sync::Arc;
use std::time::Duration;

use super::Scratch;
use crate::rows::{EvaluationRow, MetricPoint};
use crate::selfmon::{Previous, SelfMonitor};
use crate::telemetry::Counters;
use crate::writer::{DEFAULT_LIMITS, Limits, METRICS_LIMITS, Stated, TableWriter};

fn evaluation(at: i64) -> EvaluationRow {
    EvaluationRow {
        tenant_id: "acme".into(),
        monitor_id: 7,
        group_key: "*".into(),
        at,
        value: Some(1.0),
        state: "OK".into(),
        outcome: "evaluated".into(),
        detail: String::new(),
    }
}

async fn count(scratch: &Scratch, sql: &str) -> u64 {
    let rows: Vec<(u64,)> =
        scratch.reader().read(&ninjapaws_engine::Sql { text: sql.into(), parameters: Vec::new() }).await.unwrap();
    rows[0].0
}

#[tokio::test]
async fn a_writer_flushes_a_full_buffer_at_once_drops_past_its_limit_and_stores_the_rest_when_stopped() {
    let Some(scratch) = Scratch::new().await else { return };
    let limits = Limits { max_rows: 3, flush_interval: Duration::from_secs(3600), buffer_limit: 4, ..DEFAULT_LIMITS };
    let writer = TableWriter::start(scratch.writes(), "monitor_evaluations", limits);
    let now = chrono::Utc::now().timestamp_millis();

    writer.add(vec![evaluation(now), evaluation(now + 1), evaluation(now + 2)]);
    let deadline = tokio::time::Instant::now() + super::WAIT;
    while writer.stats().written < 3 {
        assert!(tokio::time::Instant::now() < deadline, "the full buffer was not flushed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    writer.add(vec![evaluation(now + 3)]);
    // Past the buffer's limit: dropped whole.
    writer.add((4..8).map(|i| evaluation(now + i)).collect());
    let stats = writer.stats();
    assert_eq!((stats.buffered, stats.written, stats.dropped, stats.failed), (1, 3, 4, 0));

    writer.stop().await;
    assert_eq!(writer.stats().written, 4);
    assert_eq!(count(&scratch, "SELECT count() FROM monitor_evaluations").await, 4);
}

#[tokio::test]
async fn self_monitoring_samples_the_process_the_writers_and_the_monitors_into_metrics() {
    let Some(scratch) = Scratch::new().await else { return };
    let metrics: Arc<TableWriter<MetricPoint>> = TableWriter::start(scratch.writes(), "metrics", METRICS_LIMITS);
    let counts = Arc::new(Counters::default());
    let monitor = SelfMonitor {
        tenant: "default".into(),
        host: "replica-1".into(),
        interval: Duration::from_secs(15),
        counts: counts.clone(),
        writers: vec![metrics.clone() as Arc<dyn Stated>],
        metrics: metrics.clone(),
    };
    let now = chrono::Utc::now().timestamp_millis();
    let mut previous = Previous::default();

    counts.evaluated.add(5);
    counts.entities.add(2);
    let first = monitor.sample(now, Duration::from_secs(1), &mut previous);
    counts.evaluated.add(3);
    let second = monitor.sample(now + 15_000, Duration::from_secs(16), &mut previous);

    let find = |points: &[MetricPoint], name: &str, tag: Option<(&str, &str)>| {
        points
            .iter()
            .find(|p| {
                p.metric == name && tag.is_none_or(|(k, v)| p.tags.get(k).is_some_and(|vs| vs == &vec![v.to_string()]))
            })
            .cloned()
    };
    let evaluated = Some(("outcome", "evaluated"));
    // A count is what was added since the last sample, so the first sample has none.
    assert!(find(&first, "ninjacat.monitor.evaluations", evaluated).is_none());
    let added = find(&second, "ninjacat.monitor.evaluations", evaluated).unwrap();
    assert_eq!((added.metric_type.as_str(), added.value), ("COUNT", 3.0));
    let entities = find(&first, "ninjacat.monitor.entities", None).unwrap();
    assert_eq!((entities.metric_type.as_str(), entities.value), ("GAUGE", 2.0));
    assert_eq!(
        (entities.tenant_id.as_str(), entities.host.as_str(), entities.source_type.as_str(), entities.interval),
        ("default", "replica-1", "ninjacat", 15)
    );
    // NaN, a peak with nothing observed, is no point.
    assert!(find(&first, "ninjacat.monitor.evaluation.lag", None).is_none());
    assert_eq!(find(&second, "ninjacat.node.uptime", None).unwrap().value, 16.0);
    assert!(find(&second, "ninjacat.storage.rows.written", Some(("table", "metrics"))).is_some());
    assert!(find(&second, "ninjacat.monitor.replicas", None).is_some());

    let stored = second.len() as u64;
    metrics.add(second);
    metrics.stop().await;
    assert_eq!(count(&scratch, "SELECT count() FROM metrics").await, stored);
    assert_eq!(count(&scratch, "SELECT count() FROM metrics WHERE env = [] AND series_id != 0").await, stored);
}
