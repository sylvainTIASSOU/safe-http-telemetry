//! Configuration for the Tower HTTP lifecycle middleware.

use crate::config::CaptureLimits;

/// Configuration for minimal HTTP lifecycle observation.
#[derive(Clone, Debug)]
pub struct MiddlewareConfig {
    /// Bounds applied before HTTP data may enter the event queue.
    pub limits: CaptureLimits,

    /// Whether a valid incoming `x-request-id` may be reused.
    ///
    /// Disabled by default because request IDs are client-controlled input.
    pub trust_incoming_request_id: bool,

    /// Emits a terminal event when a request future or response body is
    /// dropped before normal completion.
    pub emit_cancellation_events: bool,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            limits: CaptureLimits::default(),
            trust_incoming_request_id: false,
            emit_cancellation_events: true,
        }
    }
}
