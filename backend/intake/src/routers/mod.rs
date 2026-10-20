//! One module per host's router. A module's `mount` registers what its host serves; the hosts
//! that share a router (app. and api. share the core agent's) share its handlers.

pub mod api;
pub mod api_payloads;
pub mod app;
pub mod config;
pub mod containers;
pub mod dbm;
pub mod evp;
pub mod flare;
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
