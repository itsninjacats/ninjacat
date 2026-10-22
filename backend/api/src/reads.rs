//! ClickHouse as the engine's SQL runs on it: each `Sql`'s values bound as server-side
//! parameters, the rows read back as JSON arrays, by position, as the SQL selects its columns.
//! The query process and the monitors read through it, each with its own limit.

use std::time::Duration;

use ninjapaws_engine::query::compile::Row;
use ninjapaws_engine::query::execute::Executor;
use ninjapaws_engine::{Sql, SqlValue};
use serde::de::DeserializeOwned;
use serde_json::Value;

#[derive(Clone)]
pub struct ClickHouse {
    client: clickhouse::Client,
    limit_seconds: u64,
}

impl ClickHouse {
    /// A client that cannot write: readonly=2 makes ClickHouse refuse anything but reads, while
    /// still letting the client set its own settings (readonly=1 would not). `limit_seconds` is
    /// ClickHouse's limit for one query: a slow query must fail there, with a readable message,
    /// before our own timeout, a second later, gives up on it.
    pub fn new(addr: &str, database: &str, user: &str, password: &str, limit_seconds: u64) -> Self {
        let client = clickhouse::Client::default()
            .with_url(format!("http://{addr}"))
            .with_database(database)
            .with_user(user)
            .with_password(password)
            .with_setting("readonly", "2")
            .with_setting("max_execution_time", limit_seconds.to_string())
            // Counts and ids as JSON numbers, not strings.
            .with_setting("output_format_json_quote_64bit_integers", "0");
        Self { client, limit_seconds }
    }

    /// Each row as `T`, from its JSON array: a tuple or a struct, field by field in the order
    /// the SQL selects.
    pub async fn read<T: DeserializeOwned>(&self, sql: &Sql) -> Result<Vec<T>, String> {
        let body = self.fetch(sql, "JSONCompactEachRow").await?;
        lines(&body).map(|line| serde_json::from_slice(line).map_err(|e| e.to_string())).collect()
    }

    pub async fn ping(&self) -> bool {
        let sql = Sql { text: "SELECT 1".into(), parameters: Vec::new() };
        self.read::<(u8,)>(&sql).await.is_ok()
    }

    async fn fetch(&self, sql: &Sql, format: &str) -> Result<Vec<u8>, String> {
        let mut query = self.client.query(&sql.text);
        for (name, value) in &sql.parameters {
            query = match value {
                SqlValue::String(s) => query.param(name, s),
                SqlValue::Int64(i) => query.param(name, i),
                SqlValue::Float64(f) => query.param(name, f),
                SqlValue::StringArray(xs) => query.param(name, xs),
                SqlValue::Int64Array(xs) => query.param(name, xs),
            };
        }
        let fetched = async { query.fetch_bytes(format)?.collect().await };
        let wait = self.limit_seconds + 1;
        match tokio::time::timeout(Duration::from_secs(wait), fetched).await {
            Ok(Ok(body)) => Ok(body.to_vec()),
            Ok(Err(clickhouse::error::Error::BadResponse(message))) => Err(message),
            Ok(Err(e)) => Err(e.to_string()),
            Err(_) => Err(format!("ClickHouse did not answer in {wait} seconds")),
        }
    }
}

fn lines(body: &[u8]) -> impl Iterator<Item = &[u8]> {
    body.split(|b| *b == b'\n').filter(|line| !line.is_empty())
}

/// The engine's rows: N group columns, then `bucket_ms` and `value`, and `series_id` when the
/// plan fills gaps. The column names say which.
impl Executor for ClickHouse {
    async fn rows(&self, sql: Sql) -> Result<Vec<Row>, String> {
        let body = self.fetch(&sql, "JSONCompactEachRowWithNames").await?;
        let mut lines = lines(&body);
        let Some(names) = lines.next() else { return Ok(Vec::new()) };
        let names: Vec<String> = serde_json::from_slice(names).map_err(|e| e.to_string())?;
        let per_series = names.last().is_some_and(|n| n == "series_id");
        let groups = names.len() - if per_series { 3 } else { 2 };

        lines
            .map(|line| {
                let values: Vec<Value> = serde_json::from_slice(line).map_err(|e| e.to_string())?;
                Ok(Row {
                    groups: values[..groups].iter().map(|v| v.as_str().map(str::to_string)).collect(),
                    series_id: if per_series { values[groups + 2].as_u64() } else { None },
                    bucket_ms: values[groups].as_i64().ok_or("bucket_ms is not an Int64")?,
                    // NaN and infinities arrive as null.
                    value: values[groups + 1].as_f64().unwrap_or(f64::NAN),
                })
            })
            .collect()
    }
}
