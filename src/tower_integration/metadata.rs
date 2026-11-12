//! Bounded capture and redaction of HTTP query parameters and headers.

use std::{collections::BTreeMap, sync::Arc};

use http::HeaderMap;

use crate::{
    config::{CaptureLimits, ConfigError},
    core::{
        event::{QueryScalar, QueryValue},
        telemetry::InternalTelemetry,
    },
    redaction::{Redactor, UNSUPPORTED},
};

use super::config::MiddlewareConfig;

#[derive(Clone)]
pub(super) struct MetadataCapturer {
    limits: Arc<CaptureLimits>,
    redactor: Arc<Redactor>,
    telemetry: InternalTelemetry,
}

impl MetadataCapturer {
    pub(super) fn new(
        config: &MiddlewareConfig,
        telemetry: InternalTelemetry,
    ) -> Result<Self, ConfigError> {
        config.limits.validate()?;
        config.redaction.validate()?;

        let redactor = Redactor::new(&config.redaction, &config.limits, telemetry.clone())?;

        Ok(Self {
            limits: Arc::new(config.limits.clone()),
            redactor: Arc::new(redactor),
            telemetry,
        })
    }

    pub(super) fn limits(&self) -> &CaptureLimits {
        &self.limits
    }

    pub(super) fn capture_query(&self, raw_query: Option<&str>) -> BTreeMap<String, QueryValue> {
        let mut captured = BTreeMap::new();
        let Some(raw_query) = raw_query else {
            return captured;
        };

        for (index, (name, value)) in form_urlencoded::parse(raw_query.as_bytes()).enumerate() {
            if index == self.limits.max_query_parameters {
                self.telemetry.record_truncations(1);
                break;
            }

            // Omitting an oversized name is safer than truncating it before
            // sensitive-field matching. The value is never copied in this case.
            if name.len() > self.limits.max_string_bytes {
                self.telemetry.record_truncations(1);
                continue;
            }

            let name = name.into_owned();
            let value = bounded_value(&value, self.limits.max_string_bytes, &self.telemetry);

            match captured.entry(name) {
                std::collections::btree_map::Entry::Vacant(entry) => {
                    entry.insert(QueryValue::Scalar(QueryScalar::String(value)));
                }
                std::collections::btree_map::Entry::Occupied(mut entry) => match entry.get_mut() {
                    QueryValue::Scalar(previous) => {
                        let previous = std::mem::replace(previous, QueryScalar::Null);
                        *entry.get_mut() =
                            QueryValue::Array(vec![previous, QueryScalar::String(value)]);
                    }
                    QueryValue::Array(values) => {
                        values.push(QueryScalar::String(value));
                    }
                },
            }
        }

        self.redactor.redact_query(&mut captured);
        captured
    }

    pub(super) fn capture_headers(&self, headers: &HeaderMap) -> BTreeMap<String, String> {
        let mut captured = BTreeMap::new();
        let mut total_bytes = 0_usize;

        for (index, (name, value)) in headers.iter().enumerate() {
            if index == self.limits.max_headers {
                self.telemetry.record_truncations(1);
                break;
            }

            let name = name.as_str();

            if name.len() > self.limits.max_string_bytes {
                self.telemetry.record_truncations(1);
                continue;
            }

            let Some(after_name) = total_bytes.checked_add(name.len()) else {
                self.telemetry.record_truncations(1);
                break;
            };

            if after_name > self.limits.max_total_header_bytes {
                self.telemetry.record_truncations(1);
                break;
            }

            let value = match value.to_str() {
                Ok(value) => {
                    bounded_value(value, self.limits.max_header_value_bytes, &self.telemetry)
                }
                Err(_) => UNSUPPORTED.to_owned(),
            };

            let Some(entry_end) = after_name.checked_add(value.len()) else {
                self.telemetry.record_truncations(1);
                break;
            };

            if entry_end > self.limits.max_total_header_bytes {
                self.telemetry.record_truncations(1);
                break;
            }

            total_bytes = entry_end;
            captured.insert(name.to_owned(), value);
        }

        self.redactor.redact_headers(&mut captured);
        captured
    }
}

pub(super) fn bounded_value(
    value: &str,
    max_bytes: usize,
    telemetry: &InternalTelemetry,
) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }

    let prefix_budget = max_bytes - crate::redaction::TRUNCATED.len();
    let mut prefix_end = prefix_budget.min(value.len());

    while !value.is_char_boundary(prefix_end) {
        prefix_end -= 1;
    }

    let mut truncated = String::with_capacity(max_bytes);
    truncated.push_str(&value[..prefix_end]);
    truncated.push_str(crate::redaction::TRUNCATED);
    telemetry.record_truncations(1);
    truncated
}
