//! The process as `app` builds it, called in memory, with the panel's token, on a scratch database
//! of the dev ClickHouse. The tests that need ClickHouse pass without asserting when it is not
//! running.

use http_body_util::BodyExt;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode, get_current_timestamp};
use ninjapaws_engine::query::plan::DEFAULT_RETENTION;
use serde_json::{Value, json};
use tako::StatusCode;
use tako::router::Router;

use crate::app;
use crate::probes::Postgres;
use crate::reads::ClickHouse;

const SECRET: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

/// Nothing listens there: /health is the only route that connects, and it is tested only for
/// being open.
fn no_postgres() -> Postgres {
    Postgres { url: "postgres://test:test@127.0.0.1:1/test".into() }
}

fn served(clickhouse: ClickHouse) -> Router {
    app(SECRET, clickhouse, DEFAULT_RETENTION, no_postgres()).unwrap()
}

/// A ClickHouse nobody listens on: a request that asks it anything answers with the network's
/// error, not with the one the test expects.
fn no_clickhouse() -> Router {
    served(ClickHouse::new("127.0.0.1:1", "ninjacat", "ninjacat", "ninjacat"))
}

/// A token as the panel mints it (frontend/src/lib/server/ninjacat.ts).
fn panel_claims(tenant: &str) -> Value {
    let now = get_current_timestamp();
    json!({ "sub": "user-1", "tenant": tenant, "role": "member", "iss": "ninjacat-panel", "aud": "ninjacat-query",
            "iat": now, "nbf": now, "exp": now + 60 })
}

fn mint(claims: &Value) -> String {
    sign(claims, SECRET, Algorithm::HS256)
}

fn sign(claims: &Value, key: &str, algorithm: Algorithm) -> String {
    encode(&Header::new(algorithm), claims, &EncodingKey::from_secret(key.as_bytes())).unwrap()
}

async fn send(router: &Router, bearer: Option<&str>, method: &str, path: &str, body: &str) -> (StatusCode, String) {
    let mut request = http::Request::builder().method(method).uri(path);
    if let Some(token) = bearer {
        request = request.header("authorization", format!("Bearer {token}"));
    }
    if !body.is_empty() {
        request = request.header("content-type", "application/json");
    }
    let response = router.dispatch(request.body(body.to_string().into()).unwrap()).await;
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(bytes.to_vec()).unwrap())
}

async fn get(router: &Router, path: &str) -> (StatusCode, Value) {
    let (status, body) = send(router, Some(&mint(&panel_claims("acme"))), "GET", path, "").await;
    (status, serde_json::from_str(&body).unwrap_or(Value::Null))
}

// --- the scratch database ----------------------------------------------------------------------

const DEV_CLICKHOUSE: &str = "localhost:8123";

/// What the panel reads, made like the dev stack's `ninjacat` tables, and the views that fill the
/// rollup and the tag keys from `metrics`.
const TABLES: [&str; 7] =
    ["metrics", "metrics_1m", "metric_tag_keys", "logs", "check_runs", "monitor_transitions", "monitor_evaluations"];
const VIEWS: [&str; 2] = ["metrics_1m_mv", "metric_tag_keys_mv"];

fn admin() -> clickhouse::Client {
    clickhouse::Client::default().with_url(format!("http://{DEV_CLICKHOUSE}")).with_user("ninjacat").with_password("ninjacat")
}

/// A database of its own on the dev ClickHouse, dropped afterwards. None when there is no
/// ClickHouse with the dev stack's schema.
struct Scratch {
    admin: clickhouse::Client,
    database: String,
}

impl Scratch {
    async fn new() -> Option<Scratch> {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);

        let admin = admin();
        let database = format!("ninjacat_query_{}_{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed));
        if let Err(e) = admin.query(&format!("CREATE DATABASE {database}")).execute().await {
            eprintln!("skipped: ClickHouse is not reachable at {DEV_CLICKHOUSE}: {e}");
            return None;
        }
        let scratch = Scratch { admin, database };
        for table in TABLES {
            if let Err(e) = scratch.run(&format!("CREATE TABLE {{db}}.{table} AS ninjacat.{table}")).await {
                eprintln!("skipped: the dev ClickHouse has no ninjacat.{table}: {e}");
                return None;
            }
        }
        for view in VIEWS {
            let made = async {
                let create: String = scratch.admin.query(&format!("SHOW CREATE TABLE ninjacat.{view}")).fetch_one().await?;
                scratch.run(&create.replace("ninjacat.", "{db}.")).await
            };
            if let Err(e) = made.await {
                eprintln!("skipped: the dev ClickHouse has no ninjacat.{view}: {e}");
                return None;
            }
        }
        Some(scratch)
    }

    async fn run(&self, sql: &str) -> Result<(), clickhouse::error::Error> {
        self.admin.query(&sql.replace("{db}", &self.database)).execute().await
    }

    fn router(&self) -> Router {
        served(ClickHouse::new(DEV_CLICKHOUSE, &self.database, "ninjacat", "ninjacat"))
    }
}

/// A client of its own on a runtime of its own: the test's runtime is blocked while this waits.
impl Drop for Scratch {
    fn drop(&mut self) {
        let sql = format!("DROP DATABASE IF EXISTS {}", self.database);
        std::thread::spawn(move || {
            let admin = admin();
            tokio::runtime::Runtime::new().unwrap().block_on(async { admin.query(&sql).execute().await })
        })
        .join()
        .ok();
    }
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn ago(now: i64, minutes: f64) -> i64 {
    now - (minutes * 60_000.0) as i64
}

fn time(ms: Option<i64>) -> String {
    ms.map_or("NULL".into(), |ms| format!("fromUnixTimestamp64Milli({ms})"))
}

// --- the door -----------------------------------------------------------------

/// One request to each kind of guarded route that queries: the panel's, the monitors', and
/// Datadog's query API, over the hour before `now`. Each GET names another tenant in its query
/// string, which must not be believed.
fn guarded(now: i64) -> Vec<(&'static str, String, String)> {
    let from = ninjapaws_engine::json::utc(ago(now, 60.0));
    let to = ninjapaws_engine::json::utc(now);
    let range = format!("from={from}&to={to}");
    let get = |path: &str| ("GET", format!("{path}{range}&tenant=default"), String::new());
    let (from, to) = (ago(now, 60.0), now);
    vec![
        get("/internal/metrics/names?"),
        get("/internal/metrics/tags?metric=system.cpu.user&"),
        get("/internal/metrics/tag-values?metric=system.cpu.user&key=env&"),
        get("/internal/metrics/query?metric=system.cpu.user&"),
        get("/internal/logs/search?"),
        get("/internal/logs/facets?"),
        get("/internal/checks/names?"),
        get("/internal/monitors/status?ids=7,8&"),
        get("/internal/monitors/7/groups?"),
        get("/internal/monitors/7/transitions?group=host:a&"),
        get("/internal/monitors/7/evaluations?"),
        get("/internal/monitors/check-runs?query=%22disk%22.by(%22host%22).last(1).count_by_status()&"),
        ("GET", format!("/api/v1/metrics?from={}&tenant=default", from / 1000), String::new()),
        (
            "POST",
            "/api/v2/query/timeseries".into(),
            format!(
                r#"{{"data":{{"type":"timeseries_request","attributes":{{"from":{from},"to":{to},"queries":[{{"data_source":"metrics","name":"a","query":"avg:system.cpu.user{{*}} by {{host}}"}}]}}}}}}"#
            ),
        ),
        (
            "POST",
            "/api/v2/query/scalar".into(),
            format!(
                r#"{{"data":{{"type":"scalar_request","attributes":{{"from":{from},"to":{to},"queries":[{{"data_source":"metrics","name":"a","query":"avg:system.cpu.user{{*}} by {{host}}","aggregator":"avg"}}]}}}}}}"#
            ),
        ),
    ]
}

/// Validation reads nothing, but answers only the panel.
const VALIDATE: (&str, &str, &str) =
    ("POST", "/internal/monitors/validate", r#"{"type":"query alert","query":"avg(last_5m):avg:system.cpu.user{*} > 90"}"#);

#[tokio::test]
async fn a_token_that_is_not_the_panels_or_names_not_exactly_one_tenant_is_401() {
    let claims = panel_claims("acme");
    let with = |change: &dyn Fn(&mut Value)| {
        let mut c = claims.clone();
        change(&mut c);
        c
    };
    let header = json!({"alg": "none", "typ": "JWT"}).to_string();
    let unsigned = format!("{}.{}.", base64_url(header.as_bytes()), base64_url(claims.to_string().as_bytes()));
    let refused: Vec<(&str, Option<String>)> = vec![
        ("no token", None),
        ("unsigned", Some(unsigned)),
        ("another key", Some(sign(&claims, "another-key-another-key-another-key", Algorithm::HS256))),
        ("another audience", Some(mint(&with(&|c| c["aud"] = json!("ninjacat-intake"))))),
        ("another issuer", Some(mint(&with(&|c| c["iss"] = json!("someone-else"))))),
        ("expired", Some(mint(&with(&|c| c["exp"] = json!(get_current_timestamp() - 60))))),
        ("HS512", Some(sign(&claims, SECRET, Algorithm::HS512))),
        ("no tenant", Some(mint(&with(&|c| _ = c.as_object_mut().unwrap().remove("tenant"))))),
        ("empty tenant", Some(mint(&with(&|c| c["tenant"] = json!(""))))),
        ("two tenants", Some(mint(&with(&|c| c["tenant"] = json!(["acme", "default"]))))),
    ];

    // The refusal comes before any handler, so before ClickHouse.
    let router = no_clickhouse();
    let routes = guarded(now_ms());
    for (label, bearer) in &refused {
        let (method, path, body) = VALIDATE;
        assert_eq!(send(&router, bearer.as_deref(), method, path, body).await.0, StatusCode::UNAUTHORIZED, "{label}, {path}");
        for (method, path, body) in &routes {
            assert_eq!(send(&router, bearer.as_deref(), method, path, body).await.0, StatusCode::UNAUTHORIZED, "{label}, {path}");
        }
    }
}

fn base64_url(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().fold(0u32, |n, b| n << 8 | u32::from(*b)) << (8 * (3 - chunk.len()));
        for i in 0..=chunk.len() {
            out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}

/// Something of tenant `default` in every table a guarded route reads.
async fn stored_for_default(scratch: &Scratch, now: i64) {
    let at = time(Some(ago(now, 10.0)));
    let before = time(Some(ago(now, 20.0)));
    for sql in [
        format!(
            "INSERT INTO {{db}}.metrics (tenant_id, timestamp, metric, host, metric_type, interval, value, tags) VALUES \
             ('default', {before}, 'system.cpu.user', 'host-a', 'GAUGE', 10, 3, {{'env': ['prod']}}), \
             ('default', {at}, 'system.cpu.user', 'host-a', 'GAUGE', 10, 5, {{'env': ['prod']}})"
        ),
        format!(
            "INSERT INTO {{db}}.logs (tenant_id, timestamp, host, service, source, status, message, tags) VALUES \
             ('default', {at}, 'host-a', 'web', 'nginx', 'error', 'disk full', {{'env': ['prod']}})"
        ),
        format!(
            "INSERT INTO {{db}}.check_runs (tenant_id, timestamp, check_name, host, status, message, tags) VALUES \
             ('default', {at}, 'disk', 'host-a', 'CRITICAL', 'disk full', {{}})"
        ),
    ] {
        scratch.run(&sql).await.unwrap();
    }
    let rows: Vec<String> = [
        transition(now, "default", 7, "host:a", 30.0, "none", "Alert", "new"),
        transition(now, "default", 8, "*", 15.0, "none", "Alert", "new"),
    ]
    .iter()
    .map(values)
    .collect();
    scratch
        .run(&format!("INSERT INTO {{db}}.monitor_transitions ({TRANSITION_COLUMNS}) VALUES {}", rows.join(", ")))
        .await
        .unwrap();
    scratch
        .run(&format!(
            "INSERT INTO {{db}}.monitor_evaluations (tenant_id, monitor_id, group_key, at, value, state, outcome, detail) \
             VALUES ('default', 7, 'host:a', {at}, 42, 'Alert', 'evaluated', '')"
        ))
        .await
        .unwrap();
}

#[tokio::test]
async fn every_guarded_route_queries_for_the_tenant_the_token_names_not_one_in_the_query_string() {
    let Some(scratch) = Scratch::new().await else { return };
    let now = now_ms();
    stored_for_default(&scratch, now).await;
    let router = scratch.router();
    let as_tenant = |tenant: &str| mint(&panel_claims(tenant));

    for (method, path, body) in guarded(now) {
        let without = path.replace("&tenant=default", "");
        let theirs = send(&router, Some(&as_tenant("default")), method, &without, &body).await;
        let ours = send(&router, Some(&as_tenant("acme")), method, &without, &body).await;
        let asked_for_theirs = send(&router, Some(&as_tenant("acme")), method, &path, &body).await;
        assert_eq!(theirs.0, StatusCode::OK, "{path}: {}", theirs.1);
        // What `default` stored is there to be found, by `default` alone.
        assert_ne!(theirs, ours, "{path}: the stored rows are not what the test reads");
        assert_eq!(asked_for_theirs, ours, "{path}: the query string's tenant was believed");
    }
}

#[tokio::test]
async fn the_probes_need_no_token() {
    let router = no_clickhouse();
    assert_eq!(send(&router, None, "GET", "/ping", "").await, (StatusCode::OK, "pong".to_string()));
    // 503 here, nothing being reachable; what matters is that the handler answered.
    let (status, body) = send(&router, None, "GET", "/health", "").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, r#"{"clickhouse":"unreachable","postgres":"unreachable"}"#);
}

#[tokio::test]
async fn the_query_process_does_not_start_with_a_short_secret() {
    let clickhouse = || ClickHouse::new("127.0.0.1:1", "ninjacat", "ninjacat", "ninjacat");
    let error = app("too-short", clickhouse(), DEFAULT_RETENTION, no_postgres()).err().unwrap();
    assert!(error.contains("NINJACAT_QUERY_TOKEN_SECRET"));
    assert!(app(&"x".repeat(32), clickhouse(), DEFAULT_RETENTION, no_postgres()).is_ok());
}

// --- ranges past what a date can hold --------------------------------------

/// A range past what a date can hold finds nothing, as in Datadog: 200 and the empty answer,
/// never an error.
async fn answers(requests: &[(&str, String, String)], expected: &str) {
    let Some(scratch) = Scratch::new().await else { return };
    let router = scratch.router();
    for (method, path, body) in requests {
        let (status, answer) = send(&router, Some(&mint(&panel_claims("acme"))), method, path, body).await;
        assert_eq!((status, answer.as_str()), (StatusCode::OK, expected), "{path} {body}");
    }
}

fn timeseries_body(from: i64, to: i64, interval: &str, query: &str, formula: &str) -> String {
    format!(
        r#"{{"data":{{"type":"timeseries_request","attributes":{{"from":{from},"to":{to}{interval},"queries":[{{"data_source":"metrics","name":"a","query":"{query}"}}],"formulas":[{{"formula":"{formula}"}}]}}}}}}"#
    )
}

const EMPTY_TIMESERIES: &str = r#"{"data":{"id":"0","type":"timeseries_response","attributes":{"series":[],"times":[],"values":[]}}}"#;

#[tokio::test]
async fn v1_metrics() {
    for from in ["253402300800", "-62135596801"] {
        let request = ("GET", format!("/api/v1/metrics?from={from}"), String::new());
        answers(&[request], &format!(r#"{{"from":"{from}","metrics":[]}}"#)).await;
    }
}

#[tokio::test]
async fn v2_timeseries() {
    let requests: Vec<_> = [(253402300800000, 253402300800001), (-62135596800001, -62135596800000)]
        .iter()
        .map(|&(from, to)| ("POST", "/api/v2/query/timeseries".into(), timeseries_body(from, to, "", "avg:x{*}", "timeshift(a, -86400)")))
        .collect();
    answers(&requests, EMPTY_TIMESERIES).await;
}

#[tokio::test]
async fn v2_timeseries_shifted_or_stepped_past_what_a_date_can_hold() {
    let requests: Vec<_> = [
        ("avg:x{*}", "timeshift(a, -100000000000)", ""),
        ("avg:x{*}.rollup(avg, 100000000000)", "diff(a)", ""),
        ("avg:x{*}", "diff(a)", r#","interval":9000000000000000000"#),
    ]
    .iter()
    .map(|(query, formula, interval)| {
        ("POST", "/api/v2/query/timeseries".into(), timeseries_body(1760000000000, 1760003600000, interval, query, formula))
    })
    .collect();
    answers(&requests, EMPTY_TIMESERIES).await;
}

#[tokio::test]
async fn v2_scalar() {
    let requests: Vec<_> = [(253402300800000_i64, 253402300800001_i64), (-62135596800001, -62135596800000)]
        .iter()
        .map(|(from, to)| {
            let body = format!(
                r#"{{"data":{{"type":"scalar_request","attributes":{{"from":{from},"to":{to},"queries":[{{"data_source":"metrics","name":"a","aggregator":"avg","query":"avg:x{{*}}"}}]}}}}}}"#
            );
            ("POST", "/api/v2/query/scalar".into(), body)
        })
        .collect();
    let expected = r#"{"data":{"type":"scalar_response","attributes":{"columns":[{"name":"a","type":"number","values":[],"meta":{}}]}}}"#;
    answers(&requests, expected).await;
}

#[tokio::test]
async fn panel_metric_query() {
    // Before 1970, and before year 1.
    let requests: Vec<_> = [("-40000d", "-30000d"), ("-6000000d", "-5000000d")]
        .iter()
        .map(|(from, to)| ("GET", format!("/internal/metrics/query?metric=x&from={from}&to={to}"), String::new()))
        .collect();
    let expected = r#"{"from":"1970-01-01T00:00:00Z","metric":"x","series":[],"step":60,"to":"1970-01-01T00:00:00Z"}"#;
    answers(&requests, expected).await;
}

// --- monitors ----------------------------------------------------------------

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

async fn validate(router: &Router, body: &str) -> Value {
    let (status, answer) = send(router, Some(&mint(&panel_claims("acme"))), "POST", "/internal/monitors/validate", body).await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    serde_json::from_str(&answer).unwrap()
}

// Which problems a definition has is tested in the engine's monitor::validate.
#[tokio::test]
async fn validate_answers_the_problems_or_none_and_the_options_not_acted_on() {
    let router = no_clickhouse();
    let valid = validate(
        &router,
        r#"{"name":"cpu","type":"query alert","query":"avg(last_5m):avg:system.cpu.user{*} by {host} > 90",
            "options":{"thresholds":{"critical":90,"warning":80},"renotify_interval":60}}"#,
    )
    .await;
    assert!(strings(&valid["problems"]).is_empty(), "{valid}");
    assert!(strings(&valid["not_acted_on"]).contains(&"renotify_interval".to_string()));

    let invalid = validate(&router, "not json").await;
    assert!(!strings(&invalid["problems"]).is_empty());
    assert!(strings(&invalid["not_acted_on"]).is_empty());
}

#[tokio::test]
async fn a_service_check_monitor_validates() {
    let valid = validate(
        &no_clickhouse(),
        r#"{"type":"service check","query":"\"consul.check\".over(\"*\").by(\"host\").last(3).count_by_status()",
            "options":{"thresholds":{"critical":2,"warning":2,"ok":1}}}"#,
    )
    .await;
    assert!(strings(&valid["problems"]).is_empty(), "{valid}");
}

#[tokio::test]
async fn check_runs_of_a_query_that_is_no_check_query_is_400_and_asks_nothing() {
    let (status, answer) = get(&no_clickhouse(), "/internal/monitors/check-runs?query=avg:cpu%7B*%7D").await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(answer["error"].as_str().unwrap().contains("Expecting"), "{answer}");
}

#[tokio::test]
async fn status_without_good_ids_is_400_and_asks_nothing() {
    let router = no_clickhouse();
    for (query, error) in [("", "ids is required"), ("?ids=0", "bad monitor id \"0\"")] {
        let (status, answer) = get(&router, &format!("/internal/monitors/status{query}")).await;
        assert_eq!((status, answer["error"].as_str()), (StatusCode::BAD_REQUEST, Some(error)));
    }
}

#[tokio::test]
async fn a_bad_range_is_400() {
    let router = no_clickhouse();
    for (path, error) in [
        ("/internal/monitors/7/transitions?from=yesterday", "bad from: cannot parse"),
        ("/internal/monitors/7/evaluations?from=now&to=-1h", "to must be after from"),
    ] {
        let (status, answer) = get(&router, path).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
        assert!(answer["error"].as_str().unwrap().starts_with(error), "{path}: {answer}");
    }
}

// --- what the monitor service stored -------------------------------------------------------------

struct Transition {
    tenant: &'static str,
    monitor: u64,
    group: &'static str,
    at: i64,
    from: &'static str,
    into: &'static str,
    reason: &'static str,
    actor: Option<&'static str>,
    window_from: i64,
    window_to: i64,
    first_triggered_at: Option<i64>,
    last_triggered_at: Option<i64>,
}

#[allow(clippy::too_many_arguments)]
fn transition(
    now: i64,
    tenant: &'static str,
    monitor: u64,
    group: &'static str,
    minutes: f64,
    from: &'static str,
    into: &'static str,
    reason: &'static str,
) -> Transition {
    let at = ago(now, minutes);
    let triggered = if into == "Alert" || into == "Warn" { Some(at) } else { None };
    Transition {
        tenant,
        monitor,
        group,
        at,
        from,
        into,
        reason,
        actor: None,
        window_from: at - 5 * 60_000,
        window_to: at,
        first_triggered_at: triggered,
        last_triggered_at: triggered,
    }
}

fn values(t: &Transition) -> String {
    format!(
        "('{}', {}, 1, 'hash', 'metric', '{}', ['{}'], {}, '{}', '{}', '{}', {}, 1, 'none', '>', 90, NULL, NULL, NULL, {}, {}, {}, {})",
        t.tenant,
        t.monitor,
        t.group,
        t.group,
        time(Some(t.at)),
        t.from,
        t.into,
        t.reason,
        t.actor.map_or("NULL".into(), |a| format!("'{a}'")),
        time(Some(t.window_from)),
        time(Some(t.window_to)),
        time(t.first_triggered_at),
        time(t.last_triggered_at),
    )
}

const TRANSITION_COLUMNS: &str = "tenant_id, monitor_id, monitor_version, query_hash, monitor_kind, group_key, group_tags, at, \
    from_state, to_state, reason, actor, value, crossed, comparator, critical, warning, critical_recovery, warning_recovery, \
    window_from, window_to, first_triggered_at, last_triggered_at";

/// Monitor 7 of acme:
/// - host:a went to Alert, then Warn (stored twice, as a retried insert would);
/// - host:b triggered and recovered;
/// - host:d triggered, recovered, then went to No Data, whose row carries no trigger time;
/// - host:c triggered after all of them, then expired.
///
/// Monitor 8 is in Alert. Another tenant's monitor 7 is in Alert later than any of them.
async fn stored(scratch: &Scratch, now: i64) {
    let triggered = |minutes: f64, t: Transition| Transition {
        first_triggered_at: Some(ago(now, minutes)),
        last_triggered_at: Some(ago(now, minutes)),
        ..t
    };
    // The trigger began with the Alert.
    let warned = || Transition {
        first_triggered_at: Some(ago(now, 30.0)),
        ..transition(now, "acme", 7, "host:a", 20.0, "Alert", "Warn", "threshold")
    };
    let rows = [
        transition(now, "acme", 7, "host:a", 30.0, "none", "Alert", "new"),
        warned(),
        transition(now, "acme", 7, "host:b", 28.0, "none", "Alert", "new"),
        triggered(28.0, Transition { actor: Some("user-1"), ..transition(now, "acme", 7, "host:b", 25.0, "Alert", "OK", "recovery") }),
        transition(now, "acme", 7, "host:d", 35.0, "none", "Alert", "new"),
        triggered(35.0, transition(now, "acme", 7, "host:d", 32.0, "Alert", "OK", "recovery")),
        Transition { last_triggered_at: Some(ago(now, 35.0)), ..transition(now, "acme", 7, "host:d", 22.0, "OK", "No Data", "no_data") },
        transition(now, "acme", 7, "host:c", 12.0, "none", "Alert", "new"),
        Transition { window_from: 0, window_to: 0, ..transition(now, "acme", 7, "host:c", 10.0, "Alert", "OK", "expired") },
        transition(now, "acme", 8, "*", 15.0, "none", "Alert", "new"),
        transition(now, "globex", 7, "host:z", 5.0, "none", "Alert", "new"),
    ];
    let rows: Vec<String> = rows.iter().map(values).collect();
    let insert = |rows: &[String]| format!("INSERT INTO {{db}}.monitor_transitions ({TRANSITION_COLUMNS}) VALUES {}", rows.join(", "));
    scratch.run(&insert(&rows)).await.unwrap();
    scratch.run(&insert(&[values(&warned())])).await.unwrap();

    let evaluation = |tenant: &str, group: &str, minutes: f64, value: &str, outcome: &str| {
        let detail = if outcome == "evaluated" { "" } else { "window not full" };
        format!("('{tenant}', 7, '{group}', {}, {value}, 'Warn', '{outcome}', '{detail}')", time(Some(ago(now, minutes))))
    };
    let evaluations = [
        evaluation("acme", "host:a", 2.0, "42", "evaluated"),
        evaluation("acme", "host:a", 1.0, "43", "evaluated"),
        evaluation("acme", "host:a", 0.5, "NULL", "skipped"),
        evaluation("acme", "", 0.25, "NULL", "error"),
        // A sum can overflow to infinity.
        evaluation("acme", "host:d", 3.0, "inf", "evaluated"),
        // Older than a group can stay open unseen.
        evaluation("acme", "host:b", 2.0 * 24.0 * 60.0, "5", "evaluated"),
        evaluation("globex", "host:a", 0.1, "99", "evaluated"),
    ];
    let sql = format!(
        "INSERT INTO {{db}}.monitor_evaluations (tenant_id, monitor_id, group_key, at, value, state, outcome, detail) VALUES {}",
        evaluations.join(", ")
    );
    scratch.run(&sql).await.unwrap();
}

fn ms(v: &Value) -> i64 {
    chrono::DateTime::parse_from_rfc3339(v.as_str().unwrap()).unwrap().timestamp_millis()
}

fn has(v: &Value, name: &str) -> bool {
    v.get(name).is_some()
}

#[tokio::test]
async fn status_counts_the_open_groups_of_each_monitor_asked_for_for_the_tokens_tenant() {
    let Some(scratch) = Scratch::new().await else { return };
    let now = now_ms();
    stored(&scratch, now).await;
    let (status, answer) = get(&scratch.router(), "/internal/monitors/status?ids=7,8&ids=9").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let monitors = answer["monitors"].as_array().unwrap();
    assert_eq!(monitors.iter().map(|m| m["id"].as_i64().unwrap()).collect::<Vec<_>>(), [7, 8, 9]);

    let seven = &monitors[0];
    assert_eq!(seven["state"], "Warn");
    let count = |state: &str| seven["groups"][state].as_i64().unwrap();
    assert_eq!([count("Alert"), count("Warn"), count("OK"), count("No Data")], [0, 1, 1, 1]);
    // host:c's Alert, closed since, not the other tenant's later one.
    assert_eq!(ms(&seven["last_triggered_at"]), ago(now, 12.0));

    assert_eq!(monitors[1]["state"], "Alert");
    assert!(!has(&monitors[2], "state"));
    assert!(!has(&monitors[2], "last_triggered_at"));
}

#[tokio::test]
async fn groups_are_the_open_ones_with_their_latest_evaluated_value() {
    let Some(scratch) = Scratch::new().await else { return };
    let now = now_ms();
    stored(&scratch, now).await;
    let (status, answer) = get(&scratch.router(), "/internal/monitors/7/groups").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let groups = answer["groups"].as_array().unwrap();
    assert_eq!(groups.iter().map(|g| g["group"].as_str().unwrap()).collect::<Vec<_>>(), ["host:a", "host:b", "host:d"]);

    let a = &groups[0];
    assert_eq!(a["state"], "Warn");
    assert_eq!(ms(&a["since"]), ago(now, 20.0));
    assert_eq!(a["reason"], "threshold");
    assert_eq!(ms(&a["first_triggered_at"]), ago(now, 30.0));
    assert_eq!(a["value"], 43.0);
    assert_eq!(ms(&a["evaluated_at"]), ago(now, 1.0));
    assert_eq!(strings(&a["tags"]), ["host:a"]);

    // Recovered: the trigger it ended is not shown, when it was is.
    let b = &groups[1];
    assert_eq!(b["state"], "OK");
    assert!(!has(b, "first_triggered_at"));
    assert_eq!(ms(&b["last_triggered_at"]), ago(now, 28.0));
    assert!(!has(b, "value"));

    // The latest row's NULL trigger time, not an older row's.
    let d = &groups[2];
    assert_eq!(d["state"], "No Data");
    assert!(!has(d, "first_triggered_at"));
    assert_eq!(ms(&d["last_triggered_at"]), ago(now, 35.0));
    assert!(!has(d, "value"));
    assert_eq!(ms(&d["evaluated_at"]), ago(now, 3.0));
}

fn groups_and_states(rows: &Value) -> Vec<(String, String)> {
    rows.as_array()
        .unwrap()
        .iter()
        .map(|t| (t["group"].as_str().unwrap().to_string(), t["to_state"].as_str().unwrap().to_string()))
        .collect()
}

fn pairs(expected: &[(&str, &str)]) -> Vec<(String, String)> {
    expected.iter().map(|(g, s)| (g.to_string(), s.to_string())).collect()
}

#[tokio::test]
async fn transitions_are_the_timeline_of_a_range_a_repeated_row_once() {
    let Some(scratch) = Scratch::new().await else { return };
    let now = now_ms();
    stored(&scratch, now).await;
    let router = scratch.router();
    let (status, answer) = get(&router, "/internal/monitors/7/transitions?from=-1h").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    assert_eq!(answer["truncated"], false);
    assert!(groups_and_states(&answer["initial"]).is_empty());
    assert_eq!(
        groups_and_states(&answer["transitions"]),
        pairs(&[
            ("host:d", "Alert"),
            ("host:d", "OK"),
            ("host:a", "Alert"),
            ("host:b", "Alert"),
            ("host:b", "OK"),
            ("host:d", "No Data"),
            ("host:a", "Warn"),
            ("host:c", "Alert"),
            ("host:c", "OK"),
        ])
    );

    let rows = answer["transitions"].as_array().unwrap();
    let first = &rows[0];
    assert!(!has(first, "from_state"));
    assert_eq!(first["critical"], 90.0);
    assert_eq!(ms(&first["window_from"]), ago(now, 40.0));
    assert_eq!(rows[4]["actor"], "user-1");
    assert!(!has(first, "actor"));
    assert_eq!(rows[5]["from_state"], "OK");
    // Expiry judges no window.
    assert!(!has(&rows[8], "window_from"));

    let (_, one) = get(&router, "/internal/monitors/7/transitions?from=-1h&group=host:a").await;
    assert_eq!(one["transitions"].as_array().unwrap().len(), 2);

    let (_, later) = get(&router, "/internal/monitors/7/transitions?from=-15m").await;
    assert_eq!(later["transitions"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn a_range_starts_from_each_open_groups_latest_transition_before_it() {
    let Some(scratch) = Scratch::new().await else { return };
    let now = now_ms();
    stored(&scratch, now).await;
    let router = scratch.router();
    let (_, answer) = get(&router, "/internal/monitors/7/transitions?from=-21m").await;
    assert_eq!(groups_and_states(&answer["initial"]), pairs(&[("host:a", "Alert"), ("host:b", "OK"), ("host:d", "No Data")]));

    // host:c expired before it: closed, so not there.
    let (_, later) = get(&router, "/internal/monitors/7/transitions?from=-5m").await;
    assert!(later["transitions"].as_array().unwrap().is_empty());
    assert_eq!(groups_and_states(&later["initial"]), pairs(&[("host:a", "Warn"), ("host:b", "OK"), ("host:d", "No Data")]));

    let (_, one) = get(&router, "/internal/monitors/7/transitions?from=-21m&group=host:a").await;
    assert_eq!(groups_and_states(&one["initial"]), pairs(&[("host:a", "Alert")]));
}

#[tokio::test]
async fn evaluations_of_a_range_one_group_or_all_the_monitors_own_rows_among_them() {
    let Some(scratch) = Scratch::new().await else { return };
    let now = now_ms();
    stored(&scratch, now).await;
    let router = scratch.router();
    let (status, answer) = get(&router, "/internal/monitors/7/evaluations?from=-1h").await;
    assert_eq!(status, StatusCode::OK, "{answer}");
    let rows = answer["evaluations"].as_array().unwrap();
    let seen: Vec<(String, String)> = rows
        .iter()
        .map(|e| (e["group"].as_str().unwrap().to_string(), e["outcome"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(
        seen,
        pairs(&[("host:d", "evaluated"), ("host:a", "evaluated"), ("host:a", "evaluated"), ("host:a", "skipped"), ("", "error")])
    );
    assert!(!has(&rows[0], "value"));
    assert_eq!(rows[1]["value"], 42.0);
    assert_eq!(rows[3]["detail"], "window not full");

    let (_, one) = get(&router, "/internal/monitors/7/evaluations?from=-1h&group=host:a").await;
    let groups: Vec<&str> = one["evaluations"].as_array().unwrap().iter().map(|e| e["group"].as_str().unwrap()).collect();
    assert_eq!(groups, ["host:a", "host:a", "host:a", ""]);
}
