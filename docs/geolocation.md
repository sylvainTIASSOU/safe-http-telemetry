# External geolocation database

The optional `geolocation` feature reads an explicit local MMDB file. The crate contains no production database and performs no downloads during Cargo builds, application startup, or HTTP requests. Obtain the database during deployment. Tests use a small licensed synthetic fixture and require no download.

## Install a pinned DB-IP City Lite release

DB-IP provides monthly releases under CC BY 4.0. Select a release from the [official download page](https://db-ip.com/db/lite.php), review its attribution requirements, and record the SHA-256 of the approved compressed archive in your deployment manifest. Obtain that checksum from a trusted release preparation process; computing it from an untrusted download does not authenticate the file.

The repository provides a Python 3.8+ installer (Python is not a runtime dependency of the Rust crate):

```sh
python3 scripts/download-mmdb.py \
  --release "$DBIP_RELEASE" \
  --sha256 "$DBIP_ARCHIVE_SHA256" \
  --output ./data/dbip-city-lite.mmdb
```

Both environment values are required: `DBIP_RELEASE` is a fixed `YYYY-MM`, and `DBIP_ARCHIVE_SHA256` is the 64-character SHA-256 of that release's `.mmdb.gz` archive. The script downloads only from DB-IP over HTTPS, verifies the archive hash and gzip integrity, limits both compressed and decompressed sizes to 128 MiB, checks the MMDB metadata marker, and replaces the destination atomically. Failed checks leave the previous file intact. The Rust engine parses the MMDB metadata at startup; malformed records encountered later produce an absent lookup result. There is no implicit selection of the latest release.

Retain the attribution **IP Geolocation by [DB-IP](https://db-ip.com)** and the [CC BY 4.0 license](https://creativecommons.org/licenses/by/4.0/) with the deployment. Follow DB-IP's requirements for pages displaying or using the lookup results. Database licensing is separate from the crate's MIT license.

## Configure the application

```toml
[dependencies]
safe-http-telemetry = { version = "0.1.0-rc.1", features = ["axum", "geolocation"] }
```

```rust
use safe_http_telemetry::{
    geolocation::GeolocationConfig,
    tower_integration::MiddlewareConfig,
};

let config = MiddlewareConfig {
    geolocation: Some(GeolocationConfig::new("/var/lib/geoip/dbip-city-lite.mmdb")),
    ..MiddlewareConfig::default()
};
```

The path is required and cannot be empty. Enabling geolocation with a missing, unreadable, corrupt, or oversized database fails layer construction. `geolocation: None` disables lookups without requiring a file. There is no `GeolocationConfig::default()` or `bundled()` fallback.

Loading happens once at startup, with a 128 MiB default limit and no memory mapping. Cloned engines share the loaded bytes and bounded cache. Default output contains only country code and English country name. Trusted-proxy rules govern the client IP. Minimal capture performs no IP capture or lookup. IP and geographical fields never become Loki labels or structured metadata.

For the `axum_loki` example, enable `geolocation` and supply `SAFE_HTTP_MMDB_PATH`. When this variable is absent, the example disables geolocation; an explicitly empty or invalid path fails startup.

```sh
SAFE_HTTP_MMDB_PATH=./data/dbip-city-lite.mmdb \
  cargo run --locked --example axum_loki \
  --features axum,loki,tracing,opentelemetry,body-capture,geolocation
```

## Docker deployment

Mount the directory containing the file in read-only mode; mounting the directory allows atomic file replacement to be visible after a process restart:

```yaml
services:
  app:
    volumes:
      - ./data:/var/lib/geoip:ro
    environment:
      SAFE_HTTP_MMDB_PATH: /var/lib/geoip/dbip-city-lite.mmdb
```

`SAFE_HTTP_MMDB_PATH` is interpreted by the example application; other applications must read their own configuration and pass the path to `GeolocationConfig::new`.

## Updates and rollback

Approve a new release and checksum, install it using the same script, then restart the application to load it. Keep the previous approved file separately for rollback. An engine that is already running retains its in-memory database and cache until it is replaced; no recompilation is needed. There is no hot reload or scheduled downloader inside the crate.

Production files under `data/` are ignored by Git and excluded from the Cargo package. The small test fixture under `tests/fixtures/` is included with its own license. Removing the old file from the current Git tree does not rewrite existing Git history or LFS storage.

## Validation

```sh
python3 -m unittest discover -s scripts -p 'test_*.py'
cargo test --locked --all-features --test geolocation --test geolocation_http
SAFE_HTTP_TEST_MMDB=./data/dbip-city-lite.mmdb \
  cargo test --locked --all-features --test geolocation \
  external_local_mmdb_lookup_uses_the_bounded_cache -- --ignored --exact
```

The last command is optional and checks a real externally supplied database using `8.8.8.8`. The normal suite uses synthetic data at `81.2.69.160` and runs fully offline.
