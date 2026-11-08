//! Deterministic, bounded sensitive-data redaction.
//!
//! Field matching is case-insensitive and ignores separators. Structural
//! redaction happens before content is queued or exported. Unknown secret
//! formats cannot be detected automatically and require custom policy rules.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    config::{CaptureLimits, ConfigError, RedactionConfig},
    core::{
        event::{QueryScalar, QueryValue},
        telemetry::InternalTelemetry,
    },
};

pub const REDACTED: &str = "[REDACTED]";
pub const TRUNCATED: &str = "[TRUNCATED]";
pub const OMITTED: &str = "[OMITTED]";
pub const UNSUPPORTED: &str = "[UNSUPPORTED]";
pub const PARSE_ERROR: &str = "[PARSE_ERROR]";

/// Counts produced by one redaction operation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RedactionReport {
    pub redacted_values: u64,
    pub truncated_values: u64,
}

impl RedactionReport {
    fn record_redaction(&mut self) {
        self.redacted_values += 1;
    }

    fn record_truncation(&mut self) {
        self.truncated_values += 1;
    }
}

/// A reusable redaction engine with immutable rules and hard limits.
#[derive(Clone, Debug)]
pub struct Redactor {
    sensitive_fields: BTreeSet<String>,
    max_json_depth: usize,
    max_json_nodes: usize,
    max_string_bytes: usize,
    max_header_value_bytes: usize,
    telemetry: InternalTelemetry,
}

impl Redactor {
    /// Builds an engine only from validated, bounded configuration.
    pub fn new(
        config: &RedactionConfig,
        limits: &CaptureLimits,
        telemetry: InternalTelemetry,
    ) -> Result<Self, ConfigError> {
        config.validate()?;
        limits.validate()?;

        let sensitive_fields = config
            .sensitive_fields
            .iter()
            .map(|field| normalize_field_name(field))
            .collect();

        Ok(Self {
            sensitive_fields,
            max_json_depth: limits.max_json_depth,
            max_json_nodes: limits.max_json_nodes,
            max_string_bytes: limits.max_string_bytes,
            max_header_value_bytes: limits.max_header_value_bytes,
            telemetry,
        })
    }

    /// Returns whether a field name identifies sensitive data.
    pub fn is_sensitive_field(&self, field: &str) -> bool {
        self.sensitive_fields.contains(&normalize_field_name(field))
    }

    /// Recursively redacts and truncates a JSON value.
    ///
    /// The root counts as one node and has depth zero. Sensitive values are
    /// replaced as a whole, including objects and arrays.
    pub fn redact_json(&self, value: &mut serde_json::Value) -> RedactionReport {
        let mut state = RedactionState::new(self.max_json_nodes);
        self.redact_json_value(value, 0, &mut state);
        self.finish(state.report)
    }

    /// Redacts sensitive headers and bounds all remaining header values.
    pub fn redact_headers(&self, headers: &mut BTreeMap<String, String>) -> RedactionReport {
        let mut report = RedactionReport::default();

        for (name, value) in headers {
            if self.is_sensitive_field(name) {
                *value = REDACTED.to_owned();
                report.record_redaction();
            } else if let Some(truncated) = truncate_string(value, self.max_header_value_bytes) {
                *value = truncated;
                report.record_truncation();
            }
        }

        self.finish(report)
    }

    /// Redacts query parameters while preserving scalar and array shapes.
    pub fn redact_query(&self, query: &mut BTreeMap<String, QueryValue>) -> RedactionReport {
        let mut state = RedactionState::new(self.max_json_nodes);

        for (name, value) in query {
            if self.is_sensitive_field(name) {
                *value = QueryValue::from(REDACTED);
                state.report.record_redaction();
                continue;
            }

            match value {
                QueryValue::Scalar(scalar) => {
                    self.redact_query_scalar(scalar, &mut state);
                }
                QueryValue::Array(scalars) => {
                    for scalar in scalars {
                        self.redact_query_scalar(scalar, &mut state);
                    }
                }
            }
        }

        self.finish(state.report)
    }

    fn redact_json_value(
        &self,
        value: &mut serde_json::Value,
        depth: usize,
        state: &mut RedactionState,
    ) {
        if state.remaining_nodes == 0 {
            replace_with_truncated(value, &mut state.report);
            return;
        }

        state.remaining_nodes -= 1;

        if depth > self.max_json_depth {
            replace_with_truncated(value, &mut state.report);
            return;
        }

        match value {
            serde_json::Value::Object(fields) => {
                for (name, nested) in fields {
                    if self.is_sensitive_field(name) {
                        *nested = serde_json::Value::String(REDACTED.to_owned());
                        state.report.record_redaction();
                    } else {
                        self.redact_json_value(nested, depth + 1, state);
                    }
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    self.redact_json_value(item, depth + 1, state);
                }
            }
            serde_json::Value::String(text) => {
                if let Some(truncated) = truncate_string(text, self.max_string_bytes) {
                    *text = truncated;
                    state.report.record_truncation();
                }
            }
            _ => {}
        }
    }

    fn redact_query_scalar(&self, scalar: &mut QueryScalar, state: &mut RedactionState) {
        if state.remaining_nodes == 0 {
            *scalar = QueryScalar::String(TRUNCATED.to_owned());
            state.report.record_truncation();
            return;
        }

        state.remaining_nodes -= 1;

        if let QueryScalar::String(text) = scalar {
            if let Some(truncated) = truncate_string(text, self.max_string_bytes) {
                *text = truncated;
                state.report.record_truncation();
            }
        }
    }

    fn finish(&self, report: RedactionReport) -> RedactionReport {
        self.telemetry.record_redactions(report.redacted_values);
        self.telemetry.record_truncations(report.truncated_values);
        report
    }
}

struct RedactionState {
    remaining_nodes: usize,
    report: RedactionReport,
}

impl RedactionState {
    fn new(max_nodes: usize) -> Self {
        Self {
            remaining_nodes: max_nodes,
            report: RedactionReport::default(),
        }
    }
}

fn replace_with_truncated(value: &mut serde_json::Value, report: &mut RedactionReport) {
    *value = serde_json::Value::String(TRUNCATED.to_owned());
    report.record_truncation();
}

/// Normalizes `access_token`, `access-token` and `accessToken` to the same key.
fn normalize_field_name(field: &str) -> String {
    field
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Truncates a string without splitting a UTF-8 code point.
///
/// The returned value, including the marker, never exceeds `max_bytes`.
fn truncate_string(value: &str, max_bytes: usize) -> Option<String> {
    if value.len() <= max_bytes {
        return None;
    }

    debug_assert!(max_bytes >= TRUNCATED.len());

    let prefix_budget = max_bytes - TRUNCATED.len();
    let mut prefix_end = prefix_budget.min(value.len());

    while !value.is_char_boundary(prefix_end) {
        prefix_end -= 1;
    }

    let mut truncated = String::with_capacity(max_bytes);
    truncated.push_str(&value[..prefix_end]);
    truncated.push_str(TRUNCATED);
    Some(truncated)
}
