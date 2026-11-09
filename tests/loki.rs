#![cfg(feature = "loki")]

use safe_http_telemetry::{
    config::ServiceConfig,
    core::event::{CaptureInfo, HttpEvent, RequestInfo, ResponseInfo, TimingInfo},
    loki::{
        config::{LokiConfig, LokiConfigError},
        payload::{LokiPushRequest, LokiStream},
    },
};
use serde_json::json;

fn sample_event() -> HttpEvent {
    let mut request = RequestInfo::new("POST", "/api/users/{id}", "/api/users/42");
    request.body = Some(json!({
        "password": "[REDACTED]"
    }));

    let mut response = ResponseInfo::new(201);
    response.body = Some(json!({
        "access_token": "[REDACTED]"
    }));

    let mut event = HttpEvent::new(
        "request-loki-test",
        request,
        response,
        TimingInfo::new(8.25),
        CaptureInfo::default(),
    );

    event.trace_id = Some("4bf92f3577b34da6a3ce929d0e0e4736".to_owned());
    event.span_id = Some("00f067aa0ba902b7".to_owned());
    event
}

fn sample_service() -> ServiceConfig {
    ServiceConfig {
        name: "users-api".to_owned(),
        environment: "test".to_owned(),
        version: "0.1.0".to_owned(),
        region: Some("eu-west".to_owned()),
    }
}

#[test]
fn valid_loki_configuration_is_accepted() {
    let config = LokiConfig::new("https://loki.example.test/loki/api/v1/push")
        .unwrap()
        .with_tenant_id("test-tenant")
        .with_bearer_token("fake-test-token");

    assert_eq!(config.validate(), Ok(()));
}

#[test]
fn unsupported_loki_scheme_is_rejected() {
    let result = LokiConfig::new("ftp://loki.example.test/loki/api/v1/push");

    assert_eq!(result.unwrap_err(), LokiConfigError::UnsupportedScheme);
}

#[test]
fn credentials_in_url_are_rejected() {
    let result = LokiConfig::new("https://user:fake-password@loki.example.test/loki/api/v1/push");

    assert_eq!(result.unwrap_err(), LokiConfigError::UrlContainsCredentials);
}

#[test]
fn query_and_fragment_are_rejected() {
    let mut config = LokiConfig::new("https://loki.example.test/loki/api/v1/push").unwrap();

    config.push_url.set_query(Some("token=fake-query-secret"));

    assert_eq!(
        config.validate(),
        Err(LokiConfigError::UrlContainsQueryOrFragment)
    );
}

#[test]
fn invalid_tenant_is_rejected() {
    let config = LokiConfig::new("https://loki.example.test/loki/api/v1/push")
        .unwrap()
        .with_tenant_id("tenant\r\ninjected-header");

    assert_eq!(config.validate(), Err(LokiConfigError::InvalidTenantId));
}

#[test]
fn zero_batch_size_is_rejected() {
    let mut config = LokiConfig::new("https://loki.example.test/loki/api/v1/push").unwrap();

    config.batch.max_bytes = 0;

    assert_eq!(
        config.validate(),
        Err(LokiConfigError::ZeroValue("batch.max_bytes"))
    );
}

#[test]
fn invalid_backoff_order_is_rejected() {
    let mut config = LokiConfig::new("https://loki.example.test/loki/api/v1/push").unwrap();

    config.retry.initial_backoff = std::time::Duration::from_secs(3);
    config.retry.max_backoff = std::time::Duration::from_secs(2);

    assert_eq!(config.validate(), Err(LokiConfigError::InvalidBackoffOrder));
}

#[test]
fn debug_output_does_not_expose_bearer_token() {
    let config = LokiConfig::new("https://loki.example.test/loki/api/v1/push")
        .unwrap()
        .with_bearer_token("fake-token-must-not-leak");

    let debug = format!("{config:?}");

    assert!(!debug.contains("fake-token-must-not-leak"));
    assert!(debug.contains("[REDACTED]"));
}

#[test]
fn loki_payload_uses_string_nanosecond_timestamp() {
    let event = sample_event();
    let stream =
        LokiStream::from_event(&event, &sample_service(), 1_730_000_000_123_456_789).unwrap();

    let payload = LokiPushRequest::single(stream);
    let bytes = payload.to_json_bytes().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(value["streams"][0]["values"][0][0], "1730000000123456789");
}

#[test]
fn complete_event_is_preserved_inside_loki_log_line() {
    let event = sample_event();
    let stream =
        LokiStream::from_event(&event, &sample_service(), 1_730_000_000_123_456_789).unwrap();

    let line = &stream.values[0][1];
    let decoded: HttpEvent = serde_json::from_str(line).unwrap();

    assert!(decoded == event);
}

#[test]
fn labels_have_bounded_cardinality() {
    let stream = LokiStream::from_event(
        &sample_event(),
        &sample_service(),
        1_730_000_000_123_456_789,
    )
    .unwrap();

    assert_eq!(stream.stream["service"], "users-api");
    assert_eq!(stream.stream["environment"], "test");
    assert_eq!(stream.stream["version"], "0.1.0");
    assert_eq!(stream.stream["event_type"], "http");
    assert_eq!(stream.stream["region"], "eu-west");

    for forbidden in [
        "request_id",
        "trace_id",
        "span_id",
        "path",
        "client_ip",
        "body",
    ] {
        assert!(!stream.stream.contains_key(forbidden));
    }
}

#[test]
fn structured_metadata_is_flat_and_contains_correlation_ids() {
    let stream = LokiStream::from_event(
        &sample_event(),
        &sample_service(),
        1_730_000_000_123_456_789,
    )
    .unwrap();

    assert_eq!(stream.metadata["request_id"], "request-loki-test");
    assert_eq!(
        stream.metadata["trace_id"],
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
    assert_eq!(stream.metadata["http_method"], "POST");
    assert_eq!(stream.metadata["status_code"], "201");

    assert!(stream.metadata.values().all(|value| {
        serde_json::from_str::<serde_json::Value>(value).is_err() || !value.starts_with('{')
    }));
}
