#![cfg(all(feature = "axum", feature = "body-capture"))]

mod common;

use std::{convert::Infallible, future};

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::post,
    Router,
};
use bytes::Bytes;
use common::expect_http;
use safe_http_telemetry::{
    axum_adapter::{axum_telemetry_layer, AxumTelemetryLayer},
    core::{
        event::{BodyCaptureStatus, HttpEvent},
        telemetry::InternalTelemetry,
    },
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::MiddlewareConfig,
};

#[cfg(feature = "loki")]
use safe_http_telemetry::{config::ServiceConfig, loki::payload::LokiStream};

use serde_json::json;
use tower::ServiceExt;

fn queue() -> BoundedEventQueue {
    BoundedEventQueue::new(QueueConfig::new(16), InternalTelemetry::default()).unwrap()
}

fn layer(
    queue: BoundedEventQueue,
    policies: RoutePolicySet,
    config: MiddlewareConfig,
) -> AxumTelemetryLayer {
    axum_telemetry_layer(queue, policies, config).unwrap()
}

fn assert_no_fictional_secret(event: &HttpEvent) {
    let serialized = serde_json::to_string(event).unwrap();
    assert!(!serialized.contains("fictional-request-secret"));
    assert!(!serialized.contains("fictional-password"));

    #[cfg(feature = "loki")]
    {
        let loki = LokiStream::from_event(event, &ServiceConfig::default(), 1).unwrap();
        let serialized = serde_json::to_string(&loki).unwrap();

        assert!(!serialized.contains("fictional-request-secret"));
        assert!(!serialized.contains("fictional-password"));
    }
}

#[tokio::test]
async fn fragmented_json_is_redacted_and_forwarded_without_change() {
    let queue = queue();
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let policies = RoutePolicySet::new(CaptureProfile::Detailed);

    let app = Router::new()
        .route(
            "/capture",
            post(|body: Bytes| async move {
                let expected = b"{\"display\":\"alice\",\"token\":\"fictional-request-secret\",\"nested\":{\"password\":\"fictional-password\"}}";

                if body.as_ref() == expected {
                    StatusCode::OK
                } else {
                    StatusCode::BAD_REQUEST
                }
            }),
        )
        .layer(layer(
            queue,
            policies,
            MiddlewareConfig::default(),
        ));

    let chunks = [
        "{\"display\":\"alice\",\"token\":\"fictional",
        "-request-secret\",\"nested\":{\"password\":\"",
        "fictional-password\"}}",
    ];

    let stream = futures_util::stream::iter(
        chunks
            .into_iter()
            .map(|chunk| Ok::<Bytes, Infallible>(Bytes::copy_from_slice(chunk.as_bytes()))),
    );

    let response = app
        .oneshot(
            Request::post("/capture")
                .header("content-type", "application/json")
                .body(Body::from_stream(stream))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let event = expect_http(receiver.try_recv().expect("request completion event"));

    assert_eq!(
        event.request.content_type.as_deref(),
        Some("application/json")
    );

    assert!(
        event.request.body
            == Some(json!({
                "display": "alice",
                "token": "[REDACTED]",
                "nested": {
                    "password": "[REDACTED]"
                }
            }))
    );

    assert_eq!(
        event.capture.request_body_status,
        Some(BodyCaptureStatus::Captured)
    );
    assert!(!event.capture.request_body_truncated);

    assert_no_fictional_secret(&event);
    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.captured_request_bodies, 1);
    assert_eq!(snapshot.captured_response_bodies, 1);
    assert_eq!(snapshot.unsupported_bodies, 0);
    assert_eq!(snapshot.truncated_request_bodies, 0);
    assert_eq!(snapshot.truncated_response_bodies, 0);
}

#[tokio::test]
async fn capture_stops_at_the_limit_but_the_handler_receives_every_byte() {
    let queue = queue();
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let policies = RoutePolicySet::new(CaptureProfile::Detailed);
    let mut config = MiddlewareConfig::default();
    config.limits.max_request_body_bytes = 24;

    let app = Router::new()
        .route(
            "/capture",
            post(|body: Bytes| async move {
                if body.len() == 79 {
                    StatusCode::OK
                } else {
                    StatusCode::BAD_REQUEST
                }
            }),
        )
        .layer(layer(queue, policies, config));

    let body = format!(
        "{{\"display\":\"{}\",\"token\":\"fictional-request-secret\"}}",
        "x".repeat(30)
    );
    assert_eq!(body.len(), 79);

    let response = app
        .oneshot(
            Request::post("/capture")
                .header("content-type", "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let event = expect_http(receiver.try_recv().expect("truncated-body event"));

    assert!(event.capture.request_body_truncated);
    assert_eq!(
        event.capture.request_body_status,
        Some(BodyCaptureStatus::ParseError)
    );
    assert!(event.request.body.is_none());

    assert_no_fictional_secret(&event);

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.captured_request_bodies, 0);
    assert_eq!(snapshot.truncated_request_bodies, 1);
    assert_eq!(snapshot.captured_response_bodies, 1);
    assert_eq!(snapshot.truncated_response_bodies, 0);
}

#[tokio::test]
async fn exact_limit_is_not_truncated_but_one_extra_byte_is() {
    let queue = queue();
    let receiver = queue.receiver();
    let policies = RoutePolicySet::new(CaptureProfile::Detailed);
    let mut config = MiddlewareConfig::default();
    config.limits.max_request_body_bytes = 9;

    let app = Router::new()
        .route(
            "/capture",
            post(|body: Bytes| async move {
                let _ = body;
                StatusCode::OK
            }),
        )
        .layer(layer(queue, policies, config));

    for body in ["{\"a\":\"b\"}", "{\"a\":\"bc\"}"] {
        let response = app
            .clone()
            .oneshot(
                Request::post("/capture")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    let exact = expect_http(receiver.try_recv().expect("exact-limit event"));
    assert_eq!(exact.request.body, Some(json!({ "a": "b" })));
    assert!(!exact.capture.request_body_truncated);

    let exceeded = expect_http(receiver.try_recv().expect("over-limit event"));

    assert!(exceeded.capture.request_body_truncated);
    assert_eq!(
        exceeded.capture.request_body_status,
        Some(BodyCaptureStatus::ParseError)
    );

    assert!(receiver.try_recv().is_none());
}

#[tokio::test]
async fn route_policy_disables_capture_without_buffering_or_changing_the_body() {
    let queue = queue();
    let receiver = queue.receiver();
    let mut policies = RoutePolicySet::new(CaptureProfile::Detailed);

    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/disabled"), CaptureProfile::Detailed)
                .with_body_capture(false, true),
        )
        .unwrap();

    let app = Router::new()
        .route(
            "/disabled",
            post(|body: Bytes| async move {
                if body.as_ref() == b"{\"token\":\"fictional-request-secret\"}" {
                    StatusCode::OK
                } else {
                    StatusCode::BAD_REQUEST
                }
            }),
        )
        .layer(layer(queue, policies, MiddlewareConfig::default()));

    let response = app
        .oneshot(
            Request::post("/disabled")
                .header("content-type", "application/json")
                .body(Body::from("{\"token\":\"fictional-request-secret\"}"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let event = expect_http(receiver.try_recv().expect("policy-disabled event"));

    assert!(event.request.body.is_none());
    assert!(event.capture.request_body_status.is_none());
    assert!(!event.capture.request_body_truncated);

    assert_no_fictional_secret(&event);
}

#[tokio::test]
async fn cancellation_emits_one_redacted_terminal_event() {
    let queue = queue();
    let receiver = queue.receiver();
    let policies = RoutePolicySet::new(CaptureProfile::Detailed);

    let app = Router::new()
        .route(
            "/cancel",
            post(|body: Bytes| async move {
                let _ = body;
                future::pending::<()>().await;

                #[allow(unreachable_code)]
                StatusCode::OK
            }),
        )
        .layer(layer(queue, policies, MiddlewareConfig::default()));

    let stream = futures_util::stream::unfold(0_u8, |state| async move {
        if state == 0 {
            let chunk = Bytes::from_static(b"{\"token\":\"fictional");
            Some((Ok::<Bytes, Infallible>(chunk), 1))
        } else {
            future::pending().await
        }
    });

    let request = Request::post("/cancel")
        .header("content-type", "application/json")
        .body(Body::from_stream(stream))
        .unwrap();

    let mut future = Box::pin(app.oneshot(request));

    assert!(futures_util::poll!(future.as_mut()).is_pending());
    drop(future);

    let event = expect_http(receiver.try_recv().expect("cancellation event"));

    assert!(event.capture.request_body_truncated);
    assert!(event.request.body.is_none());

    assert_no_fictional_secret(&event);
    assert!(receiver.try_recv().is_none());
}
