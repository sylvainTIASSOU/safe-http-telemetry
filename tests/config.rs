use std::collections::BTreeSet;

use safe_http_telemetry::config::{ConfigError, TelemetryConfig};

#[test]
fn default_configuration_is_valid_and_bounded() {
    let config = TelemetryConfig::default();

    assert_eq!(config.validate(), Ok(()));
    assert_eq!(config.limits.max_request_body_bytes, 64 * 1024);
    assert_eq!(config.limits.max_response_body_bytes, 64 * 1024);
    assert!(config.limits.max_event_bytes > 0);
    assert!(config.limits.max_loki_batch_bytes > 0);
}

#[test]
fn zero_body_limit_is_rejected() {
    let mut config = TelemetryConfig::default();
    config.limits.max_request_body_bytes = 0;

    assert_eq!(
        config.validate(),
        Err(ConfigError::ZeroLimit("max_request_body_bytes"))
    );
}

#[test]
fn zero_json_depth_is_rejected() {
    let mut config = TelemetryConfig::default();
    config.limits.max_json_depth = 0;

    assert_eq!(
        config.validate(),
        Err(ConfigError::ZeroLimit("max_json_depth"))
    );
}

#[test]
fn event_limit_cannot_exceed_batch_limit() {
    let mut config = TelemetryConfig::default();
    config.limits.max_event_bytes = 5 * 1024 * 1024;
    config.limits.max_loki_batch_bytes = 4 * 1024 * 1024;

    assert_eq!(
        config.validate(),
        Err(ConfigError::EventLimitExceedsBatchLimit {
            max_event_bytes: 5 * 1024 * 1024,
            max_loki_batch_bytes: 4 * 1024 * 1024,
        })
    );
}

#[test]
fn empty_service_name_is_rejected() {
    let mut config = TelemetryConfig::default();
    config.service.name = "  ".to_owned();

    assert_eq!(
        config.validate(),
        Err(ConfigError::EmptyServiceField("name"))
    );
}

#[test]
fn empty_sensitive_field_is_rejected() {
    let mut config = TelemetryConfig::default();
    config.redaction.sensitive_fields = BTreeSet::from(["password".to_owned(), " ".to_owned()]);

    assert_eq!(config.validate(), Err(ConfigError::EmptySensitiveField));
}

#[test]
fn default_sensitive_fields_cover_common_credentials() {
    let config = TelemetryConfig::default();

    for field in [
        "password",
        "access_token",
        "refresh_token",
        "authorization",
        "api_key",
        "client_secret",
        "cookie",
        "set-cookie",
        "private_key",
    ] {
        assert!(
            config.redaction.sensitive_fields.contains(field),
            "missing default sensitive field: {field}"
        );
    }
}
