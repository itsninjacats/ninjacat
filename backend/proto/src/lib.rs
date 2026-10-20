//! The protobuf messages the Datadog Agent sends, generated from Datadog's own `.proto` files
//! into `generated/` by `cargo run -p protogen`. No hand-written code belongs here.

// Generated code: its lints are prost's to fix, not ours.
#![allow(clippy::all)]

include!("generated/mod.rs");
