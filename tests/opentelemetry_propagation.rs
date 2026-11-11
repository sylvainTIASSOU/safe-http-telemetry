#![cfg(feature = "opentelemetry")]

use http::{HeaderMap, HeaderValue};
use opentelemetry::{
    trace::{TraceContextExt, TracerProvider as _},
    Context,
};
use opentelemetry_sdk::trace::SdkTracerProvider;
use safe_http_telemetry::tracing_integration::propagation::{
    extract_trace_context, inject_trace_context, TraceCorrelation,
};
use tracing_subscriber::prelude::*;

const TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const PARENT_SPAN_ID: &str = "00f067aa0ba902b7";
const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

#[test]
fn valid_w3c_context_is_extracted_as_remote() {
    let mut headers = HeaderMap::new();
    headers.insert("traceparent", HeaderValue::from_static(TRACEPARENT));
    headers.insert(
        "tracestate",
        HeaderValue::from_static("vendor=fictitious-value"),
    );

    let context = extract_trace_context(&headers).expect("the valid W3C context must be accepted");
    let span_context = context.span().span_context().clone();

    assert!(span_context.is_valid());
    assert!(span_context.is_remote());
    assert_eq!(span_context.trace_id().to_string(), TRACE_ID);
    assert_eq!(span_context.span_id().to_string(), PARENT_SPAN_ID);
}

#[test]
fn missing_or_invalid_w3c_context_is_ignored() {
    assert!(extract_trace_context(&HeaderMap::new()).is_none());

    let invalid_values = [
        "invalid",
        "00-00000000000000000000000000000000-00f067aa0ba902b7-01",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01",
        "ff-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
    ];

    for value in invalid_values {
        let mut headers = HeaderMap::new();
        headers.insert("traceparent", HeaderValue::from_static(value));

        assert!(extract_trace_context(&headers).is_none());
    }
}

#[test]
fn w3c_context_can_be_injected_for_an_outgoing_request() {
    let mut incoming = HeaderMap::new();
    incoming.insert("traceparent", HeaderValue::from_static(TRACEPARENT));
    incoming.insert(
        "tracestate",
        HeaderValue::from_static("vendor=fictitious-value"),
    );

    let context = extract_trace_context(&incoming).expect("the test context must be valid");
    let mut outgoing = HeaderMap::new();

    inject_trace_context(&context, &mut outgoing);

    assert_eq!(
        outgoing
            .get("traceparent")
            .and_then(|value| value.to_str().ok()),
        Some(TRACEPARENT)
    );
    assert_eq!(
        outgoing
            .get("tracestate")
            .and_then(|value| value.to_str().ok()),
        Some("vendor=fictitious-value")
    );
}

#[test]
fn correlation_reads_the_current_opentelemetry_span_ids() {
    let provider = SdkTracerProvider::default();
    let tracer = provider.tracer("correlation-test");
    let subscriber = tracing_subscriber::Registry::default()
        .with(tracing_opentelemetry::layer().with_tracer(tracer));

    tracing::subscriber::with_default(subscriber, || {
        let span = tracing::info_span!("test.request");
        let correlation = TraceCorrelation::from_span(&span)
            .expect("the OpenTelemetry layer must provide valid IDs");

        assert_eq!(correlation.trace_id.len(), 32);
        assert_eq!(correlation.span_id.len(), 16);
        assert!(correlation
            .trace_id
            .chars()
            .all(|character| character.is_ascii_hexdigit()));
        assert!(correlation
            .span_id
            .chars()
            .all(|character| character.is_ascii_hexdigit()));
        assert!(correlation.trace_id != "00000000000000000000000000000000");
        assert!(correlation.span_id != "0000000000000000");
    });
}

#[test]
fn correlation_is_absent_without_an_opentelemetry_span() {
    let span = tracing::Span::none();
    assert!(TraceCorrelation::from_span(&span).is_none());

    let context = Context::new();
    assert!(!context.span().span_context().is_valid());
}
