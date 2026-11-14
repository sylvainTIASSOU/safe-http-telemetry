#![cfg(feature = "loki")]

use std::time::Duration;

use safe_http_telemetry::{
    config::ServiceConfig,
    core::{
        event::{CaptureInfo, HttpEvent, RequestInfo, ResponseInfo, TimingInfo},
        telemetry::InternalTelemetry,
    },
    loki::{
        config::LokiConfig,
        exporter::{LokiExporter, ShutdownOutcome},
    },
    runtime::queue::{BoundedEventQueue, QueueConfig},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
    time::timeout,
};

fn sample_service() -> ServiceConfig {
    ServiceConfig {
        name: "long-outage-test".to_owned(),
        environment: "test".to_owned(),
        version: "0.1.0".to_owned(),
        region: None,
    }
}

fn sample_event(request_id: &str) -> HttpEvent {
    let mut request = RequestInfo::new("POST", "/api/users/{id}", "/api/users/42");
    request.body = Some(serde_json::json!({ "password": "[REDACTED]" }));
    HttpEvent::new(
        request_id,
        request,
        ResponseInfo::new(201),
        TimingInfo::new(2.5),
        CaptureInfo::default(),
    )
}

fn queue_with_capacity(capacity: usize) -> (BoundedEventQueue, InternalTelemetry) {
    let telemetry = InternalTelemetry::new();
    let queue = BoundedEventQueue::new(QueueConfig::new(capacity), telemetry.clone()).unwrap();
    (queue, telemetry)
}

/// Config designed to fail fast: no retries, short timeout.
fn failing_exporter_config(url: String) -> LokiConfig {
    let mut config = LokiConfig::new(url).unwrap();
    config.batch.max_events = 1;
    config.batch.max_bytes = 1024 * 1024;
    config.batch.flush_interval = Duration::from_millis(10);
    config.request_timeout = Duration::from_millis(100);
    config.retry.max_retries = 0;
    config.shutdown.deadline = Duration::from_millis(50);
    config
}

/// Config designed to recover: a few retries so a transient 503 is retried.
fn recovering_exporter_config(url: String) -> LokiConfig {
    let mut config = LokiConfig::new(url).unwrap();
    config.batch.max_events = 1;
    config.batch.max_bytes = 1024 * 1024;
    config.batch.flush_interval = Duration::from_millis(10);
    config.request_timeout = Duration::from_millis(200);
    config.retry.max_retries = 3;
    config.retry.initial_backoff = Duration::from_millis(1);
    config.retry.max_backoff = Duration::from_millis(5);
    config.shutdown.deadline = Duration::from_secs(2);
    config
}

/// A server that always replies 503 Service Unavailable.
/// Returns the push URL, a stop signal, and the join handle.
async fn spawn_always_503_server() -> (String, oneshot::Sender<()>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop_sender, mut stop_receiver) = oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = &mut stop_receiver => break,
                accepted = listener.accept() => {
                    if let Ok((mut socket, _)) = accepted {
                        let _ = read_http_request(&mut socket).await;
                        let _ = socket
                            .write_all(
                                b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                            )
                            .await;
                    }
                }
            }
        }
    });

    (
        format!("http://{address}/loki/api/v1/push"),
        stop_sender,
        server,
    )
}

/// A server that returns 503 for the first `failures` requests, then 204.
async fn spawn_recovering_server(failures: usize) -> (String, oneshot::Sender<()>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop_sender, mut stop_receiver) = oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        let mut seen = 0_usize;
        loop {
            tokio::select! {
                _ = &mut stop_receiver => break,
                accepted = listener.accept() => {
                    if let Ok((mut socket, _)) = accepted {
                        let _ = read_http_request(&mut socket).await;
                        let status = if seen < failures { 503 } else { 204 };
                        seen += 1;
                        let response = format!(
                            "HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        let _ = socket.write_all(response.as_bytes()).await;
                    }
                }
            }
        }
    });

    (
        format!("http://{address}/loki/api/v1/push"),
        stop_sender,
        server,
    )
}

async fn read_http_request(socket: &mut TcpStream) -> Vec<u8> {
    let mut buffer = Vec::new();
    let header_end = loop {
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
        let mut chunk = [0_u8; 1_024];
        let read = socket.read(&mut chunk).await.unwrap();
        assert!(read > 0, "client closed before sending headers");
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = String::from_utf8_lossy(&buffer[..header_end]).to_ascii_lowercase();
    let content_length = head
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);

    while buffer.len() < header_end + content_length {
        let mut chunk = [0_u8; 1_024];
        let read = socket.read(&mut chunk).await.unwrap();
        assert!(read > 0, "client closed before sending body");
        buffer.extend_from_slice(&chunk[..read]);
    }

    buffer
}

// ---------------------------------------------------------------------------
// Test 1: prolonged outage saturates the bounded queue and accounts for losses
// ---------------------------------------------------------------------------

#[tokio::test]
async fn prolonged_outage_saturates_bounded_queue_and_counts_drops() {
    // 1. Start a server that always fails.
    let (push_url, _stop_server, _server) = spawn_always_503_server().await;

    // 2. Small bounded queue: capacity 2.
    let (queue, telemetry) = queue_with_capacity(2);

    // 3. Exporter with no retries.
    let config = failing_exporter_config(push_url);
    let exporter = LokiExporter::new(config, sample_service(), queue.clone(), 1)
        .expect("exporter config")
        .start();

    // 4. Push 10 events synchronously. The first 2 are accepted, the rest are dropped.
    let mut accepted = 0_usize;
    let mut rejected = 0_usize;
    for index in 0..10 {
        match queue.try_push(sample_event(&format!("req-outage-{index}"))) {
            Ok(()) => accepted += 1,
            Err(_) => rejected += 1,
        }
    }

    assert_eq!(accepted, 2, "queue capacity must bound accepted events");
    assert_eq!(rejected, 8, "overflow must be rejected under DropNewest");

    // 5. Give the worker time to drain the 2 accepted events and fail them.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let snapshot = telemetry.snapshot();
    assert!(
        snapshot.dropped_events >= 8,
        "dropped_events must account for rejected events, got {}",
        snapshot.dropped_events
    );
    assert!(
        snapshot.failed_batches >= 1,
        "the worker must have attempted at least one export"
    );

    // 6. Shutdown must complete within its deadline.
    let report = timeout(Duration::from_secs(1), exporter.shutdown())
        .await
        .expect("shutdown must not hang");
    assert!(
        matches!(
            report.outcome,
            ShutdownOutcome::Drained
                | ShutdownOutcome::Aborted
                | ShutdownOutcome::TimedOut
                | ShutdownOutcome::WorkerFailure
        ),
        "shutdown must produce a defined outcome"
    );

    // 7. Final invariant: pending_export_events must be zero after shutdown.
    assert_eq!(
        telemetry.snapshot().pending_export_events,
        0,
        "shutdown must reset pending export events"
    );
}

// ---------------------------------------------------------------------------
// Test 2: recovery after a transient outage
// ---------------------------------------------------------------------------

#[tokio::test]
async fn pipeline_recovers_after_loki_becomes_available() {
    // 1. Server fails twice, then succeeds.
    let (push_url, _stop_server, _server) = spawn_recovering_server(2).await;

    // 2. Capacity large enough that no event is dropped due to saturation.
    let (queue, telemetry) = queue_with_capacity(10);

    // 3. Exporter with a few retries.
    let config = recovering_exporter_config(push_url);
    let exporter = LokiExporter::new(config, sample_service(), queue.clone(), 1)
        .expect("exporter config")
        .start();

    // 4. Push a single event. It will be retried until the server succeeds.
    queue
        .try_push(sample_event("req-recovery"))
        .expect("queue should accept one event");

    // 5. Wait for the worker to exhaust the two 503s and then succeed on the third attempt.
    tokio::time::sleep(Duration::from_millis(500)).await;

    let snapshot = telemetry.snapshot();
    assert!(
        snapshot.exported_events >= 1,
        "the event must be exported after recovery, got {}",
        snapshot.exported_events
    );

    // 6. Shutdown cleanly.
    let report = timeout(Duration::from_secs(2), exporter.shutdown())
        .await
        .expect("shutdown must not hang");
    assert_eq!(
        report.abandoned_events, 0,
        "no event should be abandoned after a successful export"
    );
}
