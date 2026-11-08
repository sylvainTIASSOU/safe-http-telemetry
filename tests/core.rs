use safe_http_telemetry::core::{
    event::{
        BodyCaptureStatus, CaptureInfo, HttpEvent, LogLevel, QueryScalar, QueryValue, RequestInfo,
        ResponseInfo, ResponseOutcome, TimingInfo, HTTP_EVENT_NAME, HTTP_EVENT_SCHEMA_VERSION,
    },
    telemetry::InternalTelemetry,
};
use serde_json::{json, Value};

fn sample_event() -> HttpEvent {
    let mut request = RequestInfo::new("POST", "/api/users/{id}", "/api/users/42");

    request
        .query
        .insert("expanded".to_owned(), QueryValue::from(true));

    request.query.insert(
        "tag".to_owned(),
        QueryValue::Array(vec![
            QueryScalar::String("rust".to_owned()),
            QueryScalar::String("telemetry".to_owned()),
        ]),
    );

    request
        .headers
        .insert("content-type".to_owned(), "application/json".to_owned());

    request.body = Some(json!({
        "name": "example-user",
        "password": "[REDACTED]"
    }));

    request.content_type = Some("application/json".to_owned());
    request.content_length = Some(58);

    let mut response = ResponseInfo::new(201);
    response.body = Some(json!({
        "id": 42,
        "access_token": "[REDACTED]"
    }));

    let capture = CaptureInfo {
        request_body_truncated: false,
        response_body_truncated: false,
        request_body_status: Some(BodyCaptureStatus::Captured),
        response_body_status: Some(BodyCaptureStatus::Captured),
    };

    HttpEvent::new(
        "request-test-0001",
        request,
        response,
        TimingInfo::new(12.5),
        capture,
    )
}

#[test]
fn event_uses_schema_v1_identity() {
    let event = sample_event();

    assert_eq!(event.schema_version, HTTP_EVENT_SCHEMA_VERSION);
    assert_eq!(event.event, HTTP_EVENT_NAME);
    assert_eq!(event.level, LogLevel::Info);
}

#[test]
fn event_serializes_numeric_status_and_duration() {
    let value = serde_json::to_value(sample_event()).unwrap();

    assert_eq!(value["schema_version"], "1.0");
    assert_eq!(value["event"], "http.request.completed");
    assert_eq!(value["response"]["status_code"], 201);
    assert_eq!(value["timing"]["duration_ms"], 12.5);
    assert_eq!(value["request"]["query"]["expanded"], true);
    assert_eq!(
        value["request"]["query"]["tag"],
        json!(["rust", "telemetry"])
    );
}

#[test]
fn absent_correlation_ids_are_not_serialized() {
    let value = serde_json::to_value(sample_event()).unwrap();

    assert!(!value.as_object().unwrap().contains_key("trace_id"));
    assert!(!value.as_object().unwrap().contains_key("span_id"));
}

#[test]
fn correlation_ids_are_serialized_when_present() {
    let mut event = sample_event();
    event.trace_id = Some("4bf92f3577b34da6a3ce929d0e0e4736".to_owned());
    event.span_id = Some("00f067aa0ba902b7".to_owned());

    let value = serde_json::to_value(event).unwrap();

    assert_eq!(value["trace_id"], "4bf92f3577b34da6a3ce929d0e0e4736");
    assert_eq!(value["span_id"], "00f067aa0ba902b7");
}

#[test]
fn normalized_route_is_separate_from_raw_path() {
    let value = serde_json::to_value(sample_event()).unwrap();

    assert_eq!(value["request"]["route"], "/api/users/{id}");
    assert_eq!(value["request"]["path"], "/api/users/42");
}

#[test]
fn successful_response_has_success_outcome() {
    let response = ResponseInfo::new(204);

    assert_eq!(response.outcome, Some(ResponseOutcome::Success));
}

#[test]
fn status_controls_level_and_outcome() {
    let client_error = ResponseInfo::new(404);
    let server_error = ResponseInfo::new(503);

    assert_eq!(client_error.outcome, Some(ResponseOutcome::ClientError));
    assert_eq!(LogLevel::from_status(404), LogLevel::Warn);

    assert_eq!(server_error.outcome, Some(ResponseOutcome::ServerError));
    assert_eq!(LogLevel::from_status(503), LogLevel::Error);
}

#[test]
fn capture_status_uses_schema_v1_names() {
    let value = serde_json::to_value(sample_event()).unwrap();

    assert_eq!(value["capture"]["request_body_status"], "captured");
    assert_eq!(value["capture"]["response_body_status"], "captured");
    assert_eq!(
        value["capture"]["request_body_truncated"],
        Value::Bool(false)
    );
}

#[test]
fn cloned_telemetry_handles_share_counters() {
    let telemetry = InternalTelemetry::new();
    let cloned = telemetry.clone();

    telemetry.record_redactions(3);
    cloned.record_redactions(2);
    telemetry.record_truncations(4);
    cloned.record_body_omission();
    telemetry.record_parse_error();
    cloned.record_policy_denial();

    let snapshot = telemetry.snapshot();

    assert_eq!(snapshot.redacted_values, 5);
    assert_eq!(snapshot.truncated_values, 4);
    assert_eq!(snapshot.omitted_bodies, 1);
    assert_eq!(snapshot.parse_errors, 1);
    assert_eq!(snapshot.rejected_by_policy, 1);
}

#[test]
fn event_round_trips_through_json() {
    let event = sample_event();

    let encoded = serde_json::to_string(&event).unwrap();
    let decoded: HttpEvent = serde_json::from_str(&encoded).unwrap();

    assert!(decoded == event);
}
