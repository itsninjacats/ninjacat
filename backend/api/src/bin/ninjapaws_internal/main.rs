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
mod reads;
#[cfg(test)]
mod tests;
mod token;

use std::env;

use chrono::TimeDelta;
use ninjapaws_engine::query::plan::{DEFAULT_RETENTION, Retention};
use tako::Server;
use tako::middleware::IntoMiddleware;
use tako::middleware::jwt_auth::JwtAuth;
use tako::router::Router;
use tako::types::BoxError;
use tokio::net::TcpListener;

use crate::probes::Postgres;
use crate::reads::ClickHouse;

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

fn var_or(name: &str, default: &str) -> String {
    env::var(name).ok().filter(|v| !v.is_empty()).unwrap_or_else(|| default.to_string())
}

/// `:8081`, `*:8081` and `0.0.0.0:8081` are every interface, as the settings have always spelled it.
fn listen_on(addr: &str) -> String {
    match addr.rsplit_once(':') {
        Some(("" | "*", port)) => format!("0.0.0.0:{port}"),
        _ => addr.to_string(),
    }
}

/// A whole number of days, up to a century. A set value outside 1–36500 stops the start: falling
/// back to the default could set a shorter TTL than meant, and ClickHouse would delete the
/// difference.
fn days(name: &str, default: TimeDelta) -> Result<TimeDelta, String> {
    match var_or(name, "").as_str() {
        "" => Ok(default),
        raw => match raw.trim().parse::<i64>() {
            Ok(n) if (1..=36500).contains(&n) => Ok(TimeDelta::days(n)),
            _ => Err(format!("{name} must be a whole number of days from 1 to 36500, got \"{raw}\"")),
        },
    }
}

/// A step of whole minutes reads the rollup whatever its age, so a rollup kept shorter than the
/// raw points would answer nothing where the raw table still holds them.
fn metrics_retention() -> Result<Retention, String> {
    let raw = days("NINJACAT_METRICS_RETENTION_DAYS", DEFAULT_RETENTION.raw)?;
    let rollup = days("NINJACAT_METRICS_ROLLUP_RETENTION_DAYS", DEFAULT_RETENTION.rollup)?;
    if rollup < raw {
        return Err(format!(
            "NINJACAT_METRICS_ROLLUP_RETENTION_DAYS ({}) must not be shorter than NINJACAT_METRICS_RETENTION_DAYS ({})",
            rollup.num_days(),
            raw.num_days()
        ));
    }
    Ok(Retention { raw, rollup })
}
