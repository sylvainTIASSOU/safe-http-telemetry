#![no_main]
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;
use safe_http_telemetry::{
    config::{CaptureLimits, RedactionConfig},
    core::{
        event::{QueryScalar, QueryValue},
        telemetry::InternalTelemetry,
    },
    redaction::Redactor,
};

fuzz_target!(|data: &[u8]| {
    // 1. Build a query-parameter map from the input bytes.
    if let Ok(s) = std::str::from_utf8(data) {
        let mut query = BTreeMap::new();
        for part in s.split('&') {
            if let Some((key, value)) = part.split_once('=') {
                query.insert(
                    key.trim().to_owned(),
                    QueryValue::Scalar(QueryScalar::String(value.trim().to_owned())),
                );
            }
        }

        // 2. Create a Redactor and apply redaction.
        let redactor = Redactor::new(
            &RedactionConfig::default(),
            &CaptureLimits::default(),
            InternalTelemetry::new(),
        )
        .expect("Default config should always be valid");

        let _ = redactor.redact_query(&mut query);
    }
});
