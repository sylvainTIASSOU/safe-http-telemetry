//! Versioned HTTP event model corresponding to `http-event-v1`.
//!
//! Data-bearing event types intentionally do not implement `Debug`.
//! Request and response bodies must be policy-filtered and redacted before
//! an event is queued or exported.
//!

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};

pub const HTTP_EVENT_SCHEMA_VERSION: &str = "1.0";
pub const HTTP_EVENT_NAME: &str = "http.request.completed";

/// One completed HTTP request event.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct HttpEvent {
    pub schema_version: String,
    pub event: String,
    pub level: LogLevel,
    pub request_id: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,

    pub request: RequestInfo,
    pub response: ResponseInfo,
    pub timing: TimingInfo,
    pub capture: CaptureInfo,
}

impl HttpEvent {
    /// Creates a schema-v1 event and derives its level from the HTTP status.
    pub fn new(
        request_id: impl Into<String>,
        request: RequestInfo,
        response: ResponseInfo,
        timing: TimingInfo,
        capture: CaptureInfo,
    ) -> Self {
        let level = match response.outcome {
            Some(ResponseOutcome::Success) => LogLevel::Info,
            Some(ResponseOutcome::ClientError) | Some(ResponseOutcome::Cancelled) => LogLevel::Warn,
            Some(ResponseOutcome::ServerError) | Some(ResponseOutcome::Timeout) => LogLevel::Error,
            None => response
                .status_code
                .map_or(LogLevel::Info, LogLevel::from_status),
        };

        Self {
            schema_version: HTTP_EVENT_SCHEMA_VERSION.to_owned(),
            event: HTTP_EVENT_NAME.to_owned(),
            level,
            request_id: request_id.into(),
            trace_id: None,
            span_id: None,
            request,
            response,
            timing,
            capture,
        }
    }
}

/// Severity assigned to the completed HTTP event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub const fn from_status(status_code: u16) -> Self {
        if status_code >= 500 {
            Self::Error
        } else if status_code >= 400 {
            Self::Warn
        } else {
            Self::Info
        }
    }
}

/// Request-side data allowed by the event schema.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct RequestInfo {
    pub method: String,
    pub route: String,
    pub path: String,
    pub query: BTreeMap<String, QueryValue>,
    pub headers: BTreeMap<String, String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_length: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_ip: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub geolocation: Option<GeolocationInfo>,
}

impl RequestInfo {
    pub fn new(
        method: impl Into<String>,
        route: impl Into<String>,
        path: impl Into<String>,
    ) -> Self {
        Self {
            method: method.into(),
            route: route.into(),
            path: path.into(),
            query: BTreeMap::new(),
            headers: BTreeMap::new(),
            body: None,
            content_type: None,
            content_length: None,
            client_ip: None,
            geolocation: None,
        }
    }
}

// A scalar query value supported by schema v1.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum QueryScalar {
    String(String),
    Number(Number),
    Boolean(bool),
    Null,
}

/// Either one query scalar or repeated values for the same parameter.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum QueryValue {
    Scalar(QueryScalar),
    Array(Vec<QueryScalar>),
}

impl From<String> for QueryValue {
    fn from(value: String) -> Self {
        Self::Scalar(QueryScalar::String(value))
    }
}

impl From<&str> for QueryValue {
    fn from(value: &str) -> Self {
        Self::Scalar(QueryScalar::String(value.to_owned()))
    }
}

impl From<bool> for QueryValue {
    fn from(value: bool) -> Self {
        Self::Scalar(QueryScalar::Boolean(value))
    }
}

impl From<Number> for QueryValue {
    fn from(value: Number) -> Self {
        Self::Scalar(QueryScalar::Number(value))
    }
}

/// Optional geographic information derived from a local IP database.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct GeolocationInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub country_code: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub country: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub city: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub latitude: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub longitude: Option<f64>,
}

/// Response-side data allowed by the event schema.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseInfo {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status_code: Option<u16>,

    #[serde(default)]
    pub headers: BTreeMap<String, String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_length: Option<u64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome: Option<ResponseOutcome>,
}

impl ResponseInfo {
    pub fn new(status_code: u16) -> Self {
        Self {
            status_code: Some(status_code),
            headers: BTreeMap::new(),
            body: None,
            content_type: None,
            content_length: None,
            outcome: Some(ResponseOutcome::from_status(status_code)),
        }
    }

    /// Creates a terminal response state, possibly without HTTP headers.
    pub fn terminal(status_code: Option<u16>, outcome: ResponseOutcome) -> Self {
        Self {
            status_code,
            headers: BTreeMap::new(),
            body: None,
            content_type: None,
            content_length: None,
            outcome: Some(outcome),
        }
    }
}

/// Functional outcome of the HTTP exchange.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseOutcome {
    Success,
    ClientError,
    ServerError,
    Cancelled,
    Timeout,
}

impl ResponseOutcome {
    pub const fn from_status(status_code: u16) -> Self {
        if status_code >= 500 {
            Self::ServerError
        } else if status_code >= 400 {
            Self::ClientError
        } else {
            Self::Success
        }
    }
}

/// Numeric request timings in milliseconds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TimingInfo {
    pub duration_ms: f64,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_to_first_byte_ms: Option<f64>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_duration_ms: Option<f64>,
}

impl TimingInfo {
    pub const fn new(duration_ms: f64) -> Self {
        Self {
            duration_ms,
            time_to_first_byte_ms: None,
            body_duration_ms: None,
        }
    }
}

/// Body-capture outcome and truncation indicators.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureInfo {
    pub request_body_truncated: bool,
    pub response_body_truncated: bool,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_body_status: Option<BodyCaptureStatus>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_body_status: Option<BodyCaptureStatus>,
}

/// Explains why a body was captured or omitted.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyCaptureStatus {
    Captured,
    Omitted,
    Unsupported,
    ParseError,
}
