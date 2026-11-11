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
    core::{
        event::{LogLevel, ResponseOutcome},
        telemetry::InternalTelemetry,
    },
    policy::{CaptureProfile, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::{HttpTelemetryLayer, MiddlewareConfig},
};
use tower::{service_fn, Layer, ServiceExt};

fn layer(config: MiddlewareConfig) -> (BoundedEventQueue, HttpTelemetryLayer) {
    let queue = BoundedEventQueue::new(QueueConfig::new(8), InternalTelemetry::default()).unwrap();
    let layer = HttpTelemetryLayer::new(
        queue.clone(),
        RoutePolicySet::new(CaptureProfile::Minimal),
        config,
    )
    .unwrap();

    (queue, layer)
}

#[tokio::test]
async fn dropping_request_future_emits_one_cancellation_and_releases_state() {
    let (queue, layer) = layer(MiddlewareConfig::default());
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();
    let (started_sender, started_receiver) = tokio::sync::oneshot::channel();
    let mut started_sender = Some(started_sender);

    let service = service_fn(move |_request: Request<()>| {
        if let Some(sender) = started_sender.take() {
            let _ = sender.send(());
        }

        async { std::future::pending::<Result<Response<Full<Bytes>>, Infallible>>().await }
    });

    let task = tokio::spawn(async move {
        let _ = layer.layer(service).oneshot(Request::new(())).await;
    });

    started_receiver.await.unwrap();
    assert_eq!(telemetry.snapshot().active_requests, 1);

    task.abort();
    let _ = task.await;

    let event = expect_http(receiver.try_recv().expect("request cancellation event"));
    assert_eq!(event.response.status_code, None);
    assert_eq!(event.response.outcome, Some(ResponseOutcome::Cancelled));
    assert_eq!(event.level, LogLevel::Warn);
    assert!(event.timing.time_to_first_byte_ms.is_none());
    assert!(event.timing.body_duration_ms.is_none());

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.active_requests, 0);
    assert_eq!(snapshot.cancelled_requests, 1);
    assert_eq!(snapshot.completed_requests, 0);
    assert!(receiver.try_recv().is_none());
}

struct PendingAfterFirst {
    emitted: bool,
}

impl Body for PendingAfterFirst {
    type Data = Bytes;
    type Error = Infallible;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        _context: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        if self.emitted {
            return Poll::Pending;
        }

        self.emitted = true;
        Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(b"first")))))
    }
}

#[tokio::test]
async fn dropping_partial_stream_preserves_known_response_metadata() {
    let (queue, layer) = layer(MiddlewareConfig::default());
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(
            Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .body(PendingAfterFirst { emitted: false })
                .unwrap(),
        )
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();
    assert_eq!(telemetry.snapshot().active_requests, 1);

    let mut body = Box::pin(response.into_body());
    let frame = std::future::poll_fn(|context| body.as_mut().poll_frame(context))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(frame.into_data().unwrap(), Bytes::from_static(b"first"));
    assert!(receiver.try_recv().is_none());

    drop(body);

    let event = expect_http(receiver.try_recv().expect("stream cancellation event"));
    assert_eq!(event.response.status_code, Some(206));
    assert_eq!(event.response.outcome, Some(ResponseOutcome::Cancelled));
    assert!(event.timing.time_to_first_byte_ms.is_some());
    assert!(event.timing.body_duration_ms.is_some());

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.active_requests, 0);
    assert_eq!(snapshot.cancelled_requests, 1);
    assert_eq!(snapshot.completed_requests, 0);
    assert!(receiver.try_recv().is_none());
}

#[tokio::test]
async fn cancellation_events_can_be_disabled_without_leaking_active_requests() {
    let (queue, layer) = layer(MiddlewareConfig {
        emit_cancellation_events: false,
        ..MiddlewareConfig::default()
    });
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(PendingAfterFirst { emitted: false }))
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();
    assert_eq!(telemetry.snapshot().active_requests, 1);

    drop(response);

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.active_requests, 0);
    assert_eq!(snapshot.cancelled_requests, 1);
    assert!(receiver.try_recv().is_none());
}

#[tokio::test]
async fn normal_completion_does_not_emit_a_later_cancellation() {
    let (queue, layer) = layer(MiddlewareConfig::default());
    let receiver = queue.receiver();
    let telemetry = queue.telemetry();

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(Full::new(Bytes::from_static(b"complete"))))
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();

    let collected = http_body_util::BodyExt::collect(response.into_body())
        .await
        .unwrap();
    assert_eq!(collected.to_bytes(), Bytes::from_static(b"complete"));

    let event = expect_http(receiver.try_recv().expect("normal completion event"));
    assert_eq!(event.response.outcome, Some(ResponseOutcome::Success));

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.active_requests, 0);
    assert_eq!(snapshot.completed_requests, 1);
    assert_eq!(snapshot.cancelled_requests, 0);
    assert!(receiver.try_recv().is_none());
}
