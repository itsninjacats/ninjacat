//! What every intake host shares, and the handlers. A program in `api/src/bin` serves one host: it
//! lists the host's routes and serves them, so each host can be deployed, scaled and stopped on
//! its own.
//!
//! A handler turns the wire format into the plain rows of `ninjapaws_storage` and inserts them,
//! one insert per table; the sender gets its answer once they are stored. What cannot be decoded
//! is kept in raw_payloads.

pub mod auth;
pub mod body;
pub mod capture;
pub mod ddsketch;
pub mod fields;
pub mod json;
pub mod keys;
pub mod msgpack;
pub mod process_frame;
pub mod raw;
pub mod routers;
pub mod server;
pub mod tcp_logs;
pub mod wire;
