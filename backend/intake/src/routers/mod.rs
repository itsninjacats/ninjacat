//! The handlers, one module per product. Which host serves which handler is in each program's
//! `main` (`api/src/bin`); hosts that serve the same paths (app. and api.) share the handlers.

pub mod api;
pub mod api_payloads;
pub mod app;
pub mod ci_visibility;
pub mod ci_webhook;
pub mod config;
pub mod containers;
pub mod dbm;
pub mod evp;
pub mod flare;
pub mod install;
pub mod kubeops;
pub mod logs;
pub mod misc;
pub mod ndm;
pub mod pprof;
pub mod process;
pub mod process_buffers;
pub mod profiling;
pub mod rum;
pub mod security;
pub mod trace;
pub mod trace_sketch;
pub mod usm;
