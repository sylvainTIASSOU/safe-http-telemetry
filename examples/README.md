# Runnable integration examples

Run these commands from the repository root. The examples use port `3030` by default, and the local Loki stack uses ports `3100` and `3001`. Rust 1.85 or newer is required. All submitted values below are fictitious.

## Minimal Axum: observe events without Loki

```sh
cargo run --locked --example minimal_axum --features axum,tracing
```

In a second terminal:

```sh
curl http://127.0.0.1:3030/hello/example
curl http://127.0.0.1:3030/health
```

The server prints a JSON HTTP event for `/hello/{name}` and excludes `/health`. It starts one consumer for the bounded queue and drains it on Ctrl+C. This example is suitable for learning the middleware's output; use a durable sink or the Loki exporter for a deployed application.

## Complete Axum and Loki example

Start the local Loki and Grafana stack if it is not already running:

```sh
docker compose -f examples/loki/docker-compose.yaml up -d
curl --fail --retry 30 --retry-connrefused --retry-delay 1 http://127.0.0.1:3100/ready
```

In a separate terminal, start the application:

```sh
cargo run --locked --example axum_loki \
  --features axum,loki,tracing,opentelemetry,body-capture
```

This starts one bounded queue, one Loki exporter worker, one Axum middleware layer, and one `tracing` business-event layer. The default profile is `Minimal`. `/health` is excluded. `/items`, `/form`, and `/text` use `Detailed`; body observation on those routes requires the `body-capture` feature in the command above. Request and response data are bounded and redacted before they enter the queue.

Send a request with an explicit W3C trace ID:

```sh
START_NS="$(date +%s%N)"
curl -i \
  -H 'traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01' \
  'http://127.0.0.1:3030/hello/example'
END_NS="$(date +%s%N)"
```

The handler emits a business event; the middleware emits an HTTP event. Find both by trace ID:

```sh
curl --get http://127.0.0.1:3100/loki/api/v1/query_range \
  --data-urlencode 'query={service="safe-http-telemetry-example"} | json | trace_id="4bf92f3577b34da6a3ce929d0e0e4736"' \
  --data-urlencode "start=${START_NS}" \
  --data-urlencode "end=${END_NS}" \
  --data-urlencode 'limit=20' \
  --data-urlencode 'direction=forward'
```

Test bounded body capture with fictitious data:

```sh
curl -i -X POST http://127.0.0.1:3030/items \
  -H 'content-type: application/json' \
  -d '{"name":"Example","email":"example.invalid@example.com","password":"fake-secret","token":"fake-token"}'
```

The handler returns `201 Created` without echoing the submitted values. The telemetry pipeline redacts the `password` and `token` fields. Do not use this route design as a substitute for reviewing your own data and redaction rules.

Grafana is available at `http://127.0.0.1:3001` with the local Loki data source preconfigured. This Compose file enables anonymous administrator access for local evaluation; do not expose it publicly or reuse it unchanged in production. The example creates OpenTelemetry trace IDs but does **not** export spans to Tempo or another trace backend. See the [Grafana and OpenTelemetry guide](../docs/grafana-opentelemetry.md).

Stop the application with Ctrl+C. It waits for Axum to stop, shuts down the tracer provider, and drains the Loki exporter within its deadline. If you started the Compose stack solely for this walkthrough, stop it with `docker compose -f examples/loki/docker-compose.yaml down`. This leaves named data volumes intact.

## Configure the complete example

| Variable | Default | Purpose |
| --- | --- | --- |
| `APP_BIND_ADDR` | `127.0.0.1:3030` | Axum listen address; set explicitly for your deployment |
| `LOKI_PUSH_URL` | `http://127.0.0.1:3100/loki/api/v1/push` | Complete Loki push URL, without embedded credentials |
| `LOKI_BEARER_TOKEN` | unset | Bearer token supplied by the deployment secret mechanism |
| `LOKI_TENANT_ID` | unset | Optional Loki tenant ID |
| `SERVICE_NAME` | `safe-http-telemetry-example` | Stable, low-cardinality service label |
| `SERVICE_ENVIRONMENT` | `development` | Environment label |
| `SERVICE_VERSION` | crate version | Application version label |
| `SERVICE_REGION` | unset | Optional region label |
| `TRUSTED_PROXY_CIDRS` | unset | Comma-separated CIDRs of immediate proxies whose forwarding headers may be trusted |
| `SAFE_HTTP_MMDB_PATH` | unset | Local MMDB path; active only with the `geolocation` feature |

For example, configure a remote Loki endpoint and one trusted proxy network:

```sh
LOKI_PUSH_URL=https://loki.example.invalid/loki/api/v1/push \
LOKI_BEARER_TOKEN="$LOKI_TOKEN" \
SERVICE_NAME=orders-api \
SERVICE_ENVIRONMENT=staging \
TRUSTED_PROXY_CIDRS=10.0.0.0/8 \
  cargo run --locked --example axum_loki \
  --features axum,loki,tracing,opentelemetry,body-capture
```

Only list proxies you operate. Without `TRUSTED_PROXY_CIDRS`, forwarding headers are ignored. `SAFE_HTTP_MMDB_PATH` requires an existing file and a `Standard` or `Detailed` route for a geographical lookup; the example's `/hello/{name}` route is `Minimal`. Install the database separately as described in the [geolocation guide](../docs/geolocation.md). Enabling the Cargo feature alone does not download a database.

## Real Loki integration tests

With the local Loki service ready, run the ignored tests:

```sh
cargo test --locked --features axum,loki,body-capture \
  --test loki_http_e2e -- --ignored --test-threads=1
```

Set `SAFE_HTTP_TELEMETRY_LOKI_URL` if Loki uses a different base URL. These tests start their own Axum servers and verify HTTP events, redaction, labels, correlation, and drained shutdown against Loki. They do not need Grafana.

## Copy the example into your application

Use the [complete Rust source](axum_loki.rs) as the wiring reference. Its HTTP handlers and route policies are sample application code; replace them with your own routes and review every `Detailed` policy. The example uses these direct dependencies in addition to `safe-http-telemetry`:

```toml
[dependencies]
safe-http-telemetry = { version = "0.1.0", features = ["axum", "loki", "tracing", "opentelemetry", "body-capture"] }
axum = "0.8"
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "signal"] }
serde = { version = "1", features = ["derive"] }
tracing = "0.1"
tracing-subscriber = "0.3"
opentelemetry = "0.33"
opentelemetry_sdk = "0.33"
tracing-opentelemetry = "0.34"
```

Until this crate is published, use `path = "/path/to/safe-http-telemetry"` instead of the version requirement. For a minimal setup, follow [minimal_axum.rs](minimal_axum.rs) and add `serde_json = "1"` for its JSON printer. Your deployed service should export events to a durable sink, apply its own authentication and proxy policy, expose operational counters to its metrics system, and configure a real OpenTelemetry span exporter if distributed traces are required.
