use safe_http_telemetry::{
    core::{
        event::{
            BusinessEvent, CaptureInfo, HttpEvent, LogLevel, RequestInfo, ResponseInfo,
            TelemetryEvent, TimingInfo,
        },
        telemetry::InternalTelemetry,
    },
    runtime::queue::{BoundedEventQueue, QueueConfig},
};
use serde_json::json;

fn sample_business_event(message: &str) -> BusinessEvent {
    BusinessEvent::new(LogLevel::Info, Some(message.to_owned()), "orders::service")
}

fn sample_http_event(request_id: &str) -> HttpEvent {
    HttpEvent::new(
        request_id,
        RequestInfo::new("GET", "/orders/{id}", "/orders/42"),
        ResponseInfo::new(200),
        TimingInfo::new(1.0),
        CaptureInfo::default(),
    )
}

#[test]
fn business_event_has_a_distinct_versioned_json_schema() {
    let mut event = sample_business_event("Order accepted");
    event.request_id = Some("request-business-0001".to_owned());
    event.fields.insert("attempt".to_owned(), json!(1));

    let value = serde_json::to_value(&event).unwrap();

    assert_eq!(value["schema_version"], "1.0");
    assert_eq!(value["event"], "business.event");
    assert_eq!(value["level"], "info");
    assert_eq!(value["message"], "Order accepted");
    assert_eq!(value["request_id"], "request-business-0001");
    assert_eq!(value["target"], "orders::service");
    assert_eq!(value["fields"]["attempt"], 1);
    assert!(value.get("trace_id").is_none());
    assert!(value.get("span_id").is_none());
    assert!(value.get("request").is_none());
    assert!(value.get("response").is_none());
}

#[test]
fn bounded_queue_accepts_http_and_business_events() {
    let queue = BoundedEventQueue::new(QueueConfig::new(2), InternalTelemetry::new()).unwrap();
    let receiver = queue.receiver();

    queue
        .try_push(sample_http_event("request-mixed-0001"))
        .unwrap();
    queue
        .try_push(sample_business_event("Order accepted"))
        .unwrap();

    match receiver.try_recv().unwrap() {
        TelemetryEvent::Http(event) => {
            assert_eq!(event.request_id, "request-mixed-0001");
        }
        TelemetryEvent::Business(_) => {
            panic!("the HTTP event must be received first");
        }
    }

    match receiver.try_recv().unwrap() {
        TelemetryEvent::Business(event) => {
            assert_eq!(event.message.as_deref(), Some("Order accepted"));
        }
        TelemetryEvent::Http(_) => {
            panic!("the business event must be received second");
        }
    }

    assert_eq!(queue.stats().accepted_events, 2);
    assert_eq!(queue.stats().occupancy, 0);
}

#[cfg(feature = "loki")]
#[test]
fn business_loki_payload_preserves_correlation_without_labels() {
    use safe_http_telemetry::{config::ServiceConfig, loki::payload::LokiStream};

    let mut event = sample_business_event("Order accepted");
    event.request_id = Some("request-business-loki".to_owned());
    event.trace_id = Some("4bf92f3577b34da6a3ce929d0e0e4736".to_owned());
    event.span_id = Some("00f067aa0ba902b7".to_owned());
    event.fields.insert("attempt".to_owned(), json!(1));

    let service = ServiceConfig {
        name: "orders-api".to_owned(),
        environment: "test".to_owned(),
        version: "0.1.0".to_owned(),
        region: None,
    };

    let stream =
        LokiStream::from_business_event(&event, &service, 1_730_000_000_000_000_001).unwrap();

    assert_eq!(stream.stream["event_type"], "business");
    assert_eq!(stream.metadata["request_id"], "request-business-loki");
    assert_eq!(
        stream.metadata["trace_id"],
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
    assert_eq!(stream.metadata["span_id"], "00f067aa0ba902b7");

    for forbidden in ["request_id", "trace_id", "span_id", "message", "fields"] {
        assert!(!stream.stream.contains_key(forbidden));
    }

    let decoded: BusinessEvent = serde_json::from_str(&stream.values[0][1]).unwrap();
    assert!(decoded == event);
}
