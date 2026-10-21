//! A service check monitor's SQL against real `check_runs` rows, in a scratch database.

use ninjapaws_engine::TenantId;
use ninjapaws_engine::monitor::check;
use ninjapaws_engine::monitor::evaluate::{Period, Reading, Streak};
use ninjapaws_engine::query::execute::Executor;

use super::Scratch;

/// The top of the current hour, in unix seconds.
fn at_seconds() -> i64 {
    chrono::Utc::now().timestamp() / 3600 * 3600
}

/// One run `seconds_ago` before `at`; tags as ClickHouse's map literal.
fn run(tenant: &str, seconds_ago: i64, host: &str, status: &str, message: &str, tags: &str) -> String {
    format!("('{tenant}', toDateTime({}), 'c', '{host}', '{status}', '{message}', {tags})", at_seconds() - seconds_ago)
}

async fn insert(scratch: &Scratch, runs: &[String]) {
    let values = runs.join(", ");
    scratch
        .run(&format!("INSERT INTO {{db}}.check_runs (tenant_id, timestamp, check_name, host, status, message, tags) VALUES {values}"))
        .await
        .unwrap();
}

const PROD: &str = "map('env', ['prod'])";

async fn seeded(scratch: &Scratch) {
    insert(
        scratch,
        &[
            run("acme", 10, "h1", "CRITICAL", "down", PROD),
            run("acme", 25, "h1", "CRITICAL", "down", PROD),
            run("acme", 40, "h1", "OK", "up", PROD),
            run("acme", 55, "h1", "OK", "up", PROD),
            run("acme", 10, "h2", "OK", "up", "map('env', ['dev'])"),
            run("acme", 10, "", "UNKNOWN", "no answer", "map('kube_namespace', ['ns'], 'cluster', [''])"),
            // Newest of all, the same host and tags, another tenant's.
            run("other", 5, "h1", "WARNING", "slow", PROD),
        ],
    )
    .await;
}

async fn evaluated(scratch: &Scratch, query: &str) -> Vec<(String, Vec<Streak>)> {
    let c = check::definition(query, "").unwrap();
    let e = check::evaluate(&scratch.reader(), &TenantId("acme".into()), 100, &c, at_seconds() * 1000).await.unwrap();
    e.groups
        .into_iter()
        .map(|g| match g.reading {
            Reading::Streaks(s) => (g.key, s),
            other => panic!("{other:?}"),
        })
        .collect()
}

fn streak(latest: i32, critical: i32, ok: i32, unknown: i32, ok_since_unknown: i32) -> Streak {
    Streak { latest, critical, warning: 0, ok, unknown, ok_since_unknown }
}

#[tokio::test]
async fn the_newest_runs_of_the_tenants_matching_series_statuses_as_the_agent_sends_them() {
    let Some(scratch) = Scratch::new().await else { return };
    seeded(&scratch).await;

    let h1 = vec![("host:h1".to_string(), vec![streak(2, 2, 0, 0, 0)])];
    assert_eq!(evaluated(&scratch, r#""c".over("env:prod").by("host").last(3).count_by_status()"#).await, h1);
    assert_eq!(
        evaluated(&scratch, r#""c".over("*").exclude("env:dev").by("host").last(2).count_by_status()"#).await,
        h1
    );
}

#[tokio::test]
async fn a_run_with_no_host_is_na_by_host_and_grouped_by_its_tags() {
    let Some(scratch) = Scratch::new().await else { return };
    seeded(&scratch).await;

    assert_eq!(
        evaluated(&scratch, r#""c".over("*").by("kube_namespace").last(2).count_by_status()"#).await,
        vec![("kube_namespace:ns".to_string(), vec![streak(3, 0, 0, 1, 0)])]
    );
}

#[tokio::test]
async fn by_star_is_a_group_per_series_and_a_run_stored_twice_counts_once() {
    let Some(scratch) = Scratch::new().await else { return };
    seeded(&scratch).await;
    insert(&scratch, &[run("acme", 10, "h1", "CRITICAL", "down", PROD)]).await;

    let mut groups = evaluated(&scratch, r#""c".by("*").last(2).count_by_status()"#).await;
    groups.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        groups,
        vec![
            ("cluster,kube_namespace:ns".to_string(), vec![streak(3, 0, 0, 1, 0)]),
            ("env:dev,host:h2".to_string(), vec![streak(0, 0, 1, 0, 1)]),
            ("env:prod,host:h1".to_string(), vec![streak(2, 2, 0, 0, 0)]),
        ]
    );
}

#[tokio::test]
async fn n_known_runs_of_each_series_are_read_past_unknown_ones_and_the_newest_unknown_in_a_row() {
    let Some(scratch) = Scratch::new().await else { return };
    let statuses = ["UNKNOWN", "UNKNOWN", "CRITICAL", "UNKNOWN", "CRITICAL", "UNKNOWN", "CRITICAL", "OK", "OK"];
    let runs: Vec<String> =
        statuses.iter().enumerate().map(|(i, s)| run("acme", 10 + 15 * i as i64, "h1", s, "", PROD)).collect();
    insert(&scratch, &runs).await;

    let only = async |last: i32| match evaluated(&scratch, &format!(r#""c".by("host").last({last}).count_by_status()"#))
        .await
        .as_slice()
    {
        [(key, s)] if key == "host:h1" && s.len() == 1 => s[0],
        other => panic!("{other:?}"),
    };

    let q = check::query(r#""c".last(3).count_by_status()"#).unwrap();
    let at = at_seconds() * 1000;
    let rows = scratch
        .reader()
        .rows(check::sql(&TenantId("acme".into()), &q, &Period { from: at - 7_200_000, to: at }))
        .await
        .unwrap();
    let mut values: Vec<f64> = rows.iter().map(|r| r.value).collect();
    values.sort_by(|a, b| b.total_cmp(a));

    // last(3): three CRITICAL runs past the UNKNOWN ones, and three of the four UNKNOWN.
    assert_eq!(values, vec![3.0, 3.0, 3.0, 2.0, 2.0, 2.0]);
    assert_eq!(only(3).await, Streak { latest: 3, critical: 3, warning: 0, ok: 0, unknown: 2, ok_since_unknown: 0 });
    assert_eq!(only(1).await, Streak { latest: 3, critical: 1, warning: 0, ok: 0, unknown: 2, ok_since_unknown: 0 });
}

#[tokio::test]
async fn the_checks_runs_come_with_their_messages_and_tags() {
    let Some(scratch) = Scratch::new().await else { return };
    seeded(&scratch).await;

    let q = check::query(r#""c".over("*").exclude("env:prod").last(1).count_by_status()"#).unwrap();
    let at = at_seconds() * 1000;
    let sql = check::runs(&TenantId("acme".into()), &q, at - 3_600_000, at);
    let mut rows: Vec<(i64, String, String, String, Vec<String>)> = scratch.reader().read(&sql).await.unwrap();
    for row in &mut rows {
        row.4.sort();
    }
    rows.sort_by(|a, b| a.1.cmp(&b.1));

    let at10 = (at_seconds() - 10) * 1000;
    assert_eq!(
        rows,
        vec![
            (
                at10,
                String::new(),
                "UNKNOWN".into(),
                "no answer".into(),
                vec!["cluster".into(), "kube_namespace:ns".into()]
            ),
            (at10, "h2".into(), "OK".into(), "up".into(), vec!["env:dev".into()]),
        ]
    );
}
