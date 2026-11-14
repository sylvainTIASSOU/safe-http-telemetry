# MMDB test fixture

`GeoIP2-City-Test.mmdb` is synthetic test data from MaxMind, not a production geolocation database.

Source: https://github.com/maxmind/MaxMind-DB/blob/daf41996ce86d6822e213b342d67c8efb6620e07/test-data/GeoIP2-City-Test.mmdb

Redistributed under the MIT license in `LICENSE-MaxMind`. Copyright MaxMind, Inc.

SHA-256: `1394af009121820d43b620fea19fc4b6f09141698091c00f70f225a982389035`.

Tests use `81.2.69.160` (GB). This fixture is intentionally included in the Cargo package so tests run offline. Production MMDB files in `data/` are excluded.
