//! Grafana Loki configuration and JSON push payloads.
//!
//! This module contains no runtime or network worker yet. The asynchronous
//! client is added after the bounded queue.

pub mod config;
pub mod payload;
