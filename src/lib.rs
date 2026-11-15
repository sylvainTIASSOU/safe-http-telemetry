#![forbid(unsafe_code)]
#![doc = include_str!("../docs/rustdoc.md")]

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
