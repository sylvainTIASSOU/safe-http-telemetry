# safe-http-telemetry Wiki

A practical guide to the `safe-http-telemetry` Rust crate and its release progress.

> **Status (2026-11-14):** release candidate `0.1.0-rc.1` is being prepared. It has not been published to crates.io. The supported MSRV is Rust 1.85.

## Start here

- [Current status](Current-Status): what works today and what remains before release.
- [Roadmap](Roadmap): completed work and release gates.
- [Getting started](Getting-Started): run the minimal Axum example or connect Loki.
- [Architecture](Architecture): request path, bounded queue, exporters, and event format.
- [Operations](Operations): production boundaries, security, monitoring, and shutdown.

## Project documentation

The repository documentation is the detailed reference and source of truth:

- [README](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/README.md)
- [Examples](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/examples/README.md)
- [Axum integration](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/axum.md)
- [Production configuration](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/production.md)
- [Test and release checklist](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/test-matrix.md)
- [Geolocation database](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/docs/geolocation.md)
- [Event format](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/schemas/README.md)

## Contributing

Use fictitious data in examples and tests. See the repository's [contribution guide](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/CONTRIBUTING.md) and [security policy](https://github.com/sylvainTIASSOU/safe-http-telemetry/blob/master/SECURITY.md).
