# Contributing

## Requirements

- Use Rust 1.85.0 unless the MSRV is explicitly changed.
- Do not introduce unsafe code.
- Keep optional dependencies behind Cargo features.
- Add or update tests for every functional change.
- Run formatting, Clippy, tests and documentation checks.

## Architectural rules

- The core model must not depend on Axum.
- HTTP capture must not call Loki directly.
- The request path must not perform synchronous telemetry exports.
- Queues and buffers must be bounded.
- Exporter errors must not recursively enter the same logging pipeline.

## Sensitive data

Never use real credentials or personal data in tests.

Use recognizable fake secrets and verify that they are absent from exported logs and internal diagnostics.

## Fuzzing

The `fuzz/` directory contains `cargo-fuzz` targets for the query, header, and JSON parsers. Run them with the nightly toolchain:

```bash
cargo +nightly fuzz build
cargo +nightly fuzz run fuzz_query -- -max_total_time=30
cargo +nightly fuzz run fuzz_headers -- -max_total_time=30
cargo +nightly fuzz run fuzz_json_redaction -- -max_total_time=30
```

A nightly CI job runs the fuzzing targets for 10 minutes each. Crash artifacts are uploaded to the workflow run.

Property-based tests
The tests/prop_*.rs files use proptest to verify invariants on configuration limits, policy resolution, and queue capacity. Run them with:

bash
cargo test --locked --all-features --test prop_config
cargo test --locked --all-features --test prop_policy
cargo test --locked --all-features --test prop_queue
Property tests complement fuzzing: fuzzing searches for crashes on arbitrary bytes, while property tests verify business rules over generated inputs.

Before submitting a pull request
Run cargo fmt --all -- --check.

Run cargo clippy --locked --all-targets --all-features -- -D warnings.

Run cargo test --locked --all-targets --all-features.

Run cargo doc --locked --no-deps --all-features.

If you touched a parser or the redaction engine, run the fuzzing targets for at least 30 seconds each.

If you touched a public item, update docs/semver.md if the change is a minor or major bump.
