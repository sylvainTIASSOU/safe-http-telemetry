# safe-http-telemetry

Safe, bounded HTTP telemetry for Axum, Tower, `tracing`, OpenTelemetry and Grafana Loki.

&gt; **Status:** project scaffolding. The crate is not published and does not provide production functionality yet.

## Project decisions

- Code license: MIT
- Minimum supported Rust version: 1.85
- Rust edition: 2021
- Initial HTTP target: Axum through Tower
- Log format: versioned JSON
- Geolocation: local MMDB database without synchronous external requests

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
