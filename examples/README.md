Send requests:

```sh
curl http://127.0.0.1:3030/hello/fictitious-user
curl http://127.0.0.1:3030/health
```

`/hello/{name}` produces one minimal event. `/health` is excluded.

Stop the server with Ctrl+C.

## Axum with Loki and correlated business events

Start the local, single-instance Loki service:

```sh
docker compose -f examples/loki/docker-compose.yaml up -d
```

Wait for readiness with a bounded number of attempts:

```sh
curl \
  --fail \
  --retry 30 \
  --retry-connrefused \
  --retry-delay 1 \
  http://127.0.0.1:3100/ready
```

Start Axum, the native business-event layer and the bounded Loki exporter:

```sh
cargo run --locked --example axum_loki \
  --features axum,loki,tracing,opentelemetry
```

A different push endpoint can be supplied without credentials in the URL:

```sh
LOKI_PUSH_URL=http://127.0.0.1:3100/loki/api/v1/push \
  cargo run --locked --example axum_loki \
  --features axum,loki,tracing,opentelemetry
```

Record a start timestamp and send a request containing only fictitious data:

```sh
START_NS="$(date +%s%N)"

curl \
  -H 'traceparent: 00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01' \
  'http://127.0.0.1:3030/hello/fictitious-user?token=fictitious-example-secret'

END_NS="$(date +%s%N)"
```

Minimal mode records correlation, method, normalized route, status and timing.
It does not capture the query, headers or bodies. The handler additionally
produces one `business.event` with the same `trace_id` as the HTTP event.

Query both events with the bounded Loki range API:

```sh
curl --get http://127.0.0.1:3100/loki/api/v1/query_range \
  --data-urlencode 'query={service="safe-http-telemetry-example"} | json | trace_id="4bf92f3577b34da6a3ce929d0e0e4736"' \
  --data-urlencode "start=${START_NS}" \
  --data-urlencode "end=${END_NS}" \
  --data-urlencode 'limit=20' \
  --data-urlencode 'direction=forward'
```

See `docs/grafana-opentelemetry.md` for OTLP trace export and Grafana derived
fields. The example itself generates trace IDs but does not export spans.

Stop Axum with Ctrl+C. The application stops HTTP serving first, shuts down its
tracer provider, then closes and drains the exporter within its deadline.

Remove Loki and its local data:

```sh
docker compose -f examples/loki/docker-compose.yaml down --volumes
```

## Real-Loki HTTP end-to-end test

The test is ignored by default and therefore does not require Docker during the
normal CI test suite.

With local Loki running and ready, execute:

```sh
cargo test \
  --locked \
  --features axum,loki,body-capture \
  --test loki_http_e2e \
  -- \
  --ignored \
  --test-threads=1
```

An alternative Loki base URL can be selected with:

```sh
SAFE_HTTP_TELEMETRY_LOKI_URL=http://127.0.0.1:3100 \
  cargo test \
    --locked \
    --features axum,loki,body-capture \
    --test loki_http_e2e \
    -- \
    --ignored \
    --test-threads=1
```

The tests start their own Axum servers on ephemeral ports, send requests,
drain the exporter, and verify the matching Loki events. They cover both
minimal observation and detailed body capture. The checks include:

- request ID propagation;
- method and normalized route;
- HTTP status and finite duration;
- the exact controlled label set;
- query and header capture according to the selected profile;
- absent bodies in Minimal mode and captured, redacted bodies in Detailed mode;
- absent correlation IDs when OpenTelemetry is disabled;
- absence of fictitious query, header, and body secrets;
- drained shutdown with no abandoned events.

## Optional geolocation

Enable `geolocation` and set `SAFE_HTTP_MMDB_PATH` to an installed local database. Without the variable the example disables geolocation. See [database deployment](../docs/geolocation.md); the application never downloads the file itself.
