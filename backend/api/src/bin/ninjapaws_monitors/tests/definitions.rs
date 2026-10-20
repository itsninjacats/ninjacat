//! The `monitor` reader against the table Drizzle migrated (F#: `DefinitionsTests.fs`).

use ninjapaws_engine::monitor::validate;
use ninjapaws_engine::query::plan::DEFAULT_RETENTION;

use super::ScratchPostgres;
use crate::definitions::{self, MonitorKey, Seen, parse_notification};

const CPU: &str = "avg(last_5m):avg:system.cpu.user{*} > 90";

fn ours(tenant: &str, seen: &[Seen]) -> Vec<i64> {
    seen.iter().filter(|s| s.key.tenant == tenant).map(|s| s.key.id).collect()
}

#[tokio::test]
async fn a_monitor_is_read_by_its_key_deleted_or_not() {
    let Some(pg) = ScratchPostgres::new().await else { return };
    let db = definitions::pool(pg.config.clone());
    let tenant = "acme";
    let live = pg.insert(tenant, CPU, "NULL").await;
    let deleted = pg.insert(tenant, CPU, "now()").await;

    let row = definitions::by_key(&db, &MonitorKey { tenant: tenant.into(), id: live })
        .await
        .unwrap()
        .expect("the monitor is read");
    assert_eq!(
        (row.key.tenant.as_str(), row.key.id, row.version, row.monitor_type.as_str(), row.query.as_str()),
        (tenant, live, 3, "query alert", CPU)
    );
    assert_eq!((row.updated_by, row.deleted_by, row.deleted_at), (None, None, None));

    // What is read is what the engine takes.
    let now = chrono::Utc::now().timestamp_millis();
    let m = validate::by_type(DEFAULT_RETENTION, now, &row.monitor_type, &row.query, &row.options).unwrap();
    assert_eq!(m.options().thresholds.critical, Some(90.0));

    let gone = definitions::by_key(&db, &MonitorKey { tenant: tenant.into(), id: deleted }).await.unwrap().unwrap();
    assert!(gone.deleted_at.is_some_and(|at| (at - now).abs() < 60_000));

    // Another tenant's id is not this tenant's monitor.
    assert_eq!(
        definitions::by_key(&db, &MonitorKey { tenant: "no-such-tenant".into(), id: live }).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn a_snapshot_holds_the_live_monitors_and_those_deleted_in_the_last_day() {
    let Some(pg) = ScratchPostgres::new().await else { return };
    let db = definitions::pool(pg.config.clone());
    let tenant = "acme";
    let live = pg.insert(tenant, CPU, "NULL").await;
    let deleted = pg.insert(tenant, CPU, "now()").await;
    pg.insert(tenant, CPU, "now() - interval '2 days'").await;

    let first = definitions::snapshot(&db, None).await.unwrap();
    assert_eq!(ours(tenant, &first.live), vec![live]);
    assert_eq!(ours(tenant, &first.deleted), vec![deleted]);
    assert_eq!(first.live.iter().find(|s| s.key.id == live).unwrap().version, 3);
    assert!((first.at - chrono::Utc::now().timestamp_millis()).abs() < 60_000);

    // Deletions are counted from the time given.
    let earlier = definitions::snapshot(&db, Some(first.at - 3_600_000)).await.unwrap();
    assert_eq!(ours(tenant, &earlier.deleted), vec![deleted]);
    let later = definitions::snapshot(&db, Some(first.at + 1000)).await.unwrap();
    assert!(ours(tenant, &later.deleted).is_empty());
    assert_eq!(ours(tenant, &later.live), vec![live]);
}

#[test]
fn a_notification_reads_back_from_the_panels_payload() {
    let seen = |tenant: &str, id, version| Some(Seen { key: MonitorKey { tenant: tenant.into(), id }, version });
    assert_eq!(parse_notification("acme/42/3"), seen("acme", 42, 3));
    assert_eq!(parse_notification("a/b/42/3"), seen("a/b", 42, 3));
    assert_eq!(parse_notification("acme/42"), None);
    assert_eq!(parse_notification("acme/42/x"), None);
    assert_eq!(parse_notification(""), None);
}

#[test]
fn a_key_reads_back_from_its_text() {
    let key = |tenant: &str, id| Some(MonitorKey { tenant: tenant.into(), id });
    assert_eq!(MonitorKey::parse("acme/42"), key("acme", 42));
    assert_eq!(MonitorKey::parse("a/b/42"), key("a/b", 42));
    assert_eq!(MonitorKey::parse("acme"), None);
    assert_eq!(MonitorKey::parse("/42"), None);
    assert_eq!(MonitorKey::parse("acme/x"), None);
    assert_eq!(MonitorKey { tenant: "a/b".into(), id: 42 }.text(), "a/b/42");
}
