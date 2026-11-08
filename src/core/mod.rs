//! Framework-independent telemetry primitives.
//!
//! This module contains the versioned HTTP event model and internal counters.
//! It performs no network operations and has no dependency on Axum or Tower.

pub mod event;
pub mod telemetry;
