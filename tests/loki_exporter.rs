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
        exporter::{
            LokiExporter, LokiExporterError, LokiExporterHandle, ShutdownOutcome, MAX_WORKER_COUNT,
        },
    },
    runtime::queue::{BoundedEventQueue, QueueConfig},
};
use serde_json::Value;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::mpsc,
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
        "password": "[REDACTED]",
        "value": "exporter-test"
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
    config.batch.max_events = 2;
    config.batch.max_bytes = 1024 * 1024;
    config.batch.flush_interval = Duration::from_millis(25);
    config.request_timeout = Duration::from_millis(500);
    config.retry.max_retries = 2;
    config.retry.initial_backoff = Duration::from_millis(1);
    config.retry.max_backoff = Duration::from_millis(2);
    config
}

async fn spawn_server(statuses: Vec<u16>) -> (String, mpsc::Receiver<Vec<u8>>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (request_sender, request_receiver) = mpsc::channel(statuses.len());

    let server = tokio::spawn(async move {
        for status in statuses {
            let (mut socket, _) = listener.accept().await.unwrap();
            let body = read_http_request(&mut socket).await;
            request_sender.send(body).await.unwrap();

            let response =
                format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });

    (
        format!("http://{address}/loki/api/v1/push"),
        request_receiver,
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

async fn join_workers(handle: LokiExporterHandle) {
    let report = timeout(Duration::from_secs(2), handle.shutdown())
        .await
        .unwrap();

    assert_eq!(report.outcome, ShutdownOutcome::Drained);
    assert_eq!(report.abandoned_events, 0);
    assert_eq!(report.worker_failures, 0);
}

fn request_ids(bodies: &[Vec<u8>]) -> Vec<String> {
    let mut request_ids = Vec::new();

    for body in bodies {
        let payload: Value = serde_json::from_slice(body).unwrap();
        for stream in payload["streams"].as_array().unwrap() {
            for value in stream["values"].as_array().unwrap() {
                let event: Value = serde_json::from_str(value[1].as_str().unwrap()).unwrap();
                request_ids.push(event["request_id"].as_str().unwrap().to_owned());
            }
        }
    }

    request_ids.sort();
    request_ids
}

#[tokio::test]
async fn full_batch_is_exported_and_counted() {
    let (url, mut requests, server) = spawn_server(vec![204]).await;
    let (queue, telemetry) = queue_with_capacity(4);
    let handle = LokiExporter::new(exporter_config(url), sample_service(), queue.clone(), 1)
        .unwrap()
        .start();

    queue.try_push(sample_event("request-export-0001")).unwrap();
    queue.try_push(sample_event("request-export-0002")).unwrap();
    queue.close();
    join_workers(handle).await;

    let body = requests.recv().await.unwrap();
    server.await.unwrap();
    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["streams"].as_array().unwrap().len(), 2);

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.exported_batches, 1);
    assert_eq!(snapshot.exported_events, 2);
    assert_eq!(snapshot.failed_batches, 0);
}

#[tokio::test]
async fn partial_batch_flushes_at_flush_interval() {
    let (url, mut requests, server) = spawn_server(vec![204]).await;
    let mut config = exporter_config(url);
    config.batch.max_events = 10;
    config.batch.flush_interval = Duration::from_millis(20);
    let (queue, telemetry) = queue_with_capacity(4);
    let handle = LokiExporter::new(config, sample_service(), queue.clone(), 1)
        .unwrap()
        .start();

    queue.try_push(sample_event("request-export-0001")).unwrap();

    let body = timeout(Duration::from_secs(1), requests.recv())
        .await
        .unwrap()
        .unwrap();
    queue.close();
    join_workers(handle).await;
    server.await.unwrap();

    let payload: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(payload["streams"].as_array().unwrap().len(), 1);
    assert_eq!(telemetry.snapshot().exported_events, 1);
}

#[tokio::test]
async fn retryable_failure_is_retried_until_success() {
    let (url, mut requests, server) = spawn_server(vec![500, 204]).await;
    let mut config = exporter_config(url);
    config.retry.max_retries = 1;
    let (queue, telemetry) = queue_with_capacity(4);
    let handle = LokiExporter::new(config, sample_service(), queue.clone(), 1)
        .unwrap()
        .start();

    queue.try_push(sample_event("request-export-0001")).unwrap();
    queue.close();
    join_workers(handle).await;

    assert!(requests.recv().await.is_some());
    assert!(requests.recv().await.is_some());
    server.await.unwrap();

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.export_retries, 1);
    assert_eq!(snapshot.retryable_export_failures, 1);
    assert_eq!(snapshot.exported_batches, 1);
    assert_eq!(snapshot.exported_events, 1);
    assert_eq!(snapshot.failed_batches, 0);
}

#[tokio::test]
async fn permanent_failure_is_not_retried() {
    let (url, mut requests, server) = spawn_server(vec![400]).await;
    let (queue, telemetry) = queue_with_capacity(4);
    let handle = LokiExporter::new(exporter_config(url), sample_service(), queue.clone(), 1)
        .unwrap()
        .start();

    queue.try_push(sample_event("request-export-0001")).unwrap();
    queue.close();
    join_workers(handle).await;

    assert!(requests.recv().await.is_some());
    server.await.unwrap();

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.export_retries, 0);
    assert_eq!(snapshot.permanent_export_failures, 1);
    assert_eq!(snapshot.retryable_export_failures, 0);
    assert_eq!(snapshot.failed_batches, 1);
    assert_eq!(snapshot.dropped_events, 1);
}

#[tokio::test]
async fn retry_limit_is_enforced() {
    let (url, mut requests, server) = spawn_server(vec![500, 500, 500]).await;
    let (queue, telemetry) = queue_with_capacity(4);
    let handle = LokiExporter::new(exporter_config(url), sample_service(), queue.clone(), 1)
        .unwrap()
        .start();

    queue.try_push(sample_event("request-export-0001")).unwrap();
    queue.close();
    join_workers(handle).await;

    for _ in 0..3 {
        assert!(requests.recv().await.is_some());
    }
    server.await.unwrap();

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.export_retries, 2);
    assert_eq!(snapshot.retryable_export_failures, 3);
    assert_eq!(snapshot.exported_batches, 0);
    assert_eq!(snapshot.failed_batches, 1);
    assert_eq!(snapshot.dropped_events, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fixed_workers_export_each_event_exactly_once() {
    let (url, mut requests, server) = spawn_server(vec![204; 6]).await;
    let mut config = exporter_config(url);
    config.batch.max_events = 1;
    let (queue, telemetry) = queue_with_capacity(8);
    let handle = LokiExporter::new(config, sample_service(), queue.clone(), 2)
        .unwrap()
        .start();
    assert_eq!(handle.worker_count(), 2);

    for index in 0..6 {
        queue
            .try_push(sample_event(&format!("request-export-{index:04}")))
            .unwrap();
    }
    queue.close();
    join_workers(handle).await;

    let mut bodies = Vec::new();
    for _ in 0..6 {
        bodies.push(requests.recv().await.unwrap());
    }
    server.await.unwrap();

    let expected = (0..6)
        .map(|index| format!("request-export-{index:04}"))
        .collect::<Vec<_>>();
    assert_eq!(request_ids(&bodies), expected);
    assert_eq!(telemetry.snapshot().exported_events, 6);
}

#[tokio::test]
async fn oversized_event_is_dropped_without_a_network_request() {
    let mut config = exporter_config("http://127.0.0.1:9/loki/api/v1/push".to_owned());
    config.batch.max_bytes = 32;
    let (queue, telemetry) = queue_with_capacity(4);
    let handle = LokiExporter::new(config, sample_service(), queue.clone(), 1)
        .unwrap()
        .start();

    queue
        .try_push(sample_event("request-export-large"))
        .unwrap();
    queue.close();
    join_workers(handle).await;

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.oversized_events, 1);
    assert_eq!(snapshot.dropped_events, 1);
    assert_eq!(snapshot.exported_batches, 0);
    assert_eq!(snapshot.failed_batches, 0);
}

#[test]
fn invalid_worker_counts_are_rejected() {
    for worker_count in [0, MAX_WORKER_COUNT + 1] {
        let (queue, _) = queue_with_capacity(1);
        let result = LokiExporter::new(
            exporter_config("http://127.0.0.1:9/loki/api/v1/push".to_owned()),
            sample_service(),
            queue,
            worker_count,
        );

        assert!(matches!(result, Err(LokiExporterError::InvalidWorkerCount)));
    }
}
