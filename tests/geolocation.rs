#![cfg(feature = "geolocation")]

use std::{
    net::IpAddr,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::Duration,
};

use safe_http_telemetry::{
    core::telemetry::InternalTelemetry,
    geolocation::{
        GeolocationConfig, GeolocationConfigError, GeolocationEngine, GeolocationError,
        GeolocationFields,
    },
};

static NEXT_FILE_ID: AtomicU64 = AtomicU64::new(0);

fn temporary_database(contents: &[u8]) -> PathBuf {
    let id = NEXT_FILE_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "safe-http-telemetry-geolocation-{}-{id}.mmdb",
        std::process::id()
    ));
    std::fs::write(&path, contents).unwrap();
    path
}

#[test]
fn fixture_defaults_are_valid_and_retain_only_country_fields() {
    let config = GeolocationConfig::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/GeoIP2-City-Test.mmdb"
    ));

    assert!(config.database_path.is_file());
    assert_eq!(config.validate(), Ok(()));
    assert_eq!(config.cache_capacity, 10_000);
    assert_eq!(config.cache_ttl, Duration::from_secs(300));
    assert_eq!(config.max_database_bytes, 128 * 1024 * 1024);
    assert_eq!(config.fields, GeolocationFields::default());
    assert!(config.fields.country_code);
    assert!(config.fields.country);
    assert!(!config.fields.region);
    assert!(!config.fields.city);
    assert!(!config.fields.coordinates);
}

#[test]
fn unsafe_or_empty_configuration_is_rejected() {
    let config = GeolocationConfig::new("");
    assert_eq!(
        config.validate(),
        Err(GeolocationConfigError::EmptyDatabasePath)
    );

    let mut config = GeolocationConfig::new("local.mmdb");
    config.cache_capacity = 0;
    assert_eq!(
        config.validate(),
        Err(GeolocationConfigError::ZeroCacheCapacity)
    );

    config.cache_capacity = 1;
    config.cache_ttl = Duration::ZERO;
    assert_eq!(config.validate(), Err(GeolocationConfigError::ZeroCacheTtl));

    config.cache_ttl = Duration::from_secs(1);
    config.max_database_bytes = 0;
    assert_eq!(
        config.validate(),
        Err(GeolocationConfigError::ZeroDatabaseLimit)
    );

    config.max_database_bytes = 1;
    config.max_string_bytes = 0;
    assert_eq!(
        config.validate(),
        Err(GeolocationConfigError::ZeroStringLimit)
    );

    config.max_string_bytes = 1;
    config.fields = GeolocationFields {
        country_code: false,
        country: false,
        region: false,
        city: false,
        coordinates: false,
    };
    assert_eq!(
        config.validate(),
        Err(GeolocationConfigError::NoFieldsSelected)
    );
}

#[test]
fn fixture_database_loading_respects_the_size_limit() {
    let mut config = GeolocationConfig::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/GeoIP2-City-Test.mmdb"
    ));
    config.max_database_bytes = 8;
    let telemetry = InternalTelemetry::new();

    let error = match GeolocationEngine::open(&config, telemetry.clone()) {
        Err(error) => error,
        Ok(_) => panic!("oversized fixture database was accepted"),
    };
    assert_eq!(error, GeolocationError::DatabaseTooLarge { limit: 8 });
    assert_eq!(telemetry.snapshot().geolocation_errors, 1);
}

#[test]
fn database_loading_is_bounded_and_counted() {
    let path = temporary_database(b"0123456789abcdef");
    let mut config = GeolocationConfig::new(&path);
    config.max_database_bytes = 8;
    let telemetry = InternalTelemetry::new();

    let error = match GeolocationEngine::open(&config, telemetry.clone()) {
        Err(error) => error,
        Ok(_) => panic!("oversized test database was accepted"),
    };
    assert_eq!(error, GeolocationError::DatabaseTooLarge { limit: 8 });
    assert_eq!(telemetry.snapshot().geolocation_errors, 1);

    std::fs::remove_file(path).unwrap();
}

#[test]
fn invalid_database_diagnostics_do_not_expose_paths_or_contents() {
    let path = temporary_database(b"fictional-mmdb-content");
    let mut config = GeolocationConfig::new(&path);
    config.max_database_bytes = 64;
    let telemetry = InternalTelemetry::new();

    let error = match GeolocationEngine::open(&config, telemetry) {
        Err(error) => error,
        Ok(_) => panic!("invalid test database was accepted"),
    };
    assert_eq!(error, GeolocationError::InvalidDatabase);

    let diagnostics = format!("{error} {error:?}");
    assert!(!diagnostics.contains("fictional-mmdb-content"));
    assert!(!diagnostics.contains(path.to_str().unwrap()));

    std::fs::remove_file(path).unwrap();
}

#[test]
fn geolocation_counters_are_shared_by_cloned_telemetry_handles() {
    let telemetry = InternalTelemetry::new();
    let cloned = telemetry.clone();

    telemetry.record_geolocation_cache_hit();
    cloned.record_geolocation_cache_miss();
    cloned.record_geolocation_error();

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.geolocation_cache_hits, 1);
    assert_eq!(snapshot.geolocation_cache_misses, 1);
    assert_eq!(snapshot.geolocation_errors, 1);
}

#[test]
fn fixture_mmdb_lookup_uses_the_bounded_cache() {
    let config = GeolocationConfig::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/GeoIP2-City-Test.mmdb"
    ));
    let telemetry = InternalTelemetry::new();
    let engine = match GeolocationEngine::open(&config, telemetry.clone()) {
        Ok(engine) => engine,
        Err(error) => panic!("fixture MMDB startup failed: {error}"),
    };
    let address = IpAddr::from([81, 2, 69, 160]);

    let first = engine.lookup(address);
    let second = engine.lookup(address);

    let location = first.as_ref().expect("fixture address has a location");
    assert_eq!(location.country_code.as_deref(), Some("GB"));
    assert_eq!(location.country.as_deref(), Some("United Kingdom"));
    assert!(first == second);
    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.geolocation_cache_misses, 1);
    assert_eq!(snapshot.geolocation_cache_hits, 1);
    assert_eq!(snapshot.geolocation_errors, 0);
}

#[test]
fn loaded_database_remains_available_after_its_file_is_removed() {
    let fixture = include_bytes!("fixtures/GeoIP2-City-Test.mmdb");
    let path = temporary_database(fixture);
    let config = GeolocationConfig::new(&path);
    let engine = GeolocationEngine::open(&config, InternalTelemetry::new()).unwrap();

    std::fs::remove_file(&path).unwrap();

    let location = engine
        .lookup(IpAddr::from([81, 2, 69, 160]))
        .expect("the loaded snapshot remains usable");
    assert_eq!(location.country_code.as_deref(), Some("GB"));
    assert!(matches!(
        GeolocationEngine::open(&config, InternalTelemetry::new()),
        Err(GeolocationError::DatabaseOpen)
    ));
}

#[test]
#[ignore = "requires SAFE_HTTP_TEST_MMDB to point to a local GeoIP2/GeoLite2 database"]
fn external_local_mmdb_lookup_uses_the_bounded_cache() {
    let path = std::env::var_os("SAFE_HTTP_TEST_MMDB")
        .expect("SAFE_HTTP_TEST_MMDB must name a local MMDB file");
    let config = GeolocationConfig::new(path);
    let telemetry = InternalTelemetry::new();
    let engine = match GeolocationEngine::open(&config, telemetry.clone()) {
        Ok(engine) => engine,
        Err(error) => panic!("local MMDB startup failed: {error}"),
    };
    let address = IpAddr::from([8, 8, 8, 8]);

    let first = engine.lookup(address);
    let second = engine.lookup(address);

    assert!(first.is_some());
    assert!(first == second);
    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.geolocation_cache_misses, 1);
    assert_eq!(snapshot.geolocation_cache_hits, 1);
    assert_eq!(snapshot.geolocation_errors, 0);
}
