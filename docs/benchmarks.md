# Performance Benchmarks

This guide describes the Criterion benchmarks and the additional measurements needed before making production performance claims.

## Criterion benchmark

The benchmark in `benches/middleware_overhead.rs` uses `tower::ServiceExt::oneshot` to measure middleware request-processing overhead without network traffic. It compares the baseline service with the Minimal, Standard, Detailed, and Standard-with-geolocation configurations.

Run the benchmark with:

```sh
cargo bench --locked --all-features
```

Criterion writes estimates and raw samples under `target/criterion/`. Record the run date, commit, Rust version, operating system, CPU, and lockfile hash alongside any results used for a release decision. Do not compare numbers collected with different environments as if they were directly equivalent.

### Local smoke run (2026-11-14)

A short exploratory run was completed against the current source with Rust 1.99.1 using:

```sh
cargo bench --locked --all-features --bench middleware_overhead --offline -- --sample-size 10
```

The geolocation case opens the small synthetic test fixture but supplies no client IP, so it measures the no-IP path and does not include a database lookup or cache hit.

Environment: Fedora Linux 7.3.3 x86_64, Intel Core i7-8565U CPU @ 1.80 GHz, eight logical CPUs, CPU frequency scaling enabled. Toolchain: `rustc 1.99.1`. `Cargo.lock` SHA-256: `47db3528dcbe34d1f6915aad038c7fb6de4139aaac3a1585ed9e03f3bbbb0a1f`.

| Profile | Criterion estimate | Criterion interval | Notes |
|---|---:|---:|---|
| Baseline | 2.237 µs | 2.089–2.527 µs | No telemetry middleware |
| Minimal | 24.228 µs | 22.258–27.318 µs | Request lifecycle event |
| Standard | 56.646 µs | 52.529–61.810 µs | Query/header capture and redaction |
| Detailed | 60.225 µs | 55.206–67.916 µs | Bounded body capture and redaction |
| Standard + geolocation | 23.535 µs | 22.192–25.666 µs | Fixture loaded; no client IP supplied |

This is a short local snapshot, not a release performance guarantee. Each case used 10 samples and Criterion's default 3-second warm-up and measurement window. The machine had frequency scaling enabled; Criterion also reported outliers and changes relative to locally retained comparison data. The cross-profile figures are sensitive to host load and power state. Repeat the benchmark on stable, documented hardware with a clean Criterion baseline before using it to support production claims.

Do not infer throughput limits or production suitability from these isolated per-request measurements alone.

## End-to-end load test

The Criterion benchmark does not measure network throughput, tail latency under concurrency, exporter saturation, or process memory under sustained load. Use a load generator against the example service for those measurements.

Start the example service in one terminal:

```sh
cargo run --locked --example axum_loki \
  --features axum,loki,tracing,opentelemetry
```

In another terminal, run a 30-second load test with 50 concurrent workers (install [`oha`](https://github.com/hatoo/oha) separately):

```sh
oha -z 30s -c 50 http://127.0.0.1:3030/hello/fictitious-user
```

Compare the baseline service to the configured profiles. Record:

- requests per second and p50/p95/p99 latency;
- CPU and peak resident memory;
- accepted and dropped event counts, queue occupancy, and export failures;
- Loki response time and behavior during a controlled outage.

Use fictitious request data. Repeat the run for the workload and payload sizes that resemble the target service. Load-test results are environment-specific and should not be presented as general crate guarantees.
