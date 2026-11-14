# safe-http-telemetry

Bounded HTTP telemetry for Rust services using Axum or Tower, with optional `tracing` business events, OpenTelemetry correlation, Grafana Loki export, and local IP geolocation.

> **Release status:** `0.1.0` is being prepared for its first publication. The package is not yet published; use this repository to try the examples. The minimum supported Rust version is **1.85.0**.

## What it does

- Observes the HTTP lifecycle without waiting for Loki on the request path. Each event records a generated request ID, method, normalized route, status, outcome, and duration.
- Applies route-specific `Minimal`, `Standard`, or `Detailed` capture policies. Header, query, and body capture have size limits and sensitive-field redaction.
- Places events in a bounded queue. A full queue drops the newest event and increments operational counters; it does not block the application request.
- Exports bounded batches to Loki with retries and a graceful drain deadline. The same queue can receive redacted application events from a `tracing` layer.
- Correlates events with W3C `traceparent` when `opentelemetry` is enabled. **Span export is configured by the application**; the included example does not send spans to a tracing backend.
- Optionally looks up an IP in a local MMDB file. The crate does not bundle or download a production database.

The library contains no server binary. The [minimal example](examples/minimal_axum.rs) prints redacted events as JSON; the [Loki example](examples/axum_loki.rs) shows the complete queue, subscriber, exporter, and shutdown wiring.

## Try it locally

From this repository, with Rust 1.85 or newer:

```sh
cargo run --locked --example minimal_axum --features axum,tracing
```

In another terminal:

```sh
curl http://127.0.0.1:3030/hello/example
curl http://127.0.0.1:3030/health
```

The first request produces a JSON HTTP event in the server terminal. `/health` is excluded. Press Ctrl+C to stop the server and drain the example's queue. No Loki or database is needed.

For a complete local Loki walkthrough, including a query that finds both HTTP and business events, use the [examples guide](examples/README.md).

## Add it to an application

After `0.1.0` is published, select only the features your application uses:

```toml
[dependencies]
safe-http-telemetry = { version = "0.1.0", features = ["axum", "loki", "tracing", "opentelemetry", "body-capture"] }
axum = "0.8"
tokio = { version = "1", features = ["macros", "net", "rt-multi-thread", "signal"] }
tracing = "0.1"
tracing-subscriber = "0.3"
serde = { version = "1", features = ["derive"] }
opentelemetry = "0.33"
opentelemetry_sdk = "0.33"
tracing-opentelemetry = "0.34"
```

These direct dependencies support the [complete example](examples/axum_loki.rs). Omit the OpenTelemetry crates if you do not enable `opentelemetry`; the [minimal example](examples/minimal_axum.rs) needs only Axum, Tokio, `tracing`, `tracing-subscriber`, and `serde_json` in addition to this crate. Before publication, replace the version requirement with a path to this checkout.

Create one `BoundedEventQueue` for the application. Pass clones to `axum_telemetry_layer`, `BusinessLogLayer` if you want application events, and one `LokiExporter` started at application startup. Register Axum routes before applying the telemetry layer so `MatchedPath` provides a normalized route. After the HTTP server stops accepting traffic, shut down the tracer provider if used, then call `LokiExporterHandle::shutdown()` and inspect its report. The [runnable Loki example](examples/axum_loki.rs) contains this complete setup; the [Axum guide](docs/axum.md) explains each integration point.

`body-capture` is a separate opt-in feature. Enable it with `axum` to observe bounded request and response bodies on explicitly selected `Detailed` routes:

```sh
cargo run --locked --example axum_loki \
  --features axum,loki,tracing,opentelemetry,body-capture
```

Do not capture credential-bearing routes by default. Add domain-specific sensitive field names to `RedactionConfig` and review route policies before enabling `Detailed` in a service. The example uses fictitious values and never echoes submitted credentials.

## Feature flags

Default features are empty. Enable only the integrations you need.

| Feature | Purpose |
| --- | --- |
| `tower` | HTTP middleware for Tower services |
| `axum` | Axum adapter and normalized `MatchedPath` routes; enables `tower` |
| `tracing` | Redacted application-event layer for a `tracing` subscriber |
| `opentelemetry` | W3C trace-context correlation; enables `tracing` |
| `loki` | Asynchronous Loki exporter and bounded retry configuration |
| `body-capture` | Bounded request and response body observation for `Detailed` policies |
| `geolocation` | Country lookup from an explicitly supplied local MMDB path |

The [capture policy guide](docs/policies.md) describes the three profiles. The [redaction guide](docs/redaction.md) describes supported formats and limits.

## Production boundaries

- Set queue capacity, capture limits, Loki batch limits, retry timeout, and shutdown deadline for your workload. Monitor `InternalTelemetry::snapshot()` for drops and export failures. See the [production guide](docs/production.md).
- Forwarded client-IP headers are ignored unless the immediate peer belongs to your configured trusted proxy CIDRs. Request IDs supplied by clients are not trusted by default.
- Loki labels have controlled cardinality. Request IDs, IPs, paths, and payload values remain outside indexed labels. Limit access to Loki even when redaction is enabled.
- Supply Loki credentials through deployment secrets. The example reads `LOKI_BEARER_TOKEN` and optional `LOKI_TENANT_ID`; it never puts credentials in the URL.
- With `geolocation`, deploy a separately licensed MMDB file and configure `GeolocationConfig::new(path)`. See the [database guide](docs/geolocation.md) for pinned downloads, attribution, Docker mounts, and updates.
- The repository's Docker Compose stack binds Loki and Grafana to loopback for local evaluation. It is not a hardened production deployment.

## Documentation and support

The [project wiki draft](wiki/Home.md) tracks current status, roadmap, integration, architecture, and operations.

| Guide | Covers |
| --- | --- |
| [Examples](examples/README.md) | Commands, configuration variables, test requests, Loki queries, cleanup |
| [Axum integration](docs/axum.md) | Middleware placement, queue ownership, request IDs, errors, shutdown |
| [Grafana and tracing](docs/grafana-opentelemetry.md) | Subscriber wiring, trace IDs, LogQL, span-export boundary |
| [Production configuration](docs/production.md) | Limits, redaction, proxies, resilience, monitoring |
| [Geolocation](docs/geolocation.md) | External MMDB distribution and licensing |
| [Test and release matrix](docs/test-matrix.md) | Automated checks, external checks, release gates |
| [Migration](docs/migration.md) and [compatibility](docs/semver.md) | API changes and MSRV policy |

Report bugs or integration questions in [GitHub issues](https://github.com/sylvainTIASSOU/safe-http-telemetry/issues). The source is licensed under [MIT](LICENSE-MIT); optional external databases and the synthetic test fixture have their own terms in [third-party notices](THIRD_PARTY_NOTICES.md).

## Verify a checkout

```sh
cargo fmt --all -- --check
cargo test --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc --locked --no-deps --all-features
cargo +1.85.0 test --locked --all-targets --all-features -j 1
python3 -m unittest discover -s scripts -p 'test_*.py'
```

The real Loki tests and fuzz targets are described in the [test matrix](docs/test-matrix.md). Publication readiness is checked with `cargo publish --dry-run --locked --all-features` and a fresh `cargo package --locked --no-verify` archive inspection.
