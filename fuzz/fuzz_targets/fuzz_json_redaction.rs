#![no_main]
use libfuzzer_sys::fuzz_target;
use safe_http_telemetry::{
    config::{CaptureLimits, RedactionConfig},
    core::telemetry::InternalTelemetry,
    redaction::Redactor,
};

fuzz_target!(|data: &[u8]| {
    // 1. Try to parse the input bytes as JSON; ignore inputs that are not valid JSON.
    if let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(data) {
        // 2. Create a Redactor with the default limits and rules.
        let redactor = Redactor::new(
            &RedactionConfig::default(),
            &CaptureLimits::default(),
            InternalTelemetry::new(),
        )
        .expect("Default config should always be valid");

        // 3. Apply redaction. This must never panic, even on deeply nested or large structures.
        let _ = redactor.redact_json(&mut value);
    }
});
