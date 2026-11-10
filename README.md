# safe-http-telemetry

Safe, bounded HTTP telemetry for Axum, Tower, `tracing`, OpenTelemetry and Grafana Loki.

> **Status:** development release. The crate is not published. Tower and Axum
> HTTP lifecycle middleware, bounded queues and the Loki exporter are being
> validated incrementally; the public API is not stable yet.

## Project decisions

- Code license: MIT
- Minimum supported Rust version: 1.85
- Rust edition: 2021
- Initial HTTP target: Axum through Tower
- Log format: versioned JSON
- Geolocation: local MMDB database without synchronous external requests

## Axum integration

The `axum` feature provides normalized `MatchedPath` routes, bounded event
submission and response preservation for ordinary and streaming bodies.
See `docs/axum.md` for layer placement, timeout handling and shutdown order.

## Project structure

- `src/config`: configuration and validation
- `src/policy`: capture profiles and route policies
- `src/redaction`: sensitive-data redaction
- `src/tower_integration`: HTTP middleware
- `src/axum_adapter`: Axum integration
- `src/tracing_integration`: tracing and correlation
- `src/loki`: Loki exporter
- `src/runtime`: queues, workers and shutdown
- `src/geolocation`: local MMDB geolocation
- `tests`: integration tests
- `examples`: Cargo examples and data examples
- `benches`: benchmarks
- `schemas`: public data schemas
- `docs`: architecture and decision records

The project follows the standard Cargo package layout. There is no `src/main.rs` because this package is a library.

## Planned features

- `axum`
- `tower`
- `tracing`
- `opentelemetry`
- `body-capture`
- `geolocation`
- `loki`

These features do not currently expose a stable API.

## Development

Development uses Rust 1.99.1 through `rust-toolchain.toml`.
The minimum supported Rust version (MSRV) is 1.85.0.

Run:

```sh
cargo generate-lockfile
cargo check --all-targets --all-features
cargo test --all-targets --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo doc --no-deps --all-features
