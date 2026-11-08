#![forbid(unsafe_code)]

//! Safe, bounded HTTP telemetry for Axum, `tracing`, OpenTelemetry and Loki.
//!
//! The crate is currently being scaffolded. No production functionality is
//! implemented yet.

pub mod axum_adapter;
pub mod config;
pub mod core;
pub mod geolocation;
pub mod loki;
pub mod policy;
pub mod redaction;
pub mod runtime;
pub mod tower_integration;
pub mod tracing_integration;
