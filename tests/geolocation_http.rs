#![cfg(all(feature = "axum", feature = "geolocation"))]

mod common;

use std::{
    net::SocketAddr,
    sync::atomic::{AtomicU64, Ordering},
};

use axum::{
    body::Body,
    extract::ConnectInfo,
    http::{Request, StatusCode},
    routing::get,
    Router,
};
use common::expect_http;
use safe_http_telemetry::{
    axum_adapter::axum_telemetry_layer,
    config::ConfigError,
    core::telemetry::InternalTelemetry,
    geolocation::{GeolocationConfig, GeolocationError},
    policy::{CaptureProfile, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::{HttpTelemetryLayer, MiddlewareConfig},
};
use tower::ServiceExt;

static NEXT_PATH_ID: AtomicU64 = AtomicU64::new(0);

fn queue() -> BoundedEventQueue {
    BoundedEventQueue::new(QueueConfig::new(8), InternalTelemetry::default()).unwrap()
}

fn bundled_config() -> MiddlewareConfig {
    MiddlewareConfig {
        geolocation: Some(GeolocationConfig::bundled()),
        ..MiddlewareConfig::default()
    }
}

fn external_config(database_path: impl Into<std::path::PathBuf>) -> MiddlewareConfig {
    MiddlewareConfig {
        geolocation: Some(GeolocationConfig::new(database_path)),
        ..MiddlewareConfig::default()
    }
}

fn request(address: &'static str) -> Request<Body> {
    Request::builder()
        .uri("/orders/42")
        .extension(ConnectInfo("10.0.0.4:1234".parse::<SocketAddr>().unwrap()))
        .header("x-forwarded-for", address)
        .body(Body::empty())
        .unwrap()
}

#[test]
fn layer_startup_maps_database_errors_without_exposing_the_path() {
    let id = NEXT_PATH_ID.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "missing-safe-http-geolocation-{}-{id}.mmdb",
        std::process::id()
    ));
    let result = HttpTelemetryLayer::new(
        queue(),
        RoutePolicySet::new(CaptureProfile::Standard),
        external_config(&path),
    );

    let error = match result {
        Err(error) => error,
        Ok(_) => panic!("missing MMDB was accepted"),
    };
    assert!(matches!(
        error,
        ConfigError::Geolocation(GeolocationError::DatabaseOpen)
    ));
    let diagnostics = format!("{error} {error:?}");
    assert!(!diagnostics.contains(path.to_str().unwrap()));
}

#[tokio::test]
async fn bundled_mmdb_enriches_standard_http_events_and_shares_the_cache() {
    let queue = queue();
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let mut config = bundled_config();
    config.client_ip.trusted_proxies = vec!["10.0.0.0/8".parse().unwrap()];
    let app = Router::new()
        .route("/orders/{id}", get(|| async { StatusCode::OK }))
        .layer(
            axum_telemetry_layer(queue, RoutePolicySet::new(CaptureProfile::Standard), config)
                .unwrap(),
        );

    app.clone().oneshot(request("8.8.8.8")).await.unwrap();
    app.oneshot(request("8.8.8.8")).await.unwrap();

    let first = expect_http(receiver.try_recv().expect("first geolocated event"));
    let second = expect_http(receiver.try_recv().expect("cached geolocated event"));
    assert_eq!(first.request.client_ip.as_deref(), Some("8.8.8.8"));
    assert!(first.request.geolocation.is_some());
    assert!(first.request.geolocation == second.request.geolocation);

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.geolocation_cache_misses, 1);
    assert_eq!(snapshot.geolocation_cache_hits, 1);
    assert_eq!(snapshot.geolocation_errors, 0);
}

#[tokio::test]
async fn minimal_profile_never_queries_the_bundled_mmdb() {
    let queue = queue();
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let app = Router::new()
        .route("/orders/{id}", get(|| async { StatusCode::OK }))
        .layer(
            axum_telemetry_layer(
                queue,
                RoutePolicySet::new(CaptureProfile::Minimal),
                bundled_config(),
            )
            .unwrap(),
        );

    app.oneshot(request("8.8.8.8")).await.unwrap();

    let event = expect_http(receiver.try_recv().expect("minimal HTTP event"));
    assert!(event.request.client_ip.is_none());
    assert!(event.request.geolocation.is_none());

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.geolocation_cache_misses, 0);
    assert_eq!(snapshot.geolocation_cache_hits, 0);
    assert_eq!(snapshot.geolocation_errors, 0);
}
