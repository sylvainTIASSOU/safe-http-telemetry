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

The maintainer reports that every job is green in [GitHub Actions run 43648212579](https://github.com/sylvainTIASSOU/safe-http-telemetry/actions/runs/43648212579), including stable and Rust 1.85 CI and package inspection. All-target tests that bind loopback sockets pass in CI. The two real-Loki end-to-end tests passed separately against the documented Compose stack, and the config, policy, and queue property tests pass (5 total). All three fuzz targets also completed local 30-second campaigns; LeakSanitizer was disabled because this sandbox does not support its ptrace requirements. One external-MMDB test remains ignored unless a licensed local database is supplied. The semver job exited successfully but skipped all 260 lints because the initial stable release was classified as a major change. A 50-sample benchmark is recorded with its CPU-scaling caveat. The focused security review's Loki configuration debug and raw-path disclosure findings are fixed; raw paths are omitted from middleware events and covered by event and tracing regression tests. The first crates.io upload was rejected because one category slug was unsupported; it has been replaced with `network-programming`, and offline package verification passes. The crate is not published yet. The complete locked dependency license inventory is recorded in [`THIRD_PARTY_LICENSES.md`](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/THIRD_PARTY_LICENSES.md).

The project has **not** been published to crates.io. The README and repository documentation remain authoritative if this wiki snapshot becomes stale.
