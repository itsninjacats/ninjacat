//! Buffers rows for ONE table and inserts them in batches.
//!
//! One writer per table, never a pool for the same table: a pool would split the buffer into
//! smaller batches and bring back ClickHouse's "Too many parts". A slow insert does not block the
//! next one either: the full buffer is handed to a background task and a fresh one takes its
//! place, up to `max_in_flight` at once.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use tokio::task::JoinHandle;
use tokio_util::task::TaskTracker;

use crate::table::Table;

pub(crate) struct Writer<T: Table> {
    client: clickhouse::Client,
    state: Mutex<State<T>>,
    flushes: TaskTracker,
    timer: Mutex<Option<JoinHandle<()>>>,
    written: AtomicU64,
    failed: AtomicU64,
}

struct State<T> {
    buffer: Vec<T>,
    in_flight: usize,
    dropped: u64,
}

/// What a writer has done so far.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriterStats {
    pub table: &'static str,
    pub buffered: usize,
    pub written: u64,
    pub dropped: u64,
    pub failed: u64,
    pub in_flight: usize,
}

/// A writer of any table, as the sink sees it.
pub(crate) trait Running: Send + Sync {
    fn stats(&self) -> WriterStats;
    fn stop(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}

impl<T: Table> Writer<T> {
    /// A writer and the timer that flushes it. Called inside the runtime.
    pub(crate) fn start(client: clickhouse::Client) -> Arc<Self> {
        let writer = Arc::new(Writer {
            client,
            state: Mutex::new(State { buffer: Vec::new(), in_flight: 0, dropped: 0 }),
            flushes: TaskTracker::new(),
            timer: Mutex::new(None),
            written: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        });
        let ticking = writer.clone();
        let timer = tokio::spawn(async move {
            let mut every = tokio::time::interval(T::LIMITS.flush_interval);
            every.tick().await;
            loop {
                every.tick().await;
                ticking.flush();
            }
        });
        *writer.timer.lock().unwrap() = Some(timer);
        writer
    }

    pub(crate) fn add(self: &Arc<Self>, rows: Vec<T>) {
        let batch = {
            let mut state = self.state.lock().unwrap();
            if state.buffer.len() + rows.len() > T::LIMITS.buffer_limit {
                state.dropped += rows.len() as u64;
                tracing::warn!(
                    "writer {}: buffer full ({} rows), dropped {} (total {})",
                    T::TABLE,
                    state.buffer.len(),
                    rows.len(),
                    state.dropped
                );
                None
            } else {
                state.buffer.extend(rows);
                if state.buffer.len() >= T::LIMITS.max_rows { take_batch::<T>(&mut state) } else { None }
            }
        };
        if let Some(batch) = batch {
            self.flush_in_background(batch);
        }
    }

    fn flush(self: &Arc<Self>) {
        let batch = take_batch::<T>(&mut self.state.lock().unwrap());
        if let Some(batch) = batch {
            self.flush_in_background(batch);
        }
    }

    fn flush_in_background(self: &Arc<Self>, batch: Vec<T>) {
        let writer = self.clone();
        self.flushes.spawn(async move {
            writer.send(batch).await;
            writer.state.lock().unwrap().in_flight -= 1;
        });
    }

    async fn send(&self, batch: Vec<T>) {
        match insert(&self.client, &batch).await {
            Ok(()) => {
                self.written.fetch_add(batch.len() as u64, Ordering::Relaxed);
            }
            Err(e) => {
                // The batch is lost on purpose: putting it back would need the ordering and
                // memory guarantees this design avoids.
                self.failed.fetch_add(batch.len() as u64, Ordering::Relaxed);
                tracing::error!("writer {}: insert of {} rows failed: {e}", T::TABLE, batch.len());
            }
        }
    }
}

/// Takes the buffer if a flush may start now.
fn take_batch<T: Table>(state: &mut State<T>) -> Option<Vec<T>> {
    if state.buffer.is_empty() {
        None
    } else if state.in_flight >= T::LIMITS.max_in_flight {
        tracing::warn!(
            "writer {}: {} flushes in flight, deferring ({} buffered)",
            T::TABLE,
            state.in_flight,
            state.buffer.len()
        );
        None
    } else {
        state.in_flight += 1;
        Some(std::mem::take(&mut state.buffer))
    }
}

impl<T: Table> Running for Writer<T> {
    fn stats(&self) -> WriterStats {
        let state = self.state.lock().unwrap();
        WriterStats {
            table: T::TABLE,
            buffered: state.buffer.len(),
            written: self.written.load(Ordering::Relaxed),
            dropped: state.dropped,
            failed: self.failed.load(Ordering::Relaxed),
            in_flight: state.in_flight,
        }
    }

    /// Stops the timer, waits for the flushes under way and inserts what is left: at shutdown
    /// there is nothing left to protect from a slow insert.
    fn stop(&self) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            if let Some(timer) = self.timer.lock().unwrap().take() {
                timer.abort();
            }
            self.flushes.close();
            self.flushes.wait().await;
            let rest = std::mem::take(&mut self.state.lock().unwrap().buffer);
            if !rest.is_empty() {
                self.send(rest).await;
            }
        })
    }
}

/// Inserts now and fails if ClickHouse refuses.
pub(crate) async fn insert<T: Table>(client: &clickhouse::Client, rows: &[T]) -> clickhouse::error::Result<()> {
    let mut insert = client.insert::<T>(T::TABLE).await?;
    for row in rows {
        insert.write(row).await?;
    }
    insert.end().await
}
