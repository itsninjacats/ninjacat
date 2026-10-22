//! `/internal/monitors/*`: validation, and what the monitor service stored, for the token's
//! tenant. The panel writes definitions to Postgres itself.

use api::reads::ClickHouse;
use ninjapaws_engine::Sql;
use ninjapaws_engine::json;
use ninjapaws_engine::monitor::read::{self, EvaluationRecord, GroupStatus, StatusRow, TransitionRecord};
use ninjapaws_engine::monitor::state::Status;
use ninjapaws_engine::monitor::{check, validate};
use ninjapaws_engine::query::plan::Retention;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tako::StatusCode;
use tako::extractors::json::Json;
use tako::extractors::jwt::JwtClaimsVerified;
use tako::extractors::path::Path;
use tako::extractors::query_multi::QueryMulti;
use tako::extractors::state::State;
use tako::responder::Responder;
use tako::types::Response;

use crate::panel::{fail, now, run, tenant, time_range};
use crate::token::Claims;

#[derive(Serialize)]
struct Validation {
    not_acted_on: Vec<String>,
    problems: Vec<String>,
}

/// `POST validate`: the problems of a monitor in Datadog's JSON, none when it would be
/// evaluated, and the options it carries that are not acted on.
pub async fn validate(State(retention): State<Retention>, _: JwtClaimsVerified<Claims>, body: String) -> Response {
    match validate::monitor(*retention, now(), &body) {
        Ok(monitor) => Json(Validation { not_acted_on: monitor.options().not_acted_on.clone(), problems: Vec::new() })
            .into_response(),
        Err(problems) => Json(Validation { not_acted_on: Vec::new(), problems }).into_response(),
    }
}

/// A query built from checked parameters: a failure is ours, not the caller's.
async fn answer<T: DeserializeOwned, R: Serialize>(
    clickhouse: &ClickHouse,
    sql: &Sql,
    respond: impl FnOnce(Vec<T>) -> R,
) -> Response {
    match run(clickhouse, sql).await {
        Ok(rows) => Json(respond(rows)).into_response(),
        Err(e) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// A monitor's id in the path; anything else is no route at all.
fn monitor_id(raw: &str) -> Option<i64> {
    raw.parse().ok()
}

/// A reduction such as sum can overflow to infinity, which JSON cannot carry; ClickHouse sends it
/// as null.
fn finite(value: Option<f64>) -> Option<f64> {
    value.filter(|v| v.is_finite())
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct Ids {
    ids: Vec<String>,
}

#[derive(Serialize)]
struct Statuses {
    monitors: Vec<read::MonitorStatus>,
}

/// `GET status?ids=1,2`: per monitor, its state, its open groups by state and when it last
/// triggered.
pub async fn status(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    QueryMulti(q): QueryMulti<Ids>,
) -> Response {
    let ids = match read::parse_ids(&q.ids) {
        Ok(ids) => ids,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e),
    };
    type Row = (i64, String, u8, i64, Option<i64>);
    answer(&clickhouse, &read::status(&tenant(&claims), &ids), |rows: Vec<Row>| {
        let rows: Vec<StatusRow> = rows
            .into_iter()
            .map(|(monitor, state, open, groups, last_triggered_at)| StatusRow {
                monitor,
                state,
                open: open != 0,
                groups,
                last_triggered_at,
            })
            .collect();
        Statuses { monitors: read::statuses(&ids, &rows) }
    })
    .await
}

#[derive(Serialize)]
struct Groups {
    groups: Vec<GroupStatus>,
}

/// `GET {id}/groups`: the open groups, with their state and latest value.
pub async fn groups(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    Path(id): Path<String>,
) -> Response {
    let Some(monitor) = monitor_id(&id) else { return StatusCode::NOT_FOUND.into_response() };
    type Row = (String, Vec<String>, String, i64, String, Option<i64>, Option<i64>, Option<f64>, Option<i64>);
    answer(&clickhouse, &read::groups(&tenant(&claims), monitor), |rows: Vec<Row>| Groups {
        groups: rows
            .into_iter()
            .map(|(group, tags, state, since, reason, first, last, value, evaluated_at)| GroupStatus {
                first_triggered_at: if state == Status::Ok.text() { None } else { first },
                group,
                tags,
                state,
                since,
                reason,
                last_triggered_at: last,
                value: finite(value),
                evaluated_at,
            })
            .collect(),
    })
    .await
}

/// A transition as `Read::transitions` selects it.
#[derive(Deserialize)]
struct TransitionRow {
    group: String,
    tags: Vec<String>,
    at: i64,
    from_state: String,
    to_state: String,
    reason: String,
    actor: Option<String>,
    value: Option<f64>,
    crossed: String,
    comparator: String,
    critical: Option<f64>,
    warning: Option<f64>,
    critical_recovery: Option<f64>,
    warning_recovery: Option<f64>,
    window_from: i64,
    window_to: i64,
    first_triggered_at: Option<i64>,
    last_triggered_at: Option<i64>,
    monitor_version: i64,
}

/// The epoch stands for "no window" in a transition outside a tick.
fn window(ms: i64) -> Option<i64> {
    Some(ms).filter(|&ms| ms != 0)
}

impl From<TransitionRow> for TransitionRecord {
    fn from(r: TransitionRow) -> Self {
        TransitionRecord {
            group: r.group,
            tags: r.tags,
            at: r.at,
            from_state: Status::of_text(&r.from_state).map(|s| s.text().to_string()),
            to_state: r.to_state,
            reason: r.reason,
            actor: r.actor,
            value: finite(r.value),
            crossed: Some(r.crossed).filter(|c| c != "none"),
            comparator: r.comparator,
            critical: finite(r.critical),
            warning: finite(r.warning),
            critical_recovery: finite(r.critical_recovery),
            warning_recovery: finite(r.warning_recovery),
            window_from: window(r.window_from),
            window_to: window(r.window_to),
            first_triggered_at: r.first_triggered_at,
            last_triggered_at: r.last_triggered_at,
            monitor_version: r.monitor_version,
        }
    }
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct RangeOfGroup {
    from: String,
    to: String,
    group: String,
}

#[derive(Serialize)]
struct Transitions {
    initial: Vec<TransitionRecord>,
    transitions: Vec<TransitionRecord>,
    truncated: bool,
}

/// `GET {id}/transitions?from&to[&group]`: the state timeline, with each open group's latest
/// transition before `from` as where it starts.
pub async fn transitions(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    Path(id): Path<String>,
    QueryMulti(q): QueryMulti<RangeOfGroup>,
) -> Response {
    let Some(monitor) = monitor_id(&id) else { return StatusCode::NOT_FOUND.into_response() };
    let (from, until) = match time_range(&q.from, &q.to) {
        Ok(range) => range,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e),
    };
    let tenant = tenant(&claims);
    let in_range = read::transitions(&tenant, monitor, from, until, &q.group);
    let before = read::initial(&tenant, monitor, from, &q.group);
    let (in_range, before) =
        tokio::join!(run::<TransitionRow>(&clickhouse, &in_range), run::<TransitionRow>(&clickhouse, &before));
    match (in_range, before) {
        (Ok(rows), Ok(initial)) => {
            let (rows, truncated) = read::newest(rows);
            Json(Transitions {
                initial: initial.into_iter().map(TransitionRecord::from).collect(),
                transitions: rows.into_iter().map(TransitionRecord::from).collect(),
                truncated,
            })
            .into_response()
        }
        (Err(e), _) | (_, Err(e)) => fail(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

#[derive(Serialize)]
struct Evaluations {
    evaluations: Vec<EvaluationRecord>,
    truncated: bool,
}

/// `GET {id}/evaluations?from&to[&group]`: the evaluated values, for the graph.
pub async fn evaluations(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    Path(id): Path<String>,
    QueryMulti(q): QueryMulti<RangeOfGroup>,
) -> Response {
    let Some(monitor) = monitor_id(&id) else { return StatusCode::NOT_FOUND.into_response() };
    let (from, until) = match time_range(&q.from, &q.to) {
        Ok(range) => range,
        Err(e) => return fail(StatusCode::BAD_REQUEST, e),
    };
    type Row = (String, i64, Option<f64>, String, String, String);
    answer(&clickhouse, &read::evaluations(&tenant(&claims), monitor, from, until, &q.group), |rows: Vec<Row>| {
        let rows = rows
            .into_iter()
            .map(|(group, at, value, state, outcome, detail)| EvaluationRecord {
                group,
                at,
                value: finite(value),
                state,
                outcome,
                detail: Some(detail).filter(|d| !d.is_empty()),
            })
            .collect();
        let (evaluations, truncated) = read::newest(rows);
        Evaluations { evaluations, truncated }
    })
    .await
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct CheckRunsQuery {
    query: String,
    from: String,
    to: String,
}

#[derive(Serialize)]
struct CheckRun {
    #[serde(serialize_with = "json::as_offset")]
    at: i64,
    host: String,
    message: String,
    status: String,
    tags: Vec<String>,
}

#[derive(Serialize)]
struct CheckRuns {
    runs: Vec<CheckRun>,
}

/// `GET check-runs?query&from&to`: the newest 100 runs a check monitor's query reads, newest first.
pub async fn check_runs(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    QueryMulti(q): QueryMulti<CheckRunsQuery>,
) -> Response {
    let (query, (from, until)) = match (check::query(&q.query), time_range(&q.from, &q.to)) {
        (Err(problems), _) => return fail(StatusCode::BAD_REQUEST, problems.join("; ")),
        (_, Err(e)) => return fail(StatusCode::BAD_REQUEST, e),
        (Ok(query), Ok(range)) => (query, range),
    };
    type Row = (i64, String, String, String, Vec<String>);
    answer(&clickhouse, &check::runs(&tenant(&claims), &query, from, until), |rows: Vec<Row>| CheckRuns {
        runs: rows
            .into_iter()
            .map(|(at, host, status, message, tags)| CheckRun { at, host, message, status, tags })
            .collect(),
    })
    .await
}
