//! What every intake host shares, and each host's router. A program in `api/src/bin` serves one
//! host: it names the host's router and calls
//! [`server::run`], so each host can be deployed, scaled and stopped on its own.
//!
//! A handler turns the wire format into the plain rows of `ninjapaws_storage` and hands them to
//! the sink; the sender gets its answer at once. What cannot be decoded is kept in raw_payloads.

pub mod auth;
pub mod body;
pub mod ddsketch;
pub mod fields;
pub mod json;
pub mod keys;
pub mod msgpack;
pub mod process_frame;
pub mod raw;
pub mod routers;
pub mod self_metrics;
pub mod server;
pub mod tcp_logs;
pub mod wire;
