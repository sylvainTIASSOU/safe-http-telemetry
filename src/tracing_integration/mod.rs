//! Native `tracing` and OpenTelemetry correlation.
//!
//! W3C context extraction and injection operate on HTTP headers without
//! copying their values into errors or diagnostics. Business-log export is
//! integrated separately from the HTTP lifecycle event model.

#[cfg(feature = "opentelemetry")]
pub mod propagation;

#[cfg(feature = "tracing")]
pub mod business;
