//! Configuration for the Tower HTTP lifecycle middleware.

use crate::config::CaptureLimits;

/// Configuration for minimal HTTP lifecycle observation.
#[derive(Clone, Debug, Default)]
pub struct MiddlewareConfig {
    /// Bounds applied before HTTP data may enter the event queue.
    pub limits: CaptureLimits,

    /// Whether a valid incoming `x-request-id` may be reused.
    ///
    /// Disabled by default because request IDs are client-controlled input.
    pub trust_incoming_request_id: bool,
}

