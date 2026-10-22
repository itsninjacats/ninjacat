//! Open to anyone who reaches the port: the probes of compose and Kubernetes.

use std::time::Duration;

use api::reads::ClickHouse;
use serde::Serialize;
use tako::StatusCode;
use tako::extractors::json::Json;
use tako::extractors::state::State;
use tako::responder::Responder;
use tako::types::Response;

/// Postgres, which only `/health` asks after: the panel's database, as `DATABASE_URL` names it.
#[derive(Clone)]
pub struct Postgres {
    pub url: String,
}

impl Postgres {
    /// A connection of its own each time: a probe every few seconds needs no pool. Without TLS;
    /// a URL that asks for it (`sslmode=require`) reads as unreachable.
    async fn ping(&self) -> bool {
        let ping = async {
            let (client, connection) = tokio_postgres::connect(&self.url, tokio_postgres::NoTls).await?;
            tokio::spawn(connection);
            client.simple_query("SELECT 1").await
        };
        matches!(tokio::time::timeout(Duration::from_secs(5), ping).await, Ok(Ok(_)))
    }
}

pub async fn ping() -> &'static str {
    "pong"
}

#[derive(Serialize)]
struct Health {
    clickhouse: &'static str,
    postgres: &'static str,
}

fn status(ok: bool) -> &'static str {
    if ok { "ok" } else { "unreachable" }
}

pub async fn health(State(clickhouse): State<ClickHouse>, State(postgres): State<Postgres>) -> Response {
    let (clickhouse, postgres) = tokio::join!(clickhouse.ping(), postgres.ping());
    let code = if clickhouse && postgres { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (code, Json(Health { clickhouse: status(clickhouse), postgres: status(postgres) })).into_response()
}
