#![cfg(feature = "tracing")]

use safe_http_telemetry::{
    config::{CaptureLimits, RedactionConfig},
    core::{
        event::{BusinessEvent, LogLevel, TelemetryEvent},
        telemetry::InternalTelemetry,
    },
    redaction::{Redactor, REDACTED},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tracing_integration::business::{BusinessLogConfig, BusinessLogLayer},
};
use tracing_subscriber::prelude::*;

fn queue(capacity: usize) -> BoundedEventQueue {
    BoundedEventQueue::new(QueueConfig::new(capacity), InternalTelemetry::new()).unwrap()
}

fn business_layer(queue: BoundedEventQueue, config: BusinessLogConfig) -> BusinessLogLayer {
    let redactor = Redactor::new(
        &RedactionConfig::default(),
        &CaptureLimits::default(),
        queue.telemetry(),
    )
    .unwrap();

    BusinessLogLayer::new(queue, redactor, config).unwrap()
}

fn receive_business(queue: &BoundedEventQueue) -> BusinessEvent {
    match queue.receiver().try_recv().expect("one business event") {
        TelemetryEvent::Business(event) => *event,
        TelemetryEvent::Http(_) => {
            panic!("expected a business event")
        }
    }
}

#[test]
fn native_event_is_converted_redacted_and_queued() {
    const FICTITIOUS_SECRET: &str = "fictitious-business-secret-do-not-use";

    let queue = queue(4);
    let layer = business_layer(queue.clone(), BusinessLogConfig::default());
    let subscriber = tracing_subscriber::Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(
            order_id = 42_u64,
            active = true,
            ratio = 1.5_f64,
            password = FICTITIOUS_SECRET,
            "Order accepted"
        );
    });

    let event = receive_business(&queue);

    assert_eq!(event.schema_version, "1.0");
    assert_eq!(event.event, "business.event");
    assert_eq!(event.level, LogLevel::Info);
    assert_eq!(event.message.as_deref(), Some("Order accepted"));
    assert_eq!(event.fields["order_id"], 42);
    assert_eq!(event.fields["active"], true);
    assert_eq!(event.fields["ratio"], 1.5);
    assert_eq!(event.fields["password"], REDACTED);
    assert!(!event.fields_truncated);

    let serialized = serde_json::to_string(&event).unwrap();
    assert!(!serialized.contains(FICTITIOUS_SECRET));
}

#[test]
fn request_id_is_inherited_from_the_current_http_span() {
    let queue = queue(2);
    let layer = business_layer(queue.clone(), BusinessLogConfig::default());
    let subscriber = tracing_subscriber::Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("http.request", request_id = "request-business-span-0001");
        let _entered = span.enter();

        tracing::warn!(
            request_id = "conflicting-event-request-id",
            "Inventory unavailable"
        );
    });

    let event = receive_business(&queue);

    assert_eq!(event.level, LogLevel::Warn);
    assert_eq!(
        event.request_id.as_deref(),
        Some("request-business-span-0001")
    );
    assert!(!event.fields.contains_key("request_id"));
}

#[test]
fn field_limit_is_bounded_and_reported() {
    let queue = queue(2);
    let layer = business_layer(
        queue.clone(),
        BusinessLogConfig {
            max_fields: 1,
            include_module_path: false,
        },
    );
    let subscriber = tracing_subscriber::Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!(first = 1, second = 2, third = 3, "Limited fields");
    });

    let event = receive_business(&queue);

    assert_eq!(event.fields.len(), 1);
    assert!(event.fields_truncated);
}

#[test]
fn queue_saturation_drops_the_newest_business_event() {
    let queue = queue(1);
    let layer = business_layer(queue.clone(), BusinessLogConfig::default());
    let subscriber = tracing_subscriber::Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::info!("First event");
        tracing::info!("Second event");
    });

    let event = receive_business(&queue);
    assert_eq!(event.message.as_deref(), Some("First event"));

    let stats = queue.stats();
    assert_eq!(stats.accepted_events, 1);
    assert_eq!(stats.rejected_events, 1);
    assert_eq!(stats.dropped_events, 1);
    assert_eq!(stats.occupancy, 0);
}

#[test]
fn internal_crate_events_are_not_captured_recursively() {
    let queue = queue(2);
    let layer = business_layer(queue.clone(), BusinessLogConfig::default());
    let subscriber = tracing_subscriber::Registry::default().with(layer);

    tracing::subscriber::with_default(subscriber, || {
        tracing::event!(
            target: "safe_http_telemetry::tower_integration",
            tracing::Level::INFO,
            "HTTP request completed"
        );
    });

    assert!(queue.receiver().try_recv().is_none());
}

#[cfg(feature = "opentelemetry")]
#[test]
fn business_event_uses_the_current_opentelemetry_span_ids() {
    use opentelemetry::trace::TracerProvider;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use safe_http_telemetry::tracing_integration::propagation::TraceCorrelation;

    let queue = queue(2);
    let provider = SdkTracerProvider::default();
    let tracer = provider.tracer("business-layer-test");
    let business = business_layer(queue.clone(), BusinessLogConfig::default());

    let subscriber = tracing_subscriber::Registry::default()
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(business);

    let expected = tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("order.process");
        let correlation = TraceCorrelation::from_span(&span).unwrap();
        let _entered = span.enter();

        tracing::info!(order_id = 42_u64, "Order processed");

        correlation
    });

    let event = receive_business(&queue);

    assert_eq!(event.trace_id.as_deref(), Some(expected.trace_id.as_str()));
    assert_eq!(event.span_id.as_deref(), Some(expected.span_id.as_str()));
}

#[cfg(feature = "opentelemetry")]
#[test]
fn entering_the_same_span_twice_preserves_correlation_without_panicking() {
    use opentelemetry::trace::TracerProvider;
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use safe_http_telemetry::tracing_integration::propagation::TraceCorrelation;

    let queue = queue(4);
    let provider = SdkTracerProvider::default();
    let tracer = provider.tracer("repeated-span-enter-test");
    let business = business_layer(queue.clone(), BusinessLogConfig::default());

    let subscriber = tracing_subscriber::Registry::default()
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(business);

    let expected = tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("http.request", request_id = "request-repeated-enter");
        let correlation = TraceCorrelation::from_span(&span).unwrap();

        {
            let _entered = span.enter();
            tracing::info!("First poll");
        }

        {
            let _entered = span.enter();
            tracing::info!("Second poll");
        }

        correlation
    });

    let first = receive_business(&queue);
    let second = receive_business(&queue);

    for event in [&first, &second] {
        assert_eq!(event.request_id.as_deref(), Some("request-repeated-enter"));
        assert_eq!(event.trace_id.as_deref(), Some(expected.trace_id.as_str()));
        assert_eq!(event.span_id.as_deref(), Some(expected.span_id.as_str()));
    }

    assert_eq!(first.message.as_deref(), Some("First poll"));
    assert_eq!(second.message.as_deref(), Some("Second poll"));
}
