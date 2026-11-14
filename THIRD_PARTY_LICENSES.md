# Third-Party Dependency License Inventory

This inventory covers every registry package in the committed `Cargo.lock`, including transitive, optional, target-specific, build, and development dependencies. It is generated for the `0.1.0` source tree; update it whenever `Cargo.lock` changes.

**Scope:** 242 locked registry package/version entries across 21 SPDX expressions. **Cargo.lock SHA-256:** `47db3528dcbe34d1f6915aad038c7fb6de4139aaac3a1585ed9e03f3bbbb0a1f`.

SPDX expressions are normalized from each locked package version’s Cargo manifest and upstream license notices; 226 exact-version manifests were available locally for direct comparison, and the remaining 16 entries were checked against upstream package sources. Legacy slash separators are written as SPDX `OR`. `OR` means the recipient may choose a listed license; `AND` means all listed licenses apply. This is an inventory of declared package licenses, not legal advice or a replacement for preserving upstream copyright and license texts when redistributing dependency source/binaries.

The 16 source archives not cached locally are `valuable 0.1.1`, `wasm-bindgen-futures 0.4.80`, `web-time 1.1.0`, and 13 Windows support packages (`windows-link`, `windows-registry`, `windows-result`, `windows-strings`, `windows-targets`, and eight architecture-specific packages). Their license declarations were cross-checked with the [valuable 0.1.1 package source](https://docs.rs/crate/valuable/0.1.1/source/), the [wasm-bindgen upstream license](https://github.com/rustwasm/wasm-bindgen), the [web-time 1.1.0 manifest](https://docs.rs/crate/web-time/1.1.0/source/Cargo.toml), and the [windows-rs upstream project](https://github.com/microsoft/windows-rs).

## Packages by declared license

### `(MIT OR Apache-2.0) AND Unicode-3.0`

| Package | Version |
|---|---:|
| `unicode-ident` | `1.0.27` |

### `0BSD OR MIT OR Apache-2.0`

| Package | Version |
|---|---:|
| `adler2` | `2.0.1` |

### `Apache-2.0`

| Package | Version |
|---|---:|
| `ciborium` | `0.2.2` |
| `ciborium-io` | `0.2.2` |
| `ciborium-ll` | `0.2.2` |
| `opentelemetry` | `0.33.1` |
| `opentelemetry_sdk` | `0.33.0` |
| `sync_wrapper` | `1.0.2` |

### `Apache-2.0 AND ISC`

| Package | Version |
|---|---:|
| `ring` | `0.17.14` |

### `Apache-2.0 OR BSL-1.0`

| Package | Version |
|---|---:|
| `ryu` | `1.0.23` |

### `Apache-2.0 OR ISC OR MIT`

| Package | Version |
|---|---:|
| `hyper-rustls` | `0.27.10` |
| `rustls` | `0.23.46` |
| `rustls-pemfile` | `2.2.0` |

### `Apache-2.0 OR MIT`

| Package | Version |
|---|---:|
| `addr2line` | `0.27.1` |
| `async-channel` | `2.3.1` |
| `atomic-waker` | `1.1.2` |
| `autocfg` | `1.5.1` |
| `bit-set` | `0.8.0` |
| `bit-vec` | `0.8.0` |
| `concurrent-queue` | `2.5.0` |
| `criterion` | `0.5.1` |
| `event-listener` | `5.4.2` |
| `event-listener-strategy` | `0.5.4` |
| `fastrand` | `2.5.0` |
| `fnv` | `1.0.7` |
| `idna_adapter` | `1.2.1` |
| `object` | `0.40.0` |
| `parking` | `2.2.1` |
| `pin-project-lite` | `0.2.17` |
| `portable-atomic` | `1.15.0` |
| `rustc-hash` | `2.1.3` |
| `tinytemplate` | `1.2.1` |
| `utf8_iter` | `1.1.0` |
| `utf_types` | `1.0.0` |
| `zeroize` | `1.9.1` |

### `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT`

| Package | Version |
|---|---:|
| `linux-raw-sys` | `0.12.1` |
| `rustix` | `1.1.6` |
| `wasi` | `0.11.1+wasi-snapshot-preview1` |
| `wasip2` | `1.0.4+wasi-0.2.12` |
| `wit-bindgen` | `0.57.1` |

### `BSD-2-Clause OR Apache-2.0 OR MIT`

| Package | Version |
|---|---:|
| `zerocopy` | `0.8.63` |
| `zerocopy-derive` | `0.8.63` |

### `BSD-3-Clause`

| Package | Version |
|---|---:|
| `subtle` | `2.6.1` |

### `CDLA-Permissive-2.0`

| Package | Version |
|---|---:|
| `webpki-roots` | `0.26.11` |
| `webpki-roots` | `1.0.11` |

### `ISC`

| Package | Version |
|---|---:|
| `maxminddb` | `0.32.0` |
| `rustls-webpki` | `0.103.16` |
| `untrusted` | `0.9.0` |

### `MIT`

| Package | Version |
|---|---:|
| `axum` | `0.8.9` |
| `axum-core` | `0.5.6` |
| `bytes` | `1.12.1` |
| `cfg_aliases` | `0.2.2` |
| `crunchy` | `0.2.4` |
| `http-body` | `1.1.0` |
| `http-body-util` | `0.1.5` |
| `hyper` | `1.12.0` |
| `hyper-util` | `0.1.21` |
| `is-terminal` | `0.4.17` |
| `mio` | `1.2.4` |
| `oorandom` | `11.1.5` |
| `plotters` | `0.3.7` |
| `plotters-backend` | `0.3.7` |
| `plotters-svg` | `0.3.7` |
| `sharded-slab` | `0.1.7` |
| `slab` | `0.4.12` |
| `synstructure` | `0.14.0` |
| `tokio` | `1.44.2` |
| `tokio-macros` | `2.5.0` |
| `tower` | `0.5.2` |
| `tower-layer` | `0.3.3` |
| `tower-service` | `0.3.3` |
| `tracing` | `0.1.44` |
| `tracing-attributes` | `0.1.31` |
| `tracing-core` | `0.1.36` |
| `tracing-opentelemetry` | `0.34.0` |
| `tracing-subscriber` | `0.3.22` |
| `try-lock` | `0.2.5` |
| `valuable` | `0.1.1` |
| `want` | `0.3.2` |
| `zmij` | `1.0.23` |

### `MIT AND BSD-3-Clause`

| Package | Version |
|---|---:|
| `matchit` | `0.8.4` |

### `MIT OR Apache-2.0`

| Package | Version |
|---|---:|
| `anes` | `0.1.6` |
| `anstyle` | `1.0.14` |
| `backtrace` | `0.3.77` |
| `base64` | `0.22.1` |
| `bitflags` | `2.13.2` |
| `bumpalo` | `3.20.3` |
| `cast` | `0.3.0` |
| `cc` | `1.11.1` |
| `cfg-if` | `1.0.5` |
| `chacha20` | `0.10.2` |
| `clap` | `4.6.7` |
| `clap_builder` | `4.6.7` |
| `clap_lex` | `1.1.1` |
| `cpufeatures` | `0.3.1` |
| `criterion-plot` | `0.5.0` |
| `crossbeam-deque` | `0.8.8` |
| `crossbeam-epoch` | `0.9.21` |
| `crossbeam-utils` | `0.8.23` |
| `displaydoc` | `0.2.7` |
| `either` | `1.19.0` |
| `errno` | `0.3.14` |
| `find-msvc-tools` | `0.1.15` |
| `form_urlencoded` | `1.2.2` |
| `futures` | `0.3.34` |
| `futures-channel` | `0.3.34` |
| `futures-core` | `0.3.34` |
| `futures-executor` | `0.3.34` |
| `futures-io` | `0.3.34` |
| `futures-macro` | `0.3.34` |
| `futures-sink` | `0.3.34` |
| `futures-task` | `0.3.34` |
| `futures-util` | `0.3.34` |
| `getrandom` | `0.2.17` |
| `getrandom` | `0.3.4` |
| `getrandom` | `0.4.3` |
| `gimli` | `0.34.0` |
| `half` | `2.7.1` |
| `hermit-abi` | `0.5.3` |
| `http` | `1.5.0` |
| `httparse` | `1.10.1` |
| `httpdate` | `1.0.3` |
| `idna` | `1.1.0` |
| `ipnet` | `2.12.2` |
| `ipnetwork` | `0.21.1` |
| `itertools` | `0.10.5` |
| `itoa` | `1.0.18` |
| `js-sys` | `0.3.107` |
| `lazy_static` | `1.5.1` |
| `libc` | `0.2.191` |
| `log` | `0.4.35` |
| `mime` | `0.3.18` |
| `num-traits` | `0.2.19` |
| `once_cell` | `1.21.4` |
| `percent-encoding` | `2.3.2` |
| `ppv-lite86` | `0.2.21` |
| `proc-macro2` | `1.0.107` |
| `proptest` | `1.10.0` |
| `quick-error` | `1.2.3` |
| `quinn` | `0.11.12` |
| `quinn-proto` | `0.11.19` |
| `quinn-udp` | `0.5.16` |
| `quote` | `1.0.47` |
| `rand` | `0.10.3` |
| `rand` | `0.9.5` |
| `rand_chacha` | `0.9.0` |
| `rand_core` | `0.10.1` |
| `rand_core` | `0.9.5` |
| `rand_pcg` | `0.10.2` |
| `rand_xorshift` | `0.4.0` |
| `rayon` | `1.12.0` |
| `rayon-core` | `1.13.0` |
| `regex` | `1.13.1` |
| `regex-automata` | `0.4.18` |
| `regex-syntax` | `0.8.11` |
| `reqwest` | `0.12.15` |
| `rustc-demangle` | `0.1.28` |
| `rustls-pki-types` | `1.15.1` |
| `rustversion` | `1.0.23` |
| `rusty-fork` | `0.3.1` |
| `serde` | `1.0.229` |
| `serde_core` | `1.0.229` |
| `serde_derive` | `1.0.229` |
| `serde_json` | `1.0.154` |
| `serde_path_to_error` | `0.1.20` |
| `serde_urlencoded` | `0.7.1` |
| `shlex` | `2.0.1` |
| `signal-hook-registry` | `1.4.8` |
| `smallvec` | `1.16.5` |
| `socket2` | `0.5.10` |
| `socket2` | `0.6.5` |
| `stable_deref_trait` | `1.2.1` |
| `syn` | `2.0.119` |
| `syn` | `3.0.9` |
| `tempfile` | `3.27.0` |
| `thiserror` | `2.0.21` |
| `thiserror-impl` | `2.0.21` |
| `thread_local` | `1.1.10` |
| `tokio-rustls` | `0.26.6` |
| `unarray` | `0.1.4` |
| `url` | `2.5.8` |
| `wait-timeout` | `0.2.1` |
| `wasm-bindgen` | `0.2.130` |
| `wasm-bindgen-futures` | `0.4.80` |
| `wasm-bindgen-macro` | `0.2.130` |
| `wasm-bindgen-macro-support` | `0.2.130` |
| `wasm-bindgen-shared` | `0.2.130` |
| `web-sys` | `0.3.107` |
| `web-time` | `1.1.0` |
| `windows-link` | `0.1.3` |
| `windows-link` | `0.2.1` |
| `windows-registry` | `0.4.0` |
| `windows-result` | `0.3.4` |
| `windows-strings` | `0.3.1` |
| `windows-sys` | `0.52.0` |
| `windows-sys` | `0.61.2` |
| `windows-targets` | `0.52.6` |
| `windows-targets` | `0.53.5` |
| `windows_aarch64_gnullvm` | `0.52.6` |
| `windows_aarch64_gnullvm` | `0.53.1` |
| `windows_aarch64_msvc` | `0.52.6` |
| `windows_aarch64_msvc` | `0.53.1` |
| `windows_i686_gnu` | `0.52.6` |
| `windows_i686_gnu` | `0.53.1` |
| `windows_i686_gnullvm` | `0.52.6` |
| `windows_i686_gnullvm` | `0.53.1` |
| `windows_i686_msvc` | `0.52.6` |
| `windows_i686_msvc` | `0.53.1` |
| `windows_x86_64_gnu` | `0.52.6` |
| `windows_x86_64_gnu` | `0.53.1` |
| `windows_x86_64_gnullvm` | `0.52.6` |
| `windows_x86_64_gnullvm` | `0.53.1` |
| `windows_x86_64_msvc` | `0.52.6` |
| `windows_x86_64_msvc` | `0.53.1` |

### `MIT OR Apache-2.0 OR LGPL-2.1-or-later`

| Package | Version |
|---|---:|
| `r-efi` | `5.3.0` |
| `r-efi` | `6.0.0` |

### `MIT OR Apache-2.0 OR Zlib`

| Package | Version |
|---|---:|
| `lru-slab` | `0.1.3` |

### `MIT OR Zlib OR Apache-2.0`

| Package | Version |
|---|---:|
| `miniz_oxide` | `0.9.1` |

### `Unicode-3.0`

| Package | Version |
|---|---:|
| `icu_collections` | `2.0.0` |
| `icu_locale_core` | `2.0.0` |
| `icu_normalizer` | `2.0.0` |
| `icu_normalizer_data` | `2.0.0` |
| `icu_properties` | `2.0.2` |
| `icu_properties_data` | `2.0.1` |
| `icu_provider` | `2.0.0` |
| `litemap` | `0.8.3` |
| `potential_utf` | `0.1.6` |
| `tinystr` | `0.8.5` |
| `writeable` | `0.6.4` |
| `yoke` | `0.8.4` |
| `yoke-derive` | `0.8.4` |
| `zerofrom` | `0.1.8` |
| `zerofrom-derive` | `0.1.8` |
| `zerotrie` | `0.2.5` |
| `zerovec` | `0.11.9` |
| `zerovec-derive` | `0.11.7` |

### `Unlicense OR MIT`

| Package | Version |
|---|---:|
| `aho-corasick` | `1.1.5` |
| `memchr` | `2.8.3` |
| `same-file` | `1.0.6` |
| `walkdir` | `2.5.0` |
| `winapi-util` | `0.1.11` |

### `Zlib OR Apache-2.0 OR MIT`

| Package | Version |
|---|---:|
| `tinyvec` | `1.13.3` |

## Project and data licenses

- This crate is licensed under MIT; see [`LICENSE-MIT`](LICENSE-MIT).
- The synthetic MaxMind test fixture is distributed under MIT; see [`tests/fixtures/LICENSE-MaxMind`](tests/fixtures/LICENSE-MaxMind) and its [provenance record](tests/fixtures/README.md).
- DB-IP City Lite is an external data source and is not included in the crate package. Its CC BY 4.0 terms and attribution are described in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
- Other MMDB files supplied by users remain subject to their provider’s license.
