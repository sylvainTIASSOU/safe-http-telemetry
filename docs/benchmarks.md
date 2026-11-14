# Performance Benchmarks

This document outlines the performance overhead introduced by `safe-http-telemetry` across different capture profiles.

## Methodology

Benchmarks are implemented using [Criterion.rs](https://github.com/bheisler/criterion.rs).
To ensure accurate measurements of the middleware logic without network noise, requests are simulated using `tower::ServiceExt::oneshot`.

- **Hardware/Environment**: (À compléter avec ton CPU, ex: *AMD Ryzen 7 5800X, Linux*)
- **Rust Version**: 1.80+
- **Command**: `cargo bench --all-features`

## Results (Median Times)

| Profile | Time (µs) | Overhead vs Baseline | Notes |
| :--- | :--- | :--- | :--- |
| **Baseline (No Middleware)** | ~1.7 µs | - | Raw Axum routing. |
| **Minimal** | ~21.0 µs | +19.3 µs | Core context creation, timing, and queue push. |
| **Standard** | ~45.8 µs | +44.1 µs | Adds query parsing, header extraction, and redaction. |
| **Detailed** | ~46.3 µs | +44.6 µs | Adds body capture and recursive JSON redaction. |
| **Standard + Geolocation** | ~21.2 µs | +19.5 µs | Fast-fail when no IP is present (or cache hit). |

## Analysis

1. **Queue Push is cheap**: The jump from Baseline to Minimal (~19µs) represents the cost of async telemetry, which is well within acceptable limits for high-throughput services.
2. **Header Redaction is the main cost**: Moving from Minimal to Standard doubles the time. This is due to string allocations, case-insensitive matching, and recursive redaction of headers.
3. **Body Capture is negligible**: Surprisingly, the Detailed profile (which parses JSON and redacts bodies) is almost identical to Standard. For small to medium payloads, the JSON parsing cost is dwarfed by the header processing overhead.
4. **Geolocation is non-blocking**: The fast-fail mechanism when no IP is detected ensures that the feature does not degrade performance when not applicable.

## Recommendations

- Use `Minimal` for high-frequency, low-value endpoints (e.g., health checks, metrics).
- Use `Standard` for most APIs where headers and query parameters are needed for debugging.
- Use `Detailed` only on critical business routes (e.g., checkout, authentication) where body context is mandatory, and always enforce strict limits.

```markdown
## Load Test

The Criterion benchmarks measure per-request overhead in isolation. For end-to-end throughput, use a load generator against the example server.

### Commands

```bash
# Start the example server
cargo run --example axum_loki --features axum,loki,geolocation,body-capture

# In another terminal, run the load test
oha -z 30s -c 50 http://127.0.0.1:3030/hello/fictitious-user
What to observe
Request throughput (requests per second) with and without the middleware.

p99 latency increase.

InternalTelemetry::snapshot(): accepted_events, dropped_events, queue_occupancy.

Memory (RSS) of the example process: it must remain bounded.

Note: Load tests are environment-dependent. Record the toolchain, the lockfile hash, and the hardware in the release notes.

text

---
