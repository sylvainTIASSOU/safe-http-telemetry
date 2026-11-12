#![cfg(feature = "tower")]

mod common;

use std::convert::Infallible;

use bytes::Bytes;
use common::expect_http;
use http::{Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use safe_http_telemetry::{
    core::{
        event::{QueryScalar, QueryValue},
        telemetry::InternalTelemetry,
    },
    policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
    redaction::{REDACTED, TRUNCATED},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::{HttpTelemetryLayer, MiddlewareConfig},
};
use tower::{service_fn, Layer, ServiceExt};

fn layer(
    profile: CaptureProfile,
    config: MiddlewareConfig,
) -> (BoundedEventQueue, HttpTelemetryLayer) {
    let queue = BoundedEventQueue::new(QueueConfig::new(8), InternalTelemetry::new()).unwrap();
    let layer =
        HttpTelemetryLayer::new(queue.clone(), RoutePolicySet::new(profile), config).unwrap();

    (queue, layer)
}

#[tokio::test]
async fn standard_profile_captures_and_redacts_request_and_response_metadata() {
    let (queue, layer) = layer(CaptureProfile::Standard, MiddlewareConfig::default());
    let receiver = queue.receiver();

    let request = Request::builder()
        .uri("/orders?search=rust&token=fictitious-query-secret&tag=one&tag=two")
        .header("authorization", "Bearer fictitious-header-secret")
        .header("x-debug-note", "visible")
        .body(())
        .unwrap();

    let service = service_fn(|request: Request<()>| async move {
        // Redaction changes only telemetry, never the application request.
        assert_eq!(
            request.headers()["authorization"],
            "Bearer fictitious-header-secret"
        );

        Ok::<_, Infallible>(
            Response::builder()
                .status(StatusCode::OK)
                .header("x-response", "visible")
                .header("set-cookie", "session=fictitious-response-secret")
                .body(Full::new(Bytes::from_static(b"unchanged")))
                .unwrap(),
        )
    });

    let response = layer.layer(service).oneshot(request).await.unwrap();
    let collected = response.into_body().collect().await.unwrap();
    assert_eq!(collected.to_bytes(), Bytes::from_static(b"unchanged"));

    let event = expect_http(receiver.try_recv().expect("one completed HTTP event"));
    assert!(event.request.query["search"] == QueryValue::from("rust"));
    assert!(event.request.query["token"] == QueryValue::from(REDACTED));
    assert!(
        event.request.query["tag"]
            == QueryValue::Array(vec![
                QueryScalar::String("one".to_owned()),
                QueryScalar::String("two".to_owned()),
            ])
    );
    assert_eq!(event.request.headers["authorization"], REDACTED);
    assert_eq!(event.request.headers["x-debug-note"], "visible");
    assert_eq!(event.response.headers["x-response"], "visible");
    assert_eq!(event.response.headers["set-cookie"], REDACTED);

    let encoded = serde_json::to_string(&event).unwrap();
    for secret in [
        "fictitious-query-secret",
        "fictitious-header-secret",
        "fictitious-response-secret",
    ] {
        assert!(!encoded.contains(secret), "secret survived serialization");
    }

    #[cfg(feature = "loki")]
    {
        use safe_http_telemetry::{config::ServiceConfig, loki::payload::LokiStream};

        let stream = LokiStream::from_event(&event, &ServiceConfig::default(), 1).unwrap();
        let encoded = serde_json::to_string(&stream).unwrap();

        for secret in [
            "fictitious-query-secret",
            "fictitious-header-secret",
            "fictitious-response-secret",
        ] {
            assert!(!encoded.contains(secret), "secret survived Loki conversion");
        }

        assert!(!stream.stream.contains_key("token"));
        assert!(!stream.stream.contains_key("authorization"));
        assert!(!stream.stream.contains_key("set-cookie"));
        assert!(!stream.metadata.contains_key("token"));
        assert!(!stream.metadata.contains_key("authorization"));
        assert!(!stream.metadata.contains_key("set-cookie"));
    }
}

#[tokio::test]
async fn route_policy_can_disable_query_and_each_header_side_independently() {
    let queue = BoundedEventQueue::new(QueueConfig::new(8), InternalTelemetry::new()).unwrap();
    let receiver = queue.receiver();
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);
    policies
        .add_policy(
            RoutePolicy::new(RoutePattern::exact("/orders"), CaptureProfile::Detailed)
                .with_query_capture(false)
                .with_header_capture(true, false),
        )
        .unwrap();
    let layer = HttpTelemetryLayer::new(queue, policies, MiddlewareConfig::default()).unwrap();

    let request = Request::builder()
        .uri("/orders?visible=query")
        .header("x-request", "visible")
        .body(())
        .unwrap();
    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(
            Response::builder()
                .header("x-response", "not-captured")
                .body(Full::new(Bytes::new()))
                .unwrap(),
        )
    });

    let response = layer.layer(service).oneshot(request).await.unwrap();
    response.into_body().collect().await.unwrap();

    let event = expect_http(receiver.try_recv().expect("one completed HTTP event"));
    assert!(event.request.query.is_empty());
    assert_eq!(event.request.headers["x-request"], "visible");
    assert!(event.response.headers.is_empty());
    assert!(event.request.body.is_none());
    assert!(event.response.body.is_none());
}

#[tokio::test]
async fn query_and_header_limits_bound_output_and_record_truncation() {
    let mut config = MiddlewareConfig::default();
    config.limits.max_query_parameters = 2;
    config.limits.max_headers = 2;
    config.limits.max_header_value_bytes = 12;
    config.limits.max_string_bytes = 12;
    config.limits.max_total_header_bytes = 128;

    let (queue, layer) = layer(CaptureProfile::Detailed, config);
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();

    let request = Request::builder()
        .uri("/limits?first=123456789%C3%A9XYZ&second=visible&third=omitted")
        .header("x-one", "123456789éXYZ")
        .header("x-two", "visible")
        .header("x-three", "omitted")
        .body(())
        .unwrap();
    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(Full::new(Bytes::new())))
    });

    layer.layer(service).oneshot(request).await.unwrap();

    let event = expect_http(receiver.try_recv().expect("one completed HTTP event"));
    assert_eq!(event.request.query.len(), 2);
    assert!(event.request.query["first"] == QueryValue::from(format!("1{TRUNCATED}")));
    assert!(!event.request.query.contains_key("third"));
    assert!(event.request.headers.len() <= 2);
    assert!(
        event
            .request
            .headers
            .iter()
            .map(|(name, value)| name.len() + value.len())
            .sum::<usize>()
            <= 128
    );
    assert!(telemetry.snapshot().truncated_values > 0);
}

#[test]
fn minimal_profile_does_not_enable_metadata_capture() {
    let policies = RoutePolicySet::new(CaptureProfile::Minimal);
    let decision = policies.resolve("GET", "/orders?visible=query");

    assert!(!decision.capture_query);
    assert!(!decision.capture_request_headers);
    assert!(!decision.capture_response_headers);
}
