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
