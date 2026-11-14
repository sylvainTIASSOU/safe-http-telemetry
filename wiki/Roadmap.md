# Roadmap

This roadmap records project-level milestones, not a promise of dates. Release gates are tracked in the repository's [test matrix](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/test-matrix.md).

## Implemented for the planned `0.1.0` release

- Implement bounded Axum/Tower lifecycle telemetry and route policies.
- Add redaction and optional bounded request/response body capture.
- Add the bounded asynchronous Loki exporter and graceful shutdown reporting.
- Add `tracing` business events and OpenTelemetry correlation.
- Move production MMDB data out of the crate and document external installation.
- Provide runnable Axum examples and stable/MSRV validation.

## Before the first publication

- Complete security review and address release-blocking findings.
- Rerun and record benchmarks against a clean baseline.
- Review the complete locked dependency license inventory.
- Recheck changelog, README, package file list, archive size, and dry-run publication.
- Publish `0.1.0` only after the release gates are complete.

## After the first release

- Gather integration feedback and report incompatibilities or operational gaps.
- Maintain the `0.1` API compatibility policy and gather integration feedback.
- Consider machine-readable JSON Schema files and published docs.rs/crates.io links when their maintenance and compatibility policy are defined.

Open work is tracked through the repository's issues and pull requests; this page should be updated when those priorities change.
