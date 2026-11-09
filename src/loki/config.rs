//! Validated Loki connection, batching and retry configuration.
//!

use std::{error::Error, fmt, time::Duration};
use url::Url;

/// A Secret configuration value whose `Debug` output is always redacted.
#[derive(Clone)]
pub struct SecretValue(String);

impl SecretValue {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("\"[REDACTED]\"")
    }
}

/// Authentication applied by the future Loki HTTP client
#[derive(Clone, Debug)]
pub enum LokiAuth {
    Bearer(SecretValue),
}

/// Limits controlling when a batch must be flushed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchConfig {
    pub max_events: usize,
    pub max_bytes: usize,
    pub flush_interval: Duration,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            max_events: 100,
            max_bytes: 4 * 1024 * 1024,
            flush_interval: Duration::from_millis(500),
        }
    }
}

/// Bounded retry behavior for failed Loki requests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetryConfig {
    /// Number of retries after the initial request.
    ///  Zero disables retries without creating an unbounded loop.
    pub max_retries: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 2,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(2),
        }
    }
}

/// Complete configuration for one Loki push endpoint.
#[derive(Clone)]
pub struct LokiConfig {
    /// Full push URL, or example:
    /// `http://localhost:3100/loki/api/v1/push`.
    pub push_url: Url,
    pub tenant_id: Option<String>,
    pub auth: Option<LokiAuth>,
    pub batch: BatchConfig,
    pub request_timeout: Duration,
    pub retry: RetryConfig,
}

impl LokiConfig {
    pub fn new(push_url: impl AsRef<str>) -> Result<Self, LokiConfigError> {
        let push_url = Url::parse(push_url.as_ref()).map_err(|_| LokiConfigError::InvalidUrl)?;

        let config = Self {
            push_url,
            tenant_id: None,
            auth: None,
            batch: BatchConfig::default(),
            request_timeout: Duration::from_secs(5),
            retry: RetryConfig::default(),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn with_tenant_id(mut self, tenant_id: impl Into<String>) -> Self {
        self.tenant_id = Some(tenant_id.into());
        self
    }

    pub fn with_bearer_token(mut self, token: impl Into<String>) -> Self {
        self.auth = Some(LokiAuth::Bearer(SecretValue::new(token)));
        self
    }

    /// Revalidates all public fields before constructing network components.
    pub fn validate(&self) -> Result<(), LokiConfigError> {
        if !matches!(self.push_url.scheme(), "http" | "https") {
            return Err(LokiConfigError::UnsupportedScheme);
        }

        if self.push_url.host_str().is_none() {
            return Err(LokiConfigError::MissingHost);
        }

        if self.push_url.username().is_empty() || self.push_url.password().is_some() {
            return Err(LokiConfigError::UrlContainsCredentials);
        }

        if self.push_url.query().is_some() || self.push_url.fragment().is_some() {
            return Err(LokiConfigError::UrlContainsQueryOrFragment);
        }

        if let Some(tenant_id) = &self.tenant_id {
            if tenant_id.trim().is_empty()
                || tenant_id.chars().any(|character| character.is_control())
            {
                return Err(LokiConfigError::InvalidTenantId);
            }
        }

        if let Some(LokiAuth::Bearer(secret)) = &self.auth {
            if secret.0.trim().is_empty()
                || secret.0.chars().any(|character| character.is_control())
            {
                return Err(LokiConfigError::InvalidAuthentication);
            }
        }

        for (name, value) in [
            ("batch.max_events", self.batch.max_events),
            ("batch.max_bytes", self.batch.max_bytes),
        ] {
            if value == 0 {
                return Err(LokiConfigError::ZeroValue(name));
            }
        }

        for (name, value) in [
            ("request_timeout", self.request_timeout),
            ("batch.flush_interval", self.batch.flush_interval),
            ("retry.initial_backoff", self.retry.initial_backoff),
            ("retry.max_backoff", self.retry.max_backoff),
        ] {
            if value.is_zero() {
                return Err(LokiConfigError::ZeroValue(name));
            }
        }

        if self.retry.initial_backoff > self.retry.max_backoff {
            return Err(LokiConfigError::InvalidBackoffOrder);
        }

        Ok(())
    }
}

impl fmt::Debug for LokiConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LokiConfig")
            .field("push_url", &self.push_url)
            .field("tenant_id", &self.tenant_id)
            .field("auth", &self.auth)
            .field("batch", &self.batch)
            .field("request_timeout", &self.request_timeout)
            .field("retry", &self.retry)
            .finish()
    }
}

/// A Loki configuration error detected before network startup.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LokiConfigError {
    InvalidUrl,
    UnsupportedScheme,
    MissingHost,
    UrlContainsCredentials,
    UrlContainsQueryOrFragment,
    InvalidTenantId,
    InvalidAuthentication,
    ZeroValue(&'static str),
    InvalidBackoffOrder,
}

impl fmt::Display for LokiConfigError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidUrl => formatter.write_str("the Loki push URL is invalid"),
            Self::UnsupportedScheme => formatter.write_str("the Loki URL must use HTTP or HTTPS"),
            Self::MissingHost => formatter.write_str("the Loki URL must contain a host"),
            Self::UrlContainsCredentials => {
                formatter.write_str("credentials must not be embedded in the Loki URL")
            }
            Self::UrlContainsQueryOrFragment => {
                formatter.write_str("the Loki push URL must not contain a query or fragment")
            }
            Self::InvalidTenantId => formatter.write_str("the Loki tenant ID is invalid"),
            Self::InvalidAuthentication => {
                formatter.write_str("the Loki authentication value is invalid")
            }
            Self::ZeroValue(field) => {
                write!(
                    formatter,
                    "Loki setting `{field}` must be greater than zero"
                )
            }
            Self::InvalidBackoffOrder => {
                formatter.write_str("retry.initial_backoff must not exceed retry.max_backoff")
            }
        }
    }
}

impl Error for LokiConfigError {}
