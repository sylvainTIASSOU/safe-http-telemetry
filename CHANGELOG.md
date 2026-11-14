# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0-rc.1] - 2026-11-14

### Added
- Tower middleware and Axum adapter for HTTP lifecycle observation.
- Bounded event queue with `DropNewest` saturation policy and loss counters.
- Asynchronous Loki exporter with batching, retries, TLS, authentication, and tenant support.
- Centralized recursive redaction engine for JSON, forms, query parameters, and headers.
- Local MMDB geolocation with bounded cache and trusted-proxy policy.
- `tracing` business event capture and W3C Trace Context correlation.
- Versioned JSON log schema (`schema_version: "1.0"`).
- Internal telemetry counters for queue occupancy, drops, export duration, and failures.
- Property-based tests for configuration limits, policy resolution, and queue capacity.
- `cargo-fuzz` targets for JSON, header, and query redaction.
- Prolonged outage and recovery tests for the Loki exporter.
- Criterion benchmarks comparing Minimal, Standard, Detailed, and geolocation profiles.
- Nightly fuzzing workflow with corpus caching and crash artifact upload.

### Changed
- MSRV raised to Rust 1.85.
- Public feature flags documented as stable in the `README`.

### Security
- Sensitive fields are redacted before any data reaches the export queue.
- `LokiConfig` rejects URLs with embedded credentials.
- Exporter errors are never fed back into the same Loki queue.

## [0.0.0] - 2026-11-08

### Added
- Initial development snapshot. The crate was not published.
