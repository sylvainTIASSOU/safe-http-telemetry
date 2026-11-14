# Current status

**Snapshot date:** 2026-11-14  
**Planned first release:** `0.1.0`
**MSRV:** Rust 1.85

## Implemented

- Axum and Tower HTTP lifecycle middleware with bounded capture profiles.
- Bounded event queue and asynchronous Loki exporter with retries and drain deadline.
- Redaction for supported structured fields and bounded request/response capture.
- `tracing` business events and W3C trace-context correlation.
- Optional external MMDB geolocation; production databases are not shipped in the crate.
- Runnable minimal Axum and Axum/Loki examples.

## Release readiness

Local validation includes Clippy, rustdoc, the Python installer tests, package inspection, and non-socket Rust test targets on stable and Rust 1.85. Socket-based Loki tests require an unrestricted local loopback environment; real-Loki tests, a release-quality benchmark rerun, and a networked publish dry run remain release gates. The dependency license inventory is recorded in [`THIRD_PARTY_LICENSES.md`](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/THIRD_PARTY_LICENSES.md).

The project has **not** been published to crates.io. The README and repository documentation remain authoritative if this wiki snapshot becomes stale.
