use axum::{body::Body, http::Request, routing::post, Router};
use criterion::{criterion_group, criterion_main, Criterion};
use std::time::Duration;
use tokio::runtime::Runtime;
use tower::ServiceExt;

use safe_http_telemetry::{
    axum_adapter::axum_telemetry_layer,
    core::telemetry::InternalTelemetry,
    geolocation::GeolocationConfig,
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::config::MiddlewareConfig,
};

// 1. Axum app with a POST endpoint to test bodies
fn base_app() -> Router {
    Router::new().route("/api/items", post(|| async { "Created" }))
}

// 2. Simple request (no body)
fn mock_minimal_request() -> Request<Body> {
    Request::builder()
        .uri("/api/items")
        .body(Body::empty())
        .unwrap()
}

// 3. Rich request (with body, query, and headers)
fn mock_detailed_request() -> Request<Body> {
    let body_json = r#"{"name":"John","email":"john@example.com","password":"secret"}"#;
    Request::builder()
        .method("POST")
        .uri("/api/items?page=1&limit=10")
        .header("content-type", "application/json")
        .header("user-agent", "CriterionBench/1.0")
        .header("authorization", "Bearer secret-token")
        .body(Body::from(body_json))
        .unwrap()
}

// Helper to create an app with a given profile
fn create_app_with_profile(profile: CaptureProfile) -> Router {
    let queue = BoundedEventQueue::new(QueueConfig::new(1024), InternalTelemetry::new()).unwrap();

    let mut policies = RoutePolicySet::new(profile);
    policies
        .add_policy(RoutePolicy::new(RoutePattern::exact("/api/items"), profile))
        .unwrap();

    let layer = axum_telemetry_layer(queue, policies, MiddlewareConfig::default()).unwrap();
    base_app().layer(layer)
}

// Specific helper for geolocation (uses bundled database)
fn create_app_with_geolocation() -> Router {
    let queue = BoundedEventQueue::new(QueueConfig::new(1024), InternalTelemetry::new()).unwrap();

    let mut policies = RoutePolicySet::new(CaptureProfile::Standard);
    policies
        .add_policy(RoutePolicy::new(
            RoutePattern::exact("/api/items"),
            CaptureProfile::Standard,
        ))
        .unwrap();

    let middleware_config = MiddlewareConfig {
        geolocation: Some(GeolocationConfig::bundled()),
        ..Default::default()
    };

    let layer = axum_telemetry_layer(queue, policies, middleware_config).unwrap();
    base_app().layer(layer)
}

fn benchmark_middleware(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("HTTP_Middleware_Overhead");

    group.measurement_time(Duration::from_secs(3));

    // --- Benchmark 1: Baseline (No Middleware) ---
    group.bench_function("Baseline (No Middleware)", |b| {
        let app = base_app();
        b.to_async(&rt).iter(|| {
            let app = app.clone();
            let req = mock_minimal_request();
            async move {
                let _ = app.oneshot(req).await.unwrap();
            }
        });
    });

    // --- Benchmark 2: Minimal Profile ---
    group.bench_function("Minimal Profile", |b| {
        let app = create_app_with_profile(CaptureProfile::Minimal);
        b.to_async(&rt).iter(|| {
            let app = app.clone();
            let req = mock_minimal_request();
            async move {
                let _ = app.oneshot(req).await.unwrap();
            }
        });
    });

    // --- Benchmark 3: Standard Profile (Query + Headers) ---
    group.bench_function("Standard Profile (Metadata)", |b| {
        let app = create_app_with_profile(CaptureProfile::Standard);
        b.to_async(&rt).iter(|| {
            let app = app.clone();
            // On utilise la requête détaillée mais le profil Standard ignorera le body
            let req = mock_detailed_request();
            async move {
                let _ = app.oneshot(req).await.unwrap();
            }
        });
    });

    // --- Benchmark 4: Detailed Profile (Body + Redaction) ---
    group.bench_function("Detailed Profile (Bodies & Redaction)", |b| {
        let app = create_app_with_profile(CaptureProfile::Detailed);
        b.to_async(&rt).iter(|| {
            let app = app.clone();
            let req = mock_detailed_request();
            async move {
                let _ = app.oneshot(req).await.unwrap();
            }
        });
    });

    // --- Benchmark 5: Standard Profile + Geolocation ---
    group.bench_function("Standard + Geolocation (Cache Miss/Hit)", |b| {
        let app = create_app_with_geolocation();
        b.to_async(&rt).iter(|| {
            let app = app.clone();
            let req = mock_minimal_request();
            async move {
                let _ = app.oneshot(req).await.unwrap();
            }
        });
    });

    group.finish();
}

criterion_group!(benches, benchmark_middleware);
criterion_main!(benches);
