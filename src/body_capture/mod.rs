//! Bounded parsing and redaction of captured HTTP body prefixes.
//!
//! The streaming middleware supplies at most the configured number of bytes.
//! This module never forwards or exports a body itself and its data-bearing
//! result intentionally does not implement `Debug`.

use serde_json::{map::Entry, Map, Value};

use crate::{
    config::{CaptureLimits, ConfigError, RedactionConfig},
    core::{event::BodyCaptureStatus, telemetry::InternalTelemetry},
    redaction::Redactor,
};

/// selects the independent request or response byte limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodySource {
    Request,
    Response,
}

/// A bounded, redacted body-capture result.
///
/// This type intentionally does not implement `Debug`.
#[derive(Clone, PartialEq)]
pub struct BodyCaptureOutcome {
    pub body: Option<Value>,
    pub status: BodyCaptureStatus,
    pub truncated: bool,
    pub captured_bytes: usize,
}

impl BodyCaptureOutcome {
    fn empty(status: BodyCaptureStatus, truncated: bool, captured_bytes: usize) -> Self {
        Self {
            body: None,
            status,
            truncated,
            captured_bytes,
        }
    }
}

/// Reusable bounded body parser and redactor.
#[derive(Clone)]
pub struct BodyCaptureEngine {
    limits: CaptureLimits,
    redactor: Redactor,
    telemetry: InternalTelemetry,
}

impl BodyCaptureEngine {
    pub fn new(
        limits: &CaptureLimits,
        redaction: &RedactionConfig,
        telemetry: InternalTelemetry,
    ) -> Result<Self, ConfigError> {
        limits.validate()?;
        redaction.validate()?;

        let redactor = Redactor::new(redaction, limits, telemetry.clone())?;

        Ok(Self {
            limits: limits.clone(),
            redactor,
            telemetry,
        })
    }

    /// Parses and redacts at most the configured body prefix.
    ///
    /// `truncated` must be true when the streaming layer stopped capture after
    /// reaching its byte limit, even if `bytes.len()` equals that limit.
    /// Plain text is bounded but cannot receive structural field redaction;
    /// enable it only on routes whose textual content has been reviewed.
    pub fn capture(
        &self,
        source: BodySource,
        content_type: Option<&str>,
        content_encoding: Option<&str>,
        bytes: &[u8],
        truncated: bool,
    ) -> BodyCaptureOutcome {
        let limit = match source {
            BodySource::Request => self.limits.max_request_body_bytes,
            BodySource::Response => self.limits.max_response_body_bytes,
        };
        let captured_bytes = bytes.len().min(limit);
        let truncated = truncated || bytes.len() > limit;
        let bytes = &bytes[..captured_bytes];

        if bytes.is_empty() {
            return BodyCaptureOutcome::empty(BodyCaptureStatus::Captured, truncated, 0);
        }

        if !is_identity_encoding(content_encoding) {
            self.telemetry.record_body_omission();
            return BodyCaptureOutcome::empty(
                BodyCaptureStatus::Unsupported,
                truncated,
                captured_bytes,
            );
        }

        let Some(content_type) = content_type.map(normalize_content_type) else {
            self.telemetry.record_body_omission();
            return BodyCaptureOutcome::empty(
                BodyCaptureStatus::Unsupported,
                truncated,
                captured_bytes,
            );
        };

        if is_json(content_type) {
            self.capture_json(bytes, truncated, captured_bytes)
        } else if content_type.eq_ignore_ascii_case("application/x-www-form-urlencoded") {
            self.capture_form(bytes, truncated, captured_bytes)
        } else if content_type
            .split('/')
            .next()
            .is_some_and(|kind| kind.eq_ignore_ascii_case("text"))
        {
            self.capture_text(bytes, truncated, captured_bytes)
        } else {
            self.telemetry.record_body_omission();
            BodyCaptureOutcome::empty(BodyCaptureStatus::Unsupported, truncated, captured_bytes)
        }
    }

    fn capture_json(
        &self,
        bytes: &[u8],
        truncated: bool,
        captured_bytes: usize,
    ) -> BodyCaptureOutcome {
        let mut body = match serde_json::from_slice::<Value>(bytes) {
            Ok(body) => body,
            Err(_) => {
                self.telemetry.record_parse_error();
                return BodyCaptureOutcome::empty(
                    BodyCaptureStatus::ParseError,
                    truncated,
                    captured_bytes,
                );
            }
        };

        let report = self.redactor.redact_json(&mut body);
        BodyCaptureOutcome {
            body: Some(body),
            status: BodyCaptureStatus::Captured,
            truncated: truncated || report.truncated_values > 0,
            captured_bytes,
        }
    }

    fn capture_form(
        &self,
        bytes: &[u8],
        truncated: bool,
        captured_bytes: usize,
    ) -> BodyCaptureOutcome {
        // Form decoding is intentionally tolerant of incomplete pairs. A
        // truncated name could otherwise bypass sensitive-field matching.
        if truncated {
            self.telemetry.record_parse_error();
            return BodyCaptureOutcome::empty(BodyCaptureStatus::ParseError, true, captured_bytes);
        }

        let mut fields = Map::new();
        let max_fields = self.limits.max_json_nodes.saturating_sub(1);
        let mut fields_truncated = false;

        for (index, (name, value)) in form_urlencoded::parse(bytes).enumerate() {
            if index == max_fields {
                fields_truncated = true;
                self.telemetry.record_truncations(1);
                break;
            }

            // Lossy decoding could change a sensitive field name. Reject the
            // complete form instead of risking an incorrectly classified key.
            if name.contains('\u{fffd}') || value.contains('\u{fffd}') {
                self.telemetry.record_parse_error();
                return BodyCaptureOutcome::empty(
                    BodyCaptureStatus::ParseError,
                    false,
                    captured_bytes,
                );
            }

            let value = Value::String(value.into_owned());
            match fields.entry(name.into_owned()) {
                Entry::Vacant(entry) => {
                    entry.insert(value);
                }
                Entry::Occupied(mut entry) => match entry.get_mut() {
                    Value::Array(values) => values.push(value),
                    previous => {
                        let previous = std::mem::replace(previous, Value::Null);
                        *entry.get_mut() = Value::Array(vec![previous, value]);
                    }
                },
            }
        }

        let mut body = Value::Object(fields);
        let report = self.redactor.redact_json(&mut body);
        BodyCaptureOutcome {
            body: Some(body),
            status: BodyCaptureStatus::Captured,
            truncated: fields_truncated || report.truncated_values > 0,
            captured_bytes,
        }
    }

    fn capture_text(
        &self,
        bytes: &[u8],
        truncated: bool,
        captured_bytes: usize,
    ) -> BodyCaptureOutcome {
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) => {
                self.telemetry.record_parse_error();
                return BodyCaptureOutcome::empty(
                    BodyCaptureStatus::ParseError,
                    truncated,
                    captured_bytes,
                );
            }
        };

        let mut body = Value::String(text.to_owned());
        let report = self.redactor.redact_json(&mut body);
        BodyCaptureOutcome {
            body: Some(body),
            status: BodyCaptureStatus::Captured,
            truncated: truncated || report.truncated_values > 0,
            captured_bytes,
        }
    }
}

fn normalize_content_type(content_type: &str) -> &str {
    content_type.split(';').next().unwrap_or_default().trim()
}

fn is_json(content_type: &str) -> bool {
    content_type.eq_ignore_ascii_case("application/json")
        || content_type
            .rsplit_once('+')
            .is_some_and(|(prefix, suffix)| {
                prefix.contains('/') && suffix.eq_ignore_ascii_case("json")
            })
}

fn is_identity_encoding(content_encoding: Option<&str>) -> bool {
    content_encoding.is_none_or(|encoding| encoding.trim().eq_ignore_ascii_case("identity"))
}
