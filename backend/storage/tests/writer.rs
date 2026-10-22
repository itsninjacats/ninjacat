//! A table's writer through the sink, against a scratch database of the dev ClickHouse; the test
//! passes without asserting when ClickHouse is not running.

use std::time::Duration;

use ninjapaws_storage::{Limits, Sink, Table};
use serde::Serialize;

const DEV_CLICKHOUSE: &str = "localhost:8123";

/// A row of a table only this test has, with limits small enough to reach.
#[derive(clickhouse::Row, Serialize)]
struct Numbered {
    n: u64,
}

impl Table for Numbered {
    const WRITER: &'static str = "test_numbered";
    const TABLE: &'static str = "numbered";
    const LIMITS: Limits =
        Limits { max_rows: 3, flush_interval: Duration::from_secs(3600), buffer_limit: 4, ..Limits::DEFAULT };
}

fn numbered(from: u64, to: u64) -> Vec<Numbered> {
    (from..to).map(|n| Numbered { n }).collect()
}

fn client() -> clickhouse::Client {
    clickhouse::Client::default()
        .with_url(format!("http://{DEV_CLICKHOUSE}"))
        .with_user("ninjacat")
        .with_password("ninjacat")
}

#[tokio::test]
async fn a_writer_flushes_a_full_buffer_at_once_drops_past_its_limit_and_stores_the_rest_when_stopped() {
    let admin = client();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().subsec_nanos();
    let database = format!("ninjacat_storage_{}_{nanos}", std::process::id());
    if let Err(e) = admin.query(&format!("CREATE DATABASE {database}")).execute().await {
        eprintln!("skipped: ClickHouse is not reachable at {DEV_CLICKHOUSE}: {e}");
        return;
    }
    admin
        .query(&format!("CREATE TABLE {database}.numbered (n UInt64) ENGINE = MergeTree ORDER BY n"))
        .execute()
        .await
        .unwrap();
    // Synchronous inserts, so what a writer wrote can be counted at once.
    let sink = Sink::clickhouse(client().with_database(&database).with_setting("async_insert", "0"));
    let stats = || sink.stats().into_iter().find(|s| s.table == "numbered").unwrap();

    sink.write(numbered(0, 3));
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while stats().written < 3 {
        assert!(tokio::time::Instant::now() < deadline, "the full buffer was not flushed");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    sink.write(numbered(3, 4));
    // Past the buffer's limit: dropped whole.
    sink.write(numbered(4, 8));
    let s = stats();
    assert_eq!((s.buffered, s.written, s.dropped, s.failed), (1, 3, 4, 0));

    sink.stop().await;
    assert_eq!(stats().written, 4);
    let stored: u64 = admin.query(&format!("SELECT count() FROM {database}.numbered")).fetch_one().await.unwrap();
    assert_eq!(stored, 4);
    admin.query(&format!("DROP DATABASE {database}")).execute().await.unwrap();
}
