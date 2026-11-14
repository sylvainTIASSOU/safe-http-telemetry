#![cfg(all(feature = "tower", feature = "body-capture"))]

mod common;

use std::{collections::VecDeque, convert::Infallible, future};

use bytes::Bytes;
use common::expect_http;
use http::{HeaderMap, Request, Response, StatusCode};
use http_body::{Body, Frame};
use http_body_util::{BodyExt, Full, StreamBody};
#[cfg(feature = "loki")]
use safe_http_telemetry::{config::ServiceConfig, loki::payload::LokiStream};
use safe_http_telemetry::{
    core::{
        event::{BodyCaptureStatus, HttpEvent, ResponseOutcome},
        telemetry::InternalTelemetry,
    },
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::{HttpTelemetryLayer, MiddlewareConfig},
};
use serde_json::json;
use tower::{service_fn, Layer, ServiceExt};

fn layer(
    policies: RoutePolicySet,
    config: MiddlewareConfig,
) -> (BoundedEventQueue, HttpTelemetryLayer) {
    let queue = BoundedEventQueue::new(QueueConfig::new(16), InternalTelemetry::default()).unwrap();
    let layer = HttpTelemetryLayer::new(queue.clone(), policies, config).unwrap();
    (queue, layer)
}

fn assert_no_fictional_secret(event: &HttpEvent) {
    let serialized = serde_json::to_string(event).unwrap();
    assert!(!serialized.contains("fictional-response-secret"));
    assert!(!serialized.contains("fictional-password"));

    #[cfg(feature = "loki")]
    {
        let loki = LokiStream::from_event(event, &ServiceConfig::default(), 1).unwrap();
        let serialized = serde_json::to_string(&loki).unwrap();
        assert!(!serialized.contains("fictional-response-secret"));
        assert!(!serialized.contains("fictional-password"));
    }
}

#[tokio::test]
async fn fragmented_response_json_is_redacted_and_forwarded_without_change() {
    let (queue, layer) = layer(
        RoutePolicySet::new(CaptureProfile::Detailed),
        MiddlewareConfig::default(),
    );
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let expected = b"{\"display\":\"alice\",\"token\":\"fictional-response-secret\",\"nested\":{\"password\":\"fictional-password\"}}";

    let service = service_fn(|_request: Request<()>| async move {
        let frames = VecDeque::from([
            Ok::<_, Infallible>(Frame::data(Bytes::from_static(
                b"{\"display\":\"alice\",\"token\":\"fictional",
            ))),
            Ok(Frame::data(Bytes::from_static(
                b"-response-secret\",\"nested\":{\"password\":\"",
            ))),
            Ok(Frame::data(Bytes::from_static(b"fictional-password\"}}"))),
            Ok(Frame::trailers(HeaderMap::new())),
        ]);
        let body = StreamBody::new(futures_util::stream::iter(frames));

        Ok::<_, Infallible>(
            Response::builder()
                .header("content-type", "application/json")
                .header("content-length", expected.len().to_string())
                .body(body)
                .unwrap(),
        )
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(receiver.try_recv().is_none());

    let collected = response.into_body().collect().await.unwrap();
    assert!(collected.trailers().is_some());
    assert_eq!(collected.to_bytes(), Bytes::from_static(expected));

    let event = expect_http(receiver.try_recv().expect("response completion event"));
    assert_eq!(
        event.response.content_type.as_deref(),
        Some("application/json")
    );
    assert_eq!(event.response.content_length, Some(expected.len() as u64));
    assert!(
        event.response.body
            == Some(json!({
                "display": "alice",
                "token": "[REDACTED]",
                "nested": { "password": "[REDACTED]" }
            }))
    );
    assert_eq!(
        event.capture.response_body_status,
        Some(BodyCaptureStatus::Captured)
    );
    assert!(!event.capture.response_body_truncated);
    assert!(event.capture.request_body_status.is_none());
    assert_no_fictional_secret(&event);

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.captured_request_bodies, 0);
    assert_eq!(snapshot.captured_response_bodies, 1);
    assert_eq!(snapshot.unsupported_bodies, 0);
    assert_eq!(snapshot.truncated_request_bodies, 0);
    assert_eq!(snapshot.truncated_response_bodies, 0);
}

#[tokio::test]
async fn response_limit_does_not_change_or_buffer_the_application_stream() {
    let (queue, layer) = layer(
        RoutePolicySet::new(CaptureProfile::Detailed),
        MiddlewareConfig {
            limits: safe_http_telemetry::config::CaptureLimits {
                max_response_body_bytes: 24,
                ..safe_http_telemetry::config::CaptureLimits::default()
            },
            ..MiddlewareConfig::default()
        },
    );
    let receiver = queue.receiver();
    let expected = format!(
        "{{\"display\":\"{}\",\"token\":\"fictional-response-secret\"}}",
        "x".repeat(30)
    );
    let expected = Bytes::copy_from_slice(expected.as_bytes());

    let service = service_fn(|_request: Request<()>| {
        let expected = expected.clone();
        async move {
            Ok::<_, Infallible>(
                Response::builder()
                    .header("content-type", "application/json")
                    .body(Full::new(expected))
                    .unwrap(),
            )
        }
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();
    let collected = response.into_body().collect().await.unwrap();
    assert_eq!(collected.to_bytes(), expected);

    let event = expect_http(receiver.try_recv().expect("truncated response event"));
    assert!(event.capture.response_body_truncated);
    assert_eq!(
        event.capture.response_body_status,
        Some(BodyCaptureStatus::ParseError)
    );
    assert!(event.response.body.is_none());
    assert_no_fictional_secret(&event);
}

#[tokio::test]
async fn exact_response_limit_is_not_truncated_but_one_extra_byte_is() {
    let mut config = MiddlewareConfig::default();
    config.limits.max_response_body_bytes = 9;
    let (queue, layer) = layer(RoutePolicySet::new(CaptureProfile::Detailed), config);
    let receiver = queue.receiver();

    let service = service_fn(|request: Request<()>| async move {
        let body = if request.uri().path() == "/exact" {
            "{\"a\":\"b\"}"
        } else {
            "{\"a\":\"bc\"}"
        };
        Ok::<_, Infallible>(
            Response::builder()
                .header("content-type", "application/json")
                .body(Full::new(Bytes::copy_from_slice(body.as_bytes())))
                .unwrap(),
        )
    });
    let app = layer.layer(service);

    for path in ["/exact", "/exceeded"] {
        let response = app
            .clone()
            .oneshot(Request::builder().uri(path).body(()).unwrap())
            .await
            .unwrap();
        let _ = response.into_body().collect().await.unwrap();
    }

    let exact = expect_http(receiver.try_recv().expect("exact-limit response event"));
    assert_eq!(exact.response.body, Some(json!({ "a": "b" })));
    assert!(!exact.capture.response_body_truncated);

    let exceeded = expect_http(receiver.try_recv().expect("over-limit response event"));
    assert!(exceeded.capture.response_body_truncated);
    assert_eq!(
        exceeded.capture.response_body_status,
        Some(BodyCaptureStatus::ParseError)
    );
    assert!(receiver.try_recv().is_none());
}

#[tokio::test]
async fn response_policy_can_be_disabled_independently() {
    let mut policies = RoutePolicySet::new(CaptureProfile::Detailed);
    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/disabled"), CaptureProfile::Detailed)
                .with_body_capture(true, false),
        )
        .unwrap();
    let (queue, layer) = layer(policies, MiddlewareConfig::default());
    let receiver = queue.receiver();

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(
            Response::builder()
                .header("content-type", "application/json")
                .body(Full::new(Bytes::from_static(
                    b"{\"token\":\"fictional-response-secret\"}",
                )))
                .unwrap(),
        )
    });

    let response = layer
        .layer(service)
        .oneshot(Request::builder().uri("/disabled").body(()).unwrap())
        .await
        .unwrap();
    let collected = response.into_body().collect().await.unwrap();
    assert_eq!(
        collected.to_bytes(),
        Bytes::from_static(b"{\"token\":\"fictional-response-secret\"}")
    );

    let event = expect_http(receiver.try_recv().expect("policy-disabled response event"));
    assert!(event.response.body.is_none());
    assert!(event.capture.response_body_status.is_none());
    assert!(!event.capture.response_body_truncated);
    assert_no_fictional_secret(&event);
}

#[tokio::test]
async fn response_stream_errors_are_forwarded_without_exporting_the_prefix() {
    let (queue, layer) = layer(
        RoutePolicySet::new(CaptureProfile::Detailed),
        MiddlewareConfig::default(),
    );
    let receiver = queue.receiver();

    let service = service_fn(|_request: Request<()>| async {
        let frames = vec![
            Ok::<_, std::io::Error>(Frame::data(Bytes::from_static(b"{\"token\":\"fictional"))),
            Err(std::io::Error::other("test response error")),
        ];
        Ok::<_, Infallible>(
            Response::builder()
                .header("content-type", "application/json")
                .body(StreamBody::new(futures_util::stream::iter(frames)))
                .unwrap(),
        )
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();
    let mut body = response.into_body();

    let first = body.frame().await.unwrap().unwrap();
    assert_eq!(
        first.into_data().unwrap(),
        Bytes::from_static(b"{\"token\":\"fictional")
    );
    let error = body.frame().await.unwrap().unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Other);

    let event = expect_http(receiver.try_recv().expect("failed response event"));
    assert_eq!(event.response.outcome, Some(ResponseOutcome::ServerError));
    assert_eq!(
        event.capture.response_body_status,
        Some(BodyCaptureStatus::ParseError)
    );
    assert!(event.capture.response_body_truncated);
    assert!(event.response.body.is_none());
    assert_no_fictional_secret(&event);
}

#[tokio::test]
async fn dropping_a_partial_response_emits_one_redacted_cancellation() {
    let (queue, layer) = layer(
        RoutePolicySet::new(CaptureProfile::Detailed),
        MiddlewareConfig::default(),
    );
    let receiver = queue.receiver();

    let service = service_fn(|_request: Request<()>| async {
        let stream = futures_util::stream::unfold(0_u8, |state| async move {
            if state == 0 {
                Some((
                    Ok::<_, Infallible>(Frame::data(Bytes::from_static(b"{\"token\":\"fictional"))),
                    1,
                ))
            } else {
                future::pending().await
            }
        });
        Ok::<_, Infallible>(
            Response::builder()
                .header("content-type", "application/json")
                .body(StreamBody::new(stream))
                .unwrap(),
        )
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();
    let mut body = Box::pin(response.into_body());
    let first = body.frame().await.unwrap().unwrap();
    assert!(first.data_ref().is_some());
    drop(body);

    let event = expect_http(receiver.try_recv().expect("cancelled response event"));
    assert_eq!(event.response.outcome, Some(ResponseOutcome::Cancelled));
    assert!(event.capture.response_body_truncated);
    assert!(event.response.body.is_none());
    assert_no_fictional_secret(&event);
    assert!(receiver.try_recv().is_none());
}

#[tokio::test]
async fn empty_response_is_marked_captured_without_truncation() {
    let (queue, layer) = layer(
        RoutePolicySet::new(CaptureProfile::Detailed),
        MiddlewareConfig::default(),
    );
    let receiver = queue.receiver();
    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(
            Response::builder()
                .header("content-type", "application/json")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();
    assert!(response.body().is_end_stream());

    let event = expect_http(receiver.try_recv().expect("empty response event"));
    assert_eq!(
        event.capture.response_body_status,
        Some(BodyCaptureStatus::Captured)
    );
    assert!(!event.capture.response_body_truncated);
    assert!(event.response.body.is_none());
}
