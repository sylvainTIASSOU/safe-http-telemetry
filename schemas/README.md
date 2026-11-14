# Telemetry event format

`safe-http-telemetry` emits two JSON event types. Both currently use `schema_version: "1.0"`, independently of the crate version:

| `event` value | Rust type | Purpose |
| --- | --- | --- |
| `http.request.completed` | [`HttpEvent`](../src/core/event.rs) | One completed, failed, timed-out, or cancelled HTTP exchange |
| `business.event` | [`BusinessEvent`](../src/core/event.rs) | One application event captured by the optional `tracing` layer |

The serialized Rust types and constants in [`src/core/event.rs`](../src/core/event.rs) are the current source of truth. **This directory does not yet contain machine-readable JSON Schema files.** Do not configure a JSON Schema validator from a filename that is not present here.

## HTTP event

A minimal completed request can be serialized as:

```json
{
  "schema_version": "1.0",
  "event": "http.request.completed",
  "level": "info",
  "request_id": "example-request-001",
  "request": {
    "method": "GET",
    "route": "/users/{id}",
    "path": "[OMITTED]",
    "query": {},
    "headers": {}
  },
  "response": {
    "status_code": 200,
    "headers": {},
    "outcome": "success"
  },
  "timing": {
    "duration_ms": 12.5
  },
  "capture": {
    "request_body_truncated": false,
    "response_body_truncated": false
  }
}
```

For middleware-generated HTTP events, `request.route` is the normalized route template and `request.path` is always `[OMITTED]`. The raw request path is never exported because path segments can contain secrets. Neither field is a Loki label. `timing` values are numbers in milliseconds, and `response.status_code` is a number when available. A failure before response headers may have no status code. The `level` values are `trace`, `debug`, `info`, `warn`, and `error`; outcome values are `success`, `client_error`, `server_error`, `cancelled`, and `timeout`.

Optional fields such as `trace_id`, `span_id`, request and response `body`, `content_type`, `content_length`, `client_ip`, and `geolocation` are omitted when absent. `request.query` values may be a scalar (`string`, number, boolean, or `null`) or an array of scalars for repeated parameters. The `capture` object always includes both truncation booleans; optional body statuses are `captured`, `omitted`, `unsupported`, or `parse_error`. The [capture policy guide](../docs/policies.md) explains when fields may be collected.

## Business event

An application event can be serialized as:

```json
{
  "schema_version": "1.0",
  "event": "business.event",
  "level": "info",
  "message": "Order accepted",
  "target": "orders",
  "fields": {
    "attempt": 1,
    "order_id": "example-order-001"
  },
  "fields_truncated": false
}
```

`message`, `request_id`, `trace_id`, `span_id`, and `module_path` are omitted when absent. `fields` is omitted when empty. `fields_truncated` remains a boolean in the serialized event. The business layer filters and redacts supported structured fields before queueing; applications must still avoid secrets in free-form messages. See the [tracing and Grafana guide](../docs/grafana-opentelemetry.md).

## Loki transport

With the `loki` feature, the exporter serializes an event as the JSON log line inside a Loki push request. Its `values` entry contains a **string** Unix timestamp in nanoseconds and the event JSON string. The surrounding stream has controlled labels: `service`, `environment`, `version`, `event_type` (`http` or `business`), and optional `region`. Request IDs, trace IDs, IPs, paths, and body values do not become indexed labels. The crate also constructs a flat `metadata` map for selected identifiers and route or target data; see [`LokiStream`](../src/loki/payload.rs) for its exact representation.

The event JSON format is useful even without Loki: the [minimal Axum example](../examples/minimal_axum.rs) consumes the queue and prints each event as JSON.

## Compatibility and checks

Schema versions are independent of crate versions. Consumers should inspect both `event` and `schema_version`, ignore fields they do not need, and tolerate omitted optional fields. Breaking format changes require a major schema version; additive optional fields require a minor schema version. See the [compatibility policy](../docs/semver.md).

The current serialization behavior is exercised by [`tests/core.rs`](../tests/core.rs), [`tests/business_events.rs`](../tests/business_events.rs), and [`tests/loki.rs`](../tests/loki.rs):

```sh
cargo test --locked --all-features --test core --test business_events --test loki
```

When machine-readable JSON Schema files are added, they should be checked against these Rust serialization tests before publication.
