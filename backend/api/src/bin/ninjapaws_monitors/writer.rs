//! Writing to ClickHouse: one insert awaited, for rows that must be stored, and one buffered
//! writer per table for the rest.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

/// An insert gives up after this long; ClickHouse may still finish it.
const INSERT_TIMEOUT: Duration = Duration::from_secs(120);

/// The client the write path uses. Kept apart from the read client so no query can ever write,
/// and so a slow query and a slow insert do not share a timeout. Inserts are synchronous:
/// ClickHouse buffers them on its side by default (async_insert=1) and makes parts of its own
/// choosing, while the writers' batches are what the part counts are tuned for.
pub fn client(addr: &str, database: &str, user: &str, password: &str) -> clickhouse::Client {
    clickhouse::Client::default()
        .with_url(format!("http://{addr}"))
        .with_database(database)
        .with_user(user)
        .with_password(password)
        .with_setting("async_insert", "0")
}

/// Inserts now and fails if ClickHouse refuses; for rows that must be stored, as the buffered
/// writer drops a failed batch.
pub async fn insert<T>(client: &clickhouse::Client, table: &str, rows: &[T]) -> Result<(), String>
where
    T: clickhouse::RowOwned + clickhouse::RowWrite + Serialize,
{
    if rows.is_empty() {
        return Ok(());
    }
    let sent = async {
        let mut insert = client.insert::<T>(table).await?;
        for row in rows {
            insert.write(row).await?;
        }
        insert.end().await
    };
    match tokio::time::timeout(INSERT_TIMEOUT, sent).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!("the insert into {table} did not finish in {} seconds", INSERT_TIMEOUT.as_secs())),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// Flush once this many rows are buffered…
    pub max_rows: usize,
    /// …or this much time has passed.
    pub flush_interval: Duration,
    /// Rows beyond this are dropped while ClickHouse is unreachable: losing data beats running out
    /// of memory.
    pub buffer_limit: usize,
    /// Concurrent flushes, so a slow database does not multiply connections.
    pub max_in_flight: usize,
}

pub const DEFAULT_LIMITS: Limits =
    Limits { max_rows: 10_000, flush_interval: Duration::from_secs(5), buffer_limit: 200_000, max_in_flight: 4 };

/// One insert is a part here and one in each table the views fill (metrics_1m, metric_tag_keys).
pub const METRICS_LIMITS: Limits =
    Limits { max_rows: 50_000, flush_interval: Duration::from_secs(10), buffer_limit: 150_000, max_in_flight: 4 };

#[derive(Clone, Debug, PartialEq)]
pub struct Stats {
    pub table: String,
    pub buffered: usize,
    pub written: u64,
    pub dropped: u64,
    pub failed: u64,
}

/// What self-monitoring reads of a writer, whatever its rows.
pub trait Stated: Send + Sync {
    fn stats(&self) -> Stats;
}

impl<T: Send> Stated for TableWriter<T> {
    fn stats(&self) -> Stats {
        let state = self.state.lock().unwrap();
        Stats {
            table: self.table.clone(),
            buffered: state.buffer.len(),
            written: state.written,
            dropped: state.dropped,
            failed: state.failed,
        }
    }
}

struct State<T> {
    buffer: Vec<T>,
    written: u64,
    dropped: u64,
    failed: u64,
}

/// Buffers rows for ONE table and inserts them in batches.
///
/// One writer per table, never a pool for the same table: a pool would split the buffer into
/// smaller batches and bring back ClickHouse's "Too many parts". A slow insert does not block the
/// next one either: the full buffer is handed to a background task and a fresh one takes its
/// place.
pub struct TableWriter<T> {
    table: String,
    limits: Limits,
    client: clickhouse::Client,
    state: Mutex<State<T>>,
    /// A flush holds one permit while it runs, so stopping can wait for them all.
    in_flight: Arc<Semaphore>,
    timer: Mutex<Option<JoinHandle<()>>>,
}

impl<T> TableWriter<T>
where
    T: clickhouse::RowOwned + clickhouse::RowWrite + Serialize + Send + Sync + 'static,
{
    /// Flushes every `flush_interval` from now until `stop`.
    pub fn start(client: clickhouse::Client, table: &str, limits: Limits) -> Arc<Self> {
        let writer = Arc::new(TableWriter {
            table: table.to_string(),
            limits,
            client,
            state: Mutex::new(State { buffer: Vec::new(), written: 0, dropped: 0, failed: 0 }),
            in_flight: Arc::new(Semaphore::new(limits.max_in_flight)),
            timer: Mutex::new(None),
        });
        let timed = Arc::downgrade(&writer);
        let timer = tokio::spawn(async move {
            let mut every = tokio::time::interval(limits.flush_interval);
            every.tick().await;
            loop {
                every.tick().await;
                match timed.upgrade() {
                    Some(writer) => writer.flush(),
                    None => return,
                }
            }
        });
        *writer.timer.lock().unwrap() = Some(timer);
        writer
    }

    pub fn add(self: &Arc<Self>, rows: Vec<T>) {
        if rows.is_empty() {
            return;
        }
        let full = {
            let mut state = self.state.lock().unwrap();
            if state.buffer.len() + rows.len() > self.limits.buffer_limit {
                state.dropped += rows.len() as u64;
                eprintln!(
                    "warning: writer {}: buffer full ({} rows), dropped {} (total {})",
                    self.table,
                    state.buffer.len(),
                    rows.len(),
                    state.dropped
                );
                false
            } else {
                state.buffer.extend(rows);
                state.buffer.len() >= self.limits.max_rows
            }
        };
        if full {
            self.flush();
        }
    }

    /// Starts a flush of the buffer when one may start now.
    fn flush(self: &Arc<Self>) {
        let (permit, batch) = {
            let mut state = self.state.lock().unwrap();
            if state.buffer.is_empty() {
                return;
            }
            let Ok(permit) = self.in_flight.clone().try_acquire_owned() else {
                eprintln!(
                    "warning: writer {}: {} flushes in flight, deferring ({} buffered)",
                    self.table,
                    self.limits.max_in_flight,
                    state.buffer.len()
                );
                return;
            };
            (permit, std::mem::take(&mut state.buffer))
        };
        let writer = self.clone();
        tokio::spawn(async move {
            writer.send(batch).await;
            drop(permit);
        });
    }

    /// The batch is lost when the insert fails, on purpose: putting it back would need the
    /// ordering and memory guarantees this design avoids.
    async fn send(&self, batch: Vec<T>) {
        let result = insert(&self.client, &self.table, &batch).await;
        let mut state = self.state.lock().unwrap();
        match result {
            Ok(()) => state.written += batch.len() as u64,
            Err(e) => {
                state.failed += batch.len() as u64;
                eprintln!("error: writer {}: insert of {} rows failed: {e}", self.table, batch.len());
            }
        }
    }

    /// Stops the timer, waits for the flushes under way and inserts what is left: at shutdown
    /// there is nothing left to protect from a slow insert.
    pub async fn stop(&self) {
        if let Some(timer) = self.timer.lock().unwrap().take() {
            timer.abort();
        }
        let all = self.limits.max_in_flight as u32;
        let _flushed = self.in_flight.acquire_many(all).await;
        let rest = std::mem::take(&mut self.state.lock().unwrap().buffer);
        if !rest.is_empty() {
            self.send(rest).await;
        }
    }
}
