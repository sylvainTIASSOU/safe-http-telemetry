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

These tests do not require a real Loki instance. Most use in-process services and mocks. The `loki_client`, `loki_exporter`, `loki_long_outage`, and `loki_shutdown` targets bind local loopback TCP listeners to simulate Loki; they need permission to open local sockets. The ignored `loki_http_e2e` tests use an actual Loki service.

| Test target | Coverage |
|---|---|
| `axum_middleware` | Route normalization, streaming, and route exclusions |
| `body_capture`, `request_body_capture`, `response_body_capture` | JSON, forms, text, binary data, truncation, invalid UTF-8, cancellation, and stream errors |
| `business_events`, `business_tracing` | Business-event schema, tracing layer, correlation, and queue saturation |
| `client_ip`, `geolocation`, `geolocation_http` | Direct and trusted-proxy IPs, labels, local fixture lookup, and bounded cache |
| `config`, `core`, `policy`, `project-layout` | Configuration, event schema, policy resolution, and package identity |
| `http_cancellation`, `http_failures`, `http_metadata`, `http_response_preservation` | Cancellation, handler/body errors, metadata, route policies, streaming, and response preservation |
| `loki`, `loki_batch` | Payload format, bounded labels, and batch limits; these tests do not bind sockets |
| `loki_client`, `loki_exporter`, `loki_shutdown` | HTTP authentication, retry classification, worker behavior, drain, and deadlines; bind loopback sockets |
| `opentelemetry_distributed`, `opentelemetry_middleware`, `opentelemetry_propagation` | Multi-span correlation and W3C context extraction/injection |
| `redaction` | Fake-secret removal across supported input locations |
| `runtime_queue`, `tower_middleware`, `telemetry_export_metrics` | Bounded queue behavior, request IDs, saturation, and exporter metrics |
| `loki_long_outage` | Prolonged outage, recovery, drops, and shutdown behavior; binds loopback sockets |

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

`loki_long_outage` and `loki_shutdown` exercise exporter failure, queue saturation, recovery, drain deadlines, and abandoned-event accounting. Redaction tests use fictitious secrets and assert that values do not appear in exported output. These tests are included in the standard all-features test command and require local loopback socket access.

## Checks requiring external services or tools

### Real-Loki end-to-end tests

The tests in `tests/loki_http_e2e.rs` are ignored by default and require Docker, local loopback socket access, and the Loki service in `examples/loki/docker-compose.yaml`.

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
- `cargo semver-checks --baseline-rev <base-revision>` compares the proposed public API with the pull request's base commit or the preceding push commit. This Git baseline is required before the first crates.io release because there is no published version for the registry-based baseline lookup.
- `python3 -m unittest discover -s scripts -p 'test_*.py'`
- `cargo publish --dry-run --locked --all-features`, then `cargo package --locked --no-verify` with archive size and contents checked

The real-Loki tests and fuzzing are not part of that pull-request workflow.

## CI verification snapshot (2026-11-14)

The maintainer supplied successful CI logs for the current `0.1.0` commit:

- Formatting, all-target/all-feature check and tests, Clippy, rustdoc, installer tests, MSRV tests, package inspection, and publish dry-run completed successfully.
- Both stable and Rust 1.85.0 test runs report 37 targets, 197 passed, 0 failed, and 3 ignored. The ignored tests are one external-MMDB lookup requiring `SAFE_HTTP_TEST_MMDB` and two real-Loki tests in `loki_http_e2e`.
- The package dry-run contains 107 files: 616.7 KiB unpacked and 149.0 KiB compressed. The production database directory is excluded; the licensed synthetic fixture and dependency license inventory are included. Cargo's `aborting upload due to dry run` warning is expected and confirms that this job did not publish the crate.
- `cargo-semver-checks` exited successfully using Git baseline `e6da66939fe103b07f890e832ca2ba0701e5e32f`. It classified `0.1.0-rc.1` → `0.1.0` as a major change and skipped all 260 individual lints (0 checked). This is expected for the first stable publication, which has no released API to preserve; it is not evidence that 260 API checks passed. Subsequent releases should compare against the previous stable API.
- GitHub reported a non-blocking Node 20 deprecation notice for `actions/checkout@v4`. The CI run still passed; update the action runtime separately to remove the notice.
- All three fuzz targets built and completed 30-second campaigns without a finding. This sandbox cannot run LeakSanitizer under its ptrace restrictions, so the campaigns used `ASAN_OPTIONS=detect_leaks=0`; AddressSanitizer checks remained active. An initial `crash-*` artifact was reproduced as this LeakSanitizer environment failure and was not a code crash.
- The maintainer ran the ignored real-Loki end-to-end tests against the documented Compose stack: 2 passed, 0 failed (minimal event export and detailed event redaction/export).
- A 50-sample Criterion run was recorded on the documented host with a new named baseline. CPU scaling was enabled and some profiles had outliers; use the results as a host-specific release snapshot, not as a general performance guarantee. Details are in [benchmarks.md](benchmarks.md).
- After the security changes, the focused all-feature Axum, Tower, and OpenTelemetry middleware test targets pass: 18 passed, 0 failed. Formatting and `git diff --check` also pass. Rerun the full CI workflow, including Rust 1.85 and the release package dry-run, against the final release commit.

### Security review finding and resolution

Raw request paths are now omitted from middleware-generated events: the schema's `request.path` field contains `[OMITTED]`. Route templates remain available, and custom `RouteResolver` implementations are required to return stable, non-sensitive templates rather than raw paths. Regression tests send a fake reset token in an unmatched path and verify it is absent from both the serialized event and tracing output. The Loki configuration debug leak found in the review is also fixed and covered by a targeted test.

## First stable release checklist

Complete this checklist for the exact commit proposed for release. Attach or link the CI run and record external validation results; do not mark items complete from the presence of commands or workflows alone.

- [ ] Rerun stable-toolchain CI against the final release commit after the path-omission change.
- [ ] Rerun unfiltered all-target, all-feature tests on stable and Rust 1.85.0 against the final release commit, including tests that bind loopback sockets. The prior run had three environment-dependent tests ignored.
- [ ] Rerun real-Loki end-to-end tests against the final release commit; the previous source passed 2 tests, 0 failed.
- [x] All three fuzz targets have completed at least 30 seconds without a finding. This local run disabled LeakSanitizer because the sandbox does not support its ptrace requirements.
- [x] A 50-sample benchmark is recorded with hardware, operating system, toolchain, commit, and lockfile hash. CPU frequency scaling and per-profile outliers are documented as limits on comparisons.
- [x] The identified raw-path disclosure is resolved by omitting raw paths from middleware events; regression tests cover event serialization and tracing output.
- [x] The package archive is below the target registry limit and excludes production MMDB files. The licensed synthetic test fixture and license inventory are included.
- [x] `cargo publish --dry-run --locked --all-features` succeeds with registry access; the job reports the 107-file package and size.
- [x] The locked dependency license inventory is recorded in [THIRD_PARTY_LICENSES.md](../THIRD_PARTY_LICENSES.md); review it when updating `Cargo.lock`.
- [x] Version, changelog, README status, and release notes agree on the planned `0.1.0` first release.

## Database installer checks

Run `python3 -m unittest discover -s scripts -p 'test_*.py'`. These offline checks cover checksum failures, corrupted archives, size limits, invalid input, and preservation of the previous file on failure.
