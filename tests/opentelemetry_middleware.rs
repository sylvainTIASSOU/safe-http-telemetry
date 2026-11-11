#![cfg(all(feature = "tower", feature = "opentelemetry"))]

use std::{
    convert::Infallible,
    io::{self, Write},
    sync::{Arc, Mutex},
};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use safe_http_telemetry::{
    core::{event::HttpEvent, telemetry::InternalTelemetry},
    policy::{CaptureProfile, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::{HttpTelemetryLayer, MiddlewareConfig},
};
use tower::{service_fn, Layer as _, Service as _, ServiceExt as _};
use tracing_subscriber::{fmt::MakeWriter, prelude::*};

#[cfg(feature = "loki")]
use safe_http_telemetry::{config::ServiceConfig, loki::payload::LokiStream};

const TRACE_ID: &str = "4bf92f3577b34da6a3ce929d0e0e4736";
const PARENT_SPAN_ID: &str = "00f067aa0ba902b7";
const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

#[tokio::test]
async fn incoming_w3c_trace_is_shared_by_the_span_event_and_log() {
    let mut headers = HeaderMap::new();
    headers.insert("traceparent", HeaderValue::from_static(TRACEPARENT));

    let (event, logs) = run_with_opentelemetry(headers).await;

    assert_eq!(event.trace_id.as_deref(), Some(TRACE_ID));

    let span_id = event
        .span_id
        .as_deref()
        .expect("the HTTP span must have an OpenTelemetry span ID");
    assert_eq!(span_id.len(), 16);
    assert_ne!(span_id, PARENT_SPAN_ID);

    assert!(logs.contains(TRACE_ID));
    assert!(logs.contains(span_id));
    assert!(logs.contains(&event.request_id));

    // Raw paths and bodies are not added to span or summary-log attributes.
    assert!(!logs.contains("/users/42"));
    assert!(event.request.body.is_none());
    assert!(event.response.body.is_none());
}

#[tokio::test]
async fn local_span_generates_valid_correlation_without_incoming_context() {
    let (event, _) = run_with_opentelemetry(HeaderMap::new()).await;

    let trace_id = event
        .trace_id
        .as_deref()
        .expect("a local trace ID must be generated");
    let span_id = event
        .span_id
        .as_deref()
        .expect("a local span ID must be generated");

    assert_eq!(trace_id.len(), 32);
    assert_eq!(span_id.len(), 16);
    assert!(trace_id
        .chars()
        .all(|character| character.is_ascii_hexdigit()));
    assert!(span_id
        .chars()
        .all(|character| character.is_ascii_hexdigit()));
    assert_ne!(trace_id, "00000000000000000000000000000000");
    assert_ne!(span_id, "0000000000000000");
}

#[tokio::test]
async fn invalid_incoming_context_is_not_respected() {
    let mut headers = HeaderMap::new();
    headers.insert(
        "traceparent",
        HeaderValue::from_static("00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01"),
    );

    let (event, _) = run_with_opentelemetry(headers).await;

    assert!(event.trace_id.is_some());
    assert_ne!(event.trace_id.as_deref(), Some(TRACE_ID));
}

#[tokio::test]
async fn correlation_remains_absent_without_opentelemetry_layer() {
    let mut headers = HeaderMap::new();
    headers.insert("traceparent", HeaderValue::from_static(TRACEPARENT));

    let event = run_without_opentelemetry(headers).await;

    assert!(event.trace_id.is_none());
    assert!(event.span_id.is_none());
}

#[cfg(feature = "loki")]
#[tokio::test]
async fn correlation_is_exported_as_metadata_not_loki_labels() {
    let mut headers = HeaderMap::new();
    headers.insert("traceparent", HeaderValue::from_static(TRACEPARENT));

    let (event, _) = run_with_opentelemetry(headers).await;
    let service = ServiceConfig {
        name: "correlation-test".to_owned(),
        environment: "test".to_owned(),
        version: "0.1.0".to_owned(),
        region: None,
    };

    let stream = LokiStream::from_event(&event, &service, 1).unwrap();
    assert!(
        event.trace_id.is_some(),
        "the exported event must contain a trace ID"
    );
    assert!(
        event.span_id.is_some(),
        "the exported event must contain a span ID"
    );

    assert_eq!(
        stream.metadata.get("trace_id").map(String::as_str),
        event.trace_id.as_deref()
    );
    assert_eq!(
        stream.metadata.get("span_id").map(String::as_str),
        event.span_id.as_deref()
    );

    assert!(!stream.stream.contains_key("trace_id"));
    assert!(!stream.stream.contains_key("span_id"));
    assert!(!stream.stream.contains_key("request_id"));

    let line: serde_json::Value = serde_json::from_str(&stream.values[0][1]).unwrap();
    assert_eq!(line["trace_id"].as_str(), event.trace_id.as_deref());
    assert_eq!(line["span_id"].as_str(), event.span_id.as_deref());
}

async fn run_with_opentelemetry(headers: HeaderMap) -> (HttpEvent, String) {
    let output = SharedWriter::default();
    let provider = SdkTracerProvider::default();
    let tracer = provider.tracer("http-middleware-test");

    let subscriber = tracing_subscriber::Registry::default()
        .with(tracing_opentelemetry::layer().with_tracer(tracer))
        .with(tracing_subscriber::fmt::layer().with_writer(output.clone()));

    let _subscriber_guard = tracing::subscriber::set_default(subscriber);
    let event = observed_request(headers).await;

    (event, output.contents())
}

async fn run_without_opentelemetry(headers: HeaderMap) -> HttpEvent {
    let subscriber = tracing_subscriber::Registry::default();
    let _subscriber_guard = tracing::subscriber::set_default(subscriber);

    observed_request(headers).await
}

async fn observed_request(headers: HeaderMap) -> HttpEvent {
    let queue = BoundedEventQueue::new(QueueConfig::new(8), InternalTelemetry::new()).unwrap();

    let layer = HttpTelemetryLayer::new(
        queue.clone(),
        RoutePolicySet::new(CaptureProfile::Minimal),
        MiddlewareConfig::default(),
    )
    .unwrap();

    let mut service = layer.layer(service_fn(|_request| async {
        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"ok"))))
    }));

    let mut request = Request::builder()
        .method("GET")
        .uri("/users/42")
        .body(Full::new(Bytes::new()))
        .unwrap();
    *request.headers_mut() = headers;

    let response = service.ready().await.unwrap().call(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body.as_ref(), b"ok");

    queue
        .receiver()
        .try_recv()
        .expect("one completed HTTP event must be queued")
}

#[derive(Clone, Default)]
struct SharedWriter {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl SharedWriter {
    fn contents(&self) -> String {
        let bytes = self.buffer.lock().unwrap().clone();
        String::from_utf8(bytes).unwrap()
    }
}

impl<'a> MakeWriter<'a> for SharedWriter {
    type Writer = SharedWriterGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SharedWriterGuard {
            buffer: Arc::clone(&self.buffer),
        }
    }
}

struct SharedWriterGuard {
    buffer: Arc<Mutex<Vec<u8>>>,
}

impl Write for SharedWriterGuard {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.buffer.lock().unwrap().extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
