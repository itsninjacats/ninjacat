//! The `monitor` table, read only: the panel owns it.
//!
//! Without TLS, as the query process's health ping: a `DATABASE_URL` that asks for it
//! (`sslmode=require`) does not connect.

use std::future::poll_fn;
use std::time::Duration;

use deadpool_postgres::{Manager, ManagerConfig, Pool, RecyclingMethod};
use tokio::sync::mpsc;
use tokio_postgres::{AsyncMessage, NoTls};

/// The entity id, `<tenant>/<id>`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MonitorKey {
    pub tenant: String,
    pub id: i64,
}

impl MonitorKey {
    pub fn text(&self) -> String {
        format!("{}/{}", self.tenant, self.id)
    }

    /// The id is the last segment, so a tenant id with a slash still parses.
    pub fn parse(text: &str) -> Option<MonitorKey> {
        let (tenant, id) = text.rsplit_once('/')?;
        match id.parse::<i64>() {
            Ok(id) if !tenant.is_empty() => Some(MonitorKey { tenant: tenant.to_string(), id }),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct MonitorRow {
    pub key: MonitorKey,
    pub version: i32,
    pub monitor_type: String,
    pub query: String,
    pub options: String,
    pub updated_by: Option<String>,
    pub deleted_by: Option<String>,
    pub deleted_at: Option<i64>,
}

/// A pool of connections for the reads.
pub fn pool(config: tokio_postgres::Config) -> Pool {
    let manager = Manager::from_config(config, NoTls, ManagerConfig { recycling_method: RecyclingMethod::Fast });
    Pool::builder(manager).build().expect("a pool without a runtime of its own builds")
}

/// Postgres's instants as unix milliseconds, so no clock of ours is compared with them.
const NOW_MS: &str = "floor(extract(epoch FROM now()) * 1000)::int8";

pub async fn by_key(db: &Pool, key: &MonitorKey) -> Result<Option<MonitorRow>, String> {
    let client = db.get().await.map_err(|e| e.to_string())?;
    let row = client
        .query_opt(
            "SELECT version, type, query, options::text, updated_by_id, deleted_by_id, \
             floor(extract(epoch FROM deleted_at) * 1000)::int8 FROM monitor WHERE tenant_id = $1 AND id = $2",
            &[&key.tenant, &key.id],
        )
        .await
        .map_err(|e| e.to_string())?;
    Ok(row.map(|r| MonitorRow {
        key: key.clone(),
        version: r.get(0),
        monitor_type: r.get(1),
        query: r.get(2),
        options: r.get(3),
        updated_by: r.get(4),
        deleted_by: r.get(5),
        deleted_at: r.get(6),
    }))
}

#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub key: MonitorKey,
    pub version: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    /// Postgres's clock, so no clock of ours is compared with `deleted_at`.
    pub at: i64,
    pub live: Vec<Seen>,
    pub deleted: Vec<Seen>,
}

/// Every live monitor, and those deleted after `deleted_since` (None: in the last day).
pub async fn snapshot(db: &Pool, deleted_since: Option<i64>) -> Result<Snapshot, String> {
    let client = db.get().await.map_err(|e| e.to_string())?;
    let clock = client.query_one(&format!("SELECT {NOW_MS}"), &[]).await.map_err(|e| e.to_string())?;
    let rows = client
        .query(
            "SELECT tenant_id, id, version, deleted_at IS NOT NULL FROM monitor \
             WHERE deleted_at IS NULL OR deleted_at > coalesce(to_timestamp($1::int8 / 1000.0), now() - interval '1 day')",
            &[&deleted_since],
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut snapshot = Snapshot { at: clock.get(0), live: Vec::new(), deleted: Vec::new() };
    for r in rows {
        let seen = Seen { key: MonitorKey { tenant: r.get(0), id: r.get(1) }, version: r.get(2) };
        if r.get::<_, bool>(3) {
            snapshot.deleted.push(seen);
        } else {
            snapshot.live.push(seen);
        }
    }
    Ok(snapshot)
}

/// frontend/src/lib/server/monitors.ts notifies on it.
pub const CHANNEL: &str = "ninjacat_monitors";

/// `<tenant>/<id>/<version>`
pub fn parse_notification(payload: &str) -> Option<Seen> {
    let (key, version) = payload.rsplit_once('/')?;
    let version = version.parse::<i32>().ok()?;
    Some(Seen { key: MonitorKey::parse(key)?, version })
}

/// The LISTEN connection only waits, so without a query now and then a link that died silently
/// would leave it waiting for good.
const KEEPALIVE: Duration = Duration::from_secs(30);

/// Its own connection, since LISTEN belongs to a session; each payload goes to `notified`.
/// Returns, with why, when the connection ends.
pub async fn listen(config: &tokio_postgres::Config, channel: &str, notified: mpsc::UnboundedSender<String>) -> String {
    let (client, mut connection) = match config.connect(NoTls).await {
        Ok(connected) => connected,
        Err(e) => return e.to_string(),
    };
    // The connection is driven here, where its notifications arrive, while the client's queries
    // wait on it.
    let mut driver = tokio::spawn(async move {
        loop {
            match poll_fn(|cx| connection.poll_message(cx)).await {
                Some(Ok(AsyncMessage::Notification(n))) => {
                    if notified.send(n.payload().to_string()).is_err() {
                        return "nobody listens".to_string();
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return e.to_string(),
                None => return "the connection closed".to_string(),
            }
        }
    });
    let quoted = channel.replace('"', "\"\"");
    let listening = async {
        client.batch_execute(&format!("LISTEN \"{quoted}\"")).await.map_err(|e| e.to_string())?;
        let mut every = tokio::time::interval(KEEPALIVE);
        loop {
            every.tick().await;
            match tokio::time::timeout(Duration::from_secs(10), client.simple_query("SELECT 1")).await {
                Ok(Ok(_)) => {}
                Ok(Err(e)) => return Err::<(), String>(e.to_string()),
                Err(_) => return Err("the keepalive query was not answered in 10 seconds".to_string()),
            }
        }
    };
    tokio::select! {
        ended = &mut driver => {
            ended.unwrap_or_else(|e| e.to_string())
        }
        failed = listening => {
            driver.abort();
            failed.err().unwrap_or_default()
        }
    }
}
