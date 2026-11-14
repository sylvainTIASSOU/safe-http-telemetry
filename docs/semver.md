# SemVer and Schema Compatibility Policy

This document defines how `safe-http-telemetry` applies Semantic Versioning and how the public log schema evolves.

## 1. Semantic Versioning

The crate follows [SemVer 2.0.0](https://semver.org/).

### Major (X.y.z → (X+1).0.0)
- Removing or renaming a public type, trait, method, or feature flag.
- Changing the signature of a public function in a way that breaks existing callers.
- Removing a `CaptureProfile` variant, or changing the ordering semantics of profiles.
- Removing a `RedactionConfig` field, or changing the meaning of an existing sensitive-field rule.
- Changing the default value of a bounded limit in a way that rejects previously valid configurations.
- Removing a `LokiConfig` field or changing the retry/batching semantics incompatibly.
- Any change to the `schema_version` **major** number (e.g., `1.x` → `2.0`).

### Minor (X.y.z → X.(y+1).0)
- Adding a new optional field to a public configuration struct (with `Default` support).
- Adding a new feature flag that does not alter existing behavior when disabled.
- Adding a new `CaptureProfile` variant (as long as existing variants keep their ordering).
- Adding a new public method that does not change existing behavior.
- Adding a new optional field to the JSON log schema (e.g., a new `CaptureInfo` indicator).
- Any change to the `schema_version` **minor** number (e.g., `1.0` → `1.1`).

### Patch (X.y.z → X.y.(z+1))
- Bug fixes that do not change the public API.
- Documentation-only changes.
- Internal refactoring that preserves all public signatures.
- Fixing a redaction rule that was incorrectly matching (or failing to match) a sensitive field, **provided** the fix does not remove a previously redacted field.

## 2. Schema Versioning

The JSON log schema carries a `schema_version` field (currently `"1.0"`). Its lifecycle is independent from the crate version:

| Crate change | Schema impact | Action |
|---|---|---|
| Patch | No change | Keep `schema_version` unchanged. |
| Minor | Additive (new optional field) | Bump **minor** (`1.0` → `1.1`). |
| Major | Removal, rename, or type change | Bump **major** (`1.x` → `2.0`). |

Consumers (Grafana dashboards, Loki queries) can pin their queries to a specific `schema_version` to avoid breakage during a rolling upgrade.

## 3. Feature Flag Stability

Features listed in the `README.md` are considered stable once they appear in a tagged release. The following rules apply:

- **Adding a feature**: minor version bump.
- **Making a feature default**: minor version bump, with a deprecation notice in the previous minor.
- **Removing a feature**: major version bump.
- **Renaming a feature**: major version bump, with a deprecation alias for at least one minor release.

## 4. Deprecation Policy

- A public item may be marked `#[deprecated]` in a minor release.
- It must remain available for at least one subsequent minor release before removal in a major release.
- The deprecation message must reference the replacement item.

## 5. MSRV

The Minimum Supported Rust Version is declared in `Cargo.toml` (`rust-version = "1.85"`). A change to the MSRV is considered a **minor** change if it only raises the patch version of Rust, and a **major** change if it raises the minor version of Rust (e.g., 1.85 → 1.86).

## 6. Enforcement

- CI runs `cargo semver-checks --baseline-rev <base-revision>` against the pull request's base commit or the preceding push commit. This explicit Git baseline supports the first crates.io publication, when no released registry version exists to serve as a baseline. After a release, keep the default branch aligned with the latest published API before starting the next compatibility cycle.
- The `schema_version` constants and serialized event shapes in `src/core/event.rs` are checked by the tests listed in `schemas/README.md`; machine-readable JSON Schema files have not yet been added.
- Any PR that changes a public item without updating this policy is rejected.
