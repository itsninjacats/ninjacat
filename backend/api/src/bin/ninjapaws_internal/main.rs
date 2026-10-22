//! What the panel reads from: the panel's internal API (`/internal/*`) and Datadog's query API
//! (`/api/v1/metrics`, `/api/v2/query/*`), on the internal port.
//!
//! It only reads: its ClickHouse client cannot write. Every route but the probes asks for the
//! panel's token, which names the tenant; the port still belongs on a network only the panel can
//! reach.
mod monitors;
mod panel;
mod probes;
mod query_api;
#[cfg(test)]
mod tests;
mod token;

use std::env;

use api::reads::ClickHouse;
use api::{listen_on, metrics_retention, var_or};
use ninjapaws_engine::query::plan::Retention;
use tako::Server;
use tako::middleware::IntoMiddleware;
use tako::middleware::jwt_auth::JwtAuth;
use tako::router::Router;
use tako::types::BoxError;
use tokio::net::TcpListener;

use crate::probes::Postgres;

/// ClickHouse's limit for one of the panel's queries.
const QUERY_LIMIT_SECONDS: u64 = 7;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    let secret = env::var("NINJACAT_QUERY_TOKEN_SECRET").unwrap_or_default();
    let database_url = var_or("DATABASE_URL", "");
    if database_url.is_empty() {
        return Err("DATABASE_URL is not set (e.g. postgres://root:mysecretpassword@localhost:5433/local)".into());
    }
    let clickhouse = ClickHouse::new(
        &var_or("CLICKHOUSE_HTTP_ADDR", "localhost:8123"),
        &var_or("CLICKHOUSE_DB", "ninjacat"),
        &var_or("CLICKHOUSE_USER", "ninjacat"),
        &var_or("CLICKHOUSE_PASSWORD", "ninjacat"),
        QUERY_LIMIT_SECONDS,
    );
    let router = app(&secret, clickhouse, metrics_retention()?, Postgres { url: database_url })?;

    let addr = var_or("NINJACAT_INTERNAL_ADDR", ":8081");
    let listener = TcpListener::bind(listen_on(&addr)).await?;
    println!("ninjapaws_internal listening on {addr}");
    Server::builder().build().try_spawn_http(listener, router)?.result().await?;
    Ok(())
}

/// The routes, behind the panel's token but for the probes.
fn app(secret: &str, clickhouse: ClickHouse, retention: Retention, postgres: Postgres) -> Result<Router, String> {
    if secret.len() < 32 {
        return Err("NINJACAT_QUERY_TOKEN_SECRET must be at least 32 bytes (openssl rand -hex 32)".into());
    }

    // Every route here asks for the token: one added later is behind it without asking.
    let mut guarded = Router::new();
    guarded.middleware(JwtAuth::new(token::PanelToken::new(secret.as_bytes())).into_middleware());
    guarded.get("/internal/metrics/names", panel::metric_names);
    guarded.get("/internal/metrics/tags", panel::metric_tag_keys);
    guarded.get("/internal/metrics/tag-values", panel::metric_tag_values);
    guarded.get("/internal/metrics/query", panel::metric_query);
    guarded.get("/internal/logs/search", panel::log_search);
    guarded.get("/internal/logs/facets", panel::log_facets);
    guarded.get("/internal/checks/names", panel::check_names);
    guarded.post("/internal/monitors/validate", monitors::validate);
    guarded.get("/internal/monitors/status", monitors::status);
    guarded.get("/internal/monitors/check-runs", monitors::check_runs);
    guarded.get("/internal/monitors/{id}/groups", monitors::groups);
    guarded.get("/internal/monitors/{id}/transitions", monitors::transitions);
    guarded.get("/internal/monitors/{id}/evaluations", monitors::evaluations);
    guarded.get("/api/v1/metrics", query_api::active_metrics);
    guarded.post("/api/v2/query/timeseries", query_api::timeseries);
    guarded.post("/api/v2/query/scalar", query_api::scalar);

    let mut router = Router::new();
    router.with_state(clickhouse);
    router.with_state(retention);
    router.with_state(postgres);
    router.get("/ping", probes::ping);
    router.get("/health", probes::health);
    router.merge(guarded);
    Ok(router)
}
