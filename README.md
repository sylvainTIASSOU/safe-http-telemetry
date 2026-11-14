# safe-http-telemetry

Safe, bounded HTTP telemetry for Axum, Tower, `tracing`, OpenTelemetry, and Grafana Loki.

> **Status:** preparing release candidate `0.1.0-rc.1`. The API and package contents are under final review. Feedback is welcome through GitHub issues.

## Project decisions

- License: MIT
- Minimum supported Rust version (MSRV): **1.85.0**
- Rust edition: 2021
- Initial HTTP integration: Axum through Tower
- Event format: versioned JSON
- Geolocation: optional local MMDB lookup from an explicit file path; production databases are downloaded separately during deployment

## Features

- Tower middleware and an Axum adapter for HTTP lifecycle events.
- Bounded request and response capture with configurable redaction.
- Bounded asynchronous event queue and Loki exporter.
- Native `tracing` business-event capture and OpenTelemetry context correlation.
- Optional local MMDB geolocation with trusted-proxy support.
- Internal counters for queue saturation, export failures, and shutdown outcomes.

The Cargo features are opt-in; the default feature set is empty:

| Feature | Enables |
|---|---|
| `axum` | Axum middleware and normalized `MatchedPath` routes |
| `tower` | Generic Tower HTTP middleware |
| `tracing` | `tracing` event capture and correlation support |
| `opentelemetry` | W3C Trace Context propagation and span correlation |
| `body-capture` | Bounded request and response body capture |
| `geolocation` | Local MMDB lookups and bounded cache |
| `loki` | Asynchronous Loki exporter |

See [docs/semver.md](docs/semver.md) for the compatibility policy and [docs/migration.md](docs/migration.md) for upgrade guidance.

## Quick start

Add only the features your application needs:

```toml
[dependencies]
safe-http-telemetry = { version = "0.1.0-rc.1", features = ["axum", "loki", "tracing"] }
```

For complete setup, shutdown ordering, Loki configuration, and runnable examples, see [the examples guide](examples/README.md) and [the Axum integration guide](docs/axum.md).

## Project structure

- `src/config`: configuration and validation
- `src/policy`: capture profiles and route policies
- `src/redaction`: sensitive-data redaction
- `src/tower_integration` and `src/axum_adapter`: HTTP middleware
- `src/tracing_integration`: tracing events and context propagation
- `src/loki` and `src/runtime`: export, queues, workers, and shutdown
- `src/geolocation`: local MMDB lookup
- `tests`: unit and integration coverage
- `fuzz`: nightly fuzz targets
- `benches`: Criterion benchmarks
- `schemas`: versioned event schemas
- `docs`: operational and architectural documentation

This repository contains a library crate; it has no `src/main.rs`.

## Development and validation

The repository toolchain is Rust 1.99.1. The crate's supported minimum is Rust 1.85.0.

Run the same core checks as CI:

```sh
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo test --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo doc --locked --no-deps --all-features
cargo +1.85.0 test --locked --all-targets --all-features -j 1
cargo publish --dry-run --locked --all-features
```

The real-Loki end-to-end tests, fuzz targets, benchmarks, and release checklist are described in [docs/test-matrix.md](docs/test-matrix.md). The production limits and operational guidance are in [docs/production.md](docs/production.md).

See [the geolocation guide](docs/geolocation.md) for the pinned download script, configuration, Docker mounts, and database updates.

## Performance

Benchmark methodology and the latest recorded run are documented in [docs/benchmarks.md](docs/benchmarks.md). Results depend on hardware, operating system, toolchain, and workload; record those details with any results used for release decisions.
