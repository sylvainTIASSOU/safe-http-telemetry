# Production configuration

Use this guide when wiring `safe-http-telemetry` into a deployed service. The [complete Axum example](../examples/axum_loki.rs) shows the ownership and shutdown pattern; its local Docker stack and sample routes are for evaluation.

## Set explicit resource limits

`QueueConfig::new(capacity)` bounds waiting events; its default is 1,024. A full queue rejects the newest event without delaying the HTTP request. Choose a capacity for your traffic and monitor both occupancy and `dropped_events`. Avoid excessively large capacities that turn a downstream outage into a memory problem.

`CaptureLimits::default()` provides finite limits. The values below are the current code defaults; reduce them for routes that do not need the data.

| Limit | Default |
| --- | ---: |
| Request body | 64 KiB |
| Response body | 64 KiB |
| One header value | 8 KiB |
| All headers | 32 KiB |
| Header count | 128 |
| Query parameter count | 128 |
| JSON depth | 16 |
| JSON nodes | 4,096 |
| One captured string | 16 KiB |
| One event | 256 KiB |
| Loki batch allowed by capture configuration | 4 MiB |

`CaptureLimits::validate()` rejects zero or inconsistent limits, including an event limit larger than its batch limit. The Loki exporter also validates its own limits when constructed.

| `LokiConfig` field | Default |
| --- | ---: |
| `batch.max_events` | 100 |
| `batch.max_bytes` | 4 MiB |
| `batch.flush_interval` | 500 ms |
| `request_timeout` | 5 s |
| `retry.max_retries` | 2 retries after the initial attempt |
| `shutdown.deadline` | 5 s |

Tune batch size and timeout against the latency and limits of your Loki service. One exporter owns a fixed number of workers for the lifetime of the application; do not start a new exporter per request.

## Choose capture policies conservatively

Start with `Minimal` for endpoints where method, normalized route, status, timing, and correlation are enough. Use `Standard` when filtered query parameters or headers provide necessary context. Select `Detailed` only on reviewed routes and only with the `body-capture` feature enabled. Authentication, password-reset, payment, and credential-management endpoints should normally be excluded or kept at `Minimal`.

Body observation is bounded, but no generic redactor can identify every secret. Preserve the default `RedactionConfig` rules and add field names specific to your application. Never put credentials or personal data in free-form `tracing` messages; the business layer redacts configured structured fields, not arbitrary prose. See [redaction](redaction.md) and [policies](policies.md).

## Configure the export destination

Supply a complete HTTP or HTTPS Loki push URL without embedded credentials. `LokiConfig` supports a bearer token and optional tenant ID; pass the token through your deployment secret mechanism. The [example variables](../examples/README.md#configure-the-complete-example) demonstrate both. Protect access to Loki itself because redacted telemetry may still contain sensitive operational context.

Controlled Loki labels contain only service, environment, version, optional region, and event type. Request IDs, trace IDs, IP addresses, paths, messages, and body values are not indexed labels. Keep service metadata stable and low cardinality.

## Handle proxies and geolocation

By default, `Forwarded` and `X-Forwarded-For` are ignored. Put only the exact CIDR ranges of proxies you operate in `MiddlewareConfig.client_ip.trusted_proxies`. Do not accept forwarding headers from arbitrary clients. Incoming `x-request-id` is also ignored unless explicitly trusted.

If `geolocation` is enabled, install an MMDB separately and pass its path to `GeolocationConfig::new(path)`. A missing, corrupt, or oversized file fails layer construction. Country-only output is the default. Keep the application's database mount read-only and restart after an update; the engine holds the loaded bytes in memory. Follow the provider's attribution and licensing terms in the [database guide](geolocation.md).

## Monitor and shut down

Export `InternalTelemetry::snapshot()` through your existing metrics system. Watch at least `dropped_events`, queue occupancy from `BoundedEventQueue::stats()`, `failed_batches`, `export_retries`, `pending_export_events`, `geolocation_errors` when enabled, and `shutdown_abandoned_events`. Set alert thresholds for your service's normal traffic and Loki availability.

On shutdown, stop accepting HTTP requests, wait for Axum's graceful connection shutdown, shut down any tracer provider, then call `LokiExporterHandle::shutdown()`. Inspect `ShutdownReport`: a timeout, worker failure, or nonzero `abandoned_events` means some telemetry may not have reached Loki. The exporter closes the queue and respects its configured drain deadline.

Configuration errors should stop startup. Validate capture, route, geolocation, and Loki settings through their constructors and `validate()` methods instead of silently disabling a requested integration. Run the [test and release matrix](test-matrix.md) before publishing or deploying changes.
