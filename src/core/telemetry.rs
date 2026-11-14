//! Low-cardinality internal telemetry counters.
//!
//! These counters contain no request values, credentials, identifiers or
//! geographic information. Cloned handles share the same atomic counters.

use super::event::{BodyCaptureStatus, CaptureInfo};
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

    pub fn record_request_started(&self) {
        self.counters
            .observed_requests
            .fetch_add(1, Ordering::Relaxed);
        self.counters
            .active_requests
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_request_excluded(&self) {
        self.counters
            .excluded_requests
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_request_completed(&self) {
        self.counters
            .completed_requests
            .fetch_add(1, Ordering::Relaxed);
        self.decrement_active_requests();
    }

    pub fn record_request_failed(&self) {
        self.counters
            .failed_requests
            .fetch_add(1, Ordering::Relaxed);
        self.decrement_active_requests();
    }

    pub fn record_request_timed_out(&self) {
        self.counters
            .timed_out_requests
            .fetch_add(1, Ordering::Relaxed);
        self.decrement_active_requests();
    }

    pub fn record_request_cancelled(&self) {
        self.counters
            .cancelled_requests
            .fetch_add(1, Ordering::Relaxed);
        self.decrement_active_requests();
    }

    fn decrement_active_requests(&self) {
        let mut current = self.counters.active_requests.load(Ordering::Relaxed);

        loop {
            let updated = current.saturating_sub(1);

            match self.counters.active_requests.compare_exchange_weak(
                current,
                updated,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
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

    /// Records terminal body-capture outcomes without retaining HTTP data.
    ///
    /// Body truncation counters are distinct from `truncated_values`, which
    /// also includes metadata and structural redaction truncation.
    pub fn record_body_capture_outcomes(&self, capture: &CaptureInfo) {
        match capture.request_body_status {
            Some(BodyCaptureStatus::Captured) => {
                self.counters
                    .captured_request_bodies
                    .fetch_add(1, Ordering::Relaxed);
            }
            Some(BodyCaptureStatus::Unsupported) => {
                self.counters
                    .unsupported_bodies
                    .fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        match capture.response_body_status {
            Some(BodyCaptureStatus::Captured) => {
                self.counters
                    .captured_response_bodies
                    .fetch_add(1, Ordering::Relaxed);
            }
            Some(BodyCaptureStatus::Unsupported) => {
                self.counters
                    .unsupported_bodies
                    .fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }

        if capture.request_body_truncated {
            self.counters
                .truncated_request_bodies
                .fetch_add(1, Ordering::Relaxed);
        }
        if capture.response_body_truncated {
            self.counters
                .truncated_response_bodies
                .fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn record_geolocation_cache_hit(&self) {
        self.counters
            .geolocation_cache_hits
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_geolocation_cache_miss(&self) {
        self.counters
            .geolocation_cache_misses
            .fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_geolocation_error(&self) {
        self.counters
            .geolocation_errors
            .fetch_add(1, Ordering::Relaxed);
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

    pub fn record_event_dequeued(&self) {
        self.counters
            .pending_export_events
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Records that an export operation has started.
    /// Increments the in-flight batch gauge.
    pub fn record_export_started(&self) {
        self.counters
            .in_flight_batches
            .fetch_add(1, Ordering::Relaxed);
    }

    /// Records a successful or failed export operation with its duration.
    /// Decrements the in-flight batch gauge accumulates duration and count.
    pub fn record_export_completed(&self, duration: std::time::Duration) {
        let millis = duration.as_millis() as u64;
        self.counters
            .export_duration_ms_total
            .fetch_add(millis, Ordering::Relaxed);
        self.counters
            .export_operations
            .fetch_add(1, Ordering::Relaxed);
        self.counters
            .in_flight_batches
            .fetch_sub(1, Ordering::Relaxed);
    }

    pub fn record_event_processing_failure(&self) {
        self.record_event_dropped();
        self.complete_pending_events(1);
    }

    pub fn record_oversized_event(&self) {
        self.counters
            .oversized_events
            .fetch_add(1, Ordering::Relaxed);
        self.record_event_dropped();
        self.complete_pending_events(1);
    }

    pub fn record_exported_batch(&self, event_count: usize) {
        self.counters
            .exported_batches
            .fetch_add(1, Ordering::Relaxed);
        self.counters
            .exported_events
            .fetch_add(event_count as u64, Ordering::Relaxed);
        self.complete_pending_events(event_count);
    }

    pub fn record_failed_batch(&self, event_count: usize) {
        self.counters.failed_batches.fetch_add(1, Ordering::Relaxed);
        self.counters
            .dropped_events
            .fetch_add(event_count as u64, Ordering::Relaxed);
        self.complete_pending_events(event_count);
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

    pub fn record_shutdown(&self, timed_out: bool, abandoned_events: usize) {
        self.counters.shutdowns.fetch_add(1, Ordering::Relaxed);
        if timed_out {
            self.counters.drain_timeouts.fetch_add(1, Ordering::Relaxed);
        }

        let abandoned_events = abandoned_events as u64;
        self.counters
            .shutdown_abandoned_events
            .fetch_add(abandoned_events, Ordering::Relaxed);
        self.counters
            .dropped_events
            .fetch_add(abandoned_events, Ordering::Relaxed);
        self.counters
            .pending_export_events
            .store(0, Ordering::Relaxed);
    }

    fn complete_pending_events(&self, event_count: usize) {
        let event_count = event_count as u64;
        let mut current = self.counters.pending_export_events.load(Ordering::Relaxed);

        loop {
            let updated = current.saturating_sub(event_count);

            match self.counters.pending_export_events.compare_exchange_weak(
                current,
                updated,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current = actual,
            }
        }
    }

    /// Returns a consistent-enough operational snapshot.
    ///
    /// Relaxed atomics are intentional: these counters are observability
    /// signals, not synchronization primitives or exact financial balances.
    pub fn snapshot(&self) -> InternalTelemetrySnapshot {
        InternalTelemetrySnapshot {
            observed_requests: self.counters.observed_requests.load(Ordering::Relaxed),
            excluded_requests: self.counters.excluded_requests.load(Ordering::Relaxed),
            completed_requests: self.counters.completed_requests.load(Ordering::Relaxed),
            redacted_values: self.counters.redacted_values.load(Ordering::Relaxed),
            truncated_values: self.counters.truncated_values.load(Ordering::Relaxed),
            omitted_bodies: self.counters.omitted_bodies.load(Ordering::Relaxed),
            parse_errors: self.counters.parse_errors.load(Ordering::Relaxed),
            captured_request_bodies: self
                .counters
                .captured_request_bodies
                .load(Ordering::Relaxed),
            captured_response_bodies: self
                .counters
                .captured_response_bodies
                .load(Ordering::Relaxed),
            unsupported_bodies: self.counters.unsupported_bodies.load(Ordering::Relaxed),
            truncated_request_bodies: self
                .counters
                .truncated_request_bodies
                .load(Ordering::Relaxed),
            truncated_response_bodies: self
                .counters
                .truncated_response_bodies
                .load(Ordering::Relaxed),
            geolocation_cache_hits: self.counters.geolocation_cache_hits.load(Ordering::Relaxed),
            geolocation_cache_misses: self
                .counters
                .geolocation_cache_misses
                .load(Ordering::Relaxed),
            geolocation_errors: self.counters.geolocation_errors.load(Ordering::Relaxed),
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
            pending_export_events: self.counters.pending_export_events.load(Ordering::Relaxed),
            shutdowns: self.counters.shutdowns.load(Ordering::Relaxed),
            drain_timeouts: self.counters.drain_timeouts.load(Ordering::Relaxed),
            shutdown_abandoned_events: self
                .counters
                .shutdown_abandoned_events
                .load(Ordering::Relaxed),

            failed_requests: self.counters.failed_requests.load(Ordering::Relaxed),
            timed_out_requests: self.counters.timed_out_requests.load(Ordering::Relaxed),
            active_requests: self.counters.active_requests.load(Ordering::Relaxed),
            cancelled_requests: self.counters.cancelled_requests.load(Ordering::Relaxed),
            export_duration_ms_total: self
                .counters
                .export_duration_ms_total
                .load(Ordering::Relaxed),
            export_operations: self.counters.export_operations.load(Ordering::Relaxed),
            in_flight_batches: self.counters.in_flight_batches.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Default)]
struct Counters {
    observed_requests: AtomicU64,
    excluded_requests: AtomicU64,
    completed_requests: AtomicU64,
    redacted_values: AtomicU64,
    truncated_values: AtomicU64,
    omitted_bodies: AtomicU64,
    parse_errors: AtomicU64,
    captured_request_bodies: AtomicU64,
    captured_response_bodies: AtomicU64,
    unsupported_bodies: AtomicU64,
    truncated_request_bodies: AtomicU64,
    truncated_response_bodies: AtomicU64,
    geolocation_cache_hits: AtomicU64,
    geolocation_cache_misses: AtomicU64,
    geolocation_errors: AtomicU64,
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
    pending_export_events: AtomicU64,
    shutdowns: AtomicU64,
    drain_timeouts: AtomicU64,
    shutdown_abandoned_events: AtomicU64,
    failed_requests: AtomicU64,
    timed_out_requests: AtomicU64,
    active_requests: AtomicU64,
    cancelled_requests: AtomicU64,
    export_duration_ms_total: AtomicU64,
    export_operations: AtomicU64,
    in_flight_batches: AtomicU64,
}

/// An immutable snapshot suitable for diagnostics or metrics export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InternalTelemetrySnapshot {
    pub observed_requests: u64,
    pub excluded_requests: u64,
    pub completed_requests: u64,
    pub redacted_values: u64,
    pub truncated_values: u64,
    pub omitted_bodies: u64,
    pub parse_errors: u64,
    pub captured_request_bodies: u64,
    pub captured_response_bodies: u64,
    pub unsupported_bodies: u64,
    pub truncated_request_bodies: u64,
    pub truncated_response_bodies: u64,
    pub geolocation_cache_hits: u64,
    pub geolocation_cache_misses: u64,
    pub geolocation_errors: u64,
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
    pub pending_export_events: u64,
    pub shutdowns: u64,
    pub drain_timeouts: u64,
    pub shutdown_abandoned_events: u64,
    pub failed_requests: u64,
    pub timed_out_requests: u64,
    pub active_requests: u64,
    pub cancelled_requests: u64,
    pub export_duration_ms_total: u64,
    pub export_operations: u64,
    pub in_flight_batches: u64,
}
