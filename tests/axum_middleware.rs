#![cfg(feature = "axum")]

use std::{convert::Infallible, time::Duration};

use axum::{
    body::Body,
    http::{Request, Response, StatusCode},
    routing::get,
    Router,
};
use bytes::Bytes;
use http_body_util::BodyExt;
use safe_http_telemetry::{
    axum_adapter::{axum_telemetry_layer, AxumTelemetryLayer},
    core::{event::ResponseOutcome, telemetry::InternalTelemetry},
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::{MiddlewareConfig, UNMATCHED_ROUTE},
};
use tower::{timeout::TimeoutLayer, Layer, ServiceExt};

fn queue() -> BoundedEventQueue {
    BoundedEventQueue::new(QueueConfig::new(16), InternalTelemetry::default()).unwrap()
}

fn layer(queue: BoundedEventQueue, policies: RoutePolicySet) -> AxumTelemetryLayer {
    axum_telemetry_layer(queue, policies, MiddlewareConfig::default()).unwrap()
}

#[tokio::test]
async fn axum_matched_path_is_used_instead_of_a_dynamic_raw_route() {
    let queue = queue();
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let policies = RoutePolicySet::new(CaptureProfile::Minimal);

    let app = Router::new()
        .route("/users/{id}", get(|| async { StatusCode::OK }))
        .layer(layer(queue, policies));

    let request = Request::builder()
        .uri("/users/123?token=fictional-test-token")
        .body(Body::empty())
        .unwrap();

    let response = app.oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let event = receiver.try_recv().expect("Axum completion event");
    assert_eq!(event.request.route, "/users/{id}");
    assert_eq!(event.request.path, "/users/123");
    assert_eq!(event.response.status_code, Some(200));
    assert!(event.request.query.is_empty());

    let serialized = serde_json::to_string(&event).unwrap();
    assert!(!serialized.contains("fictional-test-token"));

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.observed_requests, 1);
    assert_eq!(snapshot.completed_requests, 1);
    assert_eq!(snapshot.active_requests, 0);
}

#[tokio::test]
async fn excluded_axum_route_produces_no_event_and_preserves_its_body() {
    let queue = queue();
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);
    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/health"), CaptureProfile::Minimal).exclude(),
        )
        .unwrap();

    let app = Router::new()
        .route("/health", get(|| async { "healthy" }))
        .layer(layer(queue, policies));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, Bytes::from_static(b"healthy"));
    assert!(receiver.try_recv().is_none());

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.excluded_requests, 1);
    assert_eq!(snapshot.observed_requests, 0);
    assert_eq!(snapshot.active_requests, 0);
}

#[tokio::test]
async fn unmatched_axum_request_uses_a_stable_route_value() {
    let queue = queue();
    let receiver = queue.receiver();
    let policies = RoutePolicySet::new(CaptureProfile::Minimal);

    let app = Router::new()
        .route("/known", get(|| async { StatusCode::OK }))
        .layer(layer(queue, policies));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/missing/123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let event = receiver.try_recv().expect("unmatched-route event");
    assert_eq!(event.request.route, UNMATCHED_ROUTE);
    assert_eq!(event.request.path, "/missing/123");
    assert_eq!(event.response.status_code, Some(404));
    assert_eq!(event.response.outcome, Some(ResponseOutcome::ClientError));
}

#[tokio::test]
async fn axum_stream_is_unchanged_and_finalizes_after_the_last_fragment() {
    let queue = queue();
    let receiver = queue.receiver();
    let policies = RoutePolicySet::new(CaptureProfile::Minimal);

    let app = Router::new()
        .route(
            "/stream",
            get(|| async {
                let stream = futures_util::stream::iter([
                    Ok::<Bytes, Infallible>(Bytes::from_static(b"first-")),
                    Ok::<Bytes, Infallible>(Bytes::from_static(b"second")),
                ]);

                Response::builder()
                    .header("x-stream", "unchanged")
                    .body(Body::from_stream(stream))
                    .unwrap()
            }),
        )
        .layer(layer(queue, policies));

    let response = app
        .oneshot(
            Request::builder()
                .uri("/stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.headers()["x-stream"], "unchanged");
    assert!(receiver.try_recv().is_none());

    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, Bytes::from_static(b"first-second"));

    let event = receiver.try_recv().expect("stream completion event");
    assert_eq!(event.request.route, "/stream");
    assert_eq!(event.response.status_code, Some(200));
    assert!(event.timing.time_to_first_byte_ms.is_some());
    assert!(event.timing.body_duration_ms.is_some());
    assert!(receiver.try_recv().is_none());
}

#[tokio::test]
async fn tower_elapsed_error_is_classified_as_a_timeout() {
    let queue = queue();
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let telemetry_layer = layer(queue, RoutePolicySet::new(CaptureProfile::Minimal));

    let router = Router::new().route(
        "/slow",
        get(|| async {
            tokio::time::sleep(Duration::from_millis(50)).await;
            StatusCode::OK
        }),
    );

    // Telemetry wraps the timeout service, so it receives Tower's BoxError.
    let timeout = TimeoutLayer::new(Duration::from_millis(5)).layer(router);
    let service = telemetry_layer.layer(timeout);

    let result = service
        .oneshot(Request::builder().uri("/slow").body(Body::empty()).unwrap())
        .await;
    assert!(result.is_err());

    let event = receiver.try_recv().expect("timeout event");
    assert_eq!(event.response.status_code, None);
    assert_eq!(event.response.outcome, Some(ResponseOutcome::Timeout));
    assert_eq!(telemetry.snapshot().timed_out_requests, 1);
    assert_eq!(telemetry.snapshot().active_requests, 0);
}
