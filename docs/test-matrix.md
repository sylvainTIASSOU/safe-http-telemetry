# Test Matrix

This document lists every test category in `safe-http-telemetry`, the files that contain them, and the scenarios they cover. It is the reference for the release candidate checklist.

## 1. Unit Tests

Unit tests live inside `src/` modules (`#[cfg(test)] mod tests`). They cover isolated logic without network or HTTP.

| Area | File | Scenarios covered |
|---|---|---|
| Configuration validation | `src/config/mod.rs` | Zero limits, inconsistent limits, default values |
| Capture profiles and route policies | `src/policy/mod.rs` | Profile ordering, exact vs prefix, method specificity |
| Redaction engine | `src/redaction/mod.rs` | Case-insensitive matching, nested objects, string limits |
| Queue and runtime | `src/runtime/queue.rs` | Capacity, saturation, close semantics |
| Client IP and trusted proxies | `src/tower_integration/client_ip.rs` | Forwarded parsing, untrusted peers |
| Request body capture | `src/tower_integration/request_body.rs` | Frame forwarding, trailer preservation |

## 2. Integration Tests (no network)

These tests exercise the middleware and the exporter against mock services, without real Loki or real HTTP sockets.

| File | Scenarios covered |
|---|---|
| `tests/axum_middleware.rs` | Route normalization, stream preservation, excluded routes |
| `tests/body_capture.rs` | JSON, form, text, binary, truncated bodies, invalid UTF-8 |
| `tests/business_events.rs` | Business event schema, queue interleaving, correlation |
| `tests/business_tracing.rs` | `tracing` layer, field bounds, queue saturation |
| `tests/client_ip.rs` | Direct peer, trusted chain, forwarding headers, labels |
| `tests/config.rs` | Validation edge cases, default configuration |
| `tests/core.rs` | Event schema, serialization, capture counters |
| `tests/geolocation.rs` | Bundled MMDB, cache bounds, field selection |
| `tests/geolocation_http.rs` | Layer integration, cache sharing, minimal profile |
| `tests/http_cancellation.rs` | Future drop, cancellation event, state release |
| `tests/http_failures.rs` | Handler errors, timeouts, body errors |
| `tests/http_metadata.rs` | Query and header capture, route-specific policies |
| `tests/http_response_preservation.rs` | Ordinary body, streaming, trailers, empty body |
| `tests/loki.rs` | URL validation, labels, structured metadata, timestamp |
| `tests/loki_batch.rs` | Event limits, byte limits, oversized events |
| `tests/loki_client.rs` | Retryable vs permanent errors, auth, timeouts |
| `tests/loki_exporter.rs` | Worker count, batching, retries, oversized drop |
| `tests/loki_shutdown.rs` | Drain, deadline, abandoned events |
| `tests/opentelemetry_distributed.rs` | Multi-span correlation |
| `tests/opentelemetry_middleware.rs` | W3C incoming context, span/event identity |
| `tests/opentelemetry_propagation.rs` | Extraction and injection of W3C context |
| `tests/policy.rs` | Profile elevation rules, route specificity |
| `tests/project-layout.rs` | Package identity |
| `tests/redaction.rs` | Fake secrets at every nesting level, query, headers |
| `tests/request_body_capture.rs` | Fragmented JSON, limit truncation, cancellation |
| `tests/response_body_capture.rs` | Fragmented JSON, limit truncation, stream errors |
| `tests/runtime_queue.rs` | Bounded queue, saturation, cloned receivers |
| `tests/tower_middleware.rs` | Minimal event, saturation, request ID propagation |
| `tests/telemetry_export_metrics.rs` | Export duration, in-flight batch gauge |
| `tests/loki_long_outage.rs` | Prolonged outage, recovery, shutdown deadline |

## 3. End-to-End Tests (real Loki)

These tests require a running Loki instance from `examples/loki/docker-compose.yaml`. They are marked `#[ignore]` and run manually or in a dedicated CI job.

| File | Scenarios covered |
|---|---|
| `tests/loki_http_e2e.rs` | Minimal event ingestion; detailed event with secrets, geolocation, labels |

Run them with:

```bash
docker compose -f examples/loki/docker-compose.yaml up -d
cargo test --locked --all-features --test loki_http_e2e -- --ignored



4. Property-Based Tests
These tests use proptest to verify invariants over generated inputs.

File	Invariant
tests/prop_config.rs	Zero limits are always rejected; max_event_bytes > max_loki_batch_bytes is rejected
tests/prop_policy.rs	Empty policy set returns the default profile; exact policy beats prefix policy
tests/prop_queue.rs	Occupancy never exceeds capacity; accepted + dropped = total attempts
Run them with:

bash
cargo test --locked --all-features --test prop_config
cargo test --locked --all-features --test prop_policy
cargo test --locked --all-features --test prop_queue
5. Fuzzing
cargo-fuzz targets live in fuzz/. They search for panics, aborts, and memory errors in the parsers.

Target	Entry point	What it fuzzes
fuzz_json_redaction	fuzz/fuzz_targets/fuzz_json_redaction.rs	JSON parsing + recursive redaction
fuzz_headers	fuzz/fuzz_targets/fuzz_headers.rs	Header map redaction
fuzz_query	fuzz/fuzz_targets/fuzz_query.rs	Query parameter redaction
Run them locally with:

bash
cargo +nightly fuzz build
cargo +nightly fuzz run fuzz_json_redaction -- -max_total_time=30
cargo +nightly fuzz run fuzz_headers -- -max_total_time=30
cargo +nightly fuzz run fuzz_query -- -max_total_time=30
A nightly CI job (.github/workflows/fuzz-nightly.yml) runs each target for 10 minutes.

6. Resilience Tests
These tests verify behaviour under failure and saturation.

File	Scenario
tests/loki_long_outage.rs	Loki returns 503 indefinitely; queue saturates; drops are counted
tests/loki_long_outage.rs	Loki recovers after transient failures; exported events increase
tests/loki_shutdown.rs	Shutdown deadline expires; abandoned events are counted
7. Benchmarks
Benchmarks measure the overhead of each capture profile. They are not pass/fail tests, but they are part of the release checklist.

File	Profiles measured
benches/middleware_overhead.rs	Baseline, Minimal, Standard, Detailed, Standard + Geolocation
Run them with:

bash
cargo bench --all-features
Results are documented in docs/benchmarks.md.

8. Release Candidate Checklist
Before tagging v0.1.0-rc.1:

□ cargo test --locked --all-targets --all-features passes.
□ cargo clippy --locked --all-targets --all-features -- -D warnings passes.
□ cargo fmt --all -- --check passes.
□ cargo doc --locked --no-deps --all-features passes.
□ cargo +1.85.0 test --locked --all-targets --all-features -j 1 passes (MSRV).
□ cargo test --locked --all-features --test loki_http_e2e -- --ignored passes against a real Loki.
□ Each fuzz target ran for at least 30 seconds locally without crash.
□ cargo publish --dry-run passes.
□ The security review has no open findings.
text

---
