#![cfg(all(feature = "tracing", feature = "opentelemetry"))]

use http::HeaderMap;
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use safe_http_telemetry::{
    config::{CaptureLimits, RedactionConfig},
    core::{
        event::{BusinessEvent, TelemetryEvent},
        telemetry::InternalTelemetry,
    },
    redaction::Redactor,
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tracing_integration::{
        business::{BusinessLogConfig, BusinessLogLayer},
        propagation::{extract_trace_context, inject_trace_context, TraceCorrelation},
    },
};
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use tracing_subscriber::prelude::*;

struct SimulatedService {
    event: BusinessEvent,
    server: TraceCorrelation,
    operation: TraceCorrelation,
    outgoing: HeaderMap,
}

fn run_service(incoming: Option<HeaderMap>, request_id: &str) -> SimulatedService {
    let queue = BoundedEventQueue::new(QueueConfig::new(4), InternalTelemetry::new()).unwrap();

    let redactor = Redactor::new(
        &RedactionConfig::default(),
        &CaptureLimits::default(),
        queue.telemetry(),
    )
    .unwrap();

    let business =
        BusinessLogLayer::new(queue.clone(), redactor, BusinessLogConfig::default()).unwrap();

    let provider = SdkTracerProvider::default();
    let tracer = provider.tracer("distributed-correlation-test");

    let subscriber = tracing_subscriber::Registry::default()
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(business);

    let (server, operation, outgoing) = tracing::subscriber::with_default(subscriber, || {
        let server = tracing::info_span!("http.request", request_id = request_id);

        if let Some(headers) = incoming.as_ref() {
            if let Some(parent) = extract_trace_context(headers) {
                let _ = server.set_parent(parent);
            }
        }

        let server_correlation = TraceCorrelation::from_span(&server).unwrap();
        let mut outgoing = HeaderMap::new();

        let operation_correlation = {
            let _server_entered = server.enter();

            let operation = tracing::info_span!("business.operation");
            let operation_correlation = TraceCorrelation::from_span(&operation).unwrap();

            let _operation_entered = operation.enter();

            tracing::info!(order_id = 42_u64, "Order processed");

            inject_trace_context(&Span::current().context(), &mut outgoing);

            operation_correlation
        };

        (server_correlation, operation_correlation, outgoing)
    });

    let event = match queue
        .receiver()
        .try_recv()
        .expect("one correlated business event")
    {
        TelemetryEvent::Business(event) => *event,
        TelemetryEvent::Http(_) => {
            panic!("expected a business event")
        }
    };

    SimulatedService {
        event,
        server,
        operation,
        outgoing,
    }
}

#[test]
fn correlation_is_preserved_across_spans_and_services() {
    let service_a = run_service(None, "request-service-a");
    let service_b = run_service(Some(service_a.outgoing.clone()), "request-service-b");

    // Each business event uses the IDs of its current operation span.
    assert_eq!(
        service_a.event.trace_id.as_deref(),
        Some(service_a.operation.trace_id.as_str())
    );
    assert_eq!(
        service_a.event.span_id.as_deref(),
        Some(service_a.operation.span_id.as_str())
    );
    assert_eq!(
        service_b.event.trace_id.as_deref(),
        Some(service_b.operation.trace_id.as_str())
    );
    assert_eq!(
        service_b.event.span_id.as_deref(),
        Some(service_b.operation.span_id.as_str())
    );

    // The server and business operation share a trace but not a span ID.
    assert_eq!(service_a.server.trace_id, service_a.operation.trace_id);
    assert_ne!(service_a.server.span_id, service_a.operation.span_id);
    assert_eq!(service_b.server.trace_id, service_b.operation.trace_id);
    assert_ne!(service_b.server.span_id, service_b.operation.span_id);

    // The outgoing W3C context points to the current business operation.
    let traceparent = service_a
        .outgoing
        .get("traceparent")
        .expect("outgoing traceparent")
        .to_str()
        .unwrap();
    let mut parts = traceparent.split('-');

    assert_eq!(parts.next(), Some("00"));
    assert_eq!(parts.next(), Some(service_a.operation.trace_id.as_str()));
    assert_eq!(parts.next(), Some(service_a.operation.span_id.as_str()));

    // Service B respects the incoming distributed trace and creates new spans.
    assert_eq!(service_b.server.trace_id, service_a.operation.trace_id);
    assert_eq!(
        service_b.event.trace_id.as_deref(),
        Some(service_a.event.trace_id.as_deref().unwrap())
    );
    assert_ne!(service_b.server.span_id, service_a.operation.span_id);

    // Request correlation remains local to each HTTP request.
    assert_eq!(
        service_a.event.request_id.as_deref(),
        Some("request-service-a")
    );
    assert_eq!(
        service_b.event.request_id.as_deref(),
        Some("request-service-b")
    );
    assert_ne!(service_a.event.request_id, service_b.event.request_id);
}
