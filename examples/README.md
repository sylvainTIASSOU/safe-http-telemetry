# Examples

All requests and secrets used by these examples are fictitious.

## Minimal Axum

Run the middleware with bounded in-memory event storage and structured summary
logs, without starting Loki:

```sh
cargo run --locked --example minimal_axum --features axum,tracing
```
Send requests:
sh
curl http://127.0.0.1:3030/hello/fictitious-user
curl http://127.0.0.1:3030/health
/hello/{name} produces one minimal event. /health is excluded.
Stop the server with Ctrl+C.
Axum with a real Loki instance
Start the local, single-instance Loki service:
sh
docker compose -f examples/loki/docker-compose.yaml up -d
Wait for readiness with a bounded number of attempts:
sh
curl \
  --fail \
  --retry 30 \
  --retry-connrefused \
  --retry-delay 1 \
  http://127.0.0.1:3100/ready
Start Axum and the bounded Loki exporter:
sh
cargo run --locked --example axum_loki --features axum,loki,tracing
A different push endpoint can be supplied without credentials in the URL:
sh
LOKI_PUSH_URL=http://127.0.0.1:3100/loki/api/v1/push \
  cargo run --locked --example axum_loki --features axum,loki,tracing
Send a request containing only fictitious data:
sh
curl \
  'http://127.0.0.1:3030/hello/fictitious-user?token=fictitious-example-secret'
Minimal mode records correlation, method, normalized route, status and timing.
It does not capture the query, headers or bodies.
Query Loki:
sh
curl --get http://127.0.0.1:3100/loki/api/v1/query \
  --data-urlencode 'query={service="safe-http-telemetry-example"}' \
  --data-urlencode 'limit=20'
Stop Axum with Ctrl+C. The application stops HTTP serving first, then closes
and drains the exporter within its configured deadline.
Remove Loki and its local data:
sh
docker compose -f examples/loki/docker-compose.yaml down --volumes
Real-Loki HTTP end-to-end test
The test is ignored by default and therefore does not require Docker during the
normal CI test suite.
With local Loki running and ready, execute:
sh
cargo test \
  --locked \
  --features axum,loki \
  --test loki_http_e2e \
  -- \
  --ignored \
  --test-threads=1
An alternative Loki base URL can be selected with:
sh
SAFE_HTTP_TELEMETRY_LOKI_URL=http://127.0.0.1:3100 \
  cargo test \
    --locked \
    --features axum,loki \
    --test loki_http_e2e \
    -- \
    --ignored \
    --test-threads=1
The test starts its own Axum server on an ephemeral port, sends one request,
drains the exporter and verifies exactly one matching Loki event. It checks:
request ID propagation;
method and normalized route;
HTTP status and finite duration;
the exact controlled label set;
empty query and header capture;
absent request and response bodies;
absent Phase 4 correlation IDs;
absence of the fictitious query/body secret;
drained shutdown with no abandoned events.

