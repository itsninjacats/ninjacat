//! Self-monitoring's points, stored in a scratch database, so a row that does not fit its table
//! fails here.

use std::sync::Arc;
use std::time::Duration;

use chrono::TimeDelta;
use ninjapaws_storage::rows::metrics::MetricPoint;

use super::Scratch;
use crate::selfmon::{Previous, SelfMonitor};
use crate::telemetry::Counters;

async fn count(scratch: &Scratch, sql: &str) -> u64 {
    let rows: Vec<(u64,)> =
        scratch.reader().read(&ninjapaws_engine::Sql { text: sql.into(), parameters: Vec::new() }).await.unwrap();
    rows[0].0
}

#[tokio::test]
async fn self_monitoring_samples_the_process_and_the_monitors_into_metrics() {
    let Some(scratch) = Scratch::new().await else { return };
    let clickhouse = scratch.writer();
    let counts = Arc::new(Counters::default());
    let monitor = SelfMonitor {
        tenant: "default".into(),
        host: "replica-1".into(),
        interval: Duration::from_secs(15),
        counts: counts.clone(),
        clickhouse: clickhouse.clone(),
    };
    let now = chrono::Utc::now();
    let mut previous = Previous::default();

    counts.evaluated.add(5);
    counts.entities.add(2);
    let first = monitor.sample(now, Duration::from_secs(1), &mut previous);
    counts.evaluated.add(3);
    let second = monitor.sample(now + TimeDelta::seconds(15), Duration::from_secs(16), &mut previous);

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
    assert!(find(&second, "ninjacat.monitor.replicas", None).is_some());

    // Inserted as `run` inserts them, one sample at a time.
    ninjapaws_storage::insert(&clickhouse, &first).await.unwrap();
    ninjapaws_storage::insert(&clickhouse, &second).await.unwrap();
    let stored = (first.len() + second.len()) as u64;
    assert_eq!(count(&scratch, "SELECT count() FROM metrics").await, stored);
    assert_eq!(count(&scratch, "SELECT count() FROM metrics WHERE env = [] AND series_id != 0").await, stored);
}
