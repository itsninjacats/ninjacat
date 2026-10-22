use serde::Serialize;

/// A row of one ClickHouse table. Its fields are the columns it inserts, named as the columns
/// are; the rest of the table's columns are left to their defaults.
pub trait Table: clickhouse::RowOwned + clickhouse::RowWrite + Serialize + Send + Sync {
    const TABLE: &'static str;
}
