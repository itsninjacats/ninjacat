//! The plain records handlers turn a wire format into, one module per kind of telemetry, each with
//! the table its rows go to.

pub mod agent;
pub mod checks;
pub mod events;
pub mod flares;
pub mod hosts;
pub mod metrics;
pub mod profiling;
pub mod raw;
pub mod runner;

use std::collections::BTreeMap;

/// Datadog's tags as a multiset: key → every value seen, in arrival order.
pub type Tags = BTreeMap<String, Vec<String>>;

/// Keys a payload had and no column holds, each value JSON-encoded.
pub type Extra = BTreeMap<String, String>;
