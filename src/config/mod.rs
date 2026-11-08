//! Configuration types and validation.
//!
//! All capture limits are mandatory finite values. This makes unlimited
//! capture impossible to represent in the public configuration model

use std::{collections::BTreeSet, error::Error, fmt};

const KIB: usize = 1024; // 1 KiB = 1024 bytes
const MIB: usize = 1024 * KIB; // 1 MiB = 1024 KiB = 1,048,576 bytes
const MIN_MARKER_BYTES: usize = 11;

/// Identifies the service that produces telemetry events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    pub name: String,
    pub environment: String,
    pub version: String,
    pub release: Option<String>,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            name: "unknown-service".to_string(),
            environment: "development".to_string(),
            version: "0.0.0".to_string(),
            release: None,
        }
    }
}

/// Hard limits applied before telemetry data is queued or exported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureLimits {
    pub max_request_body_bytes: usize,
    pub max_response_body_bytes: usize,
    pub max_header_value_bytes: usize,
    pub max_total_header_bytes: usize,
    pub max_query_parameters: usize,
    pub max_json_depth: usize,
    pub max_json_nodes: usize,
    pub max_string_bytes: usize,
    pub max_event_bytes: usize,
    pub max_loki_batch_bytes: usize,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            max_request_body_bytes: 64 * KIB,
            max_response_body_bytes: 64 * KIB,
            max_header_value_bytes: 8 * KIB,
            max_total_header_bytes: 32 * KIB,
            max_query_parameters: 128,
            max_json_depth: 16,
            max_json_nodes: 4_096,
            max_string_bytes: 16 * KIB,
            max_event_bytes: 256 * KIB,
            max_loki_batch_bytes: 4 * MIB,
        }
    }
}

impl CaptureLimits {
    /// Validates that every limit is finite and internally consistent. Returns an error if any limit is invalid.
    pub fn validate(&self) -> Result<(), ConfigError> {
        let limits = [
            ("max_request_body_bytes", self.max_request_body_bytes),
            ("max_response_body_bytes", self.max_response_body_bytes),
            ("max_header_value_bytes", self.max_header_value_bytes),
            ("max_total_header_bytes", self.max_total_header_bytes),
            ("max_query_parameters", self.max_query_parameters),
            ("max_json_depth", self.max_json_depth),
            ("max_json_nodes", self.max_json_nodes),
            ("max_string_bytes", self.max_string_bytes),
            ("max_event_bytes", self.max_event_bytes),
            ("max_loki_batch_bytes", self.max_loki_batch_bytes),
        ];

        for (name, value) in limits {
            if value == 0 {
                return Err(ConfigError::ZeroLimit(name));
            }
        }

        for (name, value) in [
            ("max_header_value_bytes", self.max_header_value_bytes),
            ("max_string_bytes", self.max_string_bytes),
        ] {
            if value < MIN_MARKER_BYTES {
                return Err(ConfigError::LimitBelowMinimum {
                    field: name,
                    minimum: MIN_MARKER_BYTES,
                });
            }
        }

        if self.max_event_bytes > self.max_loki_batch_bytes {
            return Err(ConfigError::EventLimitExceedsBatchLimit {
                max_event_bytes: self.max_event_bytes,
                max_loki_batch_bytes: self.max_loki_batch_bytes,
            });
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedactionConfig {
    /// Field names are matched case-insensitively after separator
    /// normalization by the redaction engine.
    pub sensitive_fields: BTreeSet<String>,
}

impl Default for RedactionConfig {
    fn default() -> Self {
        let fields = [
            "password",
            "passwd",
            "pwd",
            "token",
            "access_token",
            "refresh_token",
            "id_token",
            "authorization",
            "proxy-authorization",
            "api_key",
            "apikey",
            "secret",
            "client_secret",
            "cookie",
            "set-cookie",
            "session_id",
            "private_key",
            "signing_secret",
            "card_number",
            "security_code",
            "cvv",
            "cvc",
        ];

        Self {
            sensitive_fields: fields.into_iter().map(str::to_owned).collect(),
        }
    }
}

impl RedactionConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.sensitive_fields.iter().any(|field| {
            field.trim().is_empty() || !field.chars().any(|character| character.is_alphanumeric())
        }) {
            return Err(ConfigError::EmptySensitiveField);
        }

        Ok(())
    }
}
/// Phase-one configuration shared by capture, policies, and redaction.

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TelemetryConfig {
    pub service: ServiceConfig,
    pub limits: CaptureLimits,
    pub redaction: RedactionConfig,
}

impl TelemetryConfig {
    /// Validates the configuration befaure any telemetry component starts.
    pub fn validate(&self) -> Result<(), ConfigError> {
        validate_non_empty("name", &self.service.name)?;
        validate_non_empty("environment", &self.service.environment)?;
        validate_non_empty("version", &self.service.version)?;

        if let Some(region) = &self.service.release {
            validate_non_empty("release", region)?;
        }

        self.limits.validate()?;

        self.redaction.validate()?;
        Ok(())
    }
}

fn validate_non_empty(field: &'static str, value: &str) -> Result<(), ConfigError> {
    if value.trim().is_empty() {
        Err(ConfigError::EmptyServiceField(field))
    } else {
        Ok(())
    }
}

/// A configuration error that must be resolved during startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConfigError {
    EmptyServiceField(&'static str),
    ZeroLimit(&'static str),
    EventLimitExceedsBatchLimit {
        max_event_bytes: usize,
        max_loki_batch_bytes: usize,
    },
    EmptySensitiveField,
    LimitBelowMinimum {
        field: &'static str,
        minimum: usize,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyServiceField(field) => {
                write!(formatter, "service field `{field}` must not be empty")
            }
            Self::ZeroLimit(field) => {
                write!(
                    formatter,
                    "capture limit `{field}` must be greater than zero"
                )
            }
            Self::EventLimitExceedsBatchLimit {
                max_event_bytes,
                max_loki_batch_bytes,
            } => write!(
                formatter,
                "max_event_bytes ({max_event_bytes}) must not exceed \
                max_loki_batch_bytes ({max_loki_batch_bytes})"
            ),
            Self::EmptySensitiveField => {
                formatter.write_str("sensitive field names must not be empty")
            }
            Self::LimitBelowMinimum { field, minimum } => write!(
                formatter,
                "capture limit `{field}` must be at least {minimum} bytes"
            ),
        }
    }
}

impl Error for ConfigError {}
