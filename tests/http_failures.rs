#![cfg(feature = "tower")]

mod common;

use common::expect_http;
use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::Bytes;
use http::{Request, Response, StatusCode};
use http_body::{Body, Frame};
use http_body_util::Full;
use safe_http_telemetry::{
    core::{event::ResponseOutcome, telemetry::InternalTelemetry},
    policy::{CaptureProfile, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::{ErrorClassifier, HttpTelemetryLayer, MiddlewareConfig, RequestFailure},
};
use tower::{service_fn, Layer, ServiceExt};

#[derive(Debug)]
enum TestError {
    Handler(()),
    Timeout,
    Body,
}

#[derive(Clone)]
struct TestErrorClassifier;

impl ErrorClassifier<TestError> for TestErrorClassifier {
    fn classify(&self, error: &TestError) -> RequestFailure {
        match error {
            TestError::Timeout => RequestFailure::Timeout,
            TestError::Handler(_) | TestError::Body => RequestFailure::ServerError,
        }
    }
}

fn layer() -> (BoundedEventQueue, HttpTelemetryLayer) {
    let queue = BoundedEventQueue::new(QueueConfig::new(8), InternalTelemetry::default()).unwrap();
    let layer = HttpTelemetryLayer::new(
        queue.clone(),
        RoutePolicySet::new(CaptureProfile::Minimal),
        MiddlewareConfig::default(),
    )
    .unwrap();

    (queue, layer)
}

#[tokio::test]
async fn handler_error_has_no_invented_status_or_sensitive_message() {
    let (queue, layer) = layer();
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let layer = layer.with_error_classifier(TestErrorClassifier);

    let service = service_fn(|_request: Request<()>| async {
        Err::<Response<Full<Bytes>>, _>(TestError::Handler(()))
    });

    let result = layer.layer(service).oneshot(Request::new(())).await;
    assert!(matches!(result, Err(TestError::Handler(_))));

    let event = expect_http(receiver.try_recv().expect("handler failure event"));
    assert_eq!(event.response.status_code, None);
    assert_eq!(event.response.outcome, Some(ResponseOutcome::ServerError));
    assert!(event.timing.time_to_first_byte_ms.is_none());

    let serialized = serde_json::to_string(&event).unwrap();
    assert!(!serialized.contains("fictional-sensitive-handler-error"));
    assert_eq!(telemetry.snapshot().failed_requests, 1);
}

#[tokio::test]
async fn typed_timeout_is_distinct_from_other_service_errors() {
    let (queue, layer) = layer();
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let layer = layer.with_error_classifier(TestErrorClassifier);

    let service = service_fn(|_request: Request<()>| async {
        Err::<Response<Full<Bytes>>, _>(TestError::Timeout)
    });

    let result = layer.layer(service).oneshot(Request::new(())).await;
    assert!(matches!(result, Err(TestError::Timeout)));

    let event = expect_http(receiver.try_recv().expect("timeout event"));
    assert_eq!(event.response.status_code, None);
    assert_eq!(event.response.outcome, Some(ResponseOutcome::Timeout));
    assert_eq!(telemetry.snapshot().timed_out_requests, 1);
    assert_eq!(telemetry.snapshot().failed_requests, 0);
}

struct FailingBody {
    polled: bool,
}

impl Body for FailingBody {
    type Data = Bytes;
    type Error = TestError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.polled {
            return Poll::Ready(None);
        }

        self.polled = true;
        Poll::Ready(Some(Err(TestError::Body)))
    }
}

#[tokio::test]
async fn body_error_preserves_known_status_and_returns_original_error() {
    let (queue, layer) = layer();
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .body(FailingBody { polled: false })
                .unwrap(),
        )
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();
    assert!(receiver.try_recv().is_none());

    let mut body = Box::pin(response.into_body());
    let frame = std::future::poll_fn(|context| body.as_mut().poll_frame(context)).await;
    assert!(matches!(frame, Some(Err(TestError::Body))));

    let event = expect_http(receiver.try_recv().expect("body failure event"));
    assert_eq!(event.response.status_code, Some(206));
    assert_eq!(event.response.outcome, Some(ResponseOutcome::ServerError));
    assert!(event.timing.time_to_first_byte_ms.is_some());
    assert!(event.timing.body_duration_ms.is_some());
    assert_eq!(telemetry.snapshot().failed_requests, 1);

    drop(body);
    assert!(receiver.try_recv().is_none());
}
