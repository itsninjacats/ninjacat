//! Where handlers put rows. Writing never waits: the sender gets its answer first, and batching
//! and flushing happen behind this.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::table::Table;
use crate::writer::{self, Running, Writer, WriterStats};

#[derive(Clone)]
pub enum Sink {
    /// One writer per table, made the first time the table is written to.
    ClickHouse(Arc<Writers>),
    /// What tests read back: every write, in order.
    Capture(Arc<Capture>),
}

impl Sink {
    pub fn clickhouse(client: clickhouse::Client) -> Self {
        Sink::ClickHouse(Arc::new(Writers {
            client,
            writers: Mutex::new(HashMap::new()),
            running: Mutex::new(Vec::new()),
        }))
    }

    pub fn capture() -> (Self, Arc<Capture>) {
        let capture = Arc::new(Capture::default());
        (Sink::Capture(capture.clone()), capture)
    }

    pub fn write<T: Table>(&self, rows: Vec<T>) {
        if rows.is_empty() {
            return;
        }
        match self {
            Sink::ClickHouse(writers) => writers.writer::<T>().add(rows),
            Sink::Capture(capture) => capture.record(rows),
        }
    }

    /// Inserts now and fails if ClickHouse refuses: for rows that must be stored, as a writer
    /// drops a failed batch.
    pub async fn insert<T: Table>(&self, rows: Vec<T>) -> Result<(), String> {
        match self {
            Sink::ClickHouse(writers) if !rows.is_empty() => {
                writer::insert(&writers.client, &rows).await.map_err(|e| e.to_string())
            }
            Sink::ClickHouse(_) => Ok(()),
            Sink::Capture(capture) => {
                capture.record(rows);
                Ok(())
            }
        }
    }

    /// Flushes every writer. Called last, once nothing feeds them any more.
    pub async fn stop(&self) {
        if let Sink::ClickHouse(writers) = self {
            let running = writers.running.lock().unwrap().clone();
            for writer in running {
                writer.stop().await;
            }
        }
    }

    /// What each table's writer has done, by table.
    pub fn stats(&self) -> Vec<WriterStats> {
        match self {
            Sink::ClickHouse(writers) => {
                let mut stats: Vec<WriterStats> = writers.running.lock().unwrap().iter().map(|w| w.stats()).collect();
                stats.sort_by_key(|s| s.table);
                stats
            }
            Sink::Capture(_) => Vec::new(),
        }
    }
}

pub struct Writers {
    client: clickhouse::Client,
    writers: Mutex<HashMap<&'static str, Arc<dyn Any + Send + Sync>>>,
    running: Mutex<Vec<Arc<dyn Running>>>,
}

impl Writers {
    fn writer<T: Table>(&self) -> Arc<Writer<T>> {
        let mut writers = self.writers.lock().unwrap();
        if let Some(found) = writers.get(T::WRITER) {
            return found.clone().downcast().expect("one row type per writer");
        }
        let made = Writer::<T>::start(self.client.clone());
        writers.insert(T::WRITER, made.clone());
        self.running.lock().unwrap().push(made.clone());
        made
    }
}

/// One batch handed to the sink.
#[derive(Clone, Debug)]
pub struct Write {
    pub writer: &'static str,
    pub table: &'static str,
    pub columns: &'static [&'static str],
    /// Each row as JSON, a field per column.
    pub rows: Vec<serde_json::Value>,
}

#[derive(Default)]
pub struct Capture {
    writes: Mutex<Vec<Write>>,
}

impl Capture {
    fn record<T: Table>(&self, rows: Vec<T>) {
        let rows = rows.iter().map(|row| serde_json::to_value(row).expect("a row is JSON")).collect();
        self.writes.lock().unwrap().push(Write { writer: T::WRITER, table: T::TABLE, columns: T::COLUMN_NAMES, rows });
    }

    pub fn writes(&self) -> Vec<Write> {
        self.writes.lock().unwrap().clone()
    }
}
