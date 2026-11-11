#![cfg(feature = "tower")]

mod common;

use common::expect_http;
use std::{collections::VecDeque, convert::Infallible, time::Duration};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Request, Response, StatusCode};
use http_body::{Body, Frame};
use http_body_util::{BodyExt, Full, StreamBody};
use safe_http_telemetry::{
    core::telemetry::InternalTelemetry,
    policy::{CaptureProfile, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::{HttpTelemetryLayer, MiddlewareConfig},
};
use tokio::time::sleep;
use tower::{service_fn, Layer, ServiceExt};

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
async fn ordinary_body_is_unchanged_and_event_waits_for_completion() {
    let (queue, layer) = layer();
    let receiver = queue.receiver();

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(
            Response::builder()
                .status(StatusCode::CREATED)
                .header("content-length", "13")
                .header("x-test", "unchanged")
                .body(Full::new(Bytes::from_static(b"ordinary-body")))
                .unwrap(),
        )
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(response.headers()["content-length"], "13");
    assert_eq!(response.headers()["x-test"], "unchanged");
    assert_eq!(response.body().size_hint().exact(), Some(13));
    assert!(receiver.try_recv().is_none());

    let collected = response.into_body().collect().await.unwrap();
    assert_eq!(collected.to_bytes(), Bytes::from_static(b"ordinary-body"));

    let event = expect_http(receiver.try_recv().expect("event after body completion"));
    assert_eq!(event.response.status_code, Some(201));
    assert!(event.timing.time_to_first_byte_ms.is_some());
    assert!(event.timing.body_duration_ms.is_some());
    assert!(receiver.try_recv().is_none());
}

#[tokio::test]
async fn empty_body_completes_immediately_exactly_once() {
    let (queue, layer) = layer();
    let receiver = queue.receiver();

    let service = service_fn(|_request: Request<()>| async {
        Ok::<_, Infallible>(Response::new(Full::new(Bytes::new())))
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();

    let event = expect_http(receiver.try_recv().expect("empty-body completion event"));
    assert_eq!(event.timing.body_duration_ms, Some(0.0));

    let collected = response.into_body().collect().await.unwrap();
    assert!(collected.to_bytes().is_empty());
    assert!(receiver.try_recv().is_none());
}

#[tokio::test]
async fn delayed_stream_and_trailers_are_forwarded_without_buffering() {
    let (queue, layer) = layer();
    let receiver = queue.receiver();

    let service = service_fn(|_request: Request<()>| async {
        let mut trailers = HeaderMap::new();
        trailers.insert("x-test-trailer", HeaderValue::from_static("complete"));

        let frames: VecDeque<Result<Frame<Bytes>, Infallible>> = VecDeque::from([
            Ok(Frame::data(Bytes::from_static(b"first-"))),
            Ok(Frame::data(Bytes::from_static(b"second-"))),
            Ok(Frame::data(Bytes::from_static(b"third"))),
            Ok(Frame::trailers(trailers)),
        ]);

        let stream = futures_util::stream::unfold(frames, |mut frames| async move {
            sleep(Duration::from_millis(5)).await;
            frames.pop_front().map(|frame| (frame, frames))
        });

        Ok::<_, Infallible>(
            Response::builder()
                .header("x-stream", "unchanged")
                .body(StreamBody::new(stream))
                .unwrap(),
        )
    });

    let response = layer
        .layer(service)
        .oneshot(Request::new(()))
        .await
        .unwrap();

    assert_eq!(response.headers()["x-stream"], "unchanged");
    assert!(receiver.try_recv().is_none());

    let collected = response.into_body().collect().await.unwrap();
    assert_eq!(collected.trailers().unwrap()["x-test-trailer"], "complete");
    assert_eq!(
        collected.to_bytes(),
        Bytes::from_static(b"first-second-third")
    );

    let event = expect_http(receiver.try_recv().expect("stream completion event"));
    let duration = event.timing.duration_ms;
    let first_byte = event.timing.time_to_first_byte_ms.unwrap();
    let body_duration = event.timing.body_duration_ms.unwrap();

    assert!(first_byte >= 0.0);
    assert!(body_duration > 0.0);
    assert!(duration >= first_byte);
    assert!(receiver.try_recv().is_none());
}
