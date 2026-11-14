//! Configuration for the Tower HTTP lifecycle middleware.

use ipnet::IpNet;

use crate::config::{CaptureLimits, ConfigError, RedactionConfig};

#[cfg(feature = "geolocation")]
use crate::geolocation::GeolocationConfig;

/// Controls client-IP extraction from the direct peer and trusted proxies.
#[derive(Clone, Debug)]
pub struct ClientIpConfig {
    /// Networks whose forwarding headers may be trusted.
    ///
    /// An empty list means that `Forwarded` and `X-Forwarded-For` are always
    /// ignored. Only the immediate peer is trusted by default.
    pub trusted_proxies: Vec<IpNet>,

    /// Maximum combined size of one forwarding-header family.
    pub max_forwarded_header_bytes: usize,

    /// Maximum number of entries accepted in a forwarding chain.
    pub max_forwarded_hops: usize,
}

impl ClientIpConfig {
    pub(crate) fn validate(&self) -> Result<(), ConfigError> {
        if self.max_forwarded_header_bytes == 0 {
            return Err(ConfigError::ZeroLimit("max_forwarded_header_bytes"));
        }
        if self.max_forwarded_hops == 0 {
            return Err(ConfigError::ZeroLimit("max_forwarded_hops"));
        }

        Ok(())
    }
}

impl Default for ClientIpConfig {
    fn default() -> Self {
        Self {
            trusted_proxies: Vec::new(),
            max_forwarded_header_bytes: 4 * 1024,
            max_forwarded_hops: 16,
        }
    }
}

/// Configuration for minimal HTTP lifecycle observation.
#[derive(Clone, Debug)]
pub struct MiddlewareConfig {
    /// Bounds applied before HTTP data may enter the event queue.
    pub limits: CaptureLimits,

    /// Field names whose query, header and body values must be redacted.
    pub redaction: RedactionConfig,

    /// Whether a valid incoming `x-request-id` may be reused.
    ///
    /// Disabled by default because request IDs are client-controlled input.
    pub trust_incoming_request_id: bool,

    /// Emits a terminal event when a request future or response body is
    /// dropped before normal completion.
    pub emit_cancellation_events: bool,

    /// Captures the resolved client IP in Standard and Detailed profiles.
    /// Geolocation also enables this resolution when it is configured.
    pub capture_client_ip: bool,

    /// Trusted-proxy policy used for client-IP resolution.
    pub client_ip: ClientIpConfig,

    /// Optional geolocation configuration. The explicit local MMDB is loaded
    /// once when the layer is built, never during a request.
    #[cfg(feature = "geolocation")]
    pub geolocation: Option<GeolocationConfig>,
}

impl Default for MiddlewareConfig {
    fn default() -> Self {
        Self {
            limits: CaptureLimits::default(),
            redaction: RedactionConfig::default(),
            trust_incoming_request_id: false,
            emit_cancellation_events: true,
            capture_client_ip: false,
            client_ip: ClientIpConfig::default(),
            #[cfg(feature = "geolocation")]
            geolocation: None,
        }
    }
}
