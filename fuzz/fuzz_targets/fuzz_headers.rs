#![no_main]
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;
use safe_http_telemetry::{
    config::{CaptureLimits, RedactionConfig},
    core::telemetry::InternalTelemetry,
    redaction::Redactor,
};

fuzz_target!(|data: &[u8]| {
    // 1. Build a header map from the input bytes.
    // Use a simple separator (for example, '\0' or ':') to create key-value pairs.
    if let Ok(s) = std::str::from_utf8(data) {
        let mut headers = BTreeMap::new();
        for part in s.split('\0') {
            if let Some((key, value)) = part.split_once(':') {
                headers.insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }

        // 2. Create a Redactor and apply redaction.
        let redactor = Redactor::new(
            &RedactionConfig::default(),
            &CaptureLimits::default(),
            InternalTelemetry::new(),
        )
        .expect("Default config should always be valid");

        let _ = redactor.redact_headers(&mut headers);
    }
});
