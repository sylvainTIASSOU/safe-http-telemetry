use std::{
    collections::BTreeMap,
    env,
    error::Error,
    io,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use axum::{http::StatusCode, routing::post, Json, Router};
use safe_http_telemetry::{
    axum_adapter::axum_telemetry_layer,
    config::ServiceConfig,
    core::telemetry::InternalTelemetry,
    loki::{
        config::LokiConfig,
        exporter::{LokiExporter, ShutdownOutcome},
    },
    policy::{CaptureProfile, RoutePolicySet},
    runtime::queue::{BoundedEventQueue, QueueConfig},
    tower_integration::config::MiddlewareConfig,
};
use serde_json::Value;
use tokio::time::{sleep, timeout, Instant};

const SERVICE_NAME: &str = "safe-http-telemetry-e2e";
const FICTITIOUS_SECRET: &str = "fictitious-e2e-secret-do-not-use";

type TestResult<T> = Result<T, Box<dyn Error>>;
type LokiLabels = BTreeMap<String, String>;
type LokiLine = (LokiLabels, String);

#[tokio::test]
#[ignore = "requires the real Loki service from examples/loki/docker-compose.yaml"]
async fn exports_one_minimal_http_event_to_real_loki() {
    timeout(Duration::from_secs(40), run_scenario())
        .await
        .expect("the real-Loki scenario exceeded its global deadline")
        .expect("the real-Loki scenario failed");
}

async fn run_scenario() -> TestResult<()> {
    let loki_url = env::var("SAFE_HTTP_TELEMETRY_LOKI_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3100".to_owned());

    let telemetry = InternalTelemetry::new();
    let queue = BoundedEventQueue::new(QueueConfig::new(64), telemetry.clone())?;

    let service = ServiceConfig {
        name: SERVICE_NAME.to_owned(),
        environment: "test".to_owned(),
        version: "0.1.0".to_owned(),
        region: None,
    };

    let mut loki_config = LokiConfig::new(format!(
        "{}/loki/api/v1/push",
        loki_url.trim_end_matches('/')
    ))?;
    loki_config.batch.max_events = 10;
    loki_config.batch.flush_interval = Duration::from_millis(100);
    loki_config.request_timeout = Duration::from_secs(2);
    loki_config.retry.max_retries = 1;
    loki_config.shutdown.drain = true;
    loki_config.shutdown.deadline = Duration::from_secs(5);

    let exporter = LokiExporter::new(loki_config, service, queue.clone(), 1)?.start();

    let middleware_config = MiddlewareConfig {
        trust_incoming_request_id: true,
        ..Default::default()
    };

    let telemetry_layer = axum_telemetry_layer(
        queue,
        RoutePolicySet::new(CaptureProfile::Minimal),
        middleware_config,
    )?;

    let app = Router::new()
        .route("/e2e/items/{item_id}", post(create_item))
        .layer(telemetry_layer);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;

    let (shutdown_sender, shutdown_receiver) = tokio::sync::oneshot::channel::<()>();

    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_receiver.await;
            })
            .await
    });

    let request_id = format!("e2e-{}-{}", std::process::id(), unix_timestamp_ns()?);
    let start_ns = unix_timestamp_ns()?.saturating_sub(5_000_000_000);

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?;

    let request_body = serde_json::to_vec(&serde_json::json!({
        "kind": "fictitious-item",
        "token": FICTITIOUS_SECRET
    }))?;

    let response = client
        .post(format!("http://{address}/e2e/items/item-fictitious-001"))
        .query(&[("api_key", FICTITIOUS_SECRET)])
        .header("content-type", "application/json")
        .header("x-request-id", &request_id)
        .body(request_body)
        .send()
        .await
        .map_err(|_| static_error("the HTTP request to Axum failed"))?;

    assert_eq!(response.status(), StatusCode::CREATED);

    // Consuming the response completes the observed HTTP body.
    let response_body = response
        .text()
        .await
        .map_err(|_| static_error("reading the Axum response failed"))?;
    assert!(response_body.is_empty());

    let end_ns = unix_timestamp_ns()?.saturating_add(5_000_000_000);

    // Graceful HTTP shutdown must happen before exporter shutdown.
    let _ = shutdown_sender.send(());
    let server_result = timeout(Duration::from_secs(5), server)
        .await
        .map_err(|_| static_error("the HTTP server shutdown timed out"))??;
    server_result.map_err(|_| static_error("the HTTP server failed"))?;

    let report = timeout(Duration::from_secs(10), exporter.shutdown())
        .await
        .map_err(|_| static_error("the exporter shutdown timed out"))?;

    assert_eq!(report.outcome, ShutdownOutcome::Drained);
    assert_eq!(report.abandoned_events, 0);
    assert_eq!(report.worker_failures, 0);

    let snapshot = telemetry.snapshot();
    assert_eq!(snapshot.observed_requests, 1);
    assert_eq!(snapshot.completed_requests, 1);
    assert_eq!(snapshot.active_requests, 0);
    assert_eq!(snapshot.accepted_events, 1);
    assert_eq!(snapshot.exported_events, 1);
    assert_eq!(snapshot.dropped_events, 0);

    let (attributes, event) =
        query_event(&client, &loki_url, &request_id, start_ns, end_ns).await?;

    let expected_labels = BTreeMap::from([
        ("environment".to_owned(), "test".to_owned()),
        ("event_type".to_owned(), "http".to_owned()),
        ("service".to_owned(), SERVICE_NAME.to_owned()),
        ("service_name".to_owned(), SERVICE_NAME.to_owned()),
        ("version".to_owned(), "0.1.0".to_owned()),
    ]);

    // Log query responses combine indexed labels and structured metadata.
    // The Series API exposes only the indexed label sets defining each stream.
    let indexed_label_sets = query_indexed_label_sets(&client, &loki_url, start_ns, end_ns).await?;

    assert!(
        !indexed_label_sets.is_empty(),
        "Loki must return the indexed stream for the exported event"
    );

    for labels in indexed_label_sets {
        assert_eq!(labels, expected_labels);
    }

    // The controlled labels must also be present in the log query response.
    for (name, expected_value) in &expected_labels {
        assert_eq!(attributes.get(name), Some(expected_value));
    }

    assert_eq!(event["event"].as_str(), Some("http.request.completed"));
    assert_eq!(event["request_id"].as_str(), Some(request_id.as_str()));
    assert_eq!(event["request"]["method"].as_str(), Some("POST"));
    assert_eq!(
        event["request"]["route"].as_str(),
        Some("/e2e/items/{item_id}")
    );
    assert_eq!(event["request"]["path"].as_str(), Some("[OMITTED]"));
    assert_eq!(event["response"]["status_code"].as_u64(), Some(201));
    assert_eq!(event["response"]["outcome"].as_str(), Some("success"));

    let duration_ms = event["timing"]["duration_ms"]
        .as_f64()
        .expect("duration_ms must be numeric");
    assert!(duration_ms.is_finite());
    assert!(duration_ms >= 0.0);

    assert!(event["request"]["body"].is_null());
    assert!(event["response"]["body"].is_null());
    assert!(event["request"]["query"]
        .as_object()
        .is_some_and(|query| query.is_empty()));
    assert!(event["request"]["headers"]
        .as_object()
        .is_some_and(|headers| headers.is_empty()));

    // OpenTelemetry/W3C correlation belongs to Phase 4.
    assert!(event.get("trace_id").is_none());
    assert!(event.get("span_id").is_none());

    let serialized_line = serde_json::to_string(&event)?;
    assert!(!serialized_line.contains(FICTITIOUS_SECRET));
    assert!(!serialized_line.contains("item-fictitious-001"));
    assert!(!serialized_line.contains("api_key"));
    assert!(!serialized_line.contains("item-fictitious-001?"));

    Ok(())
}

async fn create_item(Json(payload): Json<Value>) -> StatusCode {
    if payload.get("kind").and_then(Value::as_str) != Some("fictitious-item") {
        return StatusCode::BAD_REQUEST;
    }

    StatusCode::CREATED
}

async fn query_event(
    client: &reqwest::Client,
    loki_url: &str,
    request_id: &str,
    start_ns: u128,
    end_ns: u128,
) -> TestResult<(LokiLabels, Value)> {
    let query_url = format!("{}/loki/api/v1/query_range", loki_url.trim_end_matches('/'));
    let query = format!("{{service=\"{SERVICE_NAME}\"}} | json | request_id=\"{request_id}\"");
    let start = start_ns.to_string();
    let end = end_ns.to_string();
    let deadline = Instant::now() + Duration::from_secs(15);

    loop {
        let response = client
            .get(&query_url)
            .query(&[
                ("query", query.as_str()),
                ("start", start.as_str()),
                ("end", end.as_str()),
                ("limit", "10"),
                ("direction", "forward"),
            ])
            .send()
            .await
            .map_err(|_| static_error("the Loki query failed"))?;

        if !response.status().is_success() {
            return Err(static_error("Loki returned an unsuccessful query status").into());
        }

        let body = response
            .text()
            .await
            .map_err(|_| static_error("reading the Loki query response failed"))?;
        let payload: Value = serde_json::from_str(&body)?;

        if payload["status"].as_str() != Some("success") {
            return Err(static_error("Loki returned an unsuccessful query payload").into());
        }

        let mut lines = parse_loki_lines(&payload)?;

        if lines.len() > 1 {
            return Err(static_error("more than one matching Loki event was found").into());
        }

        if lines.len() == 1 {
            let (labels, line) = lines.remove(0);
            let event = serde_json::from_str(&line)?;
            return Ok((labels, event));
        }

        if Instant::now() >= deadline {
            return Err(static_error("the expected event did not appear in Loki").into());
        }

        sleep(Duration::from_millis(250)).await;
    }
}

async fn query_indexed_label_sets(
    client: &reqwest::Client,
    loki_url: &str,
    start_ns: u128,
    end_ns: u128,
) -> TestResult<Vec<LokiLabels>> {
    let series_url = format!("{}/loki/api/v1/series", loki_url.trim_end_matches('/'));
    let selector = format!("{{service=\"{SERVICE_NAME}\"}}");
    let start = start_ns.to_string();
    let end = end_ns.to_string();

    let response = client
        .get(&series_url)
        .query(&[
            ("match[]", selector.as_str()),
            ("start", start.as_str()),
            ("end", end.as_str()),
        ])
        .send()
        .await
        .map_err(|_| static_error("the Loki series query failed"))?;

    if !response.status().is_success() {
        return Err(static_error("Loki returned an unsuccessful series query status").into());
    }

    let body = response
        .text()
        .await
        .map_err(|_| static_error("reading the Loki series response failed"))?;
    let payload: Value = serde_json::from_str(&body)?;

    if payload["status"].as_str() != Some("success") {
        return Err(static_error("Loki returned an unsuccessful series query payload").into());
    }

    let series = payload["data"]
        .as_array()
        .ok_or_else(|| static_error("Loki returned an invalid series array"))?;

    let mut label_sets = Vec::with_capacity(series.len());

    for stream in series {
        let labels = stream
            .as_object()
            .ok_or_else(|| static_error("Loki returned invalid series labels"))?
            .iter()
            .map(|(key, value)| {
                value
                    .as_str()
                    .map(|value| (key.clone(), value.to_owned()))
                    .ok_or_else(|| static_error("Loki returned a non-string series label"))
            })
            .collect::<Result<LokiLabels, _>>()?;

        label_sets.push(labels);
    }

    Ok(label_sets)
}

fn parse_loki_lines(payload: &Value) -> Result<Vec<LokiLine>, io::Error> {
    let results = payload["data"]["result"]
        .as_array()
        .ok_or_else(|| static_error("Loki returned an invalid result array"))?;

    let mut lines = Vec::new();

    for result in results {
        let labels = result["stream"]
            .as_object()
            .ok_or_else(|| static_error("Loki returned invalid stream labels"))?
            .iter()
            .map(|(key, value)| {
                value
                    .as_str()
                    .map(|value| (key.clone(), value.to_owned()))
                    .ok_or_else(|| static_error("Loki returned a non-string label"))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;

        let values = result["values"]
            .as_array()
            .ok_or_else(|| static_error("Loki returned an invalid values array"))?;

        for entry in values {
            let pair = entry
                .as_array()
                .ok_or_else(|| static_error("Loki returned an invalid log entry"))?;

            if pair.len() != 2 {
                return Err(static_error("Loki returned an invalid log entry pair"));
            }

            let line = pair[1]
                .as_str()
                .ok_or_else(|| static_error("Loki returned a non-string log line"))?;

            lines.push((labels.clone(), line.to_owned()));
        }
    }

    Ok(lines)
}

fn unix_timestamp_ns() -> Result<u128, io::Error> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .map_err(|_| static_error("the system clock is before the Unix epoch"))
}

fn static_error(message: &'static str) -> io::Error {
    io::Error::other(message)
}

#[tokio::test]
#[ignore = "requires the real Loki service from examples/loki/docker-compose.yaml"]
async fn detailed_http_event_redacts_secrets_and_exports_correctly() {
    use reqwest::Client;
    use serde_json::json;
    use std::time::Duration;
    use tokio::time::sleep;

    // 1. Setup Axum app with Detailed profile
    let (_queue, exporter, app) = setup_detailed_test_app().await;

    // Start server on a random port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // 2. Send request with fictitious secrets
    let client = Client::new();
    let secret_password = "super-secret-password-123";
    let secret_token = "bearer-secret-token-456";

    let payload = json!({
        "name": "alice",
        "password": secret_password,
        "nested": { "token": secret_token }
    });

    let response = client
        .post(format!("http://{}/e2e/detailed", addr))
        .header("authorization", format!("Bearer {}", secret_token))
        .json(&payload)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);

    // The handler response also contains a fictitious secret
    let response_body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(response_body["status"], "ok");

    // 3. Wait for the batch to be exported to Loki (flush_interval is 250ms)
    sleep(Duration::from_millis(1000)).await;

    // 4. Query Loki to retrieve the event
    let loki_url = std::env::var("LOKI_PUSH_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3100/loki/api/v1/push".to_owned())
        .replace("/push", "/query_range");

    let query = r#"{service="e2e-detailed-test"}"#.to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let loki_response = client
        .get(&loki_url)
        .query(&[
            ("query", query.as_str()),
            ("start", &format!("{}", now - 60_000_000_000)), // il y a 1 minute
            ("end", &format!("{}", now)),
            ("limit", "10"),
        ])
        .send()
        .await
        .unwrap();

    let loki_json: serde_json::Value = loki_response.json().await.unwrap();

    // 5. Assertions
    let streams = loki_json["data"]["result"]
        .as_array()
        .expect("No streams returned");
    assert!(!streams.is_empty(), "Loki did not receive any event");

    let first_stream = &streams[0];

    // A. Ensure labels do NOT contain secrets (low cardinality)
    let labels = first_stream["stream"].as_object().unwrap();
    assert!(labels.contains_key("service"));
    assert_eq!(labels["service"], "e2e-detailed-test");
    assert_eq!(labels["event_type"], "http");

    // B. Check log line content
    let values = first_stream["values"].as_array().unwrap();
    // values[0][1] corresponds to the JSON log line
    let log_line = values[0][1].as_str().expect("Log line is not a string");

    // The secret must not appear anywhere.
    assert!(
        !log_line.contains(secret_password),
        "Password leaked into Loki!"
    );
    assert!(!log_line.contains(secret_token), "Token leaked into Loki!");

    // The response secret must not leak either.
    let secret_in_response = "secret-in-response-789";
    assert!(
        !log_line.contains(secret_in_response),
        "Response secret leaked into Loki!"
    );

    // The expected values must be present.
    assert!(log_line.contains("[REDACTED]"), "Redaction marker missing");
    assert!(log_line.contains("alice"), "Safe data missing");

    // C. Check structured metadata.
    let metadata = first_stream
        .get("metadata")
        .unwrap_or(&serde_json::Value::Null);
    let metadata_str = metadata.to_string();
    assert!(!metadata_str.contains(secret_password));
    assert!(!metadata_str.contains(secret_token));

    // Shut down cleanly.
    exporter.shutdown().await;
}

// --- Helpers for the detailed test ---

async fn mock_detailed_handler(
    axum::Json(_payload): axum::Json<serde_json::Value>,
) -> axum::Json<serde_json::Value> {
    // This handler returns a secret that MUST be redacted from the response log.
    axum::Json(serde_json::json!({
        "status": "ok",
        "access_token": "secret-in-response-789"
    }))
}

async fn setup_detailed_test_app() -> (
    safe_http_telemetry::runtime::queue::BoundedEventQueue,
    safe_http_telemetry::loki::exporter::LokiExporterHandle,
    axum::Router,
) {
    use axum::routing::post;
    use safe_http_telemetry::{
        axum_adapter::axum_telemetry_layer,
        config::{CaptureLimits, RedactionConfig, ServiceConfig},
        core::telemetry::InternalTelemetry,
        loki::{config::LokiConfig, exporter::LokiExporter},
        policy::{CaptureProfile, RoutePattern, RoutePolicy, RoutePolicySet},
        redaction::Redactor,
        runtime::queue::{BoundedEventQueue, QueueConfig},
        tower_integration::config::MiddlewareConfig,
    };
    use std::time::Duration;

    // 1. Event queue.
    let queue = BoundedEventQueue::new(QueueConfig::new(1_024), InternalTelemetry::new())
        .expect("Failed to create queue");

    // 2. Redactor (required to construct the middleware, although this test focuses on export).
    let _redactor = Redactor::new(
        &RedactionConfig::default(),
        &CaptureLimits::default(),
        queue.telemetry(),
    )
    .expect("Failed to create redactor");

    // 3. Service configuration.
    let service_config = ServiceConfig {
        name: "e2e-detailed-test".to_owned(),
        environment: "test".to_owned(),
        version: "0.1.0".to_owned(),
        region: None,
    };

    // 4. Loki configuration (use a short flush interval for the test).
    let push_url = std::env::var("LOKI_PUSH_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:3100/loki/api/v1/push".to_owned());

    let mut loki_config = LokiConfig::new(push_url).expect("Invalid Loki config");
    loki_config.batch.max_events = 10;
    loki_config.batch.flush_interval = Duration::from_millis(100); // Flush quickly.
    loki_config.request_timeout = Duration::from_secs(2);
    loki_config.retry.max_retries = 1;
    loki_config.shutdown.drain = true;
    loki_config.shutdown.deadline = Duration::from_secs(2);

    // 5. Start the exporter.
    let exporter = LokiExporter::new(loki_config, service_config, queue.clone(), 1)
        .expect("Failed to start exporter")
        .start();

    // 6. Capture policies (use Detailed for the test route).
    let mut policies = RoutePolicySet::new(CaptureProfile::Minimal);
    policies
        .add_policy(RoutePolicy::new(
            RoutePattern::exact("/e2e/detailed"),
            CaptureProfile::Detailed,
        ))
        .expect("Failed to add policy");

    // 7. Middleware configuration (geolocation is disabled for this test).
    let middleware_config = MiddlewareConfig::default();

    // 8. Telemetry layer.
    let telemetry_layer = axum_telemetry_layer(queue.clone(), policies, middleware_config)
        .expect("Failed to create telemetry layer");

    // 9. Application Axum
    let app = axum::Router::new()
        .route("/e2e/detailed", post(mock_detailed_handler))
        .layer(telemetry_layer);

    (queue, exporter, app)
}
