# Performance Benchmarks

This guide describes the Criterion benchmarks and the additional measurements needed before making production performance claims.

## Criterion benchmark

The benchmark in `benches/middleware_overhead.rs` uses `tower::ServiceExt::oneshot` to measure middleware request-processing overhead without network traffic. It compares the baseline service with the Minimal, Standard, Detailed, and Standard-with-geolocation configurations.

Run the benchmark with:

```sh
cargo bench --locked --all-features
```

Criterion writes estimates and raw samples under `target/criterion/`. Record the run date, commit, Rust version, operating system, CPU, and lockfile hash alongside any results used for a release decision. Do not compare numbers collected with different environments as if they were directly equivalent.

### 50-sample release run (2026-11-14)

A 50-sample run was recorded for the `0.1.0` source using a new named Criterion baseline:

```sh
cargo bench --locked --all-features --bench middleware_overhead -- \
  --sample-size 50 \
  --save-baseline safe-http-telemetry-v0.1.0-2026-11-14
```

The geolocation case opens the small synthetic test fixture but supplies no client IP, so it measures the no-IP path and does not include a database lookup or cache hit.

Environment: Fedora Linux 7.3.3 x86_64, Intel Core i7-8565U CPU @ 1.80 GHz, eight logical CPUs. Toolchain: `rustc 1.99.1`. Commit: `93a8f20`. `Cargo.lock` SHA-256: `47db3528dcbe34d1f6915aad038c7fb6de4139aaac3a1585ed9e03f3bbbb0a1f`. CPU frequency scaling was enabled; `lscpu` reported 44% at the time of collection.

| Profile | Estimate | 95% confidence interval | Outliers | Notes |
|---|---:|---:|---:|---|
| Baseline | 2.3843 µs | 2.2430–2.5415 µs | 4/50 (2 mild, 2 severe) | No telemetry middleware |
| Minimal | 27.314 µs | 26.299–28.658 µs | 3/50 (all severe) | Request lifecycle event |
| Standard | 60.163 µs | 58.064–62.554 µs | 0/50 | Query/header capture and redaction |
| Detailed | 71.576 µs | 64.709–79.444 µs | 5/50 (2 mild, 3 severe) | Bounded body capture and redaction |
| Standard + geolocation | 27.629 µs | 26.696–28.620 µs | 3/50 (1 mild, 2 severe) | Fixture loaded; no client IP supplied |

Each profile used 50 samples with the benchmark's 3-second measurement window. The uniquely named baseline keeps this run separate from older local results. Frequency scaling was enabled and several profiles had outliers, so treat these values as a documented release snapshot rather than a general performance guarantee. Repeat on stable, documented hardware before making comparative performance claims.

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
