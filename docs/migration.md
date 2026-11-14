# Migration Guide

This guide describes how to migrate between versions of `safe-http-telemetry` and how to handle schema changes.

## 1. Versioning

`safe-http-telemetry` follows SemVer. The public log schema carries an independent `schema_version` field. See `docs/semver.md` for the full policy.

| Crate version | Schema version | Notes |
|---|---|---|
| `0.1.x-dev` | `1.0` | Development releases. API not stable. |
| `0.1.0` (candidate) | `1.0` | First release candidate. API frozen for the 0.1 line. |

## 2. Migrating from Development to `0.1.0`

### Feature flags
The following features are stable in `0.1.0`: `axum`, `tower`, `tracing`, `opentelemetry`, `body-capture`, `geolocation`, `loki`. Enable only the features you use.

### Configuration types
`CaptureLimits` is now validated at startup. If your previous configuration had a zero limit or an inconsistent `max_event_bytes > max_loki_batch_bytes`, the crate will refuse to start. Adjust the limits before upgrading.

### Redaction
The default sensitive-field list has been extended. If you were relying on a field being **not** redacted (e.g., a custom `token` field that you considered safe), explicitly remove it from `RedactionConfig::sensitive_fields` after a risk assessment.

### Geolocation
`GeolocationConfig::bundled()` now validates the MMDB at startup. If the database is missing or corrupt, startup fails with `GeolocationError::DatabaseInvalid`. Download the database and verify its checksum before deploying.

### Loki exporter
`LokiConfig::new(url)` rejects URLs with embedded credentials. Move credentials to the bearer-token or basic-auth fields.

## 3. Migrating Between Schema Versions

### `1.0` → `1.x` (minor)
- New optional fields are added. Existing fields are unchanged.
- Grafana queries that filter on `schema_version = "1.0"` continue to work but will not match new events. Update to `schema_version =~ "1\\..*"` if you want to include all `1.x` events.

### `1.x` → `2.0` (major)
- Fields may be removed, renamed, or change type. Review the changelog before upgrading.
- Dashboards pinned to `1.x` must be updated in the same deployment window.

## 4. Deprecation Policy

When a public item is deprecated:
- It is marked `#[deprecated]` in a minor release.
- It remains available for at least one subsequent minor release.
- The deprecation message points to the replacement.

Do not remove a deprecated item in a patch release.

## 5. MSRV

The MSRV is declared in `Cargo.toml` as `rust-version = "1.85"`. A change to the MSRV is announced in the changelog. If you use an older toolchain, pin the crate version that supports it.
