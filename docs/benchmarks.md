# Performance Benchmarks

This guide describes the Criterion benchmarks and the additional measurements needed before making production performance claims.

## Criterion benchmark

The benchmark in `benches/middleware_overhead.rs` uses `tower::ServiceExt::oneshot` to measure middleware request-processing overhead without network traffic. It compares the baseline service with the Minimal, Standard, Detailed, and Standard-with-geolocation configurations.

Run the benchmark with:

```sh
cargo bench --locked --all-features
```

Criterion writes estimates and raw samples under `target/criterion/`. Record the run date, commit, Rust version, operating system, CPU, and lockfile hash alongside any results used for a release decision. Do not compare numbers collected with different environments as if they were directly equivalent.

### Historical smoke-run results (before external database migration)

An exploratory run was completed on 2026-11-14 with Rust 1.99.1 using:

```sh
cargo bench --locked --all-features --bench middleware_overhead -- --sample-size 20
```

These measurements predate the external-MMDB migration and must be repeated before release. The geolocation case now opens the small test fixture; it still measures the no-client-IP path.

Environment: Fedora Linux x86_64, Intel Core i7-8565U CPU @ 1.80 GHz, eight logical CPUs. `Cargo.lock` SHA-256: `93fae80d240c36b23d86e2771526ec51c127f8a31d05d930989036c8a90420dd`.

| Profile | Criterion estimate | Criterion interval | Notes |
|---|---:|---:|---|
| Baseline | 3.2674 µs | 2.6911–3.9972 µs | No telemetry middleware |
| Minimal | 47.277 µs | 40.196–58.710 µs | Request lifecycle event |
| Standard | 67.277 µs | 63.076–72.897 µs | Query/header capture and redaction |
| Detailed | 65.461 µs | 60.891–72.621 µs | Bounded body capture and redaction |
| Standard + geolocation | 55.860 µs | 48.495–61.743 µs | No client IP is supplied, so this measures the no-IP path, not a lookup or cache hit |

This is a short local snapshot, not a release performance guarantee. Criterion reported a regression against its locally retained comparison data. The benchmark ran with 20 samples per case on a frequency-scaled laptop CPU, so host load and power state may explain some or all of that difference. Repeat it against a clean baseline on stable, documented hardware before using it to support production claims. The earlier undocumented figures in this file have been removed.

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
