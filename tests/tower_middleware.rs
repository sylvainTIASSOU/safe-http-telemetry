#![cfg(feature = "tower")]

mod common;

use common::expect_http;
use std::convert::Infallible;

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use safe_http_telemetry::{
    core::{
        event::{CaptureInfo, HttpEvent, LogLevel, RequestInfo, ResponseInfo, TimingInfo},
        telemetry::InternalTelemetry,
    },
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::{HttpTelemetryLayer, MiddlewareConfig, RouteResolver, UNMATCHED_ROUTE},
};
use tower::{service_fn, Layer, ServiceExt};

fn queue(capacity: usize) -> BoundedEventQueue {
    BoundedEventQueue::new(QueueConfig::new(capacity), InternalTelemetry::default()).unwrap()
}

fn policies() -> RoutePolicySet {
    RoutePolicySet::new(CaptureProfile::Minimal)
}

fn layer(queue: BoundedEventQueue, policies: RoutePolicySet) -> HttpTelemetryLayer {
    HttpTelemetryLayer::new(queue, policies, MiddlewareConfig::default()).unwrap()
}

fn dummy_event(request_id: &str) -> HttpEvent {
    HttpEvent::new(
        request_id,
        RequestInfo::new("GET", UNMATCHED_ROUTE, "/prefilled"),
        ResponseInfo::new(200),
        TimingInfo::new(1.0),
        CaptureInfo::default(),
    )
}

#[tokio::test]
async fn minimal_event_is_queued_without_altering_the_response() {
    let queue = queue(8);
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let layer = layer(queue, policies());

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(
            Response::builder()
                .status(StatusCode::CREATED)
                .header("x-test", "unchanged")
                .body(Full::new(Bytes::from_static(b"response-body")))
                .unwrap(),
        )
    });

    let request = Request::builder()
        .method("POST")
        .uri("/orders/42?token=fictional-test-token")
        .header("authorization", "Bearer fictional-test-credential")
        .body(())
        .unwrap();

    let response = layer.layer(service).oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()["x-test"], "unchanged");
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, Bytes::from_static(b"response-body"));

    let event = expect_http(receiver.try_recv().expect("one HTTP event"));
    assert_eq!(event.request.method, "POST");
    assert_eq!(event.request.route, UNMATCHED_ROUTE);
    assert_eq!(event.request.path, "/orders/42");
    assert_eq!(event.response.status_code, Some(201));
    assert!(event.request.query.is_empty());
    assert!(event.request.headers.is_empty());
    assert!(event.request.body.is_none());
    assert!(event.response.body.is_none());
    assert!(event.timing.duration_ms.is_finite());
    assert!(event.timing.duration_ms >= 0.0);
    assert!(!event.request_id.is_empty());

    let serialized = serde_json::to_string(&event).unwrap();
    assert!(!serialized.contains("fictional-test-token"));
    assert!(!serialized.contains("fictional-test-credential"));

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.observed_requests, 1);
    assert_eq!(snapshot.completed_requests, 1);
    assert_eq!(snapshot.excluded_requests, 0);
}

async fn event_for_status(status: StatusCode) -> HttpEvent {
    let queue = queue(2);
    let receiver = queue.receiver();
    let layer = layer(queue, policies());

    let service = service_fn(move |_request: Request<()>| async move {
        Ok::<_, Infallible>(
            Response::builder()
                .status(status)
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();
    assert_eq!(response.status(), status);

    expect_http(receiver.try_recv().expect("one HTTP event"))
}

#[tokio::test]
async fn response_status_controls_level_and_outcome() {
    let success = event_for_status(StatusCode::OK).await;
    assert_eq!(success.level, LogLevel::Info);

    let client_error = event_for_status(StatusCode::NOT_FOUND).await;
    assert_eq!(client_error.level, LogLevel::Warn);

    let server_error = event_for_status(StatusCode::INTERNAL_SERVER_ERROR).await;
    assert_eq!(server_error.level, LogLevel::Error);
}

#[tokio::test]
async fn excluded_route_preserves_response_without_an_event() {
    let queue = queue(2);
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let mut policies = policies();
    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/health"), CaptureProfile::Minimal).exclude(),
        )
        .unwrap();

    let layer = layer(queue, policies);
    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"healthy"))))
    });

    let request = Request::builder().uri("/health").body(()).unwrap();
    let response = layer.layer(service).oneshot(request).await.unwrap();

    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, Bytes::from_static(b"healthy"));
    assert!(receiver.try_recv().is_none());

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.observed_requests, 0);
    assert_eq!(snapshot.completed_requests, 0);
    assert_eq!(snapshot.excluded_requests, 1);
}

#[tokio::test]
async fn request_id_propagation_is_opt_in_and_validated() {
    let queue = queue(2);
    let receiver = queue.receiver();
    let layer = HttpTelemetryLayer::new(
        queue,
        policies(),
        MiddlewareConfig {
            trust_incoming_request_id: true,
            ..MiddlewareConfig::default()
        },
    )
    .unwrap();

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(Full::new(Bytes::new())))
    });

    let valid = Request::builder()
        .header("x-request-id", "client-request-123")
        .body(())
        .unwrap();
    layer.clone().layer(service).oneshot(valid).await.unwrap();
    assert_eq!(
        expect_http(receiver.try_recv().expect("valid request event")).request_id,
        "client-request-123"
    );

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(Full::new(Bytes::new())))
    });
    let invalid = Request::builder()
        .header("x-request-id", "invalid request id")
        .body(())
        .unwrap();
    layer.layer(service).oneshot(invalid).await.unwrap();

    let generated = expect_http(receiver.try_recv().expect("invalid request event")).request_id;
    assert_ne!(generated, "invalid request id");
    assert!(generated.starts_with("req-"));
}

#[derive(Clone)]
struct TestRouteResolver;

impl RouteResolver for TestRouteResolver {
    fn resolve<Body>(&self, _request: &Request<Body>) -> Option<String> {
        Some("/users/{id}".to_owned())
    }
}

#[tokio::test]
async fn normalized_route_is_separate_from_the_raw_path() {
    let queue = queue(2);
    let receiver = queue.receiver();
    let layer = HttpTelemetryLayer::with_route_resolver(
        queue,
        policies(),
        MiddlewareConfig::default(),
        TestRouteResolver,
    )
    .unwrap();

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(Full::new(Bytes::new())))
    });
    let request = Request::builder().uri("/users/123").body(()).unwrap();

    layer.layer(service).oneshot(request).await.unwrap();

    let event = expect_http(receiver.try_recv().expect("one HTTP event"));
    assert_eq!(event.request.route, "/users/{id}");
    assert_eq!(event.request.path, "/users/123");
}

#[tokio::test]
async fn saturated_queue_drops_telemetry_without_changing_response() {
    let queue = queue(1);
    let receiver = queue.receiver();
    queue.try_push(dummy_event("prefilled")).unwrap();
    let layer = layer(queue.clone(), policies());

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(
            b"application-response",
        ))))
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(body, Bytes::from_static(b"application-response"));

    let retained = expect_http(receiver.try_recv().expect("original queued event"));
    assert_eq!(retained.request_id, "prefilled");
    assert!(receiver.try_recv().is_none());

    let stats = queue.stats();
    assert_eq!(stats.accepted_events, 1);
    assert_eq!(stats.rejected_events, 1);
    assert_eq!(stats.dropped_events, 1);
}
