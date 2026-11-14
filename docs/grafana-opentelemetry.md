# Grafana and OpenTelemetry correlation

The crate exports logs to Loki. With `opentelemetry` enabled, it can correlate HTTP and business events with W3C trace IDs. **It does not configure a span exporter for your application.** To open a trace in Grafana, configure a compatible OpenTelemetry exporter and trace backend such as Tempo separately.

## Identifiers

- `request_id` identifies one HTTP request in one service.
- `trace_id` follows a distributed trace across services.
- `span_id` identifies one operation within that trace.

A business event emitted inside a child span keeps the trace ID and uses the child's span ID. The Axum example accepts an incoming `traceparent` header and emits a business event in the request context.

## Subscriber wiring

Construct the business layer with the same queue used by HTTP middleware and the Loki exporter:

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
    queue.clone(), redactor, BusinessLogConfig::default(),
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

This is an integration fragment; see the [runnable Axum/Loki example](../examples/axum_loki.rs) for full setup and shutdown. `SdkTracerProvider::default()` generates IDs but does not export spans. Supply an OTLP exporter compatible with your OpenTelemetry versions when traces must reach Tempo, Jaeger, or another backend.

Ordinary `tracing` macros produce application events:

```rust
tracing::info!(order_id = "example-order-001", attempt = 1_u64, "Order accepted");
```

The business layer redacts supported fields and submits them to the bounded queue. A full queue drops the new event and updates counters. Do not place secrets or personal data in the free-form message; redaction is based on configured field names and supported structures.

For outgoing instrumented HTTP calls, inject the current context into headers:

```rust
use tracing::Span;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use safe_http_telemetry::tracing_integration::propagation::inject_trace_context;

let mut headers = http::HeaderMap::new();
inject_trace_context(&Span::current().context(), &mut headers);
```

## Loki and Grafana

Indexed Loki labels are limited to stable service, environment, version, optional region, and event type values. Request IDs, trace IDs, IPs, paths, messages, and body fields are not labels. Query the JSON line by trace ID:

```logql
{service="safe-http-telemetry-example"} | json | trace_id="4bf92f3577b34da6a3ce929d0e0e4736"
```

To find only business events:

```logql
{service="safe-http-telemetry-example",event_type="business"}
```

Grafana can link a Loki trace ID to a Tempo data source using a derived field that extracts `trace_id` from the log line. The link resolves only if the separate trace backend received spans with that ID and still retains them. See the [examples guide](../examples/README.md) for a local query against real Loki.

After HTTP serving stops, shut down the application's tracer provider, then drain the Loki exporter. Inspect the exporter's shutdown report for abandoned events.
