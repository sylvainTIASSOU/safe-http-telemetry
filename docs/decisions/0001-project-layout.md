# DEC-0011: Project Layout

- Status: Accepted
- Date: 2026-11-08

## Decision

Use a single Cargo library crate without a primary binary and without a premature workspace.

## Rationale

A single crate reduces publishing and versioning complexity.

Internal modules represent the architectural boundaries, while optional Cargo features prevent users from installing every dependency.

## Consequences

- `src/lib.rs` is the library entry point.
- `src/main.rs` does not exist.
- Integration tests belong to `tests/`.
- Examples belong to `examples/`.
- Benchmarks belong to `benches/`.
- Publishing remains disabled during initial development.
- The lockfile will be retained after successful MSRV validation.
