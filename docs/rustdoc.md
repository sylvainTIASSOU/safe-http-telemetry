# safe-http-telemetry

Safe, bounded HTTP telemetry for Rust services using Axum or Tower, with optional `tracing` business events, OpenTelemetry correlation, Grafana Loki export, and local IP geolocation.

This crate is a library: it adds middleware and an event pipeline to your application, but does not start a server or configure a tracing backend for you. The application owns the HTTP server, runtime, subscriber, exporters, and shutdown sequence.

## Start here

1. Choose the integration features you need (see [feature flags](#feature-flags)).
2. Create one bounded event queue and a default capture policy.
3. Add the middleware after registering Axum routes, or wrap a Tower service.
4. Run a queue consumer. For local evaluation, use the `minimal_axum` example; for Loki, use `axum_loki`.
5. Shut down the server, close/drain the queue or exporter, and inspect its shutdown report.

For the full installation commands and copyable application walkthrough, see the [repository README](https://github.com/sylvainTIASSOU/safe-http-telemetry#add-and-integrate-the-crate). Runnable sources are available in [`minimal_axum`](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/examples/minimal_axum.rs) and [`axum_loki`](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/examples/axum_loki.rs).

## Event pipeline

The middleware records one terminal event for a completed request, failed service call, timeout, or cancellation. Producers enqueue redacted events without waiting for network I/O. A bounded queue protects application memory; when it is full, the default `DropNewest` policy rejects the incoming event and increments counters.

The queue does not export events by itself. Consume it with an application-owned worker, or pass it to `LokiExporter` when the `loki` feature is enabled. Keep the producer handle alive while requests can still arrive. At shutdown, stop serving requests first, then close and drain the queue or call the exporter shutdown method.

## Capture profiles and route policies

`CaptureProfile::Minimal` is the default and records correlation and lifecycle metadata such as method, normalized route, status, outcome, and duration. `Standard` permits bounded query and header capture after filtering and redaction. `Detailed` additionally permits bounded body-prefix capture when the `body-capture` feature is enabled.

Profiles set the maximum capture level. A route rule can restrict capture or exclude a route; it cannot raise a route above its profile. Policies can be scoped to an exact route or a path-segment prefix and optionally to an HTTP method. Exact paths take precedence over prefixes; more specific matching rules take precedence over less specific rules. Conflicting rules with the same selector are rejected.

With Axum, register routes before applying the telemetry layer. The adapter then records Axum's matched route template (for example, `/users/{id}`), not the raw path supplied by the client. When no normalized route is available, the Tower integration uses a stable fallback label. Raw URL paths are omitted from events because they can contain identifiers or secrets.

## Redaction and data limits

Redaction and size limits are applied before events enter the queue. Built-in sensitive-field matching is case-insensitive and ignores common separators, including underscores and hyphens. JSON and form data are parsed structurally; query and header capture are bounded. Add application-specific secret names to `RedactionConfig` and test them with fictitious data.

Redaction cannot identify every secret format or business-sensitive value. Start with `Minimal`, opt in to richer capture only for reviewed routes, and do not treat redaction as an access-control boundary. Limit access and retention at the telemetry destination as well.

Body capture requires the `body-capture` feature and an explicitly selected `Detailed` route. Only a configured prefix is inspected; request and response streams continue through the service. Unsupported content types, parse failures, truncation, or capture limits are represented by status markers instead of unbounded data.

Forwarded client-IP headers are ignored by default. Configure trusted proxy CIDRs only for proxies under your control. Incoming request IDs are untrusted unless `trust_incoming_request_id` is enabled. Geolocation uses an explicitly supplied local MMDB database; the crate does not bundle or download a production database.

## Feature flags

All features are disabled by default. Enable the integrations required by the application:

| Feature | Adds | Notes |
| --- | --- | --- |
| `tower` | Tower HTTP lifecycle middleware | Core HTTP adapter; enables `tracing` as a dependency. |
| `axum` | Axum layer and matched-route resolver | Enables `tower`. Register routes before applying the layer. |
| `tracing` | Business-event layer for a `tracing` subscriber | The application configures the subscriber and its output. |
| `opentelemetry` | W3C trace-context correlation | Does not configure span export; the application owns the provider/exporter. |
| `loki` | Asynchronous Loki client and bounded exporter | Runs fixed workers with bounded batching, retry, and shutdown settings. |
| `body-capture` | Bounded request/response body-prefix observation | Use only with reviewed `Detailed` routes. |
| `geolocation` | Local MMDB-based country lookup | Requires a separately obtained and licensed database path. |

Feature combinations are independent where possible. For example, a Tower application can use `tower` without Axum, and an application that only needs the middleware does not need `loki`.

## Loki export and shutdown

When using Loki, provide the push endpoint (for example, `https://logs.example/loki/api/v1/push`) and configure tenant, bearer token, batch limits, request timeout, retry policy, worker count, and shutdown deadline for your deployment. Use HTTPS for remote endpoints and keep credentials in deployment secrets. Credentials must not be embedded in the URL.

Create the exporter before serving requests and retain its handle. After graceful server shutdown, shut down the OpenTelemetry provider if your application owns one, then call the exporter's async `shutdown`. Inspect `ShutdownReport`: a timeout or nonzero abandoned-event count means some telemetry was not delivered. See the [production guide](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/production.md) and complete Loki example for a full lifecycle.

## Internal telemetry

`InternalTelemetry::snapshot()` returns low-cardinality operational counters for request outcomes, queue pressure, redaction/truncation, body omissions, geolocation, and export results. The snapshot contains no HTTP payloads, credentials, request IDs, IP addresses, or route values. Export these counters through the metrics system already used by your application.

## Compatibility

The minimum supported Rust version is **1.85**. Runtime dependency requirements use compatible caret constraints so Cargo can unify compatible versions with those already used by the host application. The crate's own lockfile pins versions for this repository's development and CI; library consumers resolve dependencies through their application lockfile.

## API map

| Module | Use it for |
| --- | --- |
| `axum_adapter` | Attach telemetry to an Axum `Router` (`axum` feature). |
| `tower_integration` | Middleware configuration, Tower layer, route resolvers, and error classifiers (`tower` feature). |
| `policy` | Capture profiles, route matching, exclusions, and policy resolution. |
| `config` | Service metadata, bounded capture limits, redaction settings, and validation. |
| `redaction` | Redact JSON, headers, query values, and business events. |
| `runtime::queue` | Bounded event queue, producers, consumers, and queue statistics. |
| `core::event` | Versioned HTTP and business event structures. |
| `core::telemetry` | Low-cardinality internal counters and snapshots. |
| `tracing_integration` | Business events and W3C trace-context helpers (`tracing` / `opentelemetry` features). |
| `loki` | Loki configuration, payloads, batching, client, exporter, and shutdown report (`loki` feature). |
| `body_capture` | Bounded parsing and capture results (`body-capture` feature). |
| `geolocation` | Local MMDB configuration and lookup (`geolocation` feature). |

See the [capture policy guide](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/policies.md), [redaction guide](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/redaction.md), [examples guide](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/examples/README.md), and [schema documentation](https://github.com/sylvainTIASSOU/safe-http-telemetry/tree/master/schemas).
