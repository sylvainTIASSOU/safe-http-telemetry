# Contributing

## Requirements

- Use Rust 1.85.0 or newer; Rust 1.85.0 is the declared MSRV.
- Do not introduce unsafe code.
- Keep optional dependencies behind Cargo features.
- Add or update tests for functional changes.
- Run formatting, lint, test, and documentation checks before opening a pull request.

## Architectural rules

- The core model must not depend on Axum.
- HTTP capture must not call Loki directly.
- The request path must not perform synchronous telemetry exports.
- Queues, buffers, body captures, retries, and shutdown waits must remain bounded.
- Exporter errors must not recursively enter the same logging pipeline.

## Sensitive data

Use only fictitious credentials and personal data in tests. Verify that fake secrets are absent from exported events and diagnostic output.

## Property-based tests

The `tests/prop_*.rs` tests use `proptest` to check configuration limits, route-policy resolution, and queue capacity:

```sh
cargo test --locked --all-features --test prop_config
cargo test --locked --all-features --test prop_policy
cargo test --locked --all-features --test prop_queue
```

Property tests complement fuzzing: fuzzing searches arbitrary byte inputs for crashes, while property tests check defined invariants across generated values.

## Fuzzing

The `fuzz/` crate contains `cargo-fuzz` targets for query parameters, headers, and JSON redaction. Fuzzing requires nightly Rust and `cargo-fuzz`:

```sh
cargo +nightly fuzz build
cargo +nightly fuzz run fuzz_query -- -max_total_time=30
cargo +nightly fuzz run fuzz_headers -- -max_total_time=30
cargo +nightly fuzz run fuzz_json_redaction -- -max_total_time=30
```

The scheduled CI workflow runs each target for ten minutes and uploads crash artifacts.

## Before submitting a pull request

Run the core checks used by CI:

```sh
cargo fmt --all -- --check
cargo check --locked --all-targets --all-features
cargo test --locked --all-targets --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo doc --locked --no-deps --all-features
cargo +1.85.0 test --locked --all-targets --all-features -j 1
```

If a change affects parsers or redaction, run each relevant fuzz target for at least 30 seconds. If a change modifies public API, review the compatibility policy in `docs/semver.md` and update the changelog as needed.
