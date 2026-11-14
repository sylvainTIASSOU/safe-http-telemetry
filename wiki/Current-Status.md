# Current status

**Snapshot date:** 2026-11-14  
**Candidate:** `0.1.0-rc.1`  
**MSRV:** Rust 1.85

## Implemented

- Axum and Tower HTTP lifecycle middleware with bounded capture profiles.
- Bounded event queue and asynchronous Loki exporter with retries and drain deadline.
- Redaction for supported structured fields and bounded request/response capture.
- `tracing` business events and W3C trace-context correlation.
- Optional external MMDB geolocation; production databases are not shipped in the crate.
- Runnable minimal Axum and Axum/Loki examples.

## Release readiness

The latest local validation recorded for the candidate includes stable and Rust 1.85 test suites, Clippy, rustdoc, real-Loki checks, and a package dry run. The candidate still has explicit release gates in the [release checklist](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/test-matrix.md), including security review, benchmark rerun, dependency license inventory, and final review of the package contents.

The project has **not** been published to crates.io. The README and repository documentation remain authoritative if this wiki snapshot becomes stale.
