# Test Matrix and Release Checklist

This document maps the project's automated checks to the relevant test files and lists validation that requires external services or maintainer review. CI is authoritative for automated results; this checklist does not claim that a check has passed unless its run is recorded.

## Automated test coverage

### Unit tests

Unit tests are colocated with the implementation under `src/`.

| Area | Source module | Coverage |
|---|---|---|
| Configuration | `src/config/mod.rs` | Default values, zero limits, and inconsistent limits |
| Capture policy | `src/policy/mod.rs` | Profile ordering and route matching |
| Redaction | `src/redaction/mod.rs` | Case-insensitive matching, nested values, and bounds |
| Runtime queue | `src/runtime/queue.rs` | Capacity, saturation, and close behavior |
| Client IP | `src/tower_integration/client_ip.rs` | Forwarded-address parsing and trusted proxies |
| Request body | `src/tower_integration/request_body.rs` | Frame forwarding and trailer preservation |

### Integration tests

These tests use in-process services and mocks; they do not require a real Loki instance.

| Test target | Coverage |
|---|---|
| `axum_middleware` | Route normalization, streaming, and route exclusions |
| `body_capture`, `request_body_capture`, `response_body_capture` | JSON, forms, text, binary data, truncation, invalid UTF-8, cancellation, and stream errors |
| `business_events`, `business_tracing` | Business-event schema, tracing layer, correlation, and queue saturation |
| `client_ip`, `geolocation`, `geolocation_http` | Direct and trusted-proxy IPs, labels, local fixture lookup, and bounded cache |
| `config`, `core`, `policy`, `project-layout` | Configuration, event schema, policy resolution, and package identity |
| `http_cancellation`, `http_failures`, `http_metadata`, `http_response_preservation` | Cancellation, handler/body errors, metadata, route policies, streaming, and response preservation |
| `loki`, `loki_batch`, `loki_client`, `loki_exporter`, `loki_shutdown` | Payload format, batching, retries, authentication, worker behavior, drain, and deadlines |
| `opentelemetry_distributed`, `opentelemetry_middleware`, `opentelemetry_propagation` | Multi-span correlation and W3C context extraction/injection |
| `redaction` | Fake-secret removal across supported input locations |
| `runtime_queue`, `tower_middleware`, `telemetry_export_metrics` | Bounded queue behavior, request IDs, saturation, and exporter metrics |
| `loki_long_outage` | Prolonged outage, recovery, drops, and shutdown behavior |

### Property-based tests

| Target | Invariant |
|---|---|
| `prop_config` | Zero limits and event limits larger than the batch limit are rejected |
| `prop_policy` | Empty sets use the default profile; exact policies beat prefix policies |
| `prop_queue` | Queue occupancy stays within capacity and accepted/rejected counts match attempts |

Run the property tests individually:

```sh
cargo test --locked --all-features --test prop_config
cargo test --locked --all-features --test prop_policy
cargo test --locked --all-features --test prop_queue
```

### Resilience and security coverage

`loki_long_outage` and `loki_shutdown` exercise exporter failure, queue saturation, recovery, drain deadlines, and abandoned-event accounting. Redaction tests use fictitious secrets and assert that values do not appear in exported output. These tests are included in the standard all-features test command.

## Checks requiring external services or tools

### Real-Loki end-to-end tests

The tests in `tests/loki_http_e2e.rs` are ignored by default and require the Loki service in `examples/loki/docker-compose.yaml`.

```sh
docker compose -f examples/loki/docker-compose.yaml up -d
cargo test --locked --features axum,loki,body-capture --test loki_http_e2e -- --ignored --test-threads=1
docker compose -f examples/loki/docker-compose.yaml down
```

See [examples/README.md](../examples/README.md) for readiness checks and configuration options.

### Fuzzing

The `fuzz/` crate provides `fuzz_json_redaction`, `fuzz_headers`, and `fuzz_query` targets. Fuzzing requires nightly Rust and `cargo-fuzz`:

```sh
cargo +nightly fuzz build
cargo +nightly fuzz run fuzz_json_redaction -- -max_total_time=30
cargo +nightly fuzz run fuzz_headers -- -max_total_time=30
cargo +nightly fuzz run fuzz_query -- -max_total_time=30
```

The scheduled workflow in `.github/workflows/fuzz-nightly.yml` runs each target for ten minutes and uploads crash artifacts.

### Benchmarks and load tests

Run Criterion benchmarks with:

```sh
cargo bench --locked --all-features
```

For end-to-end throughput and latency measurements, follow the load-test procedure in [docs/benchmarks.md](benchmarks.md). Record the toolchain, hardware, workload, and lockfile hash with results.

## CI checks

The pull-request and push workflow runs:

- `cargo fmt --all -- --check`
- `cargo check --locked --all-targets --all-features`
- `cargo test --locked --all-targets --all-features`
- `cargo clippy --locked --all-targets --all-features -- -D warnings`
- `cargo doc --locked --no-deps --all-features` with rustdoc warnings denied
- Check and test on the declared MSRV, Rust 1.85.0
- `cargo semver-checks check-release`
- `python3 -m unittest discover -s scripts -p 'test_*.py'`
- `cargo publish --dry-run --locked --all-features`, then `cargo package --locked --no-verify` with archive size and contents checked

The real-Loki tests and fuzzing are not part of that pull-request workflow.

## Release candidate checklist

Complete this checklist for the exact commit proposed for release. Attach or link the CI run and record external validation results; do not mark items complete from the presence of commands or workflows alone.

- [ ] Stable-toolchain CI checks pass.
- [ ] All-target, all-feature tests pass on Rust 1.85.0.
- [ ] Real-Loki end-to-end tests pass against the documented Loki configuration.
- [ ] All three fuzz targets have completed at least 30 seconds without a crash.
- [ ] Benchmark results include hardware, operating system, toolchain, and lockfile hash.
- [ ] Security review is complete and has no open release-blocking findings.
- [ ] The package archive is below the target registry limit and excludes production MMDB files. Only the licensed synthetic test fixture is included.
- [ ] `cargo publish --dry-run --locked --all-features` and `cargo package --locked --no-verify` succeed; review the packaged file list and size.
- [ ] Version, changelog, README status, and release notes agree.

## Database installer checks

Run `python3 -m unittest discover -s scripts -p 'test_*.py'`. These offline checks cover checksum failures, corrupted archives, size limits, invalid input, and preservation of the previous file on failure.
