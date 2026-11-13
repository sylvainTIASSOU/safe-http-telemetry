#![forbid(unsafe_code)]

//! Safe, bounded HTTP telemetry for Axum, `tracing`, OpenTelemetry and Loki.
//!
//! The Tower middleware provides bounded HTTP lifecycle events, including
//! ordinary responses, streaming, errors, timeouts and cancellation. The
//! optional Axum adapter adds normalized matched-route integration.

#[cfg(feature = "axum")]
pub mod axum_adapter;
#[cfg(feature = "body-capture")]
pub mod body_capture;

pub mod config;
pub mod core;
#[cfg(feature = "geolocation")]
pub mod geolocation;
#[cfg(feature = "loki")]
pub mod loki;

pub mod policy;
pub mod redaction;
pub mod runtime;
#[cfg(feature = "tower")]
pub mod tower_integration;

pub mod tracing_integration;
