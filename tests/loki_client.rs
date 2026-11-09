#![cfg(feature = "loki")]

use std::{collections::BTreeMap, time::Duration};

use safe_http_telemetry::loki::{
    client::{LokiClient, LokiClientError},
    config::LokiConfig,
    payload::{LokiPushRequest, LokiStream},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
    task::JoinHandle,
};

#[derive(Debug)]
struct RecordedRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

fn sample_payload() -> LokiPushRequest {
    let stream = LokiStream {
        stream: BTreeMap::from([
            ("service".to_owned(), "users-api".to_owned()),
            ("environment".to_owned(), "test".to_owned()),
            ("event_type".to_owned(), "http".to_owned()),
        ]),
        values: vec![[
            "1730000000123456789".to_owned(),
            "{\"request_id\":\"request-client-sensitive\",\"message\":\"test\"}".to_owned(),
        ]],
        metadata: BTreeMap::new(),
    };

    LokiPushRequest::single(stream)
}

fn client_for(url: String) -> LokiClient {
    let mut config = LokiConfig::new(url).unwrap();
    config.request_timeout = Duration::from_millis(500);
    LokiClient::new(config).unwrap()
}

async fn spawn_server(status: u16) -> (String, oneshot::Receiver<RecordedRequest>, JoinHandle<()>) {
    spawn_controlled_server(status, None).await
}

async fn spawn_controlled_server(
    status: u16,
    release: Option<oneshot::Receiver<()>>,
) -> (String, oneshot::Receiver<RecordedRequest>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (request_sender, request_receiver) = oneshot::channel();

    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let request = read_http_request(&mut socket).await;
        let _ = request_sender.send(request);

        if let Some(release) = release {
            let _ = release.await;
        }

        let response =
            format!("HTTP/1.1 {status} Test\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        let _ = socket.write_all(response.as_bytes()).await;
    });

    (
        format!("http://{address}/loki/api/v1/push"),
        request_receiver,
        server,
    )
}

async fn read_http_request(socket: &mut TcpStream) -> RecordedRequest {
    let mut buffer = Vec::new();

    let header_end = loop {
        if let Some(position) = find_header_end(&buffer) {
            break position + 4;
        }

        let mut chunk = [0_u8; 1_024];
        let read = socket.read(&mut chunk).await.unwrap();
        assert!(
            read > 0,
            "connection closed before HTTP headers were complete"
        );
        buffer.extend_from_slice(&chunk[..read]);
    };

    let head = std::str::from_utf8(&buffer[..header_end - 4]).unwrap();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap();
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().unwrap().to_owned();
    let path = request_parts.next().unwrap().to_owned();

    let headers = lines
        .map(|line| {
            let (name, value) = line.split_once(':').unwrap();
            (name.to_ascii_lowercase(), value.trim().to_owned())
        })
        .collect::<BTreeMap<_, _>>();

    let content_length = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);

    while buffer.len() < header_end + content_length {
        let mut chunk = [0_u8; 1_024];
        let read = socket.read(&mut chunk).await.unwrap();
        assert!(read > 0, "connection closed before HTTP body was complete");
        buffer.extend_from_slice(&chunk[..read]);
    }

    RecordedRequest {
        method,
        path,
        headers,
        body: buffer[header_end..header_end + content_length].to_vec(),
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

#[tokio::test]
async fn client_posts_json_with_tenant_and_bearer_authentication() {
    let (url, request_receiver, server) = spawn_server(204).await;
    let mut config = LokiConfig::new(url).unwrap();
    config.tenant_id = Some("test-tenant".to_owned());
    config = config.with_bearer_token("fake-client-test-token");

    let client = LokiClient::new(config).unwrap();
    client.push(&sample_payload()).await.unwrap();

    let request = request_receiver.await.unwrap();
    server.await.unwrap();

    assert_eq!(request.method, "POST");
    assert_eq!(request.path, "/loki/api/v1/push");
    assert_eq!(request.headers["content-type"], "application/json");
    assert_eq!(request.headers["x-scope-orgid"], "test-tenant");
    assert_eq!(
        request.headers["authorization"],
        "Bearer fake-client-test-token"
    );

    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["streams"][0]["stream"]["service"], "users-api");
    assert_eq!(body["streams"][0]["values"][0][0], "1730000000123456789");
}

#[tokio::test]
async fn client_classifies_retryable_and_permanent_statuses() {
    for status in [400, 401, 429, 500, 503] {
        let (url, request_receiver, server) = spawn_server(status).await;
        let client = client_for(url);

        let error = client.push(&sample_payload()).await.unwrap_err();
        let request = request_receiver.await.unwrap();
        server.await.unwrap();

        assert_eq!(request.method, "POST");
        assert_eq!(error.status_code(), Some(status));

        let retryable = matches!(status, 429 | 500 | 503);
        assert_eq!(error.is_retryable(), retryable);

        if retryable {
            assert!(matches!(error, LokiClientError::RetryableStatus(value) if value == status));
        } else {
            assert!(matches!(error, LokiClientError::PermanentStatus(value) if value == status));
        }
    }
}

#[tokio::test]
async fn request_timeout_is_retryable() {
    let (release_sender, release_receiver) = oneshot::channel();
    let (url, request_receiver, server) =
        spawn_controlled_server(204, Some(release_receiver)).await;
    let mut config = LokiConfig::new(url).unwrap();
    config.request_timeout = Duration::from_millis(50);
    let client = LokiClient::new(config).unwrap();

    let error = client.push(&sample_payload()).await.unwrap_err();

    assert!(matches!(error, LokiClientError::Timeout));
    assert!(error.is_retryable());

    let _request = request_receiver.await.unwrap();
    let _ = release_sender.send(());
    server.await.unwrap();
}

#[tokio::test]
async fn connection_failure_is_retryable() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);

    let client = client_for(format!("http://{address}/loki/api/v1/push"));
    let error = client.push(&sample_payload()).await.unwrap_err();

    assert!(matches!(error, LokiClientError::Transport));
    assert!(error.is_retryable());
    assert_eq!(error.status_code(), None);
}

#[tokio::test]
async fn error_diagnostics_do_not_expose_credentials_or_payload_data() {
    let (url, request_receiver, server) = spawn_server(400).await;
    let config = LokiConfig::new(url)
        .unwrap()
        .with_bearer_token("fake-token-must-not-leak");
    let client = LokiClient::new(config).unwrap();

    let error = client.push(&sample_payload()).await.unwrap_err();
    let _request = request_receiver.await.unwrap();
    server.await.unwrap();

    let diagnostics = format!("{error:?} {error}");
    assert!(!diagnostics.contains("fake-token-must-not-leak"));
    assert!(!diagnostics.contains("request-client-sensitive"));
    assert_eq!(
        diagnostics,
        "PermanentStatus(400) Loki returned permanent HTTP status 400"
    );
}
