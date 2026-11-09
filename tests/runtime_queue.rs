use safe_http_telemetry::{
    core::{
        event::{CaptureInfo, HttpEvent, RequestInfo, ResponseInfo, TimingInfo},
        telemetry::InternalTelemetry,
    },
    runtime::queue::{
        BoundedEventQueue, QueueConfig, QueueConfigError, SaturationPolicy, TryPushError,
    },
};
use serde_json::json;

fn sample_event(request_id: &str) -> HttpEvent {
    let mut request = RequestInfo::new("POST", "/api/users/{id}", "/api/users/42");
    request.body = Some(json!({
        "password": "[REDACTED]",
        "access_token": "[REDACTED]"
    }));

    let mut response = ResponseInfo::new(201);
    response.body = Some(json!({
        "result": "created"
    }));

    HttpEvent::new(
        request_id,
        request,
        response,
        TimingInfo::new(4.0),
        CaptureInfo::default(),
    )
}

fn queue_with_capacity(capacity: usize) -> BoundedEventQueue {
    BoundedEventQueue::new(QueueConfig::new(capacity), InternalTelemetry::new()).unwrap()
}

#[test]
fn zero_capacity_is_rejected() {
    let result = BoundedEventQueue::new(QueueConfig::new(0), InternalTelemetry::new());

    assert!(matches!(result, Err(QueueConfigError::ZeroCapacity)));
}

#[test]
fn bounded_queue_reports_capacity_and_occupancy() {
    let queue = queue_with_capacity(2);

    assert_eq!(queue.stats().capacity, 2);
    assert_eq!(queue.stats().occupancy, 0);
    assert_eq!(
        queue.stats().saturation_policy,
        SaturationPolicy::DropNewest
    );

    queue.try_push(sample_event("request-queue-0001")).unwrap();

    assert_eq!(queue.stats().capacity, 2);
    assert_eq!(queue.stats().occupancy, 1);
    assert_eq!(queue.stats().accepted_events, 1);
}

#[tokio::test]
async fn saturation_drops_the_newest_event_and_updates_counters() {
    let queue = queue_with_capacity(2);
    let receiver = queue.receiver();

    queue.try_push(sample_event("request-queue-0001")).unwrap();
    queue.try_push(sample_event("request-queue-0002")).unwrap();

    let error = queue
        .try_push(sample_event("request-queue-newest"))
        .unwrap_err();

    assert_eq!(error, TryPushError::Full);
    assert!(!format!("{error:?}").contains("request-queue-newest"));

    let stats = queue.stats();
    assert_eq!(stats.accepted_events, 2);
    assert_eq!(stats.rejected_events, 1);
    assert_eq!(stats.dropped_events, 1);
    assert_eq!(stats.occupancy, 2);

    assert_eq!(
        receiver.recv().await.unwrap().request_id,
        "request-queue-0001"
    );
    assert_eq!(
        receiver.recv().await.unwrap().request_id,
        "request-queue-0002"
    );
}

#[tokio::test]
async fn close_drains_buffered_events_then_ends_receivers() {
    let queue = queue_with_capacity(2);
    let receiver = queue.receiver();

    queue.try_push(sample_event("request-queue-0001")).unwrap();

    assert!(queue.close());
    assert!(!queue.close());
    assert!(queue.is_closed());
    assert_eq!(
        queue.try_push(sample_event("request-queue-closed")),
        Err(TryPushError::Closed)
    );

    assert_eq!(
        receiver.recv().await.unwrap().request_id,
        "request-queue-0001"
    );
    assert!(receiver.recv().await.is_none());
    assert!(receiver.recv().await.is_none());

    let stats = queue.stats();
    assert_eq!(stats.accepted_events, 1);
    assert_eq!(stats.rejected_events, 1);
    assert_eq!(stats.dropped_events, 0);
    assert_eq!(stats.occupancy, 0);
    assert!(stats.closed);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cloned_receivers_distribute_each_event_exactly_once() {
    let queue = queue_with_capacity(8);

    for index in 0..6 {
        queue
            .try_push(sample_event(&format!("request-queue-{index:04}")))
            .unwrap();
    }

    let mut workers = Vec::new();
    for _ in 0..2 {
        let receiver = queue.receiver();
        workers.push(tokio::spawn(async move {
            let mut request_ids = Vec::new();
            while let Some(event) = receiver.recv().await {
                request_ids.push(event.request_id);
            }
            request_ids
        }));
    }

    queue.close();

    let mut received = Vec::new();
    for worker in workers {
        received.extend(worker.await.unwrap());
    }
    received.sort();

    let expected = (0..6)
        .map(|index| format!("request-queue-{index:04}"))
        .collect::<Vec<_>>();

    assert_eq!(received, expected);
    assert_eq!(queue.stats().accepted_events, 6);
    assert_eq!(queue.stats().occupancy, 0);
}
