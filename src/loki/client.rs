//! Asynchronous HTTP client for the Grafana Loki push API.
//!
//! One call to [`LokiClient::push`] performs exactly one HTTP request. Retry
//! orchestration belongs to the future bounded export workers.

use std::{error::Error, fmt};

use crate::loki::{
    config::{LokiAuth, LokiConfig, LokiConfigError},
    payload::LokiPushRequest,
};
use reqwest::{header::CONTENT_TYPE, redirect::Policy, Client};

/// A reusable Loki HTTP client.
///
/// This type does not implement `Debug`, because its configuration can contain
/// authentication material. Clones share the underlying HTTP connection pool.
#[derive(Clone)]
pub struct LokiClient {
    client: Client,
    config: LokiConfig,
}

impl LokiClient {
    /// Validate the configuration and build a reusable asynchronous client
    pub fn new(config: LokiConfig) -> Result<Self, LokiClientError> {
        config
            .validate()
            .map_err(LokiClientError::InvalidConfiguration)?;

        let client = Client::builder()
            .use_rustls_tls()
            .redirect(Policy::none())
            .timeout(config.request_timeout)
            .build()
            .map_err(|_| LokiClientError::ClientBuild)?;

        Ok(Self { client, config })
    }

    /// Serializes and pushes one payload with exactly one HTTP attempt.
    ///
    /// Error response bodies are deliberately not read or included in
    /// diagnostics. Retry decisions are exposed through
    /// [`LokiClientError::is_retryable`].
    pub async fn push(&self, payload: &LokiPushRequest) -> Result<(), LokiClientError> {
        let body = payload
            .to_json_bytes()
            .map_err(|_| LokiClientError::PayloadSerialization)?;

        let mut request = self
            .client
            .post(self.config.push_url.clone())
            .header(CONTENT_TYPE, "application/json")
            .body(body);

        if let Some(tenant_id) = &self.config.tenant_id {
            request = request.header("X-Scope-OrgID", tenant_id);
        }

        if let Some(LokiAuth::Bearer(token)) = &self.config.auth {
            request = request.bearer_auth(token.expose());
        }

        let response = request.send().await.map_err(|error| {
            if error.is_timeout() {
                LokiClientError::Timeout
            } else {
                LokiClientError::Transport
            }
        })?;

        let status = response.status();
        if status.is_success() {
            Ok(())
        } else if status.as_u16() == 429 || status.is_server_error() {
            Err(LokiClientError::RetryableStatus(status.as_u16()))
        } else {
            Err(LokiClientError::PermanentStatus(status.as_u16()))
        }
    }
}

/// A Loki client failure with no payload or credential data attached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LokiClientError {
    InvalidConfiguration(LokiConfigError),
    ClientBuild,
    PayloadSerialization,
    Timeout,
    Transport,
    RetryableStatus(u16),
    PermanentStatus(u16),
}

impl LokiClientError {
    /// Whether a future bounded worker may retry the operation.
    pub const fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Timeout | Self::Transport | Self::RetryableStatus(_)
        )
    }

    /// Returns only the numeric HTTP status, when a response was received.
    pub const fn status_code(&self) -> Option<u16> {
        match self {
            Self::RetryableStatus(status) | Self::PermanentStatus(status) => Some(*status),
            _ => None,
        }
    }
}

impl fmt::Display for LokiClientError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(error) => {
                write!(formatter, "invalid Loki client configuration: {error}")
            }
            Self::ClientBuild => formatter.write_str("failed to build the Loki HTTP client"),
            Self::PayloadSerialization => {
                formatter.write_str("failed to serialize the Loki push payload")
            }
            Self::Timeout => formatter.write_str("the Loki HTTP request timed out"),
            Self::Transport => formatter.write_str("the Loki HTTP transport failed"),
            Self::RetryableStatus(status) => {
                write!(formatter, "Loki returned retryable HTTP status {status}")
            }
            Self::PermanentStatus(status) => {
                write!(formatter, "Loki returned permanent HTTP status {status}")
            }
        }
    }
}

impl Error for LokiClientError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::InvalidConfiguration(error) => Some(error),
            _ => None,
        }
    }
}
