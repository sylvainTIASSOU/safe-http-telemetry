# Geolocation Database

The optional `geolocation` feature uses a local DB-IP City Lite MMDB database. Lookups make no network requests. The bundled database is included in the crate source and embedded in applications that enable geolocation, increasing their binary size by approximately the database size.

## Bundled database and attribution

The bundled release is **2026-11**, stored at `data/dbip-city-lite-2026-11.mmdb`. Its uncompressed size is 126,873,350 bytes. The repository tracks the file with Git LFS.

> **Publication blocker (checked 2026-11-14):** `cargo publish --dry-run` packages this file into a 59.0 MiB archive. The Cargo Book documents a 10 MB crates.io archive limit. The dry run verifies the package locally but does not perform the registry's final upload checks. The crate cannot be published to crates.io in its current form. Decide how to distribute the database separately or redesign the optional geolocation feature before a crates.io release.

DB-IP Lite is licensed under Creative Commons Attribution 4.0, separately from this project's MIT-licensed source code. Retain this attribution in distributed applications and documentation:

**IP Geolocation by [DB-IP](https://db-ip.com)**

See [DB-IP's Lite database terms](https://db-ip.com/db/lite.php) for license and download information.

## Enable geolocation in an application

Add the required Cargo features and explicitly enable geolocation when building the middleware:

```toml
[dependencies]
safe-http-telemetry = { version = "0.1", features = ["axum", "geolocation"] }
```

```rust
use safe_http_telemetry::{
    geolocation::GeolocationConfig,
    tower_integration::MiddlewareConfig,
};

let config = MiddlewareConfig {
    geolocation: Some(GeolocationConfig::bundled()),
    ..MiddlewareConfig::default()
};
```

The middleware default is `geolocation: None`; IP capture and lookup are opt-in. The bundled database is opened and validated when the layer is built. The default configuration retains the country code and English country name. Client-IP resolution follows the trusted-proxy rules in `ClientIpConfig`. The Minimal capture profile performs neither client-IP capture nor geolocation lookup.

IP addresses and geographic values remain in the JSON event. They are not used as Loki labels or structured metadata.

## Use an external local database

An external local MMDB is available for tests and advanced use cases:

```rust
let config = GeolocationConfig::new("/absolute/path/to/local.mmdb");
```

Applications normally do not need this override. External files are loaded without memory mapping and are subject to `max_database_bytes`, like the bundled database. Errors do not include the file path or database contents.

An ignored integration test can validate a compatible local database:

```sh
SAFE_HTTP_TEST_MMDB=/absolute/path/to/local.mmdb \
cargo test --locked --no-default-features --features geolocation \
  --test geolocation external_local_mmdb_lookup_uses_the_bounded_cache \
  -- --ignored --exact
```

## Update the bundled database

DB-IP publishes Lite updates monthly. Updates are deliberate maintainer changes; the crate does not download or hot-update the database at runtime.

1. Download the intended release from DB-IP on a maintainer machine.
2. Verify the archive, license, source, and database format before adding it.
3. Record the release, file size, and locally calculated checksum in this guide and `THIRD_PARTY_NOTICES.md`.
4. Update the `include_bytes!` path and `BUNDLED_DATABASE_RELEASE` in `src/geolocation/mod.rs`.
5. Remove the previous bundled file and update the Git LFS pointer.
6. Review `DEFAULT_MAX_DATABASE_BYTES`; adjust it deliberately if the new database requires more space.
7. Run stable and MSRV validation, including the bundled database and HTTP integration tests.
8. Review the crate package contents and size, since the database ships with the crate source.

Applications must be recompiled and redeployed to receive a database update. Running binaries retain the release with which they were built.
