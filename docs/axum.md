# Axum integration

The `axum` feature enables the Axum adapter and the underlying `tower` feature. Apply the telemetry layer after registering routes so Axum's `MatchedPath` supplies stable route templates such as `/users/{id}`. The middleware records HTTP lifecycle events; it does not send a Loki request from the application request path.

## Build the queue and router

The following wiring uses a minimal profile and excludes health checks. A queue needs a consumer: the [minimal example](../examples/minimal_axum.rs) prints events, and the [Loki example](../examples/axum_loki.rs) starts an exporter.

```rust
use axum::{routing::get, Router};
use safe_http_telemetry::{
    axum_adapter::axum_telemetry_layer,
    core::telemetry::InternalTelemetry,
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::MiddlewareConfig,
};

let queue = BoundedEventQueue::new(QueueConfig::new(1_024), InternalTelemetry::new())?;
let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);
policies.add_policy(
    RoutePolicy::new(RoutePattern::exact("/health"), CaptureProfile::Minimal).exclude(),
)?;
let layer = axum_telemetry_layer(queue.clone(), policies, MiddlewareConfig::default())?;
let app = Router::new()
    .route("/users/{id}", get(|| async { "user" }))
    .route("/health", get(|| async { "OK" }))
    .layer(layer);
```

The fragment assumes an enclosing function returning an error type that accepts the constructors' errors. The full files in `examples/` compile and run with the commands in the [examples guide](../examples/README.md).

An unmatched request uses a stable unmatched route value. Its raw path remains separate and never becomes a Loki label.

## Request IDs and client IPs

The middleware generates request IDs. Incoming `x-request-id` values are ignored by default; enable `trust_incoming_request_id` only when your application's trust boundary permits client-controlled IDs:

```rust
let config = MiddlewareConfig {
    trust_incoming_request_id: true,
    ..MiddlewareConfig::default()
};
```

Accepted values are bounded and validated. Invalid values are replaced. `Forwarded` and `X-Forwarded-For` headers are ignored unless the direct peer is in `config.client_ip.trusted_proxies`. The [complete example](../examples/axum_loki.rs) accepts explicit `TRUSTED_PROXY_CIDRS`; do not trust broad public ranges.

## Capture and error behavior

`Minimal` records correlation, method, normalized route, status, and timing. `Standard` adds filtered query parameters and headers. `Detailed` permits bounded, redacted request and response bodies when the `body-capture` feature is enabled. Route policies can select a narrower profile or exclude a route. See [capture policies](policies.md) and [redaction](redaction.md).

Response status, headers, data frames, and trailers pass through unchanged. Streaming bodies are observed frame by frame; the final event is emitted when the response body completes. A dropped request or response body emits a cancellation event by default. Set `emit_cancellation_events: false` to suppress that event while retaining internal cancellation accounting.

The Axum classifier recognizes Tower's typed timeout error. Other service errors are classified as server errors without copying error messages into telemetry. If application timeout handling returns an HTTP response, that response's actual status is recorded. No status is invented before response headers exist.

## Lifecycle

Start one queue and one consumer or exporter group for the application, not one per request. On shutdown, stop accepting HTTP traffic, let Axum finish graceful connection shutdown, shut down any tracer provider, then call `LokiExporterHandle::shutdown()`. Inspect `ShutdownReport` for abandoned events and worker failures. The exporter has a configured drain deadline.

The [examples guide](../examples/README.md) includes commands for running the server, querying Loki, and exercising the ignored real-Loki tests.
