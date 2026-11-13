# Bundled geolocation database

The `geolocation` feature embeds DB-IP City Lite in the crate. Lookups are local:
they perform no network request and use no memory mapping. Enabling the feature
increases the resulting binary size by the database size.

The bundled release is **2026-11**, stored at
`data/dbip-city-lite-2026-11.mmdb`. Its uncompressed size is 126,873,350 bytes
and its SHA-1 is `a6f14e8da5565c713cdda9f1959a904b2eb88337`.

DB-IP Lite is licensed under Creative Commons Attribution 4.0, separately from
the project's MIT-licensed source code. Keep this attribution in distributed
applications and documentation:

**IP Geolocation by [DB-IP](https://db-ip.com)**

See &lt;https://db-ip.com/db/lite.php&gt; for the database license and download terms.

## Application activation

Enable the Cargo features and explicitly opt in at middleware construction:

```toml
safe-http-telemetry = { path = "../safe-http-telemetry", features = ["axum", "geolocation"] }


use safe_http_telemetry::{
    geolocation::GeolocationConfig,
    tower_integration::MiddlewareConfig,
};

let config = MiddlewareConfig {
    geolocation: Some(GeolocationConfig::bundled()),
    ..MiddlewareConfig::default()
};
GeolocationConfig::default() is equivalent to bundled(). The middleware
default remains geolocation: None, so applications do not capture an IP or
perform geolocation unless they opt in.
The database is opened and validated once when the layer is built. The default
configuration retains only the country code and English country name. Client-IP
resolution follows the trusted-proxy rules in ClientIpConfig. The Minimal
profile performs neither client-IP capture nor geolocation lookup.
IP addresses and geographic values stay in the redacted JSON event. They never
become Loki labels or Loki structured metadata.
Optional external database
An external local MMDB remains available for tests and advanced use cases:
rust
let config = GeolocationConfig::new("/absolute/path/to/local.mmdb");
The application normally does not need this override. External files are loaded
without memory mapping and are subject to max_database_bytes, just like the
bundled database. Errors never include the file path or database contents.
An ignored integration test can validate a compatible external database:
sh
SAFE_HTTP_TEST_MMDB=/absolute/path/to/local.mmdb \
cargo test --locked --no-default-features --features geolocation \
  --test geolocation external_local_mmdb_lookup_uses_the_bounded_cache \
  -- --ignored --exact
Maintainer update procedure
DB-IP publishes Lite updates monthly. Updates are deliberate maintainer changes;
there is no runtime download or hot update.
Download the new release on a maintainer machine:
sh
release=YYYY-MM
archive="/tmp/dbip-city-lite-${release}.mmdb.gz"
curl --fail --location \
  "https://download.db-ip.com/free/dbip-city-lite-${release}.mmdb.gz" \
  --output "$archive"
gzip --test "$archive"
gzip -dc "$archive" > "data/dbip-city-lite-${release}.mmdb"
sha1sum "data/dbip-city-lite-${release}.mmdb"
stat -c '%s bytes' "data/dbip-city-lite-${release}.mmdb"
Compare the checksum with the value published by DB-IP. Do not commit a
database whose origin, integrity, license or schema has not been checked.
In src/geolocation/mod.rs, update the include_bytes! filename and
BUNDLED_DATABASE_RELEASE. Remove the previous bundled file.
Update the release, size and checksum in this guide and in
THIRD_PARTY_NOTICES.md.
If the database exceeds DEFAULT_MAX_DATABASE_BYTES, review the new size and
deliberately adjust the bound rather than disabling the check.
Run the complete stable and MSRV validation suites, including the automated
bundled-database and HTTP integration tests.
Commit the MMDB, source, notices and lockfile changes together when the
lockfile changed.
A database update changes the crate contents. Every application must therefore
be recompiled and redeployed to receive it; already running binaries retain the
release with which they were built.
