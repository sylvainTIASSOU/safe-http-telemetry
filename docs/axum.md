# Axum integration

The `axum` feature enables the Axum adapter and implies the `tower` feature.
The middleware observes requests without performing Loki network operations
on the application request path.

## Router integration

Register routes before applying the telemetry layer. The adapter reads
`axum::extract::MatchedPath`, so `/users/123` is associated with the normalized
route `/users/{id}`.

```rust
use safe_http_telemetry::{
    axum_adapter::axum_telemetry_layer,
    core::telemetry::InternalTelemetry,
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::MiddlewareConfig,
};

let queue = BoundedEventQueue::new(
    QueueConfig::new(1_024),
    InternalTelemetry::default(),
)?;

let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);
policies.add_policy(
    RoutePolicy::new(
        RoutePattern::exact("/health"),
        CaptureProfile::Minimal,
    )
    .exclude(),
)?;

let telemetry = axum_telemetry_layer(
    queue.clone(),
    policies,
    MiddlewareConfig::default(),
)?;

let app = axum::Router::new()
    .route("/users/{id}", axum::routing::get(user_handler))
    .route("/health", axum::routing::get(health_handler))
    .layer(telemetry);
```


Requests without a matched route use the stable route value unmatched.
Their raw paths remain separate from normalized routes and never become Loki
labels.
Request IDs
Incoming x-request-id propagation is disabled by default. Enable it only
when its use is compatible with the application's trust boundaries:
rust
let config = MiddlewareConfig {
    trust_incoming_request_id: true,
    ..MiddlewareConfig::default()
};
Incoming values are bounded and validated. Invalid values are replaced by a
generated identifier.
Responses and streaming
Response status, headers, data frames and trailers are forwarded unchanged.
The final event is emitted after the complete body has been transmitted.
Streaming bodies are observed frame by frame and are never collected in full
by the middleware.
Dropping a request future or an incomplete response body releases its request
state. A cancellation event is emitted by default. This can be disabled with
emit_cancellation_events: false; internal cancellation accounting remains
active.
Timeouts and errors
AxumErrorClassifier recognizes Tower's boxed
tower::timeout::error::Elapsed as RequestFailure::Timeout. Other service
errors are classified as server errors. Error messages are never copied into
telemetry events.
To observe a timeout as a typed error rather than as a cancellation, the
telemetry service must wrap the Tower timeout service. When using
Router::layer for normalized routes, application timeout handling that
returns an actual HTTP response is instead observed with that response's real
status code.
No synthetic HTTP status is created when a failure occurs before response
headers.
Loki exporter ownership and shutdown
Keep a clone of the bounded queue for the middleware and retain the
LokiExporterHandle for application shutdown.
The shutdown order is:
stop accepting new HTTP connections;
complete Axum's graceful connection shutdown;
call LokiExporterHandle::shutdown;
respect the configured drain deadline and inspect its report.
Do not spawn one exporter per request. The exporter owns one fixed worker
group for the application lifetime.
Current capture boundary


## Executable examples and real Loki

The repository includes a minimal Axum server and an Axum server connected to a
real local Loki instance. See [the examples guide](../examples/README.md) for
startup, request, query, graceful shutdown and cleanup commands.

The real-Loki HTTP end-to-end tests are ignored by default. They start Axum
servers and verify both `Minimal` mode and detailed body capture against Loki.
Run the target with the `axum`, `loki`, and `body-capture` features. Normal CI
therefore remains independent from Docker and external services.
