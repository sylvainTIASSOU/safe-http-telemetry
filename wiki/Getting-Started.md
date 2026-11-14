# Getting started

The crate is a library and does not provide an application binary. Until publication, run examples from the repository checkout.

## Minimal Axum

```sh
cargo run --locked --example minimal_axum --features axum,tracing
```

In another terminal:

```sh
curl http://127.0.0.1:3030/hello/example
curl http://127.0.0.1:3030/health
```

The first route emits a JSON event. `/health` is excluded. Stop with Ctrl+C to drain the example queue.

## Axum and Loki

Start the local Loki stack and wait for readiness:

```sh
docker compose -f examples/loki/docker-compose.yaml up -d
curl --fail --retry 30 --retry-connrefused --retry-delay 1 http://127.0.0.1:3100/ready
```

Then run the example:

```sh
cargo run --locked --example axum_loki --features axum,loki,tracing,opentelemetry,body-capture
```

See [the full examples guide](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/examples/README.md) for environment variables, fictitious requests, LogQL queries, and shutdown details. The local Grafana stack is for evaluation and is not a hardened deployment.
