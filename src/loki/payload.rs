//! Grafana Loki JSON push payload construction.

use std::{collections::BTreeMap, error::Error, fmt};

use serde::{Deserialize, Serialize};

use crate::{config::ServiceConfig, core::event::HttpEvent};

/// A Loki push request containing one or more streams.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LokiPushRequest {
    pub streams: Vec<LokiStream>,
}

impl LokiPushRequest {
    pub fn new(streams: Vec<LokiStream>) -> Self {
        Self { streams }
    }

    pub fn single(stream: LokiStream) -> Self {
        Self {
            streams: vec![stream],
        }
    }

    pub fn to_json_bytes(&self) -> Result<Vec<u8>, LokiPayloadError> {
        serde_json::to_vec(self).map_err(LokiPayloadError::Serialization)
    }
}

/// One Loki stream with low-cardinality labels and flat string metadata.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LokiStream {
    pub stream: BTreeMap<String, String>,
    pub values: Vec<[String; 2]>,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, String>,
}

impl LokiStream {
    /// Converts an already-redacted event into one Loki stream entry.
    ///
    /// The timestamp is Unix time in nanoseconds. Loki requires it to be
    /// encoded as a string inside the `[timestamp, line]` array.
    pub fn from_event(
        event: &HttpEvent,
        service: &ServiceConfig,
        timestamp_ns: u128,
    ) -> Result<Self, LokiPayloadError> {
        let line = serde_json::to_string(event).map_err(LokiPayloadError::Serialization)?;

        let mut labels = BTreeMap::from([
            ("service".to_owned(), service.name.clone()),
            ("environment".to_owned(), service.environment.clone()),
            ("version".to_owned(), service.version.clone()),
            ("event_type".to_owned(), "http".to_owned()),
        ]);

        if let Some(region) = &service.region {
            labels.insert("region".to_owned(), region.clone());
        }

        let mut metadata = BTreeMap::from([
            ("schema_version".to_owned(), event.schema_version.clone()),
            ("request_id".to_owned(), event.request_id.clone()),
            ("http_method".to_owned(), event.request.method.clone()),
            ("route".to_owned(), event.request.route.clone()),
            (
                "status_code".to_owned(),
                event.response.status_code.to_string(),
            ),
        ]);

        if let Some(trace_id) = &event.trace_id {
            metadata.insert("trace_id".to_owned(), trace_id.clone());
        }

        if let Some(span_id) = &event.span_id {
            metadata.insert("span_id".to_owned(), span_id.clone());
        }

        Ok(Self {
            stream: labels,
            values: vec![[timestamp_ns.to_string(), line]],
            metadata,
        })
    }
}

/// A payload serialization failure.
#[derive(Debug)]
pub enum LokiPayloadError {
    Serialization(serde_json::Error),
}

impl fmt::Display for LokiPayloadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization(error) => {
                write!(formatter, "failed to serialize Loki payload: {error}")
            }
        }
    }
}

impl Error for LokiPayloadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Serialization(error) => Some(error),
        }
    }
}
