//! The query language and what the panel asks for, compiled to SQL; ported from the F# engine
//! (`api/src/Engine`). Nothing here knows HTTP or the ClickHouse driver: a query becomes [`Sql`]
//! (text with `{name:Type}` placeholders and the values bound to them), and the caller runs it.
//!
//! Instants are `i64` milliseconds since the epoch; spans are [`chrono::TimeDelta`].

pub mod api;
pub mod duration;
pub mod json;
pub mod metric_query;
pub mod metrics;
pub mod monitor;
pub mod panel;
pub mod query;
pub mod sql;
pub mod unix_time;

pub use sql::{Sql, SqlParams, SqlValue, TenantId};
