# Grafana and OpenTelemetry correlation

The crate sends logs to Loki. Trace export is performed by the OpenTelemetry
pipeline selected by the application, typically through an OTLP exporter and
an OpenTelemetry Collector. Loki is not a trace backend.

## Correlation identifiers

- `request_id` identifies one HTTP request in one service.
- `trace_id` identifies a distributed trace across operations and services.
- `span_id` identifies one operation inside that trace.

A business event emitted inside a child span uses the child `span_id` and keeps
the HTTP server span's `trace_id`.

## Subscriber wiring

Build the redactor and business layer with the same bounded queue used by the
HTTP middleware and Loki exporter:

```rust
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use safe_http_telemetry::{
    config::{CaptureLimits, RedactionConfig},
    redaction::Redactor,
    tracing_integration::business::{BusinessLogConfig, BusinessLogLayer},
};
use tracing_subscriber::prelude::*;

let redactor = Redactor::new(
    &RedactionConfig::default(),
    &CaptureLimits::default(),
    queue.telemetry(),
)?;
let business = BusinessLogLayer::new(
    queue.clone(),
    redactor,
    BusinessLogConfig::default(),
)?;

let provider = SdkTracerProvider::default();
let tracer = provider.tracer("my-application");

tracing_subscriber::registry()
    .with(tracing_subscriber::filter::LevelFilter::INFO)
    .with(tracing_subscriber::fmt::layer())
    .with(tracing_opentelemetry::layer().with_tracer(tracer))
    .with(business)
    .try_init()?;
```
SdkTracerProvider::default() generates valid IDs but does not export spans.
To send traces to Tempo, Jaeger or another backend, configure the provider with
the application's chosen OTLP exporter. Keep the OpenTelemetry crate versions
compatible with that exporter.
Stop the HTTP server first, shut down the application's tracer provider, and
then drain the Loki exporter within its configured deadline.
Emitting correlated business events
Use ordinary tracing macros:
rust
tracing::info!(
    order_id = "fictitious-order-001",
    attempt = 1_u64,
    "Order accepted"
);
The business layer converts supported fields to JSON, redacts sensitive fields,
inherits request_id from the current HTTP span and obtains OpenTelemetry IDs
from the current operation span.
The event is submitted with try_push. A full queue drops the incoming event
and updates the existing saturation counters. No Loki request is performed by
on_event.
Apply tracing_subscriber filters when only selected targets or levels should
enter this layer. Do not place credentials, bodies or unclassified sensitive
values in event messages.
Propagating W3C Trace Context
For an outgoing instrumented HTTP call, inject the current context into its
headers:
rust
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use safe_http_telemetry::tracing_integration::propagation::inject_trace_context;

let mut headers = http::HeaderMap::new();
inject_trace_context(&Span::current().context(), &mut headers);
The receiving service must extract traceparent and set the resulting context
as its server span parent. The provided Tower middleware does this when the
opentelemetry feature is enabled.
Do not copy trace headers into business fields or log them unnecessarily.
Loki labels and metadata
Controlled indexed labels include:
service;
environment;
version;
optional region;
event_type, with value http or business.
The following values remain in the JSON line or structured metadata:
request_id;
trace_id;
span_id;
normalized route;
business fields.
Do not turn IDs, IPs, paths, messages, bodies or user identifiers into indexed
Loki labels.
Example LogQL query:
logql
{service="safe-http-telemetry-example"}
  | json
  | trace_id="4bf92f3577b34da6a3ce929d0e0e4736"
To retrieve only business events:
logql
{service="safe-http-telemetry-example",event_type="business"}
Opening a trace from Grafana
Configure a derived field on the Loki data source:
name: traceID;
regular expression: "trace_id":"([0-9a-f]+)";
internal link: the configured Tempo or other tracing data source.
The trace backend must receive spans with the same OpenTelemetry trace IDs for
the link to resolve. Missing traces can indicate disabled trace export,
sampling, exporter failure or retention differences between logs and traces.
