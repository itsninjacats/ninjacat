//! The panel's internal API (`/internal/*`): what the SvelteKit server asks for on behalf of a
//! signed-in user, for the tenant its token names. A query from `ninjapaws_engine::panel` in,
//! JSON out.

use std::collections::BTreeMap;

use api::reads::ClickHouse;
use chrono::TimeDelta;
use ninjapaws_engine::json;
use ninjapaws_engine::panel::{self, LogFacet, LogSearch, SeriesQuery};
use ninjapaws_engine::query::engine;
use ninjapaws_engine::query::execute::QueryError;
use ninjapaws_engine::query::plan::{self, Retention};
use ninjapaws_engine::{Sql, TenantId, duration, unix_time};
use serde::{Deserialize, Serialize};
use tako::StatusCode;
use tako::extractors::json::Json;
use tako::extractors::jwt::JwtClaimsVerified;
use tako::extractors::query_multi::QueryMulti;
use tako::extractors::state::State;
use tako::responder::Responder;
use tako::types::Response;

use crate::token::Claims;

#[derive(Serialize)]
struct Failure {
    error: String,
}

pub fn fail(status: StatusCode, error: impl Into<String>) -> Response {
    (status, Json(Failure { error: error.into() })).into_response()
}

pub fn tenant(claims: &Claims) -> TenantId {
    TenantId(claims.tenant.clone())
}

pub fn now() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// `from` and `to` of a request: `back` before now, to now, unless given.
pub fn time_range_or(back: TimeDelta, from: &str, to: &str) -> Result<(i64, i64), String> {
    let now = now();
    match (panel::parse_time(from, now - back.num_milliseconds(), now), panel::parse_time(to, now, now)) {
        (Err(e), _) => Err(format!("bad from: {e}")),
        (_, Err(e)) => Err(format!("bad to: {e}")),
        (Ok(from), Ok(until)) if until <= from => Err("to must be after from".into()),
        (Ok(from), Ok(until)) => Ok((unix_time::of_millis(from), unix_time::of_millis(until))),
    }
}

/// `from` and `to` of a request: an hour back to now unless given.
pub fn time_range(from: &str, to: &str) -> Result<(i64, i64), String> {
    time_range_or(TimeDelta::hours(1), from, to)
}

/// The range of the metric listings unless given: a day, the same for names, keys and values, so
/// a key offered has values to offer.
fn listing_range(from: &str, to: &str) -> Result<(i64, i64), String> {
    time_range_or(TimeDelta::days(1), from, to)
}

/// A number the caller may leave out or get wrong: 0, which the engine reads as its default.
fn int(raw: &str) -> i32 {
    raw.trim().parse().unwrap_or(0)
}

/// Runs a query; a failure in ClickHouse is logged and comes back as its reason. The panel's
/// lookups answer it with a 400, as their queries are built from the caller's parameters.
pub async fn run<T: serde::de::DeserializeOwned>(clickhouse: &ClickHouse, sql: &Sql) -> Result<Vec<T>, String> {
    clickhouse.read(sql).await.inspect_err(|e| eprintln!("query: {e}"))
}

/// One-column lookups all answer the same way: `{"<name>": [...]}`.
async fn names(clickhouse: &ClickHouse, sql: Sql, name: &str) -> Response {
    match run::<(String,)>(clickhouse, &sql).await {
        Ok(found) => {
            let found: Vec<String> = found.into_iter().map(|(n,)| n).collect();
            Json(BTreeMap::from([(name, found)])).into_response()
        }
        Err(e) => fail(StatusCode::BAD_REQUEST, e),
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Listing {
    from: String,
    to: String,
    search: String,
    limit: String,
    metric: String,
    key: String,
}

pub async fn metric_names(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    QueryMulti(q): QueryMulti<Listing>,
) -> Response {
    match listing_range(&q.from, &q.to) {
        Err(e) => fail(StatusCode::BAD_REQUEST, e),
        Ok((from, until)) => {
            names(&clickhouse, panel::list_metrics(&tenant(&claims), from, until, &q.search, int(&q.limit)), "metrics")
                .await
        }
    }
}

pub async fn metric_tag_keys(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    QueryMulti(q): QueryMulti<Listing>,
) -> Response {
    match listing_range(&q.from, &q.to) {
        Err(e) => fail(StatusCode::BAD_REQUEST, e),
        Ok((from, until)) => {
            names(&clickhouse, panel::list_tag_keys(&tenant(&claims), from, until, &q.metric), "keys").await
        }
    }
}

pub async fn metric_tag_values(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    QueryMulti(q): QueryMulti<Listing>,
) -> Response {
    if q.key.is_empty() {
        return fail(StatusCode::BAD_REQUEST, "key is required");
    }
    match listing_range(&q.from, &q.to) {
        Err(e) => fail(StatusCode::BAD_REQUEST, e),
        Ok((from, until)) => {
            let sql =
                panel::list_tag_values(&tenant(&claims), from, until, &q.metric, &q.key, &q.search, int(&q.limit));
            names(&clickhouse, sql, "values").await
        }
    }
}

/// The check names reported in a range, a day unless given.
pub async fn check_names(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    QueryMulti(q): QueryMulti<Listing>,
) -> Response {
    match listing_range(&q.from, &q.to) {
        Err(e) => fail(StatusCode::BAD_REQUEST, e),
        Ok((from, until)) => names(&clickhouse, panel::list_checks(&tenant(&claims), from, until), "checks").await,
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct MetricQuery {
    metric: String,
    from: String,
    to: String,
    step: String,
    agg: String,
    tag: Vec<String>,
    host: Vec<String>,
    by: Vec<String>,
}

#[derive(Serialize)]
struct SeriesAnswer {
    #[serde(serialize_with = "json::as_utc")]
    from: i64,
    metric: String,
    series: Vec<SeriesLine>,
    step: i64,
    #[serde(serialize_with = "json::as_utc")]
    to: i64,
}

#[derive(Serialize)]
struct SeriesLine {
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    metric: String,
    name: String,
    points: Vec<SeriesPoint>,
    tags: BTreeMap<String, String>,
}

#[derive(Serialize)]
struct SeriesPoint {
    #[serde(serialize_with = "json::as_utc")]
    t: i64,
    v: f64,
}

/// One metric over a time range, one series per host or per combination of the `by` keys. The
/// query engine runs it, as it runs every other query.
pub async fn metric_query(
    State(clickhouse): State<ClickHouse>,
    State(retention): State<Retention>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    QueryMulti(q): QueryMulti<MetricQuery>,
) -> Response {
    let (from, until, tags) = match (q.metric.as_str(), time_range(&q.from, &q.to), panel::parse_tag_filters(&q.tag)) {
        ("", _, _) => return fail(StatusCode::BAD_REQUEST, "metric is required"),
        (_, Err(e), _) | (_, _, Err(e)) => return fail(StatusCode::BAD_REQUEST, e),
        (_, Ok((from, until)), Ok(tags)) => (from, until, tags),
    };
    let query = SeriesQuery {
        metric: q.metric.clone(),
        hosts: q.host,
        from,
        to: until,
        step: duration::parse(&q.step).ok().filter(|asked| *asked > TimeDelta::zero()),
        aggregation: q.agg,
        tags,
        group_by: q.by,
    };
    let now = now();

    // A failure in ClickHouse is the caller's 400 with its reason, as for the other lookups.
    let results = match engine::parsed(
        &*clickhouse,
        &tenant(&claims),
        *retention,
        now,
        &["q".into()],
        &panel::series_request(&query),
    )
    .await
    {
        Ok(results) => results,
        Err(QueryError::Invalid(problems)) => return fail(StatusCode::BAD_REQUEST, problems.join("; ")),
        Err(QueryError::Failed(e)) => {
            eprintln!("query: {e}");
            return fail(StatusCode::BAD_REQUEST, e);
        }
    };

    Json(SeriesAnswer {
        from,
        metric: q.metric.clone(),
        series: panel::series_of(&query, &results)
            .into_iter()
            .map(|s| SeriesLine {
                host: s.host,
                metric: q.metric.clone(),
                name: s.name,
                points: s.points.into_iter().map(|(t, v)| SeriesPoint { t, v }).collect(),
                tags: s.tags,
            })
            .collect(),
        step: plan::request_step(*retention, now, from, until, query.step).num_seconds(),
        to: until,
    })
    .into_response()
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct LogQuery {
    q: String,
    service: String,
    host: String,
    status: String,
    tag: Vec<String>,
    from: String,
    to: String,
    limit: String,
}

#[derive(Serialize)]
struct LogsAnswer {
    count: u64,
    logs: Vec<LogLine>,
}

#[derive(Serialize)]
struct LogLine {
    host: String,
    message: String,
    service: String,
    source: String,
    status: String,
    tags: BTreeMap<String, Vec<String>>,
    #[serde(serialize_with = "json::as_utc")]
    timestamp: i64,
}

pub async fn log_search(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    QueryMulti(q): QueryMulti<LogQuery>,
) -> Response {
    let (from, until, tags) = match (time_range(&q.from, &q.to), panel::parse_tag_filters(&q.tag)) {
        (Err(e), _) | (_, Err(e)) => return fail(StatusCode::BAD_REQUEST, e),
        (Ok((from, until)), Ok(tags)) => (from, until, tags),
    };
    let search = LogSearch {
        tenant: tenant(&claims),
        text: q.q,
        service: q.service,
        host: q.host,
        status: q.status,
        tags,
        from,
        to: until,
        limit: int(&q.limit),
    };
    let (rows_sql, count_sql) = panel::search_logs(&search);

    type LogRow = (i64, String, String, String, String, String, BTreeMap<String, Vec<String>>);
    let (logs, count) = tokio::join!(run::<LogRow>(&clickhouse, &rows_sql), run::<(u64,)>(&clickhouse, &count_sql));
    match (logs, count) {
        (Ok(logs), Ok(count)) if count.len() == 1 => Json(LogsAnswer {
            count: count[0].0,
            logs: logs
                .into_iter()
                .map(|(timestamp, host, service, source, status, message, tags)| LogLine {
                    host,
                    message,
                    service,
                    source,
                    status,
                    tags,
                    timestamp,
                })
                .collect(),
        })
        .into_response(),
        (Err(e), _) | (_, Err(e)) => fail(StatusCode::BAD_REQUEST, e),
        _ => fail(StatusCode::INTERNAL_SERVER_ERROR, "unexpected answer to the count query"),
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Range {
    from: String,
    to: String,
}

#[derive(Serialize)]
struct FacetCount {
    count: u64,
    value: String,
}

#[derive(Serialize)]
struct FacetsAnswer {
    hosts: Vec<FacetCount>,
    services: Vec<FacetCount>,
    statuses: Vec<FacetCount>,
}

async fn facet(clickhouse: &ClickHouse, sql: Sql) -> Result<Vec<FacetCount>, String> {
    let rows = run::<(String, u64)>(clickhouse, &sql).await?;
    Ok(rows.into_iter().map(|(value, count)| FacetCount { count, value }).collect())
}

pub async fn log_facets(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    QueryMulti(q): QueryMulti<Range>,
) -> Response {
    let (from, until) = match time_range(&q.from, &q.to) {
        Err(e) => return fail(StatusCode::BAD_REQUEST, e),
        Ok(range) => range,
    };
    let tenant = tenant(&claims);
    let facet = |which| facet(&clickhouse, panel::log_facet(&tenant, from, until, which));
    match tokio::join!(facet(LogFacet::Service), facet(LogFacet::Host), facet(LogFacet::Status)) {
        (Ok(services), Ok(hosts), Ok(statuses)) => Json(FacetsAnswer { hosts, services, statuses }).into_response(),
        (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => fail(StatusCode::BAD_REQUEST, e),
    }
}
