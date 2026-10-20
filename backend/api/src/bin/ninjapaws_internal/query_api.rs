//! Datadog's query API: `GET /api/v1/metrics`, `POST /api/v2/query/timeseries` and
//! `POST /api/v2/query/scalar`. HTTP in, an engine call, JSON out; no SQL and no query logic here.

use ninjapaws_engine::api::v2::{scalar, timeseries};
use ninjapaws_engine::metrics::{self, ActiveMetricsRequest};
use ninjapaws_engine::query::execute::{self, QueryError};
use ninjapaws_engine::query::plan::{self, Retention};
use ninjapaws_engine::unix_time;
use serde::{Deserialize, Serialize};
use tako::StatusCode;
use tako::extractors::json::Json;
use tako::extractors::jwt::JwtClaimsVerified;
use tako::extractors::query_multi::QueryMulti;
use tako::extractors::state::State;
use tako::responder::Responder;
use tako::types::Response;

use crate::panel::{now, run, tenant};
use crate::reads::ClickHouse;
use crate::token::Claims;

/// Datadog's error envelope, so API clients report our errors the way they report Datadog's.
#[derive(Serialize)]
struct Errors {
    errors: Vec<String>,
}

fn errors(status: StatusCode, errors: Vec<String>) -> Response {
    (status, Json(Errors { errors })).into_response()
}

fn failed(what: &str, e: String) -> Response {
    eprintln!("{what} query failed: {e}");
    errors(StatusCode::INTERNAL_SERVER_ERROR, vec![format!("query failed: {e}")])
}

#[derive(Default, Deserialize)]
#[serde(default)]
pub struct ActiveMetricsQuery {
    from: Option<String>,
    host: String,
}

#[derive(Serialize)]
struct ActiveMetrics {
    from: String,
    metrics: Vec<String>,
}

/// `GET /api/v1/metrics?from=<unix seconds>[&host=<name>]`
///
/// `tag_filter` is accepted by Datadog too; it waits for the tag filter parser.
pub async fn active_metrics(
    State(clickhouse): State<ClickHouse>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    QueryMulti(q): QueryMulti<ActiveMetricsQuery>,
) -> Response {
    let from = match q.from.as_deref().map(|f| f.trim().parse::<i64>()) {
        None => return errors(StatusCode::BAD_REQUEST, vec!["'from' is required".into()]),
        Some(Err(_)) => return errors(StatusCode::BAD_REQUEST, vec!["'from' must be a unix timestamp in seconds".into()]),
        Some(Ok(from)) => from,
    };
    let req = ActiveMetricsRequest {
        tenant: tenant(&claims),
        from: unix_time::of_seconds(from),
        host: Some(q.host).filter(|h| !h.is_empty()),
    };
    match run::<(String,)>(&clickhouse, &metrics::active_metrics(&req)).await {
        Ok(found) => Json(ActiveMetrics { from: from.to_string(), metrics: found.into_iter().map(|(m,)| m).collect() }).into_response(),
        Err(e) => failed("active metrics", e),
    }
}

/// `POST /api/v2/query/timeseries`
///
/// Read, plan, run. Everything that can be wrong with the request is a 400 listing every
/// problem; a failure while running is a 500 with the reason.
pub async fn timeseries(
    State(clickhouse): State<ClickHouse>,
    State(retention): State<Retention>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    body: String,
) -> Response {
    let planned = timeseries::read(&body).and_then(|req| plan::plan(*retention, now(), &req));
    match planned {
        Err(problems) => errors(StatusCode::BAD_REQUEST, problems),
        Ok(plan) => match execute::run(&*clickhouse, &tenant(&claims), &plan).await {
            Ok(response) => Json(response).into_response(),
            Err(e) => failed("timeseries", e),
        },
    }
}

/// `POST /api/v2/query/scalar`: one value per group and formula, for query value, top list and
/// table widgets.
pub async fn scalar(
    State(clickhouse): State<ClickHouse>,
    State(retention): State<Retention>,
    JwtClaimsVerified(claims): JwtClaimsVerified<Claims>,
    body: String,
) -> Response {
    let planned = scalar::read(&body).and_then(|r| plan::plan(*retention, now(), &r.request).map(|p| (p, r)));
    match planned {
        Err(problems) => errors(StatusCode::BAD_REQUEST, problems),
        Ok((plan, request)) => match execute::run_scalar(&*clickhouse, &tenant(&claims), &plan, &request).await {
            Ok(response) => Json(response).into_response(),
            Err(QueryError::Invalid(problems)) => errors(StatusCode::BAD_REQUEST, problems),
            Err(QueryError::Failed(e)) => failed("scalar", e),
        },
    }
}
