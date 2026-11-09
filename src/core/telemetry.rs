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

    pub fn record_event_accepted(&self) {
        self.counters
            .accepted_events
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_event_rejected(&self) {
        self.counters
            .rejected_events
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_event_dropped(&self) {
        self.counters.dropped_events.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_oversized_event(&self) {
        self.counters
            .oversized_events
            .fetch_add(1, Ordering::Relaxed);
        self.record_event_dropped();
    }

    pub fn record_exported_batch(&self, event_count: usize) {
        self.counters
            .exported_batches
            .fetch_add(1, Ordering::Relaxed);
        self.counters
            .exported_events
            .fetch_add(event_count as u64, Ordering::Relaxed);
    }

    pub fn record_failed_batch(&self, event_count: usize) {
        self.counters.failed_batches.fetch_add(1, Ordering::Relaxed);
        self.counters
            .dropped_events
            .fetch_add(event_count as u64, Ordering::Relaxed);
    }

    pub fn record_export_retry(&self) {
        self.counters.export_retries.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_retryable_export_failure(&self) {
        self.counters
            .retryable_export_failures
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_permanent_export_failure(&self) {
        self.counters
            .permanent_export_failures
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
            accepted_events: self.counters.accepted_events.load(Ordering::Relaxed),
            rejected_events: self.counters.rejected_events.load(Ordering::Relaxed),
            dropped_events: self.counters.dropped_events.load(Ordering::Relaxed),
            oversized_events: self.counters.oversized_events.load(Ordering::Relaxed),
            exported_batches: self.counters.exported_batches.load(Ordering::Relaxed),
            exported_events: self.counters.exported_events.load(Ordering::Relaxed),
            failed_batches: self.counters.failed_batches.load(Ordering::Relaxed),
            export_retries: self.counters.export_retries.load(Ordering::Relaxed),
            retryable_export_failures: self
                .counters
                .retryable_export_failures
                .load(Ordering::Relaxed),
            permanent_export_failures: self
                .counters
                .permanent_export_failures
                .load(Ordering::Relaxed),
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
    accepted_events: AtomicU64,
    rejected_events: AtomicU64,
    dropped_events: AtomicU64,
    oversized_events: AtomicU64,
    exported_batches: AtomicU64,
    exported_events: AtomicU64,
    failed_batches: AtomicU64,
    export_retries: AtomicU64,
    retryable_export_failures: AtomicU64,
    permanent_export_failures: AtomicU64,
}

/// An immutable snapshot suitable for diagnostics or metrics export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InternalTelemetrySnapshot {
    pub redacted_values: u64,
    pub truncated_values: u64,
    pub omitted_bodies: u64,
    pub parse_errors: u64,
    pub rejected_by_policy: u64,
    pub accepted_events: u64,
    pub rejected_events: u64,
    pub dropped_events: u64,
    pub oversized_events: u64,
    pub exported_batches: u64,
    pub exported_events: u64,
    pub failed_batches: u64,
    pub export_retries: u64,
    pub retryable_export_failures: u64,
    pub permanent_export_failures: u64,
}
