# Production Configuration Guide

This guide describes how to configure `safe-http-telemetry` for production deployments. It covers memory bounds, profile selection, secret management, geolocation, and resilience.

## 1. Memory and Resource Bounds

All internal structures are bounded. The following limits must be configured explicitly:

### Queue capacity
`QueueConfig::new(capacity)` sets the maximum number of events buffered before the saturation policy applies. Default: **1024 events**.
- **Low-traffic services**: 256–512 events.
- **High-traffic APIs**: 2048–4096 events, but monitor `queue_occupancy` from `InternalTelemetry`.
- **Never set to `usize::MAX`**: the queue would grow unbounded.

### Capture limits (`CaptureLimits`)
`CaptureLimits` bounds every in-memory capture:

| Field | Default | Production recommendation |
|---|---|---|
| `max_request_body_bytes` | 64 KiB | 16–64 KiB |
| `max_response_body_bytes` | 64 KiB | 16–64 KiB |
| `max_header_value_bytes` | 4 KiB | 1–4 KiB |
| `max_total_header_bytes` | 16 KiB | 8–16 KiB |
| `max_headers` | 64 | 32–64 |
| `max_query_parameters` | 64 | 16–32 |
| `max_json_depth` | 16 | 8–16 |
| `max_json_nodes` | 1024 | 256–1024 |
| `max_string_bytes` | 8 KiB | 1–8 KiB |
| `max_event_bytes` | 128 KiB | 64–128 KiB |
| `max_loki_batch_bytes` | 1 MiB | 512 KiB–1 MiB |

> **Note**: `max_event_bytes` must never exceed `max_loki_batch_bytes`; this is validated at startup.

### Loki batching (`LokiConfig`)
| Field | Default | Production recommendation |
|---|---|---|
| `batch.max_events` | 100 | 50–200 |
| `batch.max_bytes` | 1 MiB | 512 KiB–1 MiB |
| `batch.flush_interval` | 5 s | 1–5 s |
| `request_timeout` | 10 s | 2–5 s |
| `retry.max_retries` | 3 | 2–5 |
| `shutdown.deadline` | 10 s | 5–15 s |

### Geolocation cache
`GeolocationConfig` requires a non-zero `cache_capacity` and a `cache_ttl`. Production recommendation: **10 000 entries** with a **24 h TTL** for a typical API.

## 2. Capture Profile Selection

| Profile | Use case | Cost (relative) |
|---|---|---|
| `Minimal` | Health checks, metrics, high-frequency low-value endpoints | ~1× |
| `Standard` | Most business APIs where headers and query parameters are needed | ~2× |
| `Detailed` | Critical routes (checkout, authentication, payment) where body context is mandatory | ~2.1× |

> **Measured overhead** (from `benches/middleware_overhead.rs`): Baseline ~1.7 µs, Minimal ~21 µs, Standard ~46 µs, Detailed ~46 µs per request on the reference hardware. See `docs/benchmarks.md` for details.

**Recommendation**: start with `Standard` for the default profile, and explicitly elevate `Detailed` only on the routes listed in your incident-response runbook.

## 3. Secret Management

### Redaction rules
`RedactionConfig` applies a list of sensitive field names globally. The default list covers: `password`, `passwd`, `pwd`, `token`, `access_token`, `refresh_token`, `id_token`, `authorization`, `proxy-authorization`, `api_key`, `apikey`, `secret`, `client_secret`, `cookie`, `set-cookie`, `session`, `session_id`, private keys and signing secrets.

**Production recommendation**:
- Keep the default list as a baseline.
- Add domain-specific fields (e.g., `card_number`, `cvv`, `iban`).
- Never remove a default field without a documented risk assessment.

### Route-specific policies
Use `RoutePolicy` to disable body capture entirely on authentication and password-reset routes, even under a `Detailed` profile. This is the **recommended default** for sensitive routes.

### Loki credentials
`LokiConfig` rejects URLs containing embedded credentials (`user:pass@host`). Provide the bearer token or basic-auth credentials through the deployment secret mechanism (environment variable, secret manager, etc.). Never log the `LokiConfig` `Debug` representation in production.

## 4. Geolocation

### Database
The optional `geolocation` feature uses the **bundled DB-IP City Lite** MMDB database. The database is included in the crate source and embedded into applications that enable geolocation. The feature is opt-in; applications that do not enable it do not embed the database.

**Production checklist**:
- [ ] Keep the bundled DB-IP database current and review its attribution and license.
- [ ] If using an external MMDB, verify its origin and integrity before loading it.
- [ ] Set `GeolocationConfig::new(path)` only when overriding the bundled database.
- [ ] Keep `GeolocationConfig::cache_capacity` and `cache_ttl` bounded.
- [ ] If you do not need city-level precision, enable `country_only` to reduce collected fields.

### Trusted proxies
`Forwarded` and `X-Forwarded-For` headers are **ignored by default**. If your service runs behind a reverse proxy, configure `TrustedProxyConfig` with the exact proxy CIDR ranges. An untrusted peer cannot inject forwarding headers.

## 5. Resilience

### Saturation policy
The default saturation policy is `DropNewest`. When the queue is full, new events are rejected and `dropped_events` is incremented. This preserves already-accepted events and keeps the request path non-blocking.

**Monitoring**: export `InternalTelemetry::snapshot()` to your metrics backend and alert on:
- `dropped_events > 0` sustained for more than 5 minutes.
- `queue_occupancy / queue_capacity > 0.8` sustained for more than 5 minutes.
- `failed_batches > 0` with a rising trend.
- `export_duration_ms_total / export_operations > request_timeout` (exports are too slow).

### Shutdown
`LokiExporterHandle::shutdown()` closes the queue, drains the remaining events, and respects `shutdown.deadline`. If the deadline expires, remaining events are counted in `abandoned_events` and the shutdown returns `ShutdownOutcome::TimedOut`. Always call `shutdown()` from your application's graceful-shutdown handler (e.g., Axum's `with_graceful_shutdown`).

### Recursive logging prevention
Exporter errors are never fed back into the same Loki queue. They are counted in `failed_batches` and, optionally, written to a separate diagnostic sink. Do not configure the diagnostic sink to point back to the same Loki instance.

## 6. Configuration Validation

`Config::validate()` rejects configurations that would be unsafe in production. Startup must fail if:
- A limit is zero or internally inconsistent.
- `max_event_bytes > max_loki_batch_bytes`.
- A route policy is ambiguous.
- A configured regex is invalid.
- The geolocation database is unreadable.
- TLS is requested with an inconsistent configuration.

Do not bypass validation. A rejected configuration is a signal that the deployment is misconfigured.
