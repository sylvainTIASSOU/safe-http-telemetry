#![cfg(feature = "body-capture")]

use safe_http_telemetry::{
    body_capture::{BodyCaptureEngine, BodySource},
    config::TelemetryConfig,
    core::{event::BodyCaptureStatus, telemetry::InternalTelemetry},
    redaction::{REDACTED, TRUNCATED},
};
use serde_json::{json, Value};

fn engine(update: impl FnOnce(&mut TelemetryConfig)) -> (BodyCaptureEngine, InternalTelemetry) {
    let mut config = TelemetryConfig::default();
    update(&mut config);
    let telemetry = InternalTelemetry::new();
    let engine = BodyCaptureEngine::new(&config.limits, &config.redaction, telemetry.clone())
        .expect("valid body-capture configuration");

    (engine, telemetry)
}

#[test]
fn json_and_json_suffixes_are_redacted_before_export() {
    let (engine, _) = engine(|_| {});
    let input = br#"{
        "safe": "visible",
        "password": "fictitious-json-secret",
        "nested": [{"accessToken": "fictitious-nested-secret"}]
    }"#;

    for content_type in [
        "application/json; charset=utf-8",
        "application/problem+json",
        "APPLICATION/PROBLEM+JSON",
    ] {
        let outcome = engine.capture(BodySource::Request, Some(content_type), None, input, false);
        let body = outcome.body.expect("redacted JSON body");

        assert_eq!(outcome.status, BodyCaptureStatus::Captured);
        assert_eq!(body["safe"], "visible");
        assert_eq!(body["password"], REDACTED);
        assert_eq!(body["nested"][0]["accessToken"], REDACTED);

        let encoded = body.to_string();
        assert!(!encoded.contains("fictitious-json-secret"));
        assert!(!encoded.contains("fictitious-nested-secret"));
    }
}

#[test]
fn truncated_invalid_json_never_falls_back_to_raw_text() {
    let (engine, telemetry) = engine(|config| {
        config.limits.max_request_body_bytes = 24;
    });
    let input = br#"{"safe":"visible","token":"fictitious-cut-secret"}"#;

    let outcome = engine.capture(
        BodySource::Request,
        Some("application/json"),
        None,
        input,
        false,
    );

    assert_eq!(outcome.status, BodyCaptureStatus::ParseError);
    assert!(outcome.body.is_none());
    assert!(outcome.truncated);
    assert_eq!(outcome.captured_bytes, 24);
    assert_eq!(telemetry.snapshot().parse_errors, 1);
}

#[test]
fn forms_preserve_repeated_fields_and_redact_sensitive_names() {
    let (engine, _) = engine(|_| {});
    let input = b"tag=one&tag=two&access%5Ftoken=fictitious-form-secret&safe=visible";

    let outcome = engine.capture(
        BodySource::Request,
        Some("application/x-www-form-urlencoded; charset=utf-8"),
        Some("identity"),
        input,
        false,
    );
    let body = outcome.body.expect("redacted form body");

    assert_eq!(outcome.status, BodyCaptureStatus::Captured);
    assert_eq!(body["tag"], json!(["one", "two"]));
    assert_eq!(body["access_token"], REDACTED);
    assert_eq!(body["safe"], "visible");
    assert!(!body.to_string().contains("fictitious-form-secret"));
}

#[test]
fn truncated_or_lossy_forms_are_omitted_without_exposing_values() {
    let (engine, telemetry) = engine(|config| {
        config.limits.max_request_body_bytes = 16;
    });

    let truncated = engine.capture(
        BodySource::Request,
        Some("application/x-www-form-urlencoded"),
        None,
        b"safe=visible&token=fictitious-form-secret",
        false,
    );
    assert_eq!(truncated.status, BodyCaptureStatus::ParseError);
    assert!(truncated.body.is_none());
    assert!(truncated.truncated);

    let lossy = engine.capture(
        BodySource::Request,
        Some("application/x-www-form-urlencoded"),
        None,
        b"tok%FFen=fictitious-lossy-secret",
        false,
    );
    assert_eq!(lossy.status, BodyCaptureStatus::ParseError);
    assert!(lossy.body.is_none());
    assert_eq!(telemetry.snapshot().parse_errors, 2);
}

#[test]
fn form_field_limit_bounds_the_result() {
    let (engine, telemetry) = engine(|config| {
        config.limits.max_json_nodes = 2;
    });

    let outcome = engine.capture(
        BodySource::Request,
        Some("application/x-www-form-urlencoded"),
        None,
        b"first=visible&second=omitted&third=omitted",
        false,
    );
    let body = outcome.body.expect("bounded form body");

    assert_eq!(body, json!({"first": "visible"}));
    assert!(outcome.truncated);
    assert_eq!(telemetry.snapshot().truncated_values, 1);
}

#[test]
fn textual_bodies_are_utf8_bounded_without_splitting_code_points() {
    let (engine, _) = engine(|config| {
        config.limits.max_string_bytes = 12;
    });

    let outcome = engine.capture(
        BodySource::Response,
        Some("text/plain; charset=utf-8"),
        None,
        "123456789éXYZ".as_bytes(),
        false,
    );

    assert_eq!(outcome.status, BodyCaptureStatus::Captured);
    assert_eq!(outcome.body, Some(Value::String(format!("1{TRUNCATED}"))));
    assert!(outcome.truncated);
    assert_eq!(outcome.captured_bytes, "123456789éXYZ".len());
}

#[test]
fn request_and_response_byte_limits_are_independent() {
    let (engine, _) = engine(|config| {
        config.limits.max_request_body_bytes = 8;
        config.limits.max_response_body_bytes = 16;
    });
    let input = b"123456789012";

    let request = engine.capture(BodySource::Request, Some("text/plain"), None, input, false);
    let response = engine.capture(BodySource::Response, Some("text/plain"), None, input, false);

    assert_eq!(request.captured_bytes, 8);
    assert!(request.truncated);
    assert_eq!(response.captured_bytes, 12);
    assert!(!response.truncated);
}

#[test]
fn compressed_binary_and_invalid_utf8_bodies_are_not_exported() {
    let (engine, telemetry) = engine(|_| {});

    let compressed = engine.capture(
        BodySource::Request,
        Some("application/json"),
        Some("gzip"),
        br#"{"token":"fictitious-compressed-secret"}"#,
        false,
    );
    let binary = engine.capture(
        BodySource::Request,
        Some("application/octet-stream"),
        None,
        b"fictitious-binary-secret",
        false,
    );
    let invalid_text = engine.capture(
        BodySource::Request,
        Some("text/plain"),
        None,
        &[0xff, 0xfe],
        false,
    );

    assert_eq!(compressed.status, BodyCaptureStatus::Unsupported);
    assert!(compressed.body.is_none());
    assert_eq!(binary.status, BodyCaptureStatus::Unsupported);
    assert!(binary.body.is_none());
    assert_eq!(invalid_text.status, BodyCaptureStatus::ParseError);
    assert!(invalid_text.body.is_none());
    assert_eq!(telemetry.snapshot().omitted_bodies, 2);
    assert_eq!(telemetry.snapshot().parse_errors, 1);
}

#[test]
fn empty_body_is_captured_without_allocation_or_parse_error() {
    let (engine, telemetry) = engine(|_| {});
    let outcome = engine.capture(BodySource::Request, None, None, b"", false);

    assert_eq!(outcome.status, BodyCaptureStatus::Captured);
    assert!(outcome.body.is_none());
    assert_eq!(outcome.captured_bytes, 0);
    assert!(!outcome.truncated);
    assert_eq!(telemetry.snapshot().parse_errors, 0);
}
