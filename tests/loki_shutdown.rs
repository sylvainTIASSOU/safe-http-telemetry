#![cfg(feature = "loki")]

use std::time::Duration;

use safe_http_telemetry::{
    config::ServiceConfig,
    core::{
        event::{CaptureInfo, HttpEvent, RequestInfo, ResponseInfo, TimingInfo},
        telemetry::InternalTelemetry,
    },
    loki::{
        config::{LokiConfig, LokiConfigError},
        exporter::{LokiExporter, ShutdownOutcome},
    },
    runtime::queue::{BoundedEventQueue, QueueConfig, TryPushError},
};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot},
    task::JoinHandle,
    time::timeout,
};

fn sample_service() -> ServiceConfig {
    ServiceConfig {
        name: "users-api".to_owned(),
        environment: "test".to_owned(),
        version: "0.1.0".to_owned(),
        region: None,
    }
}

fn sample_event(request_id: &str) -> HttpEvent {
    let mut request = RequestInfo::new("POST", "/api/users/{id}", "/api/users/42");
    request.body = Some(serde_json::json!({
        "password": "[REDACTED]"
    }));

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

fn exporter_config(url: String) -> LokiConfig {
    let mut config = LokiConfig::new(url).unwrap();
    config.batch.max_events = 1;
    config.batch.max_bytes = 1024 * 1024;
    config.batch.flush_interval = Duration::from_millis(25);
    config.request_timeout = Duration::from_secs(5);
    config.retry.max_retries = 0;
    config.shutdown.deadline = Duration::from_millis(50);
    config
}

async fn spawn_success_server(
    request_count: usize,
) -> (String, mpsc::Receiver<Vec<u8>>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (request_sender, request_receiver) = mpsc::channel(request_count);

    let server = tokio::spawn(async move {
        for _ in 0..request_count {
            let (mut socket, _) = listener.accept().await.unwrap();
            let body = read_http_request(&mut socket).await;
            request_sender.send(body).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        }
    });

    (
        format!("http://{address}/loki/api/v1/push"),
        request_receiver,
        server,
    )
}

async fn spawn_blocking_server() -> (
    String,
    oneshot::Receiver<()>,
    oneshot::Sender<()>,
    JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (request_sender, request_receiver) = oneshot::channel();
    let (release_sender, release_receiver) = oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let _request = read_http_request(&mut socket).await;
        let _ = request_sender.send(());
        let _ = release_receiver.await;
        let _ = socket
            .write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    });

    (
        format!("http://{address}/loki/api/v1/push"),
        request_receiver,
        release_sender,
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
        assert!(read > 0);
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = std::str::from_utf8(&buffer[..header_end - 4]).unwrap();
    let content_length = head
        .split("\r\n")
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);

    while buffer.len() < header_end + content_length {
        let mut chunk = [0_u8; 1_024];
        let read = socket.read(&mut chunk).await.unwrap();
        assert!(read > 0);
        buffer.extend_from_slice(&chunk[..read]);
    }

    buffer[header_end..header_end + content_length].to_vec()
}

#[tokio::test]
async fn shutdown_closes_producers_and_drains_a_partial_batch() {
    let (url, mut requests, server) = spawn_success_server(1).await;
    let mut config = exporter_config(url);
    config.batch.max_events = 10;
    config.shutdown.deadline = Duration::from_secs(1);
    let (queue, telemetry) = queue_with_capacity(4);
    let handle = LokiExporter::new(config, sample_service(), queue.clone(), 1)
        .unwrap()
        .start();

    queue
        .try_push(sample_event("request-shutdown-0001"))
        .unwrap();
    queue
        .try_push(sample_event("request-shutdown-0002"))
        .unwrap();

    let report = handle.shutdown().await;

    assert_eq!(report.outcome, ShutdownOutcome::Drained);
    assert_eq!(report.abandoned_events, 0);
    assert_eq!(report.worker_failures, 0);
    assert_eq!(
        queue.try_push(sample_event("request-shutdown-closed")),
        Err(TryPushError::Closed)
    );

    let body = requests.recv().await.unwrap();
    server.await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["streams"].as_array().unwrap().len(), 2);

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.exported_events, 2);
    assert_eq!(snapshot.pending_export_events, 0);
    assert_eq!(snapshot.shutdowns, 1);
    assert_eq!(snapshot.drain_timeouts, 0);
    assert_eq!(snapshot.shutdown_abandoned_events, 0);
    assert_eq!(snapshot.dropped_events, 0);
}

#[tokio::test]
async fn shutdown_deadline_aborts_export_and_counts_all_remaining_events() {
    let (url, request_receiver, release_sender, server) = spawn_blocking_server().await;
    let (queue, telemetry) = queue_with_capacity(4);
    let handle = LokiExporter::new(exporter_config(url), sample_service(), queue.clone(), 1)
        .unwrap()
        .start();

    for index in 0..3 {
        queue
            .try_push(sample_event(&format!("request-shutdown-{index:04}")))
            .unwrap();
    }

    timeout(Duration::from_secs(1), request_receiver)
        .await
        .unwrap()
        .unwrap();

    let report = handle.shutdown().await;

    assert_eq!(report.outcome, ShutdownOutcome::TimedOut);
    assert_eq!(report.abandoned_events, 3);
    assert_eq!(report.worker_failures, 0);
    assert!(queue.is_closed());
    assert_eq!(queue.stats().occupancy, 0);

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.pending_export_events, 0);
    assert_eq!(snapshot.shutdowns, 1);
    assert_eq!(snapshot.drain_timeouts, 1);
    assert_eq!(snapshot.shutdown_abandoned_events, 3);
    assert_eq!(snapshot.dropped_events, 3);

    let _ = release_sender.send(());
    server.await.unwrap();
}

#[tokio::test]
async fn shutdown_without_drain_aborts_workers_immediately() {
    let (url, request_receiver, release_sender, server) = spawn_blocking_server().await;
    let mut config = exporter_config(url);
    config.shutdown.drain = false;
    config.shutdown.deadline = Duration::from_secs(1);
    let (queue, telemetry) = queue_with_capacity(4);
    let handle = LokiExporter::new(config, sample_service(), queue.clone(), 1)
        .unwrap()
        .start();

    queue
        .try_push(sample_event("request-shutdown-0001"))
        .unwrap();
    queue
        .try_push(sample_event("request-shutdown-0002"))
        .unwrap();

    timeout(Duration::from_secs(1), request_receiver)
        .await
        .unwrap()
        .unwrap();

    let report = timeout(Duration::from_secs(1), handle.shutdown())
        .await
        .unwrap();

    assert_eq!(report.outcome, ShutdownOutcome::Aborted);
    assert_eq!(report.abandoned_events, 2);
    assert_eq!(queue.stats().occupancy, 0);

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.pending_export_events, 0);
    assert_eq!(snapshot.drain_timeouts, 0);
    assert_eq!(snapshot.shutdown_abandoned_events, 2);
    assert_eq!(snapshot.dropped_events, 2);

    let _ = release_sender.send(());
    server.await.unwrap();
}

#[test]
fn zero_shutdown_deadline_is_rejected() {
    let mut config = exporter_config("http://127.0.0.1:9/loki/api/v1/push".to_owned());
    config.shutdown.deadline = Duration::ZERO;

    assert_eq!(
        config.validate(),
        Err(LokiConfigError::ZeroValue("shutdown.deadline"))
    );
}
