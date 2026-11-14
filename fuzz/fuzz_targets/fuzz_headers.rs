#![no_main]
use libfuzzer_sys::fuzz_target;
use std::collections::BTreeMap;
use safe_http_telemetry::{
    config::{CaptureLimits, RedactionConfig},
    core::telemetry::InternalTelemetry,
    redaction::Redactor,
};

fuzz_target!(|data: &[u8]| {
    // 1. Construire une map de headers à partir des bytes.
    // On utilise un séparateur simple (par exemple, '\0' ou ':') pour créer des paires clé-valeur.
    if let Ok(s) = std::str::from_utf8(data) {
        let mut headers = BTreeMap::new();
        for part in s.split('\0') {
            if let Some((key, value)) = part.split_once(':') {
                headers.insert(key.trim().to_owned(), value.trim().to_owned());
            }
        }

        // 2. Créer un Redactor et appliquer la redaction.
        let redactor = Redactor::new(
            &RedactionConfig::default(),
            &CaptureLimits::default(),
            InternalTelemetry::new(),
        )
        .expect("Default config should always be valid");

        let _ = redactor.redact_headers(&mut headers);
    }
});
