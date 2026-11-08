use std::collections::BTreeMap;

use safe_http_telemetry::{
    config::TelemetryConfig,
    core::{
        event::{QueryScalar, QueryValue},
        telemetry::InternalTelemetry,
    },
    redaction::{Redactor, REDACTED, TRUNCATED},
};
use serde_json::json;

fn default_redactor() -> Redactor {
    let config = TelemetryConfig::default();

    Redactor::new(&config.redaction, &config.limits, InternalTelemetry::new()).unwrap()
}

fn custom_redactor(update: impl FnOnce(&mut TelemetryConfig)) -> (Redactor, InternalTelemetry) {
    let mut config = TelemetryConfig::default();
    update(&mut config);

    let telemetry = InternalTelemetry::new();
    let redactor = Redactor::new(&config.redaction, &config.limits, telemetry.clone()).unwrap();

    (redactor, telemetry)
}

#[test]
fn nested_objects_and_arrays_are_redacted() {
    let redactor = default_redactor();
    let mut value = json!({
        "password": "fake-password",
        "profile": {
            "accessToken": "fake-access-token"
        },
        "sessions": [
            {
                "authorization": "Bearer fake-authorization"
            },
            {
                "client-secret": "fake-client-secret"
            }
        ],
        "safe": "visible"
    });

    let report = redactor.redact_json(&mut value);

    assert_eq!(value["password"], REDACTED);
    assert_eq!(value["profile"]["accessToken"], REDACTED);
    assert_eq!(value["sessions"][0]["authorization"], REDACTED);
    assert_eq!(value["sessions"][1]["client-secret"], REDACTED);
    assert_eq!(value["safe"], "visible");
    assert_eq!(report.redacted_values, 4);
}

#[test]
fn fake_secrets_do_not_survive_serialization() {
    let redactor = default_redactor();
    let mut value = json!({
        "password": "fake-password-never-export",
        "nested": [
            {
                "refresh_token": "fake-refresh-never-export"
            }
        ]
    });

    redactor.redact_json(&mut value);
    let encoded = serde_json::to_string(&value).unwrap();

    assert!(!encoded.contains("fake-password-never-export"));
    assert!(!encoded.contains("fake-refresh-never-export"));
}

#[test]
fn field_matching_handles_case_separators_and_camel_case() {
    let redactor = default_redactor();

    for field in [
        "password",
        "PASSWORD",
        "pass-word",
        "access_token",
        "access-token",
        "accessToken",
        "ACCESS.TOKEN",
        "api_key",
        "api-key",
        "apiKey",
        "PROXY_AUTHORIZATION",
    ] {
        assert!(
            redactor.is_sensitive_field(field),
            "field should be sensitive: {field}"
        );
    }
}

#[test]
fn similar_but_safe_field_names_are_not_matched() {
    let redactor = default_redactor();
    let mut value = json!({
        "tokenizer": "visible",
        "secretary": "visible",
        "keyboard": "visible"
    });

    redactor.redact_json(&mut value);

    assert_eq!(value["tokenizer"], "visible");
    assert_eq!(value["secretary"], "visible");
    assert_eq!(value["keyboard"], "visible");
}

#[test]
fn sensitive_container_is_replaced_as_a_whole() {
    let redactor = default_redactor();
    let mut value = json!({
        "password": {
            "nested": "fake-nested-password"
        }
    });

    redactor.redact_json(&mut value);

    assert_eq!(value["password"], REDACTED);
    assert!(!value.to_string().contains("fake-nested-password"));
}

#[test]
fn custom_sensitive_fields_are_supported() {
    let (redactor, _) = custom_redactor(|config| {
        config
            .redaction
            .sensitive_fields
            .insert("credential".to_owned());
    });

    let mut value = json!({
        "user": {
            "credential": "fake-custom-credential"
        }
    });

    redactor.redact_json(&mut value);

    assert_eq!(value["user"]["credential"], REDACTED);
}

#[test]
fn json_depth_limit_truncates_nested_content() {
    let (redactor, _) = custom_redactor(|config| {
        config.limits.max_json_depth = 1;
    });

    let mut value = json!({
        "level_one": {
            "level_two": {
                "content": "must-not-cross-depth-limit"
            }
        }
    });

    let report = redactor.redact_json(&mut value);

    assert_eq!(value["level_one"]["level_two"], TRUNCATED);
    assert_eq!(report.truncated_values, 1);
    assert!(!value.to_string().contains("must-not-cross-depth-limit"));
}

#[test]
fn json_node_limit_is_enforced() {
    let (redactor, _) = custom_redactor(|config| {
        config.limits.max_json_nodes = 3;
    });

    // The root object, `a` and `b` consume the three available nodes.
    let mut value = json!({
        "a": 1,
        "b": 2,
        "c": 3
    });

    redactor.redact_json(&mut value);

    assert_eq!(value["a"], 1);
    assert_eq!(value["b"], 2);
    assert_eq!(value["c"], TRUNCATED);
}

#[test]
fn string_truncation_respects_utf8_and_output_limit() {
    let (redactor, _) = custom_redactor(|config| {
        config.limits.max_string_bytes = 12;
    });

    let mut value = json!({
        "message": "123456789éXYZ"
    });

    redactor.redact_json(&mut value);

    assert_eq!(value["message"], "1[TRUNCATED]");
    assert_eq!(value["message"].as_str().unwrap().len(), 12);
}

#[test]
fn short_strings_are_not_modified() {
    let redactor = default_redactor();
    let mut value = json!({
        "message": "short-value"
    });

    let report = redactor.redact_json(&mut value);

    assert_eq!(value["message"], "short-value");
    assert_eq!(report.truncated_values, 0);
}

#[test]
fn sensitive_headers_are_redacted() {
    let redactor = default_redactor();
    let mut headers = BTreeMap::from([
        (
            "authorization".to_owned(),
            "Bearer fake-header-secret".to_owned(),
        ),
        ("cookie".to_owned(), "session=fake-cookie-secret".to_owned()),
        ("content-type".to_owned(), "application/json".to_owned()),
    ]);

    let report = redactor.redact_headers(&mut headers);

    assert_eq!(headers["authorization"], REDACTED);
    assert_eq!(headers["cookie"], REDACTED);
    assert_eq!(headers["content-type"], "application/json");
    assert_eq!(report.redacted_values, 2);
}

#[test]
fn non_sensitive_header_values_are_bounded() {
    let (redactor, _) = custom_redactor(|config| {
        config.limits.max_header_value_bytes = 12;
    });

    let mut headers = BTreeMap::from([("x-debug-note".to_owned(), "123456789éXYZ".to_owned())]);

    redactor.redact_headers(&mut headers);

    assert_eq!(headers["x-debug-note"], "1[TRUNCATED]");
}

#[test]
fn query_parameters_are_redacted_without_losing_shape() {
    let redactor = default_redactor();
    let mut query = BTreeMap::from([
        (
            "access_token".to_owned(),
            QueryValue::from("fake-query-token"),
        ),
        (
            "tag".to_owned(),
            QueryValue::Array(vec![
                QueryScalar::String("rust".to_owned()),
                QueryScalar::String("telemetry".to_owned()),
            ]),
        ),
        ("expanded".to_owned(), QueryValue::from(true)),
    ]);

    redactor.redact_query(&mut query);

    assert!(query["access_token"] == QueryValue::from(REDACTED));
    assert!(
        query["tag"]
            == QueryValue::Array(vec![
                QueryScalar::String("rust".to_owned()),
                QueryScalar::String("telemetry".to_owned()),
            ])
    );
    assert!(query["expanded"] == QueryValue::from(true));
}

#[test]
fn query_string_limits_are_enforced() {
    let (redactor, _) = custom_redactor(|config| {
        config.limits.max_string_bytes = 12;
    });

    let mut query = BTreeMap::from([("search".to_owned(), QueryValue::from("123456789éXYZ"))]);

    redactor.redact_query(&mut query);

    assert!(query["search"] == QueryValue::from("1[TRUNCATED]"));
}

#[test]
fn redaction_updates_shared_internal_counters() {
    let (redactor, telemetry) = custom_redactor(|config| {
        config.limits.max_string_bytes = 12;
    });

    let mut value = json!({
        "password": "fake-password",
        "nested": {
            "token": "fake-token"
        },
        "message": "123456789éXYZ"
    });

    let report = redactor.redact_json(&mut value);
    let snapshot = telemetry.snapshot();

    assert_eq!(report.redacted_values, 2);
    assert_eq!(report.truncated_values, 1);
    assert_eq!(snapshot.redacted_values, 2);
    assert_eq!(snapshot.truncated_values, 1);
}
