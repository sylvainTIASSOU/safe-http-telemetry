# Third-Party Notices

## Crate and dependency licenses

The project source code is licensed under the MIT License; see [`LICENSE-MIT`](LICENSE-MIT). The complete versioned SPDX license inventory for every registry dependency in `Cargo.lock` (including optional, development, build, and target-specific dependencies) is in [`THIRD_PARTY_LICENSES.md`](THIRD_PARTY_LICENSES.md).

The inventory records the license expressions declared by the locked package versions. When redistributing dependency source or binaries, preserve any copyright and license notices required by those dependencies. Update and review the inventory whenever `Cargo.lock` changes.

## DB-IP Lite

DB-IP City Lite is an optional external data source. No DB-IP database is shipped in the Cargo package. Applications obtain it separately under Creative Commons Attribution 4.0 International, retain its license and attribution, and comply with the provider's terms.

**IP Geolocation by [DB-IP](https://db-ip.com)**

Terms: https://db-ip.com/db/lite.php

## MaxMind test fixture

`tests/fixtures/GeoIP2-City-Test.mmdb` contains synthetic test data from MaxMind, Inc., redistributed under the MIT license in `tests/fixtures/LICENSE-MaxMind`. Provenance and SHA-256 are recorded in `tests/fixtures/README.md`. It is not suitable for production lookups.

## Other MMDB databases

Other MMDB providers may use different schemas and licenses. Their terms must be reviewed separately.
