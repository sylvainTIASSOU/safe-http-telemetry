#![no_main]
use libfuzzer_sys::fuzz_target;
use safe_http_telemetry::{
    config::{CaptureLimits, RedactionConfig},
    core::telemetry::InternalTelemetry,
    redaction::Redactor,
};

fuzz_target!(|data: &[u8]| {
    // 1. Tenter de parser les bytes en JSON. Si ce n'est pas du JSON valide, on ignore.
    if let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(data) {
        // 2. Créer un Redactor avec les limites et règles par défaut.
        let redactor = Redactor::new(
            &RedactionConfig::default(),
            &CaptureLimits::default(),
            InternalTelemetry::new(),
        )
        .expect("Default config should always be valid");

        // 3. Appliquer la redaction. Ne doit jamais paniquer, même sur des structures profondes ou volumineuses.
        let _ = redactor.redact_json(&mut value);
    }
});
