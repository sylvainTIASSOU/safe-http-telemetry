//! Low-cardinality internal telemetry counters.
//!
//! These counters contain no request values, credentials, identifiers or
//! geographic information. Cloned handles share the same atomic counters.

use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// A cheap, cloneable handle to internal telemetry counters.
#[derive(Clone, Debug, Default)]
pub struct InternalTelemetry {
    counters: Arc<Counters>,
}

impl InternalTelemetry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_redactions(&self, count: u64) {
        self.counters
            .redacted_values
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_truncations(&self, count: u64) {
        self.counters
            .truncated_values
            .fetch_add(count, Ordering::Relaxed);
    }

    pub fn record_body_omission(&self) {
        self.counters.omitted_bodies.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_parse_error(&self) {
        self.counters.parse_errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_policy_denial(&self) {
        self.counters
            .rejected_by_policy
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Returns a consistent-enough operational snapshot.
    ///
    /// Relaxed atomics are intentional: these counters are observability
    /// signals, not synchronization primitives or exact financial balances.
    pub fn snapshot(&self) -> InternalTelemetrySnapshot {
        InternalTelemetrySnapshot {
            redacted_values: self.counters.redacted_values.load(Ordering::Relaxed),
            truncated_values: self.counters.truncated_values.load(Ordering::Relaxed),
            omitted_bodies: self.counters.omitted_bodies.load(Ordering::Relaxed),
            parse_errors: self.counters.parse_errors.load(Ordering::Relaxed),
            rejected_by_policy: self.counters.rejected_by_policy.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Default)]
struct Counters {
    redacted_values: AtomicU64,
    truncated_values: AtomicU64,
    omitted_bodies: AtomicU64,
    parse_errors: AtomicU64,
    rejected_by_policy: AtomicU64,
}

/// An immutable snapshot suitable for diagnostics or metrics export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InternalTelemetrySnapshot {
    pub redacted_values: u64,
    pub truncated_values: u64,
    pub omitted_bodies: u64,
    pub parse_errors: u64,
    pub rejected_by_policy: u64,
}
