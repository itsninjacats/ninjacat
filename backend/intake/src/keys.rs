//! API keys as the intake sees them: an in-memory snapshot, replaced whole, and the keeper that
//! keeps it in step with Postgres.
//!
//! The lookup is on the path of every agent request, so it takes no lock and asks no database:
//! one pointer load and one map lookup. Postgres holds only the SHA-256 of each key, which is what
//! the lookup is by.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use arc_swap::ArcSwap;
use futures::StreamExt;
use sha2::{Digest, Sha256};
use tokio_postgres::{AsyncMessage, NoTls};

#[derive(Clone, Debug)]
pub struct Key {
    pub id: String,
    pub name: String,
    pub tenant_id: String,
}

/// SHA-256 as lowercase hex: the only form of a key that is ever stored.
pub fn hash(key: &str) -> String {
    Sha256::digest(key.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Default)]
pub struct Store {
    by_hash: ArcSwap<HashMap<String, Key>>,
}

impl Store {
    /// Replaces every key at once. `keys` pairs each key with its hash.
    pub fn publish(&self, keys: impl IntoIterator<Item = (String, Key)>) {
        self.by_hash.store(Arc::new(keys.into_iter().collect()));
    }

    pub fn lookup(&self, key: &str) -> Option<Key> {
        if key.is_empty() { None } else { self.by_hash.load().get(&hash(key)).cloned() }
    }

    pub fn count(&self) -> usize {
        self.by_hash.load().len()
    }
}

/// The same name is in the panel (frontend/src/lib/server/api-keys.ts).
const CHANNEL: &str = "ninjacat_api_keys";
const REFRESH: Duration = Duration::from_secs(30);

async fn fetch(client: &tokio_postgres::Client, store: &Store) -> Result<(), tokio_postgres::Error> {
    let rows = client.query("SELECT id::text, name, tenant_id, key_hash FROM api_key", &[]).await?;
    store.publish(rows.iter().map(|row| (row.get(3), Key { id: row.get(0), name: row.get(1), tenant_id: row.get(2) })));
    Ok(())
}

/// The keys as Postgres has them now, kept in step by a keeper for the life of the process.
pub async fn start(database_url: &str) -> Result<Arc<Store>, String> {
    if database_url.is_empty() {
        return Err("DATABASE_URL is not set (e.g. postgres://root:mysecretpassword@localhost:5433/local)".into());
    }
    let store = Arc::new(Store::default());
    fetch_now(&store, database_url).await?;
    tokio::spawn(keep(store.clone(), database_url.to_string()));
    Ok(store)
}

/// Reads the keys once, now. Called before the intake accepts anything: with no keys every agent
/// would be answered 403, which tells it to stop. A Postgres the panel has not migrated yet has no
/// keys to read: the intake starts with none rather than wait for an order of installation.
///
/// Without TLS: a `DATABASE_URL` that asks for it (`sslmode=require`) does not connect.
pub async fn fetch_now(store: &Store, database_url: &str) -> Result<(), String> {
    let (client, connection) = tokio_postgres::connect(database_url, NoTls).await.map_err(|e| format!("keys: {e}"))?;
    tokio::spawn(connection);
    match fetch(&client, store).await {
        Ok(()) => {
            tracing::info!("keeper started, {} keys, refreshing every {REFRESH:?}", store.count());
            Ok(())
        }
        Err(e) if e.code() == Some(&tokio_postgres::error::SqlState::UNDEFINED_TABLE) => {
            tracing::warn!(
                "keeper started with no keys, the api_key table does not exist yet; refreshing every {REFRESH:?}"
            );
            Ok(())
        }
        Err(e) => Err(format!("keys: {e}")),
    }
}

/// Re-reads the keys every 30 seconds, and at once when the panel says a key was added or
/// removed (NOTIFY on the channel): the intake needs no port for the panel to call. Runs for the
/// life of the process. A refresh that fails keeps the old keys in force, and a Postgres that is
/// down is asked again after the interval, not in a tight loop.
pub async fn keep(store: Arc<Store>, database_url: String) {
    loop {
        if let Err(e) = listen_and_refresh(&store, &database_url).await {
            tracing::warn!("keeper: refresh failed: {e}");
            tokio::time::sleep(REFRESH).await;
        }
    }
}

/// Until the connection breaks. LISTEN belongs to a session, so it holds one.
async fn listen_and_refresh(store: &Store, database_url: &str) -> Result<(), tokio_postgres::Error> {
    let (client, mut connection) = tokio_postgres::connect(database_url, NoTls).await?;
    let (notify, mut notified) = tokio::sync::mpsc::unbounded_channel();
    tokio::spawn(async move {
        let mut messages = futures::stream::poll_fn(move |cx| connection.poll_message(cx));
        while let Some(Ok(message)) = messages.next().await {
            if let AsyncMessage::Notification(_) = message {
                let _ = notify.send(());
            }
        }
    });
    client.batch_execute(&format!("LISTEN {CHANNEL}")).await?;
    loop {
        tokio::select! {
            _ = notified.recv() => {}
            _ = tokio::time::sleep(REFRESH) => {}
        }
        fetch(&client, store).await?;
    }
}
